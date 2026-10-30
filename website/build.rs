use std::{env, fs, path::Path};

use topcoat::tailwind::BuildConfig;

fn main() {
    let config = BuildConfig::new().input("styles.css");
    // The Docker build points this at a musl Tailwind CLI; elsewhere Topcoat
    // downloads its pinned glibc one.
    let config = if std::env::var_os("TAILWIND_CLI").is_some() {
        config.executable_env("TAILWIND_CLI")
    } else {
        config
    };
    config.render().unwrap();
    facts();
}

/// The files of the product that the site takes its facts from. A change to
/// one rebuilds the site; one whose shape no longer parses stops the build
/// with a message, instead of the site going quietly out of date.
const SOURCES: [&str; 5] = [
    "cli/Cargo.toml",
    "cli/src/models/registry.rs",
    "docs/devices.md",
    "docs/usage.md",
    "docs/roadmap.md",
];

/// How many of the roadmap's next items the site lists.
const ROADMAP_ITEMS: usize = 6;

/// Writes `facts.rs` to `OUT_DIR`: the version, the repository, the default
/// model, the devices, the install command and the next roadmap items.
fn facts() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let read = |name: &str| {
        println!("cargo:rerun-if-changed=../{name}");
        fs::read_to_string(root.join(name)).unwrap_or_else(|e| panic!("cannot read {name}: {e}"))
    };
    let [manifest, registry, devices, usage, roadmap] = SOURCES.map(read);

    let version = quoted_value(&manifest, "version").expect("cli/Cargo.toml has no version");
    let repository =
        quoted_value(&manifest, "repository").expect("cli/Cargo.toml has no repository");
    let model = default_model(&registry);
    let install = usage
        .lines()
        .find(|line| line.starts_with("curl -fsSL"))
        .expect("docs/usage.md has no install command");

    let mut out = String::new();
    out += &format!("pub const VERSION: &str = {version:?};\n");
    out += &format!("pub const REPO: &str = {repository:?};\n");
    out += &format!("pub const INSTALL_CMD: &str = {install:?};\n");
    out += &format!("pub const DEFAULT_MODEL: &str = {:?};\n", model.0);
    out += &format!("pub const DEFAULT_MODEL_SIZE: &str = {:?};\n", model.1);
    out += &format!(
        "pub const DEVICES: &[[&str; 3]] = &{:?};\n",
        devices_table(&devices)
    );
    let (machine, bench) = bench_example(&devices);
    out += &format!("pub const BENCH_MACHINE: &str = {machine:?};\n");
    out += &format!("pub const BENCH: &[(&str, &str, bool)] = &{bench:?};\n");
    out += &format!("pub const NEXT: &[&str] = &{:?};\n", next_items(&roadmap));
    let dest = Path::new(&env::var("OUT_DIR").unwrap()).join("facts.rs");
    fs::write(dest, out).unwrap();
}

/// The first `key = "value"` line of a TOML file.
fn quoted_value(toml: &str, key: &str) -> Option<String> {
    let prefix = format!("{key} = \"");
    toml.lines()
        .find_map(|line| line.strip_prefix(&prefix)?.strip_suffix('"'))
        .map(str::to_owned)
}

/// The id of the default model and its download size, like `1.1 GB`.
fn default_model(registry: &str) -> (String, String) {
    let id = registry
        .lines()
        .find_map(|line| {
            line.strip_prefix("pub const DEFAULT: &str = \"")?
                .strip_suffix("\";")
        })
        .expect("the registry has no DEFAULT")
        .to_owned();
    let entry = registry
        .split_once(&format!("id: \"{id}\","))
        .expect("the registry lacks the default model")
        .1;
    let bytes: u64 = entry
        .lines()
        .find_map(|line| line.trim().strip_prefix("size: ")?.strip_suffix(','))
        .expect("the default model has no size")
        .replace('_', "")
        .parse()
        .expect("the size is a number");
    (id, format!("{:.1} GB", bytes as f64 / 1e9))
}

/// The cells of a Markdown table row, trimmed.
fn cells(row: &str) -> Vec<&str> {
    row.trim()
        .trim_matches('|')
        .split('|')
        .map(str::trim)
        .collect()
}

/// Device, backend and hardware of the first table of `docs/devices.md`.
fn devices_table(devices: &str) -> Vec<[String; 3]> {
    let rows: Vec<[String; 3]> = devices
        .lines()
        .skip_while(|line| !line.starts_with("| Device |"))
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .map(|row| {
            let c = cells(row);
            [c[0], c[1], c[2]].map(|cell| cell.replace('`', ""))
        })
        .collect();
    assert!(!rows.is_empty(), "docs/devices.md has no device table");
    rows
}

/// The machine of the guide's `smart-commit bench` example ("Ryzen 5 5500
/// with an RTX 3050") and its rows: device, time per message and whether it
/// was selected.
fn bench_example(devices: &str) -> (String, Vec<(String, String, bool)>) {
    let machine = devices
        .lines()
        .find_map(|line| line.strip_prefix("On a ")?.split_once(" (").map(|(m, _)| m))
        .expect("docs/devices.md has no benchmark example machine")
        .to_owned();
    let rows: Vec<(String, String, bool)> = devices
        .lines()
        .skip_while(|line| !line.starts_with("  device "))
        .skip(1)
        .take_while(|line| !line.is_empty())
        .map(|line| {
            let mut words = line.split_whitespace();
            let device = words.next().unwrap();
            let time = words.next().unwrap();
            (
                device.to_owned(),
                format!("{time} s"),
                line.contains("<- selected"),
            )
        })
        .collect();
    assert!(!rows.is_empty(), "docs/devices.md has no benchmark example");
    (machine, rows)
}

/// The titles (the bold part) of the first rows of the roadmap's `Next` table.
fn next_items(roadmap: &str) -> Vec<String> {
    let items: Vec<String> = roadmap
        .lines()
        .skip_while(|line| *line != "## Next")
        .filter(|line| {
            line.starts_with("| ") && line[2..].starts_with(|c: char| c.is_ascii_digit())
        })
        .filter_map(|row| {
            let item = cells(row).get(1).copied()?;
            Some(item.split("**").nth(1)?.to_owned())
        })
        .take(ROADMAP_ITEMS)
        .collect();
    assert!(!items.is_empty(), "docs/roadmap.md has no Next table");
    items
}

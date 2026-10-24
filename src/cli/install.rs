//! `smart-commit update` and `smart-commit uninstall`: they run the
//! install.sh that came with this release (in `share/smart-commit/scripts`),
//! which downloads releases, checks them against their SHA256SUMS and knows
//! which files it installed. Only an install made by install.sh has one: a
//! build from source updates with git and cargo.

use std::io::{self, BufRead, Write as _};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, Result, anyhow, bail};
use smart_commit::{paths, server, tui};

use super::settings::{env, resolve};

const INSTALL: &str =
    "curl -fsSL https://raw.githubusercontent.com/Lucas-BRT/smart-commit/main/install.sh | bash";

/// An installation made by install.sh: `PREFIX/bin/smart-commit`, with the
/// release's files in `PREFIX/share/smart-commit`.
#[derive(Debug, PartialEq, Eq)]
struct Installed {
    prefix: PathBuf,
    version: String,
    installer: PathBuf,
}

fn installed(exe: &Path) -> Result<Installed> {
    let not_installed = || {
        anyhow!(
            "{} was not installed by install.sh; a build from source is updated and removed \
             with git and cargo",
            exe.display()
        )
    };
    let bin = exe.parent().ok_or_else(not_installed)?;
    if bin.file_name().is_none_or(|name| name != "bin") {
        return Err(not_installed());
    }
    let prefix = bin.parent().ok_or_else(not_installed)?.to_path_buf();
    let share = prefix.join("share/smart-commit");
    let version = std::fs::read_to_string(share.join("VERSION")).map_err(|_| not_installed())?;
    let version = version.trim().to_owned();
    let installer = share.join("scripts/install.sh");
    if !installer.is_file() {
        bail!(
            "smart-commit {version} predates `smart-commit update` and `uninstall`; update once \
             with `{INSTALL} -s -- --no-setup`, or remove it with `{INSTALL} -s -- --purge`"
        );
    }
    Ok(Installed {
        prefix,
        version,
        installer,
    })
}

/// A model kept loaded runs the binary and the files about to change.
fn stop_kept_models() {
    if let Some(runtime) = paths::runtime_dir(env) {
        let stopped = server::stop(&runtime);
        if stopped > 0 {
            println!("Stopped {stopped} model(s) kept loaded.");
        }
    }
}

fn run_installer(install: &Installed, args: &[&str]) -> Result<()> {
    let status = Command::new("bash")
        .arg(&install.installer)
        .args(args)
        .arg("--prefix")
        .arg(&install.prefix)
        .status()
        .with_context(|| format!("cannot run {}", install.installer.display()))?;
    if !status.success() {
        bail!("{} failed ({status})", install.installer.display());
    }
    Ok(())
}

pub(super) fn update() -> Result<()> {
    let install = installed(&std::env::current_exe()?.canonicalize()?)?;
    println!(
        "smart-commit {} in {}",
        install.version,
        install.prefix.display()
    );
    stop_kept_models();
    run_installer(&install, &["--update"])
}

pub(super) fn uninstall(yes: bool, system: bool) -> Result<()> {
    let install = installed(&std::env::current_exe()?.canonicalize()?)?;
    print!("{}", plan(&install, system));
    if !yes {
        if !tui::interactive() {
            bail!("uninstall removes the models and the configuration; pass --yes");
        }
        print!("Remove all of it? [y/N] ");
        io::stdout().flush()?;
        let mut answer = String::new();
        io::stdin().lock().read_line(&mut answer)?;
        if !matches!(answer.trim(), "y" | "Y" | "yes" | "s" | "S" | "sim") {
            println!("Nothing was removed.");
            return Ok(());
        }
    }
    stop_kept_models();
    let mut args = vec!["--purge"];
    if system {
        args.push("--system");
    }
    if yes {
        args.push("--yes");
    }
    run_installer(&install, &args)
}

/// What uninstall removes and keeps, as install.sh --purge does it.
fn plan(install: &Installed, system: bool) -> String {
    let data = paths::data_dir(env);
    let mut text = format!(
        "This removes smart-commit {} from this machine:\n  binary:        {}\n",
        install.version,
        install.prefix.join("bin/smart-commit").display()
    );
    if let Some(data) = &data {
        let size = size_of(data);
        let size = if size > 0 {
            format!(" ({})", human(size))
        } else {
            String::new()
        };
        text += &format!(
            "  models, llama.cpp and the benchmark: {}{size}\n",
            data.display()
        );
    }
    if let Some(cache) = paths::cache_root(env) {
        text += &format!("  caches:        {}\n", cache.display());
    }
    if let Some(config) = paths::config_dir(env) {
        text += &format!("  configuration: {}\n", config.display());
    }
    if system {
        text += "  OpenVINO, if smart-commit's setup installed it (asks for sudo)\n";
    }
    text += "Kept: driver packages, repositories' .smart-commit.toml and their \
             prepare-commit-msg hooks (remove .git/hooks/prepare-commit-msg by hand).\n";
    for file in models_elsewhere(data.as_deref()) {
        text += &format!("Kept, not downloaded by smart-commit: {}\n", file.display());
    }
    text
}

/// Model files the settings name outside smart-commit's data directory
/// (`model.gguf`, `model.dir`): the user's own, which uninstall leaves.
fn models_elsewhere(data: Option<&Path>) -> Vec<PathBuf> {
    let Ok(resolved) = resolve(None, Vec::new()) else {
        return Vec::new();
    };
    let settings = resolved.settings();
    [settings.gguf, settings.model_dir]
        .into_iter()
        .flatten()
        .filter(|p| p.exists() && data.is_none_or(|d| !p.starts_with(d)))
        .collect()
}

/// Bytes under `path`, without following links.
fn size_of(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if !meta.is_dir() {
        return meta.len();
    }
    std::fs::read_dir(path)
        .map(|entries| entries.flatten().map(|e| size_of(&e.path())).sum())
        .unwrap_or(0)
}

fn human(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 30 => format!("{:.1} GB", b as f64 / (1u64 << 30) as f64),
        b if b >= 1 << 20 => format!("{:.0} MB", b as f64 / (1u64 << 20) as f64),
        b => format!("{} KB", b.div_ceil(1 << 10)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(dir: &Path, installer: bool) -> PathBuf {
        let share = dir.join("share/smart-commit/scripts");
        std::fs::create_dir_all(&share).unwrap();
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::write(dir.join("share/smart-commit/VERSION"), "0.8.0\n").unwrap();
        if installer {
            std::fs::write(share.join("install.sh"), "").unwrap();
        }
        dir.join("bin/smart-commit")
    }

    #[test]
    fn finds_what_install_sh_installed() {
        let dir = tempfile::TempDir::new().unwrap();
        let exe = layout(dir.path(), true);
        assert_eq!(
            installed(&exe).unwrap(),
            Installed {
                prefix: dir.path().to_path_buf(),
                version: "0.8.0".into(),
                installer: dir.path().join("share/smart-commit/scripts/install.sh"),
            }
        );
    }

    #[test]
    fn refuses_builds_and_older_installs() {
        let dir = tempfile::TempDir::new().unwrap();
        // A build from source: target/release/smart-commit.
        let err = installed(&dir.path().join("target/release/smart-commit")).unwrap_err();
        assert!(
            err.to_string().contains("not installed by install.sh"),
            "{err}"
        );
        // bin/ without the release's files.
        std::fs::create_dir_all(dir.path().join("bin")).unwrap();
        let err = installed(&dir.path().join("bin/smart-commit")).unwrap_err();
        assert!(
            err.to_string().contains("not installed by install.sh"),
            "{err}"
        );
        // A release from before install.sh was shipped.
        let exe = layout(dir.path(), false);
        let err = installed(&exe).unwrap_err();
        assert!(
            err.to_string()
                .contains("0.8.0 predates `smart-commit update`"),
            "{err}"
        );
        assert!(err.to_string().contains("--purge"), "{err}");
    }

    #[test]
    fn sizes_are_for_people() {
        assert_eq!(human(512), "1 KB");
        assert_eq!(human(5 << 20), "5 MB");
        assert_eq!(human(3 << 29), "1.5 GB");
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("a"), vec![0u8; 2048]).unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/b"), vec![0u8; 1024]).unwrap();
        assert_eq!(size_of(dir.path()), 3072);
        assert_eq!(size_of(&dir.path().join("missing")), 0);
    }
}

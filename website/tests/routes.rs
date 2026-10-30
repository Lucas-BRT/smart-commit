use smart_commit_website::router;
use topcoat::router::{Body, to_bytes};

async fn get(path: &str) -> (u16, String, String) {
    let request = http::Request::builder()
        .uri(path)
        .body(Body::empty())
        .unwrap();
    let response = router().handle(request).await;
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get(http::header::CONTENT_TYPE)
        .map(|value| value.to_str().unwrap().to_owned())
        .unwrap_or_default();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (
        status,
        content_type,
        String::from_utf8(bytes.to_vec()).unwrap(),
    )
}

async fn page(path: &str, needles: &[&str]) -> String {
    let (status, content_type, body) = get(path).await;
    assert_eq!(status, 200, "{path}");
    assert!(
        content_type.starts_with("text/html"),
        "{path}: {content_type}"
    );
    for needle in needles {
        assert!(body.contains(needle), "{path} lacks {needle:?}");
    }
    body
}

#[tokio::test]
async fn pages_are_accessible_documents() {
    for path in [
        "/",
        "/install",
        "/tutorial",
        "/settings",
        "/troubleshooting",
        "/devices",
        "/measurements",
        "/how-it-works",
        "/roadmap",
    ] {
        let body = page(
            path,
            &[
                "<html lang=\"en\">",
                "href=\"#main\"",
                "<main id=\"main\"",
                "<h1",
            ],
        )
        .await;
        assert_eq!(body.matches("<h1").count(), 1, "{path}: one h1");
        assert!(!body.contains("<script"), "{path}: no script");
        assert!(!body.contains("http://"), "{path}: no insecure link");
    }
}

#[tokio::test]
async fn every_doc_page_lists_all_the_docs() {
    for path in [
        "/tutorial",
        "/install",
        "/settings",
        "/troubleshooting",
        "/devices",
        "/measurements",
        "/how-it-works",
        "/roadmap",
    ] {
        let body = page(path, &["aria-label=\"Documentation\""]).await;
        for target in [
            "/tutorial",
            "/install",
            "/settings",
            "/troubleshooting",
            "/devices",
            "/measurements",
            "/how-it-works",
            "/roadmap",
        ] {
            assert!(
                body.contains(&format!("href=\"{target}\"")),
                "{path} lacks {target}"
            );
        }
    }
}

#[tokio::test]
async fn navigation_marks_the_current_page() {
    let body = page("/devices", &[]).await;
    assert!(body.contains("href=\"/devices\" aria-current=\"page\""));
    assert!(!body.contains("href=\"/install\" aria-current"));
}

#[tokio::test]
async fn stylesheet_is_css() {
    let (status, content_type, body) = get("/style.css").await;
    assert_eq!(status, 200);
    assert!(content_type.starts_with("text/css"));
    assert!(body.contains(".flex-wrap"), "the Tailwind utilities");
}

#[tokio::test]
async fn the_display_font_is_served_by_the_site() {
    let request = http::Request::builder()
        .uri("/fonts/bricolage-grotesque-latin.woff2")
        .body(Body::empty())
        .unwrap();
    let response = router().handle(request).await;
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(response.headers()[http::header::CONTENT_TYPE], "font/woff2");
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(&bytes[..4], b"wOF2", "a WOFF2 file");
    let (_, _, css) = get("/style.css").await;
    assert!(css.contains("/fonts/bricolage-grotesque-latin.woff2"));
    assert!(!css.contains("fonts.googleapis.com"), "no font host");
}

#[tokio::test]
async fn logo_is_an_svg_and_the_pages_link_it_as_favicon() {
    let (status, content_type, body) = get("/logo.svg").await;
    assert_eq!(status, 200);
    assert_eq!(content_type, "image/svg+xml");
    assert!(body.starts_with("<svg"));
    page("/", &[r#"rel="icon" href="/logo.svg""#]).await;
}

#[tokio::test]
async fn unknown_paths_get_a_404_page_with_the_layout() {
    let (status, content_type, body) = get("/no/such/page").await;
    assert_eq!(status, 404);
    assert!(content_type.starts_with("text/html"));
    assert!(body.contains("Page not found"));
    assert!(
        body.contains("aria-label=\"Main\""),
        "the layout around the 404"
    );
}

// The site takes its facts from the product's files; these tests read the
// same files on their own, so a wrong parse in build.rs cannot hide.

fn product(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn value(text: &str, key: &str) -> String {
    let prefix = format!("{key} = \"");
    text.lines()
        .find_map(|line| line.strip_prefix(&prefix)?.strip_suffix('"'))
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn every_page_names_the_version_of_the_product() {
    let version = value(&product("cli/Cargo.toml"), "version");
    for path in [
        "/",
        "/install",
        "/tutorial",
        "/settings",
        "/troubleshooting",
        "/devices",
        "/measurements",
        "/how-it-works",
        "/roadmap",
    ] {
        page(path, &[&format!("smart-commit {version}")]).await;
    }
}

#[tokio::test]
async fn the_repository_and_the_installer_are_the_products() {
    let repository = value(&product("cli/Cargo.toml"), "repository");
    let usage = product("docs/usage.md");
    let install = usage.lines().find(|l| l.starts_with("curl -fsSL")).unwrap();
    page("/", &[&format!("href=\"{repository}\""), install]).await;
    page("/install", &[install]).await;
    assert!(install.contains(&repository.replace("github.com", "raw.githubusercontent.com")));
}

#[tokio::test]
async fn install_names_the_default_model_and_its_size() {
    let registry = product("cli/src/models/registry.rs");
    let id = registry
        .lines()
        .find_map(|l| {
            l.strip_prefix("pub const DEFAULT: &str = \"")?
                .strip_suffix("\";")
        })
        .unwrap();
    let entry = registry.split_once(&format!("id: \"{id}\",")).unwrap().1;
    let bytes: f64 = entry
        .lines()
        .find_map(|l| l.trim().strip_prefix("size: ")?.strip_suffix(','))
        .unwrap()
        .replace('_', "")
        .parse()
        .unwrap();
    page("/install", &[id, &format!("{:.1} GB", bytes / 1e9)]).await;
}

#[tokio::test]
async fn install_lists_the_platform_and_the_download_sizes() {
    let usage = product("docs/usage.md");
    let platform = usage
        .lines()
        .find_map(|l| l.strip_prefix("Requirements: ")?.split_once('.'))
        .unwrap()
        .0;
    let glibc = product("install.sh")
        .lines()
        .find_map(|l| l.strip_prefix("MIN_GLIBC=").map(str::to_owned))
        .unwrap();
    let install = product("cli/src/backend/llamacpp/install.rs");
    let bytes: f64 = install
        .lines()
        .find_map(|l| {
            l.strip_prefix("pub const ARCHIVE_SIZE: u64 = ")?
                .strip_suffix(';')
        })
        .unwrap()
        .replace('_', "")
        .parse()
        .unwrap();
    page(
        "/install",
        &[
            "id=\"requirements\"",
            &format!("Platform: {platform}, with glibc {glibc} or newer, and git."),
            &format!("{:.1} MB", bytes / 1e6),
            "~/.local/share/smart-commit",
            "~/.cache/smart-commit",
        ],
    )
    .await;
}

#[tokio::test]
async fn devices_are_the_ones_in_the_devices_guide() {
    let guide = product("docs/devices.md");
    let table: Vec<&str> = guide
        .lines()
        .skip_while(|l| !l.starts_with("| Device |"))
        .skip(2)
        .take_while(|l| l.starts_with('|'))
        .collect();
    assert!(table.len() >= 5, "the guide lists the devices");
    let body = page("/devices", &["OpenVINO GenAI", "llama.cpp"]).await;
    for row in table {
        let device = row.split('|').nth(1).unwrap().trim().replace('`', "");
        assert!(body.contains(&format!("<code>{device}</code>")), "{device}");
    }
}

#[tokio::test]
async fn the_roadmap_page_starts_with_the_first_item_of_the_roadmap() {
    let roadmap = product("docs/roadmap.md");
    let first = roadmap
        .lines()
        .skip_while(|l| *l != "## Next")
        .find(|l| l.starts_with("| 1 |"))
        .unwrap();
    let title = first.split("**").nth(1).unwrap();
    page("/roadmap", &[title]).await;
}

#[tokio::test]
async fn the_tutorial_review_screen_is_the_one_in_the_usage_guide() {
    let usage = product("docs/usage.md");
    let status = usage
        .lines()
        .find(|l| l.starts_with(" qwen2.5-coder-1.5b on cpu"))
        .unwrap();
    let body = page("/tutorial", &[status]).await;
    for key in [
        "[Enter] commit",
        "[e] edit, then commit",
        "[a] another message",
        "[q] cancel",
    ] {
        assert!(usage.contains(key) && body.contains(key), "{key}");
    }
}

#[tokio::test]
async fn the_benchmark_example_is_the_one_in_the_devices_guide() {
    let guide = product("docs/devices.md");
    let machine = guide
        .lines()
        .find_map(|l| l.strip_prefix("On a ")?.split_once(" (").map(|(m, _)| m))
        .unwrap();
    let body = page("/devices", &[&format!("On a {machine}:")]).await;
    // What the page shows, not how it is marked up.
    let text = strip_tags(&body);
    let rows: Vec<&str> = guide
        .lines()
        .skip_while(|l| !l.starts_with("  device "))
        .skip(1)
        .take_while(|l| !l.is_empty())
        .collect();
    for row in rows {
        let mut words = row.split_whitespace();
        let (device, time) = (words.next().unwrap(), words.next().unwrap());
        assert!(
            text.contains(&format!("  {device:<15}{time} s")),
            "{device} {time}"
        );
        assert_eq!(
            row.contains("<- selected"),
            text.contains(&format!("{device:<15}{time} s  <- selected")),
            "{device}"
        );
    }
}

// The measurements page draws numbers of docs/measurements.md; these tests
// read that file on their own and look for each of them on the page.

/// The rows of the `n`th table (from 0) under the heading `## {heading}`,
/// each cell without its Markdown emphasis and backticks.
fn table_rows(doc: &str, heading: &str, n: usize) -> Vec<Vec<String>> {
    let section = doc
        .split_once(&format!("\n## {heading}\n"))
        .unwrap_or_else(|| panic!("no section {heading}"))
        .1;
    let section = section.split("\n## ").next().unwrap();
    let mut tables: Vec<Vec<Vec<String>>> = Vec::new();
    let mut in_table = false;
    for line in section.lines() {
        if line.starts_with('|') {
            if !in_table {
                tables.push(Vec::new());
                in_table = true;
            }
            let cells: Vec<String> = line
                .trim_matches('|')
                .split('|')
                .map(|c| c.trim().replace("**", "").replace('`', ""))
                .collect();
            tables.last_mut().unwrap().push(cells);
        } else {
            in_table = false;
        }
    }
    // Drop the header and the rule under it.
    tables[n].iter().skip(2).cloned().collect()
}

#[tokio::test]
async fn measurements_draws_the_numbers_of_the_measurements_document() {
    let doc = product("docs/measurements.md");
    let body = page("/measurements", &["The first message and the next ones"]).await;
    let expect = |text: &str| {
        let text = compact(text);
        assert!(body.contains(&text), "the page lacks {text:?}");
    };

    for row in table_rows(&doc, "Keeping the model loaded", 0) {
        expect(&row[0]);
        expect(&row[1]);
        expect(&row[2]);
    }
    for row in table_rows(&doc, "Keeping the model loaded", 1) {
        for cell in &row {
            expect(cell);
        }
    }
    for row in table_rows(&doc, "Per message, by device", 0) {
        expect(&row[0]);
        expect(&row[1]);
    }
    for row in table_rows(&doc, "Per message, by device", 2) {
        for cell in [&row[0], &row[1], &row[2], &row[3], &row[5]] {
            expect(cell);
        }
    }
    for row in table_rows(&doc, "Per message, by model", 0) {
        if let Some((cpu, vulkan)) = row[4].split_once(" / ")
            && cpu != "—"
        {
            expect(&row[0].replace(" (default)", ""));
            expect(cpu);
            expect(vulkan);
        }
    }
    for row in table_rows(&doc, "Where the time goes on the CPU and Vulkan", 0) {
        if !row[0].starts_with("0.5B") {
            expect(&format!("{} ms", row[4]));
        }
    }
    for row in table_rows(&doc, "Asking for the type", 0) {
        expect(&row[0]);
        expect(&row[2]);
    }
}

// What the pages show must hold against the product: its files, its sources
// and its documents. No test here reads a sentence of marketing copy.

const PAGES: [&str; 9] = [
    "/",
    "/tutorial",
    "/install",
    "/settings",
    "/troubleshooting",
    "/devices",
    "/measurements",
    "/how-it-works",
    "/roadmap",
];

/// Everything in `html` between `open` and `close`, for each occurrence.
fn between<'a>(html: &'a str, open: &str, close: &str) -> Vec<&'a str> {
    let mut found = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find(open) {
        let after = &rest[start + open.len()..];
        // `open` is the start of a tag: skip to the end of it.
        let Some(tag_end) = after.find('>') else {
            break;
        };
        let inner = &after[tag_end + 1..];
        let Some(end) = inner.find(close) else { break };
        found.push(&inner[..end]);
        rest = &inner[end + close.len()..];
    }
    found
}

fn strip_tags(html: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => text.push(c),
            _ => {}
        }
    }
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// The code a page shows: inline `code` and every line of a terminal.
fn snippets(html: &str) -> Vec<String> {
    let mut found: Vec<String> = between(html, "<code", "</code>")
        .into_iter()
        .map(strip_tags)
        .collect();
    for pre in between(html, "<pre", "</pre>") {
        found.extend(strip_tags(pre).lines().map(str::to_owned));
    }
    found
}

/// A time as the site writes it: no space before the unit (`2.8s`, `7873ms`),
/// where the document writes `2.8 s`. Other text is left as it is.
fn compact(text: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let after_digit = i > 0 && chars[i - 1].is_ascii_digit();
        if chars[i] == ' ' && after_digit {
            let unit: String = chars[i + 1..]
                .iter()
                .take_while(|c| c.is_alphabetic())
                .collect();
            if unit == "s" || unit == "ms" {
                i += 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn squeeze(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The commands a page shows, each as typed: `smart-commit ...` without the
/// prompt, a trailing comment or a placeholder.
fn commands(html: &str) -> Vec<String> {
    snippets(html)
        .iter()
        .filter_map(|snippet| {
            let line = snippet.trim();
            let line = line.strip_prefix("$ ").unwrap_or(line);
            let line = line.split_once(" # ").map_or(line, |(command, _)| command);
            let line = squeeze(line);
            (line.starts_with("smart-commit ") && !line.contains(['<', '…'])).then_some(line)
        })
        .collect()
}

fn docs_text() -> String {
    [
        "docs/usage.md",
        "docs/configuration.md",
        "docs/devices.md",
        "docs/models.md",
        "README.md",
    ]
    .iter()
    .map(|name| squeeze(&product(name)))
    .collect::<Vec<_>>()
    .join("\n")
}

/// The source of the product: its Rust code and its scripts.
fn product_sources() -> String {
    fn walk(dir: &std::path::Path, out: &mut String) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if matches!(path.extension().and_then(|e| e.to_str()), Some("rs" | "sh")) {
                out.push_str(&std::fs::read_to_string(&path).unwrap_or_default());
            }
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut out = product("install.sh");
    for dir in ["cli/src", "scripts"] {
        walk(&root.join(dir), &mut out);
    }
    out
}

fn attribute_values(html: &str, attribute: &str) -> Vec<String> {
    let pattern = format!(" {attribute}=\"");
    html.match_indices(&pattern)
        .filter_map(|(at, _)| {
            let rest = &html[at + pattern.len()..];
            rest.find('"').map(|end| rest[..end].to_owned())
        })
        .collect()
}

#[tokio::test]
async fn every_link_of_the_site_leads_somewhere() {
    for path in PAGES {
        let (_, _, body) = get(path).await;
        let own_ids = attribute_values(&body, "id");
        for href in attribute_values(&body, "href") {
            if href.starts_with("http") || href.starts_with("//") {
                continue;
            }
            let (target, fragment) = match href.split_once('#') {
                Some((target, fragment)) => (target, Some(fragment)),
                None => (href.as_str(), None),
            };
            let ids = if target.is_empty() {
                own_ids.clone()
            } else {
                let (status, _, linked) = get(target).await;
                assert_eq!(status, 200, "{path} links to {href}");
                attribute_values(&linked, "id")
            };
            if let Some(fragment) = fragment {
                assert!(
                    ids.iter().any(|id| id == fragment),
                    "{path} links to {href}, which has no #{fragment}"
                );
            }
        }
    }
}

#[tokio::test]
async fn every_command_a_page_shows_is_in_the_documentation() {
    let docs = docs_text();
    let mut checked = 0;
    for path in PAGES {
        let (_, _, body) = get(path).await;
        for command in commands(&body) {
            assert!(
                docs.contains(&command),
                "{path} shows `{command}`, which no document has"
            );
            checked += 1;
        }
    }
    assert!(checked > 30, "the pages show commands: {checked}");
}

#[tokio::test]
async fn every_setting_and_variable_a_page_names_exists() {
    let configuration = product("docs/configuration.md");
    let mut checked = 0;
    for path in PAGES {
        let (_, _, body) = get(path).await;
        for snippet in snippets(&body) {
            for word in snippet.split_whitespace() {
                let word = word.trim_matches(|c: char| ",;:.()\"".contains(c));
                if let Some(key) = ["model.", "style.", "update."]
                    .iter()
                    .find_map(|prefix| word.starts_with(prefix).then_some(word))
                    && key
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c == '.' || c == '_')
                {
                    assert!(
                        configuration.contains(key),
                        "{path} names {key}, which is no setting"
                    );
                    checked += 1;
                }
                if let Some(name) = word.strip_prefix("SMART_COMMIT_") {
                    let name = name.split('=').next().unwrap();
                    if !name.contains('*') {
                        let variable = format!("SMART_COMMIT_{name}");
                        assert!(configuration.contains(&variable), "{path} names {variable}");
                        checked += 1;
                    }
                }
            }
        }
    }
    assert!(checked >= 8, "the pages name settings: {checked}");
}

#[tokio::test]
async fn the_error_messages_of_troubleshooting_are_in_the_product() {
    let sources = product_sources();
    let usage = product("docs/usage.md");
    // The fixed part of each message the page lists; a few are built with a
    // device's name, so the part before it is what the source holds.
    for message in [
        "smart-commit is not set up yet",
        "no device could run the model",
        "cannot load the model on the",
        "has no driver bound",
        "has no OpenVINO version",
        "OpenVINO GenAI not found",
        "is the old name of",
    ] {
        assert!(sources.contains(message), "no source prints {message:?}");
        assert!(usage.contains(message), "the usage guide lacks {message:?}");
        page("/troubleshooting", &[message]).await;
    }
}

#[tokio::test]
async fn the_doctor_example_is_the_one_in_the_usage_guide() {
    let usage = product("docs/usage.md");
    let example: Vec<&str> = usage
        .lines()
        .skip_while(|l| !l.starts_with("A run on an installed machine"))
        .skip(3)
        .take_while(|l| !l.starts_with("```"))
        .collect();
    assert_eq!(example.len(), 9, "the doctor example has nine lines");
    let body = page("/troubleshooting", &[]).await;
    for line in example {
        assert!(body.contains(line), "doctor example lacks {line:?}");
    }
}

#[tokio::test]
async fn the_installer_options_are_the_ones_in_the_usage_guide() {
    let usage = product("docs/usage.md");
    let options: Vec<&str> = usage
        .lines()
        .skip_while(|l| !l.starts_with("| Option | Effect |"))
        .skip(2)
        .take_while(|l| l.starts_with('|'))
        .collect();
    assert_eq!(options.len(), 9, "the guide lists nine installer options");
    let body = page("/install", &[]).await;
    for row in options {
        let option = row.split('|').nth(1).unwrap().trim().replace('`', "");
        assert!(body.contains(&option), "/install lacks {option}");
    }
}

#[tokio::test]
async fn the_models_listed_are_the_registry_with_their_sizes() {
    let registry = product("cli/src/models/registry.rs");
    let registry = registry.split("#[cfg(test)]").next().unwrap();
    let ids: Vec<&str> = registry
        .lines()
        .filter_map(|l| l.trim().strip_prefix("id: \"")?.strip_suffix("\","))
        .collect();
    let sizes: Vec<f64> = registry
        .lines()
        .filter_map(|l| l.trim().strip_prefix("size: ")?.strip_suffix(','))
        .map(|n| n.replace('_', "").parse().unwrap())
        .collect();
    assert_eq!(ids.len(), sizes.len(), "one size for each model");
    assert!(ids.len() >= 9, "the registry has its models");
    let body = page("/settings", &[]).await;
    for (id, bytes) in ids.iter().zip(sizes) {
        assert!(
            body.contains(&format!("<code>{id}</code>")),
            "/settings lacks {id}"
        );
        let size = if bytes >= 1e9 {
            format!("{:.1} GB", bytes / 1e9)
        } else {
            format!("{} MB", (bytes / 1e6).floor())
        };
        assert!(body.contains(&size), "/settings lacks {size} for {id}");
    }
}

#[tokio::test]
async fn the_home_page_shows_the_times_of_the_measurements_document() {
    let doc = product("docs/measurements.md");
    let kept = table_rows(&doc, "Keeping the model loaded", 0);
    let npu = table_rows(&doc, "Per message, by device", 0)
        .into_iter()
        .find(|row| row[0] == "npu")
        .unwrap();
    // The first two rows are `vulkan` and `cpu` of the default model; the NPU
    // has no server, so its time is the one of every message.
    page(
        "/",
        &[
            &compact(&kept[0][2]),
            &compact(&kept[1][2]),
            &compact(&npu[1]),
        ],
    )
    .await;
}

#[tokio::test]
async fn the_npu_numbers_are_the_ones_in_the_measurements_document() {
    let doc = squeeze(&product("docs/measurements.md"));
    assert!(doc.contains("(11.7 s); later loads read it from the cache (1.7 s, 246 MB)"));
    assert!(
        squeeze(&product("docs/roadmap.md"))
            .contains("the NPU and the Intel GPU, which have no server")
    );
    page(
        "/measurements",
        &["(11.7s)", "(1.7s, 246 MB)", "have no server"],
    )
    .await;
}

#[test]
fn compact_times_drop_the_space_before_the_unit_and_nothing_else() {
    assert_eq!(compact("2.8 s"), "2.8s");
    assert_eq!(compact("7873 ms"), "7873ms");
    assert_eq!(compact("1.1 GB as shipped"), "1.1 GB as shipped");
    assert_eq!(compact("4.0 s / 2.4 s"), "4.0s / 2.4s");
}

#[tokio::test]
async fn each_documentation_page_ends_with_one_button_to_the_next_page() {
    // The order of the side list is the order of the pages.
    let (_, _, first) = get("/tutorial").await;
    let list = first
        .split_once("aria-label=\"Documentation\"")
        .unwrap()
        .1
        .split("</nav>")
        .next()
        .unwrap();
    let order = attribute_values(list, "href");
    assert!(
        order.len() >= 8,
        "the side list names every page: {order:?}"
    );

    for (at, path) in order.iter().enumerate() {
        let (_, _, body) = get(path).await;
        let next = body
            .split_once("aria-label=\"Next page\"")
            .map(|(_, rest)| rest.split("</nav>").next().unwrap().to_owned());
        match order.get(at + 1) {
            Some(following) => {
                let links = attribute_values(
                    &next.unwrap_or_else(|| panic!("{path} has no next button")),
                    "href",
                );
                assert_eq!(
                    links,
                    std::slice::from_ref(following),
                    "{path} leads to the next page only"
                );
            }
            None => assert!(next.is_none(), "{path} is the last page: nothing follows"),
        }
    }
}

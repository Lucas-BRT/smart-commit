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
async fn home_names_the_tool_and_shows_the_install_and_the_demo() {
    page(
        "/",
        &[
            "<title>smart-commit",
            "Commit messages in your repo's style, on your own machine.",
            "install.sh | bash",
            "git status -s",
            "refactor(auth): move session handling into its own module",
            "NPU, GPU or CPU",
            "https://github.com/Lucas-BRT/smart-commit",
        ],
    )
    .await;
}

#[tokio::test]
async fn install_has_the_installer_command() {
    page(
        "/install",
        &[
            "curl -fsSL https://raw.githubusercontent.com/Lucas-BRT/smart-commit/main/install.sh | bash",
            "smart-commit hook install",
        ],
    )
    .await;
}

#[tokio::test]
async fn tutorial_covers_staging_review_and_the_hook() {
    page(
        "/tutorial",
        &[
            "git add -p",
            "smart-commit --commit",
            "[a] another message",
            "smart-commit hook install",
        ],
    )
    .await;
}

#[tokio::test]
async fn how_it_works_describes_the_pipeline() {
    let body = page("/how-it-works", &["Infer the style", "one retry"]).await;
    // Messages are English only; the style no longer includes a language.
    assert!(!body.contains("language"), "the style has no language");
}

#[tokio::test]
async fn roadmap_lists_what_is_next() {
    let body = page("/roadmap", &["Next"]).await;
    assert!(!body.contains("Publish v0.2"), "v0.2 is published");
}

#[tokio::test]
async fn pages_are_accessible_documents() {
    for path in [
        "/",
        "/install",
        "/tutorial",
        "/devices",
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
        "/devices",
        "/how-it-works",
        "/roadmap",
    ] {
        let body = page(path, &["aria-label=\"Documentation\""]).await;
        for target in [
            "/tutorial",
            "/install",
            "/devices",
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
    assert!(body.contains("--section-gap"), "the site stylesheet");
    assert!(body.contains(".flex-wrap"), "the Tailwind utilities");
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
    assert!(body.contains("site-nav"));
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
        "/devices",
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
            &format!("Platform: {platform}."),
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
async fn the_benchmark_example_is_the_one_in_the_devices_guide() {
    let guide = product("docs/devices.md");
    let machine = guide
        .lines()
        .find_map(|l| l.strip_prefix("On a ")?.split_once(" (").map(|(m, _)| m))
        .unwrap();
    let body = page("/devices", &[&format!("On a {machine}:")]).await;
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
            body.contains(&format!("  {device:<15}{time} s")),
            "{device} {time}"
        );
        assert_eq!(
            row.contains("<- selected"),
            body.contains(&format!("{device:<15}{time} s  <span class=\"sel\">")),
            "{device}"
        );
    }
}

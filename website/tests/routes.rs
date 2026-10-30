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
            "Commit messages in your repo's style, offline.",
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
async fn devices_lists_every_device() {
    let body = page(
        "/devices",
        &["OpenVINO GenAI", "llama.cpp", "AMD, NVIDIA, Intel"],
    )
    .await;
    for device in ["npu", "intel-gpu", "vulkan", "cpu", "openvino-cpu"] {
        assert!(body.contains(&format!("<code>{device}</code>")), "{device}");
    }
}

#[tokio::test]
async fn how_it_works_describes_the_pipeline() {
    page("/how-it-works", &["Infer the style", "one retry"]).await;
}

#[tokio::test]
async fn roadmap_lists_what_is_next() {
    page("/roadmap", &["Next", "Publish v0.2"]).await;
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

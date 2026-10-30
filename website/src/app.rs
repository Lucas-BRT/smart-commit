mod devices;
mod how_it_works;
mod install;
mod roadmap;
mod tutorial;

use http::header::{CACHE_CONTROL, CONTENT_TYPE};
use topcoat::{
    Result,
    context::Cx,
    router::{
        Router, RouterBuilderDiscoverExt, Slot, StatusCode, error::NotFoundError, href, layout,
        module_router, not_found, page, request::uri, route,
    },
    view::{View, error_boundary, view},
};

use crate::components::button::{ButtonSize, ButtonVariant, button_variants};
use crate::ui::{INSTALL_CMD, REPO, facts::VERSION};

pub fn router() -> Router {
    module_router!().discover().build()
}

// Built by build.rs: Tailwind's utilities plus assets/style.css.
const STYLE: &str = include_str!(concat!(env!("OUT_DIR"), "/tailwind.css"));
const LOGO: &str = include_str!("../assets/logo.svg");

/// Debug builds (what `topcoat dev` runs) never cache, so a phone sees each edit on reload.
const ASSET_CACHE: &str = if cfg!(debug_assertions) {
    "no-store"
} else {
    "public, max-age=3600"
};

#[route(GET "/style.css")]
async fn style() -> Result<impl topcoat::router::response::IntoResponse> {
    Ok((
        [
            (CONTENT_TYPE, "text/css; charset=utf-8"),
            (CACHE_CONTROL, ASSET_CACHE),
        ],
        STYLE,
    ))
}

/// The mark: favicon and header logo.
#[route(GET "/logo.svg")]
async fn logo() -> Result<impl topcoat::router::response::IntoResponse> {
    Ok((
        [
            (CONTENT_TYPE, "image/svg+xml"),
            (CACHE_CONTROL, ASSET_CACHE),
        ],
        LOGO,
    ))
}

/// Title and description of each page, by path.
fn meta(path: &str) -> (&'static str, &'static str) {
    match path {
        "/" => (
            "smart-commit: commit messages in your repo's style, run locally",
            "Commit messages in your repo's style, from a small model that runs on your own hardware: no account, no API key, nothing sent anywhere.",
        ),
        "/install" => (
            "Install and use smart-commit",
            "Install smart-commit, write your first message, add the git hook and change devices, models and settings.",
        ),
        "/devices" => (
            "Devices: NPU, GPU and CPU - smart-commit",
            "smart-commit runs on an Intel NPU, an Intel GPU, any GPU with Vulkan, or the CPU. Which device does what, and how the fastest is chosen.",
        ),
        "/how-it-works" => (
            "How smart-commit works",
            "From git history and the staged diff to a message: style detection, diff fitting, prompt, model, sanitizing, retry and fallback.",
        ),
        "/tutorial" => (
            "Your first commit - smart-commit",
            "A five-minute tutorial: install smart-commit, stage your changes, preview a message, review it and commit.",
        ),
        "/roadmap" => (
            "Roadmap - smart-commit",
            "What smart-commit has done and what comes next.",
        ),
        _ => ("Page not found - smart-commit", "This page does not exist."),
    }
}

#[layout]
async fn root_layout(cx: &Cx, slot: Slot<'_>) -> Result<impl View> {
    let path = uri(cx).path().to_owned();
    let (title, description) = meta(&path);
    let docs_cur = [
        "/tutorial",
        "/install",
        "/devices",
        "/how-it-works",
        "/roadmap",
    ]
    .contains(&path.as_str())
    .then_some("page");

    Ok(view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8">
                <meta name="viewport" content="width=device-width, initial-scale=1">
                <title>(title)</title>
                <meta name="description" content=(description)>
                <meta name="color-scheme" content="dark">
                <link rel="icon" href="/logo.svg" type="image/svg+xml">
                <link rel="stylesheet" href="/style.css">
                topcoat::dev::script()
            </head>
            <body>
                <a class="skip" href="#main">"Skip to content"</a>
                <header class="site-header">
                    <div class="wrap">
                        <a class="brand" href=(href!(page))>"smart-commit"</a>
                        <nav class="site-nav" aria-label="Main">
                            <ul>
                                <li><a href=(href!(tutorial::page)) aria-current=(docs_cur)>"Docs"</a></li>
                                <li><a href=(REPO)>"GitHub"</a></li>
                            </ul>
                        </nav>
                    </div>
                </header>
                <main id="main" class="wrap">
                    error_boundary(
                        fallback: |error| {
                            if error.downcast_ref::<NotFoundError>().is_none() {
                                // Any other error is rethrown for the router to answer.
                                return Err(error);
                            }
                            Ok(view! {
                                (StatusCode::NOT_FOUND)
                                <header class="page-head">
                                    <p class="eyebrow">"404"</p>
                                    <h1>"Page not found"</h1>
                                    <p class="lede">
                                        "This page does not exist. "
                                        <a href=(href!(page))>"Go to the home page"</a>
                                        "."
                                    </p>
                                </header>
                            })
                        },
                        (slot)
                    )
                </main>
                <footer class="site-footer">
                    <div class="wrap">
                        <p>"smart-commit " (VERSION) ". Released under the MIT License. Models are downloaded from their publishers under their own licences."</p>
                    </div>
                </footer>
            </body>
        </html>
    })
}

#[page]
pub async fn page() -> Result<impl View> {
    Ok(view! {
        <section class="pt-10 text-center md:pt-16">
            <h1 class="font-mono text-[clamp(2.25rem,8vw,5.5rem)] leading-[1.05] font-extrabold tracking-tighter">
                "smart-commit"
                <span class="ml-[0.12em] inline-block h-[0.8em] w-[0.45em] bg-primary align-[-0.06em]" aria-hidden="true"></span>
            </h1>
            <p class="mx-auto mt-5 max-w-none text-[clamp(1.2rem,2.8vw,1.75rem)] leading-snug text-balance text-muted-foreground">"Commit messages in your repo's style, on your own machine."</p>

            <div class="mx-auto mt-10 flex max-w-4xl flex-wrap items-center justify-between gap-4 rounded-xl border border-border bg-(--term-bg) px-5 py-4 text-left font-mono text-[0.9375rem] leading-relaxed text-(--term-ink)">
                <span class="min-w-0 select-all [overflow-wrap:anywhere]"><span class="text-(--term-green)">"$ "</span>(INSTALL_CMD)</span>
            </div>

            <div class="mt-8 flex flex-wrap justify-center gap-3.5">
                <a href=(href!(tutorial::page)) class=(button_variants(ButtonVariant::Primary, ButtonSize::Lg))>"Get started"</a>
                <a href=(REPO) class=(button_variants(ButtonVariant::Outline, ButtonSize::Lg))>"GitHub"</a>
            </div>
        </section>

        <section class="mx-auto mt-12 max-w-4xl md:mt-16" aria-label="Running smart-commit">
            <pre class="m-0 rounded-2xl border border-border bg-(--term-bg) p-6 font-mono text-base leading-[1.65] whitespace-pre-wrap text-(--term-ink) [overflow-wrap:anywhere]">
                <span class="block"><span class="text-(--term-green)">"$ "</span>"git status -s"</span>
                <span class="block text-(--term-dim)">" M src/auth.rs"</span>
                <span class="block text-(--term-dim)">" M src/middleware.rs"</span>
                <span class="block text-(--term-dim)">" M src/routes/login.rs"</span>
                <span class="block text-(--term-dim)">" M src/routes/logout.rs"</span>
                <span class="block text-(--term-dim)">" M src/routes/refresh.rs"</span>
                <span class="block text-(--term-dim)">" M tests/auth.rs"</span>
                <span class="block text-(--term-dim)">" M tests/routes.rs"</span>
                <span class="block text-(--term-dim)">"?? src/session/mod.rs"</span>
                <span class="block text-(--term-dim)">"?? src/session/store.rs"</span>
                <span class="block"><span class="text-(--term-green)">"$ "</span>"git add -p"</span>
                <span class="block"><span class="text-(--term-green)">"$ "</span>"smart-commit"</span>
                <span class="block text-(--term-amber)">"refactor(auth): move session handling into its own module"</span>
            </pre>
        </section>

        <dl class="mx-auto mt-14 grid max-w-4xl grid-cols-2 gap-x-8 gap-y-6 md:mt-16 md:grid-cols-4">
            <div>
                <dt class="text-[0.8125rem] text-muted-foreground">"License"</dt>
                <dd class="mt-1 ml-0 font-mono font-semibold">"MIT"</dd>
            </div>
            <div>
                <dt class="text-[0.8125rem] text-muted-foreground">"Written in"</dt>
                <dd class="mt-1 ml-0 font-mono font-semibold">"Rust"</dd>
            </div>
            <div>
                <dt class="text-[0.8125rem] text-muted-foreground">"Platform"</dt>
                <dd class="mt-1 ml-0 font-mono font-semibold">"Linux x86_64"</dd>
            </div>
            <div>
                <dt class="text-[0.8125rem] text-muted-foreground">"Runs on"</dt>
                <dd class="mt-1 ml-0 font-mono font-semibold">"NPU, GPU or CPU"</dd>
            </div>
        </dl>
    })
}

// A URL matching no route would get a bare 404 that skips the layout.
not_found!();

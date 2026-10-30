//! Components shared by the pages.

use topcoat::{
    Result,
    view::{Child, View, component, view},
};

/// What the site says about the product, read from the product's own files
/// by `build.rs`: the version, the repository, the install command, the
/// default model, the devices and the next roadmap items.
pub mod facts {
    include!(concat!(env!("OUT_DIR"), "/facts.rs"));
}

pub use facts::{INSTALL_CMD, REPO};

pub fn docs_url(page: &str) -> String {
    format!("{REPO}/blob/main/docs/{page}")
}

/// A dark terminal window around preformatted text. The `pre` can scroll
/// sideways, so it takes the keyboard focus.
#[component]
pub async fn terminal(title: &str, label: &str, #[default] child: Child<'_>) -> Result<impl View> {
    Ok(view! {
        <figure class="term">
            <div class="term-bar" aria-hidden="true">(title)</div>
            <pre tabindex="0" role="region" aria-label=(label)>(child)</pre>
        </figure>
    })
}

/// A table that scrolls sideways on narrow screens, reachable by keyboard.
#[component]
pub async fn table(label: &str, #[default] child: Child<'_>) -> Result<impl View> {
    Ok(view! {
        <div class="table-wrap" tabindex="0" role="region" aria-label=(label)>
            <table class="table-wide">(child)</table>
        </div>
    })
}

/// The plain heading block at the top of every page but the home page.
#[component]
pub async fn page_head(title: &str, lede: &str) -> Result<impl View> {
    Ok(view! {
        <header class="page-head">
            <h1>(title)</h1>
            <p class="lede">(lede)</p>
        </header>
    })
}

/// An external link to a page of the smart-commit documentation.
#[component]
pub async fn doc_link(page: &str, #[default] child: Child<'_>) -> Result<impl View> {
    let url = docs_url(page);
    Ok(view! { <a href=(url)>(child)</a> })
}

/// The documentation layout: a side list of the pages next to the page itself.
/// On narrow screens the list becomes a strip of tabs above the page.
#[component]
pub async fn docs(current: &str, #[default] child: Child<'_>) -> Result<impl View> {
    let cur = |path: &str| (current == path).then_some("page");
    let (tutorial, install, devices, how, roadmap) = (
        cur("/tutorial"),
        cur("/install"),
        cur("/devices"),
        cur("/how-it-works"),
        cur("/roadmap"),
    );
    Ok(view! {
        <div class="docs">
            <nav class="docs-nav" aria-label="Documentation">
                <p class="docs-group">"Start"</p>
                <ul>
                    <li><a href="/tutorial" aria-current=(tutorial)>"Tutorial"</a></li>
                    <li><a href="/install" aria-current=(install)>"Install"</a></li>
                </ul>
                <p class="docs-group">"Reference"</p>
                <ul>
                    <li><a href="/devices" aria-current=(devices)>"Devices"</a></li>
                    <li><a href="/how-it-works" aria-current=(how)>"How it works"</a></li>
                </ul>
                <p class="docs-group">"Project"</p>
                <ul>
                    <li><a href="/roadmap" aria-current=(roadmap)>"Roadmap"</a></li>
                </ul>
            </nav>
            <article class="docs-body">(child)</article>
        </div>
    })
}

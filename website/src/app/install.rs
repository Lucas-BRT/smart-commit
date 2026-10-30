use topcoat::{
    Result,
    router::{href, page},
    view::{View, view},
};

use crate::ui::{INSTALL_CMD, doc_link, docs, page_head, terminal};

#[page]
pub async fn page() -> Result<impl View> {
    Ok(view! {
        docs(current: "/install",
        page_head(
            title: "Install and use",
            lede: "Linux on x86_64. The installer does not need sudo; the setup asks before any step that does.",
        )

        <section class="rail" aria-labelledby="install">
            <h2 id="install">"Install"</h2>
            terminal(title: "install", label: "Install command",
                <span class="p">"$ "</span>(INSTALL_CMD)
            )
            <p>"It lists your hardware, asks before anything that needs " <code>"sudo"</code> ", downloads the model and ends with a benchmark that picks the fastest device."</p>
            <p>"For a walkthrough, see the " <a href=(href!(super::tutorial::page))>"tutorial"</a> "."</p>
            <p class="dim">"Drivers are installed with " <code>"apt"</code> "; Ubuntu 24.04 and derivatives are tested."</p>
        </section>

        <section class="rail" aria-labelledby="use">
            <h2 id="use">"Use"</h2>
            terminal(title: "everyday use", label: "Everyday commands",
"git add -p
"<span class="dim">"# print the message"</span>"
smart-commit
"<span class="dim">"# review it, then commit"</span>"
smart-commit --commit"
            )
            <p>"The message follows the style of your history."</p>
        </section>

        <section class="rail" aria-labelledby="hook">
            <h2 id="hook">"On every commit"</h2>
            terminal(title: "hook", label: "Install the git hook",
"smart-commit hook install
"<span class="dim">"# the editor opens with the message written"</span>"
git commit"
            )
            <p>"It works from any git client, and never blocks a commit."</p>
        </section>

        <section class="rail" aria-labelledby="more">
            <h2 id="more">"More"</h2>
            <p>"Devices, models, settings, troubleshooting and uninstalling are in the " doc_link(page: "usage.md", "usage guide") "."</p>
        </section>
        )
    })
}

use topcoat::{
    Result,
    router::{href, page},
    view::{View, view},
};

use crate::ui::{INSTALL_CMD, doc_link, docs, page_head, terminal};

#[page]
pub async fn page() -> Result<impl View> {
    Ok(view! {
        docs(current: "/tutorial",
            page_head(
                    title: "Tutorial: your first commit",
                lede: "Install, stage, review and commit. About five minutes.",
            )

            <section class="rail" aria-labelledby="need">
                <h2 id="need">"Requirements"</h2>
                <p>"Linux on x86_64, and a git repository with some history, so there is a style to learn."</p>
            </section>

            <section class="rail" aria-labelledby="install">
                <h2 id="install">"1. Install"</h2>
                terminal(title: "install", label: "Install command",
                    <span class="p">"$ "</span>(INSTALL_CMD)
                )
                <p>"The setup lists your hardware, asks before anything that needs " <code>"sudo"</code> ", downloads the model and ends with a benchmark. When a device shows " <code>"<- selected"</code> ", you are ready."</p>
            </section>

            <section class="rail" aria-labelledby="stage">
                <h2 id="stage">"2. Stage your changes"</h2>
                terminal(title: "stage", label: "Staging changes",
"git add src/retry.rs
"<span class="dim">"# or, hunk by hunk"</span>"
git add -p"
                )
                <p>"Use what you already use. " <code>"-p"</code> " is handy to keep each commit focused, but it is not required."</p>
                <p>"smart-commit only reads what is staged. With nothing staged it stops with an error."</p>
            </section>

            <section class="rail" aria-labelledby="preview">
                <h2 id="preview">"3. Preview the message " <span class="dim">"(optional)"</span></h2>
                terminal(title: "preview", label: "Print the suggested message",
                    <span class="p">"$ "</span>"smart-commit\n"
                    <span class="out">"feat(retry): add a retry loop"</span>
                )
                <p>"This only prints a suggestion, in the style of your history. It does not commit anything, so you can skip it."</p>
            </section>

            <section class="rail" aria-labelledby="commit">
                <h2 id="commit">"4. Review and commit"</h2>
                terminal(title: "smart-commit --commit", label: "Example review screen of smart-commit --commit",
    <span class="box">"┌ Commit message ──────────────────────┐
│feat(retry): add a retry loop         │
│                                      │
│Retries failed requests three times.  │
└──────────────────────────────────────┘
 qwen2.5-coder-1.5b on cpu · 1.6 s
┌ Staged: 2 files ─────────────────────┐
│ M src/retry.rs                       │
│ R src/http.rs -> src/net.rs          │
└──────────────────────────────────────┘"</span>"
"<span class="out">"[Enter] commit  [e] edit, then commit  [a] another message  [q] cancel"</span>
                )
                <p>"Nothing is committed until you press a key."</p>
                <ul>
                    <li><code>"Enter"</code> " commits with this message."</li>
                    <li><code>"e"</code> " lets you edit it first."</li>
                    <li><code>"a"</code> " asks for another message."</li>
                    <li><code>"q"</code> " cancels. You can also ignore the suggestion and write your own with " <code>"git commit"</code> "."</li>
                </ul>
            </section>

            <section class="rail" aria-labelledby="hook">
                <h2 id="hook">"5. Make it automatic " <span class="dim">"(optional)"</span></h2>
                terminal(title: "hook", label: "Install the git hook",
"smart-commit hook install
"<span class="dim">"# the editor opens with the message written"</span>"
git commit"
                )
                <p>"Edit it or replace it. The hook only acts on a plain " <code>"git commit"</code> ", and never blocks one. " <code>"smart-commit hook uninstall"</code> " removes it."</p>
            </section>

            <section class="rail" aria-labelledby="next">
                <h2 id="next">"Next"</h2>
                <p>
                    <a href=(href!(super::devices::page))>"Devices"</a>
                    " · "
                    <a href=(href!(super::how_it_works::page))>"How it works"</a>
                    " · the "
                    doc_link(page: "usage.md", "usage guide")
                </p>
            </section>
            )
    })
}

use topcoat::{
    Result,
    view::{View, component, view},
};

use crate::{
    components::kbd::kbd,
    ui::{
        docs::doc_section,
        terminal::{boxed, message, terminal},
    },
};

#[component]
pub async fn section() -> Result<impl View> {
    Ok(view! {
        doc_section(
            id: "commit",
            title: "4. Review and commit",
            terminal(
                title: "smart-commit --commit",
                label: "Example review screen of smart-commit --commit",
                boxed(
                    "┌ Commit message ──────────────────────┐
│feat(retry): add a retry loop         │
│                                      │
│Retries failed requests three times.  │
└──────────────────────────────────────┘
 qwen2.5-coder-1.5b on cpu · 1.6 s · from the model
┌ Staged: 2 files ─────────────────────┐
│ M src/retry.rs                       │
│ R src/http.rs -> src/net.rs          │
└──────────────────────────────────────┘"
                )
                "\n"
                message(
                    "[Enter] commit  [e] edit, then commit  [a] another message  [q] cancel"
                )
            )
            <p>"Nothing is committed until you press a key:"</p>
            <ul>
                <li>
                    kbd("Enter")
                    " commits."
                </li>
                <li>
                    kbd("e")
                    " edits the message, then commits."
                </li>
                <li>
                    kbd("a")
                    " asks for another message."
                </li>
                <li>
                    kbd("q")
                    " cancels."
                </li>
            </ul>
        )
    })
}

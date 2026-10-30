use topcoat::{
    Result,
    view::{View, component, view},
};

use crate::ui::{
    docs::doc_section,
    terminal::{note, terminal},
};

#[component]
pub async fn section() -> Result<impl View> {
    Ok(view! {
        doc_section(
            id: "use",
            title: "Use",
            terminal(
                title: "everyday use",
                label: "Everyday commands",
                "git add -p\n"
                note("print the message")
                "smart-commit\n"
                note("review it, then commit")
                "smart-commit --commit"
            )
            <p>"The message follows the style of your history."</p>
        )
        doc_section(
            id: "hook",
            title: "On every commit",
            terminal(
                title: "hook",
                label: "Install the git hook",
                "smart-commit hook install\n"
                note("the editor opens with the message written")
                "git commit"
            )
            <p>
                "It works from any git client and never blocks a commit."
            </p>
        )
    })
}

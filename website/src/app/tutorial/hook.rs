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
            id: "hook",
            title: "5. Make it automatic",
            note: "(optional)",
            terminal(
                title: "hook",
                label: "Install the git hook",
                "smart-commit hook install\n"
                note("the editor opens with the message written")
                "git commit"
            )
            <p>
                "Edit the message as usual. " <code>"smart-commit hook uninstall"</code> " removes the hook."
            </p>
        )
    })
}

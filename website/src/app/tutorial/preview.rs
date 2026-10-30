use topcoat::{
    Result,
    view::{View, component, view},
};

use crate::ui::{
    docs::doc_section,
    terminal::{cmd, message, terminal},
};

#[component]
pub async fn section() -> Result<impl View> {
    Ok(view! {
        doc_section(
            id: "preview",
            title: "3. Preview the message",
            note: "(optional)",
            terminal(
                title: "preview",
                label: "Print the suggested message",
                cmd("smart-commit")
                message("feat(retry): add a retry loop")
            )
            <p>"This only prints a suggestion; nothing is committed."</p>
        )
    })
}

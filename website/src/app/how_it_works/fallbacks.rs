use topcoat::{
    Result,
    view::{View, component, view},
};

use crate::ui::{doc_link::doc_link, docs::doc_section};

#[component]
pub async fn section() -> Result<impl View> {
    Ok(view! {
        doc_section(
            id: "safe",
            title: "Failures and fallbacks",
            <p>
                "A bad reply gets one retry, then a message built from the file list. Merges, reverts and cherry-picks keep git's message. A hook error is a warning, and the commit goes on."
            </p>
            <p>
                "The full description is in the "
                doc_link(page: "architecture.md", "architecture guide")
                "."
            </p>
        )
    })
}

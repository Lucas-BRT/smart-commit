use topcoat::{
    Result,
    view::{View, component, view},
};

use crate::ui::{doc_link::doc_link, docs::doc_section};

#[component]
pub async fn section() -> Result<impl View> {
    Ok(view! {
        doc_section(
            id: "more",
            title: "More",
            <p>
                "Everything else is in the " doc_link(page: "usage.md", "usage guide") "."
            </p>
        )
    })
}

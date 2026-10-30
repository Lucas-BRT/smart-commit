use topcoat::{
    Result,
    view::{View, component, view},
};

use crate::ui::docs::doc_section;

#[component]
pub async fn section() -> Result<impl View> {
    Ok(view! {
        doc_section(
            id: "need",
            title: "Requirements",
            <p>"Linux on x86_64 and a git repository with some history."</p>
        )
    })
}

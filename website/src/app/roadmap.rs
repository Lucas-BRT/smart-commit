//! The roadmap: what comes next, from the product's own roadmap.

use topcoat::{
    Result,
    router::page,
    view::{View, view},
};

use crate::{
    facts::NEXT,
    ui::{
        doc_link::doc_link,
        docs::{doc_section, docs},
    },
};

#[page]
pub async fn page() -> Result<impl View> {
    Ok(view! {
        docs(
            title: "Roadmap",
            lede: "Next items, highest priority first.",
            doc_section(
                id: "next",
                title: "Next",
                <ol>
                    for item in NEXT {
                        <li>(*item)</li>
                    }
                </ol>
                <p>
                    "The full list is in the " doc_link(page: "roadmap.md", "repository") "."
                </p>
            )
        )
    })
}

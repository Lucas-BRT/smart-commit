//! How it works: the pipeline, and what happens when it fails.

mod fallbacks;
mod pipeline;

use topcoat::{
    Result,
    router::page,
    view::{View, view},
};

use crate::ui::docs::docs;

#[page]
pub async fn page() -> Result<impl View> {
    Ok(view! {
        docs(
            title: "How it works",
            lede: "From your history and staged diff to one message.",
            pipeline::section()
            fallbacks::section()
        )
    })
}

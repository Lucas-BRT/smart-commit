use topcoat::{
    Result,
    view::{View, component, view},
};

use crate::ui::{
    docs::doc_section,
    steps::{step, steps},
};

#[component]
pub async fn section() -> Result<impl View> {
    Ok(view! {
        doc_section(
            id: "pipeline",
            title: "The pipeline",
            steps(
                step(
                    name: "Read git",
                    "The log, the branch and " <code>"git diff --staged"</code> "."
                )
                step(
                    name: "Infer the style",
                    "Convention, scopes and ticket format, from your commit subjects. Messages are in English."
                )
                step(
                    name: "Compact the diff",
                    "Files are fitted into a token budget."
                )
                step(
                    name: "Build the prompt",
                    "A system message and a user message."
                )
                step(
                    name: "Ask the model",
                    "On the chosen device, with greedy decoding."
                )
                step(
                    name: "Sanitize",
                    "The reply is cleaned up; bad replies are rejected."
                )
            )
        )
    })
}

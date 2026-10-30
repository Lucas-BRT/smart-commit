use topcoat::{
    Result,
    view::{View, attributes, component, view},
};

use crate::{
    measures::CONFIDENCE,
    ui::{
        chart::{Tone, bar, chart},
        doc_link::doc_link,
        docs::doc_section,
    },
};

#[component]
pub async fn section() -> Result<impl View> {
    // How often the type is right, by confidence: the share as a number to draw.
    let bands: Vec<_> = CONFIDENCE
        .iter()
        .map(|band| {
            (
                format!("{} ({} commits)", band.range, band.commits),
                band.right.parse().unwrap_or(0.0),
                band.right,
                if band.range == "90% or more" {
                    Tone::Accent
                } else {
                    Tone::Quiet
                },
            )
        })
        .collect();

    Ok(view! {
        doc_section(
            id: "ask-type",
            title: "Asking for the type",
            <p>
                "The more confident the model is in the type, the more often it is right. Overall it matched the author 0.700 of the time, on 250 commits:"
            </p>
            chart(
                label: "How often the type is right, by the model's confidence",
                attrs: attributes! { class="gap-2" },
                for (name, share, right, tone) in &bands {
                    bar(name: name, value: *share, max: 1.0, text: *right, tone: *tone)
                }
            )
            <p class="mt-6">
                <code>"model.ask_type"</code> " costs one more reading of the prompt per commit. More in the "
                doc_link(page: "measurements.md", "measurements document")
                "."
            </p>
        )
    })
}

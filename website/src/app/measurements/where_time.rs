use topcoat::{
    Result,
    view::{View, attributes, component, view},
};

use crate::{
    measures::{CORE_ULTRA, WHERE, number, plain},
    ui::{
        chart::{Part, Tone, chart, legend, stacked, swatch},
        docs::doc_section,
    },
};

#[component]
pub async fn section() -> Result<impl View> {
    let longest = WHERE
        .iter()
        .map(|row| number(row.total))
        .fold(0.0, f64::max);
    let configurations: Vec<_> = WHERE
        .iter()
        .map(|row| {
            let parts = [
                Part {
                    amount: number(row.load),
                    tone: Tone::Quiet,
                },
                Part {
                    amount: number(row.prefill),
                    tone: Tone::Accent,
                },
                Part {
                    amount: number(row.decode),
                    tone: Tone::Ink,
                },
            ];
            (plain(row.configuration), format!("{}ms", row.total), parts)
        })
        .collect();

    Ok(view! {
        doc_section(
            id: "where",
            title: "Where the time goes",
            <p>
                "Reading the prompt dominates: about 90% of a message on the CPU. Milliseconds per message on a " (CORE_ULTRA) ", over 6 real prompts:"
            </p>
            chart(
                label: "Where the time goes, per configuration",
                attrs: attributes! { class="gap-4" },
                legend(
                    swatch(tone: Tone::Quiet, label: "Load")
                    swatch(tone: Tone::Accent, label: "Prefill")
                    swatch(tone: Tone::Ink, label: "Decode")
                )
                for (name, total, parts) in &configurations {
                    stacked(caption: name, total: total, max: longest, parts: parts)
                }
            )
            <ul class="mt-6">
                <li>
                    "One thread per core halves the prefill, so smart-commit sets " <code>"model.threads_batch"</code> "."
                </li>
                <li>
                    "Halving " <code>"model.max_prompt_tokens"</code> " halves the prefill, at a cost in quality."
                </li>
                <li>
                    "No gain from flash attention, weight repacking or speculative decoding."
                </li>
            </ul>
        )
    })
}

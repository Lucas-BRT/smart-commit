use topcoat::{
    Result,
    view::{View, attributes, component, view},
};

use crate::{
    measures::{BENCH_RYZEN, secs},
    ui::{
        chart::{Tone, bar, bar_group, chart, legend, swatch},
        docs::doc_section,
    },
};

#[component]
pub async fn section() -> Result<impl View> {
    let slowest = BENCH_RYZEN
        .iter()
        .map(|run| secs(run.without_fast_load))
        .fold(0.0, f64::max);

    Ok(view! {
        doc_section(
            id: "fast",
            title: "Fast load",
            <p>
                "Two steps of starting " <code>"llama-server"</code> " are rarely needed for a 1 GB model: repacking the weights and fitting them to GPU memory. " <code>"smart-commit bench"</code> " keeps whichever is faster."
            </p>
            chart(
                label: "Per message without and with fast load",
                attrs: attributes! { class="gap-5" },
                legend(
                    swatch(tone: Tone::Quiet, label: "Without fast load")
                    swatch(tone: Tone::Accent, label: "With fast load")
                )
                for run in BENCH_RYZEN {
                    bar_group(
                        bar(
                            name: run.device,
                            value: secs(run.without_fast_load),
                            max: slowest,
                            text: run.without_fast_load
                        )
                        bar(
                            name: "",
                            value: secs(run.total),
                            max: slowest,
                            text: run.total,
                            tone: Tone::Accent
                        )
                    )
                }
            )
        )
    })
}

use topcoat::{
    Result,
    view::{View, attributes, component, view},
};

use crate::{
    measures::{BY_MODEL, RYZEN, plain, secs},
    ui::{
        chart::{Tone, bar, bar_group, chart, legend, swatch},
        docs::doc_section,
    },
};

#[component]
pub async fn section() -> Result<impl View> {
    let slowest = BY_MODEL.iter().map(|row| secs(row.cpu)).fold(0.0, f64::max);
    let models: Vec<_> = BY_MODEL
        .iter()
        .map(|row| (plain(row.model), row.cpu, row.vulkan))
        .collect();

    Ok(view! {
        doc_section(
            id: "by-model",
            title: "By model",
            <p>
                "Per message on the " (RYZEN) ". The fine-tunes write shorter replies, so they are faster."
            </p>
            chart(
                label: "Per message by model, on the CPU and on Vulkan",
                attrs: attributes! { class="gap-5" },
                legend(
                    swatch(tone: Tone::Quiet, label: "cpu")
                    swatch(tone: Tone::Accent, label: "vulkan")
                )
                for (name, cpu, vulkan) in &models {
                    bar_group(
                        bar(name: name, value: secs(cpu), max: slowest, text: *cpu)
                        bar(
                            name: "",
                            value: secs(vulkan),
                            max: slowest,
                            text: *vulkan,
                            tone: Tone::Accent
                        )
                    )
                }
            )
            <p class="mt-6">
                "The 7B model spends 7.8s of its 8.3s copying 4.4 GB to the GPU. Kept loaded, the next commit takes 0.57s."
            </p>
        )
    })
}

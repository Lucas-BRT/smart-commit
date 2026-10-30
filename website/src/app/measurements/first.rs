use topcoat::{
    Result,
    view::{View, attributes, component, view},
};

use super::costs;
use crate::{
    measures::{KEPT, RYZEN, default_model_on, five_commits, plain, secs},
    ui::{
        chart::{Part, Tone, bar, bar_group, chart, legend, stacked, swatch},
        docs::doc_section,
    },
};

#[component]
pub async fn section() -> Result<impl View> {
    let longest = KEPT.iter().map(|row| secs(row.cold)).fold(0.0, f64::max);
    let models: Vec<_> = KEPT
        .iter()
        .map(|row| (plain(row.label), row.cold, row.kept))
        .collect();

    // Five commits in a row on the default model, on vulkan and on cpu: each
    // device twice, loading every time and with the model kept loaded.
    let widest = secs(default_model_on("vulkan").cold) * 5.0;
    let commits: Vec<_> = ["vulkan", "cpu"]
        .into_iter()
        .flat_map(|device| {
            let row = default_model_on(device);
            let (always, once) = five_commits(secs(row.cold), secs(row.kept));
            [
                (format!("{device}, load every commit"), always, Tone::Quiet),
                (format!("{device}, model kept loaded"), once, Tone::Accent),
            ]
        })
        .map(|(caption, times, tone)| {
            let total = format!("{:.1}s in total", times.iter().sum::<f64>());
            (caption, total, times.map(|amount| Part { amount, tone }))
        })
        .collect();

    Ok(view! {
        doc_section(
            id: "first",
            title: "The first message and the next ones",
            <p>
                "Most of a commit is loading the model. Kept loaded (" <code>"model.keep_loaded"</code> ", on by default), only the first commit after a pause pays for it."
            </p>
            <p>
                "End to end through the CLI, on a "
                (RYZEN)
                ":"
            </p>
            chart(
                label: "Per message: load every commit against the model kept loaded",
                attrs: attributes! { class="gap-5" },
                legend(
                    swatch(tone: Tone::Quiet, label: "Load every commit")
                    swatch(
                        tone: Tone::Accent,
                        label: "Kept loaded, from the second commit"
                    )
                )
                for (name, cold, kept) in &models {
                    bar_group(
                        bar(name: name, value: secs(cold), max: longest, text: *cold)
                        bar(
                            name: "",
                            value: secs(kept),
                            max: longest,
                            text: *kept,
                            tone: Tone::Accent
                        )
                    )
                }
            )
            <p class="mt-6">
                "A GPU spends most of the time loading, so it gains the most. The CPU spends it reading the prompt."
            </p>

            <h3 class="mt-10">"Five commits in a row"</h3>
            <p class="mt-3">
                <code>"qwen2.5-coder-1.5b"</code> ", five commits in a row. The first one starts the server (an estimate)."
            </p>
            chart(
                label: "Five commits in a row, with and without the model kept loaded",
                attrs: attributes! { class="mt-5" },
                for (caption, total, parts) in &commits {
                    stacked(
                        caption: caption,
                        total: total,
                        max: widest,
                        parts: parts,
                        apart: true
                    )
                }
            )
            costs::costs()
        )
    })
}

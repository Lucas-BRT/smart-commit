use topcoat::{
    Result,
    view::{View, attributes, component, view},
};

use crate::{
    measures::{BENCH_RYZEN, BY_DEVICE, CORE_ULTRA, RYZEN, secs},
    ui::{
        chart::{Part, Tone, bar, chart, legend, stacked, swatch},
        docs::doc_section,
    },
};

#[component]
pub async fn section() -> Result<impl View> {
    let slowest = BY_DEVICE
        .iter()
        .map(|row| secs(row.time))
        .fold(0.0, f64::max);
    let slowest_bench = BENCH_RYZEN
        .iter()
        .map(|run| secs(run.total))
        .fold(0.0, f64::max);
    let runs: Vec<_> = BENCH_RYZEN
        .iter()
        .map(|run| {
            let parts = [
                Part {
                    amount: secs(run.load),
                    tone: Tone::Quiet,
                },
                Part {
                    amount: secs(run.generate),
                    tone: Tone::Accent,
                },
            ];
            (run, parts)
        })
        .collect();

    Ok(view! {
        doc_section(
            id: "by-device",
            title: "By device",
            <p>
                "On a " (CORE_ULTRA) ", per message, end to end. Here " <code>"openvino-cpu"</code> " was still called " <code>"cpu"</code> "."
            </p>
            chart(
                label: "Per message by device on the Core Ultra 5 225H",
                attrs: attributes! { class="gap-2" },
                for row in BY_DEVICE {
                    bar(
                        name: row.device,
                        value: secs(row.time),
                        max: slowest,
                        text: row.time,
                        tone: if row.device == "intel-gpu" {
                            Tone::Accent
                        } else {
                            Tone::Quiet
                        }
                    )
                }
            )
            <p class="mt-6">
                "The integrated GPU is the fastest. The NPU is the slowest but uses far less CPU."
            </p>

            <p class="mt-8">
                "On a "
                (RYZEN)
                ", "
                <code>"smart-commit bench"</code>
                " on four prompts sized like real ones:"
            </p>
            chart(
                label: "Per message on the Ryzen 5 5500, load and generation",
                attrs: attributes! { class="mt-5" },
                legend(
                    swatch(tone: Tone::Quiet, label: "Load")
                    swatch(tone: Tone::Accent, label: "Generate")
                )
                for (run, parts) in &runs {
                    stacked(
                        caption: run.device,
                        total: run.total,
                        max: slowest_bench,
                        parts: parts,
                        <p class="m-0 mt-1 text-sm text-muted-foreground">
                            "Load "
                            (run.load)
                            ", generate "
                            (run.generate)
                            "."
                        </p>
                    )
                }
            )
            <p class="mt-5">
                "Most of the Vulkan load is copying the weights to the GPU."
            </p>
        )
    })
}

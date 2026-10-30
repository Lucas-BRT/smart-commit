use topcoat::{
    Result,
    router::href,
    view::{View, component, view},
};

use crate::{
    app::measurements,
    facts::{BENCH, BENCH_MACHINE},
    ui::{
        doc_link::doc_link,
        docs::doc_section,
        terminal::{selected, terminal},
    },
};

#[component]
pub async fn section() -> Result<impl View> {
    Ok(view! {
        doc_section(
            id: "choosing",
            title: "Choosing a device",
            <p>
                "Setup measures every device your machine has. On a "
                (BENCH_MACHINE)
                ":"
            </p>
            terminal(
                title: "smart-commit bench (abridged)",
                label: "Example output of smart-commit bench",
                aligned: true,
                "  device   per message\n"
                for line in BENCH {
                    // The columns are padded to the width of the longest name.
                    (format!("  {:<15}{}", line.device, line.time))
                    if line.selected {
                        "  "
                        selected("<- selected")
                    }
                    "\n"
                }
            )
            <p>
                "Run "
                <code>"smart-commit bench"</code>
                " again after changing drivers. Force a device with "
                <code>"smart-commit device use npu"</code>
                "."
            </p>
            <p>
                "More in the " <a href=(href!(measurements::page))>"measurements"</a> " and the " doc_link(page: "devices.md", "devices guide") "."
            </p>
        )
    })
}

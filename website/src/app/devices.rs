use topcoat::{
    Result,
    router::page,
    view::{View, view},
};

use crate::ui::{doc_link, docs, facts::DEVICES, page_head, table, terminal};

#[page]
pub async fn page() -> Result<impl View> {
    Ok(view! {
        docs(current: "/devices",
        page_head(
            title: "Devices",
            lede: "Backends, supported hardware and how a device is chosen.",
        )

        <section class="rail" aria-labelledby="table">
            <h2 id="table">"Devices"</h2>
            table(label: "Devices, backends and hardware",
                <thead><tr><th scope="col">"Device"</th><th scope="col">"Backend"</th><th scope="col">"Hardware"</th></tr></thead>
                <tbody>
                    for [device, backend, hardware] in DEVICES {
                        <tr><td data-label="Device"><code>(*device)</code></td><td data-label="Backend">(*backend)</td><td data-label="Hardware">(*hardware)</td></tr>
                    }
                </tbody>
            )
            <p>"OpenVINO is only needed for Intel's NPU and GPUs. AMD and NVIDIA go through llama.cpp."</p>
        </section>

        <section class="rail" aria-labelledby="choosing">
            <h2 id="choosing">"Choosing a device"</h2>
            <p>"Setup measures every device your machine has. On a Ryzen 5 5500 with an RTX 3050:"</p>
            terminal(title: "smart-commit bench (abridged)", label: "Example output of smart-commit bench",
"  device   per message
  vulkan         2.4 s  "<span class="sel">"<- selected"</span>"
  cpu            4.0 s"
            )
            <p>"Run " <code>"smart-commit bench"</code> " again after changing drivers. Force a device with " <code>"smart-commit device use npu"</code> "."</p>
            <p>"More numbers are in the " doc_link(page: "measurements.md", "measurements") " and the " doc_link(page: "devices.md", "devices guide") "."</p>
        </section>
        )
    })
}

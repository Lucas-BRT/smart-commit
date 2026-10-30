use topcoat::{
    Result,
    view::{View, component, view},
};

use crate::{
    facts::DEVICES,
    ui::{
        data_table::{cell, data_table, row},
        docs::doc_section,
    },
};

#[component]
pub async fn section() -> Result<impl View> {
    Ok(view! {
        doc_section(
            id: "table",
            title: "Devices",
            data_table(
                label: "Devices, backends and hardware",
                head: &["Device", "Backend", "Hardware"],
                for device in DEVICES {
                    row(
                        cell(label: "Device", <code>(device.name)</code>)
                        cell(label: "Backend", (device.backend))
                        cell(label: "Hardware", (device.hardware))
                    )
                }
            )
            <p>
                "OpenVINO is only needed for Intel's NPU and GPUs. AMD and NVIDIA go through llama.cpp."
            </p>
        )
    })
}

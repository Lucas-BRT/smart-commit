use topcoat::{
    Result,
    view::{View, component, view},
};

use crate::ui::{doc_link::doc_link, docs::doc_section};

#[component]
pub async fn section() -> Result<impl View> {
    Ok(view! {
        doc_section(
            id: "method",
            title: "Machines and method",
            <ul>
                <li>
                    "Ryzen 5 5500 (6 cores), GeForce RTX 3050 8 GB with the proprietary driver 595.104, 23 GiB RAM, Pop!_OS 24.04, kernel 7.2."
                </li>
                <li>
                    "Core Ultra 5 225H (Arrow Lake-H: Arc iGPU, NPU 37xx), 15 GiB RAM, Pop!_OS 24.04, kernel 7.2."
                </li>
                <li>
                    "Your times will differ: " <code>"smart-commit bench"</code> " measures your machine."
                </li>
            </ul>
            <p>
                "How each number was measured, and how to repeat it, is in the "
                doc_link(page: "measurements.md", "measurements document")
                "."
            </p>
        )
    })
}

//! Devices: the backends, the hardware, and how one is chosen.

mod choosing;
mod table;

use topcoat::{
    Result,
    router::page,
    view::{View, view},
};

use crate::ui::docs::docs;

#[page]
pub async fn page() -> Result<impl View> {
    Ok(view! {
        docs(
            title: "Devices",
            lede: "The hardware it runs on, and how one is chosen.",
            table::section()
            choosing::section()
        )
    })
}

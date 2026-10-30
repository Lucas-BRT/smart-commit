//! Measurements: how long a message takes, drawn from the product's own
//! measurements document.

mod ask_type;
mod by_device;
mod by_model;
mod costs;
mod fast_load;
mod first;
mod method;
mod where_time;

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
            title: "Measurements",
            lede: "How long a message takes, and why the first one is slower.",
            first::section()
            by_device::section()
            by_model::section()
            where_time::section()
            fast_load::section()
            ask_type::section()
            method::section()
        )
    })
}

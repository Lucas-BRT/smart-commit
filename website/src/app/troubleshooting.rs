//! Troubleshooting: checking an installation, reading a message, reporting one.

mod doctor;
mod messages;
mod report;

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
            title: "Troubleshooting",
            lede: "Check an installation and fix common errors.",
            doctor::section()
            messages::section()
            report::section()
        )
    })
}

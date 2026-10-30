//! Settings and models: changing a setting, and choosing a model.

mod ask_type;
mod config;
mod loaded;
mod models;
mod update_notice;

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
            title: "Settings and models",
            lede: "Change settings and choose a model.",
            config::section()
            loaded::section()
            ask_type::section()
            update_notice::section()
            models::section()
        )
    })
}

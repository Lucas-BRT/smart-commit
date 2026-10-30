//! Install and use: the installer, its options, updating and removing.

mod everyday;
mod getting;
mod more;
mod options;
mod plan;
mod requirements;
mod uninstall;
mod update;
mod verify;

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
            title: "Install and use",
            lede: "Linux on x86_64. The installer needs no sudo.",
            getting::section()
            requirements::section()
            plan::section()
            options::section()
            verify::section()
            everyday::section()
            update::section()
            uninstall::section()
            more::section()
        )
    })
}

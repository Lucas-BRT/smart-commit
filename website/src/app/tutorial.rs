//! The tutorial: install, stage, review and commit.

mod debug;
mod hook;
mod install;
mod preview;
mod requirements;
mod review;
mod stage;

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
            title: "Tutorial: your first commit",
            lede: "Install, stage, review and commit.",
            requirements::section()
            install::section()
            stage::section()
            preview::section()
            review::section()
            hook::section()
            debug::section()
        )
    })
}

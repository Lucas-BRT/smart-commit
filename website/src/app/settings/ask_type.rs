use topcoat::{
    Result,
    router::href,
    view::{View, component, view},
};

use crate::{app::measurements, ui::docs::doc_section};

#[component]
pub async fn section() -> Result<impl View> {
    Ok(view! {
        doc_section(
            id: "ask-type",
            title: "Asking for the type",
            <p>
                "With " <code>"smart-commit config set model.ask_type low_confidence"</code> ", " <code>"--commit"</code> " asks for the type when the model is unsure."
            </p>
            <p>
                "Off by default. It works for Conventional Commits, on " <code>"vulkan"</code> " and " <code>"cpu"</code> ", in a terminal. See the "
                <a href=(href!(measurements::page).fragment("ask-type"))>
                    "measurements"
                </a>
                "."
            </p>
        )
    })
}

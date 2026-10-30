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
            id: "keep",
            title: "Keeping the model loaded",
            <p>
                "On " <code>"vulkan"</code> " and " <code>"cpu"</code> " the model stays loaded for 10 idle minutes. Change it with " <code>"model.keep_loaded"</code> "; 0 turns it off. See the "
                <a href=(href!(measurements::page).fragment("first"))>
                    "measurements"
                </a>
                " for what it saves."
            </p>
        )
    })
}

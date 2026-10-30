use topcoat::{
    Result,
    router::href,
    view::{View, component, view},
};

use crate::{
    app::settings,
    ui::{docs::doc_section, terminal::terminal},
};

#[component]
pub async fn section() -> Result<impl View> {
    Ok(view! {
        doc_section(
            id: "update",
            title: "Update",
            terminal(
                title: "update",
                label: "Update smart-commit",
                "smart-commit update"
            )
            <p>
                "Installs the latest release. Models and settings stay. To be told about new releases, turn on the "
                <a href=(href!(settings::page).fragment("update-check"))>
                    "update notice"
                </a>
                "."
            </p>
        )
    })
}

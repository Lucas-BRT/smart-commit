use topcoat::{
    Result,
    view::{View, component, view},
};

use crate::{
    facts::INSTALL_CMD,
    ui::{
        docs::doc_section,
        terminal::{cmd, terminal},
    },
};

#[component]
pub async fn section() -> Result<impl View> {
    Ok(view! {
        doc_section(
            id: "plan",
            title: "See the plan first",
            <p>"To see what the setup would do first:"</p>
            terminal(
                title: "dry run",
                label: "Install without the setup, then print its plan",
                cmd(
                    (INSTALL_CMD)
                    " -s -- --no-setup"
                )
                cmd("smart-commit setup --dry-run")
            )
            <p>
                "It lists every step and changes nothing."
            </p>
        )
    })
}

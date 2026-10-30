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
            id: "install",
            title: "1. Install",
            terminal(title: "install", label: "Install command", cmd((INSTALL_CMD)))
            <p>
                "The setup asks before anything that needs "
                <code>"sudo"</code>
                ", downloads the model and benchmarks your devices. The best shows with "
                <code>"<- selected"</code>
                "."
            </p>
        )
    })
}

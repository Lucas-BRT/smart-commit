use topcoat::{
    Result,
    router::href,
    view::{View, component, view},
};

use crate::{
    app::tutorial,
    facts::{DEFAULT_MODEL, DEFAULT_MODEL_SIZE, INSTALL_CMD, VERSION},
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
            title: "Install",
            terminal(title: "install", label: "Install command", cmd((INSTALL_CMD)))
            <p>
                "This installs version " (VERSION) ". The setup asks before anything that needs " <code>"sudo"</code> "."
            </p>
            <p>
                "The default model is " <code>(DEFAULT_MODEL)</code> ", " (DEFAULT_MODEL_SIZE) "."
            </p>
            <p>
                "For a walkthrough, see the "
                <a href=(href!(tutorial::page))>"tutorial"</a>
                "."
            </p>
            <p class="text-muted-foreground">
                "Drivers are installed with "
                <code>"apt"</code>
                "; Ubuntu 24.04 and derivatives are tested."
            </p>
        )
    })
}

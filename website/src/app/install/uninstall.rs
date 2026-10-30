use topcoat::{
    Result,
    view::{View, component, view},
};

use crate::ui::{
    docs::doc_section,
    terminal::{note, terminal},
};

#[component]
pub async fn section() -> Result<impl View> {
    Ok(view! {
        doc_section(
            id: "uninstall",
            title: "Uninstall",
            terminal(
                title: "uninstall",
                label: "Uninstall smart-commit",
                "smart-commit uninstall\n"
                note(
                    "the binary, the models, llama.cpp, the benchmark, the caches and the config"
                )
                "smart-commit uninstall --system\n"
                note("and the OpenVINO the setup installed (asks before using sudo)")
            )
            <p>
                "It lists what it removes and asks first."
            </p>
            <p>
                "Drivers stay, since other programs may use them. The hook stays in each repository: run " <code>"smart-commit hook uninstall"</code> " first."
            </p>
        )
    })
}

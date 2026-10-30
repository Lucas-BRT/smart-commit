use topcoat::{
    Result,
    view::{View, component, view},
};

use crate::ui::{
    data_table::{cell, data_table, row},
    docs::doc_section,
};

#[component]
pub async fn section() -> Result<impl View> {
    Ok(view! {
        doc_section(
            id: "options",
            title: "Installer options",
            <p>
                "After "
                <code>"bash -s --"</code>
                " when piping:"
            </p>
            data_table(
                label: "Installer options",
                head: &["Option", "Effect"],
                row(
                    cell(label: "Option", <code>"--version 0.3.0"</code>)
                    cell(label: "Effect", "install that release instead of the latest")
                )
                row(
                    cell(label: "Option", <code>"--prefix DIR"</code>)
                    cell(
                        label: "Effect",
                        "install in "
                        <code>"DIR/bin"</code>
                        " and "
                        <code>"DIR/share"</code>
                    )
                )
                row(
                    cell(label: "Option", <code>"--no-setup"</code>)
                    cell(
                        label: "Effect",
                        "only install; run "
                        <code>"smart-commit setup"</code>
                        " later"
                    )
                )
                row(
                    cell(label: "Option", <code>"--update"</code>)
                    cell(
                        label: "Effect",
                        "install only if the release is not the version installed, without the setup"
                    )
                )
                row(
                    cell(label: "Option", <code>"--verify"</code>)
                    cell(
                        label: "Effect",
                        "also verify the download's build provenance with "
                        <code>"gh"</code>
                    )
                )
                row(
                    cell(label: "Option", <code>"--yes"</code>)
                    cell(label: "Effect", "answer yes to the setup's questions")
                )
                row(
                    cell(label: "Option", <code>"--uninstall"</code>)
                    cell(
                        label: "Effect",
                        "remove smart-commit (models, caches and config are kept)"
                    )
                )
                row(
                    cell(label: "Option", <code>"--purge"</code>)
                    cell(
                        label: "Effect",
                        "remove it, plus the models, llama.cpp, the benchmark, the caches and the config"
                    )
                )
                row(
                    cell(label: "Option", <code>"--system"</code>)
                    cell(
                        label: "Effect",
                        "remove it, plus the OpenVINO that the setup installed (asks before using "
                        <code>"sudo"</code>
                        ")"
                    )
                )
            )
            <p>
                "If " <code>"~/.local/bin"</code> " is not on your " <code>"PATH"</code> ", the installer prints the line to add."
            </p>
        )
    })
}

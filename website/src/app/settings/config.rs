use topcoat::{
    Result,
    view::{View, component, view},
};

use crate::ui::{
    data_table::{cell, data_table, row},
    doc_link::doc_link,
    docs::doc_section,
    terminal::{note, terminal},
};

#[component]
pub async fn section() -> Result<impl View> {
    Ok(view! {
        doc_section(
            id: "settings",
            title: "Settings",
            <p>
                "smart-commit works without configuration. Settings change the style, the device or the model."
            </p>
            terminal(
                title: "settings",
                label: "Commands to read and change settings",
                "smart-commit config list --show-origin\n"
                note("everything, and the layer each value comes from")
                "smart-commit config set model.device cpu\n"
                note("global")
                "smart-commit config set --repo style.convention conventional\n"
                note(".smart-commit.toml, shared through git")
                "smart-commit config get style.convention\n"
                "smart-commit config unset model.device\n"
                "smart-commit config init\n"
                note("a file with every key commented out")
            )
            <p>"A value comes from the strongest layer that sets it:"</p>
            <ol>
                <li>
                    "command-line flags ("
                    <code>"--backend"</code>
                    ", "
                    <code>"--device"</code>
                    ", "
                    <code>"--model-dir"</code>
                    ");"
                </li>
                <li>
                    "environment variables ("
                    <code>"SMART_COMMIT_*"</code>
                    ");"
                </li>
                <li>
                    "the repository's "
                    <code>".smart-commit.toml"</code>
                    ", at the root of the work tree;"
                </li>
                <li>
                    "the global "
                    <code>"~/.config/smart-commit/config.toml"</code>
                    ";"
                </li>
                <li>"the built-in defaults."</li>
            </ol>
            <h3 class="mt-8">"Common tasks"</h3>
            data_table(
                label: "Common settings",
                head: &["To", "Run"],
                row(
                    cell(label: "To", "enforce Conventional Commits for a team")
                    cell(
                        label: "Run",
                        <code>
                            "smart-commit config set --repo style.convention conventional"
                        </code>
                    )
                )
                row(
                    cell(
                        label: "To",
                        "get faster NPU replies that see less of the diff"
                    )
                    cell(
                        label: "Run",
                        <code>
                            "smart-commit config set model.max_prompt_tokens 512"
                        </code>
                    )
                )
                row(
                    cell(label: "To", "use another GPU on a multi-GPU machine")
                    cell(
                        label: "Run",
                        <code>"smart-commit config set model.vulkan_device 1"</code>
                    )
                )
                row(
                    cell(label: "To", "keep replies short")
                    cell(
                        label: "Run",
                        <code>"smart-commit config set model.max_new_tokens 64"</code>
                    )
                )
            )
            <p>
                "Every key is in the "
                doc_link(page: "configuration.md", "configuration guide")
                "."
            </p>
        )
    })
}

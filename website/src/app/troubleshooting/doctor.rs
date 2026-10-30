use topcoat::{
    Result,
    view::{View, component, view},
};

use crate::ui::{
    data_table::{cell, data_table, row},
    docs::doc_section,
    terminal::{note, terminal},
};

#[component]
pub async fn section() -> Result<impl View> {
    Ok(view! {
        doc_section(
            id: "doctor",
            title: "Check the installation",
            terminal(
                title: "doctor",
                label: "Check an installation",
                "smart-commit doctor\n"
                note("offline")
                "smart-commit doctor --thorough\n"
                note("also hash the model file (minutes for a big model)")
            )
            <p>
                "One line per check: " <code>"ok"</code> ", " <code>"warn"</code> " or " <code>"fail"</code> ", with the fix. It exits 1 if anything fails."
            </p>
            data_table(
                label: "What doctor checks",
                head: &["Check", "Looks at"],
                row(
                    cell(label: "Check", <code>"path"</code>)
                    cell(
                        label: "Looks at",
                        "the directory of the running binary is on "
                        <code>"PATH"</code>
                    )
                )
                row(
                    cell(label: "Check", <code>"git"</code>)
                    cell(
                        label: "Looks at",
                        "git runs and is 2.20 or newer"
                    )
                )
                row(
                    cell(label: "Check", <code>"config"</code>)
                    cell(
                        label: "Looks at",
                        "config files read fine"
                    )
                )
                row(
                    cell(label: "Check", <code>"model"</code>)
                    cell(
                        label: "Looks at",
                        "the model files exist and match"
                    )
                )
                row(
                    cell(label: "Check", <code>"llama.cpp"</code>)
                    cell(
                        label: "Looks at",
                        "llama-server is installed, and is the build this release pins"
                    )
                )
                row(
                    cell(label: "Check", <code>"device"</code>)
                    cell(
                        label: "Looks at",
                        "the device exists, and a benchmark was run"
                    )
                )
                row(
                    cell(label: "Check", <code>"server"</code>)
                    cell(
                        label: "Looks at",
                        "the models kept loaded"
                    )
                )
                row(
                    cell(label: "Check", <code>"hook"</code>)
                    cell(
                        label: "Looks at",
                        "the hook is installed and runs this binary"
                    )
                )
            )
            <p>"Example:"</p>
            terminal(
                title: "smart-commit doctor",
                label: "Example output of smart-commit doctor",
                "ok    path       /home/me/.local/bin is on PATH
ok    git        git 2.43
ok    config     no files: the defaults apply
ok    model      qwen2.5-coder-1.5b-instruct-q4_k_m.gguf: size and record match (--thorough hashes the file)
ok    llama.cpp  /home/me/.local/share/smart-commit/llama.cpp/llama-b11381/llama-server
ok    device     cpu (auto: no benchmark, the default order)
warn  device     no benchmark yet - run `smart-commit bench`
ok    server     none running: the next commit starts one
warn  hook       not installed in this repository - `smart-commit hook install`"
            )
        )
    })
}

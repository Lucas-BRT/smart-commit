use topcoat::{
    Result,
    router::href,
    view::{View, component, view},
};

use crate::{
    app::measurements,
    ui::{
        data_table::{cell, data_table, row},
        doc_link::doc_link,
        docs::doc_section,
        terminal::{note, terminal},
    },
};

/// How far a model has been checked: the default is the one the prompt was
/// tuned on, the rest load and answer.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Validated,
    Experimental,
}

impl Status {
    const fn name(self) -> &'static str {
        match self {
            Self::Validated => "validated",
            Self::Experimental => "experimental",
        }
    }
}

/// A recommended model, as `docs/models.md` lists it. Only the default has an
/// OpenVINO export, so only it runs on every device.
struct Model {
    id: &'static str,
    size: &'static str,
    status: Status,
    on_openvino: bool,
}

impl Model {
    const fn gguf(id: &'static str, size: &'static str) -> Self {
        Self {
            id,
            size,
            status: Status::Experimental,
            on_openvino: false,
        }
    }

    const fn devices(&self) -> &'static str {
        if self.on_openvino {
            "all"
        } else {
            "vulkan, cpu"
        }
    }
}

const MODELS: &[Model] = &[
    Model {
        id: "qwen2.5-coder-1.5b",
        size: "1.1 GB",
        status: Status::Validated,
        on_openvino: true,
    },
    Model::gguf("qwen2.5-coder-0.5b", "491 MB"),
    Model::gguf("qwen2.5-coder-7b", "4.7 GB"),
    Model::gguf("smart-commit-1.5b-v0.1", "986 MB"),
    Model::gguf("smart-commit-0.5b-v0.1", "397 MB"),
    Model::gguf("smart-commit-0.5b-v0.2", "397 MB"),
    Model::gguf("smart-commit-1.5b-v0.2", "986 MB"),
    Model::gguf("smart-commit-0.5b-v0.3", "397 MB"),
    Model::gguf("smart-commit-1.5b-v0.3", "986 MB"),
];

#[component]
pub async fn section() -> Result<impl View> {
    Ok(view! {
        doc_section(
            id: "models",
            title: "Models",
            <p>
                "smart-commit ships no model. " <code>"model use"</code> " downloads one from Hugging Face and checks it against a SHA-256. The project's own fine-tunes are on its " <a href="https://huggingface.co/LucasBRT">"Hugging Face account"</a> "."
            </p>
            terminal(
                title: "models",
                label: "Commands to choose a model",
                "smart-commit model\n"
                note("a picker, in a terminal")
                "smart-commit model list\n"
                note("the recommended models and those from disk, and which is in use")
                "smart-commit model use qwen2.5-coder-7b\n"
                note("download it from Hugging Face and use it")
                "smart-commit model use ~/models/my.gguf\n"
                note("a GGUF file from disk, unverified, remembered")
            )
            data_table(
                label: "Recommended models",
                head: &["Model", "Size", "Status", "Devices"],
                for model in MODELS {
                    row(
                        cell(label: "Model", <code>(model.id)</code>)
                        cell(label: "Size", (model.size))
                        cell(label: "Status", (model.status.name()))
                        cell(label: "Devices", (model.devices()))
                    )
                }
            )
            <ul>
                <li>
                    "The default is the only model the NPU and the Intel GPU can run."
                </li>
                <li>
                    "Experimental models are not fully tested."
                </li>
                <li>
                    <code>"smart-commit-*"</code> " models are fine-tuned for this prompt."
                </li>
                <li>
                    "After switching, run "
                    <code>"smart-commit bench"</code>
                    " again."
                </li>
            </ul>
            <p>
                "Times per model are in the "
                <a href=(href!(measurements::page).fragment("by-model"))>
                    "measurements"
                </a>
                ". Licences are in the "
                doc_link(page: "models.md", "models guide")
                "."
            </p>
        )
    })
}

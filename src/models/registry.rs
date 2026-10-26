use std::fmt;

use crate::prompt::PromptRules;

/// Where models are downloaded from.
pub const HUGGING_FACE: &str = "https://huggingface.co";

/// The Hugging Face account the project's own fine-tunes are published
/// under, one repository per model and version (AGENTS.md).
pub const OWN_ACCOUNT: &str = "LucasBRT";

/// The model used unless `model.name` says otherwise.
pub const DEFAULT: &str = "qwen2.5-coder-1.5b";

/// The ids of [`REGISTRY`], in its order (`model.name`'s choices).
pub const IDS: &[&str] = &[
    "qwen2.5-coder-1.5b",
    "qwen2.5-coder-0.5b",
    "qwen2.5-coder-7b",
    "smart-commit-1.5b-v0.1",
    "smart-commit-0.5b-v0.1",
    "smart-commit-0.5b-v0.2",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Checked on the fixture prompts and the hardware tests.
    Validated,
    /// Loads and answers; its messages have not been evaluated.
    Experimental,
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Validated => "validated",
            Self::Experimental => "experimental",
        })
    }
}

/// A GGUF file in a Hugging Face repository, for the llama.cpp devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gguf {
    pub repo: &'static str,
    /// A commit of the repository, never a branch.
    pub revision: &'static str,
    pub file: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

impl Gguf {
    pub fn url(&self, base: &str) -> String {
        format!(
            "{}/{}/resolve/{}/{}",
            base.trim_end_matches('/'),
            self.repo,
            self.revision,
            self.file
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Model {
    pub id: &'static str,
    pub name: &'static str,
    pub license: &'static str,
    pub status: Status,
    pub summary: &'static str,
    pub gguf: Gguf,
    /// The directory `scripts/export-model.sh` writes, for the OpenVINO
    /// devices; `None` when the model has no pinned export yet.
    pub openvino: Option<&'static str>,
    /// The instructions the model needs: [`PromptRules::Omitted`] only for
    /// a model trained without them (training H6).
    pub rules: PromptRules,
}

pub const REGISTRY: &[Model] = &[
    Model {
        id: "qwen2.5-coder-1.5b",
        name: "Qwen2.5-Coder-1.5B-Instruct",
        license: "Apache-2.0",
        status: Status::Validated,
        summary: "the default: fast on every device, code-focused",
        gguf: Gguf {
            repo: "Qwen/Qwen2.5-Coder-1.5B-Instruct-GGUF",
            revision: "f86cb2c1fa58255f8052cc32aeede1b7482d4361",
            file: "qwen2.5-coder-1.5b-instruct-q4_k_m.gguf",
            size: 1_117_320_768,
            sha256: "cc324af070c2ecbfd324a30884d2f951a7ff756aba85cb811a6ec436933bb046",
        },
        openvino: Some("qwen2.5-coder-1.5b-instruct-int4-sym-g128"),
        rules: PromptRules::Full,
    },
    Model {
        id: "qwen2.5-coder-0.5b",
        name: "Qwen2.5-Coder-0.5B-Instruct",
        license: "Apache-2.0",
        status: Status::Experimental,
        summary: "the smallest: faster, for slow CPUs; weaker messages",
        gguf: Gguf {
            repo: "Qwen/Qwen2.5-Coder-0.5B-Instruct-GGUF",
            revision: "ebb2015119c907b064c512bf053e945850b5875f",
            file: "qwen2.5-coder-0.5b-instruct-q4_k_m.gguf",
            size: 491_400_064,
            sha256: "1d9614638d18024d0fbb36575a15f1302a3adf044df10345688ec4f6e1c4ff32",
        },
        openvino: None,
        rules: PromptRules::Full,
    },
    Model {
        id: "qwen2.5-coder-7b",
        name: "Qwen2.5-Coder-7B-Instruct",
        license: "Apache-2.0",
        status: Status::Experimental,
        summary: "the largest: better messages, but slow to load (8 s on a GPU)",
        gguf: Gguf {
            repo: "Qwen/Qwen2.5-Coder-7B-Instruct-GGUF",
            revision: "13fb94bfda8c8cf22497dc57b78f391a9acb426a",
            file: "qwen2.5-coder-7b-instruct-q4_k_m.gguf",
            size: 4_683_073_536,
            sha256: "509287f78cb4d4cf6b3843734733b914b2c158e43e22a7f4bf5e963800894d3c",
        },
        openvino: None,
        rules: PromptRules::Full,
    },
    Model {
        id: "smart-commit-1.5b-v0.1",
        name: "smart-commit 1.5B v0.1",
        license: "Apache-2.0",
        status: Status::Experimental,
        summary: "Qwen2.5-Coder-1.5B fine-tuned for smart-commit: the right commit types",
        gguf: Gguf {
            repo: "LucasBRT/smart-commit-1.5b-v0.1",
            revision: "9147da25797e21b63815f9ad80a1e79547198a37",
            file: "smart-commit-1.5b-v0.1-q4_k_m.gguf",
            size: 986_048_192,
            sha256: "7b8f456ff62c050e43ebdb90593960990e490e1dd7c2635652a82929a040d9f7",
        },
        openvino: None,
        rules: PromptRules::Full,
    },
    Model {
        id: "smart-commit-0.5b-v0.1",
        name: "smart-commit 0.5B v0.1",
        license: "Apache-2.0",
        status: Status::Experimental,
        summary: "Qwen2.5-Coder-0.5B fine-tuned: as good as the 1.5B on types, faster",
        gguf: Gguf {
            repo: "LucasBRT/smart-commit-0.5b-v0.1",
            revision: "829633003d096d47256a8dd12fe666ea29f89cb1",
            file: "smart-commit-0.5b-v0.1-q4_k_m.gguf",
            size: 397_807_616,
            sha256: "83b9acfc91e67cba1a2b669a0afe59ddc1b3ce5909617355b88d3a2ff6e66c11",
        },
        openvino: None,
        rules: PromptRules::Full,
    },
    Model {
        id: "smart-commit-0.5b-v0.2",
        name: "smart-commit 0.5B v0.2",
        license: "Apache-2.0",
        status: Status::Experimental,
        summary: "the 0.5B trained without the rules in its prompt: faster, copies less in young \
                  repositories",
        gguf: Gguf {
            repo: "LucasBRT/smart-commit-0.5b-v0.2",
            revision: "bdb93a5dff791bb70c44810a76f857f839bbcb15",
            file: "smart-commit-0.5b-v0.2-q4_k_m.gguf",
            size: 397_807_488,
            sha256: "87b81896a02fc10e6ead34701e71ae19083bbc5f03738b0a45d73b3f2d7b824a",
        },
        openvino: None,
        // Trained on prompts without the fixed rules (training H6).
        rules: PromptRules::Omitted,
    },
];

pub fn find(id: &str) -> Option<&'static Model> {
    REGISTRY.iter().find(|m| m.id == id)
}

/// The size of a download, for people.
pub fn size(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1e9)
    } else {
        format!("{} MB", bytes / 1_000_000)
    }
}

//! The arguments shared by several commands.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, ValueEnum};
use smart_commit::backend::device::Target;
use smart_commit::prompt::{
    DiffFormat, PromptExamples, PromptNames, PromptOptions, PromptRules, PromptScopes,
};

#[derive(Debug, Args)]
pub(super) struct GenerateArgs {
    /// Repository to work on.
    #[arg(short = 'C', long = "path", default_value = ".")]
    pub(super) path: PathBuf,
    /// Review the message (commit, edit, ask for another), then commit; outside
    /// a terminal, open the editor with it.
    #[arg(long)]
    pub(super) commit: bool,
    #[command(flatten)]
    pub(super) backend: BackendArgs,
}

/// How messages are generated. Unset options come from the environment,
/// then the config files (see `smart-commit config list --show-origin`).
#[derive(Debug, Args)]
pub(super) struct BackendArgs {
    /// [config: backend; env: SMART_COMMIT_BACKEND]
    #[arg(long, value_enum)]
    backend: Option<BackendArg>,
    #[command(flatten)]
    pub(super) model: ModelArgs,
    /// Reply of the mock backend; repeat it for the following attempts.
    /// The environment variable holds a single reply.
    #[arg(
        long = "mock-response",
        env = "SMART_COMMIT_MOCK_RESPONSE",
        value_name = "TEXT"
    )]
    pub(super) mock_responses: Vec<String>,
    /// Record inputs, prompts and replies to FILE, for `replay`. It
    /// includes the staged diff and recent commit subjects.
    #[arg(long, env = "SMART_COMMIT_TRACE", value_name = "FILE")]
    pub(super) trace: Option<PathBuf>,
}

/// For building training sets and evaluating a model or a prompt before
/// it is the default; otherwise the prompt follows the model in use.
#[derive(Debug, Args)]
pub(super) struct PromptArgs {
    /// The instructions in the prompt: all of them, or only what varies by
    /// repository, for a model trained without them [default: what the
    /// model in use needs]
    #[arg(long, value_enum)]
    rules: Option<RulesArg>,
    /// The file list: line counts only, or also each file's status,
    /// renames, and whitespace-only and comment-only changes
    /// [default: annotated]
    #[arg(long, value_enum)]
    diff: Option<DiffArg>,
    /// The scopes the repository uses: their names, or each with the
    /// places its commits touched, for a model trained on that
    /// [default: names]
    #[arg(long, value_enum)]
    scopes: Option<ScopesArg>,
    /// The subjects shown as examples of the style: the most recent, or
    /// those of the commits that touched the closest files, to evaluate
    /// that [default: recent]
    #[arg(long, value_enum)]
    examples: Option<ExamplesArg>,
    /// A line with the functions, classes and the like the change
    /// declares, and one with the comments it adds, read from every file
    /// [default: off]
    #[arg(long, value_enum)]
    names: Option<NamesArg>,
}

impl PromptArgs {
    pub(super) fn apply(&self, options: &mut PromptOptions) {
        if let Some(rules) = self.rules {
            options.rules = match rules {
                RulesArg::Full => PromptRules::Full,
                RulesArg::Omitted => PromptRules::Omitted,
            };
        }
        if let Some(scopes) = self.scopes {
            options.scopes = match scopes {
                ScopesArg::List => PromptScopes::List,
                ScopesArg::Map => PromptScopes::Map,
            };
        }
        if let Some(examples) = self.examples {
            options.examples = match examples {
                ExamplesArg::Recent => PromptExamples::Recent,
                ExamplesArg::Similar => PromptExamples::Similar,
                ExamplesArg::Mixed => PromptExamples::Mixed,
            };
        }
        if let Some(names) = self.names {
            options.names = match names {
                NamesArg::Off => PromptNames::Off,
                NamesArg::On => PromptNames::On,
            };
        }
        if let Some(diff) = self.diff {
            options.diff = match diff {
                DiffArg::Plain => DiffFormat::Plain,
                DiffArg::Annotated => DiffFormat::Annotated,
            };
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum DiffArg {
    Plain,
    Annotated,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum NamesArg {
    Off,
    On,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ExamplesArg {
    Recent,
    Similar,
    Mixed,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ScopesArg {
    List,
    Map,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum RulesArg {
    Full,
    Omitted,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum BackendArg {
    /// Whichever --device needs.
    Auto,
    /// OpenVINO GenAI: Intel NPU, Intel GPU or CPU.
    Openvino,
    /// llama.cpp with Vulkan: any GPU (AMD, NVIDIA, Intel).
    Llamacpp,
    /// Scripted replies from --mock-response, for tests and development.
    Mock,
}

/// Which model runs where, for the OpenVINO backend.
#[derive(Debug, Args)]
pub(super) struct ModelArgs {
    /// [config: model.device; env: SMART_COMMIT_DEVICE]
    #[arg(long, value_enum)]
    device: Option<DeviceArg>,
    /// [config: model.dir; env: SMART_COMMIT_MODEL_DIR]
    #[arg(long, value_name = "DIR")]
    model_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(super) enum DeviceArg {
    /// The fastest benchmarked device, else the first that works.
    Auto,
    /// Intel NPU, through OpenVINO.
    Npu,
    /// Intel GPU, through OpenVINO.
    #[value(alias = "gpu")]
    IntelGpu,
    /// Any GPU with a Vulkan driver (AMD, NVIDIA, Intel), through llama.cpp.
    Vulkan,
    /// Any x86_64 CPU (Intel or AMD), through llama.cpp.
    Cpu,
    /// The CPU through OpenVINO, when it is installed for an Intel NPU or GPU.
    OpenvinoCpu,
}

impl DeviceArg {
    /// The device it names; none for `auto`.
    pub(super) fn target(self) -> Option<Target> {
        match self {
            Self::Auto => None,
            Self::Npu => Some(Target::Npu),
            Self::IntelGpu => Some(Target::IntelGpu),
            Self::Vulkan => Some(Target::Vulkan),
            Self::Cpu => Some(Target::Cpu),
            Self::OpenvinoCpu => Some(Target::OpenvinoCpu),
        }
    }

    /// The value `model.device` takes for it.
    pub(super) fn name(self) -> &'static str {
        self.target().map_or("auto", Target::name)
    }
}

impl BackendArgs {
    pub(super) fn flags(&self) -> Result<Vec<(&'static str, String)>> {
        let mut flags = self.model.flags()?;
        if let Some(backend) = self.backend {
            let name = match backend {
                BackendArg::Auto => "auto",
                BackendArg::Openvino => "openvino",
                BackendArg::Llamacpp => "llamacpp",
                BackendArg::Mock => "mock",
            };
            flags.push(("backend", name.to_owned()));
        }
        Ok(flags)
    }
}

impl ModelArgs {
    pub(super) fn flags(&self) -> Result<Vec<(&'static str, String)>> {
        let mut flags = Vec::new();
        if let Some(device) = self.device {
            flags.push(("model.device", device.name().to_owned()));
        }
        if let Some(dir) = &self.model_dir {
            // Relative to where the command runs, as users expect from a flag.
            let dir = std::path::absolute(dir)?;
            flags.push(("model.dir", dir.to_string_lossy().into_owned()));
        }
        Ok(flags)
    }
}

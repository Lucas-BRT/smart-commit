//! What `smart-commit setup` does on a machine: the steps its hardware
//! needs, and only those, as `smart-commit setup` plans them.
//!
//!   always              llama.cpp and the chosen model, no sudo
//!   per GPU             its system drivers (setup-gpu.sh)
//!   Intel NPU           the NPU driver, Level Zero and OpenVINO
//!   Intel GPU, no NPU   OpenVINO
//!   Intel NPU or GPU    the model exported for OpenVINO, here
//!   always              the benchmark

use crate::backend::llamacpp;
use crate::hardware::{Hardware, Npu};
use crate::models::{self, Model};

/// What a step downloads, for `setup --dry-run`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Download {
    Nothing,
    Bytes(u64),
    /// It downloads, but how much depends on the machine or the mirror.
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepKind {
    LlamaCpp,
    Model,
    GpuDrivers,
    /// The NPU driver, Level Zero and OpenVINO (setup-npu.sh).
    NpuStack,
    /// OpenVINO alone, for an Intel GPU (setup-npu.sh --openvino-only).
    Openvino,
    ExportModel,
    Bench,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub kind: StepKind,
    pub title: String,
    pub detail: String,
    /// Asks for the password, outside the full-screen interface.
    pub sudo: bool,
    /// What the sudo commands touch (empty without sudo).
    pub sudo_what: &'static str,
    pub download: Download,
    /// Already set up: listed, but off unless the user turns it on.
    pub installed: bool,
    pub enabled: bool,
}

impl Step {
    fn new(kind: StepKind, title: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            kind,
            title: title.into(),
            detail: detail.into(),
            sudo: false,
            sudo_what: "",
            download: Download::Nothing,
            installed: false,
            enabled: true,
        }
    }

    fn sudo(mut self, what: &'static str) -> Self {
        self.sudo = true;
        self.sudo_what = what;
        self
    }

    fn download(mut self, download: Download) -> Self {
        self.download = download;
        self
    }

    fn installed(mut self, installed: bool) -> Self {
        self.installed = installed;
        self.enabled = !installed;
        self
    }
}

/// What is already on the machine, and what the user asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    pub model: &'static Model,
    pub llama_installed: bool,
    pub model_installed: bool,
    /// check-npu.sh passes: the NPU driver and OpenVINO work.
    pub npu_ready: bool,
    /// export-model.sh --check passes.
    pub exported: bool,
    /// What setup-gpu.sh --dry-run would do, one line per GPU.
    pub gpu_plan: Vec<String>,
    /// Not `--no-drivers`.
    pub drivers: bool,
    /// Not `--no-bench`.
    pub bench: bool,
}

pub fn plan(hw: &Hardware, facts: &Facts) -> Vec<Step> {
    let mut steps = vec![
        Step::new(
            StepKind::LlamaCpp,
            "llama.cpp b11381",
            "the cpu device, and vulkan on the GPUs; no sudo",
        )
        .download(Download::Bytes(llamacpp::install::ARCHIVE_SIZE))
        .installed(facts.llama_installed),
        model_step(facts.model, facts.model_installed),
    ];
    if facts.drivers {
        if !hw.gpus.is_empty() {
            steps.push(
                Step::new(
                    StepKind::GpuDrivers,
                    "GPU drivers",
                    facts.gpu_plan.join("; "),
                )
                .sudo("apt packages (the plan above lists them) and the render group")
                .download(Download::Unknown),
            );
        }
        if hw.npu == Npu::Intel {
            steps.push(
                Step::new(
                    StepKind::NpuStack,
                    "Intel NPU driver and OpenVINO",
                    "NPU driver 1.38.0, Level Zero 1.32.0, OpenVINO GenAI 2026.3.1",
                )
                .sudo(
                    "apt packages, dpkg installs, files in /opt/intel and \
                     /etc/ld.so.conf.d, and the render group",
                )
                .download(Download::Unknown)
                .installed(facts.npu_ready),
            );
        } else if hw.has_intel_gpu() && !hw.openvino_installed {
            steps.push(
                Step::new(
                    StepKind::Openvino,
                    "OpenVINO",
                    "OpenVINO GenAI 2026.3.1, for the intel-gpu device",
                )
                .sudo("apt packages, files in /opt/intel and /etc/ld.so.conf.d")
                .download(Download::Unknown),
            );
        }
    }
    if (hw.needs_openvino() || hw.openvino_installed) && facts.model.openvino.is_some() {
        steps.push(
            Step::new(
                StepKind::ExportModel,
                "the model for OpenVINO",
                "exported here from Hugging Face; about 10 GB of disk while it runs",
            )
            .download(Download::Unknown)
            .installed(facts.exported),
        );
    }
    if facts.bench {
        steps.push(Step::new(
            StepKind::Bench,
            "Benchmark",
            "measures every device; auto then uses the fastest",
        ));
    }
    steps
}

pub fn model_step(model: &'static Model, installed: bool) -> Step {
    Step::new(
        StepKind::Model,
        format!("Model {}", model.id),
        format!(
            "{} from huggingface.co/{}, {}",
            models::size(model.gguf.size),
            model.gguf.repo,
            model.license
        ),
    )
    .download(Download::Bytes(model.gguf.size))
    .installed(installed)
}

/// Why OpenVINO and Intel's drivers are left out, when they are.
pub fn not_needed(hw: &Hardware) -> Option<&'static str> {
    (!hw.needs_openvino() && !hw.openvino_installed).then_some(
        "Not needed: OpenVINO, the Intel NPU driver and Intel's GPU runtime, since there \
         is no Intel NPU or GPU here.",
    )
}

/// The plan as `smart-commit setup --dry-run` prints it: every step with its
/// sudo and download, nothing applied.
pub fn dry_run_text(inventory: &str, not_needed: Option<&str>, steps: &[Step]) -> String {
    let mut out = format!(
        "==> Hardware\n{}\n\n==> Plan (dry run)\n",
        inventory.trim_end()
    );
    for (n, step) in steps.iter().enumerate() {
        out.push_str(&format!(
            "\n{}. {}\n   {}\n",
            n + 1,
            step.title,
            step.detail
        ));
        if step.sudo {
            out.push_str(&format!("   needs sudo: {}\n", step.sudo_what));
        } else {
            out.push_str("   needs sudo: no\n");
        }
        match step.download {
            Download::Nothing => {}
            Download::Bytes(bytes) => {
                out.push_str(&format!("   downloads: {}\n", models::size(bytes)));
            }
            Download::Unknown => out.push_str("   downloads: yes, size not known here\n"),
        }
        if step.installed {
            out.push_str("   already done, would be skipped\n");
        }
    }
    if let Some(why) = not_needed {
        out.push_str(&format!("\n{why}\n"));
    }
    let pending = || steps.iter().filter(|s| !s.installed);
    if pending().all(|s| s.download != Download::Unknown) {
        let total: u64 = pending()
            .map(|s| match s.download {
                Download::Bytes(bytes) => bytes,
                _ => 0,
            })
            .sum();
        out.push_str(&format!("\nTotal download: {}\n", models::size(total)));
    }
    out.push_str("\nNothing was changed. Run `smart-commit setup` to apply it.\n");
    out
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::hardware::{Sources, fake};

    fn machine(build: fn(&Path)) -> Hardware {
        let dir = tempfile::TempDir::new().unwrap();
        let sysfs = dir.path().join("sys");
        std::fs::create_dir_all(sysfs.join("bus/pci/devices")).unwrap();
        build(&sysfs);
        Hardware::detect(&Sources {
            cpuinfo: sysfs.join("cpuinfo"),
            sysfs,
            openvino_dir: dir.path().join("none"),
        })
    }

    fn facts() -> Facts {
        Facts {
            model: models::find(models::DEFAULT).unwrap(),
            llama_installed: false,
            model_installed: false,
            npu_ready: false,
            exported: false,
            gpu_plan: vec!["NVIDIA GPU (0000:01:00.0): its own Vulkan ICD".into()],
            drivers: true,
            bench: true,
        }
    }

    fn kinds(steps: &[Step]) -> Vec<StepKind> {
        steps.iter().map(|s| s.kind).collect()
    }

    #[test]
    fn nvidia_on_amd_gets_no_openvino() {
        let hw = machine(fake::amd_nvidia);
        let steps = plan(&hw, &facts());
        assert_eq!(
            kinds(&steps),
            [
                StepKind::LlamaCpp,
                StepKind::Model,
                StepKind::GpuDrivers,
                StepKind::Bench
            ]
        );
        assert!(steps[2].sudo && !steps[0].sudo);
        assert_eq!(
            steps[2].detail,
            "NVIDIA GPU (0000:01:00.0): its own Vulkan ICD"
        );
        assert!(not_needed(&hw).unwrap().contains("no Intel NPU or GPU"));
    }

    #[test]
    fn intel_gets_the_npu_stack_and_the_export() {
        let hw = machine(fake::all_intel);
        let steps = plan(&hw, &facts());
        assert_eq!(
            kinds(&steps),
            [
                StepKind::LlamaCpp,
                StepKind::Model,
                StepKind::GpuDrivers,
                StepKind::NpuStack,
                StepKind::ExportModel,
                StepKind::Bench
            ]
        );
        assert_eq!(not_needed(&hw), None);
    }

    #[test]
    fn what_is_installed_is_listed_but_off() {
        let hw = machine(fake::all_intel);
        let facts = Facts {
            llama_installed: true,
            model_installed: true,
            npu_ready: true,
            exported: true,
            ..facts()
        };
        let steps = plan(&hw, &facts);
        let off: Vec<StepKind> = steps
            .iter()
            .filter(|s| !s.enabled)
            .map(|s| s.kind)
            .collect();
        assert_eq!(
            off,
            [
                StepKind::LlamaCpp,
                StepKind::Model,
                StepKind::NpuStack,
                StepKind::ExportModel
            ]
        );
        assert!(steps.iter().filter(|s| !s.enabled).all(|s| s.installed));
    }

    #[test]
    fn an_intel_gpu_without_npu_gets_openvino_alone() {
        let hw = machine(|sys| {
            fake::cpu(sys, "GenuineIntel", "Intel(R) Core(TM) i5-1135G7");
            fake::pci(sys, "0000:00:02.0", "0x030000", "0x8086", "0x9a49", "i915");
        });
        assert!(kinds(&plan(&hw, &facts())).contains(&StepKind::Openvino));
    }

    #[test]
    fn options_and_models_change_the_plan() {
        let hw = machine(fake::all_intel);
        let no_drivers = Facts {
            drivers: false,
            bench: false,
            ..facts()
        };
        assert_eq!(
            kinds(&plan(&hw, &no_drivers)),
            [StepKind::LlamaCpp, StepKind::Model, StepKind::ExportModel]
        );
        // A model without an OpenVINO version has nothing to export.
        let small = Facts {
            model: models::find("qwen2.5-coder-0.5b").unwrap(),
            ..facts()
        };
        let steps = plan(&hw, &small);
        assert!(!kinds(&steps).contains(&StepKind::ExportModel));
        assert_eq!(steps[1].title, "Model qwen2.5-coder-0.5b");
        assert!(
            steps[1]
                .detail
                .starts_with("491 MB from huggingface.co/Qwen/")
        );

        let hw = machine(fake::amd_only);
        assert_eq!(
            kinds(&plan(&hw, &facts())),
            [StepKind::LlamaCpp, StepKind::Model, StepKind::Bench],
            "no GPU, an unsupported NPU"
        );
    }

    fn text(build: fn(&Path), facts: &Facts) -> String {
        let hw = machine(build);
        dry_run_text(&hw.inventory(), not_needed(&hw), &plan(&hw, facts))
    }

    #[test]
    fn the_dry_run_of_an_nvidia_machine_has_one_sudo_step_and_a_total() {
        let out = text(fake::amd_nvidia, &facts());
        assert!(out.contains("1. llama.cpp b11381"), "{out}");
        assert!(out.contains("downloads: 31 MB"), "{out}");
        assert!(out.contains("needs sudo: apt packages"), "{out}");
        assert_eq!(out.matches("needs sudo: no").count(), 3, "{out}");
        // The GPU drivers' size is not known here, so there is no total.
        assert!(!out.contains("Total download"), "{out}");
        assert!(out.contains("Not needed: OpenVINO"), "{out}");
        assert!(out.ends_with("Nothing was changed. Run `smart-commit setup` to apply it.\n"));
    }

    #[test]
    fn the_dry_run_of_an_intel_npu_lists_its_system_files() {
        let out = text(fake::all_intel, &facts());
        assert!(out.contains("Intel NPU driver and OpenVINO"), "{out}");
        assert!(out.contains("/opt/intel"), "{out}");
        assert!(out.contains("the model for OpenVINO"), "{out}");
        assert!(!out.contains("Not needed"), "{out}");
    }

    #[test]
    fn the_dry_run_of_a_cpu_only_machine_adds_up_the_download() {
        let out = text(fake::amd_only, &facts());
        assert!(!out.contains("sudo: apt"), "{out}");
        let size = llamacpp::install::ARCHIVE_SIZE + facts().model.gguf.size;
        assert!(out.contains(&format!("Total download: {}", models::size(size))));
    }

    #[test]
    fn the_dry_run_without_drivers_has_no_sudo_step() {
        let no_drivers = Facts {
            drivers: false,
            ..facts()
        };
        let out = text(fake::all_intel, &no_drivers);
        assert_eq!(
            out.matches("needs sudo:").count(),
            out.matches("needs sudo: no").count(),
            "{out}"
        );
    }

    #[test]
    fn the_dry_run_marks_what_is_done_and_leaves_it_out_of_the_total() {
        let done = Facts {
            llama_installed: true,
            model_installed: true,
            ..facts()
        };
        let out = text(fake::amd_only, &done);
        assert_eq!(out.matches("already done, would be skipped").count(), 2);
        assert!(out.contains("Total download: 0 MB"), "{out}");
    }
}

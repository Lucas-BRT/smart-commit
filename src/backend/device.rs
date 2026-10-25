//! Which device runs the model, and on which backend.
//!
//! With `model.device = "auto"` the candidates are the benchmarked ones,
//! fastest first, then the rest in [`DEFAULT_ORDER`]; the first that loads
//! wins. The device that actually ran is part of the backend's description,
//! so traces record it.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::backend::{Backend, BackendError};
use crate::config::{BackendKind, DeviceSetting};
use crate::hardware::{Hardware, Npu};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Target {
    /// Intel NPU through OpenVINO.
    Npu,
    /// Intel GPU through OpenVINO.
    IntelGpu,
    /// Any GPU with a Vulkan driver (AMD, NVIDIA, Intel), through llama.cpp.
    Vulkan,
    /// Any x86_64 CPU (Intel or AMD), through llama.cpp.
    Cpu,
    /// Any x86_64 CPU, through OpenVINO, for machines that have it anyway.
    OpenvinoCpu,
}

/// Without a benchmark: the Intel GPU first (fastest and lightest in our
/// measurements), then the NPU, any other GPU, and the CPU, which always
/// works.
pub const DEFAULT_ORDER: [Target; 5] = [
    Target::IntelGpu,
    Target::Npu,
    Target::Vulkan,
    Target::Cpu,
    Target::OpenvinoCpu,
];

/// The name `intel-gpu` had before 0.4, still accepted.
pub const OLD_INTEL_GPU_NAME: &str = "gpu";

impl Target {
    pub fn name(self) -> &'static str {
        match self {
            Self::Npu => "npu",
            Self::IntelGpu => "intel-gpu",
            Self::Vulkan => "vulkan",
            Self::Cpu => "cpu",
            Self::OpenvinoCpu => "openvino-cpu",
        }
    }

    /// The device called `name` in `model.device` (or its old name).
    pub fn from_name(name: &str) -> Option<Self> {
        if name == OLD_INTEL_GPU_NAME {
            return Some(Self::IntelGpu);
        }
        DEFAULT_ORDER.into_iter().find(|t| t.name() == name)
    }

    pub fn backend(self) -> BackendKind {
        match self {
            Self::Vulkan | Self::Cpu => BackendKind::LlamaCpp,
            Self::Npu | Self::IntelGpu | Self::OpenvinoCpu => BackendKind::Openvino,
        }
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Why no device could be chosen or loaded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DeviceError {
    #[error("the mock backend has no devices")]
    MockHasNoDevices,
    #[error("model.device = {device} needs backend = {needs} (or auto)")]
    WrongBackend { device: Target, needs: &'static str },
    /// The only candidate failed: its own error.
    #[error(transparent)]
    Only(BackendError),
    #[error("no device could run the model:{}", list_failures(.0))]
    NoneWorked(Vec<(Target, BackendError)>),
}

fn list_failures(failures: &[(Target, BackendError)]) -> String {
    failures
        .iter()
        .map(|(target, err)| format!("\n  {target}: {err}"))
        .collect()
}

impl Target {
    /// Whether `hardware` has what this device runs on: an Intel NPU, an
    /// Intel GPU with its kernel driver, a GPU with a Vulkan driver, an
    /// x86_64 CPU (OpenVINO's also needs OpenVINO installed). The runtime
    /// may still be missing: that is for loading to say.
    pub fn is_present(self, hardware: &Hardware) -> bool {
        let x86_64 = hardware.arch == "x86_64";
        match self {
            Self::Npu => hardware.npu == Npu::Intel,
            Self::IntelGpu => hardware
                .gpus
                .iter()
                .any(|gpu| gpu.is_intel() && matches!(gpu.driver.as_str(), "i915" | "xe")),
            Self::Vulkan => hardware.gpus.iter().any(|gpu| {
                matches!(
                    (gpu.vendor.as_str(), gpu.driver.as_str()),
                    ("0x8086", "i915" | "xe")
                        | ("0x1002", "amdgpu")
                        | ("0x10de", "nvidia" | "nouveau")
                )
            }),
            Self::Cpu => x86_64,
            Self::OpenvinoCpu => x86_64 && hardware.openvino_installed,
        }
    }
}

/// The devices `hardware` has, in [`DEFAULT_ORDER`].
pub fn present(hardware: &Hardware) -> Vec<Target> {
    DEFAULT_ORDER
        .into_iter()
        .filter(|target| target.is_present(hardware))
        .collect()
}

/// `order` without the devices `hardware` lacks, so `auto` does not try an
/// NPU on a machine without one. If that leaves nothing (hardware that is
/// not recognised), `order` as it is: a load can still work.
pub fn only_present(order: Vec<Target>, hardware: &Hardware) -> Vec<Target> {
    let kept: Vec<Target> = order
        .iter()
        .copied()
        .filter(|target| target.is_present(hardware))
        .collect();
    if kept.is_empty() { order } else { kept }
}

/// The devices to try, in order. `preferred` (from a benchmark, fastest
/// first) goes before the default order. Not for the mock backend.
pub fn candidates(
    backend: BackendKind,
    device: DeviceSetting,
    preferred: &[Target],
) -> Result<Vec<Target>, DeviceError> {
    let fixed = match device {
        DeviceSetting::Auto => None,
        DeviceSetting::Fixed(target) => Some(target),
    };
    let allowed = |target: Target| match backend {
        BackendKind::Auto => true,
        BackendKind::Mock => false,
        kind => target.backend() == kind,
    };
    if backend == BackendKind::Mock {
        return Err(DeviceError::MockHasNoDevices);
    }
    if let Some(target) = fixed {
        if !allowed(target) {
            let needs = match target.backend() {
                BackendKind::LlamaCpp => "llamacpp",
                _ => "openvino",
            };
            return Err(DeviceError::WrongBackend {
                device: target,
                needs,
            });
        }
        return Ok(vec![target]);
    }
    let mut order: Vec<Target> = Vec::new();
    for target in preferred.iter().chain(&DEFAULT_ORDER) {
        if allowed(*target) && !order.contains(target) {
            order.push(*target);
        }
    }
    Ok(order)
}

pub struct Selection {
    pub target: Target,
    pub backend: Box<dyn Backend>,
    /// Candidates tried before it, and why they failed.
    pub skipped: Vec<(Target, BackendError)>,
}

/// Loads the first candidate that works. Fails listing every reason.
pub fn load_first(
    candidates: &[Target],
    mut load: impl FnMut(Target) -> Result<Box<dyn Backend>, BackendError>,
) -> Result<Selection, DeviceError> {
    let mut skipped = Vec::new();
    for &target in candidates {
        match load(target) {
            Ok(backend) => {
                return Ok(Selection {
                    target,
                    backend,
                    skipped,
                });
            }
            Err(err) => {
                tracing::debug!(%target, %err, "device unavailable");
                skipped.push((target, err));
            }
        }
    }
    match skipped.as_slice() {
        [(_, only)] => Err(DeviceError::Only(only.clone())),
        _ => Err(DeviceError::NoneWorked(skipped)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::MockBackend;

    fn machine(gpus: &[(&str, &str)], npu: Npu, openvino: bool) -> Hardware {
        Hardware {
            cpu_vendor: crate::hardware::CpuVendor::Other,
            cpu_model: "Test CPU".into(),
            arch: "x86_64".into(),
            gpus: gpus
                .iter()
                .map(|(vendor, driver)| crate::hardware::Gpu {
                    vendor: (*vendor).into(),
                    device: "0x0000".into(),
                    driver: (*driver).into(),
                    slot: "0000:00:02.0".into(),
                    name: None,
                    nvidia_version: None,
                })
                .collect(),
            npu,
            openvino_installed: openvino,
        }
    }

    #[test]
    fn a_machine_has_the_devices_its_hardware_gives() {
        let all = machine(&[("0x8086", "xe")], Npu::Intel, true);
        assert_eq!(present(&all), DEFAULT_ORDER);
        let amd = machine(&[("0x1002", "amdgpu")], Npu::None, false);
        assert_eq!(present(&amd), [Target::Vulkan, Target::Cpu]);
        let nvidia = machine(&[("0x10de", "nvidia")], Npu::None, false);
        assert_eq!(present(&nvidia), [Target::Vulkan, Target::Cpu]);
        let nouveau = machine(&[("0x10de", "nouveau")], Npu::None, false);
        assert_eq!(present(&nouveau), [Target::Vulkan, Target::Cpu]);
        let bare = machine(&[], Npu::None, false);
        assert_eq!(present(&bare), [Target::Cpu]);
    }

    #[test]
    fn a_gpu_needs_its_kernel_driver() {
        let unbound = machine(&[("0x8086", "none"), ("0x10de", "none")], Npu::None, false);
        assert_eq!(present(&unbound), [Target::Cpu]);
        let old = machine(&[("0x1002", "radeon")], Npu::None, false);
        assert_eq!(present(&old), [Target::Cpu]);
    }

    #[test]
    fn the_openvino_devices_of_an_intel_machine_wait_for_openvino_but_the_npu_does_not() {
        // The NPU is listed so that loading can say what is missing.
        let hardware = machine(&[("0x8086", "i915")], Npu::Intel, false);
        assert_eq!(
            present(&hardware),
            [Target::IntelGpu, Target::Npu, Target::Vulkan, Target::Cpu]
        );
    }

    #[test]
    fn the_ryzen_ai_npu_is_not_one_of_ours() {
        let hardware = machine(&[], Npu::AmdXdna, true);
        assert_eq!(present(&hardware), [Target::Cpu, Target::OpenvinoCpu]);
    }

    #[test]
    fn only_x86_64_has_the_cpu_devices() {
        let mut arm = machine(&[("0x10de", "nvidia")], Npu::None, true);
        arm.arch = "aarch64".into();
        assert_eq!(present(&arm), [Target::Vulkan]);
    }

    #[test]
    fn auto_keeps_its_order_without_the_devices_a_machine_lacks() {
        let amd = machine(&[("0x1002", "amdgpu")], Npu::None, false);
        let order = vec![Target::Npu, Target::Cpu, Target::Vulkan, Target::IntelGpu];
        assert_eq!(only_present(order, &amd), [Target::Cpu, Target::Vulkan]);
    }

    #[test]
    fn auto_keeps_every_candidate_when_none_is_recognised() {
        let arm = Hardware {
            arch: "riscv64".into(),
            ..machine(&[], Npu::None, false)
        };
        assert_eq!(only_present(DEFAULT_ORDER.to_vec(), &arm), DEFAULT_ORDER);
    }

    #[test]
    fn names_round_trip_and_the_old_gpu_name_still_works() {
        for target in DEFAULT_ORDER {
            assert_eq!(Target::from_name(target.name()), Some(target));
            let json = serde_json::to_string(&target).unwrap();
            assert_eq!(json, format!("\"{}\"", target.name()));
        }
        assert_eq!(Target::from_name("gpu"), Some(Target::IntelGpu));
        assert_eq!(Target::from_name("auto"), None);
        assert_eq!(Target::from_name(""), None);
    }

    #[test]
    fn the_cpu_runs_on_llamacpp_unless_openvino_is_asked_for() {
        assert_eq!(Target::Cpu.backend(), BackendKind::LlamaCpp);
        assert_eq!(Target::Vulkan.backend(), BackendKind::LlamaCpp);
        assert_eq!(Target::OpenvinoCpu.backend(), BackendKind::Openvino);
        assert_eq!(Target::IntelGpu.backend(), BackendKind::Openvino);
        assert_eq!(Target::Npu.backend(), BackendKind::Openvino);
    }

    #[test]
    fn auto_follows_the_benchmark_then_the_default_order() {
        let order = candidates(BackendKind::Auto, DeviceSetting::Auto, &[]).unwrap();
        assert_eq!(order, DEFAULT_ORDER);
        let order = candidates(
            BackendKind::Auto,
            DeviceSetting::Auto,
            &[Target::Vulkan, Target::Cpu],
        )
        .unwrap();
        assert_eq!(
            order,
            [
                Target::Vulkan,
                Target::Cpu,
                Target::IntelGpu,
                Target::Npu,
                Target::OpenvinoCpu
            ]
        );
    }

    #[test]
    fn a_fixed_backend_limits_the_devices() {
        let openvino = candidates(
            BackendKind::Openvino,
            DeviceSetting::Auto,
            &[Target::Vulkan],
        );
        assert_eq!(
            openvino.unwrap(),
            [Target::IntelGpu, Target::Npu, Target::OpenvinoCpu]
        );
        let llamacpp = candidates(BackendKind::LlamaCpp, DeviceSetting::Auto, &[]);
        assert_eq!(llamacpp.unwrap(), [Target::Vulkan, Target::Cpu]);
    }

    #[test]
    fn a_fixed_device_is_the_only_candidate() {
        let fixed = DeviceSetting::Fixed;
        assert_eq!(
            candidates(BackendKind::Auto, fixed(Target::Npu), &[Target::IntelGpu]).unwrap(),
            [Target::Npu]
        );
        assert_eq!(
            candidates(BackendKind::Auto, fixed(Target::Vulkan), &[]).unwrap(),
            [Target::Vulkan]
        );
        let err = candidates(BackendKind::Openvino, fixed(Target::Cpu), &[]).unwrap_err();
        assert_eq!(
            err.to_string(),
            "model.device = cpu needs backend = llamacpp (or auto)"
        );
        let err = candidates(BackendKind::LlamaCpp, fixed(Target::OpenvinoCpu), &[]).unwrap_err();
        assert_eq!(
            err.to_string(),
            "model.device = openvino-cpu needs backend = openvino (or auto)"
        );
        assert!(candidates(BackendKind::Mock, DeviceSetting::Auto, &[]).is_err());
    }

    #[test]
    fn the_first_device_that_loads_wins() {
        let mut tried = Vec::new();
        let selection = load_first(&DEFAULT_ORDER, |target| {
            tried.push(target);
            match target {
                Target::IntelGpu => Err(BackendError::Unavailable("no GPU runtime".into())),
                _ => Ok(Box::new(MockBackend::default())),
            }
        })
        .unwrap();
        assert_eq!(selection.target, Target::Npu);
        assert_eq!(
            tried,
            [Target::IntelGpu, Target::Npu],
            "stops at the first success"
        );
        assert_eq!(selection.skipped.len(), 1);
        assert_eq!(selection.skipped[0].0, Target::IntelGpu);
    }

    #[test]
    fn failures_are_all_reported() {
        let fail = |target: Target| -> Result<Box<dyn Backend>, BackendError> {
            Err(BackendError::Unavailable(format!("{target} missing")))
        };
        let err = load_first(&[Target::IntelGpu, Target::Cpu], fail)
            .err()
            .unwrap();
        assert_eq!(
            err.to_string(),
            "no device could run the model:\n  intel-gpu: backend unavailable: intel-gpu \
             missing\n  cpu: backend unavailable: cpu missing"
        );
        // One candidate: its error as is.
        let err = load_first(&[Target::Cpu], fail).err().unwrap();
        assert_eq!(err.to_string(), "backend unavailable: cpu missing");
    }
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    /// Intel NPU through OpenVINO.
    Npu,
    /// Intel GPU through OpenVINO.
    Gpu,
    /// CPU through OpenVINO.
    Cpu,
    /// Any GPU with a Vulkan driver, through llama.cpp.
    Vulkan,
}

/// Without a benchmark: GPU first (fastest and lightest in our
/// measurements), then the NPU, any other GPU, and the CPU, which always
/// works.
pub const DEFAULT_ORDER: [Target; 4] = [Target::Gpu, Target::Npu, Target::Vulkan, Target::Cpu];

impl Target {
    pub fn name(self) -> &'static str {
        match self {
            Self::Npu => "npu",
            Self::Gpu => "gpu",
            Self::Cpu => "cpu",
            Self::Vulkan => "vulkan",
        }
    }

    pub fn backend(self) -> BackendKind {
        match self {
            Self::Vulkan => BackendKind::LlamaCpp,
            _ => BackendKind::Openvino,
        }
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The devices to try, in order. `preferred` (from a benchmark, fastest
/// first) goes before the default order. Not for the mock backend.
pub fn candidates(
    backend: BackendKind,
    device: DeviceSetting,
    preferred: &[Target],
) -> Result<Vec<Target>, String> {
    let fixed = match device {
        DeviceSetting::Auto => None,
        DeviceSetting::Npu => Some(Target::Npu),
        DeviceSetting::Gpu => Some(Target::Gpu),
        DeviceSetting::Cpu => Some(Target::Cpu),
        DeviceSetting::Vulkan => Some(Target::Vulkan),
    };
    let allowed = |target: Target| match backend {
        BackendKind::Auto => true,
        BackendKind::Mock => false,
        kind => target.backend() == kind,
    };
    if backend == BackendKind::Mock {
        return Err("the mock backend has no devices".to_owned());
    }
    if let Some(target) = fixed {
        if !allowed(target) {
            let needs = match target.backend() {
                BackendKind::LlamaCpp => "llamacpp",
                _ => "openvino",
            };
            return Err(format!(
                "model.device = {target} needs backend = {needs} (or auto)"
            ));
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
) -> Result<Selection, String> {
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
            Err(err) => skipped.push((target, err)),
        }
    }
    match skipped.as_slice() {
        [(_, only)] => Err(only.to_string()),
        all => Err(format!(
            "no device could run the model:{}",
            all.iter()
                .map(|(target, err)| format!("\n  {target}: {err}"))
                .collect::<String>()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::MockBackend;

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
            [Target::Vulkan, Target::Cpu, Target::Gpu, Target::Npu]
        );
    }

    #[test]
    fn a_fixed_backend_limits_the_devices() {
        let openvino = candidates(
            BackendKind::Openvino,
            DeviceSetting::Auto,
            &[Target::Vulkan],
        );
        assert_eq!(openvino.unwrap(), [Target::Gpu, Target::Npu, Target::Cpu]);
        let llamacpp = candidates(BackendKind::LlamaCpp, DeviceSetting::Auto, &[]);
        assert_eq!(llamacpp.unwrap(), [Target::Vulkan]);
    }

    #[test]
    fn a_fixed_device_is_the_only_candidate() {
        assert_eq!(
            candidates(BackendKind::Auto, DeviceSetting::Npu, &[Target::Gpu]).unwrap(),
            [Target::Npu]
        );
        assert_eq!(
            candidates(BackendKind::Auto, DeviceSetting::Vulkan, &[]).unwrap(),
            [Target::Vulkan]
        );
        let err = candidates(BackendKind::Openvino, DeviceSetting::Vulkan, &[]).unwrap_err();
        assert_eq!(
            err,
            "model.device = vulkan needs backend = llamacpp (or auto)"
        );
        assert!(candidates(BackendKind::LlamaCpp, DeviceSetting::Npu, &[]).is_err());
        assert!(candidates(BackendKind::Mock, DeviceSetting::Auto, &[]).is_err());
    }

    #[test]
    fn the_first_device_that_loads_wins() {
        let mut tried = Vec::new();
        let selection = load_first(&DEFAULT_ORDER, |target| {
            tried.push(target);
            match target {
                Target::Gpu => Err(BackendError::Unavailable("no GPU runtime".into())),
                _ => Ok(Box::new(MockBackend::default())),
            }
        })
        .unwrap();
        assert_eq!(selection.target, Target::Npu);
        assert_eq!(
            tried,
            [Target::Gpu, Target::Npu],
            "stops at the first success"
        );
        assert_eq!(selection.skipped.len(), 1);
        assert_eq!(selection.skipped[0].0, Target::Gpu);
    }

    #[test]
    fn failures_are_all_reported() {
        let fail = |target: Target| -> Result<Box<dyn Backend>, BackendError> {
            Err(BackendError::Unavailable(format!("{target} missing")))
        };
        let err = load_first(&[Target::Gpu, Target::Cpu], fail).err().unwrap();
        assert_eq!(
            err,
            "no device could run the model:\n  gpu: backend unavailable: gpu missing\n  \
             cpu: backend unavailable: cpu missing"
        );
        // One candidate: its error as is.
        let err = load_first(&[Target::Cpu], fail).err().unwrap();
        assert_eq!(err, "backend unavailable: cpu missing");
    }
}

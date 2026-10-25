//! The hardware of this machine, as smart-commit sees it: the CPU, every GPU
//! and NPU, their vendor and kernel driver, and which smart-commit devices
//! each one gives. Read from sysfs and /proc/cpuinfo, without root.

#[cfg(test)]
pub(crate) mod fake;
mod sysfs;
#[cfg(test)]
mod tests;

use std::fmt::Write;
use std::path::{Path, PathBuf};

use sysfs::{lspci_name, pci_devices};

pub use sysfs::physical_cores;

/// Where to read from: the real machine, or a fake one in tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sources {
    pub sysfs: PathBuf,
    pub cpuinfo: PathBuf,
    pub openvino_dir: PathBuf,
}

impl Sources {
    pub fn real() -> Self {
        Self {
            sysfs: PathBuf::from("/sys"),
            cpuinfo: PathBuf::from("/proc/cpuinfo"),
            openvino_dir: PathBuf::from("/opt/intel/openvino"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuVendor {
    Intel,
    Amd,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gpu {
    /// PCI vendor id, e.g. `0x10de`.
    pub vendor: String,
    pub device: String,
    /// The kernel driver bound to it, or `none`.
    pub driver: String,
    pub slot: String,
    /// The marketing name from lspci, on the real machine.
    pub name: Option<String>,
    /// The proprietary NVIDIA driver's version.
    pub nvidia_version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Npu {
    None,
    Intel,
    /// Ryzen AI (XDNA), which smart-commit does not use.
    AmdXdna,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hardware {
    pub cpu_vendor: CpuVendor,
    pub cpu_model: String,
    pub arch: String,
    pub gpus: Vec<Gpu>,
    pub npu: Npu,
    pub openvino_installed: bool,
}

pub fn vendor_name(id: &str) -> String {
    match id {
        "0x8086" => "Intel".to_owned(),
        "0x1002" | "0x1022" => "AMD".to_owned(),
        "0x10de" => "NVIDIA".to_owned(),
        other => format!("vendor {other}"),
    }
}

impl Gpu {
    pub fn is_intel(&self) -> bool {
        self.vendor == "0x8086"
    }

    /// The smart-commit devices this GPU gives.
    pub fn devices(&self) -> &'static str {
        match (self.vendor.as_str(), self.driver.as_str()) {
            ("0x8086", "i915" | "xe") => "intel-gpu (OpenVINO), vulkan (Mesa ANV)",
            ("0x1002", "amdgpu") => "vulkan (Mesa RADV)",
            ("0x10de", "nvidia") => "vulkan (the NVIDIA driver's Vulkan ICD)",
            ("0x10de", "nouveau") => "vulkan (Mesa NVK, Turing or newer)",
            (_, "none") => "none until a kernel driver is bound",
            ("0x1002", "radeon") => "none: the old radeon driver has no Vulkan; amdgpu does",
            _ => "none: not supported",
        }
    }

    /// `NVIDIA device 2507 [0000:01:00.0], driver nvidia 595.104.02`.
    pub fn describe(&self) -> String {
        let name = self.name.clone().unwrap_or_else(|| {
            format!(
                "{} device {}",
                vendor_name(&self.vendor),
                self.device.trim_start_matches("0x")
            )
        });
        let version = self
            .nvidia_version
            .as_ref()
            .map(|v| format!(" {v}"))
            .unwrap_or_default();
        format!("{name} [{}], driver {}{version}", self.slot, self.driver)
    }
}

impl Hardware {
    pub fn detect(sources: &Sources) -> Self {
        let cpuinfo = std::fs::read_to_string(&sources.cpuinfo).unwrap_or_default();
        let field = |name: &str| {
            cpuinfo.lines().find_map(|line| {
                let (key, value) = line.split_once(':')?;
                (key.trim_end() == name).then(|| value.trim().to_owned())
            })
        };
        let cpu_vendor = match field("vendor_id").as_deref() {
            Some("GenuineIntel") => CpuVendor::Intel,
            Some("AuthenticAMD") => CpuVendor::Amd,
            _ => CpuVendor::Other,
        };
        let real = sources.sysfs == Path::new("/sys");
        let gpus = pci_devices(&sources.sysfs, "0x03")
            .into_iter()
            .map(|d| {
                let nvidia_version = (d.driver == "nvidia").then(|| {
                    std::fs::read_to_string(sources.sysfs.join("module/nvidia/version"))
                        .map(|v| v.trim().to_owned())
                        .unwrap_or_else(|_| "(unknown version)".to_owned())
                });
                Gpu {
                    name: if real { lspci_name(&d.slot) } else { None },
                    vendor: d.vendor,
                    device: d.device,
                    driver: d.driver,
                    slot: d.slot,
                    nvidia_version,
                }
            })
            .collect();
        let intel_npu = pci_devices(&sources.sysfs, "0x1200")
            .iter()
            .any(|d| d.vendor == "0x8086")
            || sources.sysfs.join("module/intel_vpu").is_dir();
        let amd_npu = pci_devices(&sources.sysfs, "0x11")
            .iter()
            .any(|d| d.vendor == "0x1022" && matches!(d.device.as_str(), "0x1502" | "0x17f0"));
        Self {
            cpu_vendor,
            cpu_model: field("model name").unwrap_or_else(|| "unknown CPU".to_owned()),
            arch: std::env::consts::ARCH.to_owned(),
            gpus,
            npu: if intel_npu {
                Npu::Intel
            } else if amd_npu {
                Npu::AmdXdna
            } else {
                Npu::None
            },
            openvino_installed: sources
                .openvino_dir
                .join("runtime/lib/intel64/libopenvino_genai_c.so")
                .exists(),
        }
    }

    pub fn has_intel_gpu(&self) -> bool {
        self.gpus.iter().any(Gpu::is_intel)
    }

    /// Only Intel's NPU and GPUs need OpenVINO.
    pub fn needs_openvino(&self) -> bool {
        self.npu == Npu::Intel || self.has_intel_gpu()
    }

    /// The inventory, as `smart-commit hardware` prints it.
    pub fn inventory(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "  CPU  {} ({})", self.cpu_model, self.arch);
        let _ = writeln!(
            out,
            "       -> {}",
            if self.arch == "x86_64" {
                "cpu (llama.cpp); openvino-cpu where OpenVINO is installed"
            } else {
                "none: smart-commit runs on x86_64 only"
            }
        );
        for gpu in &self.gpus {
            let _ = writeln!(out, "  GPU  {}", gpu.describe());
            let _ = writeln!(out, "       -> {}", gpu.devices());
        }
        if self.gpus.is_empty() {
            let _ = writeln!(out, "  GPU  none");
        }
        match self.npu {
            Npu::Intel => {
                let _ = writeln!(out, "  NPU  Intel NPU\n       -> npu (OpenVINO)");
            }
            Npu::AmdXdna => {
                let _ = writeln!(
                    out,
                    "  NPU  AMD Ryzen AI (XDNA)\n       -> none: not supported"
                );
            }
            Npu::None => {
                let _ = writeln!(out, "  NPU  none");
            }
        }
        out
    }
}

//! The hardware of this machine, as smart-commit sees it: the CPU, every GPU
//! and NPU, their vendor and kernel driver, and which smart-commit devices
//! each one gives. Read from sysfs and /proc/cpuinfo, without root.
//!
//! `scripts/hardware.sh` does the same for the setup scripts; a test checks
//! that both print the same inventory.

use std::fmt::Write;
use std::path::{Path, PathBuf};

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

    /// The inventory, as `scripts/hardware.sh` prints it.
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

struct PciDevice {
    vendor: String,
    device: String,
    driver: String,
    slot: String,
}

/// PCI devices whose class starts with `class_prefix`, in slot order (as the
/// shell's glob lists them).
/// Physical cores, from the CPU topology in sysfs: logical CPUs sharing a
/// core (SMT) list the same `core_cpus_list`. `None` when unreadable.
pub fn physical_cores(sysfs: &Path) -> Option<usize> {
    let dir = std::fs::read_dir(sysfs.join("devices/system/cpu")).ok()?;
    let mut cores = std::collections::BTreeSet::new();
    for entry in dir.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let is_cpu = name
            .strip_prefix("cpu")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        if !is_cpu {
            continue;
        }
        // Older kernels name it thread_siblings_list.
        let topology = entry.path().join("topology");
        let siblings = std::fs::read_to_string(topology.join("core_cpus_list"))
            .or_else(|_| std::fs::read_to_string(topology.join("thread_siblings_list")));
        if let Ok(siblings) = siblings {
            cores.insert(siblings.trim().to_owned());
        }
    }
    (!cores.is_empty()).then_some(cores.len())
}

fn pci_devices(sysfs: &Path, class_prefix: &str) -> Vec<PciDevice> {
    let Ok(dir) = std::fs::read_dir(sysfs.join("bus/pci/devices")) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = dir.flatten().map(|e| e.path()).collect();
    paths.sort();
    let read = |path: &Path, file: &str| {
        std::fs::read_to_string(path.join(file)).map(|s| s.trim().to_owned())
    };
    paths
        .iter()
        .filter_map(|path| {
            let class = read(path, "class").ok()?;
            if !class.starts_with(class_prefix) {
                return None;
            }
            let driver = std::fs::read_link(path.join("driver"))
                .ok()
                .and_then(|l| l.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_else(|| "none".to_owned());
            Some(PciDevice {
                vendor: read(path, "vendor").unwrap_or_default(),
                device: read(path, "device").unwrap_or_else(|_| "0x0000".to_owned()),
                driver,
                slot: path.file_name()?.to_string_lossy().into_owned(),
            })
        })
        .collect()
}

fn lspci_name(slot: &str) -> Option<String> {
    let output = std::process::Command::new("lspci")
        .args(["-vmm", "-s", slot])
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|l| {
            l.strip_prefix("Device:")
                .map(|n| n.trim().to_owned())
                .filter(|n| !n.is_empty())
        })
}

#[cfg(test)]
pub(crate) mod fake {
    //! Fake machines: a sysfs tree and a cpuinfo, like
    //! `tests/scripts/test-setup.sh` builds.

    use std::path::Path;

    /// Adds a PCI device; `driver` `none` leaves it unbound.
    pub fn pci(sysfs: &Path, slot: &str, class: &str, vendor: &str, device: &str, driver: &str) {
        let dev = sysfs.join("bus/pci/devices").join(slot);
        std::fs::create_dir_all(&dev).unwrap();
        std::fs::create_dir_all(sysfs.join("bus/pci/drivers").join(driver)).unwrap();
        std::fs::write(dev.join("class"), format!("{class}\n")).unwrap();
        std::fs::write(dev.join("vendor"), format!("{vendor}\n")).unwrap();
        std::fs::write(dev.join("device"), format!("{device}\n")).unwrap();
        if driver != "none" {
            std::os::unix::fs::symlink(
                format!("../../../bus/pci/drivers/{driver}"),
                dev.join("driver"),
            )
            .unwrap();
        }
    }

    pub fn cpu(root: &Path, vendor: &str, model: &str) {
        std::fs::write(
            root.join("cpuinfo"),
            format!("vendor_id\t: {vendor}\nmodel name\t: {model}\n"),
        )
        .unwrap();
    }

    pub fn amd_nvidia(sysfs: &Path) {
        cpu(sysfs, "AuthenticAMD", "AMD Ryzen 5 5500");
        pci(
            sysfs,
            "0000:01:00.0",
            "0x030000",
            "0x10de",
            "0x2507",
            "nvidia",
        );
        std::fs::create_dir_all(sysfs.join("module/nvidia")).unwrap();
        std::fs::write(sysfs.join("module/nvidia/version"), "595.104.02\n").unwrap();
    }

    pub fn all_intel(sysfs: &Path) {
        cpu(sysfs, "GenuineIntel", "Intel(R) Core(TM) Ultra 5 225H");
        pci(sysfs, "0000:00:02.0", "0x030000", "0x8086", "0x7d51", "xe");
        pci(
            sysfs,
            "0000:00:0b.0",
            "0x120000",
            "0x8086",
            "0xad1d",
            "intel_vpu",
        );
    }

    pub fn amd_only(sysfs: &Path) {
        cpu(sysfs, "AuthenticAMD", "AMD Ryzen 7 8845HS");
        pci(
            sysfs,
            "0000:c4:00.1",
            "0x118000",
            "0x1022",
            "0x1502",
            "amdxdna",
        );
    }

    pub fn old_amd(sysfs: &Path) {
        cpu(sysfs, "AuthenticAMD", "AMD FX-8350");
        pci(
            sysfs,
            "0000:01:00.0",
            "0x030000",
            "0x1002",
            "0x6798",
            "radeon",
        );
        pci(
            sysfs,
            "0000:02:00.0",
            "0x030000",
            "0x10de",
            "0x1c82",
            "none",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Logical CPUs, each with the siblings sharing its core.
    fn topology(sysfs: &Path, file: &str, cpus: &[&str]) {
        for (n, siblings) in cpus.iter().enumerate() {
            let dir = sysfs.join(format!("devices/system/cpu/cpu{n}/topology"));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(file), format!("{siblings}\n")).unwrap();
        }
    }

    #[test]
    fn physical_cores_count_smt_siblings_once() {
        let dir = tempfile::TempDir::new().unwrap();
        // Ryzen 5 5500: 6 cores, 2 threads each; plus non-CPU entries.
        let smt: Vec<String> = (0..12)
            .map(|n| format!("{},{}", n % 6, n % 6 + 6))
            .collect();
        let smt: Vec<&str> = smt.iter().map(String::as_str).collect();
        topology(dir.path(), "core_cpus_list", &smt);
        std::fs::create_dir_all(dir.path().join("devices/system/cpu/cpufreq")).unwrap();
        std::fs::create_dir_all(dir.path().join("devices/system/cpu/cpuidle")).unwrap();
        assert_eq!(physical_cores(dir.path()), Some(6));

        // Hybrid without SMT (Core Ultra 5 225H): every CPU is a core.
        let hybrid = tempfile::TempDir::new().unwrap();
        let cpus: Vec<String> = (0..14).map(|n| n.to_string()).collect();
        let cpus: Vec<&str> = cpus.iter().map(String::as_str).collect();
        topology(hybrid.path(), "core_cpus_list", &cpus);
        assert_eq!(physical_cores(hybrid.path()), Some(14));

        // Older kernels.
        let old = tempfile::TempDir::new().unwrap();
        topology(
            old.path(),
            "thread_siblings_list",
            &["0-1", "0-1", "2-3", "2-3"],
        );
        assert_eq!(physical_cores(old.path()), Some(2));

        let empty = tempfile::TempDir::new().unwrap();
        assert_eq!(physical_cores(empty.path()), None);
    }

    fn machine(build: fn(&Path)) -> (tempfile::TempDir, Hardware) {
        let dir = tempfile::TempDir::new().unwrap();
        let sysfs = dir.path().join("sys");
        std::fs::create_dir_all(sysfs.join("bus/pci/devices")).unwrap();
        build(&sysfs);
        let hardware = Hardware::detect(&Sources {
            cpuinfo: sysfs.join("cpuinfo"),
            sysfs,
            openvino_dir: dir.path().join("no-openvino"),
        });
        (dir, hardware)
    }

    #[test]
    fn an_amd_cpu_with_an_nvidia_gpu_needs_no_openvino() {
        let (_dir, hw) = machine(fake::amd_nvidia);
        assert_eq!(hw.cpu_vendor, CpuVendor::Amd);
        assert_eq!(hw.cpu_model, "AMD Ryzen 5 5500");
        assert_eq!(hw.gpus.len(), 1);
        assert_eq!(
            hw.gpus[0].describe(),
            "NVIDIA device 2507 [0000:01:00.0], driver nvidia 595.104.02"
        );
        assert_eq!(
            hw.gpus[0].devices(),
            "vulkan (the NVIDIA driver's Vulkan ICD)"
        );
        assert_eq!(hw.npu, Npu::None);
        assert!(!hw.needs_openvino());
        assert!(!hw.openvino_installed);
    }

    #[test]
    fn intel_devices_need_openvino() {
        let (_dir, hw) = machine(fake::all_intel);
        assert_eq!(hw.cpu_vendor, CpuVendor::Intel);
        assert!(hw.has_intel_gpu());
        assert_eq!(hw.npu, Npu::Intel);
        assert!(hw.needs_openvino());
        assert_eq!(
            hw.gpus[0].devices(),
            "intel-gpu (OpenVINO), vulkan (Mesa ANV)"
        );
    }

    #[test]
    fn unsupported_devices_say_why() {
        let (_dir, hw) = machine(fake::amd_only);
        assert!(hw.gpus.is_empty());
        assert_eq!(hw.npu, Npu::AmdXdna);
        assert!(!hw.needs_openvino());
        let inventory = hw.inventory();
        assert!(inventory.contains("  GPU  none\n"), "{inventory}");
        assert!(inventory.contains("  NPU  AMD Ryzen AI (XDNA)\n       -> none: not supported"));

        let (_dir, hw) = machine(fake::old_amd);
        assert_eq!(
            hw.gpus[0].devices(),
            "none: the old radeon driver has no Vulkan; amdgpu does"
        );
        assert_eq!(hw.gpus[1].driver, "none");
        assert_eq!(hw.gpus[1].devices(), "none until a kernel driver is bound");
    }

    #[test]
    fn an_empty_machine_is_described_too() {
        let dir = tempfile::TempDir::new().unwrap();
        let hw = Hardware::detect(&Sources {
            sysfs: dir.path().join("sys"),
            cpuinfo: dir.path().join("cpuinfo"),
            openvino_dir: dir.path().join("ov"),
        });
        assert_eq!(hw.cpu_model, "unknown CPU");
        assert_eq!(hw.cpu_vendor, CpuVendor::Other);
        assert!(hw.gpus.is_empty());
    }

    #[test]
    fn openvino_is_found_by_its_library() {
        let (dir, _) = machine(fake::amd_only);
        let lib = dir.path().join("ov/runtime/lib/intel64");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("libopenvino_genai_c.so"), "").unwrap();
        let hw = Hardware::detect(&Sources {
            sysfs: dir.path().join("sys"),
            cpuinfo: dir.path().join("sys/cpuinfo"),
            openvino_dir: dir.path().join("ov"),
        });
        assert!(hw.openvino_installed);
    }
}

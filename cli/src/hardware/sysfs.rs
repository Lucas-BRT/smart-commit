use std::path::{Path, PathBuf};

pub(super) struct PciDevice {
    pub(super) vendor: String,
    pub(super) device: String,
    pub(super) driver: String,
    pub(super) slot: String,
}

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

/// PCI devices whose class starts with `class_prefix`, in slot order (as the
/// shell's glob lists them).
pub(super) fn pci_devices(sysfs: &Path, class_prefix: &str) -> Vec<PciDevice> {
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

pub(super) fn lspci_name(slot: &str) -> Option<String> {
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

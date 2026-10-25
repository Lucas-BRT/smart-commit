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

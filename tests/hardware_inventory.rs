//! `smart-commit hardware` and `scripts/hardware.sh` print the same
//! inventory of the same (fake) machines: the setup screen uses the first,
//! the setup scripts the second.

use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

mod common;

fn pci(sysfs: &Path, slot: &str, class: &str, vendor: &str, device: &str, driver: &str) {
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

fn cpu(sysfs: &Path, vendor: &str, model: &str) {
    std::fs::write(
        sysfs.join("cpuinfo"),
        format!("vendor_id\t: {vendor}\nmodel name\t: {model}\n"),
    )
    .unwrap();
}

/// Runs both on the machine `build` makes; returns their outputs.
fn both(build: impl FnOnce(&Path)) -> (String, String) {
    let dir = TempDir::new().unwrap();
    let sysfs = dir.path().join("sys");
    std::fs::create_dir_all(sysfs.join("bus/pci/devices")).unwrap();
    build(&sysfs);
    let env = [
        ("SMART_COMMIT_SYSFS", sysfs.clone()),
        ("SMART_COMMIT_CPUINFO", sysfs.join("cpuinfo")),
        ("SMART_COMMIT_OPENVINO_DIR", dir.path().join("no-openvino")),
    ];
    let rust = common::isolate(&mut Command::new(env!("CARGO_BIN_EXE_smart-commit")))
        .arg("hardware")
        .envs(env.iter().map(|(k, v)| (k, v)))
        .output()
        .unwrap();
    let script = Command::new("bash")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/hardware.sh"))
        .envs(env.iter().map(|(k, v)| (k, v)))
        .output()
        .unwrap();
    assert!(rust.status.success() && script.status.success());
    (
        String::from_utf8(rust.stdout).unwrap(),
        String::from_utf8(script.stdout).unwrap(),
    )
}

#[test]
fn amd_cpu_and_nvidia_gpu() {
    let (rust, script) = both(|sys| {
        cpu(sys, "AuthenticAMD", "AMD Ryzen 5 5500");
        pci(
            sys,
            "0000:01:00.0",
            "0x030000",
            "0x10de",
            "0x2507",
            "nvidia",
        );
        std::fs::create_dir_all(sys.join("module/nvidia")).unwrap();
        std::fs::write(sys.join("module/nvidia/version"), "595.104.02\n").unwrap();
    });
    assert_eq!(rust, script);
    assert!(rust.contains("GPU  NVIDIA device 2507 [0000:01:00.0], driver nvidia 595.104.02"));
}

#[test]
fn intel_cpu_gpu_and_npu() {
    let (rust, script) = both(|sys| {
        cpu(sys, "GenuineIntel", "Intel(R) Core(TM) Ultra 5 225H");
        pci(sys, "0000:00:02.0", "0x030000", "0x8086", "0x7d51", "xe");
        pci(
            sys,
            "0000:00:0b.0",
            "0x120000",
            "0x8086",
            "0xad1d",
            "intel_vpu",
        );
    });
    assert_eq!(rust, script);
    assert!(rust.contains("NPU  Intel NPU"));
}

#[test]
fn unsupported_and_unbound_devices() {
    let (rust, script) = both(|sys| {
        cpu(sys, "AuthenticAMD", "AMD FX-8350");
        pci(
            sys,
            "0000:01:00.0",
            "0x030000",
            "0x1002",
            "0x6798",
            "radeon",
        );
        pci(sys, "0000:02:00.0", "0x030000", "0x10de", "0x1c82", "none");
        pci(
            sys,
            "0000:c4:00.1",
            "0x118000",
            "0x1022",
            "0x1502",
            "amdxdna",
        );
    });
    assert_eq!(rust, script);
    assert!(rust.contains("NPU  AMD Ryzen AI (XDNA)"));
}

#[test]
fn nothing_but_a_cpu() {
    let (rust, script) = both(|sys| cpu(sys, "AuthenticAMD", "AMD Ryzen 7 8845HS"));
    assert_eq!(rust, script);
    assert!(rust.contains("GPU  none"));
}

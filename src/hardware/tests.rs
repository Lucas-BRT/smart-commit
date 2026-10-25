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

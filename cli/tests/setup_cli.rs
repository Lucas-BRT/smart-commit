//! `smart-commit setup` as text (`--yes`, or without a terminal), on fake
//! machines (a fake /sys and /proc/cpuinfo) with stub setup scripts: it lists
//! the hardware, plans only what that hardware needs, exports the OpenVINO
//! model only for Intel devices, and says what failed instead of stopping.
//! No network, sudo or hardware.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use tempfile::TempDir;

mod common;

struct Machine {
    dir: TempDir,
}

/// Writing a file and running it while another test's child process still
/// holds the descriptor fails with "text file busy"; so one test at a time
/// creates machines and spawns processes.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

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

fn amd_nvidia(sysfs: &Path) {
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

fn all_intel(sysfs: &Path) {
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

fn amd_only(sysfs: &Path) {
    cpu(sysfs, "AuthenticAMD", "AMD Ryzen 7 8845HS");
    // A Ryzen AI NPU, which smart-commit does not use.
    pci(
        sysfs,
        "0000:c4:00.1",
        "0x118000",
        "0x1022",
        "0x1502",
        "amdxdna",
    );
}

fn old_amd(sysfs: &Path) {
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

impl Machine {
    fn new(build: fn(&Path)) -> Self {
        let dir = TempDir::new().unwrap();
        let sysfs = dir.path().join("sys");
        std::fs::create_dir_all(sysfs.join("bus/pci/devices")).unwrap();
        build(&sysfs);
        // The installed layout: prefix/bin and prefix/share/smart-commit/scripts.
        let prefix = dir.path().join("prefix");
        let scripts = prefix.join("share/smart-commit/scripts");
        std::fs::create_dir_all(prefix.join("bin")).unwrap();
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::copy(
            env!("CARGO_BIN_EXE_smart-commit"),
            prefix.join("bin/smart-commit"),
        )
        .unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts");
        std::fs::copy(root.join("setup-gpu.sh"), scripts.join("setup-gpu.sh")).unwrap();
        let stub = |name: &str, body: &str| {
            use std::os::unix::fs::PermissionsExt;
            let path = scripts.join(name);
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        // Neither the NPU stack nor the export is there yet.
        stub("check-npu.sh", "exit 1");
        stub("setup-npu.sh", "echo \"stub setup-npu $*\"");
        stub(
            "export-model.sh",
            "[ \"$1\" = --check ] && exit 1\necho \"stub export-model $*\"",
        );
        Self { dir }
    }

    /// Puts llama.cpp and the default model where the setup finds them, so
    /// that it has nothing to download (the model is a sparse file of the
    /// pinned size, with the sidecar that names the pinned hash).
    fn install_everything(&self) {
        let data = self.dir.path().join("data");
        let server = smart_commit::paths::default_llama_server(|name| {
            (name == "XDG_DATA_HOME").then(|| data.display().to_string())
        })
        .unwrap();
        std::fs::create_dir_all(server.parent().unwrap()).unwrap();
        std::fs::write(&server, "").unwrap();
        let model = smart_commit::models::find(smart_commit::models::DEFAULT).unwrap();
        let file = data.join("smart-commit/models").join(model.gguf.file);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::File::create(&file)
            .unwrap()
            .set_len(model.gguf.size)
            .unwrap();
        std::fs::write(
            smart_commit::models::sidecar(&file),
            format!("{}  {}\n", model.gguf.sha256, model.gguf.file),
        )
        .unwrap();
    }

    fn binary(&self) -> PathBuf {
        self.dir.path().join("prefix/bin/smart-commit")
    }

    /// Runs `setup` with `args`, no terminal; its output.
    fn setup(&self, args: &[&str]) -> String {
        let sysfs = self.dir.path().join("sys");
        let mut command = Command::new(self.binary());
        let output = common::isolate(&mut command)
            .arg("setup")
            .args(args)
            .env("SMART_COMMIT_SYSFS", &sysfs)
            .env("SMART_COMMIT_CPUINFO", sysfs.join("cpuinfo"))
            .env(
                "SMART_COMMIT_OPENVINO_DIR",
                self.dir.path().join("no-openvino"),
            )
            .env("SMART_COMMIT_DPKG_QUERY", "false")
            .env("XDG_DATA_HOME", self.dir.path().join("data"))
            .env("XDG_CONFIG_HOME", self.dir.path().join("config"))
            .env(
                "SMART_COMMIT_LLAMACPP_URL",
                "file:///nonexistent/llama.tar.gz",
            )
            .env("SMART_COMMIT_HF_BASE", "file:///nonexistent/hf")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.status.success(), "setup failed:\n{text}");
        text
    }
}

fn has(output: &str, text: &str) {
    assert!(output.contains(text), "expected {text:?} in:\n{output}");
}

fn lacks(output: &str, text: &str) {
    assert!(!output.contains(text), "unexpected {text:?} in:\n{output}");
}

/// The hardware comes before the plan, and the plan before any step.
fn in_order(output: &str, first: &str, second: &str) {
    let at = |text: &str| {
        output
            .find(text)
            .unwrap_or_else(|| panic!("no {text:?} in:\n{output}"))
    };
    assert!(
        at(first) < at(second),
        "{first:?} before {second:?} in:\n{output}"
    );
}

#[test]
fn an_amd_cpu_with_an_nvidia_gpu_needs_llama_cpp_and_the_vulkan_driver_only() {
    let _serial = serial();
    let out = Machine::new(amd_nvidia).setup(&["--no-bench"]);
    has(&out, "CPU  AMD Ryzen 5 5500");
    has(&out, "-> cpu (llama.cpp)");
    has(
        &out,
        "GPU  NVIDIA device 2507 [0000:01:00.0], driver nvidia 595.104.02",
    );
    has(&out, "-> vulkan (the NVIDIA driver's Vulkan ICD)");
    has(&out, "NPU  none");
    has(
        &out,
        "NVIDIA GPU (0000:01:00.0, proprietary driver 595.104.02)",
    );
    has(
        &out,
        "Not needed: OpenVINO, the Intel NPU driver and Intel's GPU runtime",
    );
    has(&out, "==> llama.cpp");
    lacks(&out, "OpenVINO GenAI 2026.3.1");
    lacks(&out, "OpenVINO model");
    lacks(&out, "stub export-model");
    in_order(&out, "==> Hardware", "==> Plan");
    in_order(&out, "==> Plan", "==> llama.cpp");
    // Nothing needing sudo runs without being asked, and nothing asks here.
    has(
        &out,
        "Set up the GPUs as planned above (may use sudo)? [skipped: no terminal",
    );
}

#[test]
fn an_intel_machine_exports_the_openvino_model_when_told_to() {
    let _serial = serial();
    let out = Machine::new(all_intel).setup(&["--yes", "--no-drivers", "--no-bench"]);
    has(&out, "CPU  Intel(R) Core(TM) Ultra 5 225H");
    has(&out, "-> intel-gpu (OpenVINO), vulkan (Mesa ANV)");
    has(&out, "NPU  Intel NPU");
    has(&out, "-> npu (OpenVINO)");
    has(&out, "the model for OpenVINO");
    has(&out, "stub export-model");
    lacks(&out, "Not needed");
    in_order(&out, "==> OpenVINO model", "stub export-model");
}

#[test]
fn without_yes_and_without_a_terminal_the_export_is_not_started() {
    let _serial = serial();
    let out = Machine::new(all_intel).setup(&["--no-drivers", "--no-bench"]);
    has(&out, "Export the model for OpenVINO here");
    has(
        &out,
        "[skipped: no terminal; run `smart-commit setup` again to do it]",
    );
    lacks(&out, "stub export-model");
}

#[test]
fn an_intel_machine_asks_before_its_sudo_steps() {
    let _serial = serial();
    let out = Machine::new(all_intel).setup(&["--no-bench"]);
    has(
        &out,
        "Install the Intel NPU driver and OpenVINO (uses sudo)? [skipped: no terminal",
    );
    has(
        &out,
        "Set up the GPUs as planned above (may use sudo)? [skipped: no terminal",
    );
    lacks(&out, "stub setup-npu");
    let yes = Machine::new(all_intel).setup(&["--yes", "--no-bench"]);
    has(&yes, "stub setup-npu");
}

#[test]
fn an_amd_npu_is_listed_as_unsupported_and_needs_nothing() {
    let _serial = serial();
    let out = Machine::new(amd_only).setup(&["--no-bench"]);
    has(&out, "GPU  none");
    has(&out, "NPU  AMD Ryzen AI (XDNA)");
    has(&out, "-> none: not supported");
    has(&out, "Not needed: OpenVINO");
    has(&out, "==> llama.cpp");
    lacks(&out, "OpenVINO model");
    lacks(&out, "stub export-model");
}

#[test]
fn gpus_without_vulkan_are_explained() {
    let _serial = serial();
    let out = Machine::new(old_amd).setup(&["--no-bench"]);
    has(
        &out,
        "-> none: the old radeon driver has no Vulkan; amdgpu does",
    );
    has(&out, "-> none until a kernel driver is bound");
    has(&out, "has no driver bound");
    has(&out, "==> llama.cpp");
}

#[test]
fn steps_that_fail_are_reported_and_the_setup_goes_on() {
    let _serial = serial();
    // The URLs are unreachable: the setup says so and still finishes.
    let out = Machine::new(amd_nvidia).setup(&["--no-bench", "--no-drivers"]);
    has(&out, "Could not install llama.cpp");
    has(&out, "Could not install qwen2.5-coder-1.5b");
    has(&out, "run `smart-commit setup` again");
    has(&out, "==> Done");
    in_order(&out, "==> llama.cpp", "==> Done");
}

#[test]
fn a_setup_that_worked_says_what_to_do_next() {
    let _serial = serial();
    let machine = Machine::new(amd_only);
    machine.install_everything();
    let out = machine.setup(&["--no-drivers", "--no-bench"]);
    in_order(&out, "==> Done", "Next:");
    has(&out, "git add -p");
    has(&out, "smart-commit --commit");
    has(&out, "smart-commit hook install");
}

#[test]
fn a_setup_with_a_failed_step_does_not_say_what_to_do_next() {
    let _serial = serial();
    let out = Machine::new(amd_nvidia).setup(&["--no-bench", "--no-drivers"]);
    has(&out, "==> Done");
    lacks(&out, "Next:");
    lacks(&out, "smart-commit hook install");
}

#[test]
fn the_benchmark_runs_last_unless_the_render_group_is_pending() {
    let _serial = serial();
    let out = Machine::new(amd_only).setup(&["--no-drivers"]);
    // Either, depending on the user running the test.
    assert!(
        out.contains("==> Benchmark\n") || out.contains("==> Benchmark postponed"),
        "{out}"
    );
    in_order(&out, "==> llama.cpp", "==> Benchmark");
    in_order(&out, "==> Benchmark", "==> Done");
}

#[test]
fn an_update_only_installs_llama_cpp() {
    let _serial = serial();
    let machine = Machine::new(amd_only);
    let mut command = Command::new(machine.binary());
    let output = common::isolate(&mut command)
        .args(["setup", "--llamacpp-only"])
        .env("XDG_DATA_HOME", machine.dir.path().join("data"))
        .env(
            "SMART_COMMIT_LLAMACPP_URL",
            "file:///nonexistent/llama.tar.gz",
        )
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(
        err.contains("cannot download file:///nonexistent/llama.tar.gz"),
        "{err}"
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("==> Hardware"));
}

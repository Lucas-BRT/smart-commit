use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use smart_commit::backend::device::Target;
use smart_commit::bench::{self, Entry, Machine, Measurement, Report};
use smart_commit::config::{Resolved, Settings, Sources};
use smart_commit::hardware::{CpuVendor, Gpu, Hardware, Npu};
use smart_commit::hook;
use smart_commit::models;
use tempfile::TempDir;

use super::checks::*;

fn level_of(findings: &[Finding]) -> Vec<Level> {
    findings.iter().map(|f| f.level).collect()
}

fn one(finding: Finding, level: Level, text: &str) {
    assert_eq!(finding.level, level, "{finding:?}");
    assert!(finding.detail.contains(text), "{finding:?}");
}

fn dir(path: &str) -> PathBuf {
    PathBuf::from(path)
}

fn hardware(npu: Npu, gpus: Vec<Gpu>) -> Hardware {
    Hardware {
        cpu_vendor: CpuVendor::Other,
        cpu_model: "test".to_owned(),
        arch: "x86_64".to_owned(),
        gpus,
        npu,
        openvino_installed: false,
    }
}

fn amd_gpu() -> Gpu {
    Gpu {
        vendor: "0x1002".to_owned(),
        device: "0x1".to_owned(),
        driver: "amdgpu".to_owned(),
        slot: "0000:01:00.0".to_owned(),
        name: None,
        nvidia_version: None,
    }
}

/// Settings from a global config file with `toml` in a fake home.
fn settings(home: &TempDir, toml: &str) -> Settings {
    let file = home.path().join("config.toml");
    fs::write(&file, toml).unwrap();
    let env = |name: &str| match name {
        "HOME" => Some(home.path().to_string_lossy().into_owned()),
        _ => None,
    };
    Resolved::resolve(&Sources {
        global: Some(file),
        repo: None,
        env: &env,
        flags: Vec::new(),
    })
    .unwrap()
    .settings()
}

fn report(entries: Vec<(Target, f64, Option<&str>)>) -> Report {
    Report {
        format: bench::FORMAT,
        tool_version: "0".to_owned(),
        machine: Machine {
            cpu: "cpu".to_owned(),
            kernel: "k".to_owned(),
        },
        entries: entries
            .into_iter()
            .map(|(target, ms, error)| Entry {
                target,
                backend: None,
                measurement: error.is_none().then(|| Measurement {
                    load_ms: 0.0,
                    generate_ms: 0.0,
                    per_message_ms: ms,
                    cpu_cores: 1.0,
                    replies: Vec::new(),
                }),
                error: error.map(str::to_owned),
                fast_load: None,
            })
            .collect(),
    }
}

#[test]
fn render_aligns_tags_and_exit_ignores_warnings() {
    let findings = vec![
        Finding::ok_for_test("path"),
        git(None),
        binary_on_path(Some(&dir("/a")), &[]),
    ];
    let text = render(&findings);
    assert!(text.starts_with("ok    path       fine\n"), "{text}");
    assert!(
        text.contains("\nfail  git        git is not on PATH - "),
        "{text}"
    );
    assert!(
        text.contains("\nwarn  path       /a is not on PATH - "),
        "{text}"
    );
    assert!(!healthy(&findings));
    assert!(healthy(&findings[2..]));
}

impl Finding {
    fn ok_for_test(check: &'static str) -> Self {
        Self {
            level: Level::Ok,
            check,
            detail: "fine".to_owned(),
        }
    }
}

#[test]
fn path_check() {
    let bin = dir("/home/me/.local/bin");
    one(
        binary_on_path(Some(&bin), &[dir("/usr/bin"), bin.clone()]),
        Level::Ok,
        "is on PATH",
    );
    let missing = binary_on_path(Some(&bin), &[dir("/usr/bin")]);
    one(missing.clone(), Level::Warn, "is not on PATH");
    assert!(
        missing
            .detail
            .contains("export PATH=\"/home/me/.local/bin:$PATH\"")
    );
    one(binary_on_path(None, &[]), Level::Warn, "cannot tell");
}

#[test]
fn git_check() {
    one(git(Some("git version 2.43.0\n")), Level::Ok, "git 2.43");
    one(
        git(Some("git version 2.39.3 (Apple Git-145)\n")),
        Level::Ok,
        "git 2.39",
    );
    one(
        git(Some("git version 2.19.1\n")),
        Level::Warn,
        "older than 2.20",
    );
    one(git(Some("git version 1.8.3\n")), Level::Warn, "older");
    one(git(Some("git version 3.0.0\n")), Level::Ok, "git 3.0");
    one(git(Some("what\n")), Level::Warn, "cannot read the version");
    one(git(None), Level::Fail, "not on PATH");
}

fn config_input<'a>(home: &TempDir, env: &'a dyn Fn(&str) -> Option<String>) -> ConfigInput<'a> {
    ConfigInput {
        global: Some(home.path().join("global.toml")),
        repo: Some(home.path().join("repo.toml")),
        env,
    }
}

fn no_env(_: &str) -> Option<String> {
    None
}

#[test]
fn config_without_files_is_the_defaults() {
    let home = TempDir::new().unwrap();
    let (findings, settings) = config(&config_input(&home, &no_env));
    assert_eq!(level_of(&findings), [Level::Ok]);
    assert!(findings[0].detail.contains("defaults"));
    assert!(settings.is_some());
}

#[test]
fn config_files_that_parse_are_ok() {
    let home = TempDir::new().unwrap();
    fs::write(
        home.path().join("global.toml"),
        "[model]\ndevice = \"cpu\"\n",
    )
    .unwrap();
    fs::write(
        home.path().join("repo.toml"),
        "[style]\nconvention = \"conventional\"\n",
    )
    .unwrap();
    let (findings, settings) = config(&config_input(&home, &no_env));
    assert_eq!(level_of(&findings), [Level::Ok, Level::Ok]);
    assert!(findings[0].detail.contains("global.toml"));
    assert!(settings.is_some());
}

#[test]
fn config_that_does_not_parse_fails_and_keeps_the_rest_usable() {
    let home = TempDir::new().unwrap();
    fs::write(home.path().join("global.toml"), "[model\n").unwrap();
    fs::write(home.path().join("repo.toml"), "[nope]\nx = 1\n").unwrap();
    let (findings, settings) = config(&config_input(&home, &no_env));
    assert_eq!(level_of(&findings), [Level::Fail, Level::Fail]);
    assert!(findings[0].detail.contains("global.toml"), "{findings:?}");
    assert!(
        findings[1].detail.contains("unknown key `nope.x`"),
        "{findings:?}"
    );
    assert!(!findings[1].detail.contains("known keys"), "{findings:?}");
    assert!(settings.is_some(), "the defaults still apply");
}

#[test]
fn config_with_a_value_it_cannot_read_fails() {
    let home = TempDir::new().unwrap();
    fs::write(
        home.path().join("global.toml"),
        "[model]\ndevice = \"tpu\"\n",
    )
    .unwrap();
    let (findings, _) = config(&config_input(&home, &no_env));
    one(findings[0].clone(), Level::Fail, "model.device");
}

#[test]
fn config_warns_about_retired_keys_and_old_names() {
    let home = TempDir::new().unwrap();
    fs::write(
        home.path().join("global.toml"),
        "[style]\nlanguage = \"pt\"\n[model]\ndevice = \"gpu\"\n",
    )
    .unwrap();
    let (findings, settings) = config(&config_input(&home, &no_env));
    assert_eq!(level_of(&findings), [Level::Warn, Level::Warn]);
    assert!(
        findings[0].detail.contains("style.language"),
        "{findings:?}"
    );
    assert!(
        findings[1].detail.contains("old name of intel-gpu"),
        "{findings:?}"
    );
    assert!(settings.is_some());
}

#[test]
fn config_fails_on_a_bad_environment() {
    let home = TempDir::new().unwrap();
    let env = |name: &str| (name == "SMART_COMMIT_DEVICE").then(|| "tpu".to_owned());
    let (findings, settings) = config(&config_input(&home, &env));
    assert!(
        findings.iter().any(|f| f.level == Level::Fail),
        "{findings:?}"
    );
    assert!(settings.is_none());
}

#[test]
fn device_in_use_follows_the_setting_the_benchmark_and_the_hardware() {
    let home = TempDir::new().unwrap();
    let auto = settings(&home, "");
    let gpu_machine = hardware(Npu::None, vec![amd_gpu()]);
    // No benchmark: the default order among what is present.
    assert_eq!(device_in_use(&auto, &gpu_machine, None), Ok(Target::Vulkan));
    // The benchmark ranks the CPU first.
    let ranked = report(vec![
        (Target::Vulkan, 900.0, None),
        (Target::Cpu, 400.0, None),
    ]);
    assert_eq!(
        device_in_use(&auto, &gpu_machine, Some(&ranked)),
        Ok(Target::Cpu)
    );
    // A device set wins, present or not.
    let fixed = settings(&home, "[model]\ndevice = \"npu\"\n");
    assert_eq!(
        device_in_use(&fixed, &gpu_machine, Some(&ranked)),
        Ok(Target::Npu)
    );
    let mock = settings(&home, "backend = \"mock\"\n");
    assert!(device_in_use(&mock, &gpu_machine, None).is_err());
    let wrong = settings(&home, "backend = \"llamacpp\"\n[model]\ndevice = \"npu\"\n");
    assert!(
        device_in_use(&wrong, &gpu_machine, None)
            .unwrap_err()
            .contains("needs backend")
    );
}

#[test]
fn device_lines_say_where_it_runs_and_whether_it_was_measured() {
    let home = TempDir::new().unwrap();
    let machine = hardware(Npu::None, vec![amd_gpu()]);
    let auto = settings(&home, "");

    let lines = device_lines(&auto, &Ok(Target::Vulkan), &machine, None);
    assert_eq!(level_of(&lines), [Level::Ok, Level::Warn]);
    assert!(
        lines[0].detail.contains("vulkan (auto: no benchmark"),
        "{lines:?}"
    );
    assert!(lines[1].detail.contains("smart-commit bench"), "{lines:?}");

    let measured = report(vec![(Target::Vulkan, 500.0, None)]);
    let lines = device_lines(&auto, &Ok(Target::Vulkan), &machine, Some(&measured));
    assert_eq!(level_of(&lines), [Level::Ok, Level::Ok]);
    assert!(
        lines[0].detail.contains("fastest in the last benchmark"),
        "{lines:?}"
    );
    assert!(lines[1].detail.contains("1 device"), "{lines:?}");

    let broken = report(vec![(Target::Vulkan, 0.0, Some("no driver"))]);
    let lines = device_lines(&auto, &Ok(Target::Vulkan), &machine, Some(&broken));
    assert_eq!(level_of(&lines), [Level::Ok, Level::Warn]);
    assert!(lines[1].detail.contains("no driver"), "{lines:?}");

    let fixed = settings(&home, "[model]\ndevice = \"vulkan\"\n");
    let lines = device_lines(&fixed, &Ok(Target::Vulkan), &machine, None);
    assert!(lines[0].detail.contains("(model.device)"), "{lines:?}");
}

#[test]
fn device_lines_fail_for_a_device_the_machine_lacks() {
    let home = TempDir::new().unwrap();
    let machine = hardware(Npu::None, Vec::new());
    let fixed = settings(&home, "[model]\ndevice = \"npu\"\n");
    let lines = device_lines(&fixed, &Ok(Target::Npu), &machine, None);
    one(lines[0].clone(), Level::Fail, "npu is not available");
    let mock = settings(&home, "backend = \"mock\"\n");
    let lines = device_lines(&mock, &Err("mock".to_owned()), &machine, None);
    one(lines[0].clone(), Level::Ok, "backend is mock");
    let lines = device_lines(&fixed, &Err("no device".to_owned()), &machine, None);
    one(lines[0].clone(), Level::Fail, "no device");
}

/// The pinned GGUF of the default model, as `install_gguf` leaves it.
fn install_gguf(home: &TempDir, size: u64, record: Option<&str>) -> Settings {
    let model = models::find(models::DEFAULT).unwrap();
    let file = home.path().join(model.gguf.file);
    fs::File::create(&file).unwrap().set_len(size).unwrap();
    if let Some(record) = record {
        fs::write(models::sidecar(&file), record).unwrap();
    }
    settings(home, &format!("[model]\ngguf = \"{}\"\n", file.display()))
}

fn pinned() -> &'static models::Gguf {
    &models::find(models::DEFAULT).unwrap().gguf
}

fn model_of(settings: &Settings, thorough: bool) -> Finding {
    let lines = model(settings, &Ok(Target::Cpu), thorough);
    assert_eq!(lines.len(), 1);
    lines[0].clone()
}

#[test]
fn model_missing_fails_with_the_fix() {
    let home = TempDir::new().unwrap();
    let missing = settings(&home, "");
    let finding = model_of(&missing, false);
    one(finding.clone(), Level::Fail, "no GGUF model at");
    assert!(finding.detail.contains("smart-commit setup"));
}

#[test]
fn model_installed_is_ok_by_size_and_record() {
    let home = TempDir::new().unwrap();
    let installed = install_gguf(&home, pinned().size, Some(pinned().sha256));
    one(
        model_of(&installed, false),
        Level::Ok,
        "size and record match",
    );
}

#[test]
fn model_with_the_wrong_size_or_record_fails() {
    let home = TempDir::new().unwrap();
    let truncated = install_gguf(&home, pinned().size - 1, Some(pinned().sha256));
    let finding = model_of(&truncated, false);
    one(finding.clone(), Level::Fail, "bytes, not the pinned");
    assert!(
        finding
            .detail
            .contains("smart-commit model use qwen2.5-coder-1.5b")
    );
    let other = install_gguf(&home, pinned().size, Some("0000 file"));
    one(model_of(&other, false), Level::Fail, "not the pinned one");
}

#[test]
fn model_without_a_record_warns() {
    let home = TempDir::new().unwrap();
    let unrecorded = install_gguf(&home, pinned().size, None);
    one(
        model_of(&unrecorded, false),
        Level::Warn,
        "no .sha256 record",
    );
}

#[test]
fn model_is_hashed_only_when_asked() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("m.gguf");
    // The SHA-256 of "hello".
    let hello = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
    let pin = models::Gguf {
        repo: "r",
        revision: "v",
        file: "m.gguf",
        size: 5,
        sha256: hello,
    };
    fs::write(models::sidecar(&file), hello).unwrap();
    // Same size and record, other content: only hashing tells.
    fs::write(&file, "world").unwrap();
    one(
        verify(&file, "id", &pin, false),
        Level::Ok,
        "size and record match",
    );
    one(
        verify(&file, "id", &pin, true),
        Level::Fail,
        "SHA-256 differs",
    );
    fs::write(&file, "hello").unwrap();
    one(
        verify(&file, "id", &pin, true),
        Level::Ok,
        "SHA-256 matches",
    );
}

#[test]
fn model_of_your_own_must_be_a_gguf() {
    let home = TempDir::new().unwrap();
    let file = home.path().join("mine.gguf");
    fs::write(&file, b"GGUF....").unwrap();
    let mine = settings(&home, &format!("[model]\ngguf = \"{}\"\n", file.display()));
    one(model_of(&mine, false), Level::Ok, "a file of yours");
    fs::write(&file, b"nope").unwrap();
    one(model_of(&mine, false), Level::Fail, "not a GGUF file");
}

#[test]
fn model_for_openvino_needs_its_export() {
    let home = TempDir::new().unwrap();
    let exported = home.path().join("export");
    fs::create_dir(&exported).unwrap();
    let dir = settings(
        &home,
        &format!("[model]\ndir = \"{}\"\n", exported.display()),
    );
    let check = |s: &Settings| model(s, &Ok(Target::Npu), false)[0].clone();
    one(check(&dir), Level::Fail, "no OpenVINO model in");
    fs::write(exported.join("openvino_model.xml"), "").unwrap();
    one(check(&dir), Level::Ok, "export");
    // A model without an export has no directory at all.
    let without = settings(&home, "[model]\nname = \"smart-commit-1.5b-v0.1\"\n");
    one(check(&without), Level::Fail, "has no OpenVINO version");
}

#[test]
fn model_is_not_checked_without_a_device() {
    let home = TempDir::new().unwrap();
    let mock = settings(&home, "backend = \"mock\"\n");
    one(
        model(&mock, &Err("mock".into()), false)[0].clone(),
        Level::Ok,
        "mock",
    );
    let auto = settings(&home, "");
    one(
        model(&auto, &Err("x".into()), false)[0].clone(),
        Level::Ok,
        "no device",
    );
}

#[test]
fn llama_server_present_missing_and_other_build() {
    let home = TempDir::new().unwrap();
    let machine = hardware(Npu::None, Vec::new());
    let build = smart_commit::backend::llamacpp::LLAMA_CPP_BUILD;
    let at = |path: &Path| {
        settings(
            &home,
            &format!("[model]\nllama_server = \"{}\"\n", path.display()),
        )
    };
    let pinned_dir = home.path().join(format!("llama-{build}"));
    fs::create_dir(&pinned_dir).unwrap();
    let server = pinned_dir.join("llama-server");
    one(
        llama_cpp(&at(&server), &machine),
        Level::Warn,
        "no llama-server at",
    );
    fs::write(&server, "").unwrap();
    one(llama_cpp(&at(&server), &machine), Level::Ok, "llama-server");
    let old_dir = home.path().join("llama-b1");
    fs::create_dir(&old_dir).unwrap();
    fs::write(old_dir.join("llama-server"), "").unwrap();
    one(
        llama_cpp(&at(&old_dir.join("llama-server")), &machine),
        Level::Warn,
        "not the build this release pins",
    );
    // Nothing of this machine uses it.
    let openvino = settings(&home, "backend = \"openvino\"\n");
    one(llama_cpp(&openvino, &machine), Level::Ok, "not needed");
    let arm = Hardware {
        arch: "aarch64".to_owned(),
        ..machine
    };
    one(llama_cpp(&at(&server), &arm), Level::Ok, "not needed");
}

fn kept(idle: u64) -> KeptServer {
    KeptServer {
        model: "m.gguf".to_owned(),
        device: "vulkan".to_owned(),
        idle: Duration::from_secs(idle),
        stops_after: Duration::from_secs(600),
    }
}

#[test]
fn server_lines() {
    let mut input = ServerInput {
        keep_loaded: Duration::from_secs(600),
        runtime_dir_set: true,
        kept: Vec::new(),
        stale: Vec::new(),
    };
    one(server(&input)[0].clone(), Level::Ok, "none running");
    input.runtime_dir_set = false;
    one(server(&input)[0].clone(), Level::Warn, "XDG_RUNTIME_DIR");
    input.keep_loaded = Duration::ZERO;
    one(server(&input)[0].clone(), Level::Ok, "off");

    input.keep_loaded = Duration::from_secs(600);
    input.runtime_dir_set = true;
    input.kept = vec![kept(42)];
    input.stale = vec![dir("/run/user/1/smart-commit/abc")];
    let lines = server(&input);
    assert_eq!(level_of(&lines), [Level::Ok, Level::Warn]);
    assert!(
        lines[0]
            .detail
            .contains("m.gguf on vulkan: idle 42 s, stops after 600 s idle")
    );
    assert!(lines[1].detail.contains("abc") && lines[1].detail.contains("server stop"));
}

fn hook_input(home: &TempDir, text: Option<&str>, exe: &Path) -> HookInput {
    HookInput {
        hooks_dir: home.path().join("hooks"),
        hooks_path: None,
        default_dir: home.path().join(".git/hooks"),
        hook: text.map(|text| HookFile {
            text: text.to_owned(),
            executable: true,
        }),
        default_hook: None,
        exe: Some(exe.to_path_buf()),
    }
}

fn binary(home: &TempDir, name: &str) -> PathBuf {
    let path = home.path().join(name);
    fs::write(&path, "").unwrap();
    path
}

#[test]
fn hook_installed_for_this_binary_is_ok() {
    let home = TempDir::new().unwrap();
    let exe = binary(&home, "smart-commit");
    let input = hook_input(&home, Some(&hook::script(&exe)), &exe);
    one(hook(&input), Level::Ok, "runs this binary");
    // A path with a quote in it is read back.
    let odd = binary(&home, "it's");
    one(
        hook(&hook_input(&home, Some(&hook::script(&odd)), &odd)),
        Level::Ok,
        "runs this binary",
    );
}

#[test]
fn hook_missing_or_foreign_warns() {
    let home = TempDir::new().unwrap();
    let exe = binary(&home, "smart-commit");
    let missing = hook(&hook_input(&home, None, &exe));
    one(missing, Level::Warn, "smart-commit hook install");
    let foreign = hook(&hook_input(&home, Some("#!/bin/sh\nlint\n"), &exe));
    one(foreign, Level::Warn, "another hook");
    assert!(foreign_fix(&home, &exe).contains("--force"));
}

fn foreign_fix(home: &TempDir, exe: &Path) -> String {
    hook(&hook_input(home, Some("#!/bin/sh\nlint\n"), exe)).detail
}

#[test]
fn hook_pointing_elsewhere_or_not_executable_warns() {
    let home = TempDir::new().unwrap();
    let exe = binary(&home, "smart-commit");
    let other = binary(&home, "other");
    one(
        hook(&hook_input(&home, Some(&hook::script(&other)), &exe)),
        Level::Warn,
        "not this binary",
    );
    one(
        hook(&hook_input(
            &home,
            Some(&hook::script(&home.path().join("gone"))),
            &exe,
        )),
        Level::Warn,
        "does not exist",
    );
    let mut input = hook_input(&home, Some(&hook::script(&exe)), &exe);
    input.hook.as_mut().unwrap().executable = false;
    one(hook(&input), Level::Warn, "not executable");
}

#[test]
fn hook_ignored_because_of_core_hooks_path_warns() {
    let home = TempDir::new().unwrap();
    let exe = binary(&home, "smart-commit");
    let mut input = hook_input(&home, None, &exe);
    input.hooks_path = Some(".githooks".to_owned());
    input.default_hook = Some(HookFile {
        text: hook::script(&exe),
        executable: true,
    });
    let finding = hook(&input);
    one(finding.clone(), Level::Warn, "core.hooksPath is .githooks");
    assert!(finding.detail.contains("hooks"), "{finding:?}");
}

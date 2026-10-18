//! Runs the `smart-commit` binary end to end.

use std::path::Path;
use std::process::Output;

mod common;
use common::TestRepo;

fn smart_commit(args: &[&str], dir: &Path) -> Output {
    smart_commit_with(args, dir, &[])
}

fn smart_commit_with(args: &[&str], dir: &Path, env: &[(&str, &str)]) -> Output {
    common::smart_commit(args, dir, env)
}

/// A repository with a conventional history and one staged file.
fn staged_repo() -> TestRepo {
    let repo = TestRepo::new();
    repo.commit("feat(git): add history");
    repo.commit("fix(git): handle detached HEAD");
    repo.write("main.rs", "fn main() {}\n");
    repo.git(&["add", "main.rs"]);
    repo
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

#[test]
fn invalid_arguments_are_usage_errors() {
    let repo = TestRepo::new();
    for args in [
        &["frobnicate"][..],
        &["context", "a", "b"],
        &["--backend", "gpu"],
        &["--device", "tpu"],
        &["context", "--commit"],
    ] {
        let output = smart_commit(args, repo.path());
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(stderr(&output).starts_with("error:"), "{args:?}");
    }
}

#[test]
fn context_outside_a_repository_fails() {
    let dir = tempfile::TempDir::new().unwrap();
    let output = smart_commit(&["context"], dir.path());
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("not a git repository"));
}

#[test]
fn context_reports_the_detected_style() {
    let repo = TestRepo::new();
    repo.git(&["checkout", "--quiet", "-b", "feature/NPU-7-x"]);
    repo.commit("feat(cli): adiciona o comando context");
    repo.commit("fix(git): corrige a leitura da branch");

    let output = smart_commit(&["context"], repo.path());
    assert!(output.status.success(), "{}", stderr(&output));
    let out = stdout(&output);
    assert!(out.contains("branch:      feature/NPU-7-x"));
    assert!(out.contains("ticket:      NPU-7"));
    assert!(out.contains("convention:  Conventional (from 2 commits)"));
    assert!(out.contains("language:    Portuguese"));
    assert!(out.contains("scopes:      cli (1), git (1)"));
}

#[test]
fn context_accepts_a_path() {
    let repo = TestRepo::new();
    let elsewhere = tempfile::TempDir::new().unwrap();
    let path = repo.path().to_str().unwrap();
    let output = smart_commit(&["context", "-C", path], elsewhere.path());
    assert!(output.status.success(), "{}", stderr(&output));
}

#[test]
fn prompt_without_staged_changes_fails() {
    let repo = TestRepo::new();
    repo.commit("init");
    let output = smart_commit(&["prompt"], repo.path());
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(stderr(&output), "error: nothing staged\n");
}

#[test]
fn prompt_shows_style_and_staged_diff() {
    let repo = TestRepo::new();
    repo.commit("Add the parser for the diff");
    repo.commit("Fix the crash when the diff is empty");
    repo.write("main.rs", "fn main() {}\n");
    repo.git(&["add", "main.rs"]);

    let output = smart_commit(&["prompt"], repo.path());
    assert!(output.status.success(), "{}", stderr(&output));
    let out = stdout(&output);
    assert!(out.contains("Write in English"));
    assert!(out.contains("Fix the crash when the diff is empty"));
    assert!(out.contains("main.rs | +1 -0"));
    assert!(out.contains("+fn main() {}"));
    assert!(out.contains("of 1024 tokens ====="));
}

/// The inputs of a mock generation in [`staged_repo`] as one JSON line, and
/// the prompt smart-commit sent for them.
fn recorded_inputs(repo: &TestRepo) -> (String, serde_json::Value) {
    let trace = repo.path().join("trace.json");
    let output = smart_commit(
        &[
            "--backend",
            "mock",
            "--mock-response",
            "feat: add the entry point",
            "--trace",
            trace.to_str().unwrap(),
        ],
        repo.path(),
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let trace: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&trace).unwrap()).unwrap();
    (
        trace["inputs"].to_string(),
        trace["generation"]["attempts"][0]["prompt"].clone(),
    )
}

#[test]
fn prompt_from_inputs_matches_the_prompt_sent() {
    let repo = staged_repo();
    let (inputs, prompt) = recorded_inputs(&repo);
    let elsewhere = tempfile::TempDir::new().unwrap();
    let file = elsewhere.path().join("inputs.jsonl");
    std::fs::write(&file, format!("{inputs}\n\n{inputs}\n")).unwrap();

    let output = smart_commit(
        &["prompt", "--inputs", file.to_str().unwrap()],
        elsewhere.path(),
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let lines: Vec<serde_json::Value> = stdout(&output)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines, [prompt.clone(), prompt]);
}

#[test]
fn prompt_from_inputs_reads_stdin_and_global_settings() {
    let repo = staged_repo();
    let (inputs, _) = recorded_inputs(&repo);
    let config_home = tempfile::TempDir::new().unwrap();
    let config = config_home.path().join("smart-commit/config.toml");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(&config, "[style]\nlanguage = \"portuguese\"\n").unwrap();

    let mut child = common::isolate(
        std::process::Command::new(env!("CARGO_BIN_EXE_smart-commit"))
            .args(["prompt", "--inputs", "-"])
            .current_dir(config_home.path()),
    )
    .env("XDG_CONFIG_HOME", config_home.path())
    .stdin(std::process::Stdio::piped())
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped())
    .spawn()
    .unwrap();
    std::io::Write::write_all(&mut child.stdin.take().unwrap(), inputs.as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert_eq!(stderr(&output), "");
    let prompt: serde_json::Value = serde_json::from_str(stdout(&output).trim()).unwrap();
    assert!(
        prompt["system"]
            .as_str()
            .unwrap()
            .contains("Brazilian Portuguese")
    );
}

#[test]
fn prompt_from_empty_inputs_prints_nothing() {
    let dir = tempfile::TempDir::new().unwrap();
    let file = dir.path().join("inputs.jsonl");
    std::fs::write(&file, "").unwrap();
    let output = smart_commit(&["prompt", "--inputs", file.to_str().unwrap()], dir.path());
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "");
}

#[test]
fn prompt_from_invalid_inputs_names_the_line() {
    let repo = staged_repo();
    let (inputs, _) = recorded_inputs(&repo);
    let dir = tempfile::TempDir::new().unwrap();
    let file = dir.path().join("inputs.jsonl");
    std::fs::write(&file, format!("{inputs}\n{{\"branch\": null}}\n")).unwrap();
    let path = file.to_str().unwrap();

    let output = smart_commit(&["prompt", "--inputs", path], dir.path());
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).starts_with(&format!("error: {path}:2: invalid inputs: missing field")),
        "{}",
        stderr(&output)
    );

    let missing = dir.path().join("missing.jsonl");
    let output = smart_commit(
        &["prompt", "--inputs", missing.to_str().unwrap()],
        dir.path(),
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("cannot read"));

    let output = smart_commit(&["prompt", "--inputs", path, "-C", "."], dir.path());
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn lockfiles_alone_get_a_message_without_loading_a_model() {
    let repo = TestRepo::new();
    repo.commit("feat: Add a simple hello world program");
    repo.write(
        "Cargo.lock",
        "# This file is automatically @generated by Cargo.\nversion = 4\n",
    );
    repo.git(&["add", "Cargo.lock"]);
    // A server that does not exist: loading a model would fail.
    let output = smart_commit_with(
        &["--backend", "llamacpp", "--device", "cpu"],
        repo.path(),
        &[("SMART_COMMIT_LLAMA_SERVER", "/nonexistent/llama-server")],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "chore: add Cargo.lock\n");
    assert_eq!(stderr(&output), "");
}

#[test]
fn repeating_the_history_twice_says_so() {
    let repo = staged_repo();
    let output = smart_commit(
        &[
            "--backend",
            "mock",
            "--mock-response",
            "fix(git): handle detached HEAD",
            "--mock-response",
            "fix(git): handle detached HEAD",
        ],
        repo.path(),
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stderr(&output).starts_with("warning: the model only repeated earlier commit messages"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn generate_prints_the_sanitized_reply() {
    let repo = staged_repo();
    let reply = "Here is the commit message:\n```\nFeat(cli): add the main entry point.\n```";
    let output = smart_commit(
        &["--backend", "mock", "--mock-response", reply],
        repo.path(),
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "feat(cli): add the main entry point\n");
    assert_eq!(stderr(&output), "");
}

#[test]
fn generate_retries_then_falls_back_with_a_warning() {
    let repo = staged_repo();
    let output = smart_commit(
        &[
            "--backend",
            "mock",
            "--mock-response",
            "Add the entry point",
        ],
        repo.path(),
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "chore: add main.rs\n");
    assert!(stderr(&output).contains("unusable after 2 attempts"));
}

#[test]
fn generate_uses_the_second_reply_when_the_first_is_rejected() {
    let repo = staged_repo();
    let output = smart_commit(
        &[
            "--backend",
            "mock",
            "--mock-response",
            "Add the entry point",
            "--mock-response",
            "feat: add the entry point",
        ],
        repo.path(),
    );
    assert_eq!(stdout(&output), "feat: add the entry point\n");
    assert_eq!(stderr(&output), "");
}

#[test]
fn auto_without_any_model_lists_every_device() {
    let repo = staged_repo();
    let output = smart_commit(&[], repo.path());
    assert_eq!(output.status.code(), Some(1));
    let env = |name: &str| (name == "XDG_DATA_HOME").then(|| common::DATA_HOME.to_owned());
    let ir = smart_commit::paths::default_model_dir(env)
        .unwrap()
        .display()
        .to_string();
    let gguf = smart_commit::paths::default_gguf_file(env)
        .unwrap()
        .display()
        .to_string();
    let no_ir = format!("backend unavailable: no model in {ir}; `smart-commit setup` downloads it");
    let no_gguf =
        format!("backend unavailable: no GGUF model at {gguf}; `smart-commit setup` downloads it");
    assert_eq!(
        stderr(&output),
        format!(
            "error: no device could run the model:\n  intel-gpu: {no_ir}\n  npu: {no_ir}\n  \
             vulkan: {no_gguf}\n  cpu: {no_gguf}\n  openvino-cpu: {no_ir}\n"
        )
    );
}

#[test]
fn a_fixed_device_reports_only_its_own_problem() {
    let repo = staged_repo();
    let output = smart_commit(&["--device", "openvino-cpu"], repo.path());
    assert!(
        stderr(&output).starts_with("error: backend unavailable: no model in"),
        "{}",
        stderr(&output)
    );
    let output = smart_commit(&["--device", "cpu"], repo.path());
    assert!(
        stderr(&output).starts_with("error: backend unavailable: no GGUF model at"),
        "{}",
        stderr(&output)
    );
    // The old name of intel-gpu is still a valid flag.
    let output = smart_commit(&["--device", "gpu"], repo.path());
    assert!(
        stderr(&output).starts_with("error: backend unavailable: no model in"),
        "{}",
        stderr(&output)
    );
    let output = smart_commit(&["--device", "vulkan"], repo.path());
    assert!(
        stderr(&output).starts_with("error: backend unavailable: no GGUF model at"),
        "{}",
        stderr(&output)
    );
    let output = smart_commit(
        &["--backend", "openvino", "--device", "vulkan"],
        repo.path(),
    );
    assert_eq!(
        stderr(&output),
        "error: model.device = vulkan needs backend = llamacpp (or auto)\n"
    );
}

#[test]
fn model_dir_can_be_given() {
    let repo = staged_repo();
    let empty = tempfile::TempDir::new().unwrap();
    let dir = empty.path().to_str().unwrap();
    for (args, env) in [
        (vec!["--model-dir", dir], vec![]),
        (vec![], vec![("SMART_COMMIT_MODEL_DIR", dir)]),
    ] {
        let output = smart_commit_with(&args, repo.path(), &env);
        assert_eq!(output.status.code(), Some(1));
        assert!(
            stderr(&output).contains(&format!("no model in {dir};")),
            "{}",
            stderr(&output)
        );
    }
}

#[test]
fn replay_with_model_needs_the_model() {
    let repo = staged_repo();
    let trace = repo.path().join("trace.json");
    let path = trace.to_str().unwrap();
    smart_commit(
        &[
            "--backend",
            "mock",
            "--mock-response",
            "feat: x y",
            "--trace",
            path,
        ],
        repo.path(),
    );
    let output = smart_commit(&["replay", path, "--with-model"], repo.path());
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("no model in"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn generate_with_nothing_staged_fails() {
    let repo = TestRepo::new();
    repo.commit("init");
    let output = smart_commit(&["--backend", "mock"], repo.path());
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(stderr(&output), "error: nothing staged\n");
}

#[test]
fn generate_accepts_a_path() {
    let repo = staged_repo();
    let elsewhere = tempfile::TempDir::new().unwrap();
    let path = repo.path().to_str().unwrap();
    let output = smart_commit(
        &[
            "-C",
            path,
            "--backend",
            "mock",
            "--mock-response",
            "feat: x y",
        ],
        elsewhere.path(),
    );
    assert_eq!(stdout(&output), "feat: x y\n");
}

#[test]
fn commit_goes_through_the_editor() {
    let repo = staged_repo();
    // `true` accepts the pre-filled message unchanged.
    let output = smart_commit_with(
        &[
            "--commit",
            "--backend",
            "mock",
            "--mock-response",
            "feat(cli): add the entry point\n\nSo the binary can run.",
        ],
        repo.path(),
        &[("GIT_EDITOR", "true")],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        repo.git_output(&["log", "-1", "--format=%B"]),
        "feat(cli): add the entry point\n\nSo the binary can run.\n\n"
    );
    assert_eq!(repo.git_output(&["diff", "--staged", "--name-only"]), "");
}

#[test]
fn failed_editor_aborts_the_commit() {
    let repo = staged_repo();
    let output = smart_commit_with(
        &[
            "--commit",
            "--backend",
            "mock",
            "--mock-response",
            "feat: x y",
        ],
        repo.path(),
        &[("GIT_EDITOR", "false")],
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("`git commit --edit` exited with"));
    assert_eq!(repo.git_output(&["rev-list", "--count", "HEAD"]), "2\n");
}

#[test]
fn trace_is_written_and_replays() {
    let repo = staged_repo();
    let trace = repo.path().join("trace.json");
    let output = smart_commit(
        &[
            "--backend",
            "mock",
            "--mock-response",
            "Add the entry point",
            "--mock-response",
            "feat: add the entry point",
            "--trace",
            trace.to_str().unwrap(),
        ],
        repo.path(),
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let json = std::fs::read_to_string(&trace).unwrap();
    assert!(json.contains("\"raw\": \"Add the entry point\""));
    assert!(!json.contains(repo.path().to_str().unwrap()));

    // Replays anywhere, without the repository.
    let elsewhere = tempfile::TempDir::new().unwrap();
    let output = smart_commit(&["replay", trace.to_str().unwrap()], elsewhere.path());
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "reproduced: 2 attempt(s), Model message\nfeat: add the entry point\n"
    );
}

#[test]
fn replay_reports_differences() {
    let repo = staged_repo();
    let trace = repo.path().join("trace.json");
    let path = trace.to_str().unwrap();
    smart_commit(
        &[
            "--backend",
            "mock",
            "--mock-response",
            "feat: add it",
            "--trace",
            path,
        ],
        repo.path(),
    );
    let json = std::fs::read_to_string(&trace).unwrap();
    // Only the recorded outcome changes, not the reply it came from.
    let tampered = json.replace(
        "\"subject\": \"feat: add it\"",
        "\"subject\": \"feat: add that\"",
    );
    assert_ne!(tampered, json);
    std::fs::write(&trace, tampered).unwrap();

    let output = smart_commit(&["replay", path], repo.path());
    assert_eq!(output.status.code(), Some(1));
    let err = stderr(&output);
    assert!(err.contains("differs: message: recorded"), "{err}");
    assert!(err.contains("error: not reproduced"), "{err}");
}

#[test]
fn replay_of_a_missing_file_fails() {
    let repo = TestRepo::new();
    let output = smart_commit(&["replay", "missing.json"], repo.path());
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).starts_with("error: cannot read missing.json"));
}

#[test]
fn backend_options_can_come_from_the_environment() {
    let repo = staged_repo();
    let trace = repo.path().join("env-trace.json");
    let output = smart_commit_with(
        &[],
        repo.path(),
        &[
            ("SMART_COMMIT_BACKEND", "mock"),
            (
                "SMART_COMMIT_MOCK_RESPONSE",
                "feat: add the entry point\n\nWith a body, too.",
            ),
            ("SMART_COMMIT_TRACE", trace.to_str().unwrap()),
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "feat: add the entry point\n\nWith a body, too.\n"
    );
    assert!(trace.exists());
}

#[test]
fn flags_override_the_environment() {
    let repo = staged_repo();
    let output = smart_commit_with(
        &["--mock-response", "fix: from the flag"],
        repo.path(),
        &[
            ("SMART_COMMIT_BACKEND", "mock"),
            ("SMART_COMMIT_MOCK_RESPONSE", "feat: from the environment"),
        ],
    );
    assert_eq!(stdout(&output), "fix: from the flag\n");
}

#[test]
fn setup_runs_the_guided_setup_next_to_the_binary() {
    let dir = tempfile::TempDir::new().unwrap();
    let data = tempfile::TempDir::new().unwrap();
    // An empty machine (no GPU, no NPU), no drivers, no benchmark, and
    // downloads that do not exist: llama.cpp fails gracefully and nothing
    // touches the network or depends on this machine's hardware.
    let sysfs = tempfile::TempDir::new().unwrap();
    let cpuinfo = sysfs.path().join("cpuinfo");
    std::fs::write(
        &cpuinfo,
        "vendor_id\t: AuthenticAMD\nmodel name\t: Test CPU\n",
    )
    .unwrap();
    let output = smart_commit_with(
        &["setup", "--no-drivers", "--no-bench"],
        dir.path(),
        &[
            ("SMART_COMMIT_SYSFS", sysfs.path().to_str().unwrap()),
            ("SMART_COMMIT_CPUINFO", cpuinfo.to_str().unwrap()),
            ("SMART_COMMIT_OPENVINO_DIR", "/nonexistent/openvino"),
            (
                "SMART_COMMIT_LLAMACPP_URL",
                "file:///nonexistent/llama.tar.gz",
            ),
            ("SMART_COMMIT_GGUF_URL", "file:///nonexistent/model.gguf"),
            ("SMART_COMMIT_RELEASE_BASE", "file:///nonexistent/releases"),
            ("XDG_DATA_HOME", data.path().to_str().unwrap()),
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let out = stdout(&output);
    assert!(out.contains("==> Hardware\n  CPU  Test CPU"), "{out}");
    assert!(out.contains("  GPU  none\n"), "{out}");
    assert!(out.contains("not needed: OpenVINO"), "{out}");
    assert!(out.contains("Could not install llama.cpp"), "{out}");
    assert!(!out.contains("OpenVINO model"), "{out}");
    assert!(out.contains("smart-commit hook install"), "{out}");
}

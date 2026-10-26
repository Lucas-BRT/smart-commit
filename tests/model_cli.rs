//! `smart-commit model`, without the network: recommended models are either
//! "installed" by hand (the pinned size and recorded hash) or fail to
//! download from a `file://` base that does not exist.

use std::fs;
use std::path::PathBuf;
use std::process::Output;

use smart_commit::models::{self, Model};
use tempfile::TempDir;

mod common;
use common::smart_commit;

struct Home {
    config: TempDir,
    data: TempDir,
}

impl Home {
    fn new() -> Self {
        Self {
            config: TempDir::new().unwrap(),
            data: TempDir::new().unwrap(),
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        smart_commit(
            args,
            self.data.path(),
            &[
                ("XDG_CONFIG_HOME", self.config.path().to_str().unwrap()),
                ("XDG_DATA_HOME", self.data.path().to_str().unwrap()),
                ("SMART_COMMIT_HF_BASE", "file:///nonexistent/hf"),
            ],
        )
    }

    fn models(&self) -> PathBuf {
        self.data.path().join("smart-commit/models")
    }

    fn config(&self) -> String {
        fs::read_to_string(self.config.path().join("smart-commit/config.toml")).unwrap_or_default()
    }

    /// What `install_gguf` leaves, without the content: a file of the pinned
    /// size and the recorded hash.
    fn install(&self, model: &Model) {
        fs::create_dir_all(self.models()).unwrap();
        let file = self.models().join(model.gguf.file);
        fs::File::create(&file)
            .unwrap()
            .set_len(model.gguf.size)
            .unwrap();
        fs::write(models::sidecar(&file), model.gguf.sha256).unwrap();
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn model(id: &str) -> &'static Model {
    models::find(id).unwrap()
}

#[test]
fn list_shows_every_recommended_model_and_the_one_in_use() {
    let home = Home::new();
    home.install(model("qwen2.5-coder-0.5b"));
    let output = home.run(&["model"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let out = stdout(&output);
    assert!(
        out.contains("  qwen2.5-coder-1.5b      1.1 GB  Apache-2.0  validated     -  <- in use"),
        "{out}"
    );
    assert!(
        out.contains("  qwen2.5-coder-0.5b      491 MB  Apache-2.0  experimental  gguf\n"),
        "{out}"
    );
    assert!(out.contains("  qwen2.5-coder-7b "), "{out}");
    assert!(
        out.contains("  smart-commit-1.5b-v0.1  986 MB  Apache-2.0  experimental  -\n"),
        "{out}"
    );
    assert!(out.contains("  smart-commit-0.5b-v0.1  397 MB  "), "{out}");
    assert!(out.contains("  smart-commit-0.5b-v0.2  397 MB  "), "{out}");
    assert!(
        out.contains("or smart-commit's own fine-tunes (smart-commit-*)"),
        "{out}"
    );
    assert!(
        out.contains("llama.cpp devices only (vulkan, cpu)"),
        "{out}"
    );
    assert!(
        out.contains("In use: qwen2.5-coder-1.5b (model.name, default)."),
        "{out}"
    );
    assert_eq!(stdout(&home.run(&["model", "list"])), out);
}

#[test]
fn use_switches_to_an_installed_model() {
    let home = Home::new();
    home.install(model("qwen2.5-coder-0.5b"));
    let output = home.run(&["model", "use", "qwen2.5-coder-0.5b"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stderr(&output).contains("Qwen2.5-Coder-0.5B-Instruct is already installed"));
    let out = stdout(&output);
    assert!(out.contains("model.name = qwen2.5-coder-0.5b"), "{out}");
    assert!(
        out.contains("qwen2.5-coder-0.5b has no OpenVINO version"),
        "{out}"
    );
    assert!(home.config().contains("name = \"qwen2.5-coder-0.5b\""));

    let gguf = stdout(&home.run(&["config", "get", "model.gguf"]));
    assert!(
        gguf.contains("qwen2.5-coder-0.5b-instruct-q4_k_m.gguf"),
        "{gguf}"
    );
    let dir = stderr(&home.run(&["config", "get", "model.dir"]));
    assert!(
        dir.contains("model.dir has no value: qwen2.5-coder-0.5b has no OpenVINO version"),
        "{dir}"
    );
    assert!(stdout(&home.run(&["model"])).contains("experimental  gguf  <- in use"));
}

#[test]
fn use_switches_to_a_fine_tune() {
    let home = Home::new();
    home.install(model("smart-commit-0.5b-v0.1"));
    let output = home.run(&["model", "use", "smart-commit-0.5b-v0.1"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let out = stdout(&output);
    assert!(out.contains("model.name = smart-commit-0.5b-v0.1"), "{out}");
    assert!(
        out.contains("smart-commit-0.5b-v0.1 has no OpenVINO version"),
        "{out}"
    );
    let gguf = stdout(&home.run(&["config", "get", "model.gguf"]));
    assert!(
        gguf.contains("smart-commit-0.5b-v0.1-q4_k_m.gguf"),
        "{gguf}"
    );
    assert!(stdout(&home.run(&["model"])).contains("In use: smart-commit-0.5b-v0.1"));
}

#[test]
fn a_fine_tune_downloads_from_the_project_account() {
    let home = Home::new();
    let output = home.run(&["model", "use", "smart-commit-1.5b-v0.1"]);
    assert_eq!(output.status.code(), Some(1));
    let err = stderr(&output);
    assert!(
        err.contains("downloading smart-commit 1.5B v0.1 (986 MB, Apache-2.0)"),
        "{err}"
    );
    assert!(
        err.contains(
            "cannot download file:///nonexistent/hf/LucasBRT/smart-commit-1.5b-v0.1/resolve/\
             9147da25797e21b63815f9ad80a1e79547198a37/smart-commit-1.5b-v0.1-q4_k_m.gguf"
        ),
        "{err}"
    );
    assert_eq!(home.config(), "");
}

#[test]
fn a_failed_download_changes_nothing() {
    let home = Home::new();
    let output = home.run(&["model", "use", "qwen2.5-coder-7b"]);
    assert_eq!(output.status.code(), Some(1));
    let err = stderr(&output);
    assert!(
        err.contains("downloading Qwen2.5-Coder-7B-Instruct (4.7 GB, Apache-2.0)"),
        "{err}"
    );
    assert!(
        err.contains(
            "cannot download file:///nonexistent/hf/Qwen/Qwen2.5-Coder-7B-Instruct-GGUF/resolve/"
        ),
        "{err}"
    );
    assert_eq!(home.config(), "");
}

#[test]
fn use_takes_a_gguf_file_and_back() {
    let home = Home::new();
    let file = home.data.path().join("mine.gguf");
    fs::write(&file, b"GGUF\x03\x00\x00\x00").unwrap();
    let output = home.run(&["model", "use", file.to_str().unwrap()]);
    assert!(output.status.success(), "{}", stderr(&output));
    let out = stdout(&output);
    assert!(
        out.contains(&format!("model.gguf = {}", file.display())),
        "{out}"
    );
    assert!(
        out.contains("vulkan and cpu now run this file, unverified"),
        "{out}"
    );
    let list = stdout(&home.run(&["model"]));
    assert!(
        list.contains(&format!("model.gguf = {} (global", file.display())),
        "{list}"
    );

    // A recommended model replaces the file.
    home.install(model("qwen2.5-coder-1.5b"));
    let out = stdout(&home.run(&["model", "use", "qwen2.5-coder-1.5b"]));
    assert!(out.contains("model.gguf removed"), "{out}");
    assert!(out.contains("its OpenVINO version (npu, intel-gpu, openvino-cpu) is not installed"));
    assert!(!home.config().contains("gguf"), "{}", home.config());
}

#[test]
fn a_gguf_file_stays_listed_after_switching_away() {
    let home = Home::new();
    let file = home.data.path().join("smart-commit-0.5b-v0.9-q4_k_m.gguf");
    fs::write(&file, b"GGUF\x03\x00\x00\x00").unwrap();
    let output = home.run(&["model", "use", file.to_str().unwrap()]);
    assert!(output.status.success(), "{}", stderr(&output));
    let list = stdout(&home.run(&["model"]));
    assert!(
        list.contains(
            "  smart-commit-0.5b-v0.9-q4_k_m    0 MB  -           unverified    gguf  <- in use\n"
        ),
        "{list}"
    );
    assert!(
        list.contains("  qwen2.5-coder-1.5b             1.1 GB  Apache-2.0  validated     -\n"),
        "the file replaces model.name on vulkan and cpu: {list}"
    );
    assert!(
        list.contains(&format!(
            "  smart-commit-0.5b-v0.9-q4_k_m from disk, {}; vulkan and cpu",
            file.display()
        )),
        "{list}"
    );

    home.install(model("qwen2.5-coder-0.5b"));
    home.run(&["model", "use", "qwen2.5-coder-0.5b"]);
    let list = stdout(&home.run(&["model"]));
    assert!(
        list.contains("  smart-commit-0.5b-v0.9-q4_k_m    0 MB  -           unverified    gguf\n"),
        "still listed, no longer in use: {list}"
    );
    assert!(list.contains("experimental  gguf  <- in use"), "{list}");

    // Gone from disk, gone from the list.
    fs::remove_file(&file).unwrap();
    let list = stdout(&home.run(&["model"]));
    assert!(!list.contains("smart-commit-0.5b-v0.9"), "{list}");
    assert!(list.contains("  qwen2.5-coder-0.5b      491 MB"), "{list}");
}

#[test]
fn models_from_disk_are_listed_most_recent_first_and_once() {
    let home = Home::new();
    let a = home.data.path().join("a.gguf");
    let b = home.data.path().join("b.gguf");
    fs::write(&a, b"GGUF").unwrap();
    fs::write(&b, b"GGUF").unwrap();
    for file in [&a, &b, &a] {
        let output = home.run(&["model", "use", file.to_str().unwrap()]);
        assert!(output.status.success(), "{}", stderr(&output));
    }
    let list = stdout(&home.run(&["model", "list"]));
    let rows: Vec<&str> = list.lines().filter(|l| l.contains("unverified")).collect();
    assert_eq!(rows.len(), 2, "{list}");
    assert!(
        rows[0].starts_with("  a ") && rows[0].ends_with("<- in use"),
        "{list}"
    );
    assert!(
        rows[1].starts_with("  b ") && rows[1].ends_with("gguf"),
        "{list}"
    );

    // A refused file is not remembered.
    let zip = home.data.path().join("model.zip");
    fs::write(&zip, b"PK\x03\x04").unwrap();
    home.run(&["model", "use", zip.to_str().unwrap()]);
    assert!(!stdout(&home.run(&["model"])).contains("model.zip"));
}

#[test]
fn a_file_set_by_hand_is_listed_in_use() {
    let home = Home::new();
    let file = home.data.path().join("by-hand.gguf");
    fs::write(&file, b"GGUF").unwrap();
    let output = home.run(&["config", "set", "model.gguf", file.to_str().unwrap()]);
    assert!(output.status.success(), "{}", stderr(&output));
    let list = stdout(&home.run(&["model"]));
    assert!(
        list.lines()
            .any(|l| l.starts_with("  by-hand ") && l.ends_with("gguf  <- in use")),
        "{list}"
    );
}

#[test]
fn use_takes_an_openvino_directory() {
    let home = Home::new();
    let dir = home.data.path().join("my-ov-model");
    fs::create_dir_all(&dir).unwrap();
    let output = home.run(&["model", "use", dir.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("has no openvino_model.xml"));

    fs::write(dir.join("openvino_model.xml"), "<net/>").unwrap();
    let out = stdout(&home.run(&["model", "use", dir.to_str().unwrap()]));
    assert!(
        out.contains(&format!("model.dir = {}", dir.display())),
        "{out}"
    );
    assert!(
        out.contains("npu, intel-gpu and openvino-cpu now run this file"),
        "{out}"
    );
    let list = stdout(&home.run(&["model"]));
    assert!(
        list.lines()
            .any(|l| l.starts_with("  my-ov-model ") && l.ends_with("openvino  <- in use")),
        "{list}"
    );
    assert!(
        list.contains("validated     -  <- in use"),
        "model.name still runs on vulkan and cpu: {list}"
    );
}

#[test]
fn use_refuses_what_is_not_a_model() {
    let home = Home::new();
    let output = home.run(&["model", "use", "llama-3"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains(
            "llama-3 is neither a recommended model (qwen2.5-coder-1.5b, qwen2.5-coder-0.5b, \
             qwen2.5-coder-7b, smart-commit-1.5b-v0.1, smart-commit-0.5b-v0.1, \
             smart-commit-0.5b-v0.2) nor a file"
        ),
        "{}",
        stderr(&output)
    );
    let zip = home.data.path().join("model.zip");
    fs::write(&zip, b"PK\x03\x04").unwrap();
    let output = home.run(&["model", "use", zip.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("is not a GGUF file"));
    assert_eq!(home.config(), "");
}

#[test]
fn model_name_is_checked_and_npu_explains_a_model_without_openvino() {
    let home = Home::new();
    let output = home.run(&["config", "set", "model.name", "gpt"]);
    assert_eq!(output.status.code(), Some(1));
    home.run(&["config", "set", "model.name", "qwen2.5-coder-0.5b"]);
    let repo = common::TestRepo::new();
    repo.write("a.rs", "fn a() {}\n");
    repo.git(&["add", "."]);
    let output = smart_commit(
        &["--device", "npu"],
        repo.path(),
        &[
            ("XDG_CONFIG_HOME", home.config.path().to_str().unwrap()),
            ("XDG_DATA_HOME", home.data.path().to_str().unwrap()),
        ],
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains(
            "qwen2.5-coder-0.5b has no OpenVINO version, which npu needs; \
             `smart-commit model use qwen2.5-coder-1.5b` has one"
        ),
        "{}",
        stderr(&output)
    );
}

//! `update` and `uninstall` run the install.sh of the release next to the
//! binary. Here the binary is copied into a fake install, whose install.sh
//! only records how it was called: no network, nothing removed.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use tempfile::TempDir;

mod common;

/// A prefix as install.sh leaves it, with the test binary in bin/ and an
/// install.sh that writes its arguments to `calls`.
struct FakeInstall {
    dir: TempDir,
}

impl FakeInstall {
    fn new(with_installer: bool) -> Self {
        let dir = TempDir::new().unwrap();
        let share = dir.path().join("prefix/share/smart-commit");
        std::fs::create_dir_all(share.join("scripts")).unwrap();
        std::fs::create_dir_all(dir.path().join("prefix/bin")).unwrap();
        std::fs::write(share.join("VERSION"), "0.8.0\n").unwrap();
        let bin = dir.path().join("prefix/bin/smart-commit");
        std::fs::copy(env!("CARGO_BIN_EXE_smart-commit"), &bin).unwrap();
        if with_installer {
            let calls = dir.path().join("calls");
            std::fs::write(
                share.join("scripts/install.sh"),
                format!("echo \"$*\" >>'{}'\n", calls.display()),
            )
            .unwrap();
        }
        Self { dir }
    }

    fn prefix(&self) -> PathBuf {
        self.dir.path().join("prefix")
    }

    fn run(&self, args: &[&str]) -> Output {
        output(common::isolate(
            Command::new(self.prefix().join("bin/smart-commit"))
                .args(args)
                .current_dir(self.dir.path())
                .stdin(Stdio::null()),
        ))
    }

    fn calls(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("calls")).unwrap_or_default()
    }
}

/// Runs a binary copied just before: while another test thread forks, the
/// child may still hold the copy's write descriptor until it execs, and
/// running the copy fails with ETXTBSY for that moment. Tried again then.
fn output(command: &mut Command) -> Output {
    for _ in 0..100 {
        match command.output() {
            Err(err) if err.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            result => return result.unwrap(),
        }
    }
    panic!("the copied binary stayed busy");
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn update_runs_the_installer_of_this_release() {
    let install = FakeInstall::new(true);
    let output = install.run(&["update"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(text(&output.stdout).contains("smart-commit 0.8.0 in "));
    let prefix = install.prefix().canonicalize().unwrap();
    assert_eq!(
        install.calls(),
        format!("--update --prefix {}\n", prefix.display())
    );
}

#[test]
fn uninstall_says_what_goes_and_asks() {
    let install = FakeInstall::new(true);
    // No terminal and no --yes: nothing is run.
    let output = install.run(&["uninstall"]);
    assert!(!output.status.success());
    assert!(
        text(&output.stderr).contains("pass --yes"),
        "{}",
        text(&output.stderr)
    );
    assert_eq!(install.calls(), "");
    let plan = text(&output.stdout);
    assert!(
        plan.contains("This removes smart-commit 0.8.0 from this machine:"),
        "{plan}"
    );
    assert!(
        plan.contains(&format!(
            "models, llama.cpp and the benchmark: {}/smart-commit",
            common::DATA_HOME
        )),
        "{plan}"
    );
    assert!(
        plan.contains(&format!(
            "configuration: {}/smart-commit",
            common::CONFIG_HOME
        )),
        "{plan}"
    );
    assert!(plan.contains("prepare-commit-msg"), "{plan}");

    let output = install.run(&["uninstall", "--yes", "--system"]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(text(&output.stdout).contains("OpenVINO, if smart-commit's setup installed it"));
    let prefix = install.prefix().canonicalize().unwrap();
    assert_eq!(
        install.calls(),
        format!("--purge --system --yes --prefix {}\n", prefix.display())
    );
}

#[test]
fn uninstall_keeps_the_users_own_model_files() {
    let install = FakeInstall::new(true);
    let config = install.dir.path().join("config");
    std::fs::create_dir_all(config.join("smart-commit")).unwrap();
    let own = install.dir.path().join("Models/mine.gguf");
    std::fs::create_dir_all(own.parent().unwrap()).unwrap();
    std::fs::write(&own, "gguf").unwrap();
    std::fs::write(
        config.join("smart-commit/config.toml"),
        format!("[model]\ngguf = \"{}\"\n", own.display()),
    )
    .unwrap();
    let output = output(
        common::isolate(
            Command::new(install.prefix().join("bin/smart-commit"))
                .args(["uninstall"])
                .stdin(Stdio::null()),
        )
        .env("XDG_CONFIG_HOME", &config),
    );
    let plan = text(&output.stdout);
    assert!(
        plan.contains(&format!(
            "Kept, not downloaded by smart-commit: {}",
            own.display()
        )),
        "{plan}"
    );
}

#[test]
fn a_build_from_source_is_refused() {
    let dir = TempDir::new().unwrap();
    for command in ["update", "uninstall"] {
        let output = common::smart_commit(
            &[command, "--yes"][..if command == "update" { 1 } else { 2 }],
            dir.path(),
            &[],
        );
        assert!(!output.status.success(), "{command}");
        assert!(
            text(&output.stderr).contains("was not installed by install.sh"),
            "{command}: {}",
            text(&output.stderr)
        );
    }
}

#[test]
fn an_install_from_before_the_commands_points_to_install_sh() {
    let install = FakeInstall::new(false);
    let output = install.run(&["update"]);
    assert!(!output.status.success());
    let err = text(&output.stderr);
    assert!(
        err.contains("0.8.0 predates `smart-commit update`"),
        "{err}"
    );
    assert!(err.contains("install.sh | bash -s -- --no-setup"), "{err}");
    assert!(!Path::new(&install.dir.path().join("calls")).exists());
}

#[test]
fn the_installer_is_run_with_bash() {
    // A release's install.sh is executable, but the copy may not be (a
    // prefix copied without modes): bash runs it either way.
    let install = FakeInstall::new(true);
    let script = install
        .prefix()
        .join("share/smart-commit/scripts/install.sh");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(install.run(&["update"]).status.success());
}

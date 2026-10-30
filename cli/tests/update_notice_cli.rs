//! The opt-in notice of a newer release, with a fake `curl` in PATH: the
//! network is never touched.

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::process::Output;

use tempfile::TempDir;

mod common;
use common::smart_commit;

const NEWER: &str = r#"{"tag_name": "v99.0.0"}"#;

/// A `curl` that records that it ran and prints `reply`.
struct FakeCurl {
    dir: TempDir,
    cache: TempDir,
}

impl FakeCurl {
    fn new(reply: &str, exit: i32) -> Self {
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("curl");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\necho \"$@\" >> \"{}/calls\"\ncat <<'EOF'\n{reply}\nEOF\nexit {exit}\n",
                dir.path().display()
            ),
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        Self {
            dir,
            cache: TempDir::new().unwrap(),
        }
    }

    fn calls(&self) -> String {
        fs::read_to_string(self.dir.path().join("calls")).unwrap_or_default()
    }

    /// `smart-commit` with the fake curl first in PATH. `setting` is the
    /// `update.check` environment variable, if any.
    fn run(&self, args: &[&str], setting: Option<&str>, terminal: bool) -> Output {
        let path = format!(
            "{}:{}",
            self.dir.path().display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut env = vec![
            ("PATH", path.as_str()),
            ("XDG_CACHE_HOME", self.cache.path().to_str().unwrap()),
        ];
        if let Some(setting) = setting {
            env.push(("SMART_COMMIT_UPDATE_CHECK", setting));
        }
        if terminal {
            env.push(("SMART_COMMIT_NOTICE_TTY", "1"));
        }
        smart_commit(args, Path::new("."), &env)
    }
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn off_by_default_curl_is_never_called() {
    let curl = FakeCurl::new(NEWER, 0);
    let output = curl.run(&["model"], None, true);
    assert!(output.status.success(), "{}", stderr(&output));
    let output = curl.run(&["model"], Some("false"), true);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(curl.calls(), "");
    assert!(!stderr(&output).contains("is available"));
}

#[test]
fn the_note_is_printed_once_a_day() {
    let curl = FakeCurl::new(NEWER, 0);
    let first = curl.run(&["model"], Some("true"), true);
    assert!(first.status.success(), "{}", stderr(&first));
    let note = format!(
        "note: smart-commit 99.0.0 is available (you have {}); run smart-commit update\n",
        env!("CARGO_PKG_VERSION")
    );
    assert_eq!(
        stderr(&first).matches(&note).count(),
        1,
        "{}",
        stderr(&first)
    );
    let calls = curl.calls();
    assert!(calls.contains("--max-time 2"), "{calls}");
    assert!(calls.contains(&format!("smart-commit/{}", env!("CARGO_PKG_VERSION"))));
    assert!(calls.contains("https://api.github.com/repos/Lucas-BRT/smart-commit/releases/latest"));

    let second = curl.run(&["model"], Some("true"), true);
    assert!(
        !stderr(&second).contains("is available"),
        "{}",
        stderr(&second)
    );
    assert_eq!(curl.calls(), calls, "no second request inside the window");
}

#[test]
fn nothing_without_a_terminal() {
    let curl = FakeCurl::new(NEWER, 0);
    let output = curl.run(&["model"], Some("true"), false);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(curl.calls(), "");
}

#[test]
fn a_failing_curl_is_silent_and_does_not_change_the_exit_code() {
    let curl = FakeCurl::new("", 28);
    let output = curl.run(&["model"], Some("true"), true);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(!stderr(&output).contains("is available"));
    assert!(!stderr(&output).contains("curl"), "{}", stderr(&output));
    let calls = curl.calls();
    curl.run(&["model"], Some("true"), true);
    assert_eq!(curl.calls(), calls, "the failure started the window");
}

#[test]
fn the_hook_and_the_inspection_commands_never_ask() {
    let curl = FakeCurl::new(NEWER, 0);
    let file = curl.cache.path().join("COMMIT_EDITMSG");
    fs::write(&file, "").unwrap();
    for args in [
        vec!["hook", "run", file.to_str().unwrap()],
        vec!["context"],
        vec!["hardware"],
        vec!["config", "list"],
    ] {
        curl.run(&args, Some("true"), true);
        assert_eq!(curl.calls(), "", "{args:?}");
    }
}

#[test]
fn the_setting_is_a_global_boolean() {
    let curl = FakeCurl::new(NEWER, 0);
    let output = curl.run(&["config", "set", "update.check", "maybe"], None, false);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("false, true"),
        "{}",
        stderr(&output)
    );
}

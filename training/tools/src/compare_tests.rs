use std::os::unix::fs::PermissionsExt as _;
use std::sync::{Mutex, MutexGuard};

use super::*;

/// Scripts are written and run at once: two tests doing that in threads can
/// make one's `exec` fail with "text file busy", so they take turns.
static TURNS: Mutex<()> = Mutex::new(());

fn turn() -> MutexGuard<'static, ()> {
    TURNS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A `smart-commit` that wants a clean absolute path, counts its runs in
/// `runs` and answers `fix: add a` to `good.gguf` and `feat: add b` to the rest.
fn fake_smart_commit(dir: &Path) -> PathBuf {
    let path = dir.join("smart-commit");
    std::fs::write(
        &path,
        format!(
            r#"#!/bin/sh
case "$SMART_COMMIT_GGUF" in
  *..*) echo "error: not a clean path: $SMART_COMMIT_GGUF" >&2; exit 1;;
  /*) ;;
  *) echo "error: expected an absolute path, got $SMART_COMMIT_GGUF" >&2; exit 1;;
esac
echo x >> "{runs}"
case "$(basename "$SMART_COMMIT_GGUF")" in
  good.gguf) s="fix: add a";;
  broken.gguf) echo "error: no such model" >&2; exit 1;;
  *) s="feat: add b";;
esac
n=$(wc -l < "$2")
i=0
while [ "$i" -lt "$n" ]; do
  echo "{{\"inputs\":{{\"branch\":null,\"subjects\":[\"fix: old\"],\"staged\":[],\"files\":[]}},\"generation\":{{\"message\":{{\"subject\":\"$s\",\"body\":null}},\"source\":\"model\",\"attempts\":[{{\"rejected\":null}}]}}}}"
  i=$((i+1))
done
"#,
            runs = dir.join("runs").display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn runs(dir: &Path) -> usize {
    std::fs::read_to_string(dir.join("runs")).map_or(0, |t| t.lines().count())
}

/// A set of two Conventional cases and a free one, whose real subject is `fix: add a`.
fn write_set(dir: &Path) -> PathBuf {
    let set = dir.join("val");
    std::fs::create_dir_all(&set).unwrap();
    let case = |kind: Option<&str>, group: &str, target: &str| json!({"repo": "o/r", "hash": target, "target": target, "type": kind, "history": 12, "set": group});
    let meta = [
        case(Some("fix"), "conventional", "fix: add a"),
        case(Some("fix"), "conventional", "fix(core): add a too"),
        case(None, "free", "Add a"),
    ];
    std::fs::write(
        set.join("meta.jsonl"),
        meta.iter().map(|m| format!("{m}\n")).collect::<String>(),
    )
    .unwrap();
    std::fs::write(set.join("inputs.jsonl"), "{}\n{}\n{}\n").unwrap();
    set
}

fn options(dir: &Path, set: &Path, binary: &Path) -> Options {
    for name in ["good", "bad", "broken"] {
        std::fs::write(dir.join(format!("{name}.gguf")), "model").unwrap();
    }
    Options {
        out: dir.join("out"),
        binary: binary.to_owned(),
        sets: vec![set.to_owned()],
        baseline: Model {
            name: "sft".into(),
            gguf: dir.join("good.gguf"),
        },
        candidates: vec![Model {
            name: "dpo".into(),
            gguf: dir.join("bad.gguf"),
        }],
        rules: "full".into(),
        diff: "plain".into(),
        device: "vulkan".into(),
    }
}

#[test]
fn a_flag_gained_is_better_for_a_good_metric_and_worse_for_a_bad_one() {
    assert_eq!(better("type_match", 9, 13), -4);
    assert_eq!(better("first_try", 27, 3), 24);
    assert_eq!(better("ungrounded", 0, 48), 48);
    assert_eq!(better("fallback", 5, 0), -5);
    assert_eq!(better("expected", 5, 2), 3);
    // Not a yes/no metric of either kind.
    assert_eq!(better("word_f1", 3, 1), 0);
}

#[test]
fn a_model_is_a_name_and_a_file() {
    let model = Model::parse("b01=runs/b01/model.gguf").unwrap();
    assert_eq!(
        (model.name.as_str(), model.gguf.to_str().unwrap()),
        ("b01", "runs/b01/model.gguf")
    );
    for bad in ["", "b01", "=x.gguf", "b01="] {
        assert!(Model::parse(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn habits_show_a_model_that_stopped_writing_scopes_or_took_to_one_type() {
    let meta = [
        json!({"set": "conventional"}),
        json!({"set": "conventional"}),
        json!({"set": "conventional"}),
        json!({"set": "free"}),
    ];
    let real = habits(&meta, &["fix(core): a", "tests: b", "feat: c", "Add d"]);
    assert_eq!(real["conventional"]["n"], 3);
    assert_eq!(real["conventional"]["scoped"], 0.333);
    // `tests` is `test`, and a subject without a header is `-`.
    assert_eq!(
        real["conventional"]["types"],
        json!({"fix": 1, "test": 1, "feat": 1})
    );
    assert_eq!(real["free"]["types"], json!({"-": 1}));
    // Chore for everything, no scope: its scope_known would be 1.0.
    let model = habits(&meta, &["chore: a", "chore: b", "chore: c", "Add d"]);
    assert_eq!(model["conventional"]["scoped"], 0.0);
    assert_eq!(model["conventional"]["types"], json!({"chore": 3}));
    assert_eq!(mean_chars(&["abcd", "ab"]), 3.0);
}

#[test]
fn traces_are_made_once_by_the_model_asked_for_with_a_clean_absolute_path() {
    let _turn = turn();
    let dir = tempfile::TempDir::new().unwrap();
    let binary = fake_smart_commit(dir.path());
    let set = write_set(dir.path());
    let options = options(dir.path(), &set, &binary);
    let out = dir.path().join("out/val/sft.jsonl");
    // A path with `..` in it, as a relative one would be: made clean.
    let gguf = dir.path().join("sub/../good.gguf");
    std::fs::create_dir_all(dir.path().join("sub")).unwrap();
    assert!(run_eval(&binary, &gguf, &set.join("inputs.jsonl"), &out, &options).unwrap());
    assert_eq!(line_count(&out), 3);
    assert!(
        std::fs::read_to_string(&out)
            .unwrap()
            .contains("fix: add a")
    );
    // Complete: not made again. Cut short, as a stopped run leaves it: made again.
    assert!(!run_eval(&binary, &gguf, &set.join("inputs.jsonl"), &out, &options).unwrap());
    let first = std::fs::read_to_string(&out)
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .to_owned();
    std::fs::write(&out, first + "\n").unwrap();
    assert!(run_eval(&binary, &gguf, &set.join("inputs.jsonl"), &out, &options).unwrap());
    assert_eq!((line_count(&out), runs(dir.path())), (3, 2));
}

#[test]
fn a_model_that_cannot_run_leaves_no_trace_and_says_what_smart_commit_said() {
    let _turn = turn();
    let dir = tempfile::TempDir::new().unwrap();
    let binary = fake_smart_commit(dir.path());
    let set = write_set(dir.path());
    let options = options(dir.path(), &set, &binary);
    let out = dir.path().join("out/val/broken.jsonl");
    let err = run_eval(
        &binary,
        &dir.path().join("broken.gguf"),
        &set.join("inputs.jsonl"),
        &out,
        &options,
    )
    .unwrap_err();
    assert!(err.to_string().contains("no such model"), "{err}");
    assert!(!out.exists());
    let err = run_eval(
        &binary,
        &dir.path().join("absent.gguf"),
        &set.join("inputs.jsonl"),
        &out,
        &options,
    )
    .unwrap_err();
    assert!(err.to_string().contains("no model at"), "{err}");
}

#[test]
fn a_candidate_is_compared_with_the_baseline_case_by_case() {
    let _turn = turn();
    let dir = tempfile::TempDir::new().unwrap();
    let binary = fake_smart_commit(dir.path());
    let set = write_set(dir.path());
    let options = options(dir.path(), &set, &binary);
    let (text, summary) = compare(&options).unwrap();
    // The baseline writes the real type for both Conventional cases, the candidate neither.
    let val = &summary["val"];
    assert_eq!(val["sft"]["scores"]["conventional"]["type_match"], 1.0);
    assert_eq!(val["dpo"]["scores"]["conventional"]["type_match"], 0.0);
    assert_eq!(
        val["dpo"]["against_baseline"]["conventional"]["type_match"]["lost"],
        2
    );
    assert_eq!(val["sft"]["against_baseline"], json!({}));
    assert_eq!(
        val["dpo"]["habits"]["conventional"]["types"],
        json!({"feat": 2})
    );
    assert_eq!(val["real"]["habits"]["conventional"]["scoped"], 0.5);
    assert_eq!(val["dpo"]["mean_subject_chars"], 11.0);
    for expected in [
        "== val / conventional",
        "== val / free",
        "type_match-2",
        "types (n=2, scoped 0.5)",
        "feat 2",
    ] {
        assert!(text.contains(expected), "{expected:?} not in\n{text}");
    }
    // `expected` exists only for the scenarios: no column of dashes.
    assert!(!text.contains("expected"));
    assert!(options.out.join("summary.json").exists());
    // A second time nothing is evaluated again.
    let before = runs(dir.path());
    compare(&options).unwrap();
    assert_eq!(runs(dir.path()), before);
}

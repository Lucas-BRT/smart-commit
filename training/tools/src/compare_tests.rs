use super::*;
use std::os::unix::fs::PermissionsExt as _;

/// The `smart-commit` of the tests is a shell script, and a script just
/// written can fail to run ("text file busy") when another thread forks while
/// the file is open. So it is written once, under a name made from its
/// content, and every test after the first only runs it.
const FAKE: &str = r#"#!/bin/sh
case "$SMART_COMMIT_GGUF" in
  *..*) echo "error: not a clean path: $SMART_COMMIT_GGUF" >&2; exit 1;;
  /*) ;;
  *) echo "error: expected an absolute path, got $SMART_COMMIT_GGUF" >&2; exit 1;;
esac
echo x >> "$(dirname "$2")/runs"
case "$(basename "$SMART_COMMIT_GGUF")" in
  good.gguf) s="fix: add a";;
  broken.gguf) echo "error: no such model" >&2; exit 1;;
  *) s="feat: add b";;
esac
n=$(wc -l < "$2")
i=0
while [ "$i" -lt "$n" ]; do
  echo "{\"inputs\":{\"branch\":null,\"subjects\":[\"fix: old\"],\"staged\":[],\"files\":[]},\"generation\":{\"message\":{\"subject\":\"$s\",\"body\":null},\"source\":\"model\",\"attempts\":[{\"rejected\":null}]}}"
  i=$((i+1))
done
"#;

/// A `smart-commit` that wants a clean absolute path, counts its runs in the
/// file `runs` beside the inputs it is given, and answers `fix: add a` to
/// `good.gguf`, an error to `broken.gguf` and `feat: add b` to the rest.
fn fake_smart_commit() -> PathBuf {
    use sha2::{Digest as _, Sha256};
    // Once per process: the tests run in threads, and two creating the file at
    // once would fight over the same temporary name.
    static PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    PATH.get_or_init(|| {
        let id: String = Sha256::digest(FAKE.as_bytes())
            .iter()
            .take(6)
            .map(|b| format!("{b:02x}"))
            .collect();
        let path = std::env::temp_dir().join(format!("sc-training-fake-smart-commit-{id}"));
        if !path.exists() {
            // Written aside and renamed in: nobody ever sees it half written.
            let aside = path.with_extension(format!("{}", std::process::id()));
            std::fs::write(&aside, FAKE).unwrap();
            std::fs::set_permissions(&aside, std::fs::Permissions::from_mode(0o755)).unwrap();
            std::fs::rename(&aside, &path).unwrap();
        }
        path
    })
    .clone()
}

/// How many times the stand-in ran for the set in `dir` (`write_set`'s `val`).
fn runs(dir: &Path) -> usize {
    std::fs::read_to_string(dir.join("val/runs")).map_or(0, |t| t.lines().count())
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
        read: 0,
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
    let nothing = vec![String::new(); 4];
    let real = habits(
        &meta,
        &["fix(core): a", "tests: b", "feat: c", "Add d"],
        &nothing,
    );
    assert_eq!(real["conventional"]["n"], 3);
    assert_eq!(real["conventional"]["scoped"], 0.333);
    // `tests` is `test`, and a subject without a header is `-`.
    assert_eq!(
        real["conventional"]["types"],
        json!({"fix": 1, "test": 1, "feat": 1})
    );
    assert_eq!(real["free"]["types"], json!({"-": 1}));
    // Chore for everything, no scope: its scope_known would be 1.0.
    let model = habits(
        &meta,
        &["chore: a", "chore: b", "chore: c", "Add d"],
        &nothing,
    );
    assert_eq!(model["conventional"]["scoped"], 0.0);
    assert_eq!(model["conventional"]["types"], json!({"chore": 3}));
    assert_eq!(mean_chars(&["abcd", "ab"]), 3.0);
}

#[test]
fn traces_are_made_once_by_the_model_asked_for_with_a_clean_absolute_path() {
    let dir = tempfile::TempDir::new().unwrap();
    let binary = fake_smart_commit();
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
    let dir = tempfile::TempDir::new().unwrap();
    let binary = fake_smart_commit();
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
    let dir = tempfile::TempDir::new().unwrap();
    let binary = fake_smart_commit();
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
        "types (n=2, scoped 0.5, novel 1.0)",
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

#[test]
fn the_novel_share_counts_content_words_found_in_nothing_the_model_was_given() {
    let meta = [
        json!({"set": "conventional"}),
        json!({"set": "conventional"}),
        json!({"set": "free"}),
    ];
    // What was given, lowercase, per case: the history, the paths and the diff.
    let known = vec![
        "src/parser.rs retry the parser".to_owned(),
        "readme.md".to_owned(),
        "main.rs".to_owned(),
    ];
    // `add` and `the` are stop words; case 1 has two content words (retry, parser)
    // both known, case 2 has two (kubernetes, helm) both new; the free case is apart.
    let got = habits(
        &meta,
        &[
            "fix(parser): add retry to the parser",
            "feat: kubernetes helm",
            "Add main",
        ],
        &known,
    );
    assert_eq!(got["conventional"]["novel"], 0.5);
    assert_eq!(got["free"]["novel"], 0.0);
    // Nothing to count is not a division by zero.
    assert_eq!(
        habits(&meta, &["fix: add", "feat: add", "Add"], &known)["conventional"]["novel"],
        0.0
    );
}

#[test]
fn the_cases_to_read_are_half_of_each_set_and_the_same_whatever_the_models() {
    let meta: Vec<Value> = (0..12)
        .map(|n| json!({"set": if n % 2 == 0 { "conventional" } else { "free" }, "hash": format!("h{n}")}))
        .collect();
    let chosen = sample(&meta, 4);
    assert_eq!(chosen.len(), 4);
    let of = |group: &str| chosen.iter().filter(|&&i| meta[i]["set"] == group).count();
    assert_eq!((of("conventional"), of("free")), (2, 2));
    assert_eq!(sample(&meta, 4), chosen, "the same cases again");
    // An odd number gives the extra case to the Conventional set; more than there are is all.
    assert_eq!(sample(&meta, 3).len(), 3);
    assert_eq!(sample(&meta, 100).len(), 12);
    assert!(sample(&meta, 0).is_empty());
}

#[test]
fn the_cases_to_read_come_with_the_real_subject_and_every_model() {
    let dir = tempfile::TempDir::new().unwrap();
    let binary = fake_smart_commit();
    let set = write_set(dir.path());
    let options = Options {
        read: 3,
        ..options(dir.path(), &set, &binary)
    };
    let (text, summary) = compare(&options).unwrap();
    assert!(text.contains("== val / 3 cases side by side"), "{text}");
    for expected in [
        "real         fix: add a",
        "sft          fix: add a",
        "dpo          feat: add b",
    ] {
        assert!(text.contains(expected), "{expected:?} not in\n{text}");
    }
    assert_eq!(
        summary["val"]["sft"]["habits"]["conventional"]["novel"],
        0.0
    );
    // Not asked for: not printed.
    let (quiet, _) = compare(&Options { read: 0, ..options }).unwrap();
    assert!(!quiet.contains("side by side"));
}

#[test]
fn traces_of_another_model_or_other_settings_under_the_same_name_are_made_again() {
    let dir = tempfile::TempDir::new().unwrap();
    let binary = fake_smart_commit();
    let set = write_set(dir.path());
    let options = options(dir.path(), &set, &binary);
    let (inputs, out) = (set.join("inputs.jsonl"), dir.path().join("out/val/m.jsonl"));
    let (good, bad) = (dir.path().join("good.gguf"), dir.path().join("bad.gguf"));
    let said = || std::fs::read_to_string(&out).unwrap();
    assert!(run_eval(&binary, &good, &inputs, &out, &options).unwrap());
    assert!(said().contains("fix: add a"));
    // The same file and settings: kept.
    assert!(!run_eval(&binary, &good, &inputs, &out, &options).unwrap());
    // Another file under the same name: its traces, not the old ones scored as its own.
    assert!(run_eval(&binary, &bad, &inputs, &out, &options).unwrap());
    assert!(said().contains("feat: add b") && !said().contains("fix: add a"));
    // The same file, other settings.
    let other = Options {
        rules: "omitted".into(),
        ..options
    };
    assert!(run_eval(&binary, &bad, &inputs, &out, &other).unwrap());
    assert!(!run_eval(&binary, &bad, &inputs, &out, &other).unwrap());
    assert_eq!(runs(dir.path()), 3);
    // Traces made before keys existed are taken as they are, and marked.
    std::fs::remove_file(out.with_extension("key")).unwrap();
    assert!(!run_eval(&binary, &bad, &inputs, &out, &other).unwrap());
    assert!(out.with_extension("key").exists());
    assert_eq!(runs(dir.path()), 3);
}

#[test]
fn a_model_name_cannot_leave_its_directory_nor_repeat_nor_be_real() {
    for bad in ["../x=m.gguf", "a/b=m.gguf", "real=m.gguf", "a b=m.gguf"] {
        let err = Model::parse(bad).unwrap_err().to_string();
        assert!(err.contains("is not a name for a model"), "{bad}: {err}");
    }
    assert!(Model::parse("b01-lr2.v2_x=m.gguf").is_ok());
    // Two models with one name would overwrite each other's results.
    let dir = tempfile::TempDir::new().unwrap();
    let binary = fake_smart_commit();
    let set = write_set(dir.path());
    let mut options = options(dir.path(), &set, &binary);
    options.candidates = vec![Model {
        name: "sft".into(),
        gguf: dir.path().join("bad.gguf"),
    }];
    let err = compare(&options).unwrap_err().to_string();
    assert!(err.contains("two models are named \"sft\""), "{err}");
}

#[test]
fn the_length_of_the_subjects_is_that_of_each_set_not_of_both() {
    let meta = [
        json!({"set": "conventional"}),
        json!({"set": "free"}),
        json!({"set": "free"}),
    ];
    let nothing = vec![String::new(); 3];
    let got = habits(&meta, &["fix: ab", "Add something long", "Add"], &nothing);
    assert_eq!(got["conventional"]["chars"], 7.0);
    assert_eq!(got["free"]["chars"], 10.5);
}

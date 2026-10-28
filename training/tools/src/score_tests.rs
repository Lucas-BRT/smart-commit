use serde_json::json;

use super::*;

#[test]
fn a_description_has_no_header_or_emoji() {
    assert_eq!(description("feat(cli): add `eval`"), "add `eval`");
    assert_eq!(description("✨ add login page"), "add login page");
}

#[test]
fn specifics_are_what_could_be_invented() {
    assert_eq!(
        specifics("bump to v5.5.0-beta.6 for PR 1234, use `foo` in fooBar and my_var"),
        ["v5.5.0-beta.6", "1234", "foo", "fooBar", "my_var"]
    );
}

#[test]
fn support_and_grounding_are_read_from_the_evidence() {
    let known = "src/combinelatest.ts\n+  const foobar = 1\n"; // evidence() lowercases
    assert!(!unsupported("fix(rx): rename fooBar", known));
    assert!(unsupported("chore(release): 1.1.0", known));
    assert!(!unsupported("fix: handle empty input", known));
    assert!(ungrounded("fix: fix typo", known));
    assert!(ungrounded("docs: update docs", known));
    assert!(!ungrounded(
        "fix(rx): guard combineLatest against empty arrays",
        known
    ));
}

#[test]
fn a_scope_fits_where_the_history_used_it() {
    assert_eq!(area("src/diff.rs"), "src/");
    assert_eq!(area("mise.toml"), "");
    let mut inputs = json!({
        "subjects": ["feat(training): add train.py", "fix(cli): parse flags", "docs: readme"],
        "history_files": [["training/train.py"], ["src/main.rs"], ["README.md"]],
        "files": [{"path": "mise.toml"}],
    });
    assert_eq!(
        scope_fits("feat(training): rust 1.99", &inputs),
        Some(false)
    );
    assert_eq!(
        scope_fits("chore: update rust in mise.toml", &inputs),
        Some(true)
    );
    inputs["files"] = json!([{"path": "src/main.rs"}, {"path": "mise.toml"}]);
    assert_eq!(scope_fits("fix(cli): handle -C", &inputs), Some(true));
    // A scope fits but none is given, or one never used.
    assert_eq!(scope_fits("fix: handle -C", &inputs), Some(false));
    assert_eq!(scope_fits("fix(api): handle -C", &inputs), Some(false));
    assert_eq!(
        scope_fits("fix: x", &json!({"subjects": ["a"], "files": []})),
        None
    );
}

fn pairs(list: &[(&str, Option<&str>, usize)]) -> Pairs {
    let mut counted = Pairs::default();
    for (real, generated, n) in list {
        for _ in 0..*n {
            counted.add(real, generated.map(str::to_owned));
        }
    }
    counted
}

#[test]
fn types_are_scored_one_by_one() {
    let counted = pairs(&[
        ("fix", Some("fix"), 3),
        ("fix", Some("feat"), 1),
        ("feat", Some("feat"), 1),
        ("docs", None, 2),
    ]);
    let (scores, macro_f1) = per_type(&counted);
    let order: Vec<&String> = scores.as_object().unwrap().keys().collect();
    assert_eq!(order, ["fix", "docs", "feat"]);
    assert_eq!(
        scores["fix"],
        json!({"n": 4, "precision": 1.0, "recall": 0.75, "f1": 0.857})
    );
    assert_eq!(
        scores["feat"],
        json!({"n": 1, "precision": 0.5, "recall": 1.0, "f1": 0.667})
    );
    assert_eq!(
        scores["docs"],
        json!({"n": 2, "precision": 0.0, "recall": 0.0, "f1": 0.0})
    );
    assert_eq!(macro_f1, round3((0.857 + 0.667 + 0.0) / 3.0));
    assert_eq!(per_type(&Pairs::default()), (json!({}), 0.0));
}

#[test]
fn rounding_is_pythons() {
    assert_eq!(round3(0.0625), 0.062); // a tie goes to the even digit
    assert_eq!(round3(2.0 / 3.0), 0.667);
    assert_eq!(round3(1.0), 1.0);
}

#[test]
fn mcnemar_is_the_exact_two_sided_test() {
    assert_eq!(mcnemar(0, 0), 1.0);
    assert_eq!(round3(mcnemar(8, 0)) * 1000.0, 8.0); // 2 / 2**8 = 0.0078
    assert_eq!(mcnemar(4, 1), 0.375);
    assert_eq!(mcnemar(1, 4), 0.375);
    assert_eq!(mcnemar(10, 10), 1.0);
}

fn conventional_meta(i: usize) -> Value {
    json!({"repo": "r", "hash": i.to_string(), "target": "fix(a): b", "type": "fix",
           "history": 1, "set": "conventional"})
}

fn trace(subject: &str) -> Value {
    json!({
        "inputs": {"branch": null, "subjects": ["x"], "staged": [], "files": []},
        "generation": {"message": {"subject": subject, "body": null}, "source": "model",
                       "attempts": [{"rejected": null}]},
    })
}

#[test]
fn two_runs_are_compared_case_by_case() {
    let meta: Vec<Value> = (0..3).map(conventional_meta).collect();
    let base = [trace("feat(a): b"), trace("fix(a): b"), trace("feat(a): c")];
    let other = [trace("fix(a): b"), trace("fix(a): b"), trace("feat(a): c")];
    let report = paired(&meta, &base, &other);
    assert_eq!(
        report["conventional"]["type_match"],
        json!({"gained": 1, "lost": 0, "p": 1.0})
    );
    assert_eq!(
        report["all"]["exact"],
        json!({"gained": 1, "lost": 0, "p": 1.0})
    );
    assert!(report["all"].get("word_f1").is_none());
    assert!(report["all"].get("unsupported_real").is_none());
}

#[test]
fn a_message_made_by_rules_has_no_attempt() {
    let meta = vec![
        json!({"repo": "r", "hash": "h", "target": "chore(deps): update lockfile",
                           "type": "chore", "history": 1, "set": "conventional"}),
    ];
    let traces = vec![json!({
        "inputs": {"branch": "main", "subjects": ["a"], "staged": [], "files": []},
        "generation": {"message": {"subject": "chore(deps): update package-lock.json", "body": null},
                       "source": "rules", "attempts": []},
    })];
    assert_eq!(report(&meta, &traces)["all"]["first_try"], json!(1.0));
}

#[test]
fn the_report_lists_types_the_way_python_did() {
    let meta: Vec<Value> = (0..2).map(conventional_meta).collect();
    let traces = vec![trace("fix(a): b"), trace("Update the thing")];
    let report = report(&meta, &traces);
    let all = &report["all"];
    assert_eq!(all["n"], json!(2));
    assert_eq!(all["type_match"], json!(0.5));
    assert_eq!(
        all["types (real->generated)"],
        json!({"fix->fix": 1, "fix->None": 1})
    );
    assert_eq!(all["type_scores"]["fix"]["recall"], json!(0.5));
}

#[test]
fn sections_follow_the_set_and_the_level() {
    assert_eq!(groups(&json!({"set": "free"})), ["free", "all"]);
    assert_eq!(
        groups(&json!({"set": "free", "level": 2})),
        ["free", "all", "level 2"]
    );
}

#[test]
fn values_print_like_python() {
    assert_eq!(
        pyrepr(&json!({"a": [1, 0.5, null, true], "it's": "x"})),
        "{'a': [1, 0.5, None, True], \"it's\": 'x'}"
    );
}

#[test]
fn types_are_grouped_as_a_repository_may_choose_between_them() {
    assert_eq!(type_group("build", false), type_group("ci", false));
    assert_eq!(type_group("perf", false), type_group("style", false));
    assert_eq!(type_group("tests", false), "test");
    assert_ne!(type_group("feat", false), type_group("fix", false));
    assert_eq!(type_group("feat", true), type_group("fix", true));
    assert_ne!(type_group("feat", true), type_group("docs", true));
    assert_ne!(type_group("chore", true), type_group("refactor", true));
    assert_eq!(type_group("revert", true), "revert");
}

#[test]
fn a_type_of_the_same_group_counts_for_the_group_and_not_for_the_match() {
    let real = |kind: &str| {
        json!({"repo": "r", "hash": "h", "target": format!("{kind}: x"), "type": kind,
               "history": 1, "set": "conventional"})
    };
    let cases = [
        ("chore", "ci: b", (false, true, true)),
        ("feat", "fix: b", (false, false, true)),
        ("docs", "fix: b", (false, false, false)),
        ("fix", "fix: b", (true, true, true)),
        ("fix", "Update b", (false, false, false)),
    ];
    for (kind, written, (exact, group, behaviour)) in cases {
        let row = row_of(&real(kind), &trace(written));
        let get = |name: &str| row.iter().find(|(k, _)| *k == name).map(|(_, v)| *v);
        assert_eq!(
            get("type_match"),
            Some(Metric::Yes(exact)),
            "{kind} {written}"
        );
        assert_eq!(
            get("type_group"),
            Some(Metric::Yes(group)),
            "{kind} {written}"
        );
        assert_eq!(
            get("type_behaviour"),
            Some(Metric::Yes(behaviour)),
            "{kind} {written}"
        );
    }
    // A free-style commit has no type to match.
    let free =
        json!({"repo": "r", "hash": "h", "target": "x", "type": null, "history": 1, "set": "free"});
    assert!(
        row_of(&free, &trace("x"))
            .iter()
            .all(|(k, _)| !k.starts_with("type_"))
    );
}

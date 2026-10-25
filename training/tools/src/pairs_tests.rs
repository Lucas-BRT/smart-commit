use serde_json::json;

use super::*;

fn inputs() -> Value {
    json!({
        "subjects": [
            "fix(cats): ensure that previousSet is actually a Set",
            "feat(api): add a retry loop for failed requests",
        ],
        "files": [{
            "path": "src/cats.rs",
            "old_path": null,
            "hunks": "@@ -1,2 +1,2 @@\n-let previousSet = old;\n+let previousSet = HashSet::new();\n",
        }],
    })
}

fn judge() -> Judge {
    Judge::new("conventional", &inputs())
}

#[test]
fn the_subject_of_a_reply_is_its_first_real_line() {
    assert_eq!(subject_of("fix(a): b\n\nbody"), "fix(a): b");
    assert_eq!(subject_of("```\nfix(a): b\n```"), "fix(a): b");
    assert_eq!(subject_of("\n  fix(a): b  \n"), "fix(a): b");
    assert_eq!(subject_of("\"fix(a): b\""), "fix(a): b");
    assert_eq!(subject_of(""), "");
}

#[test]
fn a_good_subject_passes_every_check() {
    assert_eq!(
        judge().failure("fix(cats): create the set with HashSet::new"),
        None
    );
}

#[test]
fn each_check_has_its_failure() {
    let judge = judge();
    assert_eq!(judge.failure(""), Some(Failure::Empty));
    assert_eq!(judge.failure("..."), Some(Failure::Empty));
    assert_eq!(judge.failure("Fix the set"), Some(Failure::Format));
    assert_eq!(
        judge.failure(&format!("fix(cats): {}", "set ".repeat(20))),
        Some(Failure::Long)
    );
    assert_eq!(
        judge.failure("fix(cats): ensure that previousSet is actually a Set"),
        Some(Failure::Copied)
    );
    assert_eq!(
        judge.failure("chore(release): bump to v9.9.9"),
        Some(Failure::Unsupported)
    );
    assert_eq!(
        judge.failure("fix(cats): improve things"),
        Some(Failure::Ungrounded)
    );
}

#[test]
fn a_copy_ignores_case_and_a_final_period() {
    let judge = judge();
    assert_eq!(
        judge.failure("fix(cats): Ensure that previousSet is actually a Set."),
        Some(Failure::Copied)
    );
    // A few words dropped: a near copy; words of its own added: not a copy.
    assert_eq!(
        judge.failure("fix(cats): ensure previousSet is actually a Set"),
        Some(Failure::Copied)
    );
    assert_eq!(
        judge.failure("fix(cats): ensure previousSet is a Set in HashSet::new"),
        None
    );
    // Too short to be a near copy.
    assert_ne!(
        judge.failure("feat(api): add a retry"),
        Some(Failure::Copied)
    );
}

#[test]
fn the_free_set_has_no_header_to_demand() {
    let judge = Judge::new("free", &inputs());
    assert_eq!(judge.failure("Create the set with HashSet::new"), None);
}

#[test]
fn the_pair_prefers_the_real_subject_when_it_passes() {
    let replies = vec![
        "fix(cats): ensure that previousSet is actually a Set".to_owned(),
        "fix(cats): create the set".to_owned(),
    ];
    let (chosen, rejected, failure) = pair(
        &judge(),
        "fix(cats): build previousSet with HashSet::new",
        &replies,
    )
    .unwrap();
    assert_eq!(chosen, "fix(cats): build previousSet with HashSet::new");
    assert_eq!(
        rejected,
        "fix(cats): ensure that previousSet is actually a Set"
    );
    assert_eq!(failure, Failure::Copied);
}

#[test]
fn a_passing_sample_stands_in_when_the_real_subject_fails() {
    let replies = vec![
        "fix(cats): improve things".to_owned(),
        "fix(cats): build previousSet with HashSet::new".to_owned(),
    ];
    // The real subject is vague here.
    let (chosen, rejected, failure) = pair(&judge(), "fix: update stuff", &replies).unwrap();
    assert_eq!(chosen, "fix(cats): build previousSet with HashSet::new");
    assert_eq!(rejected, "fix(cats): improve things");
    assert_eq!(failure, Failure::Ungrounded);
}

#[test]
fn no_pair_without_a_failure_or_without_something_good_to_prefer() {
    let good = vec!["fix(cats): build previousSet with HashSet::new".to_owned()];
    assert_eq!(
        pair(
            &judge(),
            "fix(cats): create the set with HashSet::new",
            &good
        ),
        None
    );
    // A reply that fails, no passing subject at all.
    let bad = vec!["fix(cats): improve things".to_owned()];
    assert_eq!(pair(&judge(), "fix: update stuff", &bad), None);
    assert_eq!(pair(&judge(), "fix(cats): create the set", &[]), None);
    // The chosen would be the rejected.
    let same = vec!["fix(cats): improve things".to_owned()];
    assert_eq!(pair(&judge(), "fix(cats): improve things", &same), None);
}

#[test]
fn pairs_are_written_one_a_line_with_what_they_came_from() {
    let dir = tempfile::TempDir::new().unwrap();
    let write_lines = |name: &str, rows: &[Value]| {
        let path = dir.path().join(name);
        let text: String = rows.iter().map(|r| format!("{r}\n")).collect();
        std::fs::write(&path, text).unwrap();
        path
    };
    let candidates = write_lines(
        "c.jsonl",
        &[
            json!({"system": "S", "user": "U1", "replies": [
                "fix(cats): ensure that previousSet is actually a Set"]}),
            json!({"system": "S", "user": "U2", "replies": [
                "fix(cats): build previousSet with HashSet::new"]}),
        ],
    );
    let inputs_path = write_lines("i.jsonl", &[inputs(), inputs()]);
    let meta = write_lines(
        "m.jsonl",
        &[
            json!({"set": "conventional", "target": "fix(cats): replace previousSet with a HashSet"}),
            json!({"set": "conventional", "target": "fix(cats): replace previousSet with a HashSet"}),
        ],
    );
    let out = dir.path().join("pairs.jsonl");
    let summary = write(&candidates, &inputs_path, &meta, &out).unwrap();
    assert_eq!(
        (summary.inputs, summary.pairs, summary.real_chosen),
        (2, 1, 1)
    );
    assert_eq!(summary.by_failure["copied"], 1);
    let text = std::fs::read_to_string(&out).unwrap();
    assert_eq!(text.lines().count(), 1);
    let row: Value = serde_json::from_str(text.trim()).unwrap();
    assert_eq!(row["user"], "U1");
    assert_eq!(
        row["chosen"],
        "fix(cats): replace previousSet with a HashSet"
    );
    assert_eq!(
        row["rejected"],
        "fix(cats): ensure that previousSet is actually a Set"
    );
    assert_eq!(row["failure"], "copied");
    assert_eq!(row["source"], "real");
}

#[test]
fn files_that_do_not_line_up_are_an_error() {
    let dir = tempfile::TempDir::new().unwrap();
    let one = dir.path().join("one.jsonl");
    let two = dir.path().join("two.jsonl");
    std::fs::write(&one, "{}\n").unwrap();
    std::fs::write(&two, "{}\n{}\n").unwrap();
    let err = write(&one, &two, &one, &dir.path().join("out")).unwrap_err();
    assert!(
        err.to_string().contains("1 candidates, 2 inputs, 1 meta"),
        "{err}"
    );
}

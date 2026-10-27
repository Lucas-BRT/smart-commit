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
    Judge::new("conventional", None, &inputs())
}

/// A judge of an input whose real commit is of type `kind`.
fn judge_of(kind: &str) -> Judge {
    Judge::new("conventional", Some(kind), &inputs())
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
    // A scope no history subject used, whatever the type.
    assert_eq!(
        judge.failure("fix(dogs): create the set with HashSet::new"),
        Some(Failure::Scope)
    );
    assert_eq!(
        judge.failure("fix: create the set with HashSet::new"),
        None,
        "no scope is always allowed"
    );
}

#[test]
fn the_type_is_the_real_commits_only_when_it_is_known() {
    let wrong = "feat(cats): create the set with HashSet::new";
    assert_eq!(judge().failure(wrong), None);
    assert_eq!(judge_of("fix").failure(wrong), Some(Failure::Type));
    assert_eq!(judge_of("feat").failure(wrong), None);
    // `tests` and `test` are one type.
    assert_eq!(
        judge_of("tests").failure("test(cats): create the set with HashSet::new"),
        None
    );
    // The scope is checked before the type.
    assert_eq!(
        judge_of("fix").failure("feat(dogs): create the set with HashSet::new"),
        Some(Failure::Scope)
    );
    // The free set has neither.
    let free = Judge::new("free", Some("fix"), &inputs());
    assert_eq!(free.failure("Create the set with HashSet::new"), None);
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
    let judge = Judge::new("free", None, &inputs());
    assert_eq!(judge.failure("Create the set with HashSet::new"), None);
}

#[test]
fn the_pair_prefers_the_real_subject_when_asked_and_it_passes() {
    let replies = vec![
        "fix(cats): ensure that previousSet is actually a Set".to_owned(),
        "fix(cats): create the set with HashSet::new".to_owned(),
    ];
    let real = "fix(cats): build previousSet with HashSet::new";
    let (chosen, rejected, failure) = pair(&judge(), real, &replies, Prefer::Real).unwrap();
    assert_eq!(chosen, real);
    assert_eq!(
        rejected,
        "fix(cats): ensure that previousSet is actually a Set"
    );
    assert_eq!(failure, Failure::Copied);
}

#[test]
fn a_passing_sample_is_preferred_by_default_and_the_closest_to_the_real_one_wins() {
    let replies = vec![
        "fix(cats): improve things".to_owned(),
        "fix(cats): create the set with HashSet::new".to_owned(),
        "fix(cats): build previousSet with HashSet::new".to_owned(),
    ];
    let real = "fix(cats): build previousSet with a HashSet::new";
    let (chosen, rejected, _) = pair(&judge(), real, &replies, Prefer::Sample).unwrap();
    assert_eq!(chosen, "fix(cats): build previousSet with HashSet::new");
    assert_eq!(rejected, "fix(cats): improve things");
    // Equally close: the first sampled.
    let tied = vec![
        "fix(cats): improve things".to_owned(),
        "fix(cats): make a HashSet::new".to_owned(),
        "fix(cats): make HashSet::new a set".to_owned(),
    ];
    let (chosen, _, _) = pair(&judge(), "docs: x", &tied, Prefer::Sample).unwrap();
    assert!(
        chosen == "fix(cats): make a HashSet::new"
            || chosen == "fix(cats): make HashSet::new a set"
    );
}

#[test]
fn the_real_subject_stands_in_when_no_sample_passes_and_the_other_way_round() {
    let bad = vec!["fix(cats): improve things".to_owned()];
    let real = "fix(cats): build previousSet with HashSet::new";
    let (chosen, ..) = pair(&judge(), real, &bad, Prefer::Sample).unwrap();
    assert_eq!(chosen, real);
    // The real subject is vague here: a sample stands in even if Real is asked.
    let replies = vec![
        "fix(cats): improve things".to_owned(),
        "fix(cats): build previousSet with HashSet::new".to_owned(),
    ];
    let (chosen, rejected, failure) =
        pair(&judge(), "fix: update stuff", &replies, Prefer::Real).unwrap();
    assert_eq!(chosen, "fix(cats): build previousSet with HashSet::new");
    assert_eq!(rejected, "fix(cats): improve things");
    assert_eq!(failure, Failure::Ungrounded);
}

#[test]
fn a_wrong_type_is_a_pair_and_a_real_subject_with_an_unknown_scope_is_not_preferred() {
    let judge = judge_of("fix");
    let replies = vec![
        "feat(cats): build previousSet with HashSet::new".to_owned(),
        "fix(cats): build previousSet with HashSet::new".to_owned(),
    ];
    let (chosen, rejected, failure) = pair(
        &judge,
        "fix(dogs): build previousSet with HashSet::new",
        &replies,
        Prefer::Real,
    )
    .unwrap();
    // The real subject names a scope the history never used: it cannot be preferred.
    assert_eq!(chosen, "fix(cats): build previousSet with HashSet::new");
    assert_eq!(rejected, "feat(cats): build previousSet with HashSet::new");
    assert_eq!(failure, Failure::Type);
}

#[test]
fn the_rejected_reply_is_the_one_failing_the_most_serious_check() {
    let judge = judge_of("fix");
    let replies = vec![
        "feat(cats): build previousSet with HashSet::new".to_owned(),
        "fix(cats): improve things".to_owned(),
        "fix(dogs): build previousSet with HashSet::new".to_owned(),
    ];
    let real = "fix(cats): build previousSet with HashSet::new";
    let (_, rejected, failure) = pair(&judge, real, &replies, Prefer::Real).unwrap();
    assert_eq!(
        (rejected.as_str(), failure),
        ("fix(cats): improve things", Failure::Ungrounded)
    );
    // The first of those failing alike.
    let alike = vec![
        "feat(cats): build previousSet with HashSet::new".to_owned(),
        "chore(cats): build previousSet with HashSet::new".to_owned(),
    ];
    let (_, rejected, failure) = pair(&judge, real, &alike, Prefer::Real).unwrap();
    assert_eq!(
        (rejected.as_str(), failure),
        (
            "feat(cats): build previousSet with HashSet::new",
            Failure::Type
        )
    );
}

#[test]
fn no_pair_without_a_failure_or_without_something_good_to_prefer() {
    for prefer in [Prefer::Sample, Prefer::Real] {
        let good = vec!["fix(cats): build previousSet with HashSet::new".to_owned()];
        assert_eq!(
            pair(
                &judge(),
                "fix(cats): create the set with HashSet::new",
                &good,
                prefer
            ),
            None
        );
        // A reply that fails, no passing subject at all.
        let bad = vec!["fix(cats): improve things".to_owned()];
        assert_eq!(pair(&judge(), "fix: update stuff", &bad, prefer), None);
        assert_eq!(
            pair(&judge(), "fix(cats): create the set", &[], prefer),
            None
        );
        // The chosen would be the rejected, or its case and final period apart.
        let same = vec!["fix(cats): improve things".to_owned()];
        assert_eq!(
            pair(&judge(), "fix(cats): improve things", &same, prefer),
            None
        );
        let other = vec!["Fix(cats): build previousSet with HashSet::new.".to_owned()];
        assert_eq!(
            pair(
                &judge_of("fix"),
                "fix(cats): build previousSet with hashset::new",
                &other,
                prefer
            ),
            None
        );
    }
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
            json!({"set": "conventional", "target": "fix(cats): replace previousSet with a HashSet",
                   "repo": "o/r", "hash": "h1", "type": "fix", "history": 12}),
            json!({"set": "conventional", "target": "fix(cats): replace previousSet with a HashSet",
                   "repo": "o/r", "hash": "h2", "type": "fix", "history": 12}),
        ],
    );
    let out = dir.path().join("pairs.jsonl");
    let summary = write(&candidates, &inputs_path, &meta, &out, Prefer::Real).unwrap();
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
    // What the pairs are balanced by later.
    assert_eq!(
        (
            row["repo"].as_str(),
            row["hash"].as_str(),
            row["type"].as_str(),
            row["history"].as_u64()
        ),
        (Some("o/r"), Some("h1"), Some("fix"), Some(12))
    );
}

#[test]
fn files_that_do_not_line_up_are_an_error() {
    let dir = tempfile::TempDir::new().unwrap();
    let one = dir.path().join("one.jsonl");
    let two = dir.path().join("two.jsonl");
    std::fs::write(&one, "{}\n").unwrap();
    std::fs::write(&two, "{}\n{}\n").unwrap();
    let err = write(&one, &two, &one, &dir.path().join("out"), Prefer::Sample).unwrap_err();
    assert!(
        err.to_string().contains("1 candidates, 2 inputs, 1 meta"),
        "{err}"
    );
}

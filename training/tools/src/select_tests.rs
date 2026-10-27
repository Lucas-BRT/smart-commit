use std::path::Path;

use serde_json::json;

use super::*;

fn options(src: &Path, out: &Path) -> Options {
    Options {
        src: src.to_owned(),
        out: out.to_owned(),
        conventional: 2,
        free: 0,
        per_repo: 40,
        min_history: 10,
        max_history: None,
        supported_only: false,
        max_type_share: 1.0,
        releases: false,
        exclude_hashes_of: Vec::new(),
        exclude_repos_of: Vec::new(),
    }
}

fn write_set(dir: &Path, items: &[(Value, Value)]) {
    std::fs::create_dir_all(dir).unwrap();
    let mut inputs = String::new();
    let mut meta = String::new();
    for (i, m) in items {
        inputs += &format!("{}\n", pyjson::dumps(i));
        meta += &format!("{}\n", pyjson::dumps(m));
    }
    std::fs::write(dir.join("inputs.jsonl"), inputs).unwrap();
    std::fs::write(dir.join("meta.jsonl"), meta).unwrap();
}

fn lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn release_bumps_are_dropped_unless_the_source_holds_them() {
    let dir = tempfile::TempDir::new().unwrap();
    let src = dir.path().join("src");
    let meta = |hash: &str, target: &str| json!({"repo": "r", "hash": hash, "target": target, "type": "chore", "history": 10});
    write_set(
        &src,
        &[
            (json!({}), meta("a", "chore: release 0.5.0")),
            (json!({}), meta("b", "chore: release 0.6.0")),
        ],
    );
    for (releases, kept) in [(false, 0), (true, 2)] {
        let out = dir.path().join(format!("out{kept}"));
        select(&Options {
            releases,
            ..options(&src, &out)
        })
        .unwrap();
        assert_eq!(lines(&out.join("meta.jsonl")).len(), kept);
    }
}

#[test]
fn selection_is_by_hash_and_marks_the_set() {
    let dir = tempfile::TempDir::new().unwrap();
    let src = dir.path().join("src");
    let items: Vec<(Value, Value)> = (0..6)
        .map(|n| {
            (
                json!({"n": n}),
                json!({"repo": format!("r{}", n % 3), "hash": format!("h{n}"),
                       "target": format!("feat: add thing number {n}"), "type": "feat", "history": 12}),
            )
        })
        .collect();
    write_set(&src, &items);
    let (a, b) = (dir.path().join("a"), dir.path().join("b"));
    select(&Options {
        conventional: 3,
        ..options(&src, &a)
    })
    .unwrap();
    select(&Options {
        conventional: 3,
        ..options(&src, &b)
    })
    .unwrap();
    assert_eq!(lines(&a.join("meta.jsonl")), lines(&b.join("meta.jsonl")));
    let chosen = lines(&a.join("meta.jsonl"));
    assert_eq!(chosen.len(), 3);
    assert!(
        chosen
            .iter()
            .all(|l| l.ends_with("\"set\": \"conventional\"}"))
    );
    // The inputs are the chosen commits' own lines.
    for (input, meta) in lines(&a.join("inputs.jsonl")).iter().zip(&chosen) {
        let meta: Value = serde_json::from_str(meta).unwrap();
        let n: usize = meta["hash"].as_str().unwrap()[1..].parse().unwrap();
        assert_eq!(input, &pyjson::dumps(&json!({"n": n})));
    }
}

#[test]
fn at_most_per_repo_per_repository_and_the_history_range_apply() {
    let dir = tempfile::TempDir::new().unwrap();
    let src = dir.path().join("src");
    let items: Vec<(Value, Value)> = (0..6)
        .map(|n| {
            (
                json!({}),
                json!({"repo": "same", "hash": format!("h{n}"), "target": "fix: something long enough",
                       "type": "fix", "history": if n < 2 { 3 } else { 12 }}),
            )
        })
        .collect();
    write_set(&src, &items);
    let out = dir.path().join("out");
    select(&Options {
        conventional: 6,
        per_repo: 2,
        ..options(&src, &out)
    })
    .unwrap();
    assert_eq!(lines(&out.join("meta.jsonl")).len(), 2);
    let young = dir.path().join("young");
    select(&Options {
        conventional: 6,
        min_history: 0,
        max_history: Some(5),
        ..options(&src, &young)
    })
    .unwrap();
    assert_eq!(lines(&young.join("meta.jsonl")).len(), 2);
}

#[test]
fn the_commits_and_repositories_of_other_selections_are_left_out() {
    let dir = tempfile::TempDir::new().unwrap();
    let src = dir.path().join("src");
    let items: Vec<(Value, Value)> = ["a", "a", "b", "b", "c"]
        .iter()
        .enumerate()
        .map(|(n, repo)| {
            (
                json!({}),
                json!({"repo": repo, "hash": format!("h{n}"), "target": "fix: something long enough",
                       "type": "fix", "history": 12}),
            )
        })
        .collect();
    write_set(&src, &items);
    let hashes = |out: &Path| -> Vec<String> {
        lines(&out.join("meta.jsonl"))
            .iter()
            .map(|l| {
                serde_json::from_str::<Value>(l).unwrap()["hash"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect()
    };
    // What a model was trained on: two commits (h0 of "a", h2 of "b").
    let seen = dir.path().join("seen");
    write_set(
        &seen,
        &[
            (
                json!({}),
                json!({"repo": "a", "hash": "h0", "target": "x", "type": "fix", "history": 12}),
            ),
            (
                json!({}),
                json!({"repo": "b", "hash": "h2", "target": "x", "type": "fix", "history": 12}),
            ),
        ],
    );
    let out = dir.path().join("hashes");
    select(&Options {
        conventional: 10,
        exclude_hashes_of: vec![seen.clone()],
        ..options(&src, &out)
    })
    .unwrap();
    let mut got = hashes(&out);
    got.sort();
    assert_eq!(got, ["h1", "h3", "h4"]);
    // An evaluation set: its repositories go whole, whatever the commit.
    let out = dir.path().join("repos");
    select(&Options {
        conventional: 10,
        exclude_repos_of: vec![seen],
        ..options(&src, &out)
    })
    .unwrap();
    assert_eq!(hashes(&out), ["h4"]);
}

#[test]
fn the_type_share_caps_each_type() {
    let dir = tempfile::TempDir::new().unwrap();
    let src = dir.path().join("src");
    let items: Vec<(Value, Value)> = (0..8)
        .map(|n| {
            (
                json!({}),
                json!({"repo": format!("r{n}"), "hash": format!("h{n}"), "target": "fix: something long enough",
                       "type": "fix", "history": 12}),
            )
        })
        .collect();
    write_set(&src, &items);
    let out = dir.path().join("out");
    select(&Options {
        conventional: 8,
        max_type_share: 0.5,
        ..options(&src, &out)
    })
    .unwrap();
    assert_eq!(lines(&out.join("meta.jsonl")).len(), 4);
}

#[test]
fn filter_supported_keeps_what_the_diff_and_history_support() {
    let commits = [
        ("feat(cli): add retry", "conventional", "+fn retry() {}\n"),
        ("chore(release): 1.2.3", "conventional", "+x\n"),
        ("fix: bump vue to 2.6.13", "free", "+x\n"),
        ("fix: bump vue to 2.6.13", "free", "+\"vue\": \"2.6.13\"\n"),
    ];
    let dir = tempfile::TempDir::new().unwrap();
    let (src, out) = (dir.path().join("src"), dir.path().join("out"));
    let items: Vec<(Value, Value)> = commits
        .iter()
        .map(|(target, set, hunks)| {
            (
                json!({"subjects": [], "files": [{"path": "a", "hunks": hunks}]}),
                json!({"target": target, "set": set}),
            )
        })
        .collect();
    write_set(&src, &items);
    filter_supported(&src, &out).unwrap();
    let kept = lines(&out.join("inputs.jsonl"));
    assert_eq!(
        kept,
        [
            lines(&src.join("inputs.jsonl"))[0].clone(),
            lines(&src.join("inputs.jsonl"))[3].clone()
        ]
    );
    assert_eq!(lines(&out.join("meta.jsonl")).len(), 2);
}

#[test]
fn inputs_and_meta_must_line_up() {
    let dir = tempfile::TempDir::new().unwrap();
    let src = dir.path().join("src");
    write_set(
        &src,
        &[(
            json!({}),
            json!({"repo": "r", "hash": "a", "target": "feat: x", "history": 1}),
        )],
    );
    std::fs::write(src.join("inputs.jsonl"), "{}\n{}\n").unwrap();
    assert!(select(&options(&src, &dir.path().join("out"))).is_err());
    assert!(filter_supported(&src, &dir.path().join("out2")).is_err());
}

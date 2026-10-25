use std::path::Path;

use serde_json::json;

use super::*;

#[allow(clippy::too_many_arguments)]
fn pair(
    repo: &str,
    hash: &str,
    set: &str,
    history: u64,
    kind: Option<&str>,
    chosen: &str,
    rejected: &str,
    failure: &str,
) -> Value {
    json!({"repo": repo, "hash": hash, "set": set, "history": history, "type": kind,
           "chosen": chosen, "rejected": rejected, "failure": failure, "source": "sample"})
}

fn options(files: &[&Path], out: &Path) -> Options {
    Options {
        pairs: files.iter().map(|p| p.to_path_buf()).collect(),
        out: out.to_owned(),
        val_share: 0.1,
        max_per_repo: usize::MAX,
        max_type_share: 1.0,
        max_length_ratio: f64::MAX,
        conventional: usize::MAX,
        young: usize::MAX,
        free: usize::MAX,
    }
}

fn write(path: &Path, pairs: &[Value]) {
    std::fs::write(
        path,
        pairs.iter().map(|p| format!("{p}\n")).collect::<String>(),
    )
    .unwrap();
}

fn read(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn repos(pairs: &[Value]) -> BTreeSet<String> {
    pairs
        .iter()
        .map(|p| p["repo"].as_str().unwrap().to_owned())
        .collect()
}

/// Two pairs from each of 60 repositories.
fn many() -> Vec<Value> {
    (0..60)
        .flat_map(|r| {
            (0..2).map(move |n| {
                pair(
                    &format!("o/r{r}"),
                    &format!("h{r}-{n}"),
                    "conventional",
                    20,
                    Some("fix"),
                    &format!("fix: chosen {r} {n}"),
                    &format!("fix: rejected {r} {n}"),
                    "ungrounded",
                )
            })
        })
        .collect()
}

#[test]
fn the_validation_set_is_whole_repositories_whatever_the_order() {
    let dir = tempfile::TempDir::new().unwrap();
    let (a, b) = (dir.path().join("a.jsonl"), dir.path().join("b.jsonl"));
    let all = many();
    write(&a, &all[..50]);
    write(&b, &all[50..]);
    let first = balance(&options(&[&a, &b], &dir.path().join("one"))).unwrap();
    let second = balance(&options(&[&b, &a], &dir.path().join("two"))).unwrap();
    let (train, val) = (
        read(&dir.path().join("one/train.jsonl")),
        read(&dir.path().join("one/val.jsonl")),
    );
    assert!(repos(&train).is_disjoint(&repos(&val)));
    assert_eq!(train.len() + val.len(), 120);
    // About a tenth of 60 repositories, and always both pairs of each.
    assert!(
        (1..=14).contains(&repos(&val).len()),
        "{}",
        repos(&val).len()
    );
    assert_eq!(val.len(), 2 * repos(&val).len());
    assert_eq!(
        (first.train, first.validation),
        (second.train, second.validation)
    );
    assert_eq!(read(&dir.path().join("two/train.jsonl")), train);
    assert_eq!(repos(&read(&dir.path().join("two/val.jsonl"))), repos(&val));
}

#[test]
fn a_repository_a_bucket_and_a_type_are_capped() {
    let dir = tempfile::TempDir::new().unwrap();
    let file = dir.path().join("p.jsonl");
    let mut pairs = Vec::new();
    // One repository with ten pairs, in the Conventional bucket.
    for n in 0..10 {
        pairs.push(pair(
            "big/repo",
            &format!("b{n}"),
            "conventional",
            30,
            Some("fix"),
            &format!("fix: c{n}"),
            &format!("fix: r{n}"),
            "type",
        ));
    }
    // Forty repositories, one pair each: half fix, half feat.
    for n in 0..40 {
        let kind = if n % 2 == 0 { "fix" } else { "feat" };
        pairs.push(pair(
            &format!("o/s{n}"),
            &format!("s{n}"),
            "conventional",
            30,
            Some(kind),
            &format!("{kind}: c{n}"),
            &format!("{kind}: r{n}"),
            "type",
        ));
    }
    // Young repositories and the free set keep to their own quotas.
    for n in 0..20 {
        let kind = if n % 2 == 0 { "chore" } else { "docs" };
        pairs.push(pair(
            &format!("y/r{n}"),
            &format!("y{n}"),
            "conventional",
            3,
            Some(kind),
            &format!("{kind}: c{n}"),
            &format!("{kind}: r{n}"),
            "type",
        ));
        pairs.push(pair(
            &format!("f/r{n}"),
            &format!("f{n}"),
            "free",
            30,
            None,
            &format!("add c{n}"),
            &format!("add r{n}"),
            "copied",
        ));
    }
    write(&file, &pairs);
    let out = dir.path().join("out");
    let summary = balance(&Options {
        val_share: 0.0,
        max_per_repo: 2,
        max_type_share: 0.5,
        conventional: 20,
        young: 5,
        free: 7,
        ..options(&[&file], &out)
    })
    .unwrap();
    let train = read(&out.join("train.jsonl"));
    assert!(read(&out.join("val.jsonl")).is_empty());
    assert_eq!(summary.by_bucket["conventional"], 20);
    assert_eq!(summary.by_bucket["young"], 5);
    assert_eq!(summary.by_bucket["free"], 7);
    assert_eq!(train.len(), 32);
    let in_repo = |name: &str| train.iter().filter(|p| p["repo"] == name).count();
    assert!(in_repo("big/repo") <= 2);
    // No more than half the Conventional bucket (10 of 20) in one type.
    let conventional: Vec<&Value> = train
        .iter()
        .filter(|p| p["set"] == "conventional" && p["history"] == 30)
        .collect();
    for kind in ["fix", "feat"] {
        assert!(
            conventional.iter().filter(|p| p["type"] == kind).count() <= 10,
            "{kind}"
        );
    }
    assert_eq!(summary.train_repos, repos(&train).len());
    // The summary says how even the lengths are.
    let json = summary.json();
    let chars = |key: &str| -> usize {
        train
            .iter()
            .map(|p| p[key].as_str().unwrap().chars().count())
            .sum()
    };
    assert_eq!(summary.chosen_chars, chars("chosen"));
    assert_eq!(summary.rejected_chars, chars("rejected"));
    assert!(json["chosen_longer_share"].as_f64().unwrap() <= 1.0);
    // The free set has no type: 20 Conventional and 5 young pairs have one.
    assert_eq!(summary.by_type.values().sum::<usize>(), 25);
}

fn plain(repo: &str, hash: &str, chosen: &str, rejected: &str, failure: &str) -> Value {
    pair(repo, hash, "free", 20, None, chosen, rejected, failure)
}

#[test]
fn pairs_of_very_different_length_and_repeated_ones_are_dropped() {
    let dir = tempfile::TempDir::new().unwrap();
    let file = dir.path().join("p.jsonl");
    let long = "fix the parser ".repeat(8);
    let pairs = |failure: &str| {
        vec![
            plain("a/a", "1", "fix the parser", "fix the parser now", "copied"),
            plain("b/b", "2", "fix the parser for good", "fix", "copied"),
            // A reply rejected for being too long is long on purpose.
            plain("c/c", "3", "fix the parser", &long, failure),
            plain("d/d", "4", "fix the parser", "fix the parser now", "copied"),
        ]
    };
    let (out, plain_options) = (dir.path().join("out"), |f: &Path, o: &Path| Options {
        val_share: 0.0,
        max_length_ratio: 1.6,
        ..options(&[f], o)
    });
    write(&file, &pairs("long"));
    let summary = balance(&plain_options(&file, &out)).unwrap();
    assert_eq!(
        (
            summary.read,
            summary.dropped_length,
            summary.dropped_repeated
        ),
        (4, 1, 1)
    );
    let kept: Vec<String> = read(&out.join("train.jsonl"))
        .iter()
        .map(|p| p["hash"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(kept.len(), 2);
    assert!(kept.contains(&"3".to_owned()));
    // The same pair rejected for something else is dropped for its length.
    write(&file, &pairs("copied"));
    let summary = balance(&plain_options(&file, &out)).unwrap();
    assert_eq!(summary.dropped_length, 2);
}

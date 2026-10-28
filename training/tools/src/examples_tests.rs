use std::os::unix::fs::PermissionsExt as _;

use serde_json::json;

use super::*;

/// The `smart-commit` of the tests, written once per process (the tests run in
/// threads) and under a name made from its content (a script written and run at
/// once can fail with "text file busy" when another thread forks): it answers a prompt per input line whose
/// system message is its own arguments and what its environment holds.
fn fake_prompt_binary() -> std::path::PathBuf {
    const SCRIPT: &str = r#"#!/bin/sh
n=$(wc -l < "$3")
i=0
while [ "$i" -lt "$n" ]; do
  echo "{\"system\": \"$* home=[$HOME] xdg=[$XDG_CONFIG_HOME] café\", \"user\": \"u$i\"}"
  i=$((i+1))
done
"#;
    static PATH: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    PATH.get_or_init(|| {
        let id: String = Sha256::digest(SCRIPT.as_bytes())
            .iter()
            .take(6)
            .map(|b| format!("{b:02x}"))
            .collect();
        let path = std::env::temp_dir().join(format!("sc-training-fake-prompt-{id}"));
        if !path.exists() {
            // Written aside and renamed in: nobody ever sees it half written.
            let aside = path.with_extension(format!("{}", std::process::id()));
            std::fs::write(&aside, SCRIPT).unwrap();
            std::fs::set_permissions(&aside, std::fs::Permissions::from_mode(0o755)).unwrap();
            std::fs::rename(&aside, &path).unwrap();
        }
        path
    })
    .clone()
}

fn write_set(dir: &Path, targets: &[&str]) {
    std::fs::write(dir.join("inputs.jsonl"), "{}\n".repeat(targets.len())).unwrap();
    let meta: String = targets
        .iter()
        .enumerate()
        .map(|(n, t)| {
            format!(
                "{}\n",
                json!({"target": t, "repo": "r", "hash": format!("h{n}"), "set": "conventional"})
            )
        })
        .collect();
    std::fs::write(dir.join("meta.jsonl"), meta).unwrap();
}

fn examples(dir: &Path) -> Vec<Value> {
    std::fs::read_to_string(dir.join("examples.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn the_examples_carry_the_prompt_the_target_without_its_period_and_where_it_came_from() {
    let dir = tempfile::TempDir::new().unwrap();
    write_set(
        dir.path(),
        &["feat: add x.", "fix: handle y...", "docs: z", "fix: w. "],
    );
    assert_eq!(
        make_examples(dir.path(), &fake_prompt_binary(), "full", "plain", "list").unwrap(),
        4
    );
    let got = examples(dir.path());
    // Every trailing period goes, then the whitespace, as the Python script did: a
    // period followed by a space (`fix: w. `) keeps its period.
    assert_eq!(
        got.iter()
            .map(|e| e["target"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["feat: add x", "fix: handle y", "docs: z", "fix: w."]
    );
    assert_eq!(
        (
            got[0]["repo"].clone(),
            got[1]["hash"].clone(),
            got[2]["set"].clone()
        ),
        (json!("r"), json!("h1"), json!("conventional"))
    );
    assert_eq!(got[2]["user"], "u2");
    // The keys, in the order the Python script wrote them, and what is not ASCII raw.
    let text = std::fs::read_to_string(dir.path().join("examples.jsonl")).unwrap();
    let keys: Vec<&String> = got[0].as_object().unwrap().keys().collect();
    assert_eq!(keys, ["system", "user", "target", "repo", "hash", "set"]);
    assert!(
        text.contains("café") && text.starts_with("{\"system\": \""),
        "{text}"
    );
}

#[test]
fn rules_the_file_list_and_scopes_are_always_passed_and_nothing_of_the_environment_is() {
    let dir = tempfile::TempDir::new().unwrap();
    write_set(dir.path(), &["feat: a"]);
    for (rules, diff, scopes) in [
        ("full", "plain", "list"),
        ("omitted", "annotated", "list"),
        ("full", "annotated", "map"),
    ] {
        make_examples(dir.path(), &fake_prompt_binary(), rules, diff, scopes).unwrap();
        let system = examples(dir.path())[0]["system"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(
            system.starts_with(&format!(
                "prompt --inputs {} --rules {rules} --diff {diff} --scopes {scopes} ",
                dir.path().join("inputs.jsonl").display()
            )),
            "{system}"
        );
        // Only an empty configuration directory: not the user's home, not their config.
        assert!(system.contains("home=[]"), "{system}");
        assert!(
            system.contains("xdg=[/") && !system.contains("xdg=[]"),
            "{system}"
        );
    }
}

#[test]
fn a_prompt_for_every_case_or_an_error() {
    let dir = tempfile::TempDir::new().unwrap();
    write_set(dir.path(), &["feat: a", "feat: b"]);
    std::fs::write(dir.path().join("meta.jsonl"), "{}\n").unwrap();
    let err = make_examples(dir.path(), &fake_prompt_binary(), "full", "plain", "list")
        .unwrap_err()
        .to_string();
    assert!(err.contains("2 prompts for 1 cases"), "{err}");
    let err = make_examples(
        dir.path(),
        Path::new("/no/such/smart-commit"),
        "full",
        "plain",
        "list",
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("cannot run"), "{err}");
}

fn write_examples(path: &Path, hashes: &[&str]) {
    let text: String = hashes
        .iter()
        .enumerate()
        .map(|(n, h)| format!("{}\n", pyjson::dumps(&json!({"hash": h, "set": if n % 2 == 0 { "conventional" } else { "free" }, "target": "t"}))))
        .collect();
    std::fs::write(path, text).unwrap();
}

#[test]
fn each_size_holds_every_example_of_the_smaller_ones_whatever_the_order_of_the_file() {
    let dir = tempfile::TempDir::new().unwrap();
    let hashes: Vec<String> = (0..30).map(|n| format!("commit{n}")).collect();
    let refs: Vec<&str> = hashes.iter().map(String::as_str).collect();
    let forward = dir.path().join("forward.jsonl");
    let backward = dir.path().join("backward.jsonl");
    write_examples(&forward, &refs);
    write_examples(&backward, &refs.iter().rev().copied().collect::<Vec<_>>());
    let prefix = |name: &str| dir.path().join(name).display().to_string();
    let said = nest(&forward, &prefix("a"), &[5, 12, 30]).unwrap();
    nest(&backward, &prefix("b"), &[5, 12, 30]).unwrap();
    let read = |p: String, n: usize| -> Vec<String> {
        lines_of(Path::new(&format!("{p}-{n}.jsonl"))).unwrap()
    };
    let hash = |line: &String| {
        serde_json::from_str::<Value>(line).unwrap()["hash"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let (small, middle, all) = (
        read(prefix("a"), 5),
        read(prefix("a"), 12),
        read(prefix("a"), 30),
    );
    assert_eq!((small.len(), middle.len(), all.len()), (5, 12, 30));
    assert_eq!(
        &middle[..5],
        &small[..],
        "a larger size starts with the smaller"
    );
    assert_eq!(&all[..12], &middle[..]);
    // The same subsets from the file in the other order.
    for n in [5, 12, 30] {
        assert_eq!(
            read(prefix("a"), n).iter().map(hash).collect::<Vec<_>>(),
            read(prefix("b"), n).iter().map(hash).collect::<Vec<_>>()
        );
    }
    // The lines are copied as they are, and the count of conventional ones is said.
    assert_eq!(
        small[0],
        lines_of(&forward)
            .unwrap()
            .into_iter()
            .find(|l| l.contains(&format!("\"{}\"", hash(&small[0]))))
            .unwrap()
    );
    assert!(
        said[0].ends_with(": 5 examples, ") || said[0].contains(": 5 examples, "),
        "{said:?}"
    );
}

#[test]
fn the_ranking_is_by_the_salted_hash_not_the_plain_one_and_a_size_too_big_is_refused() {
    let dir = tempfile::TempDir::new().unwrap();
    // Find two hashes whose order the salt reverses: the point of the salt.
    let hex = |text: String| -> String {
        Sha256::digest(text.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    };
    let (mut a, mut b) = (String::new(), String::new());
    for n in 0..200 {
        let (x, y) = (format!("c{n}"), format!("c{}", n + 1));
        if (hex(x.clone()) < hex(y.clone()))
            != (hex(format!("nest:{x}")) < hex(format!("nest:{y}")))
        {
            (a, b) = (x, y);
            break;
        }
    }
    assert!(!a.is_empty());
    let src = dir.path().join("e.jsonl");
    write_examples(&src, &[&a, &b]);
    let prefix = dir.path().join("o").display().to_string();
    nest(&src, &prefix, &[1]).unwrap();
    let first = &lines_of(Path::new(&format!("{prefix}-1.jsonl"))).unwrap()[0];
    let salted_first = if hex(format!("nest:{a}")) < hex(format!("nest:{b}")) {
        &a
    } else {
        &b
    };
    assert!(first.contains(&format!("\"{salted_first}\"")), "{first}");
    let err = nest(&src, &prefix, &[3]).unwrap_err().to_string();
    assert!(err.contains("3 examples asked of 2"), "{err}");
}

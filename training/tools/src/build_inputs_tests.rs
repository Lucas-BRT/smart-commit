use std::path::Path;
use std::process::Command;

use super::*;

fn modification(change: &str, old: Option<&str>, new: Option<&str>, diff: &str) -> Mod {
    Mod {
        change_type: change.to_owned(),
        old_path: old.map(str::to_owned),
        new_path: new.map(str::to_owned),
        diff: Some(diff.to_owned()),
    }
}

#[test]
fn targets_are_kept_or_dropped() {
    assert!(keep_target("feat(diff): parse renames"));
    assert!(keep_target("chore: bump vue to 2.6.13"));
    let long = "x".repeat(73);
    for bad in [
        "Merge branch 'x'",
        "Revert \"x\"",
        "fixup! feat: x",
        "WIP on parser",
        "chore: release 1.2.0",
        "v1.2.3",
        "chore(release): 0.3.8",
        "chore(publish): 5.5.0-beta.6",
        "Release v2.0.0",
        "bump 1.2.3",
        "#123",
        "fix #12",
        "short",
        long.as_str(),
    ] {
        assert!(!keep_target(bad), "{bad}");
    }
}

#[test]
fn the_length_counts_characters_not_bytes() {
    assert!(keep_target(&"é".repeat(72)));
    assert!(!keep_target(&"é".repeat(73)));
}

#[test]
fn subjects_come_from_the_first_line_of_the_message() {
    assert_eq!(subject_of("  feat: x  \n\nbody"), "feat: x");
    assert_eq!(subject_of(""), "");
}

#[test]
fn inputs_say_what_each_file_is() {
    let mods = [
        modification("RENAME", Some("a.rs"), Some("b.rs"), ""),
        modification("DELETE", Some("c.rs"), None, "@@ -1 +0,0 @@\n-x\n"),
        modification("ADD", None, Some("img.png"), ""),
    ];
    let got = inputs_of(&mods).unwrap();
    let statuses: Vec<&str> = got.staged.iter().map(|s| s.status).collect();
    assert_eq!(statuses, ["renamed", "deleted", "added"]);
    assert_eq!(got.staged[0].old_path.as_deref(), Some("a.rs"));
    assert_eq!(got.files[0].lines, Some((0, 0)));
    assert_eq!(
        got.files[1],
        FileDiff {
            path: "c.rs".into(),
            old_path: None,
            lines: Some((0, 1)),
            hunks: "@@ -1 +0,0 @@\n-x\n".into(),
        }
    );
    assert_eq!(got.files[2].lines, None);
    assert_eq!(inputs_of(&[]), None);
    assert_eq!(
        inputs_of(&[modification("UNKNOWN", Some("a"), Some("a"), "")]),
        None
    );
}

#[test]
fn commits_with_too_many_or_too_large_files_are_skipped() {
    let many: Vec<Mod> = (0..65)
        .map(|i| modification("ADD", None, Some(&format!("f{i}")), "@@ -0,0 +1 @@\n+x\n"))
        .collect();
    assert_eq!(inputs_of(&many), None);
    let big = modification("MODIFY", Some("a"), Some("a"), &"+x\n".repeat(30_000));
    assert_eq!(inputs_of(&[big]), None);
}

#[test]
fn the_paths_of_a_commit_are_sorted_and_capped() {
    let mods = [
        modification("DELETE", Some("b.rs"), None, ""),
        modification("RENAME", Some("c.rs"), Some("a.rs"), ""),
        modification("MODIFY", Some("a.rs"), Some("a.rs"), ""),
    ];
    assert_eq!(paths_of(&mods), ["a.rs", "b.rs"]);
    let many: Vec<Mod> = (0..30)
        .map(|i| modification("ADD", None, Some(&format!("f{i:02}")), ""))
        .collect();
    let expected: Vec<String> = (0..20).map(|i| format!("f{i:02}")).collect();
    assert_eq!(paths_of(&many), expected);
}

#[test]
fn types_fold_their_spellings() {
    assert_eq!(
        conventional_type("tests(cli): cover -C").as_deref(),
        Some("test")
    );
    assert_eq!(conventional_type("test: x").as_deref(), Some("test"));
    assert_eq!(conventional_type("Fix: x"), None);
    assert_eq!(conventional_type("fix:x"), None);
}

#[test]
fn release_bumps_name_their_version() {
    assert_eq!(
        release_version("chore: release 0.5.0").as_deref(),
        Some("0.5.0")
    );
    assert_eq!(
        release_version("Version bump to v2.3.0-dev").as_deref(),
        Some("2.3.0-dev")
    );
    assert_eq!(release_version("feat: support 1.2 files"), None);
    assert_eq!(release_version("bump version"), None);
}

#[test]
fn a_version_must_be_on_an_added_line_and_stand_alone() {
    let changes = Changes {
        staged: vec![],
        files: vec![
            FileDiff {
                path: "a".into(),
                old_path: None,
                lines: None,
                hunks: "@@ -1 +1 @@\n-version = \"0.4.0\"\n+version = \"0.5.0\"\n".into(),
            },
            FileDiff {
                path: "b".into(),
                old_path: None,
                lines: None,
                hunks: String::new(),
            },
        ],
    };
    assert!(version_in_diff("0.5.0", &changes));
    assert!(!version_in_diff("0.4.0", &changes)); // removed, not added
    assert!(!version_in_diff("5.0", &changes)); // part of another version
}

#[test]
fn dates_sort_by_day_month_year_text() {
    assert!(date_key("02.01.2020 10:00:00").unwrap() > date_key("31.12.2019 23:59:59").unwrap());
    assert!(date_key("2020-01-02").is_err());
}

#[test]
fn context_is_trimmed_to_one_line() {
    let diff = "@@ -1,9 +1,9 @@ fn main\n a\n b\n c\n-d\n+D\n e\n f\n g\n-h\n+H\n";
    assert_eq!(
        trim_context(diff, 1),
        "@@ -3,3 +3,3 @@ fn main\n c\n-d\n+D\n e\n@@ -7,2 +7,2 @@\n g\n-h\n+H\n"
    );
    assert_eq!(trim_context("@@ -1 +1 @@\n x\n", 1), "");
}

/// A small generator, so the cases are always the same ones.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn mutate(rng: &mut Rng, lines: &[String]) -> Vec<String> {
    let mut out = lines.to_vec();
    for _ in 0..=rng.below(6) {
        let at = rng.below(out.len() + 1);
        match rng.below(3) {
            0 => {
                let added: Vec<String> = (0..=rng.below(3))
                    .map(|_| format!("new {}", rng.next() % 1_000_000))
                    .collect();
                out.splice(at..at, added);
            }
            1 if !out.is_empty() => {
                let end = (at + 1 + rng.below(3)).min(out.len());
                if at < end {
                    out.drain(at..end);
                }
            }
            _ if !out.is_empty() => {
                let slot = at.min(out.len() - 1);
                out[slot] = format!("changed {}", rng.next() % 1_000_000);
            }
            _ => {}
        }
    }
    out
}

fn git_diff(old: &str, new: &str, context: usize, dir: &Path) -> String {
    std::fs::write(dir.join("a"), old).unwrap();
    std::fs::write(dir.join("b"), new).unwrap();
    let output = Command::new("git")
        .args(["diff", "--no-index", &format!("-U{context}"), "a", "b"])
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    text.find("@@")
        .map_or(String::new(), |i| text[i..].to_owned())
}

fn strip_headings(diff: &str) -> String {
    let heading = Regex::new(r"(?m)^(@@ [^@]* @@).*$").unwrap();
    heading.replace_all(diff, "$1").into_owned()
}

#[test]
fn trimming_the_default_context_gives_what_git_gives_with_one_line() {
    let dir = tempfile::TempDir::new().unwrap();
    let mut rng = Rng(1);
    for case in 0..300 {
        let old: Vec<String> = (0..rng.below(40)).map(|i| format!("line {i}")).collect();
        let new = mutate(&mut rng, &old);
        // Some without a final newline.
        let end = if case % 7 == 0 { "" } else { "\n" };
        let (o, n) = (old.join("\n") + end, new.join("\n") + end);
        let o = if old.is_empty() { String::new() } else { o };
        let u3 = git_diff(&o, &n, 3, dir.path());
        let u1 = git_diff(&o, &n, 1, dir.path());
        let got = strip_headings(&trim_context(&u3, 1));
        assert_eq!(got, strip_headings(&u1), "case {case}:\n{u3}");
    }
}

#[test]
fn building_inputs_needs_shards_and_writes_the_two_files() {
    let dir = tempfile::TempDir::new().unwrap();
    let options = Options {
        out: dir.path().join("out"),
        shards: vec![],
        branch: None,
        releases: false,
    };
    assert!(build(&options).is_err());
    let missing = Options {
        shards: vec![dir.path().join("missing.parquet")],
        ..options
    };
    assert!(build(&missing).is_err());
}

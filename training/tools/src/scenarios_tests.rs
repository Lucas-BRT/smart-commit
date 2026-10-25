use serde_json::json;

use super::*;

fn root() -> PathBuf {
    crate::build_scenarios::default_root()
}

fn table(text: &str) -> Table {
    text.parse().unwrap()
}

fn raises<T: std::fmt::Debug>(result: Result<T>, text: &str) {
    let error = result.unwrap_err().to_string();
    assert!(error.contains(text), "{text:?} not in {error:?}");
}

fn case(id: &str) -> Case {
    Case {
        id: id.into(),
        title: "t".into(),
        repo: "r".into(),
        level: 0,
        split: "eval".into(),
        target: "x".into(),
        ..Case::default()
    }
}

fn repos() -> Vec<Repo> {
    vec![Repo {
        name: "r".into(),
        convention: "free".into(),
        commits: vec![],
        files: PathBuf::new(),
    }]
}

fn tiny_repo(dir: &Path) -> Repo {
    let files = dir.join("files");
    std::fs::create_dir_all(files.join("src")).unwrap();
    std::fs::write(files.join("src/a.py"), "x = 1\ny = 2\n").unwrap();
    std::fs::write(files.join("README.md"), "# tiny\n").unwrap();
    let commits = [
        r#"message = "commit inicial"
paths = ["README.md"]"#,
        r#"message = "adiciona a.py"
paths = ["src/a.py"]"#,
        r#"message = "muda y"
ops = [{ op = "replace", path = "src/a.py", old = "y = 2", new = "y = 3" }]"#,
    ];
    Repo {
        name: "tiny".into(),
        convention: "free".into(),
        commits: commits.iter().map(|c| table(c)).collect(),
        files,
    }
}

#[test]
fn the_shipped_cases_are_valid() {
    let repos = load_repos(&root()).unwrap();
    let cases = load_cases(&root(), &repos).unwrap();
    let names: Vec<&str> = repos.iter().map(|r| r.name.as_str()).collect();
    assert!(names.contains(&"cafeteria") && names.contains(&"ledger"));
    assert!(cases.len() >= 80);
    for split in ["train", "eval"] {
        let levels: BTreeSet<i64> = cases
            .iter()
            .filter(|c| c.split == split)
            .map(|c| c.level)
            .collect();
        assert_eq!(levels, (0..6).collect(), "{split}");
    }
    for c in &cases {
        assert!(
            !(c.split == "train" && c.tags.iter().any(|t| EVAL_ONLY_TAGS.contains(&t.as_str()))),
            "{}",
            c.id
        );
    }
}

#[test]
fn variants_fill_in_their_variables() {
    let raw = table(
        r#"id = "v"
title = "t"
repo = "r"
split = "train"
target = "set {{name}}"
ops = [{ op = "write", path = "{{name}}.txt", text = "`${keep}`" }]
variants = [{ name = "one", vars = { name = "a" } }, { vars = { name = "b" }, split = "eval", target = "other" }]"#,
    );
    let defaults = table("level = 3");
    let cases = expand(&raw, &defaults).unwrap();
    let (one, two) = (&cases[0], &cases[1]);
    assert_eq!(
        (
            one.id.as_str(),
            one.split.as_str(),
            one.target.as_str(),
            one.level
        ),
        ("v/one", "train", "set a", 3)
    );
    assert_eq!(
        one.ops[0],
        table(
            r#"op = "write"
path = "a.txt"
text = "`${keep}`""#
        )
    );
    assert_eq!(
        (two.id.as_str(), two.split.as_str(), two.target.as_str()),
        ("v/2", "eval", "other")
    );
}

#[test]
fn without_variants_nothing_is_substituted() {
    let raw = table(
        r#"id = "v"
title = "t"
repo = "r"
split = "train"
target = "{{literal}}"
ops = []"#,
    );
    let cases = expand(&raw, &table("level = 0")).unwrap();
    assert_eq!(cases.len(), 1);
    assert_eq!(
        (cases[0].target.as_str(), cases[0].id.as_str()),
        ("{{literal}}", "v")
    );
}

#[test]
fn bad_cases_are_reported_with_their_id() {
    let base = r#"id = "v"
title = "t"
repo = "r"
split = "train"
target = "x"
ops = []
"#;
    let defaults = table("level = 0");
    let variables = base.replace("target = \"x\"", "target = \"{{x}}\"");
    raises(
        expand(
            &table(&format!(
                "{variables}variants = [{{ vars = {{ other = 1 }} }}]"
            )),
            &defaults,
        ),
        "unknown variable",
    );
    raises(
        expand(&table(&format!("{base}bogus = 1")), &defaults),
        "case v",
    );
    raises(expand(&table("id = \"v\""), &defaults), "case v");
}

#[test]
fn cases_are_validated() {
    validate(&[case("c")], &repos()).unwrap();
    raises(validate(&[case("c"), case("c")], &repos()), "duplicate id");
    raises(
        validate(
            &[Case {
                repo: "nope".into(),
                ..case("c")
            }],
            &repos(),
        ),
        "unknown repo",
    );
    raises(
        validate(
            &[Case {
                split: "test".into(),
                ..case("c")
            }],
            &repos(),
        ),
        "split must be",
    );
    raises(
        validate(
            &[Case {
                split: "train".into(),
                tags: vec!["git-state".into()],
                ..case("c")
            }],
            &repos(),
        ),
        "eval only",
    );
    raises(
        validate(
            &[Case {
                target: String::new(),
                ..case("c")
            }],
            &repos(),
        ),
        "no target",
    );
    validate(
        &[Case {
            target: String::new(),
            outcome: "error".into(),
            ..case("c")
        }],
        &repos(),
    )
    .unwrap();
    raises(
        validate(
            &[Case {
                ops: vec![table("op = \"teleport\"")],
                ..case("c")
            }],
            &repos(),
        ),
        "unknown op",
    );
}

#[test]
fn a_repository_history_is_deterministic() {
    let dir = tempfile::TempDir::new().unwrap();
    let repo = tiny_repo(dir.path());
    let mut heads = Vec::new();
    for n in 0..2 {
        let dest = dir.path().join(format!("r{n}"));
        let git = build_repo(&repo, &dest, true).unwrap();
        heads.push(git.run(&["rev-parse", "HEAD"]).unwrap().trim().to_owned());
        let log = git.run(&["log", "--format=%s"]).unwrap();
        assert_eq!(
            log.lines().take(3).collect::<Vec<_>>(),
            ["muda y", "adiciona a.py", "commit inicial"]
        );
        assert_eq!(
            std::fs::read_to_string(dest.join("src/a.py")).unwrap(),
            "x = 1\ny = 3\n"
        );
    }
    assert_eq!(heads[0], heads[1]);
    let fresh = dir.path().join("fresh");
    let git = build_repo(&repo, &fresh, false).unwrap();
    let status = git.run(&["status", "--short"]).unwrap();
    assert_eq!(
        status.split_whitespace().collect::<Vec<_>>(),
        ["??", "README.md", "??", "src/"]
    );
    assert_eq!(
        std::fs::read_to_string(fresh.join("src/a.py")).unwrap(),
        "x = 1\ny = 3\n"
    );
    std::fs::write(repo.files.join("stray.txt"), "").unwrap();
    raises(build_repo(&repo, &dir.path().join("r2"), true), "stray.txt");
}

fn op(text: &str) -> Table {
    table(text)
}

#[test]
fn operations_change_the_repository() {
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path();
    let git = Git::new(root);
    git.run(&["init", "-q", "-b", "main"]).unwrap();
    let big = root.join("big.txt");
    run_op(
        &op(r#"op = "write"
path = "big.txt"
text = "l{{i}}\n"
repeat = 3"#),
        root,
        &git,
    )
    .unwrap();
    assert_eq!(std::fs::read_to_string(&big).unwrap(), "l0\nl1\nl2\n");
    run_op(
        &op("op = \"append\"\npath = \"big.txt\"\ntext = \"end\\n\""),
        root,
        &git,
    )
    .unwrap();
    run_op(
        &op("op = \"prepend\"\npaths = [\"*.txt\"]\ntext = \"# h\\n\""),
        root,
        &git,
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(&big).unwrap(),
        "# h\nl0\nl1\nl2\nend\n"
    );
    raises(
        run_op(
            &op("op = \"replace\"\npath = \"big.txt\"\nold = \"l\"\nnew = \"L\""),
            root,
            &git,
        ),
        "3 occurrences",
    );
    run_op(
        &op("op = \"replace\"\npath = \"big.txt\"\nold = \"l\"\nnew = \"L\"\nall = true"),
        root,
        &git,
    )
    .unwrap();
    run_op(
        &op(r#"op = "regex"
path = "big.txt"
pattern = '^L(\d)$'
repl = '''n\1'''"#),
        root,
        &git,
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(&big).unwrap(),
        "# h\nn0\nn1\nn2\nend\n"
    );
    raises(
        run_op(
            &op("op = \"regex\"\npath = \"big.txt\"\npattern = \"zzz\"\nrepl = \"\""),
            root,
            &git,
        ),
        "no match",
    );
    run_op(
        &op("op = \"regex\"\npath = \"big.txt\"\npattern = \"\\n\"\nrepl = \"\\r\\n\""),
        root,
        &git,
    )
    .unwrap();
    assert_eq!(
        std::fs::read(&big)
            .unwrap()
            .windows(2)
            .filter(|w| w == b"\r\n")
            .count(),
        5
    );
}

#[test]
fn regexes_may_look_ahead() {
    // As in Python: `"(?!"")([^"\n]*)"` -> `'\1'`, which also turns the quotes
    // of a docstring into pairs.
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path();
    let git = Git::new(root);
    std::fs::write(root.join("a.py"), "x = \"one\"\ny = \"\"\"doc\"\"\"\n").unwrap();
    let spec = concat!(
        "op = \"regex\"\npath = \"a.py\"\n",
        "pattern = \"\\\"(?!\\\"\\\")([^\\\"\\\\n]*)\\\"\"\n",
        "repl = \"'\\\\1'\"\n"
    );
    run_op(&op(spec), root, &git).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("a.py")).unwrap(),
        "x = 'one'\ny = \"''doc\"''\n"
    );
}

#[test]
fn files_move_get_modes_links_binaries_and_vanish() {
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path();
    let git = Git::new(root);
    std::fs::write(root.join("a"), "x").unwrap();
    run_op(&op("op = \"move\"\nfrom = \"a\"\nto = \"d/b\""), root, &git).unwrap();
    assert!(root.join("d/b").is_file() && !root.join("a").exists());
    run_op(
        &op("op = \"chmod\"\npath = \"d/b\"\nmode = \"755\""),
        root,
        &git,
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(root.join("d/b"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
    }
    run_op(
        &op("op = \"symlink\"\npath = \"link\"\ntarget = \"d/b\""),
        root,
        &git,
    )
    .unwrap();
    assert_eq!(
        std::fs::read_link(root.join("link")).unwrap(),
        Path::new("d/b")
    );
    run_op(
        &op("op = \"binary\"\npath = \"b/x.png\"\nsize = 64\nseed = 1"),
        root,
        &git,
    )
    .unwrap();
    let first = std::fs::read(root.join("b/x.png")).unwrap();
    run_op(
        &op("op = \"binary\"\npath = \"b/x.png\"\nsize = 64\nseed = 1"),
        root,
        &git,
    )
    .unwrap();
    assert_eq!(first.len(), 64);
    assert_eq!(std::fs::read(root.join("b/x.png")).unwrap(), first);
    run_op(&op("op = \"remove\"\npath = \"d\""), root, &git).unwrap();
    assert!(!root.join("d").exists());
}

#[test]
fn operations_stay_inside_the_repository_and_need_their_files() {
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path();
    let git = Git::new(root);
    raises(
        run_op(
            &op("op = \"write\"\npath = \"../escape\"\ntext = \"\""),
            root,
            &git,
        ),
        "outside",
    );
    raises(
        run_op(
            &op("op = \"append\"\npath = \"none/*.py\"\ntext = \"\""),
            root,
            &git,
        ),
        "no file matches",
    );
    raises(run_op(&op("op = \"teleport\""), root, &git), "unknown op");
}

#[test]
fn a_commit_can_be_named_by_its_subject() {
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path();
    let git = Git::new(root);
    git.run(&["init", "-q", "-b", "main"]).unwrap();
    std::fs::write(root.join("f"), "x").unwrap();
    git.run(&["add", "-A"]).unwrap();
    git.run(&["commit", "-q", "-m", "um assunto único"])
        .unwrap();
    run_op(
        &op("op = \"git\"\nargs = [\"tag\", \"t\", \"{{commit:assunto único}}\"]"),
        root,
        &git,
    )
    .unwrap();
    assert_eq!(
        git.run(&["rev-parse", "t"]).unwrap(),
        git.run(&["rev-parse", "HEAD"]).unwrap()
    );
    raises(
        run_op(
            &op("op = \"git\"\nargs = [\"tag\", \"u\", \"{{commit:nenhum}}\"]"),
            root,
            &git,
        ),
        "0 commits",
    );
}

#[test]
fn a_case_stages_what_it_says() {
    let dir = tempfile::TempDir::new().unwrap();
    let repo = tiny_repo(dir.path());
    let staged_all = Case {
        repo: "tiny".into(),
        ops: vec![op("op = \"write\"\npath = \"new.py\"\ntext = \"z = 0\\n\"")],
        ..case("c")
    };
    let git = apply(&staged_all, &repo, &dir.path().join("a")).unwrap();
    assert_eq!(
        git.run(&["diff", "--cached", "--name-only"])
            .unwrap()
            .trim(),
        "new.py"
    );
    let partial = Case {
        repo: "tiny".into(),
        stage: "none".into(),
        ops: vec![
            op("op = \"replace\"\npath = \"src/a.py\"\nold = \"x = 1\"\nnew = \"x = 10\""),
            op("op = \"git\"\nargs = [\"add\", \"-A\"]"),
            op("op = \"replace\"\npath = \"README.md\"\nold = \"tiny\"\nnew = \"small\""),
        ],
        ..case("c")
    };
    let git = apply(&partial, &repo, &dir.path().join("b")).unwrap();
    assert_eq!(
        git.run(&["diff", "--cached", "--name-only"])
            .unwrap()
            .trim(),
        "src/a.py"
    );
    assert_eq!(
        git.run(&["diff", "--name-only"]).unwrap().trim(),
        "README.md"
    );
    let broken = Case {
        repo: "tiny".into(),
        ops: vec![op(
            "op = \"replace\"\npath = \"src/a.py\"\nold = \"q\"\nnew = \"r\"",
        )],
        ..case("c")
    };
    raises(
        apply(&broken, &repo, &dir.path().join("c")),
        "case c, op 1 (replace)",
    );
}

#[test]
fn expected_words_ignore_case_and_accents() {
    let patterns = |p: &[&str]| p.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    assert_eq!(
        expected("Corrige o PREÇO do café", &patterns(&["preco", "cafe|cha"])),
        Some(true)
    );
    assert_eq!(
        expected("corrige o preço", &patterns(&["preco", "cafe|cha"])),
        Some(false)
    );
    assert_eq!(expected("anything", &[]), None);
}

#[test]
fn the_meta_line_says_what_scoring_and_training_read() {
    let repo = Repo {
        name: "ledger".into(),
        convention: "conventional".into(),
        commits: vec![],
        files: PathBuf::new(),
    };
    let inputs = json!({"subjects": ["a", "b"]});
    let mut c = Case {
        repo: "ledger".into(),
        target: "feat(cli): add x".into(),
        level: 3,
        tags: vec!["feature".into()],
        ..case("c")
    };
    let m = meta_of(&c, &repo, &inputs);
    assert_eq!(
        (
            m["type"].as_str(),
            m["set"].as_str(),
            m["history"].as_u64(),
            m["level"].as_i64(),
            m["hash"].as_str()
        ),
        (
            Some("feat"),
            Some("conventional"),
            Some(2),
            Some(3),
            Some("c")
        )
    );
    let keys: Vec<&String> = m.as_object().unwrap().keys().collect();
    assert_eq!(
        keys,
        [
            "repo", "hash", "target", "type", "history", "set", "title", "level", "split", "tags",
            "expect"
        ]
    );
    c.target = "chore: initial commit".into();
    c.history = false;
    assert_eq!(meta_of(&c, &repo, &json!({"subjects": []}))["set"], "free");
    c.history = true;
    c.target = "Revert \"feat: x\"".into();
    let m = meta_of(&c, &repo, &inputs);
    assert_eq!(
        (m["type"].as_str(), m["set"].as_str()),
        (Some("revert"), Some("conventional"))
    );
}

/// Every case builds, twice to the same bytes, and lines up with its meta.
/// Needs the debug binary: `cargo build`, then `cargo test -- --ignored`.
#[test]
#[ignore = "needs the smart-commit binary: cargo build -p smart-commit first"]
fn every_case_builds_twice_to_the_same_bytes() {
    let binary = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/smart-commit");
    assert!(binary.is_file(), "build smart-commit first");
    let dir = tempfile::TempDir::new().unwrap();
    let (first, second) = (dir.path().join("a"), dir.path().join("b"));
    for out in [&first, &second] {
        let failures =
            crate::build_scenarios::build(&root(), out, binary.to_str().unwrap(), "all", "")
                .unwrap();
        assert_eq!(failures, 0);
    }
    for name in ["inputs.jsonl", "meta.jsonl"] {
        assert_eq!(
            std::fs::read(first.join(name)).unwrap(),
            std::fs::read(second.join(name)).unwrap(),
            "{name}"
        );
    }
    let repos = load_repos(&root()).unwrap();
    let cases = load_cases(&root(), &repos).unwrap();
    let count = cases
        .iter()
        .filter(|c| c.outcome == "message" && c.issue.is_empty())
        .count();
    let inputs = std::fs::read_to_string(first.join("inputs.jsonl")).unwrap();
    let meta = std::fs::read_to_string(first.join("meta.jsonl")).unwrap();
    assert_eq!(inputs.lines().count(), count);
    for (input, meta) in inputs.lines().zip(meta.lines()) {
        let (input, meta): (Value, Value) = (
            serde_json::from_str(input).unwrap(),
            serde_json::from_str(meta).unwrap(),
        );
        // A merge resolved to HEAD stages nothing; git prepared its message.
        let hash = meta["hash"].as_str().unwrap();
        assert!(
            input["staged"].as_array().is_some_and(|s| !s.is_empty())
                || input.get("prepared").is_some(),
            "{hash}"
        );
        let case = cases.iter().find(|c| c.id == hash).unwrap();
        assert_eq!(meta["target"].as_str().unwrap(), case.target);
    }
}

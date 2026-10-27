use std::path::PathBuf;

use super::types::TEMPLATE_OVERHEAD_TOKENS;
use super::*;
use crate::context::RepoContext;
use crate::diff::{self, FileDiff};
use crate::style;

fn context(subjects: &[&str], branch: Option<&str>) -> RepoContext {
    RepoContext::new(
        PathBuf::from("/repo"),
        branch.map(str::to_owned),
        subjects.iter().map(|s| s.to_string()).collect(),
        Vec::new(),
    )
}

fn files() -> Vec<FileDiff> {
    vec![
        FileDiff {
            path: "src/git.rs".into(),
            old_path: None,
            lines: Some((2, 1)),
            hunks: "@@ -80,3 +80,4 @@\n-        let out = self.git(&[\"branch\"])?;\n\
                    +        let out = self.git(&[\"branch\", \"--show-current\"])?;\n\
                    +        let name = out.trim();\n"
                .into(),
        },
        FileDiff {
            path: "Cargo.lock".into(),
            old_path: None,
            lines: Some((40, 3)),
            hunks: "@@ -1 +1 @@\n-x\n+y\n".into(),
        },
    ]
}

#[test]
fn annotated_file_list() {
    use crate::git::{ChangeStatus, FileChange};
    let staged = vec![
        FileChange {
            status: ChangeStatus::Modified,
            path: "src/git.rs".into(),
            old_path: None,
        },
        FileChange {
            status: ChangeStatus::Renamed,
            path: "src/vcs/repo.rs".into(),
            old_path: Some("src/repo.rs".into()),
        },
        FileChange {
            status: ChangeStatus::Modified,
            path: "src/style.rs".into(),
            old_path: None,
        },
    ];
    let ctx = RepoContext::new(
        PathBuf::from("/repo"),
        Some("main".into()),
        vec![
            "feat(git): read the branch".into(),
            "fix(diff): keep renames".into(),
        ],
        staged,
    );
    let mut changed = files();
    changed.truncate(1);
    changed.push(FileDiff {
        path: "src/vcs/repo.rs".into(),
        old_path: Some("src/repo.rs".into()),
        lines: Some((0, 0)),
        hunks: String::new(),
    });
    changed.push(FileDiff {
        path: "src/style.rs".into(),
        old_path: None,
        lines: Some((1, 1)),
        hunks: "@@ -3 +3 @@\n-fn detect(s: &[String]) -> Style {\n+fn detect(s: &[String]) -> Style {   \n"
            .into(),
    });
    let prompt = build(&ctx, &changed, &PromptOptions::default());
    insta::assert_snapshot!(prompt.user);
    // The plain list of the same change keeps the format trained on.
    let options = PromptOptions {
        diff: DiffFormat::Plain,
        ..PromptOptions::default()
    };
    let plain = build(&ctx, &changed, &options);
    assert!(
        plain
            .user
            .contains("src/repo.rs => src/vcs/repo.rs | +0 -0\n")
    );
    assert!(plain.user.contains("--- src/style.rs"));
}

#[test]
fn conventional_portuguese_repository() {
    let ctx = context(
        &[
            "feat(cli): adiciona o comando context",
            "fix(git): corrige a leitura da branch",
            "docs: atualiza a documentação da fase 1",
        ],
        Some("feature/login"),
    );
    let prompt = build(&ctx, &files(), &PromptOptions::default());
    insta::assert_snapshot!(format!("{}\n=====\n{}", prompt.system, prompt.user));
}

#[test]
fn scopes_are_listed_without_their_places() {
    // The places of each scope are recorded but not shown: alone they
    // did not help the models (training H4), and cost prompt tokens.
    let subjects = ["feat(cli): add context", "fix(git): read branch"];
    let paths = vec![
        vec!["src/main.rs".to_string()],
        vec!["src/git.rs".to_string()],
    ];
    let ctx = context(&subjects, None).with_history_files(paths);
    let prompt = build(&ctx, &files(), &PromptOptions::default());
    assert!(
        prompt
            .system
            .contains("- Scopes used here: cli, git. Omit the scope if none fits.\n"),
        "{}",
        prompt.system
    );
}

fn omitted() -> PromptOptions {
    PromptOptions {
        rules: PromptRules::Omitted,
        ..PromptOptions::default()
    }
}

#[test]
fn omitted_rules_keep_only_what_varies_by_repository() {
    let ctx = context(
        &[
            "feat(cli): add context",
            "fix(git): read branch",
            "docs: describe the hook",
        ],
        None,
    );
    let prompt = build(&ctx, &files(), &omitted());
    insta::assert_snapshot!(format!("{}\n=====\n{}", prompt.system, prompt.user));
    let full = build(&ctx, &files(), &PromptOptions::default());
    assert!(prompt.system.len() < full.system.len());
    // The diff is the same: only the instructions differ.
    assert!(prompt.user.contains("Diff:\n--- src/git.rs"));
}

#[test]
fn omitted_rules_keep_what_training_does_not_teach() {
    // Tickets, Portuguese and gitmoji are absent from the training data,
    // so the model still needs to be told.
    let gitmoji = context(
        &["✨ ABC-1 add login page", "🐛 ABC-2 fix crash on logout"],
        Some("feature/ABC-3-signup"),
    );
    let prompt = build(&gitmoji, &files(), &omitted());
    assert!(prompt.system.contains("Start the subject with one gitmoji"));
    assert!(
        prompt
            .system
            .contains("Start the description with ABC-3, right after the gitmoji.")
    );
    let portuguese = context(&["feat(cli): adiciona o comando context"], None);
    let options = PromptOptions {
        language: Some(Language::Portuguese),
        ..omitted()
    };
    let prompt = build(&portuguese, &files(), &options);
    assert!(prompt.system.contains("- Write in Brazilian Portuguese.\n"));
    // An English, Conventional repository without history needs no list.
    let prompt = build(&context(&[], None), &files(), &omitted());
    assert_eq!(prompt.system, "- Use Conventional Commits.\n");
}

#[test]
fn full_rules_are_left_out_of_recorded_options() {
    let full = serde_json::to_value(PromptOptions::default()).unwrap();
    assert!(full.get("rules").is_none(), "{full}");
    let value = serde_json::to_value(omitted()).unwrap();
    assert_eq!(value["rules"], "omitted");
    let back: PromptOptions = serde_json::from_value(value).unwrap();
    assert_eq!(back.rules, PromptRules::Omitted);
    // Options recorded before rules existed read as full.
    let old: PromptOptions = serde_json::from_value(full).unwrap();
    assert_eq!(old, PromptOptions::default());
}

#[test]
fn empty_history_uses_defaults() {
    let ctx = context(&[], None);
    let prompt = build(&ctx, &files(), &PromptOptions::default());
    insta::assert_snapshot!(prompt.system);
}

#[test]
fn gitmoji_repository_with_ticket_prefix() {
    let ctx = context(
        &["✨ ABC-1 add login page", "🐛 ABC-2 fix crash on logout"],
        Some("feature/ABC-3-signup"),
    );
    let prompt = build(&ctx, &files(), &PromptOptions::default());
    assert!(prompt.system.contains("Start the subject with one gitmoji"));
    assert!(
        prompt
            .system
            .contains("Start the description with ABC-3, right after the gitmoji.")
    );
}

#[test]
fn ticket_prefix_and_mention_in_free_style() {
    let prefixed = context(&["ABC-1: add login", "[ABC-2] fix crash"], Some("ABC-3"));
    let prompt = build(&prefixed, &files(), &PromptOptions::default());
    assert!(prompt.system.contains("Start the subject with ABC-3."));

    let mentioned = context(&["add login (ABC-1)", "fix crash, ABC-2"], Some("ABC-3"));
    let prompt = build(&mentioned, &files(), &PromptOptions::default());
    assert!(prompt.system.contains("Mention ABC-3 in the subject."));
}

#[test]
fn forced_style_overrides_the_detected_one() {
    let options = PromptOptions {
        convention: Some(Convention::Conventional),
        language: Some(Language::Portuguese),
        ..PromptOptions::default()
    };
    let english_free = style::detect(&["Add the parser", "Fix the crash"]);
    assert_eq!(
        effective_style(&english_free, &options),
        (Convention::Conventional, Language::Portuguese)
    );
}

#[test]
fn forced_convention_drops_examples_that_contradict_it() {
    let ctx = context(
        &["feat: add the parser", "Fix the crash on empty input"],
        None,
    );
    let forced = PromptOptions {
        convention: Some(Convention::Conventional),
        ..PromptOptions::default()
    };
    let prompt = build(&ctx, &files(), &forced);
    assert!(prompt.system.contains("Use Conventional Commits"));
    assert!(prompt.system.contains("feat: add the parser"));
    assert!(!prompt.system.contains("Fix the crash on empty input"));

    // Gitmoji forced on a history without any: no examples at all.
    let gitmoji = PromptOptions {
        convention: Some(Convention::Gitmoji),
        ..PromptOptions::default()
    };
    let prompt = build(&ctx, &files(), &gitmoji);
    assert!(!prompt.system.contains("Recent subjects"));
}

#[test]
fn the_subject_limit_is_told() {
    let ctx = context(
        &["feat(git): read the branch", "fix(diff): keep renames"],
        None,
    );
    let full = build(&ctx, &files(), &PromptOptions::default());
    assert!(
        full.system
            .contains("Subject line of at most 72 characters")
    );
    let wide = PromptOptions {
        subject_max: 100,
        ..PromptOptions::default()
    };
    assert!(
        build(&ctx, &files(), &wide)
            .system
            .contains("Subject line of at most 100 characters")
    );
    // Without the rules, 72 is learnt from the data; another limit is told.
    let omitted = PromptOptions {
        rules: PromptRules::Omitted,
        ..PromptOptions::default()
    };
    assert!(
        !build(&ctx, &files(), &omitted)
            .system
            .contains("Subject line")
    );
    let omitted_wide = PromptOptions {
        subject_max: 100,
        ..omitted
    };
    assert!(
        build(&ctx, &files(), &omitted_wide)
            .system
            .contains("at most 100 characters")
    );
    // Recorded in traces only when it is not 72.
    assert!(
        !serde_json::to_string(&PromptOptions::default())
            .unwrap()
            .contains("subject_max")
    );
    let json = serde_json::to_string(&wide).unwrap();
    assert!(json.contains(r#""subject_max":100"#), "{json}");
    assert_eq!(serde_json::from_str::<PromptOptions>(&json).unwrap(), wide);
}

#[test]
fn unset_overrides_are_left_out_of_traces() {
    let json = serde_json::to_string(&PromptOptions::default()).unwrap();
    assert!(!json.contains("\"convention\""), "{json}");
    let old: PromptOptions = serde_json::from_str(
        r#"{"max_prompt_tokens":1024,"default_convention":"conventional","default_language":"english"}"#,
    )
    .unwrap();
    // A trace from before the annotated list read its prompts plain.
    assert_eq!(
        old,
        PromptOptions {
            diff: DiffFormat::Plain,
            ..PromptOptions::default()
        }
    );
    // Today's default is recorded, and read back as it was.
    assert!(json.contains(r#""diff":"annotated""#), "{json}");
    let back: PromptOptions = serde_json::from_str(&json).unwrap();
    assert_eq!(back, PromptOptions::default());
}

#[test]
fn known_style_overrides_the_defaults() {
    let options = PromptOptions {
        default_convention: Convention::Gitmoji,
        default_language: Language::Portuguese,
        ..PromptOptions::default()
    };
    let empty = style::detect::<&str>(&[]);
    assert_eq!(
        effective_style(&empty, &options),
        (Convention::Gitmoji, Language::Portuguese)
    );
    let english = style::detect(&["Add the parser", "Fix the crash"]);
    assert_eq!(
        effective_style(&english, &options),
        (Convention::Free, Language::English)
    );
}

#[test]
fn ticket_is_left_out_when_the_repository_does_not_cite_them() {
    let ctx = context(&["Add login page", "Fix crash"], Some("ABC-3-signup"));
    let prompt = build(&ctx, &files(), &PromptOptions::default());
    assert!(!prompt.system.contains("ABC-3"));
}

#[test]
fn note_goes_last_and_the_diff_makes_room_for_it() {
    let big: Vec<FileDiff> = (0..20)
        .map(|n| FileDiff {
            path: format!("src/m{n}.rs"),
            old_path: None,
            lines: Some((60, 0)),
            hunks: format!(
                "@@ -0,0 +1,60 @@\n{}",
                "+let value = compute();\n".repeat(60)
            ),
        })
        .collect();
    let ctx = context(&["feat: add a thing to the parser"], None);
    let options = PromptOptions::default();
    let note = "Your previous reply was rejected. ".repeat(10);

    let plain = build(&ctx, &big, &options);
    let noted = build_with_note(&ctx, &big, &options, Some(&note));
    assert_eq!(noted.system, plain.system);
    assert!(noted.user.ends_with(&format!("\n{note}\n")));
    assert!(noted.user.len() - note.len() < plain.user.len());
    let used = diff::estimate_tokens(&noted.system)
        + diff::estimate_tokens(&noted.user)
        + TEMPLATE_OVERHEAD_TOKENS;
    assert!(used <= options.max_prompt_tokens, "{used}");
}

#[test]
fn large_diff_fits_the_prompt_budget() {
    let big: Vec<FileDiff> = (0..50)
        .map(|n| FileDiff {
            path: format!("src/module_{n}.rs"),
            old_path: None,
            lines: Some((60, 0)),
            hunks: format!(
                "@@ -0,0 +1,60 @@\n{}",
                "+let value = compute();\n".repeat(60)
            ),
        })
        .collect();
    let ctx = context(&["feat: add a thing to the parser"], None);
    let options = PromptOptions::default();
    let prompt = build(&ctx, &big, &options);
    let used = diff::estimate_tokens(&prompt.system)
        + diff::estimate_tokens(&prompt.user)
        + TEMPLATE_OVERHEAD_TOKENS;
    assert!(used <= options.max_prompt_tokens, "{used}");
    assert!(prompt.user.contains("partly shown"));
}

fn map() -> PromptOptions {
    PromptOptions {
        scopes: PromptScopes::Map,
        ..PromptOptions::default()
    }
}

fn training_history() -> RepoContext {
    let paths = |p: &[&str]| p.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    context(
        &[
            "feat(training): add train.py",
            "docs(training): record results",
            "fix(cli): parse -C",
            "feat(training): add score.py",
        ],
        None,
    )
    .with_history_files(vec![
        paths(&["training/train.py"]),
        paths(&["docs/training.md", "training/HYPOTHESES.md"]),
        paths(&["src/main.rs", "tests/cli.rs"]),
        paths(&["training/score.py"]),
    ])
}

#[test]
fn scopes_show_where_their_commits_went() {
    let prompt = build(&training_history(), &files(), &map());
    insta::assert_snapshot!(prompt.system);
}

#[test]
fn the_map_is_told_the_same_way_when_the_rules_are_omitted() {
    let options = PromptOptions {
        rules: PromptRules::Omitted,
        ..map()
    };
    let prompt = build(&training_history(), &files(), &options);
    assert!(
        prompt
            .system
            .contains("- Scopes used here, with where their commits went: training (training/, docs/), cli (src/, tests/)."),
        "{}",
        prompt.system
    );
}

#[test]
fn scopes_without_history_files_are_listed_even_when_the_map_is_asked() {
    let subjects = ["feat(cli): add context", "fix(git): read branch"];
    let list = "- Scopes used here: cli, git. Omit the scope if none fits.\n";
    // Older traces have no paths, and empty commits touch none.
    let without = context(&subjects, None);
    let empty = context(&subjects, None).with_history_files(vec![vec![], vec![]]);
    for ctx in [without, empty] {
        let prompt = build(&ctx, &files(), &map());
        assert!(prompt.system.contains(list), "{}", prompt.system);
    }
}

#[test]
fn a_scope_without_places_is_listed_alone_in_the_map() {
    let subjects = ["feat(cli): add context", "fix(git): read branch"];
    let ctx =
        context(&subjects, None).with_history_files(vec![vec!["src/cli.rs".to_string()], vec![]]);
    let prompt = build(&ctx, &files(), &map());
    assert!(
        prompt
            .system
            .contains("with where their commits went: cli (src/), git. Use a scope"),
        "{}",
        prompt.system
    );
}

#[test]
fn the_map_does_not_take_room_from_the_diff() {
    let big: Vec<FileDiff> = (0..50)
        .map(|n| FileDiff {
            path: format!("src/module_{n}.rs"),
            old_path: None,
            lines: Some((60, 0)),
            hunks: format!(
                "@@ -0,0 +1,60 @@\n{}",
                "+let value = compute();\n".repeat(60)
            ),
        })
        .collect();
    let ctx = training_history();
    let listed = build(&ctx, &big, &PromptOptions::default());
    let mapped = build(&ctx, &big, &map());
    assert!(mapped.system.len() > listed.system.len());
    assert_eq!(mapped.user, listed.user);
}

#[test]
fn the_map_is_recorded_only_when_asked() {
    let json = serde_json::to_string(&PromptOptions::default()).unwrap();
    assert!(!json.contains("\"scopes\""), "{json}");
    let json = serde_json::to_string(&map()).unwrap();
    assert!(json.contains("\"scopes\":\"map\""), "{json}");
    let read: PromptOptions = serde_json::from_str(&json).unwrap();
    assert_eq!(read, map());
}

/// Eight scopes whose commits touched three places each, `depth` directories deep.
fn deep_monorepo(depth: usize) -> RepoContext {
    let dir = |scope: usize, place: usize| {
        let mut path = String::from("packages/manager/apps");
        for level in 0..depth {
            path.push_str(&format!("/area{scope}_{place}_{level}"));
        }
        path.push_str("/file.ts");
        path
    };
    let subjects: Vec<String> = (0..8).map(|n| format!("feat(scope{n}): change")).collect();
    let files: Vec<Vec<String>> = (0..8)
        .map(|n| (0..3).map(|place| dir(n, place)).collect())
        .collect();
    let subjects: Vec<&str> = subjects.iter().map(String::as_str).collect();
    context(&subjects, None).with_history_files(files)
}

#[test]
fn a_long_map_shows_fewer_places_per_scope() {
    // The map of the scopes' line, and how many places each scope has in it.
    let map_of = |depth| {
        let system = build(&deep_monorepo(depth), &files(), &map()).system;
        let line = system
            .lines()
            .find(|l| l.contains("where their commits went: "))
            .unwrap_or_else(|| panic!("no map in {system}"))
            .to_owned();
        let map = line
            .split_once("went: ")
            .and_then(|(_, rest)| rest.split_once(". Use a scope"))
            .map(|(map, _)| map.to_owned())
            .unwrap();
        let places = map.matches("packages/manager/apps/").count() / 8;
        (map, places)
    };
    let (short, places) = map_of(1);
    assert_eq!(places, 3, "{short}");
    let (medium, places) = map_of(3);
    assert!((1..3).contains(&places), "{places} places in {medium}");
    assert!(medium.chars().count() <= 1000, "{medium}");
}

#[test]
fn a_map_too_long_even_with_one_place_falls_back_to_the_names() {
    let prompt = build(&deep_monorepo(12), &files(), &map());
    assert!(
        prompt.system.contains(
            "- Scopes used here: scope0, scope1, scope2, scope3, scope4, scope5, scope6, scope7. \
             Omit the scope if none fits.\n"
        ),
        "{}",
        prompt.system
    );
}

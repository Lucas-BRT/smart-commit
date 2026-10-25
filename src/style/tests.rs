#[cfg(test)]
mod tests {
    use crate::style::detect::is_noise;
    use crate::style::language::description_of;
    use crate::style::*;

    #[test]
    fn scope_areas_rank_places_once_per_commit() {
        let subjects = [
            "feat(cli): add context",
            "fix(cli): parse -C",
            "docs: readme",
            "fix(cli): wide change",
            "feat(git): history",
        ];
        let files: Vec<Vec<String>> = [
            &["src/main.rs", "tests/cli.rs"][..],
            &["src/main.rs"],
            &["README.md"],
            &["src/a.rs", "src/b.rs", "src/c.rs", "docs/usage.md"],
            &["src/git.rs"],
        ]
        .iter()
        .map(|p| p.iter().map(|s| s.to_string()).collect())
        .collect();
        assert_eq!(
            scope_areas("cli", &subjects, &files, 3),
            ["src/", "docs/", "tests/"]
        );
        assert_eq!(scope_areas("cli", &subjects, &files, 1), ["src/"]);
        assert_eq!(scope_areas("git", &subjects, &files, 3), ["src/"]);
        assert!(scope_areas("api", &subjects, &files, 3).is_empty());
        // Root files are their own place; missing files lists are no places.
        let root = vec![vec!["mise.toml".to_string()]];
        assert_eq!(
            scope_areas("ci", &["ci(x): y"], &root, 3),
            Vec::<String>::new()
        );
        assert_eq!(
            scope_areas("ci", &["ci: y"], &root, 3),
            Vec::<String>::new()
        );
        assert_eq!(scope_areas("x", &["ci(x): y"], &root, 3), ["mise.toml"]);
        assert!(scope_areas("x", &["ci(x): y"], &[], 3).is_empty());
    }

    #[test]
    fn parses_full_conventional_header() {
        assert_eq!(
            parse_conventional("feat(api)!: drop v1 endpoints"),
            Some(ConventionalHeader {
                kind: "feat",
                scope: Some("api"),
                breaking: true,
                description: "drop v1 endpoints",
            })
        );
    }

    #[test]
    fn parses_header_without_scope() {
        let header = parse_conventional("fix: handle empty input").unwrap();
        assert_eq!((header.kind, header.scope), ("fix", None));
        assert!(!header.breaking);
    }

    #[test]
    fn rejects_non_conventional_subjects() {
        for subject in [
            "Fix: capitalised type",
            "fix:no space",
            "fix: ",
            "fix(): empty scope",
            "fix(api: unclosed scope",
            ": no type",
            "Update README.md",
            "",
        ] {
            assert_eq!(parse_conventional(subject), None, "{subject:?}");
        }
    }

    #[test]
    fn recognises_gitmoji() {
        assert!(is_gitmoji("✨ add login page"));
        assert!(is_gitmoji("🐛 fix crash on empty diff"));
        assert!(is_gitmoji("♻️ refactor parser"));
        assert!(is_gitmoji(":sparkles: add login page"));
        assert!(!is_gitmoji(":: not a shortcode"));
        assert!(!is_gitmoji(":Not_lowercase: nope"));
        assert!(!is_gitmoji("add login page"));
        assert!(!is_gitmoji(""));
    }

    #[test]
    fn empty_history_is_unknown() {
        let style = detect::<&str>(&[]);
        assert_eq!(style.convention, Convention::Unknown);
        assert_eq!(style.language, Language::Unknown);
        assert_eq!(style.sample_size, 0);
        assert!(style.examples.is_empty());
    }

    #[test]
    fn detects_conventional_commits_with_ranked_scopes() {
        let style = detect(&[
            "feat(cli): add --dry-run flag",
            "fix(git): handle detached HEAD",
            "feat(cli): print detected style",
            "docs: describe phase 1",
            "Merge pull request #3 from someone/branch",
        ]);
        assert_eq!(style.convention, Convention::Conventional);
        assert_eq!(
            style.types,
            vec![("feat".into(), 2), ("docs".into(), 1), ("fix".into(), 1)]
        );
        assert_eq!(style.scopes, vec![("cli".into(), 2), ("git".into(), 1)]);
        assert_eq!(
            style.examples.len(),
            4,
            "the merge subject is not an example"
        );
    }

    #[test]
    fn detects_gitmoji() {
        let style = detect(&["✨ add login", "🐛 fix crash", ":memo: update docs", "bump"]);
        assert_eq!(style.convention, Convention::Gitmoji);
        assert_eq!(
            style.examples,
            vec!["✨ add login", "🐛 fix crash", ":memo: update docs"]
        );
    }

    #[test]
    fn mixed_history_below_threshold_is_free() {
        let style = detect(&["feat: one", "fix: two", "Update README", "Initial commit"]);
        assert_eq!(style.convention, Convention::Free);
    }

    #[test]
    fn detects_portuguese_ignoring_conventional_prefix() {
        let style = detect(&[
            "fix: corrige o parser de diff",
            "feat: adiciona suporte para gitmoji",
            "docs: atualiza a documentação da fase 1",
        ]);
        assert_eq!(style.language, Language::Portuguese);
    }

    #[test]
    fn detects_english() {
        let style = detect(&[
            "Add support for renames",
            "Fix crash when the diff is empty",
        ]);
        assert_eq!(style.language, Language::English);
    }

    #[test]
    fn type_words_alone_do_not_make_english() {
        let style = detect(&["fix: parser", "feat: gitmoji"]);
        assert_eq!(style.language, Language::Unknown);
    }

    #[test]
    fn gitmoji_prefix_is_stripped_for_language() {
        assert_eq!(description_of(":bug: corrige crash"), "corrige crash");
        assert_eq!(description_of("♻️ refatora parser"), "refatora parser");
    }

    #[test]
    fn language_of_one_subject() {
        for (subject, language) in [
            ("add main", Language::English),
            ("Add basic 3D shapes", Language::English),
            ("update requirements", Language::English),
            ("feat(cli): add the eval command", Language::English),
            ("adiciona contagem de itens ao pedido", Language::Portuguese),
            ("fix: corrige o total do pedido", Language::Portuguese),
            // Mixed, as Portuguese repositories often write: unknown.
            ("fix do keyboard", Language::Unknown),
            ("add paletas de cores", Language::Unknown),
            ("zoom em 0.05 até 10", Language::Portuguese),
            ("chore: bump typescript", Language::Unknown),
            // Found on eval-v1: English subjects with words Portuguese has too.
            ("refactor: remove unused exports", Language::Unknown),
            ("fix: do not crash on empty input", Language::Unknown),
            ("feat: use a Set as the cache", Language::English),
            ("fix do desktop", Language::Unknown),
        ] {
            assert_eq!(language_of(subject), language, "{subject}");
        }
    }

    #[test]
    fn detects_ticket_prefix() {
        let style = detect(&[
            "ABC-12: fix login",
            "[ABC-13] add logout",
            "chore: bump deps",
        ]);
        assert_eq!(style.tickets, TicketStyle::Prefix);
    }

    #[test]
    fn ticket_after_convention_prefix_is_a_prefix() {
        let gitmoji = detect(&["✨ ABC-1 add login page", "🐛 ABC-2 fix crash"]);
        assert_eq!(gitmoji.tickets, TicketStyle::Prefix);
        let conventional = detect(&["feat: [ABC-1] add login", "fix(api): ABC-2 crash"]);
        assert_eq!(conventional.tickets, TicketStyle::Prefix);
    }

    #[test]
    fn detects_ticket_mention() {
        let style = detect(&["fix login (ABC-12)", "add logout, refs OPS-1"]);
        assert_eq!(style.tickets, TicketStyle::Mention);
    }

    #[test]
    fn occasional_tickets_are_ignored() {
        let style = detect(&["ABC-12: fix login", "add logout", "bump deps"]);
        assert_eq!(style.tickets, TicketStyle::None);
        assert_eq!(detect::<&str>(&[]).tickets, TicketStyle::None);
    }

    #[test]
    fn recognises_noise_subjects() {
        for subject in [
            "fixup! feat: add flag",
            "squash! fix parser",
            "Revert \"feat: add flag\"",
            "WIP",
            "wip: half done",
            "[WIP] login page",
        ] {
            assert!(is_noise(subject), "{subject:?}");
        }
        for subject in [
            "wipe stale cache",
            "Revert to the old parser",
            "feat: add flag",
        ] {
            assert!(!is_noise(subject), "{subject:?}");
        }
    }

    #[test]
    fn examples_skip_noise_duplicates_and_bad_lengths() {
        let long = format!("feat: {}", "x".repeat(120));
        let style = detect(&[
            "fixup! feat: add flag",
            "WIP: half done",
            "feat: add flag",
            "feat: add flag",
            "fix: ok",
            long.as_str(),
            "feat: one",
            "feat: two thing",
            "feat: three thing",
            "feat: four thing",
            "feat: five thing",
        ]);
        assert_eq!(
            style.examples,
            vec![
                "feat: add flag",
                "feat: one",
                "feat: two thing",
                "feat: three thing",
                "feat: four thing",
            ]
        );
    }
}

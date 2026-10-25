use super::*;

fn file(path: &str, hunks: &str) -> FileDiff {
    let added = hunks.lines().filter(|l| l.starts_with('+')).count();
    let deleted = hunks.lines().filter(|l| l.starts_with('-')).count();
    FileDiff {
        path: path.to_owned(),
        old_path: None,
        lines: Some((added, deleted)),
        hunks: hunks.to_owned(),
    }
}

fn change(status: ChangeStatus, path: &str, old: Option<&str>) -> FileChange {
    FileChange {
        status,
        path: path.to_owned(),
        old_path: old.map(str::to_owned),
    }
}

#[test]
fn whitespace_changes() {
    for (hunks, spacing) in [
        (
            "@@ -1,2 +1,2 @@\n-x = 1\n-y = 2\n+x = 1   \n+y = 2  \n",
            Spacing::TrailingSpaces,
        ),
        (
            "@@ -1,2 +1,2 @@\n-x = 1\r\n-y = 2\r\n+x = 1\n+y = 2\n",
            Spacing::LineEndings { to_lf: true },
        ),
        (
            "@@ -1 +1 @@\n-x = 1\n+x = 1\r\n",
            Spacing::LineEndings { to_lf: false },
        ),
        (
            "@@ -1 +1 @@\n-tabulate==0.9.0\n\\ No newline at end of file\n+tabulate==0.9.0\n",
            Spacing::FinalNewline,
        ),
        (
            "@@ -1,2 +1,2 @@\n-if x:\n-  go()\n+if x:\n+    go()\n",
            Spacing::Indentation,
        ),
        ("@@ -1,2 +1,4 @@\n a\n+\n+\n b\n", Spacing::BlankLines),
        (
            "@@ -1 +1,3 @@\n-f(a, b)\n+f(\n+    a, b\n+)\n",
            Spacing::LineBreaks,
        ),
    ] {
        assert_eq!(
            change_of(&file("a.py", hunks)),
            Change::Whitespace(spacing),
            "{hunks}"
        );
    }
    assert!(Spacing::LineBreaks.shows_in_diff());
    assert!(!Spacing::TrailingSpaces.shows_in_diff());
    assert_eq!(
        change_of(&file("a.py", "@@ -1 +1 @@\n-x = 1\n+x = 2\n")),
        Change::Content
    );
}

#[test]
fn comment_changes() {
    let python = [
        "@@ -1,2 +1,3 @@\n def run():\n+    # read the input first\n     go()\n",
        "@@ -1,2 +1,3 @@\n def run():\n+    \"\"\"Main loop: reads, updates, draws.\"\"\"\n     go()\n",
        "@@ -1 +1,4 @@\n def run():\n+    \"\"\"Main loop.\n+\n+    Reads, updates, draws.\n+    \"\"\"\n",
    ];
    for hunks in python {
        assert_eq!(
            change_of(&file("game.py", hunks)),
            Change::Comments,
            "{hunks}"
        );
    }
    let ts = "@@ -1 +1,4 @@\n+/**\n+ * Sums cents.\n+ */\n export function sum() {}\n";
    assert_eq!(change_of(&file("money.ts", ts)), Change::Comments);
    assert_eq!(
        change_of(&file("money.ts", "@@ -1 +1 @@\n-// acount\n+// account\n")),
        Change::Comments
    );
    // Code next to a comment, or a language without known comments.
    let mixed = "@@ -1 +1,2 @@\n+# new limit\n+LIMIT = 10\n";
    assert_eq!(change_of(&file("a.py", mixed)), Change::Content);
    assert_eq!(
        change_of(&file("a.md", "@@ -1 +1 @@\n+# Title\n")),
        Change::Content
    );
    // A string that only looks like a comment start is code.
    assert_eq!(
        change_of(&file("a.py", "@@ -1 +1 @@\n-x = \"#1\"\n+x = \"#2\"\n")),
        Change::Content
    );
}

#[test]
fn no_lines_and_binaries() {
    assert_eq!(change_of(&file("run.sh", "")), Change::NoLines);
    let binary = FileDiff {
        lines: None,
        ..file("logo.png", "")
    };
    assert_eq!(change_of(&binary), Change::Content);
}

#[test]
fn rename_paths() {
    assert_eq!(
        rename_path("src/controls.py", "src/input.py"),
        "src/{controls.py => input.py}"
    );
    assert_eq!(
        rename_path("src/objects/chair.py", "src/models/chair.py"),
        "src/{objects => models}/chair.py"
    );
    assert_eq!(rename_path("a.rs", "b/a.rs"), "{a.rs => b/a.rs}");
    assert_eq!(
        rename_path("x/y/z.rs", "x/y/z.rs.bak"),
        "x/y/{z.rs => z.rs.bak}"
    );
}

#[test]
fn statuses_in_the_list() {
    let files = [
        file("src/new.py", "@@ -0,0 +1,2 @@\n+a = 1\n+b = 2\n"),
        file("src/old.py", "@@ -1 +0,0 @@\n-gone = True\n"),
        file("src/py.typed", ""),
        file("scripts/run.sh", ""),
        FileDiff {
            old_path: Some("src/controls.py".into()),
            ..file("src/input.py", "")
        },
        file("src/state.py", "@@ -1 +1 @@\n-x = 1\n+x = 1  \n"),
        file(
            "src/game.py",
            "@@ -1 +1,2 @@\n def run():\n+    # main loop\n",
        ),
        file("src/cli.py", "@@ -1 +1 @@\n-x = 1\n+x = 2\n"),
    ];
    let staged = [
        change(ChangeStatus::Added, "src/new.py", None),
        change(ChangeStatus::Deleted, "src/old.py", None),
        change(ChangeStatus::Added, "src/py.typed", None),
        change(ChangeStatus::Modified, "scripts/run.sh", None),
        change(
            ChangeStatus::Renamed,
            "src/input.py",
            Some("src/controls.py"),
        ),
        change(ChangeStatus::Modified, "src/state.py", None),
        change(ChangeStatus::Modified, "src/game.py", None),
        change(ChangeStatus::Modified, "src/cli.py", None),
    ];
    assert_eq!(
        stat_lines(&files, &staged).concat(),
        "src/new.py | new, +2 -0\n\
         src/old.py | deleted, +0 -1\n\
         src/py.typed | new, empty, +0 -0\n\
         scripts/run.sh | no line changed (mode), +0 -0\n\
         src/{controls.py => input.py} | renamed, unchanged, +0 -0\n\
         src/state.py | whitespace only: trailing spaces, +1 -1\n\
         src/game.py | comments only, +1 -0\n\
         src/cli.py | +1 -1\n"
    );
}

#[test]
fn renames_between_two_directories_take_one_line() {
    let mut files: Vec<FileDiff> = ["chair", "table", "lamp"]
        .iter()
        .map(|name| FileDiff {
            old_path: Some(format!("src/objects/{name}.py")),
            ..file(&format!("src/models/{name}.py"), "@@ -1 +1 @@\n-a\n+b\n")
        })
        .collect();
    files.insert(1, file("src/game.py", "@@ -1 +1 @@\n-a\n+b\n"));
    let staged: Vec<FileChange> = files
        .iter()
        .map(|f| match &f.old_path {
            Some(old) => change(ChangeStatus::Renamed, &f.path, Some(old)),
            None => change(ChangeStatus::Modified, &f.path, None),
        })
        .collect();
    assert_eq!(
        stat_lines(&files, &staged),
        [
            "src/{objects => models}/ | 3 files renamed, +3 -3\n",
            "src/game.py | +1 -1\n"
        ]
    );
    // Two are not a group.
    files.truncate(3);
    assert_eq!(stat_lines(&files, &staged[..3]).len(), 3);
}

#[test]
fn a_deletion_like_a_new_file_is_named() {
    let old = "@@ -1,4 +0,0 @@\n-class Config:\n-    loja = \"Café\"\n-    moeda = \"R$\"\n-    taxa = 0.12\n";
    let new = "@@ -0,0 +1,6 @@\n+class Ajustes(NamedTuple):\n+    loja = \"Café\"\n+    moeda = \"R$\"\n+    taxa = 0.12\n+\n+PADRAO = Ajustes()\n";
    let files = [
        file("src/config.py", old),
        file("src/ajustes.py", new),
        file("src/other.py", "@@ -0,0 +1 @@\n+unrelated = 1\n"),
    ];
    let staged = [
        change(ChangeStatus::Deleted, "src/config.py", None),
        change(ChangeStatus::Added, "src/ajustes.py", None),
        change(ChangeStatus::Added, "src/other.py", None),
    ];
    let lines = stat_lines(&files, &staged);
    assert_eq!(
        lines[1],
        "src/ajustes.py | new, maybe renamed from src/config.py, +6 -0\n"
    );
    assert_eq!(lines[2], "src/other.py | new, +1 -0\n");
}

#[test]
fn unknown_files_keep_the_plain_line() {
    // Recorded inputs without a staged entry for the file.
    let files = [file("src/a.rs", "@@ -1 +1 @@\n-a\n+b\n")];
    assert_eq!(stat_lines(&files, &[]), ["src/a.rs | +1 -1\n"]);
}

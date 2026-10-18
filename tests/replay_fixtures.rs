//! Replays every recorded trace in `tests/fixtures/traces`.
//!
//! A failure means the pipeline between git and the model changed. If that
//! was intended, regenerate the fixtures with `tests/fixtures/make-traces.sh`
//! and review the diff.

use std::fs;
use std::path::Path;

use smart_commit::trace::Trace;

#[test]
fn every_fixture_is_reproduced() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/traces");
    let mut paths: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no fixtures in {}", dir.display());

    let failures: Vec<String> = paths
        .iter()
        .filter_map(|path| {
            let trace = Trace::from_json(&fs::read_to_string(path).unwrap()).unwrap();
            let differences = trace.replay().unwrap().differences;
            (!differences.is_empty())
                .then(|| format!("{}:\n  {}", path.display(), differences.join("\n  ")))
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

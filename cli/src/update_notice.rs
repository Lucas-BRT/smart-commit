//! The opt-in notice of a newer release (`update.check`).
//!
//! The only request smart-commit makes by itself, so it is off by default,
//! rare (once in 24 hours, failures included), bounded (curl's `--max-time
//! 2`) and silent when it fails. The clock and the fetch are passed in, so
//! tests need neither the network nor the time.

use std::path::Path;
use std::process::{Command, Stdio};

/// Where the latest release is asked for.
pub const LATEST_URL: &str = "https://api.github.com/repos/Lucas-BRT/smart-commit/releases/latest";

/// Seconds between two requests.
pub const INTERVAL: u64 = 24 * 60 * 60;

/// The file in the cache directory that remembers the last request.
pub const CACHE_FILE: &str = "update-check";

/// `MAJOR.MINOR.PATCH`, with an optional leading `v`. Anything else (a
/// pre-release suffix included) is not a release to announce.
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text
        .trim()
        .strip_prefix('v')
        .unwrap_or(text.trim())
        .split('.');
    let mut number = || {
        let part = parts.next()?;
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        part.parse::<u64>().ok()
    };
    let version = (number()?, number()?, number()?);
    parts.next().is_none().then_some(version)
}

/// Whether `latest` is a release newer than `current`.
pub fn is_newer(latest: &str, current: &str) -> bool {
    matches!((parse_version(latest), parse_version(current)), (Some(l), Some(c)) if l > c)
}

/// The `tag_name` of GitHub's answer for the latest release.
pub fn latest_tag(json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    Some(value.get("tag_name")?.as_str()?.to_owned())
}

/// When the last request was made, in seconds since the epoch.
fn last_check(path: &Path) -> Option<u64> {
    std::fs::read_to_string(path)
        .ok()?
        .lines()
        .next()?
        .trim()
        .parse()
        .ok()
}

/// Remembers a request made at `now` and its answer (none after a failure).
fn remember(path: &Path, now: u64, latest: Option<&str>) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, format!("{now}\n{}\n", latest.unwrap_or("")));
}

/// The line to print if a release newer than `current` is out. Asks at most
/// once in [`INTERVAL`] seconds, counted from the last request even when it
/// failed; `fetch` returns GitHub's JSON.
pub fn check(
    cache: &Path,
    current: &str,
    now: u64,
    fetch: impl FnOnce() -> Result<String, String>,
) -> Option<String> {
    if last_check(cache).is_some_and(|at| now.saturating_sub(at) < INTERVAL && at <= now) {
        return None;
    }
    let answer = fetch().and_then(|json| latest_tag(&json).ok_or_else(|| "no tag_name".into()));
    match answer {
        Ok(tag) => {
            remember(cache, now, Some(&tag));
            let latest = tag.trim().trim_start_matches('v');
            is_newer(&tag, current).then(|| {
                format!(
                    "smart-commit {latest} is available (you have {current}); \
                     run smart-commit update"
                )
            })
        }
        Err(problem) => {
            remember(cache, now, None);
            tracing::debug!("update check failed: {problem}");
            None
        }
    }
}

/// Asks GitHub with curl, which every supported system has.
pub fn fetch_latest(current: &str) -> Result<String, String> {
    let output = Command::new("curl")
        .args(["-fsSL", "--max-time", "2", "-A"])
        .arg(format!("smart-commit/{current}"))
        .arg(LATEST_URL)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|err| format!("cannot run curl: {err}"))?;
    if !output.status.success() {
        return Err(format!("curl failed ({})", output.status));
    }
    String::from_utf8(output.stdout).map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    const NEWER: &str = r#"{"tag_name": "v1.2.0", "name": "1.2.0"}"#;

    #[test]
    fn versions_parse_as_semver_and_nothing_else() {
        assert_eq!(parse_version("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("v0.10.0\n"), Some((0, 10, 0)));
        for bad in [
            "",
            "garbage",
            "1.2",
            "1.2.3.4",
            "1.2.x",
            "1.2.3-rc1",
            "-1.2.3",
            "1..3",
        ] {
            assert_eq!(parse_version(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn only_a_newer_release_counts() {
        assert!(is_newer("1.2.0", "1.1.9"));
        assert!(is_newer("v0.10.0", "0.9.0"), "numbers, not text");
        assert!(!is_newer("1.2.0", "1.2.0"));
        assert!(!is_newer("1.1.0", "1.2.0"));
        assert!(!is_newer("garbage", "1.2.0"));
        assert!(!is_newer("1.3.0-rc1", "1.2.0"), "a pre-release is ignored");
        assert!(!is_newer("1.3.0", "1.2.0-dev"), "an unparsable own version");
    }

    #[test]
    fn the_tag_is_read_from_github_json() {
        assert_eq!(latest_tag(NEWER).as_deref(), Some("v1.2.0"));
        assert_eq!(latest_tag("{}"), None);
        assert_eq!(latest_tag("not json"), None);
        assert_eq!(latest_tag(r#"{"tag_name": 3}"#), None);
    }

    #[test]
    fn a_newer_release_gives_the_note_and_is_remembered() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("sub/update-check");
        let note = check(&cache, "1.1.0", 1000, || Ok(NEWER.into()));
        assert_eq!(
            note.as_deref(),
            Some("smart-commit 1.2.0 is available (you have 1.1.0); run smart-commit update")
        );
        assert_eq!(std::fs::read_to_string(&cache).unwrap(), "1000\nv1.2.0\n");
    }

    #[test]
    fn an_equal_or_older_release_gives_no_note() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("update-check");
        assert_eq!(check(&cache, "1.2.0", 1000, || Ok(NEWER.into())), None);
        assert_eq!(
            check(&cache, "1.3.0", 1000 + INTERVAL, || Ok(NEWER.into())),
            None
        );
    }

    #[test]
    fn nothing_is_asked_inside_the_window_and_it_is_after() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("update-check");
        let calls = Cell::new(0);
        let fetch = || {
            calls.set(calls.get() + 1);
            Ok(NEWER.to_owned())
        };
        assert!(check(&cache, "1.1.0", 1000, fetch).is_some());
        assert_eq!(check(&cache, "1.1.0", 1000 + INTERVAL - 1, fetch), None);
        assert_eq!(calls.get(), 1);
        assert!(check(&cache, "1.1.0", 1000 + INTERVAL, fetch).is_some());
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn a_clock_that_went_back_asks_again() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("update-check");
        remember(&cache, 5000, None);
        assert!(check(&cache, "1.1.0", 100, || Ok(NEWER.into())).is_some());
    }

    #[test]
    fn a_failure_is_silent_and_starts_the_window() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("update-check");
        assert_eq!(check(&cache, "1.1.0", 1000, || Err("offline".into())), None);
        assert_eq!(std::fs::read_to_string(&cache).unwrap(), "1000\n\n");
        let asked = Cell::new(false);
        let again = check(&cache, "1.1.0", 1001, || {
            asked.set(true);
            Ok(NEWER.into())
        });
        assert_eq!((again, asked.get()), (None, false));
    }

    #[test]
    fn bad_json_is_a_failure_too() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("update-check");
        assert_eq!(check(&cache, "1.1.0", 7, || Ok("<html>".into())), None);
        assert_eq!(std::fs::read_to_string(&cache).unwrap(), "7\n\n");
    }

    #[test]
    fn an_unwritable_cache_does_not_matter() {
        let note = check(Path::new("/proc/nope/update-check"), "1.1.0", 1, || {
            Ok(NEWER.into())
        });
        assert!(note.is_some());
    }
}

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use super::{Spec, healthy};

/// A server kept loaded, as [`status`] finds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kept {
    pub dir: PathBuf,
    pub spec: Spec,
    pub idle: Duration,
}

/// The servers kept loaded in `runtime`, in a fixed order.
pub fn status(runtime: &Path) -> Vec<Kept> {
    let Ok(entries) = std::fs::read_dir(runtime) else {
        return Vec::new();
    };
    let mut kept: Vec<Kept> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|dir| healthy(&dir.join("server.sock")))
        .filter_map(|dir| {
            let spec = serde_json::from_slice(&std::fs::read(dir.join("spec.json")).ok()?).ok()?;
            let used = std::fs::metadata(dir.join("used"))
                .and_then(|m| m.modified())
                .ok()?;
            let idle = SystemTime::now().duration_since(used).unwrap_or_default();
            Some(Kept { dir, spec, idle })
        })
        .collect();
    kept.sort_by(|a, b| a.dir.cmp(&b.dir));
    kept
}

/// Stops every server kept in `runtime`, e.g. before a benchmark, which
/// needs the memory; returns how many there were.
pub fn stop(runtime: &Path) -> usize {
    let dirs = kept_dirs(runtime);
    for dir in &dirs {
        stop_dir(dir);
    }
    // The locks stay between commits, one per server ever started; with
    // nothing kept they are leftovers.
    if let Ok(entries) = std::fs::read_dir(runtime) {
        for lock in entries.filter_map(Result::ok).map(|e| e.path()) {
            if lock.extension().is_some_and(|e| e == "lock") {
                let _ = std::fs::remove_file(lock);
            }
        }
    }
    dirs.len()
}

pub(super) fn kept_dirs(runtime: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(runtime) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect()
}

/// Stops the supervisor of `dir` and its server (one process group), then
/// removes `dir`.
pub(super) fn stop_dir(dir: &Path) {
    if let Ok(pid) = std::fs::read_to_string(dir.join("pid"))
        && let Ok(pid) = pid.trim().parse::<u32>()
    {
        // A negative pid signals the whole group: std has no kill(2).
        let _ = Command::new("kill")
            .args(["-TERM", "--", &format!("-{pid}")])
            .stderr(Stdio::null())
            .status();
    }
    let _ = std::fs::remove_dir_all(dir);
}

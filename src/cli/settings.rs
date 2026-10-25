//! What a command reads from outside: settings, the environment and the
//! machine, and what changing a setting affects.

use std::path::PathBuf;

use smart_commit::backend::server;
use smart_commit::config::{self, Resolved, Sources};
use smart_commit::git::Repo;
use smart_commit::hardware;
use smart_commit::paths;

pub(super) fn env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// Every layer for `repo` (or none outside a repository), with `flags` on
/// top.
pub(super) fn resolve(
    repo: Option<&Repo>,
    flags: Vec<(&'static str, String)>,
) -> Result<Resolved, config::ConfigError> {
    Resolved::resolve(&Sources {
        global: paths::default_config_file(env),
        repo: repo.map(|r| r.root().join(config::REPO_FILE)),
        env: &env,
        flags,
    })
}

/// Warns about settings that still use an old name.
pub(super) fn warn_renamed(resolved: &Resolved) {
    for warning in resolved.renamed_values() {
        tracing::warn!("{warning}");
    }
}

/// After `key` changed: a model kept loaded was started with the old
/// settings, so it is stopped (and its memory freed) rather than left
/// until idle. Returns what to tell the user, if anything was stopped.
pub(super) fn stop_kept_after(key: &str) -> Option<String> {
    if key != "backend" && !key.starts_with("model.") {
        return None;
    }
    let stopped = server::stop(&paths::runtime_dir(env)?);
    (stopped > 0).then(|| format!("stopped the model kept loaded, since {key} changed"))
}

/// The real machine, or the fake one tests point at (`SMART_COMMIT_SYSFS`
/// is read by `scripts/setup-gpu.sh` too).
pub(super) fn hardware_sources() -> hardware::Sources {
    let real = hardware::Sources::real();
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
    hardware::Sources {
        sysfs: var("SMART_COMMIT_SYSFS").unwrap_or(real.sysfs),
        cpuinfo: var("SMART_COMMIT_CPUINFO").unwrap_or(real.cpuinfo),
        openvino_dir: var("SMART_COMMIT_OPENVINO_DIR").unwrap_or(real.openvino_dir),
    }
}

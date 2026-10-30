//! `smart-commit doctor`: what is wrong with this installation, and how to
//! fix it. It reads the machine and the files and never uses the network.

mod checks;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use smart_commit::backend::server;
use smart_commit::config::REPO_FILE;
use smart_commit::git::Repo;
use smart_commit::hardware::Hardware;
use smart_commit::paths;

use super::benchmark::last_report;
use super::settings::{env, hardware_sources};
use checks::{ConfigInput, Finding, HookFile, HookInput, KeptServer, ServerInput};

/// Prints the report; whether nothing failed.
pub(super) fn run(thorough: bool) -> bool {
    let findings = diagnose(thorough);
    print!("{}", checks::render(&findings));
    checks::healthy(&findings)
}

fn diagnose(thorough: bool) -> Vec<Finding> {
    let mut findings = Vec::new();
    let exe = std::env::current_exe()
        .ok()
        .map(|exe| exe.canonicalize().unwrap_or(exe));
    findings.push(checks::binary_on_path(
        exe.as_deref().and_then(Path::parent),
        &path_dirs(),
    ));
    findings.push(checks::git(git_version().as_deref()));

    let repo = Repo::discover(".").ok();
    let (config, settings) = checks::config(&ConfigInput {
        global: paths::default_config_file(env),
        repo: repo.as_ref().map(|r| r.root().join(REPO_FILE)),
        env: &env,
    });
    findings.extend(config);

    if let Some(settings) = settings {
        let hardware = Hardware::detect(&hardware_sources());
        let bench = last_report();
        let chosen = checks::device_in_use(&settings, &hardware, bench.as_ref());
        findings.extend(checks::model(&settings, &chosen, thorough));
        findings.push(checks::llama_cpp(&settings, &hardware));
        findings.extend(checks::device_lines(
            &settings,
            &chosen,
            &hardware,
            bench.as_ref(),
        ));
        findings.extend(checks::server(&server_input(settings.keep_loaded)));
    }
    if let Some(repo) = repo
        && let Some(input) = hook_input(&repo, exe)
    {
        findings.push(checks::hook(&input));
    }
    findings
}

/// The directories of PATH, as the files they are (a link to a directory
/// is that directory).
fn path_dirs() -> Vec<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .map(|dir| dir.canonicalize().unwrap_or(dir))
        .collect()
}

/// What `git --version` prints, if git runs.
fn git_version() -> Option<String> {
    let output = Command::new("git").arg("--version").output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn server_input(keep_loaded: Duration) -> ServerInput {
    let runtime = paths::runtime_dir(env);
    let kept = runtime.as_deref().map(server::status).unwrap_or_default();
    let stale = runtime
        .as_deref()
        .and_then(|dir| std::fs::read_dir(dir).ok())
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|dir| dir.is_dir() && !kept.iter().any(|k| &k.dir == dir))
        .collect();
    ServerInput {
        keep_loaded,
        runtime_dir_set: runtime.is_some(),
        kept: kept
            .iter()
            .map(|k| KeptServer {
                model: k
                    .spec
                    .options
                    .model
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                device: k.spec.options.device.to_string(),
                idle: k.idle,
                stops_after: k.spec.keep,
            })
            .collect(),
        stale,
    }
}

fn hook_input(repo: &Repo, exe: Option<PathBuf>) -> Option<HookInput> {
    let hooks_dir = repo.hooks_dir().ok()?;
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo.root())
            .args(args)
            .output()
            .ok()?;
        output.status.success().then(|| {
            String::from_utf8_lossy(&output.stdout)
                .trim_end()
                .to_owned()
        })
    };
    let hooks_path = git(&["config", "--get", "core.hooksPath"]).filter(|p| !p.is_empty());
    let default_dir = repo
        .root()
        .join(git(&["rev-parse", "--git-common-dir"])?)
        .join("hooks");
    let read = |dir: &Path| {
        let file = dir.join("prepare-commit-msg");
        Some(HookFile {
            text: std::fs::read_to_string(&file).ok()?,
            executable: is_executable(&file),
        })
    };
    Some(HookInput {
        hook: read(&hooks_dir),
        default_hook: read(&default_dir),
        hooks_dir,
        hooks_path,
        default_dir,
        exe,
    })
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_: &Path) -> bool {
    true
}

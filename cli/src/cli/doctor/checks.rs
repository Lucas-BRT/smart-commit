//! The checks of `smart-commit doctor`. Each one is a function of what it
//! is given (paths, text, the hardware inventory, a benchmark), so tests
//! build the installation they want in a temporary directory; `super` only
//! gathers the real inputs.

use std::path::{Path, PathBuf};
use std::time::Duration;

use smart_commit::backend::device::{self, Target};
use smart_commit::backend::llamacpp::LLAMA_CPP_BUILD;
use smart_commit::bench::Report;
use smart_commit::config::{
    BackendKind, ConfigError, DeviceSetting, Origin, RETIRED, Resolved, Settings, Sources,
    parse_file,
};
use smart_commit::hardware::Hardware;
use smart_commit::hook;
use smart_commit::models::{self, Gguf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Level {
    Ok,
    Warn,
    Fail,
}

/// One line of the report: what was checked, what was found and, unless
/// it is fine, how to fix it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Finding {
    pub level: Level,
    pub check: &'static str,
    pub detail: String,
}

impl Finding {
    fn new(level: Level, check: &'static str, found: impl Into<String>, fix: &str) -> Self {
        let mut detail = found.into();
        if !fix.is_empty() {
            detail = format!("{detail} - {fix}");
        }
        Self {
            level,
            check,
            detail,
        }
    }

    fn ok(check: &'static str, found: impl Into<String>) -> Self {
        Self::new(Level::Ok, check, found, "")
    }

    fn warn(check: &'static str, found: impl Into<String>, fix: &str) -> Self {
        Self::new(Level::Warn, check, found, fix)
    }

    fn fail(check: &'static str, found: impl Into<String>, fix: &str) -> Self {
        Self::new(Level::Fail, check, found, fix)
    }
}

/// The report: a line per finding, aligned, tagged `ok`, `warn` or `fail`.
pub(super) fn render(findings: &[Finding]) -> String {
    findings
        .iter()
        .map(|f| {
            let tag = match f.level {
                Level::Ok => "ok",
                Level::Warn => "warn",
                Level::Fail => "fail",
            };
            format!("{tag:<4}  {:<9}  {}\n", f.check, f.detail)
        })
        .collect()
}

/// Whether the command succeeds: nothing failed (warnings do not count).
pub(super) fn healthy(findings: &[Finding]) -> bool {
    findings.iter().all(|f| f.level != Level::Fail)
}

/// The directory of the running binary is one of the directories of PATH.
pub(super) fn binary_on_path(exe_dir: Option<&Path>, path_dirs: &[PathBuf]) -> Finding {
    let Some(dir) = exe_dir else {
        return Finding::warn(
            "path",
            "cannot tell where this binary is",
            "run it by its full path",
        );
    };
    if path_dirs.iter().any(|p| p == dir) {
        return Finding::ok("path", format!("{} is on PATH", dir.display()));
    }
    Finding::warn(
        "path",
        format!("{} is not on PATH", dir.display()),
        &format!(
            "add it in your shell's profile: export PATH=\"{}:$PATH\"",
            dir.display()
        ),
    )
}

/// The oldest git that smart-commit is used with.
const GIT_MINIMUM: (u32, u32) = (2, 20);

/// `output` is what `git --version` printed, or `None` if git did not run.
pub(super) fn git(output: Option<&str>) -> Finding {
    let Some(output) = output else {
        return Finding::fail("git", "git is not on PATH", "install git 2.20 or newer");
    };
    let version = output.trim().trim_start_matches("git version ");
    let numbers: Vec<u32> = version
        .split_whitespace()
        .next()
        .unwrap_or("")
        .split('.')
        .map_while(|part| part.parse().ok())
        .collect();
    let [major, minor, ..] = numbers[..] else {
        return Finding::warn("git", format!("cannot read the version: {version}"), "");
    };
    if (major, minor) < GIT_MINIMUM {
        return Finding::warn(
            "git",
            format!("git {major}.{minor} is older than 2.20"),
            "update git",
        );
    }
    Finding::ok("git", format!("git {major}.{minor}"))
}

/// Where the settings come from.
pub(super) struct ConfigInput<'a> {
    pub global: Option<PathBuf>,
    pub repo: Option<PathBuf>,
    pub env: &'a dyn Fn(&str) -> Option<String>,
}

/// The settings, and the findings about the files that gave them. When a
/// file is wrong the settings come from the environment and defaults alone,
/// so the other checks still say something.
pub(super) fn config(input: &ConfigInput) -> (Vec<Finding>, Option<Settings>) {
    let home = (input.env)("HOME");
    let mut findings = Vec::new();
    let files = [
        (
            input.global.as_ref(),
            Origin::Global as fn(PathBuf) -> Origin,
        ),
        (input.repo.as_ref(), Origin::Repo),
    ];
    for (path, origin) in files {
        let Some(path) = path else { continue };
        let origin = origin(path.clone());
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                findings.push(Finding::fail(
                    "config",
                    format!("cannot read {}: {err}", path.display()),
                    "check its permissions",
                ));
                continue;
            }
        };
        match parse_file(&text, path, &origin, home.as_deref()) {
            Err(err) => findings.push(Finding::fail(
                "config",
                short(&err),
                "fix the file, or remove the line",
            )),
            Ok(_) => {
                let retired = retired_keys(&text);
                findings.push(if retired.is_empty() {
                    Finding::ok("config", format!("{} reads fine", path.display()))
                } else {
                    Finding::warn(
                        "config",
                        format!(
                            "{} sets {}, which no longer has an effect",
                            path.display(),
                            retired.join(", ")
                        ),
                        "delete the line",
                    )
                });
            }
        }
    }
    let failed = findings.iter().any(|f| f.level == Level::Fail);
    let sources = |with_files: bool| Sources {
        global: input.global.clone().filter(|_| with_files),
        repo: input.repo.clone().filter(|_| with_files),
        env: input.env,
        flags: Vec::new(),
    };
    let resolved = if failed {
        Resolved::resolve(&sources(false))
    } else {
        Resolved::resolve(&sources(true))
    };
    match resolved {
        Ok(resolved) => {
            if !failed {
                findings.extend(
                    resolved
                        .renamed_values()
                        .into_iter()
                        .map(|w| Finding::warn("config", w, "")),
                );
                if findings.is_empty() {
                    findings.push(Finding::ok("config", "no files: the defaults apply"));
                }
            }
            (findings, Some(resolved.settings()))
        }
        Err(err) => {
            findings.push(Finding::fail(
                "config",
                short(&err),
                "fix the environment variable",
            ));
            (findings, None)
        }
    }
}

/// A config error without the list of every key.
fn short(err: &ConfigError) -> String {
    let text = err.to_string();
    text.split("; known keys")
        .next()
        .unwrap_or(&text)
        .to_owned()
}

/// The retired settings a config file still has.
fn retired_keys(text: &str) -> Vec<&'static str> {
    let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else {
        return Vec::new();
    };
    RETIRED
        .iter()
        .copied()
        .filter(|name| {
            let (section, key) = name.split_once('.').unwrap_or(("", name));
            doc.get(section)
                .and_then(|s| s.as_table())
                .is_some_and(|t| t.contains_key(key))
        })
        .collect()
}

/// The device the model runs on, as the next commit would pick it: the one
/// set, else the fastest of the last benchmark, else the default order,
/// among those the machine has. `Err` says why there is none.
pub(super) fn device_in_use(
    settings: &Settings,
    hardware: &Hardware,
    bench: Option<&Report>,
) -> Result<Target, String> {
    let ranking = bench.map(Report::ranking).unwrap_or_default();
    let mut candidates = device::candidates(settings.backend, settings.device, &ranking)
        .map_err(|err| err.to_string())?;
    if settings.device == DeviceSetting::Auto {
        candidates = device::only_present(candidates, hardware);
    }
    candidates
        .first()
        .copied()
        .ok_or_else(|| "no device is allowed by `backend`".to_owned())
}

/// The device in use, whether the machine has it, and the benchmark.
pub(super) fn device_lines(
    settings: &Settings,
    chosen: &Result<Target, String>,
    hardware: &Hardware,
    bench: Option<&Report>,
) -> Vec<Finding> {
    let mut lines = Vec::new();
    match chosen {
        Err(_) if settings.backend == BackendKind::Mock => {
            lines.push(Finding::ok("device", "none: backend is mock"));
        }
        Err(problem) => lines.push(Finding::fail(
            "device",
            problem,
            "`smart-commit config set backend auto`",
        )),
        Ok(target) if !target.is_present(hardware) => lines.push(Finding::fail(
            "device",
            format!("{target} is not available on this machine"),
            "`smart-commit hardware` lists what is; `smart-commit device use auto` picks one",
        )),
        Ok(target) => {
            let why = match (settings.device, bench) {
                (DeviceSetting::Fixed(_), _) => "model.device",
                (DeviceSetting::Auto, Some(_)) => "auto: fastest in the last benchmark",
                (DeviceSetting::Auto, None) => "auto: no benchmark, the default order",
            };
            lines.push(Finding::ok("device", format!("{target} ({why})")));
        }
    }
    match bench {
        None => lines.push(Finding::warn(
            "device",
            "no benchmark yet",
            "run `smart-commit bench`",
        )),
        Some(report) => {
            let failed = chosen
                .as_ref()
                .ok()
                .and_then(|t| report.entry(*t))
                .and_then(|e| e.error.as_deref());
            lines.push(match failed {
                Some(error) => Finding::warn(
                    "device",
                    format!("the last benchmark could not run it: {error}"),
                    "`smart-commit device` shows why",
                ),
                None => Finding::ok(
                    "device",
                    format!(
                        "the last benchmark measured {} device(s)",
                        report.ranking().len()
                    ),
                ),
            });
        }
    }
    lines
}

/// The model files the device in use needs.
pub(super) fn model(
    settings: &Settings,
    chosen: &Result<Target, String>,
    thorough: bool,
) -> Vec<Finding> {
    let Ok(target) = chosen else {
        let note = if settings.backend == BackendKind::Mock {
            "none needed: backend is mock"
        } else {
            "not checked: there is no device to run it"
        };
        return vec![Finding::ok("model", note)];
    };
    match target.backend() {
        BackendKind::LlamaCpp => vec![gguf(settings.gguf.as_deref(), thorough)],
        _ => vec![openvino_model(settings)],
    }
}

fn gguf(path: Option<&Path>, thorough: bool) -> Finding {
    let Some(path) = path else {
        return Finding::fail(
            "model",
            "model.gguf has no default without $HOME",
            "set model.gguf",
        );
    };
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    if !path.is_file() {
        return Finding::fail(
            "model",
            format!("no GGUF model at {}", path.display()),
            "`smart-commit setup` downloads it",
        );
    }
    let Some(known) = models::REGISTRY.iter().find(|m| m.gguf.file == name) else {
        return match models::check_gguf(path) {
            Ok(()) => Finding::ok(
                "model",
                format!(
                    "{} (a file of yours, not checked against the pinned models)",
                    path.display()
                ),
            ),
            Err(err) => Finding::fail("model", err.to_string(), "`smart-commit model use <id>`"),
        };
    };
    verify(path, known.id, &known.gguf, thorough)
}

/// A file that should be `pinned`: its size, its record and, if asked, its
/// hash.
pub(super) fn verify(path: &Path, id: &str, pinned: &Gguf, thorough: bool) -> Finding {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let again = format!("`smart-commit model use {id}` downloads it again");
    let size = std::fs::metadata(path).map_or(0, |m| m.len());
    if size != pinned.size {
        return Finding::fail(
            "model",
            format!("{name} is {size} bytes, not the pinned {}", pinned.size),
            &again,
        );
    }
    let recorded = std::fs::read_to_string(models::sidecar(path)).unwrap_or_default();
    match recorded.split_whitespace().next() {
        None => {
            return Finding::warn("model", format!("{name} has no .sha256 record"), &again);
        }
        Some(hash) if hash != pinned.sha256 => {
            return Finding::fail(
                "model",
                format!("{name}: its .sha256 is not the pinned one"),
                &again,
            );
        }
        Some(_) => {}
    }
    if thorough {
        return match models::sha256_file(path) {
            Ok(actual) if actual == pinned.sha256 => {
                Finding::ok("model", format!("{name}: SHA-256 matches"))
            }
            Ok(_) => Finding::fail(
                "model",
                format!("{name}: SHA-256 differs from the pinned one"),
                &again,
            ),
            Err(err) => Finding::fail("model", format!("cannot read {name}: {err}"), ""),
        };
    }
    Finding::ok(
        "model",
        format!("{name}: size and record match (--thorough hashes the file)"),
    )
}

fn openvino_model(settings: &Settings) -> Finding {
    let Some(dir) = settings.model_dir.as_deref() else {
        let fix = format!("`smart-commit model use {}` has one", models::DEFAULT);
        return Finding::fail(
            "model",
            format!("{} has no OpenVINO version", settings.model.id),
            &fix,
        );
    };
    if dir.join("openvino_model.xml").is_file() {
        return Finding::ok("model", format!("{}", dir.display()));
    }
    Finding::fail(
        "model",
        format!("no OpenVINO model in {}", dir.display()),
        "`smart-commit setup` exports it",
    )
}

/// `llama-server`, for the devices that run on llama.cpp.
pub(super) fn llama_cpp(settings: &Settings, hardware: &Hardware) -> Finding {
    let usable = settings.backend != BackendKind::Openvino
        && settings.backend != BackendKind::Mock
        && device::present(hardware)
            .iter()
            .any(|t| t.backend() == BackendKind::LlamaCpp);
    if !usable {
        return Finding::ok("llama.cpp", "not needed: no device of this machine uses it");
    }
    let Some(server) = settings.llama_server.as_deref() else {
        return Finding::warn(
            "llama.cpp",
            "model.llama_server has no default without $HOME",
            "set it",
        );
    };
    if !server.is_file() {
        return Finding::warn(
            "llama.cpp",
            format!("no llama-server at {}", server.display()),
            "`smart-commit setup` installs it",
        );
    }
    let pinned = format!("llama-{LLAMA_CPP_BUILD}");
    let build = server
        .parent()
        .and_then(Path::file_name)
        .map(|n| n.to_string_lossy());
    match build {
        Some(build) if build.starts_with("llama-b") && build != pinned => Finding::warn(
            "llama.cpp",
            format!(
                "{} is not the build this release pins ({LLAMA_CPP_BUILD})",
                server.display()
            ),
            "`smart-commit setup` installs it",
        ),
        _ => Finding::ok("llama.cpp", format!("{}", server.display())),
    }
}

/// A model kept loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct KeptServer {
    pub model: String,
    pub device: String,
    pub idle: Duration,
    pub stops_after: Duration,
}

/// What is in the runtime directory.
pub(super) struct ServerInput {
    pub keep_loaded: Duration,
    pub runtime_dir_set: bool,
    pub kept: Vec<KeptServer>,
    /// Directories whose server no longer answers.
    pub stale: Vec<PathBuf>,
}

pub(super) fn server(input: &ServerInput) -> Vec<Finding> {
    let mut lines: Vec<Finding> = input
        .kept
        .iter()
        .map(|k| {
            Finding::ok(
                "server",
                format!(
                    "{} on {}: idle {} s, stops after {} s idle",
                    k.model,
                    k.device,
                    k.idle.as_secs(),
                    k.stops_after.as_secs()
                ),
            )
        })
        .collect();
    lines.extend(input.stale.iter().map(|dir| {
        Finding::warn(
            "server",
            format!("{} holds a server that does not answer", dir.display()),
            "`smart-commit server stop` clears it",
        )
    }));
    if lines.is_empty() {
        lines.push(if input.keep_loaded.is_zero() {
            Finding::ok("server", "off: model.keep_loaded is 0")
        } else if !input.runtime_dir_set {
            Finding::warn(
                "server",
                "$XDG_RUNTIME_DIR is not set, so no model is kept loaded",
                "log in with a session that sets it, or `smart-commit config set model.keep_loaded 0`",
            )
        } else {
            Finding::ok("server", "none running: the next commit starts one")
        });
    }
    lines
}

/// A hook file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HookFile {
    pub text: String,
    pub executable: bool,
}

/// The `prepare-commit-msg` hook of the repository.
pub(super) struct HookInput {
    /// Where git looks for hooks (`core.hooksPath` or `.git/hooks`).
    pub hooks_dir: PathBuf,
    pub hooks_path: Option<String>,
    /// `.git/hooks`, which `core.hooksPath` makes git ignore.
    pub default_dir: PathBuf,
    pub hook: Option<HookFile>,
    pub default_hook: Option<HookFile>,
    pub exe: Option<PathBuf>,
}

pub(super) fn hook(input: &HookInput) -> Finding {
    let ours = |h: &HookFile| hook::is_ours(&h.text);
    let install = "`smart-commit hook install`";
    let Some(found) = input.hook.as_ref() else {
        return match (&input.hooks_path, &input.default_hook) {
            (Some(path), Some(other)) if ours(other) => Finding::warn(
                "hook",
                format!(
                    "core.hooksPath is {path}, so git ignores the hook in {}",
                    input.default_dir.display()
                ),
                &format!("{install} puts it in {}", input.hooks_dir.display()),
            ),
            _ => Finding::warn("hook", "not installed in this repository", install),
        };
    };
    let path = input.hooks_dir.join("prepare-commit-msg");
    if !ours(found) {
        return Finding::warn(
            "hook",
            format!("{} is another hook", path.display()),
            "`smart-commit hook install --force` keeps it as a backup",
        );
    }
    if !found.executable {
        return Finding::warn(
            "hook",
            format!("{} is not executable", path.display()),
            install,
        );
    }
    let target = hook_target(&found.text);
    match (&target, &input.exe) {
        (Some(t), _) if !t.is_file() => Finding::warn(
            "hook",
            format!(
                "{} runs {}, which does not exist",
                path.display(),
                t.display()
            ),
            install,
        ),
        (Some(t), Some(exe)) if t != exe => Finding::warn(
            "hook",
            format!(
                "{} runs {}, not this binary ({})",
                path.display(),
                t.display(),
                exe.display()
            ),
            install,
        ),
        _ => Finding::ok("hook", format!("{} runs this binary", path.display())),
    }
}

/// The binary a hook script of ours runs (see [`hook::script`]).
fn hook_target(text: &str) -> Option<PathBuf> {
    let line = text.lines().find_map(|l| l.strip_prefix("[ -x "))?;
    let quoted = line.strip_suffix(" ] || exit 0")?;
    let path = quoted.strip_prefix('\'')?.strip_suffix('\'')?;
    Some(PathBuf::from(path.replace(r"'\''", "'")))
}

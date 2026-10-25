//! A llama-server kept loaded between commits (`model.keep_loaded`).
//!
//! Loading the model is most of a commit on a GPU (2.1 of 2.4 s on an RTX
//! 3050). With `model.keep_loaded`, the first commit starts a supervisor, a
//! detached `smart-commit keep-server DIR`, that owns the llama-server and
//! stops it after that many minutes without a request; later commits
//! connect to its socket. There is one server per configuration: its
//! directory is named after a hash of everything that shapes the server,
//! so another model or device gets its own, and the old one stops when
//! idle. Requests are the same as to a fresh server (no prompt cache), so
//! replies do not change.
//!
//! Everything lives in [`crate::paths::runtime_dir`], private to the user:
//! `<key>.lock` (held while a commit connects or starts a server) and
//! `<key>/` with the socket, the log, `spec.json` (what the supervisor
//! runs), `pid` (the supervisor, leader of its process group) and `used`
//! (touched by every commit; its age is the idle time).

mod manage;
mod supervisor;
#[cfg(test)]
mod tests;

use manage::{kept_dirs, stop_dir};

pub use manage::{Kept, status, stop};
pub use supervisor::{Stop, serve, watch};

use std::fs::File;
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::backend::BackendError;
use crate::backend::llamacpp::{self, LlamaCppBackend, LlamaCppOptions};

/// The hidden subcommand that runs the supervisor.
pub const SUBCOMMAND: &str = "keep-server";

/// What the supervisor runs, written by the commit that starts it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spec {
    pub options: LlamaCppOptions,
    /// Idle time after which the server stops.
    pub keep: Duration,
}

/// The name of the server for `options`: a hash of every option but the
/// startup timeout, and of the model's and the server's identity (the
/// model's recorded SHA-256, else size and modification time), so a
/// replaced file gets a new server.
pub fn key(options: &LlamaCppOptions) -> String {
    let shaping = LlamaCppOptions {
        startup_timeout: Duration::ZERO,
        ..options.clone()
    };
    let mut hash = Sha256::new();
    hash.update(serde_json::to_vec(&shaping).expect("options serialize"));
    for file in [&options.model, &options.server] {
        hash.update(identity(file).as_bytes());
    }
    hash.finalize()[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn identity(file: &Path) -> String {
    if let Ok(recorded) = std::fs::read_to_string(crate::models::sidecar(file)) {
        return recorded;
    }
    match std::fs::metadata(file) {
        Ok(m) => format!("{} {:?}", m.len(), m.modified().ok()),
        Err(_) => "missing".to_owned(),
    }
}

/// Connects to the server kept for `options` in `runtime`, or starts it
/// (with `exe`, this binary, as its supervisor) and waits until it has
/// loaded the model.
pub fn connect(
    runtime: &Path,
    options: &LlamaCppOptions,
    keep: Duration,
    exe: &Path,
) -> Result<LlamaCppBackend, BackendError> {
    let fail = |what: &str, err: io::Error| BackendError::Unavailable(format!("{what}: {err}"));
    create_private(runtime).map_err(|e| fail("cannot create the runtime directory", e))?;
    let key = key(options);
    // Held until connected: two commits at once start one server.
    let lock = File::create(runtime.join(format!("{key}.lock")))
        .map_err(|e| fail("cannot create the lock", e))?;
    lock.lock().map_err(|e| fail("cannot lock", e))?;
    let dir = runtime.join(&key);
    let socket = dir.join("server.sock");
    if !healthy(&socket) {
        // One model kept at a time: another model or device in memory too
        // could leave the new one without room on the GPU.
        for other in kept_dirs(runtime).into_iter().filter(|d| *d != dir) {
            stop_dir(&other);
        }
        start(
            &dir,
            &Spec {
                options: options.clone(),
                keep,
            },
            exe,
        )?;
    }
    touch(&dir.join("used")).map_err(|e| fail("cannot mark the server used", e))?;
    Ok(LlamaCppBackend::connect(
        socket,
        llamacpp::describe(options),
    ))
}

fn healthy(socket: &Path) -> bool {
    matches!(llamacpp::http(socket, "GET", "/health", None), Ok((200, _)))
}

/// Starts the supervisor in a clean `dir` and waits for its server.
fn start(dir: &Path, spec: &Spec, exe: &Path) -> Result<(), BackendError> {
    let unavailable = BackendError::Unavailable;
    // What is left there is a server that stopped answering, or its traces.
    stop_dir(dir);
    create_private(dir)
        .and_then(|()| std::fs::write(dir.join("spec.json"), serde_json::to_vec(spec)?))
        .and_then(|()| touch(&dir.join("used")))
        .map_err(|e| unavailable(format!("cannot prepare {}: {e}", dir.display())))?;
    use std::os::unix::process::CommandExt;
    let mut child = Command::new(exe)
        .arg(SUBCOMMAND)
        .arg(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        // Its own process group, so `server stop` stops the llama-server too,
        // and a Ctrl+C in the commit's terminal does not.
        .process_group(0)
        .spawn()
        .map_err(|e| unavailable(format!("cannot start {}: {e}", exe.display())))?;
    let deadline = Instant::now() + spec.options.startup_timeout;
    let socket = dir.join("server.sock");
    loop {
        if healthy(&socket) {
            return Ok(());
        }
        if let Ok(Some(status)) = child.try_wait() {
            return Err(unavailable(format!(
                "the server to keep loaded exited with {status}: {}",
                last_line(&dir.join("supervisor.log"))
            )));
        }
        if Instant::now() > deadline {
            stop_dir(dir);
            return Err(unavailable(format!(
                "the server to keep loaded was not ready after {}s",
                spec.options.startup_timeout.as_secs()
            )));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn create_private(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

fn touch(file: &Path) -> io::Result<()> {
    File::create(file)?.set_modified(SystemTime::now())
}

fn last_line(file: &Path) -> String {
    let text = std::fs::read_to_string(file).unwrap_or_default();
    text.lines().last().unwrap_or("no output").to_owned()
}

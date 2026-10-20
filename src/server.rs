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

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::backend::BackendError;
use crate::llamacpp::{self, LlamaCppBackend, LlamaCppOptions};

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

/// The supervisor: runs the server of `dir/spec.json` until it has been
/// idle for its `keep` time or it exits, then stops it and removes `dir`.
pub fn serve(dir: &Path) -> Result<(), BackendError> {
    let unavailable = BackendError::Unavailable;
    let log = |text: &str| {
        let _ = std::fs::write(dir.join("supervisor.log"), text);
    };
    let spec: Spec = std::fs::read(dir.join("spec.json"))
        .and_then(|bytes| Ok(serde_json::from_slice(&bytes)?))
        .map_err(|e| unavailable(format!("cannot read {}/spec.json: {e}", dir.display())))?;
    let _ = std::fs::write(dir.join("pid"), std::process::id().to_string());
    let backend = match LlamaCppBackend::start_or_load_normally_in(&spec.options, Some(dir)) {
        Ok((backend, _)) => backend,
        Err(err) => {
            log(&err.to_string());
            return Err(err);
        }
    };
    let used = dir.join("used");
    let stop = watch(
        spec.keep,
        || backend.is_running(),
        || std::fs::metadata(&used).and_then(|m| m.modified()).ok(),
        SystemTime::now,
        std::thread::sleep,
    );
    log(&format!("stopped: {stop:?}"));
    drop(backend); // stops the server and removes `dir`
    Ok(())
}

/// Why [`watch`] returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    Idle,
    ServerExited,
}

/// Waits until the server has been idle for `keep` (no `used` time newer
/// than that, or none at all) or is no longer `running`. The clock and the
/// sleep are passed in, so tests need neither.
pub fn watch(
    keep: Duration,
    mut running: impl FnMut() -> bool,
    used: impl Fn() -> Option<SystemTime>,
    now: impl Fn() -> SystemTime,
    sleep: impl Fn(Duration),
) -> Stop {
    let poll = (keep / 4).clamp(Duration::from_secs(1), Duration::from_secs(15));
    loop {
        sleep(poll);
        if !running() {
            return Stop::ServerExited;
        }
        let idle = used().map_or(keep, |t| now().duration_since(t).unwrap_or_default());
        if idle >= keep {
            return Stop::Idle;
        }
    }
}

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

fn kept_dirs(runtime: &Path) -> Vec<PathBuf> {
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
fn stop_dir(dir: &Path) {
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

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::os::unix::net::UnixListener;

    use super::*;
    use crate::llamacpp::LlamaDevice;

    fn options(dir: &Path) -> LlamaCppOptions {
        LlamaCppOptions {
            server: dir.join("llama-server"),
            model: dir.join("model.gguf"),
            device: LlamaDevice::Vulkan(0),
            max_prompt_tokens: 1024,
            max_new_tokens: 128,
            threads_batch: 6,
            startup_timeout: Duration::from_secs(2),
            fast_load: true,
        }
    }

    /// Answers `/health` with 200 until dropped, like a loaded server.
    fn fake_server(socket: &Path) {
        let listener = UnixListener::bind(socket).unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                use std::io::{Read, Write};
                // The whole request first: the client writes it in pieces.
                let mut request = Vec::new();
                let mut buf = [0; 1024];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => request.extend_from_slice(&buf[..n]),
                    }
                }
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}");
            }
        });
    }

    #[test]
    fn key_follows_what_shapes_the_server() {
        let dir = tempfile::TempDir::new().unwrap();
        let base = options(dir.path());
        assert_eq!(key(&base), key(&base));
        assert_eq!(key(&base).len(), 16);
        let slower = LlamaCppOptions {
            startup_timeout: Duration::from_secs(99),
            ..base.clone()
        };
        assert_eq!(key(&slower), key(&base));
        for other in [
            LlamaCppOptions {
                device: LlamaDevice::Cpu,
                ..base.clone()
            },
            LlamaCppOptions {
                fast_load: false,
                ..base.clone()
            },
            LlamaCppOptions {
                model: dir.path().join("other.gguf"),
                ..base.clone()
            },
        ] {
            assert_ne!(key(&other), key(&base));
        }
        // Another file at the same path is another model.
        std::fs::write(crate::models::sidecar(&base.model), "abc  model.gguf\n").unwrap();
        let checked = key(&base);
        std::fs::write(crate::models::sidecar(&base.model), "def  model.gguf\n").unwrap();
        assert_ne!(key(&base), checked);
    }

    #[test]
    fn watch_stops_when_idle_or_when_the_server_exits() {
        let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let clock = Cell::new(start);
        let keep = Duration::from_secs(600);
        let sleep = |d: Duration| clock.set(clock.get() + d);
        // Last used at the start: stops once `keep` has passed, not before.
        let stop = watch(keep, || true, || Some(start), || clock.get(), sleep);
        assert_eq!(stop, Stop::Idle);
        let waited = clock.get().duration_since(start).unwrap();
        assert!(
            waited >= keep && waited < keep + Duration::from_secs(16),
            "{waited:?}"
        );
        // Used again meanwhile: waits longer.
        clock.set(start);
        let used = start + Duration::from_secs(300);
        watch(keep, || true, || Some(used), || clock.get(), sleep);
        assert!(clock.get().duration_since(used).unwrap() >= keep);
        // The server died: stops at once.
        clock.set(start);
        assert_eq!(
            watch(keep, || false, || Some(start), || clock.get(), sleep),
            Stop::ServerExited
        );
        // No `used` file: idle.
        assert_eq!(
            watch(keep, || true, || None, || clock.get(), sleep),
            Stop::Idle
        );
    }

    #[test]
    fn connects_to_a_running_server_without_starting_one() {
        let runtime = tempfile::TempDir::new().unwrap();
        let options = options(runtime.path());
        let dir = runtime.path().join(key(&options));
        std::fs::create_dir_all(&dir).unwrap();
        fake_server(&dir.join("server.sock"));
        let backend = connect(
            runtime.path(),
            &options,
            Duration::from_secs(60),
            Path::new("/nonexistent/smart-commit"),
        )
        .unwrap();
        assert!(backend.is_running());
        assert!(dir.join("used").is_file());
    }

    #[test]
    fn a_stale_directory_is_replaced_and_a_failed_start_reported() {
        let runtime = tempfile::TempDir::new().unwrap();
        let options = options(runtime.path());
        let dir = runtime.path().join(key(&options));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("server.sock"), "").unwrap();
        // `false` exits at once, like a supervisor that could not start.
        let err = connect(
            runtime.path(),
            &options,
            Duration::from_secs(60),
            Path::new("/bin/false"),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("exited"), "{err}");
        let spec: Spec =
            serde_json::from_slice(&std::fs::read(dir.join("spec.json")).unwrap()).unwrap();
        assert_eq!(spec.options, options);
        assert!(!dir.join("server.sock").exists());
    }

    #[test]
    fn starting_another_server_stops_the_one_kept() {
        let runtime = tempfile::TempDir::new().unwrap();
        let old = runtime.path().join("0123456789abcdef");
        create_private(&old).unwrap();
        std::fs::write(old.join("used"), "").unwrap();
        let options = options(runtime.path());
        connect(
            runtime.path(),
            &options,
            Duration::from_secs(60),
            Path::new("/bin/false"),
        )
        .unwrap_err();
        assert!(!old.exists());
    }

    #[test]
    fn status_lists_healthy_servers_and_stop_removes_them() {
        let runtime = tempfile::TempDir::new().unwrap();
        assert!(status(&runtime.path().join("missing")).is_empty());
        let options = options(runtime.path());
        let dir = runtime.path().join(key(&options));
        create_private(&dir).unwrap();
        let spec = Spec {
            options,
            keep: Duration::from_secs(600),
        };
        std::fs::write(dir.join("spec.json"), serde_json::to_vec(&spec).unwrap()).unwrap();
        touch(&dir.join("used")).unwrap();
        fake_server(&dir.join("server.sock"));
        let kept = status(runtime.path());
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].spec, spec);
        assert!(kept[0].idle < Duration::from_secs(60));
        std::fs::write(runtime.path().join("0123456789abcdef.lock"), "").unwrap();
        assert_eq!(stop(runtime.path()), 1);
        assert!(!dir.exists());
        assert_eq!(std::fs::read_dir(runtime.path()).unwrap().count(), 0);
        assert!(status(runtime.path()).is_empty());
    }
}

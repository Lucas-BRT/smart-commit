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

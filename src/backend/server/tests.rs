use std::cell::Cell;
use std::os::unix::net::UnixListener;

use super::*;
use crate::backend::llamacpp::LlamaDevice;
use crate::memory::Pressure;

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

/// A machine with 64 GiB of RAM, nearly all of it available, in a file.
fn roomy(dir: &Path) -> Share {
    machine(dir, 64 * 1024 * 1024, 60 * 1024 * 1024)
}

/// A machine with `total` and `available` kilobytes of RAM, in a file.
fn machine(dir: &Path, total: u64, available: u64) -> Share {
    let meminfo = dir.join("meminfo");
    std::fs::write(
        &meminfo,
        format!("MemTotal: {total} kB\nMemFree: 1 kB\nMemAvailable: {available} kB\n"),
    )
    .unwrap();
    Share {
        max_percent: 15,
        meminfo,
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
    let stop = watch(
        keep,
        || true,
        || Some(start),
        || None,
        || clock.get(),
        sleep,
    );
    assert_eq!(stop, Stop::Idle);
    let waited = clock.get().duration_since(start).unwrap();
    assert!(
        waited >= keep && waited < keep + Duration::from_secs(16),
        "{waited:?}"
    );
    // Used again meanwhile: waits longer.
    clock.set(start);
    let used = start + Duration::from_secs(300);
    watch(keep, || true, || Some(used), || None, || clock.get(), sleep);
    assert!(clock.get().duration_since(used).unwrap() >= keep);
    // The server died: stops at once.
    clock.set(start);
    assert_eq!(
        watch(
            keep,
            || false,
            || Some(start),
            || None,
            || clock.get(),
            sleep
        ),
        Stop::ServerExited
    );
    // No `used` file: idle.
    assert_eq!(
        watch(keep, || true, || None, || None, || clock.get(), sleep),
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
        &roomy(runtime.path()),
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
        &roomy(runtime.path()),
        Path::new("/bin/false"),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("exited"), "{err}");
    let spec: Spec =
        serde_json::from_slice(&std::fs::read(dir.join("spec.json")).unwrap()).unwrap();
    assert_eq!(spec.options, options);
    assert_eq!(spec.share, roomy(runtime.path()));
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
        &roomy(runtime.path()),
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
        share: Share::default(),
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

#[test]
fn watch_stops_when_the_memory_runs_short() {
    let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    let clock = Cell::new(start);
    let keep = Duration::from_secs(600);
    let sleep = |d: Duration| clock.set(clock.get() + d);
    let low = Pressure::Low {
        available_percent: 4,
    };
    // Fine for two polls, then short: stops at once, long before idle.
    let polls = Cell::new(0);
    let stop = watch(
        keep,
        || true,
        || Some(clock.get()),
        || {
            polls.set(polls.get() + 1);
            (polls.get() > 2).then_some(low)
        },
        || clock.get(),
        sleep,
    );
    assert_eq!(stop, Stop::Memory(low));
    assert_eq!(polls.get(), 3);
    assert!(clock.get().duration_since(start).unwrap() < keep);
    // A server that exited is that, whatever the memory.
    assert_eq!(
        watch(
            keep,
            || false,
            || Some(start),
            || Some(low),
            || clock.get(),
            sleep
        ),
        Stop::ServerExited
    );
}

#[test]
fn a_model_too_large_a_share_of_the_ram_is_not_kept_and_nothing_is_started() {
    let runtime = tempfile::TempDir::new().unwrap();
    let options = options(runtime.path());
    // A 3 GB model on a 16 GiB machine: 19% with the overhead, over 15%.
    let model = std::fs::File::create(&options.model).unwrap();
    model.set_len(3_000_000_000).unwrap();
    let share = machine(runtime.path(), 16 * 1024 * 1024, 12 * 1024 * 1024);
    let err = connect(
        runtime.path(),
        &options,
        Duration::from_secs(60),
        &share,
        Path::new("/nonexistent/smart-commit"),
    )
    .unwrap_err();
    assert!(
        matches!(
            err,
            ConnectError::NotKept(Refusal::TooBig { percent: 19..=21 })
        ),
        "{err}"
    );
    assert_eq!(
        err.to_string().split(' ').take(3).collect::<Vec<_>>(),
        ["it", "would", "take"]
    );
    assert!(
        !runtime.path().join(key(&options)).exists(),
        "no directory, no supervisor"
    );
    // With a larger share it is kept (here the supervisor cannot start).
    let share = Share {
        max_percent: 30,
        ..share
    };
    let err = connect(
        runtime.path(),
        &options,
        Duration::from_secs(60),
        &share,
        Path::new("/bin/false"),
    )
    .unwrap_err();
    assert!(matches!(err, ConnectError::Failed(_)), "{err}");
}

#[test]
fn a_busy_machine_does_not_start_a_server_to_keep() {
    let runtime = tempfile::TempDir::new().unwrap();
    let options = options(runtime.path());
    std::fs::File::create(&options.model)
        .unwrap()
        .set_len(500_000_000)
        .unwrap();
    // 700 MB would be taken of 1 GiB available on 16 GiB: 6% left.
    let share = machine(runtime.path(), 16 * 1024 * 1024, 1024 * 1024 + 700_000);
    let err = connect(
        runtime.path(),
        &options,
        Duration::from_secs(60),
        &Share {
            max_percent: 100,
            ..share
        },
        Path::new("/nonexistent/smart-commit"),
    )
    .unwrap_err();
    assert!(
        matches!(err, ConnectError::NotKept(Refusal::MachineBusy { .. })),
        "{err}"
    );
}

#[test]
fn a_server_already_running_is_used_even_when_the_machine_is_short() {
    // Its own memory is already out of what is available.
    let runtime = tempfile::TempDir::new().unwrap();
    let options = options(runtime.path());
    std::fs::File::create(&options.model)
        .unwrap()
        .set_len(3_000_000_000)
        .unwrap();
    let dir = runtime.path().join(key(&options));
    std::fs::create_dir_all(&dir).unwrap();
    fake_server(&dir.join("server.sock"));
    let share = machine(runtime.path(), 16 * 1024 * 1024, 1024);
    assert!(
        connect(
            runtime.path(),
            &options,
            Duration::from_secs(60),
            &share,
            Path::new("/nonexistent/smart-commit"),
        )
        .is_ok()
    );
}

#[test]
fn an_unreadable_meminfo_does_not_stop_a_model_from_being_kept() {
    let runtime = tempfile::TempDir::new().unwrap();
    let options = options(runtime.path());
    let share = Share {
        max_percent: 15,
        meminfo: runtime.path().join("no-such-meminfo"),
    };
    let err = connect(
        runtime.path(),
        &options,
        Duration::from_secs(60),
        &share,
        Path::new("/bin/false"),
    )
    .unwrap_err();
    assert!(matches!(err, ConnectError::Failed(_)), "{err}");
}

#[test]
fn a_spec_written_before_the_share_existed_still_reads() {
    let runtime = tempfile::TempDir::new().unwrap();
    let spec = Spec {
        options: options(runtime.path()),
        keep: Duration::from_secs(600),
        share: Share::default(),
    };
    let mut json: serde_json::Value = serde_json::to_value(&spec).unwrap();
    json.as_object_mut().unwrap().remove("share");
    let read: Spec = serde_json::from_value(json).unwrap();
    assert_eq!(read, spec);
    assert_eq!(read.share.max_percent, 15);
}

#[test]
fn a_frozen_server_is_replaced_and_does_not_hang_the_commit() {
    let runtime = tempfile::TempDir::new().unwrap();
    let options = options(runtime.path());
    let dir = runtime.path().join(key(&options));
    std::fs::create_dir_all(&dir).unwrap();
    // Accepts connections and never answers.
    let listener = UnixListener::bind(dir.join("server.sock")).unwrap();
    let held = std::thread::spawn(move || listener.accept().map(|(stream, _)| stream));
    let started = Instant::now();
    // `false` stands for a supervisor that cannot start: the frozen server
    // was judged unhealthy, so a new one was tried.
    let err = connect(
        runtime.path(),
        &options,
        Duration::from_secs(60),
        &roomy(runtime.path()),
        Path::new("/bin/false"),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("exited"), "{err}");
    // It waited for the health check, and not for a generation's 300 s.
    assert!(started.elapsed() >= llamacpp::HEALTH_TIMEOUT);
    assert!(started.elapsed() < Duration::from_secs(60));
    drop(held);
}

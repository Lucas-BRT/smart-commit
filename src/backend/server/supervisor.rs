use std::time::{Duration, SystemTime};

use super::*;
use crate::memory::Pressure;

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
        || {
            let machine = Memory::read(&spec.share.meminfo)?;
            let resident = backend
                .pid()
                .and_then(|pid| std::fs::read_to_string(format!("/proc/{pid}/status")).ok())
                .and_then(|status| memory::resident(&status));
            memory::pressure(resident, &machine, spec.share.max_percent)
        },
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
    /// The machine ran short of RAM, or the server grew past its share.
    Memory(Pressure),
}

/// Waits until the server has been idle for `keep` (no `used` time newer
/// than that, or none at all), is no longer `running` or `pressure` says
/// the memory is short. The clock and the sleep are passed in, so tests
/// need neither.
pub fn watch(
    keep: Duration,
    mut running: impl FnMut() -> bool,
    used: impl Fn() -> Option<SystemTime>,
    mut pressure: impl FnMut() -> Option<Pressure>,
    now: impl Fn() -> SystemTime,
    sleep: impl Fn(Duration),
) -> Stop {
    let poll = (keep / 4).clamp(Duration::from_secs(1), Duration::from_secs(15));
    loop {
        sleep(poll);
        if !running() {
            return Stop::ServerExited;
        }
        if let Some(pressure) = pressure() {
            return Stop::Memory(pressure);
        }
        let idle = used().map_or(keep, |t| now().duration_since(t).unwrap_or_default());
        if idle >= keep {
            return Stop::Idle;
        }
    }
}

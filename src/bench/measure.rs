use std::time::Instant;

use super::*;
use crate::backend::device::Target;
use crate::backend::{Backend, BackendError, GenerationConfig};
use crate::prompt::Prompt;

/// CPU time used by this process and its waited-for children (such as
/// llama-server), in milliseconds; from `/proc/self/stat`.
pub fn cpu_time_ms() -> f64 {
    let Ok(stat) = std::fs::read_to_string("/proc/self/stat") else {
        return 0.0;
    };
    // Fields after the parenthesised command name; utime is field 14.
    let Some(rest) = stat.rsplit_once(')').map(|(_, r)| r) else {
        return 0.0;
    };
    let fields: Vec<f64> = rest
        .split_whitespace()
        .skip(11)
        .take(4)
        .filter_map(|f| f.parse().ok())
        .collect();
    // Clock ticks; 100 per second on every Linux build that matters.
    fields.iter().sum::<f64>() * 10.0
}

/// Measures one device: a warm-up load, then a timed load and every prompt.
pub fn measure(
    target: Target,
    prompts: &[Prompt],
    config: &GenerationConfig,
    mut load: impl FnMut(Target) -> Result<Box<dyn Backend>, BackendError>,
) -> Entry {
    let failed = |error: String| Entry {
        target,
        backend: None,
        measurement: None,
        error: Some(error),
        fast_load: None,
    };
    // Warm-up: compiles and caches the model, which only the first run pays.
    match load(target) {
        Ok(backend) => drop(backend),
        Err(err) => return failed(err.to_string()),
    }

    let cpu_start = cpu_time_ms();
    let wall_start = Instant::now();
    let backend = match load(target) {
        Ok(backend) => backend,
        Err(err) => return failed(err.to_string()),
    };
    let load_ms = wall_start.elapsed().as_secs_f64() * 1000.0;
    let description = backend.describe();
    let mut replies = Vec::new();
    let generate_start = Instant::now();
    for prompt in prompts {
        match backend.generate(prompt, config) {
            Ok(reply) => replies.push(reply),
            Err(err) => return failed(err.to_string()),
        }
    }
    let generate_ms = generate_start.elapsed().as_secs_f64() * 1000.0 / prompts.len().max(1) as f64;
    drop(backend);
    let wall_ms = wall_start.elapsed().as_secs_f64() * 1000.0;
    let cpu_ms = cpu_time_ms() - cpu_start;
    Entry {
        target,
        backend: Some(description),
        measurement: Some(Measurement {
            load_ms,
            generate_ms,
            per_message_ms: load_ms + generate_ms,
            cpu_cores: if wall_ms > 0.0 { cpu_ms / wall_ms } else { 0.0 },
            replies,
        }),
        error: None,
        fast_load: None,
    }
}

/// Measures a llama.cpp device without and with fast load (`load`'s second
/// argument) and keeps the mode [`FastLoadTrial`] picks.
pub fn measure_load_modes(
    target: Target,
    prompts: &[Prompt],
    config: &GenerationConfig,
    load: impl FnMut(Target, bool) -> Result<Box<dyn Backend>, BackendError>,
) -> Entry {
    measure_load_modes_with(target, prompts, config, load, |_| {})
}

/// Like [`measure_load_modes`], telling `phase` which mode it measures.
pub fn measure_load_modes_with(
    target: Target,
    prompts: &[Prompt],
    config: &GenerationConfig,
    mut load: impl FnMut(Target, bool) -> Result<Box<dyn Backend>, BackendError>,
    mut phase: impl FnMut(&'static str),
) -> Entry {
    phase("measuring, normal load");
    let normal = measure(target, prompts, config, |t| load(t, false));
    phase("measuring, with fast load");
    let fast = measure(target, prompts, config, |t| load(t, true));
    let per_message = |e: &Entry| e.measurement.as_ref().map(|m| m.per_message_ms);
    let trial = FastLoadTrial::new(per_message(&normal), per_message(&fast));
    let chosen = if trial.enabled { fast } else { normal };
    Entry {
        fast_load: Some(trial),
        ..chosen
    }
}

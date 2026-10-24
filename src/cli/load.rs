//! Loading the model on a device, or on the first that works.

use anyhow::Result;
use smart_commit::backend::{Backend, BackendError};
use smart_commit::config::{DeviceSetting, Settings};
use smart_commit::device::{self, Target};
use smart_commit::genai::{Device, GenaiBackend};
use smart_commit::llamacpp::{LlamaCppBackend, LlamaCppOptions};
use smart_commit::{paths, server};

use super::benchmark;
use super::settings::env;

/// Loads the model on the configured device, or picks one for `auto`.
pub(super) fn load_model(settings: &Settings) -> Result<(Box<dyn Backend>, Target)> {
    let candidates = device::candidates(settings.backend, settings.device, &preferred_devices())?;
    let selection = device::load_first(&candidates, |target| load_on(settings, target))?;
    if settings.device == DeviceSetting::Auto && !selection.skipped.is_empty() {
        let skipped: Vec<&str> = selection.skipped.iter().map(|(t, _)| t.name()).collect();
        eprintln!(
            "note: running on {}; {} unavailable (`smart-commit device` shows why)",
            selection.target,
            skipped.join(", ")
        );
    }
    Ok((selection.backend, selection.target))
}

/// Loads on `target`; on llama.cpp devices, with fast load as
/// `model.fast_load` and the last benchmark say.
fn load_on(settings: &Settings, target: Target) -> Result<Box<dyn Backend>, BackendError> {
    let fast_load = settings
        .fast_load
        .enabled(|| benchmark::last_report().is_some_and(|report| report.fast_load(target)));
    load_with(settings, target, fast_load, Retry::Normally)
}

/// What [`load_with`] does when fast load fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Retry {
    /// Loads normally, with a note: what a commit wants.
    Normally,
    /// Fails: what the benchmark, which measures each mode, wants.
    Never,
}

/// Loads on `target`, with or without fast load on llama.cpp devices.
pub(super) fn load_with(
    settings: &Settings,
    target: Target,
    fast_load: bool,
    retry: Retry,
) -> Result<Box<dyn Backend>, BackendError> {
    let device = match target {
        Target::Vulkan | Target::Cpu => {
            let options = settings
                .llamacpp_options(target, fast_load)
                .map_err(BackendError::Unavailable)?;
            if retry == Retry::Never {
                return Ok(Box::new(LlamaCppBackend::start(&options)?));
            }
            if let Some(backend) = keep_loaded(settings, &options) {
                return Ok(Box::new(backend));
            }
            let (backend, note) =
                LlamaCppBackend::start_or_load_normally(&options).map_err(with_kept_model_hint)?;
            if let Some(note) = note {
                eprintln!("note: {note}");
            }
            return Ok(Box::new(backend));
        }
        Target::Npu => Device::Npu,
        Target::IntelGpu => Device::Gpu,
        Target::OpenvinoCpu => Device::Cpu,
    };
    let options = settings
        .genai_options(device)
        .map_err(BackendError::Unavailable)?;
    Ok(Box::new(GenaiBackend::load(&options)?))
}

/// The server kept loaded for `options`, when `model.keep_loaded` asks for
/// one; on any failure the model is loaded for this command only, with a
/// note.
fn keep_loaded(settings: &Settings, options: &LlamaCppOptions) -> Option<LlamaCppBackend> {
    if settings.keep_loaded.is_zero() {
        return None;
    }
    let result = match (paths::runtime_dir(env), std::env::current_exe()) {
        (Some(runtime), Ok(exe)) => server::connect(&runtime, options, settings.keep_loaded, &exe)
            .map_err(|e| e.to_string()),
        (None, _) => Err("$XDG_RUNTIME_DIR is not set".to_owned()),
        (_, Err(err)) => Err(err.to_string()),
    };
    result
        .map_err(|err| {
            eprintln!("note: cannot keep the model loaded ({err}); loading it for this commit")
        })
        .ok()
}

/// A model kept loaded may hold the memory a load needed: the error says so,
/// and how to free it.
fn with_kept_model_hint(err: BackendError) -> BackendError {
    let kept = paths::runtime_dir(env).is_some_and(|runtime| !server::status(&runtime).is_empty());
    match err {
        BackendError::Unavailable(text) if kept => BackendError::Unavailable(format!(
            "{text}; a model kept loaded holds memory too: `smart-commit server stop` frees it"
        )),
        other => other,
    }
}

/// Devices in the order the last benchmark ranked them, fastest first.
fn preferred_devices() -> Vec<Target> {
    benchmark::last_report()
        .map(|report| report.ranking())
        .unwrap_or_default()
}

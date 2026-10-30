//! Loading the model on a device, or on the first that works.

use anyhow::Result;
use smart_commit::backend::device::{self, Target};
use smart_commit::backend::genai::{Device, GenaiBackend};
use smart_commit::backend::llamacpp::{self, LlamaCppBackend, LlamaCppOptions};
use smart_commit::backend::server::{self, ConnectError};
use smart_commit::backend::{Backend, BackendError};
use smart_commit::config::{DeviceSetting, Settings};
use smart_commit::hardware::Hardware;
use smart_commit::paths;

use super::benchmark;
use super::settings::{env, hardware_sources, meminfo_path};
use super::ui::note;

/// Loads the model on the configured device, or picks one for `auto`.
pub(super) fn load_model(settings: &Settings) -> Result<(Box<dyn Backend>, Target)> {
    let mut candidates =
        device::candidates(settings.backend, settings.device, &preferred_devices())?;
    if settings.device == DeviceSetting::Auto {
        candidates = device::only_present(candidates, &Hardware::detect(&hardware_sources()));
    }
    let selection = device::load_first(&candidates, |target| load_on(settings, target))?;
    if settings.device == DeviceSetting::Auto && !selection.skipped.is_empty() {
        let skipped: Vec<&str> = selection.skipped.iter().map(|(t, _)| t.name()).collect();
        note!(
            "running on {}; {} unavailable (`smart-commit device` shows why)",
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
                note!("{note}");
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
    // Without its files there is nothing to keep: loading it normally says
    // what is missing, once.
    if settings.keep_loaded.is_zero() || llamacpp::missing_file(options).is_some() {
        return None;
    }
    let share = server::Share {
        max_percent: settings.keep_loaded_max_ram as u64,
        meminfo: meminfo_path(),
    };
    let result = match (paths::runtime_dir(env), std::env::current_exe()) {
        (Some(runtime), Ok(exe)) => {
            server::connect(&runtime, options, settings.keep_loaded, &share, &exe)
        }
        (None, _) => {
            Err(BackendError::Unavailable("$XDG_RUNTIME_DIR is not set".to_owned()).into())
        }
        (_, Err(err)) => Err(BackendError::Unavailable(err.to_string()).into()),
    };
    match result {
        Ok(backend) => Some(backend),
        // Not a failure: the machine is too small for this model to stay in
        // memory. Said to whoever asked for it to be kept; the default is
        // for the machines that can.
        Err(ConnectError::NotKept(refusal)) if !settings.keep_loaded_explicit => {
            tracing::debug!("not keeping the model loaded: {refusal}");
            None
        }
        Err(err @ ConnectError::NotKept(_)) => {
            note!(
                "not keeping the model loaded ({err}; model.keep_loaded_max_ram is {}%); \
                 loading it for this commit",
                share.max_percent
            );
            None
        }
        Err(err) => {
            note!("cannot keep the model loaded ({err}); loading it for this commit");
            None
        }
    }
}

/// A model kept loaded may hold the memory a load needed: the error says so,
/// and how to free it.
fn with_kept_model_hint(err: BackendError) -> BackendError {
    let kept = paths::runtime_dir(env).is_some_and(|runtime| !server::status(&runtime).is_empty());
    let hint = |text: String| {
        format!("{text}; a model kept loaded holds memory too: `smart-commit server stop` frees it")
    };
    match err {
        BackendError::Unavailable(text) if kept => BackendError::Unavailable(hint(text)),
        BackendError::NotInstalled(text) if kept => BackendError::NotInstalled(hint(text)),
        other => other,
    }
}

/// Devices in the order the last benchmark ranked them, fastest first.
fn preferred_devices() -> Vec<Target> {
    benchmark::last_report()
        .map(|report| report.ranking())
        .unwrap_or_default()
}

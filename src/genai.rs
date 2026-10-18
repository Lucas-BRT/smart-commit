//! The real backend: an OpenVINO GenAI `LLMPipeline` on the NPU (or GPU or
//! CPU).
//!
//! The OpenVINO GenAI library is loaded at run time, so npu-commit builds
//! and runs without it and reports how to install it when it is missing.
//! The binding's errors carry no message, so everything the user can fix
//! (missing model, missing library) is checked here first.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use openvino_genai::{ChatHistory, ChatMessage, LlmPipeline};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::backend::{Backend, BackendError, GenerationConfig, Metrics};
use crate::prompt::Prompt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Device {
    Npu,
    Gpu,
    Cpu,
}

impl Device {
    /// The name OpenVINO knows the device by.
    pub fn openvino_name(self) -> &'static str {
        match self {
            Self::Npu => "NPU",
            Self::Gpu => "GPU",
            Self::Cpu => "CPU",
        }
    }
}

impl fmt::Display for Device {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.openvino_name())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenaiOptions {
    /// A model exported by `scripts/export-model.sh`.
    pub model_dir: PathBuf,
    pub device: Device,
    /// Where OpenVINO keeps compiled models; without it every load on the
    /// NPU recompiles the graph, which takes tens of seconds.
    pub cache_dir: PathBuf,
    /// The NPU compiles for a fixed prompt length; must match
    /// [`crate::prompt::PromptOptions::max_prompt_tokens`].
    pub max_prompt_tokens: usize,
    /// The NPU also reserves room for the reply up front.
    pub max_new_tokens: usize,
    pub npu: NpuTuning,
}

/// How the NPU compiles the model. The defaults are what we measured to be
/// fastest with identical replies (docs/08-plano-proximas-fases.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NpuTuning {
    /// Prompt tokens processed per step. Below `max_prompt_tokens` the NPU
    /// only pays for the prompt's real length (TTFT 2.4 s -> 1.1 s with 512);
    /// 128 and 256 fail to compile with driver 1.38.
    pub prefill_chunk: usize,
    /// `BEST_PERF` compiles slower (once, then cached) for ~10% faster
    /// tokens; `FAST_COMPILE` is OpenVINO's default.
    pub best_perf: bool,
}

impl Default for NpuTuning {
    fn default() -> Self {
        Self {
            prefill_chunk: 512,
            best_perf: true,
        }
    }
}

pub struct GenaiBackend {
    pipeline: Mutex<LlmPipeline>,
    description: String,
    last_metrics: Mutex<Option<Metrics>>,
}

impl GenaiBackend {
    pub fn load(options: &GenaiOptions) -> Result<Self, BackendError> {
        let model_xml = options.model_dir.join("openvino_model.xml");
        if !model_xml.is_file() {
            return Err(BackendError::Unavailable(format!(
                "no model in {}; export one with scripts/export-model.sh",
                options.model_dir.display()
            )));
        }
        let identity = model_identity(&options.model_dir)?;
        openvino_genai::load().map_err(|err| {
            BackendError::Unavailable(format!(
                "OpenVINO GenAI not found ({err}); install it with scripts/setup-npu.sh"
            ))
        })?;
        std::fs::create_dir_all(&options.cache_dir).map_err(|err| {
            BackendError::Unavailable(format!(
                "cannot create {}: {err}",
                options.cache_dir.display()
            ))
        })?;

        let properties = pipeline_properties(options);
        let properties: Vec<(&str, &str)> =
            properties.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let model_dir = options.model_dir.to_string_lossy();
        let pipeline =
            LlmPipeline::with_properties(&model_dir, options.device.openvino_name(), &properties)
                .map_err(|err| {
                BackendError::Unavailable(format!(
                    "cannot load the model on the {} ({err:?}); \
                         scripts/check-npu.sh checks the setup",
                    options.device
                ))
            })?;
        Ok(Self {
            pipeline: Mutex::new(pipeline),
            description: describe(options, &identity),
            last_metrics: Mutex::new(None),
        })
    }
}

impl Backend for GenaiBackend {
    fn describe(&self) -> String {
        self.description.clone()
    }

    fn last_metrics(&self) -> Option<Metrics> {
        self.last_metrics.lock().expect("metrics lock").clone()
    }

    fn generate(&self, prompt: &Prompt, config: &GenerationConfig) -> Result<String, BackendError> {
        let failed = |what: &str, err: &dyn fmt::Debug| {
            BackendError::Generation(format!("{what} failed: {err:?}"))
        };
        let mut history = ChatHistory::new().map_err(|e| failed("chat history", &e))?;
        for message in [
            ChatMessage::system(&prompt.system),
            ChatMessage::user(&prompt.user),
        ] {
            history
                .push(&message)
                .map_err(|e| failed("chat history", &e))?;
        }

        // Everything explicit, nothing from the model's generation_config.json.
        let mut genai_config =
            openvino_genai::GenerationConfig::new().map_err(|e| failed("config", &e))?;
        genai_config
            .set_max_new_tokens(config.max_new_tokens)
            .and_then(|()| genai_config.set_do_sample(false))
            .and_then(|()| genai_config.set_num_beams(1))
            .and_then(|()| genai_config.set_repetition_penalty(1.0))
            .map_err(|e| failed("config", &e))?;

        let results = self
            .pipeline
            .lock()
            .expect("pipeline lock")
            .generate_with_history(&history, Some(&genai_config), None)
            .map_err(|e| failed("generation", &e))?;
        let text = results.get_string().map_err(|e| failed("decoding", &e))?;
        if let Ok(perf) = results.get_perf_metrics() {
            let metrics = (|| {
                Some(Metrics {
                    input_tokens: perf.get_num_input_tokens().ok()?,
                    output_tokens: perf.get_num_generation_tokens().ok()?,
                    load_ms: perf.get_load_time().ok()?,
                    time_to_first_token_ms: perf.get_ttft().ok()?.0,
                    time_per_output_token_ms: perf.get_tpot().ok()?.0,
                    generate_ms: perf.get_generate_duration().ok()?.0,
                })
            })();
            *self.last_metrics.lock().expect("metrics lock") = metrics;
        }
        Ok(text)
    }
}

/// Device properties for the pipeline. The NPU-only ones are rejected by
/// the other devices.
pub fn pipeline_properties(options: &GenaiOptions) -> Vec<(&'static str, String)> {
    let mut properties = vec![(
        "CACHE_DIR",
        options.cache_dir.to_string_lossy().into_owned(),
    )];
    if options.device == Device::Npu {
        properties.push(("MAX_PROMPT_LEN", options.max_prompt_tokens.to_string()));
        properties.push(("MIN_RESPONSE_LEN", options.max_new_tokens.to_string()));
        properties.push((
            "NPUW_LLM_PREFILL_CHUNK_SIZE",
            options.npu.prefill_chunk.to_string(),
        ));
        let hint = if options.npu.best_perf {
            "BEST_PERF"
        } else {
            "FAST_COMPILE"
        };
        properties.push(("GENERATE_HINT", hint.to_owned()));
    }
    properties
}

/// The first 16 hex digits of the SHA-256 of the model's MANIFEST.sha256,
/// which lists the SHA-256 of every model file; `unverified` without one.
pub fn model_identity(model_dir: &Path) -> Result<String, BackendError> {
    match std::fs::read(model_dir.join("MANIFEST.sha256")) {
        Ok(manifest) => {
            let digest = Sha256::digest(&manifest);
            Ok(digest[..8].iter().map(|b| format!("{b:02x}")).collect())
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok("unverified".to_owned()),
        Err(err) => Err(BackendError::Unavailable(format!(
            "cannot read the manifest in {}: {err}",
            model_dir.display()
        ))),
    }
}

/// What traces record about the backend: enough to tell whether a replay
/// on a real model should give the same replies.
fn describe(options: &GenaiOptions, identity: &str) -> String {
    let model = options.model_dir.file_name().map_or_else(
        || options.model_dir.to_string_lossy(),
        |n| n.to_string_lossy(),
    );
    let mut description = format!(
        "openvino-genai device={} model={model} manifest={identity}",
        options.device
    );
    if options.device == Device::Npu {
        description.push_str(&format!(
            " max_prompt_len={} min_response_len={} prefill_chunk={} generate_hint={}",
            options.max_prompt_tokens,
            options.max_new_tokens,
            options.npu.prefill_chunk,
            if options.npu.best_perf {
                "best_perf"
            } else {
                "fast_compile"
            }
        ));
    }
    description
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(model_dir: &Path, device: Device) -> GenaiOptions {
        GenaiOptions {
            model_dir: model_dir.to_path_buf(),
            device,
            cache_dir: PathBuf::from("/cache/npu-commit/openvino"),
            max_prompt_tokens: 1024,
            max_new_tokens: 128,
            npu: NpuTuning::default(),
        }
    }

    #[test]
    fn npu_gets_static_shape_properties() {
        let npu = pipeline_properties(&options(Path::new("/m"), Device::Npu));
        assert_eq!(
            npu,
            [
                ("CACHE_DIR", "/cache/npu-commit/openvino".to_owned()),
                ("MAX_PROMPT_LEN", "1024".to_owned()),
                ("MIN_RESPONSE_LEN", "128".to_owned()),
                ("NPUW_LLM_PREFILL_CHUNK_SIZE", "512".to_owned()),
                ("GENERATE_HINT", "BEST_PERF".to_owned()),
            ]
        );
        let mut untuned = options(Path::new("/m"), Device::Npu);
        untuned.npu.best_perf = false;
        let props = pipeline_properties(&untuned);
        assert_eq!(
            props.last().unwrap(),
            &("GENERATE_HINT", "FAST_COMPILE".to_owned())
        );
        for device in [Device::Cpu, Device::Gpu] {
            let props = pipeline_properties(&options(Path::new("/m"), device));
            assert_eq!(
                props,
                [("CACHE_DIR", "/cache/npu-commit/openvino".to_owned())]
            );
        }
    }

    #[test]
    fn model_identity_hashes_the_manifest() {
        let dir = tempfile::TempDir::new().unwrap();
        assert_eq!(model_identity(dir.path()).unwrap(), "unverified");
        std::fs::write(
            dir.path().join("MANIFEST.sha256"),
            "abc  openvino_model.xml\n",
        )
        .unwrap();
        let identity = model_identity(dir.path()).unwrap();
        assert_eq!(identity.len(), 16);
        assert_eq!(identity, model_identity(dir.path()).unwrap());
        std::fs::write(
            dir.path().join("MANIFEST.sha256"),
            "abd  openvino_model.xml\n",
        )
        .unwrap();
        assert_ne!(identity, model_identity(dir.path()).unwrap());
    }

    #[test]
    fn description_names_device_model_and_manifest() {
        let dir = Path::new("/models/qwen-int4");
        assert_eq!(
            describe(&options(dir, Device::Npu), "0123456789abcdef"),
            "openvino-genai device=NPU model=qwen-int4 manifest=0123456789abcdef \
             max_prompt_len=1024 min_response_len=128 prefill_chunk=512 generate_hint=best_perf"
        );
        assert_eq!(
            describe(&options(dir, Device::Cpu), "unverified"),
            "openvino-genai device=CPU model=qwen-int4 manifest=unverified"
        );
    }

    #[test]
    fn missing_model_is_reported_before_loading_openvino() {
        let dir = tempfile::TempDir::new().unwrap();
        let err = GenaiBackend::load(&options(dir.path(), Device::Npu))
            .err()
            .expect("no model");
        assert_eq!(
            err,
            BackendError::Unavailable(format!(
                "no model in {}; export one with scripts/export-model.sh",
                dir.path().display()
            ))
        );
    }
}

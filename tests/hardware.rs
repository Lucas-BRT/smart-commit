//! Runs the real model. Needs OpenVINO GenAI (scripts/setup-npu.sh) and an
//! exported model (scripts/export-model.sh), so it is ignored by default:
//!
//!   cargo test --test hardware -- --ignored --test-threads 1
//!
//! `NPU_COMMIT_TEST_DEVICE` picks the device: `npu` (the default), `gpu` or
//! `cpu` through OpenVINO, or `vulkan` through llama.cpp.
//! `NPU_COMMIT_MODEL_DIR`, `NPU_COMMIT_GGUF` and `NPU_COMMIT_LLAMA_SERVER`
//! override where the models and the server are. The prompts
//! come from the recorded traces in tests/fixtures/traces, so every run sees
//! the same inputs.

use std::fs;
use std::path::{Path, PathBuf};

use npu_commit::backend::{Backend, GenerationConfig};
use npu_commit::diff::estimate_tokens;
use npu_commit::genai::{Device, GenaiBackend, GenaiOptions, NpuTuning};
use npu_commit::llamacpp::{LlamaCppBackend, LlamaCppOptions};
use npu_commit::paths;
use npu_commit::prompt::{DEFAULT_MAX_PROMPT_TOKENS, Prompt};
use npu_commit::trace::Trace;

fn backend() -> Box<dyn Backend> {
    let env = |name: &str| std::env::var(name).ok();
    let path = |var: &str, default: Option<PathBuf>| {
        env(var)
            .map(PathBuf::from)
            .or(default)
            .expect("HOME or the override variable")
    };
    let device = match env("NPU_COMMIT_TEST_DEVICE").as_deref() {
        Some("vulkan") => {
            let backend = LlamaCppBackend::start(&LlamaCppOptions {
                server: path("NPU_COMMIT_LLAMA_SERVER", paths::default_llama_server(env)),
                model: path("NPU_COMMIT_GGUF", paths::default_gguf_file(env)),
                vulkan_device: 0,
                max_prompt_tokens: DEFAULT_MAX_PROMPT_TOKENS,
                max_new_tokens: GenerationConfig::default().max_new_tokens,
                startup_timeout: std::time::Duration::from_secs(120),
            });
            return Box::new(backend.unwrap_or_else(|err| panic!("{err}")));
        }
        Some("cpu") => Device::Cpu,
        Some("gpu") => Device::Gpu,
        Some("npu") | None => Device::Npu,
        Some(other) => panic!("unknown NPU_COMMIT_TEST_DEVICE {other:?}"),
    };
    let backend = GenaiBackend::load(&GenaiOptions {
        model_dir: path("NPU_COMMIT_MODEL_DIR", paths::default_model_dir(env)),
        device,
        cache_dir: paths::default_cache_dir(env).expect("HOME"),
        max_prompt_tokens: DEFAULT_MAX_PROMPT_TOKENS,
        max_new_tokens: GenerationConfig::default().max_new_tokens,
        npu: NpuTuning::default(),
    });
    Box::new(backend.unwrap_or_else(|err| panic!("{err}")))
}

/// Every prompt recorded in the fixtures, in a fixed order.
fn fixture_prompts() -> Vec<(String, Prompt)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/traces");
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    paths.sort();
    paths
        .iter()
        .flat_map(|path| {
            let trace = Trace::from_json(&fs::read_to_string(path).unwrap()).unwrap();
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            trace
                .generation
                .attempts
                .into_iter()
                .enumerate()
                .map(move |(n, a)| (format!("{name}#{}", n + 1), a.prompt))
        })
        .collect()
}

#[test]
#[ignore = "needs OpenVINO GenAI and an exported model"]
fn generates_a_reply_for_every_fixture_prompt() {
    let backend = backend();
    let config = GenerationConfig::default();
    println!("backend: {}", backend.describe());
    for (name, prompt) in fixture_prompts() {
        let reply = backend.generate(&prompt, &config).unwrap();
        let metrics = backend.last_metrics().unwrap();
        println!("{name}: {reply:?}\n  {metrics:?}");
        assert!(!reply.trim().is_empty(), "{name}: empty reply");
        assert!(metrics.output_tokens <= config.max_new_tokens, "{name}");
    }
}

#[test]
#[ignore = "needs OpenVINO GenAI and an exported model"]
fn same_prompt_gives_the_same_reply() {
    let backend = backend();
    let config = GenerationConfig::default();
    let (name, prompt) = fixture_prompts().remove(0);
    let first = backend.generate(&prompt, &config).unwrap();
    let second = backend.generate(&prompt, &config).unwrap();
    assert_eq!(
        first, second,
        "{name}: greedy decoding must be deterministic"
    );
}

#[test]
#[ignore = "needs OpenVINO GenAI and an exported model"]
fn token_estimate_never_undercounts() {
    // The prompt budget relies on estimate_tokens being pessimistic: if the
    // real count were higher, a full prompt would overflow the NPU's
    // MAX_PROMPT_LEN and the generation would fail.
    let backend = backend();
    let config = GenerationConfig { max_new_tokens: 1 };
    for (name, prompt) in fixture_prompts() {
        backend.generate(&prompt, &config).unwrap();
        let real = backend.last_metrics().unwrap().input_tokens;
        let estimate = estimate_tokens(&prompt.system) + estimate_tokens(&prompt.user);
        println!(
            "{name}: estimated {estimate} (+ template), real {real} ({:.2} chars/token)",
            (prompt.system.chars().count() + prompt.user.chars().count()) as f64 / real as f64
        );
        // The prompt module reserves 40 tokens for the chat template.
        assert!(real <= estimate + 40, "{name}: {real} > {estimate} + 40");
    }
}

#[test]
#[ignore = "needs an NPU, OpenVINO GenAI and an exported model"]
fn npu_tuning_does_not_change_replies() {
    if std::env::var("NPU_COMMIT_TEST_DEVICE").is_ok_and(|d| d != "npu") {
        return;
    }
    let env = |name: &str| std::env::var(name).ok();
    let load = |npu: NpuTuning| {
        GenaiBackend::load(&GenaiOptions {
            model_dir: env("NPU_COMMIT_MODEL_DIR")
                .map(PathBuf::from)
                .or_else(|| paths::default_model_dir(env))
                .unwrap(),
            device: Device::Npu,
            cache_dir: paths::default_cache_dir(env).unwrap(),
            max_prompt_tokens: DEFAULT_MAX_PROMPT_TOKENS,
            max_new_tokens: GenerationConfig::default().max_new_tokens,
            npu,
        })
        .unwrap_or_else(|err| panic!("{err}"))
    };
    let config = GenerationConfig::default();
    let reply_all = |backend: &GenaiBackend| -> Vec<String> {
        fixture_prompts()
            .iter()
            .map(|(_, p)| backend.generate(p, &config).unwrap())
            .collect()
    };
    let untuned = reply_all(&load(NpuTuning {
        prefill_chunk: DEFAULT_MAX_PROMPT_TOKENS,
        best_perf: false,
    }));
    let tuned = reply_all(&load(NpuTuning::default()));
    assert_eq!(tuned, untuned);
}

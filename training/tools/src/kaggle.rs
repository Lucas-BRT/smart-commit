//! The bundles of the Kaggle notebooks, to upload as private datasets:
//! `train` for `kaggle_train.ipynb` (SFT), `mine` for `kaggle_mine.ipynb`
//! (sampling the replies the pairs are made of) and `dpo` for
//! `kaggle_dpo.ipynb`. Each holds the scripts the notebook runs, what the
//! runs read, a `runs.json` describing them and the dataset's metadata. What a
//! notebook downloads is pinned in `runs.json`: the base models by revision,
//! llama.cpp's CUDA build by SHA-256 and the fine-tunes by the revision and
//! SHA-256 of the registry. `requirements.txt` has our `uv.lock` versions,
//! without torch: Kaggle's own CUDA build is kept, and `run.json` records it.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail, ensure};
use serde_json::{Map, Value, json};

/// Kaggle's username, the owner of the datasets.
const USER: &str = "lucasbrt";
/// The revisions `scripts/export-model.sh` pins.
const BASES: [(&str, &str, &str); 2] = [
    (
        "1.5b",
        "Qwen/Qwen2.5-Coder-1.5B-Instruct",
        "2e1fd397ee46e1388853d2af2c993145b0f1098a",
    ),
    (
        "0.5b",
        "Qwen/Qwen2.5-Coder-0.5B-Instruct",
        "ea3f2471cf1b1f0db85067f1ef93848e38e88c25",
    ),
];
const PACKAGES: [&str; 7] = [
    "transformers",
    "tokenizers",
    "safetensors",
    "huggingface-hub",
    "peft",
    "accelerate",
    "bitsandbytes",
];
/// T4 and P100 have no bf16: a 4-bit base with fp16 compute for the SFT, fp16 for the DPO.
const TRAIN_ARGS: [&str; 5] = ["--quant", "4bit", "--dtype", "fp16", "--resume"];
const DPO_ARGS: [&str; 2] = ["--dtype", "fp16"];
const ADAPTER_FILES: [&str; 2] = ["adapter_config.json", "adapter_model.safetensors"];
/// The version of the models the DPO starts from and the mining samples.
const MODEL_VERSION: &str = "v0.1";
/// The CUDA 12.8 build Kaggle's T4 driver runs, and the runtime libraries it needs.
const LLAMA_RELEASE: &str = "b11381";
const LLAMA_ASSETS: [(&str, &str); 2] = [
    (
        "llama-b11381-bin-ubuntu-cuda-12.8-x64.tar.gz",
        "86805e29e1277297334d20bcb2a75cd6d921d0b8fcc22d521d5c3961f3990fbb",
    ),
    (
        "cudart-llama-b11381-bin-ubuntu-cuda-12.8-x64.tar.gz",
        "13c6d5079689e8c0bc90fc4b89188a5d9777cc2f362398f3376fd4c8d5c3c044",
    ),
];
const LLAMA_URL: &str = "https://github.com/ggml-org/llama.cpp/releases/download/{release}/{name}";
/// Seconds a job takes per input and sample on a T4, to put the longest first.
const COST: [(&str, f64); 2] = [("0.5b", 1.0), ("1.5b", 2.2)];

/// `training/`, where the scripts and `uv.lock` are.
pub fn training_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn sha256_file(path: &Path) -> Result<String> {
    smart_commit::models::sha256_file(path)
        .with_context(|| format!("cannot read {}", path.display()))
}

fn copy(from: &Path, to: &Path) -> Result<()> {
    std::fs::copy(from, to)
        .with_context(|| format!("cannot copy {} to {}", from.display(), to.display()))?;
    Ok(())
}

fn write_json(path: &Path, value: &Value) -> Result<()> {
    std::fs::write(path, serde_json::to_string_pretty(value)? + "\n")
        .with_context(|| format!("cannot write {}", path.display()))
}

fn base(model: &str) -> Result<Value> {
    let known: Vec<&str> = BASES.iter().map(|(key, ..)| *key).collect();
    let (_, repo, revision) = BASES
        .iter()
        .find(|(key, ..)| *key == model)
        .with_context(|| format!("no base model {model:?}; the bundle knows {known:?}"))?;
    Ok(json!({"repo": repo, "revision": revision}))
}

/// The versions of `uv.lock`, by package name.
fn locked_versions(root: &Path) -> Result<Map<String, Value>> {
    let lock: toml::Table = std::fs::read_to_string(root.join("uv.lock"))?.parse()?;
    let mut versions = Map::new();
    for package in lock["package"]
        .as_array()
        .context("uv.lock has no packages")?
    {
        if let (Some(name), Some(version)) = (package["name"].as_str(), package["version"].as_str())
        {
            versions.insert(name.to_owned(), json!(version));
        }
    }
    Ok(versions)
}

/// Copies the scripts, writes `requirements.txt` (for the notebooks that
/// install packages) and the dataset's metadata.
fn start(out: &Path, scripts: &[&str], requirements: bool) -> Result<()> {
    let root = training_root();
    std::fs::create_dir_all(out)?;
    for script in scripts {
        copy(&root.join(script), &out.join(script))?;
    }
    if requirements {
        let versions = locked_versions(&root)?;
        let mut lines = String::new();
        for package in PACKAGES {
            let version = versions
                .get(package)
                .and_then(Value::as_str)
                .with_context(|| format!("{package} is not in uv.lock"))?;
            lines.push_str(&format!("{package}=={version}\n"));
        }
        std::fs::write(out.join("requirements.txt"), lines)?;
    }
    let name = out
        .file_name()
        .and_then(|n| n.to_str())
        .context("a bundle without a name")?;
    write_json(
        &out.join("dataset-metadata.json"),
        &json!({
            "title": format!("smart-commit-training-{name}"),
            "id": format!("{USER}/smart-commit-training-{name}"),
            "licenses": [{"name": "other"}],
        }),
    )
}

fn words(text: Option<&str>) -> Vec<String> {
    text.map(|t| t.split_whitespace().map(str::to_owned).collect())
        .unwrap_or_default()
}

/// The SFT bundle: `NAME=MODEL:EXAMPLES[:TRAIN_ARGS]` for each run.
pub fn train(out: &Path, val: Option<&Path>, specs: &[String]) -> Result<Vec<String>> {
    ensure!(!specs.is_empty(), "no runs");
    start(out, &["train.py"], true)?;
    if let Some(val) = val {
        copy(val, &out.join("val.examples.jsonl"))?;
    }
    let mut runs = Vec::new();
    for spec in specs {
        let (name, rest) = spec
            .split_once('=')
            .with_context(|| format!("{spec:?} is not NAME=MODEL:EXAMPLES"))?;
        let (model, rest) = rest
            .split_once(':')
            .with_context(|| format!("{spec:?} has no examples"))?;
        let (examples, extra) = rest
            .split_once(':')
            .map_or((rest, None), |(e, x)| (e, Some(x)));
        let target = format!("{name}.examples.jsonl");
        copy(Path::new(examples), &out.join(&target))?;
        let mut args: Vec<String> = TRAIN_ARGS.iter().map(|a| (*a).to_owned()).collect();
        args.extend(words(extra));
        runs.push(json!({
            "name": name,
            "base": base(model)?,
            "examples": target,
            "sha256": sha256_file(&out.join(&target))?,
            "val": val.map(|_| "val.examples.jsonl"),
            "args": args,
        }));
    }
    write_json(&out.join("runs.json"), &json!({"runs": runs}))?;
    Ok(runs
        .iter()
        .map(|r| {
            format!(
                "{}: {} {}",
                r["name"].as_str().unwrap_or(""),
                r["examples"].as_str().unwrap_or(""),
                words(Some(
                    &r["args"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(" ")
                ))
                .join(" ")
            )
        })
        .collect())
}

/// The fine-tune of `MODEL_VERSION` for a size, as the registry pins it.
fn pinned_model(size: &str) -> Result<Value> {
    let id = format!("smart-commit-{size}-{MODEL_VERSION}");
    let model = smart_commit::models::find(&id).with_context(|| {
        let known: Vec<&str> = BASES.iter().map(|(key, ..)| *key).collect();
        format!("{id} is not in the registry (sizes {known:?})")
    })?;
    Ok(json!({
        "repo": model.gguf.repo,
        "revision": model.gguf.revision,
        "file": model.gguf.file,
        "size": model.gguf.size,
        "sha256": model.gguf.sha256,
    }))
}

/// Every `count`-th line of `inputs`, `meta` and `prompts` to each `shard-K`.
fn shards(src: &Path, out: &Path, count: usize) -> Result<Vec<(String, usize, String)>> {
    let read = |name: &str| -> Result<Vec<String>> {
        let text = std::fs::read_to_string(src.join(format!("{name}.jsonl")))
            .with_context(|| format!("cannot read {name}.jsonl in {}", src.display()))?;
        Ok(text.lines().map(str::to_owned).collect())
    };
    let files = [
        ("inputs", read("inputs")?),
        ("meta", read("meta")?),
        ("prompts", read("prompts")?),
    ];
    ensure!(
        files
            .iter()
            .all(|(_, lines)| lines.len() == files[0].1.len()),
        "{}: inputs, meta and prompts must have the same number of lines",
        src.display()
    );
    let mut written = Vec::new();
    for k in 0..count {
        let dir = out.join(format!("shard-{k}"));
        std::fs::create_dir_all(&dir)?;
        let mut kept = 0;
        for (name, lines) in &files {
            let mine: String = lines
                .iter()
                .skip(k)
                .step_by(count)
                .map(|l| format!("{l}\n"))
                .collect();
            kept = lines.iter().skip(k).step_by(count).count();
            std::fs::write(dir.join(format!("{name}.jsonl")), mine)?;
        }
        written.push((
            format!("shard-{k}"),
            kept,
            sha256_file(&dir.join("prompts.jsonl"))?,
        ));
    }
    Ok(written)
}

/// The mining bundle: the prompts of `src` (what `mine_pairs.py --prompts-only`
/// wrote) in `count` shards, and a job for each model and shard.
pub fn mine(
    out: &Path,
    src: &Path,
    count: usize,
    samples: usize,
    seed: u64,
    models: &[String],
) -> Result<Vec<String>> {
    ensure!(count > 0, "at least one shard");
    let models: Vec<String> = if models.is_empty() {
        vec!["0.5b".into(), "1.5b".into()]
    } else {
        models.to_vec()
    };
    let mut pinned = Map::new();
    for model in &models {
        pinned.insert(
            model.clone(),
            pinned_model(model).with_context(|| format!("{model}: not in the registry"))?,
        );
    }
    start(out, &["mine_pairs.py"], false)?;
    let parts = shards(src, out, count)?;
    let cost = |model: &str| {
        COST.iter()
            .find(|(m, _)| *m == model)
            .map_or(1.0, |(_, c)| *c)
    };
    let mut jobs: Vec<(f64, Value)> = Vec::new();
    for model in &models {
        for (dir, inputs, digest) in &parts {
            jobs.push((
                *inputs as f64 * cost(model),
                json!({"name": format!("{model}-{dir}"), "model": model, "shard": dir, "inputs": inputs, "prompts_sha256": digest}),
            ));
        }
    }
    jobs.sort_by(|a, b| b.0.total_cmp(&a.0));
    let assets: Vec<Value> = LLAMA_ASSETS
        .iter()
        .map(|(name, sha)| json!({"name": name, "sha256": sha}))
        .collect();
    write_json(
        &out.join("runs.json"),
        &json!({
            "llama": {"release": LLAMA_RELEASE, "assets": assets, "url": LLAMA_URL},
            "models": pinned,
            "sampling": {"rules": "full", "diff": "plain", "samples": samples, "temperature": 0.7, "top_p": 0.95, "seed": seed, "workers": 4},
            "jobs": jobs.iter().map(|(_, j)| j).collect::<Vec<_>>(),
        }),
    )?;
    Ok(jobs
        .iter()
        .map(|(_, j)| {
            format!(
                "{}: {} inputs",
                j["name"].as_str().unwrap_or(""),
                j["inputs"]
            )
        })
        .collect())
}

/// The DPO bundle: `NAME=MODEL:SFT_ADAPTER:TRAIN:VAL[:TRAIN_ARGS]` for each run.
pub fn dpo(out: &Path, per_gpu: usize, specs: &[String]) -> Result<Vec<String>> {
    ensure!(
        !specs.is_empty() && per_gpu > 0,
        "no runs, or fewer than one a GPU"
    );
    start(out, &["train.py", "train_dpo.py"], true)?;
    let mut runs = Vec::new();
    for spec in specs {
        let (name, rest) = spec
            .split_once('=')
            .with_context(|| format!("{spec:?} is not NAME=MODEL:SFT:TRAIN:VAL"))?;
        let mut parts = rest.splitn(5, ':');
        let (model, adapter, train, val) = (parts.next(), parts.next(), parts.next(), parts.next());
        let (Some(model), Some(adapter), Some(train), Some(val)) = (model, adapter, train, val)
        else {
            bail!("{spec:?} is not NAME=MODEL:SFT:TRAIN:VAL");
        };
        let base = base(model).with_context(|| name.to_owned())?;
        let files = [("train", train), ("val", val)]
            .map(|(kind, source)| (format!("{name}.{kind}.jsonl"), source));
        for (target, source) in &files {
            copy(Path::new(source), &out.join(target))?;
        }
        let adapter = Path::new(adapter);
        // Named by its weights: two adapters of the same parent directory name
        // (`v1/x/adapter`, `v2/x/adapter`) do not overwrite each other.
        let weights = sha256_file(&adapter.join("adapter_model.safetensors"))?;
        let sft = format!("sft-{}", &weights[..12]);
        std::fs::create_dir_all(out.join(&sft))?;
        for file in ADAPTER_FILES {
            copy(&adapter.join(file), &out.join(&sft).join(file))?;
        }
        let mut args: Vec<String> = DPO_ARGS.iter().map(|a| (*a).to_owned()).collect();
        args.extend(words(parts.next()));
        runs.push(json!({
            "name": name,
            "base": base,
            "sft": sft,
            "sft_sha256": sha256_file(&out.join(&sft).join("adapter_model.safetensors"))?,
            "train": files[0].0,
            "train_sha256": sha256_file(&out.join(&files[0].0))?,
            "val": files[1].0,
            "args": args,
        }));
    }
    write_json(
        &out.join("runs.json"),
        &json!({"per_gpu": per_gpu, "runs": runs}),
    )?;
    Ok(runs
        .iter()
        .map(|r| {
            let args: Vec<&str> = r["args"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(Value::as_str)
                .collect();
            format!(
                "{}: {} {}",
                r["name"].as_str().unwrap_or(""),
                r["train"].as_str().unwrap_or(""),
                args.join(" ")
            )
        })
        .collect())
}

#[cfg(test)]
#[path = "kaggle_tests.rs"]
mod tests;

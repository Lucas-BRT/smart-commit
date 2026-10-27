use std::path::Path;

use super::*;

/// Inputs, meta and prompts of `count` cases, a number in each line.
fn write_src(dir: &Path, count: usize) {
    std::fs::create_dir_all(dir).unwrap();
    for name in ["inputs", "meta", "prompts"] {
        let text: String = (0..count)
            .map(|n| format!("{}\n", json!({ name: n })))
            .collect();
        std::fs::write(dir.join(format!("{name}.jsonl")), text).unwrap();
    }
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn lines(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn is_hex(text: &Value, len: usize) -> bool {
    text.as_str()
        .is_some_and(|t| t.len() == len && t.bytes().all(|b| b.is_ascii_hexdigit()))
}

#[test]
fn the_mining_models_are_those_of_the_registry() {
    for (size, id) in [
        ("0.5b", "smart-commit-0.5b-v0.1"),
        ("1.5b", "smart-commit-1.5b-v0.1"),
    ] {
        let model = pinned_model(size).unwrap();
        assert_eq!(model["repo"], format!("LucasBRT/{id}"));
        assert_eq!(model["file"], format!("{id}-q4_k_m.gguf"));
        assert!(is_hex(&model["revision"], 40) && is_hex(&model["sha256"], 64));
        assert!(model["size"].as_u64().unwrap() > 100_000_000);
    }
    let err = pinned_model("7b").unwrap_err().to_string();
    assert!(err.contains("not in the registry"), "{err}");
}

#[test]
fn every_nth_input_goes_to_each_shard() {
    let dir = tempfile::TempDir::new().unwrap();
    write_src(&dir.path().join("src"), 10);
    let parts = shards(&dir.path().join("src"), &dir.path().join("out"), 3).unwrap();
    assert_eq!(
        parts.iter().map(|(_, n, _)| *n).collect::<Vec<_>>(),
        [4, 3, 3]
    );
    for k in 0..3 {
        for name in ["inputs", "meta", "prompts"] {
            let rows: Vec<u64> = lines(&dir.path().join(format!("out/shard-{k}/{name}.jsonl")))
                .iter()
                .map(|r| r[name].as_u64().unwrap())
                .collect();
            assert_eq!(
                rows,
                (k as u64..10).step_by(3).collect::<Vec<_>>(),
                "shard {k} {name}"
            );
        }
    }
    // A source whose files disagree is refused.
    std::fs::write(dir.path().join("src/meta.jsonl"), "{}\n").unwrap();
    let err = shards(&dir.path().join("src"), &dir.path().join("bad"), 2)
        .unwrap_err()
        .to_string();
    assert!(err.contains("same number of lines"), "{err}");
}

#[test]
fn the_longest_mining_jobs_come_first_and_everything_is_pinned() {
    let dir = tempfile::TempDir::new().unwrap();
    write_src(&dir.path().join("src"), 9);
    let out = dir.path().join("bundle");
    let lines = mine(&out, &dir.path().join("src"), 3, 5, 7, &[]).unwrap();
    let spec = read_json(&out.join("runs.json"));
    let names: Vec<&str> = spec["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|j| j["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "1.5b-shard-0",
            "1.5b-shard-1",
            "1.5b-shard-2",
            "0.5b-shard-0",
            "0.5b-shard-1",
            "0.5b-shard-2"
        ]
    );
    assert_eq!(lines.len(), 6);
    assert_eq!(
        (
            spec["sampling"]["samples"].clone(),
            spec["sampling"]["seed"].clone()
        ),
        (json!(5), json!(7))
    );
    assert_eq!(spec["sampling"]["rules"], "full");
    for asset in spec["llama"]["assets"].as_array().unwrap() {
        assert!(is_hex(&asset["sha256"], 64));
    }
    assert!(is_hex(&spec["jobs"][0]["prompts_sha256"], 64));
    for file in [
        "mine_pairs.py",
        "dataset-metadata.json",
        "shard-0/prompts.jsonl",
    ] {
        assert!(out.join(file).exists(), "{file}");
    }
    // The notebook installs nothing: sampling uses the standard library.
    assert!(!out.join("requirements.txt").exists());
    assert_eq!(
        read_json(&out.join("dataset-metadata.json"))["id"],
        "lucasbrt/smart-commit-training-bundle"
    );
    // One model only; one the registry does not have is refused.
    let one = dir.path().join("one");
    mine(&one, &dir.path().join("src"), 3, 6, 0, &["0.5b".to_owned()]).unwrap();
    let jobs = read_json(&one.join("runs.json"))["jobs"]
        .as_array()
        .unwrap()
        .clone();
    assert!(jobs.iter().all(|j| j["model"] == "0.5b"));
    let err = mine(
        &dir.path().join("x"),
        &dir.path().join("src"),
        3,
        6,
        0,
        &["7b".to_owned()],
    )
    .unwrap_err();
    assert!(
        format!("{err:#}").contains("not in the registry"),
        "{err:#}"
    );
}

#[test]
fn an_sft_run_has_the_precision_flags_of_the_t4_and_its_own_arguments() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(dir.path().join("e.jsonl"), "{}\n").unwrap();
    std::fs::write(dir.path().join("v.jsonl"), "{}\n{}\n").unwrap();
    let out = dir.path().join("b");
    let spec = format!("h2=0.5b:{}:--rank 32", dir.path().join("e.jsonl").display());
    let plain = format!("h3=1.5b:{}", dir.path().join("e.jsonl").display());
    train(&out, Some(&dir.path().join("v.jsonl")), &[spec, plain]).unwrap();
    let runs = read_json(&out.join("runs.json"));
    let (first, second) = (&runs["runs"][0], &runs["runs"][1]);
    assert_eq!(
        first["args"],
        json!([
            "--quant", "4bit", "--dtype", "fp16", "--resume", "--rank", "32"
        ])
    );
    assert_eq!(
        second["args"],
        json!(["--quant", "4bit", "--dtype", "fp16", "--resume"])
    );
    assert_eq!(first["base"]["repo"], "Qwen/Qwen2.5-Coder-0.5B-Instruct");
    assert_eq!(second["base"]["repo"], "Qwen/Qwen2.5-Coder-1.5B-Instruct");
    assert_eq!(first["val"], "val.examples.jsonl");
    assert_eq!(first["sha256"], sha256_of(b"{}\n"));
    assert!(out.join("val.examples.jsonl").exists() && out.join("train.py").exists());
    // Without validation examples, no val; a base the bundle does not know is refused.
    let none = dir.path().join("n");
    train(
        &none,
        None,
        &[format!("a=0.5b:{}", dir.path().join("e.jsonl").display())],
    )
    .unwrap();
    assert!(read_json(&none.join("runs.json"))["runs"][0]["val"].is_null());
    let err = train(
        &dir.path().join("x"),
        None,
        &[format!("a=7b:{}", dir.path().join("e.jsonl").display())],
    )
    .unwrap_err();
    assert!(err.to_string().contains("no base model"), "{err}");
}

#[test]
fn a_dpo_run_carries_what_it_reads_and_nothing_else() {
    let dir = tempfile::TempDir::new().unwrap();
    let adapter = dir.path().join("c40k-0.5b/adapter");
    std::fs::create_dir_all(&adapter).unwrap();
    std::fs::write(adapter.join("adapter_config.json"), "{}").unwrap();
    std::fs::write(adapter.join("adapter_model.safetensors"), "weights").unwrap();
    std::fs::write(adapter.join("tokenizer.json"), "big").unwrap(); // the base has the tokenizer
    std::fs::write(dir.path().join("t.jsonl"), "{\"chosen\": \"a\"}\n").unwrap();
    std::fs::write(dir.path().join("v.jsonl"), "{\"chosen\": \"b\"}\n").unwrap();
    let out = dir.path().join("bundle");
    let spec = |name: &str, extra: &str| {
        format!(
            "{name}=0.5b:{}:{}:{}{extra}",
            adapter.display(),
            dir.path().join("t.jsonl").display(),
            dir.path().join("v.jsonl").display()
        )
    };
    let printed = dpo(
        &out,
        3,
        &[spec("b1", ":--beta 0.3 --lr 1e-5"), spec("b2", "")],
    )
    .unwrap();
    assert_eq!(printed.len(), 2);
    let runs = read_json(&out.join("runs.json"));
    assert_eq!(runs["per_gpu"], 3);
    assert_eq!(
        runs["runs"][0]["args"],
        json!(["--dtype", "fp16", "--beta", "0.3", "--lr", "1e-5"])
    );
    assert_eq!(runs["runs"][1]["args"], json!(["--dtype", "fp16"]));
    assert_eq!(runs["runs"][0]["sft_sha256"], sha256_of(b"weights"));
    assert_eq!(
        std::fs::read_to_string(out.join("b1.train.jsonl")).unwrap(),
        "{\"chosen\": \"a\"}\n"
    );
    let mut kept: Vec<String> =
        std::fs::read_dir(out.join(format!("sft-{}", &sha256_of(b"weights")[..12])))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
    kept.sort();
    assert_eq!(kept, ["adapter_config.json", "adapter_model.safetensors"]);
    for file in [
        "train.py",
        "train_dpo.py",
        "requirements.txt",
        "dataset-metadata.json",
    ] {
        assert!(out.join(file).exists(), "{file}");
    }
    assert!(
        !std::fs::read_to_string(out.join("requirements.txt"))
            .unwrap()
            .contains("torch")
    );
    let err = dpo(
        &dir.path().join("x"),
        1,
        &[format!(
            "x=7b:{}:{}:{}",
            adapter.display(),
            dir.path().display(),
            dir.path().display()
        )],
    )
    .unwrap_err();
    assert!(format!("{err:#}").contains("no base model"), "{err:#}");
}

#[test]
fn two_sft_adapters_with_the_same_directory_name_do_not_overwrite_each_other() {
    let dir = tempfile::TempDir::new().unwrap();
    let adapter = |version: &str, weights: &str| {
        let path = dir.path().join(version).join("x/adapter");
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("adapter_config.json"), "{}").unwrap();
        std::fs::write(path.join("adapter_model.safetensors"), weights).unwrap();
        path
    };
    let (v1, v2) = (adapter("v1", "first"), adapter("v2", "second"));
    std::fs::write(dir.path().join("t.jsonl"), "{}\n").unwrap();
    let spec = |name: &str, adapter: &Path| {
        let t = dir.path().join("t.jsonl");
        format!(
            "{name}=0.5b:{}:{}:{}",
            adapter.display(),
            t.display(),
            t.display()
        )
    };
    let out = dir.path().join("bundle");
    dpo(&out, 1, &[spec("a", &v1), spec("b", &v2)]).unwrap();
    let runs = read_json(&out.join("runs.json"));
    let (a, b) = (&runs["runs"][0], &runs["runs"][1]);
    assert_ne!(a["sft"], b["sft"]);
    for (run, weights) in [(a, "first"), (b, "second")] {
        let kept = std::fs::read(
            out.join(run["sft"].as_str().unwrap())
                .join("adapter_model.safetensors"),
        )
        .unwrap();
        assert_eq!(kept, weights.as_bytes());
        assert_eq!(run["sft_sha256"], sha256_of(weights.as_bytes()));
    }
}

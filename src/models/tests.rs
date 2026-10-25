use std::path::{Path, PathBuf};

use super::*;
use crate::prompt::PromptRules;

#[test]
fn prompt_rules_follow_the_checked_file() {
    let dir = tempfile::TempDir::new().unwrap();
    let model = &REGISTRY[0];
    let file = dir.path().join(model.gguf.file);
    // Sparse: the size is checked, not the bytes.
    std::fs::File::create(&file)
        .unwrap()
        .set_len(model.gguf.size)
        .unwrap();
    std::fs::write(sidecar(&file), format!("{}  x\n", model.gguf.sha256)).unwrap();
    assert_eq!(prompt_rules(&file), model.rules);
    // Any other file gets every rule, whatever it is called.
    std::fs::write(sidecar(&file), "0000  x\n").unwrap();
    assert_eq!(prompt_rules(&file), PromptRules::Full);
    assert_eq!(
        prompt_rules(&dir.path().join("missing.gguf")),
        PromptRules::Full
    );
}

#[test]
fn no_model_drops_the_rules_it_was_not_trained_without() {
    // Base models fall apart without them (training H6), and v0.1 was
    // trained with the full prompt. A model trained without them
    // changes this test on purpose.
    for model in REGISTRY {
        assert_eq!(model.rules, PromptRules::Full, "{}", model.id);
    }
}

#[test]
fn ids_follow_the_registry() {
    let ids: Vec<&str> = REGISTRY.iter().map(|m| m.id).collect();
    assert_eq!(ids, IDS);
    assert!(find(DEFAULT).is_some());
    assert_eq!(find(DEFAULT).unwrap().status, Status::Validated);
    assert_eq!(find("gpt-5"), None);
}

#[test]
fn every_model_is_pinned() {
    for model in REGISTRY {
        let gguf = model.gguf;
        assert_eq!(
            gguf.revision.len(),
            40,
            "{}: a commit, not a branch",
            model.id
        );
        assert!(gguf.revision.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(gguf.sha256.len(), 64, "{}", model.id);
        assert!(gguf.sha256.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(gguf.file.ends_with(".gguf"), "{}", model.id);
        assert!(gguf.size > 100_000_000, "{}", model.id);
        assert_eq!(
            model.license, "Apache-2.0",
            "{}: open licences only",
            model.id
        );
    }
}

#[test]
fn own_fine_tunes_follow_the_naming_rule() {
    let own: Vec<&Model> = REGISTRY
        .iter()
        .filter(|m| m.gguf.repo.starts_with(&format!("{OWN_ACCOUNT}/")))
        .collect();
    assert!(!own.is_empty());
    for model in &own {
        // smart-commit-<size>-v<version>, one repository per version.
        let rest = model.id.strip_prefix("smart-commit-").unwrap();
        let (size, version) = rest.split_once("-v").unwrap();
        assert!(size.ends_with('b'), "{}", model.id);
        assert!(
            version.split('.').all(|n| n.parse::<u32>().is_ok()),
            "{}",
            model.id
        );
        assert_eq!(model.gguf.repo, format!("{OWN_ACCOUNT}/{}", model.id));
        assert_eq!(model.gguf.file, format!("{}-q4_k_m.gguf", model.id));
    }
    for model in REGISTRY.iter().filter(|m| !own.contains(m)) {
        assert!(
            !model.id.starts_with("smart-commit-"),
            "{}: smart-commit-* names only the project's own fine-tunes",
            model.id
        );
    }
}

#[test]
fn the_default_model_is_the_one_the_export_script_makes() {
    let model = find(DEFAULT).unwrap();
    let export = include_str!("../../scripts/export-model.sh");
    assert!(export.contains(&format!("NAME={}\n", model.openvino.unwrap())));
}

#[test]
fn urls_point_at_the_pinned_revision() {
    let gguf = find("qwen2.5-coder-0.5b").unwrap().gguf;
    assert_eq!(
        gguf.url("https://huggingface.co/"),
        "https://huggingface.co/Qwen/Qwen2.5-Coder-0.5B-Instruct-GGUF/resolve/\
         ebb2015119c907b064c512bf053e945850b5875f/qwen2.5-coder-0.5b-instruct-q4_k_m.gguf"
    );
}

#[test]
fn sizes_read_well() {
    assert_eq!(size(491_400_064), "491 MB");
    assert_eq!(size(1_117_320_768), "1.1 GB");
    assert_eq!(size(4_683_073_536), "4.7 GB");
}

#[test]
fn hashes_files() {
    let dir = tempfile::TempDir::new().unwrap();
    let file = dir.path().join("f");
    std::fs::write(&file, "abc").unwrap();
    assert_eq!(
        sha256_file(&file).unwrap(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn recognises_gguf_files() {
    let dir = tempfile::TempDir::new().unwrap();
    let good = dir.path().join("m.gguf");
    std::fs::write(&good, b"GGUF\x03\x00\x00\x00rest").unwrap();
    assert!(check_gguf(&good).is_ok());
    let bad = dir.path().join("m.bin");
    std::fs::write(&bad, b"PK\x03\x04").unwrap();
    assert!(matches!(check_gguf(&bad), Err(ModelError::NotGguf(_))));
    assert!(matches!(
        check_gguf(&dir.path().join("none")),
        Err(ModelError::Io(_))
    ));
}

/// A registry entry for a file served from `dir` through `file://`.
fn local_model(dir: &Path, content: &[u8]) -> &'static Model {
    let repo_dir = dir.join("org/repo/resolve/0123456789012345678901234567890123456789");
    std::fs::create_dir_all(&repo_dir).unwrap();
    std::fs::write(repo_dir.join("m.gguf"), content).unwrap();
    let sha = sha256_file(&repo_dir.join("m.gguf")).unwrap();
    Box::leak(Box::new(Model {
        gguf: Gguf {
            repo: "org/repo",
            revision: "0123456789012345678901234567890123456789",
            file: "m.gguf",
            size: content.len() as u64,
            sha256: Box::leak(sha.into_boxed_str()),
        },
        ..REGISTRY[0]
    }))
}

#[test]
fn installs_checks_and_skips_what_is_there() {
    let served = tempfile::TempDir::new().unwrap();
    let models = tempfile::TempDir::new().unwrap();
    let model = local_model(served.path(), b"GGUF model weights");
    let base = format!("file://{}", served.path().display());

    let mut seen = Vec::new();
    let file = install_gguf(model, models.path(), &base, |n| seen.push(n)).unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), b"GGUF model weights");
    assert_eq!(seen.last(), Some(&18));
    assert!(is_installed(&model.gguf, &file));
    assert!(!models.path().join("m.gguf.partial").exists());

    // Already there: nothing is downloaded.
    std::fs::remove_dir_all(served.path().join("org")).unwrap();
    assert_eq!(
        install_gguf(model, models.path(), &base, |_| {}).unwrap(),
        file
    );
}

#[test]
fn a_different_file_is_refused_and_removed() {
    let served = tempfile::TempDir::new().unwrap();
    let models = tempfile::TempDir::new().unwrap();
    let model = local_model(served.path(), b"GGUF the pinned one");
    let path = served
        .path()
        .join("org/repo/resolve/0123456789012345678901234567890123456789/m.gguf");
    std::fs::write(path, b"GGUF something else").unwrap();
    let base = format!("file://{}", served.path().display());
    let err = install_gguf(model, models.path(), &base, |_| {}).unwrap_err();
    assert!(matches!(err, ModelError::Checksum { .. }), "{err}");
    assert!(
        err.to_string().contains("is not the file pinned for"),
        "{err}"
    );
    assert!(!models.path().join("m.gguf").exists());
    assert!(!models.path().join("m.gguf.partial").exists());
}

#[test]
fn download_failures_name_the_url() {
    let models = tempfile::TempDir::new().unwrap();
    let model = local_model(models.path(), b"GGUF");
    let err = install_gguf(
        model,
        &models.path().join("out"),
        "file:///nonexistent",
        |_| {},
    )
    .unwrap_err();
    assert!(matches!(err, ModelError::Download { .. }), "{err}");
    assert!(
        err.to_string()
            .contains("file:///nonexistent/org/repo/resolve/"),
        "{err}"
    );
}

#[test]
fn local_models_are_remembered_most_recent_first() {
    let dir = tempfile::TempDir::new().unwrap();
    let store = dir.path().join("state/local-models");
    assert_eq!(local_models(&store), Vec::new(), "nothing recorded yet");

    let a = dir.path().join("a-q4_k_m.gguf");
    let b = dir.path().join("b.gguf");
    let ov = dir.path().join("ov");
    std::fs::write(&a, "GGUF").unwrap();
    std::fs::write(&b, "GGUF").unwrap();
    std::fs::create_dir(&ov).unwrap();
    remember_local(&store, &a).unwrap();
    remember_local(&store, &b).unwrap();
    remember_local(&store, &ov).unwrap();
    remember_local(&store, &a).unwrap();
    let paths: Vec<PathBuf> = local_models(&store).into_iter().map(|l| l.path).collect();
    assert_eq!(paths, [a.clone(), ov.clone(), b.clone()], "no duplicates");

    // A path that no longer exists is not listed, and dropped on the
    // next write.
    std::fs::remove_file(&b).unwrap();
    assert_eq!(local_models(&store).len(), 2);
    remember_local(&store, &ov).unwrap();
    assert_eq!(
        std::fs::read_to_string(&store).unwrap(),
        format!("{}\n{}\n", ov.display(), a.display())
    );
}

#[test]
fn local_models_keep_the_most_recent_ones() {
    let dir = tempfile::TempDir::new().unwrap();
    let store = dir.path().join("local-models");
    for i in 0..LOCAL_LIMIT + 2 {
        let file = dir.path().join(format!("m{i}.gguf"));
        std::fs::write(&file, "GGUF").unwrap();
        remember_local(&store, &file).unwrap();
    }
    let local = local_models(&store);
    assert_eq!(local.len(), LOCAL_LIMIT);
    assert_eq!(local[0].name(), format!("m{}", LOCAL_LIMIT + 1));
    assert_eq!(local[LOCAL_LIMIT - 1].name(), "m2");
}

#[test]
fn local_models_say_what_they_are() {
    let dir = tempfile::TempDir::new().unwrap();
    let file = Local {
        path: dir.path().join("smart-commit-0.5b-v0.2-q4_k_m.gguf"),
    };
    std::fs::write(&file.path, "GGUF1234").unwrap();
    assert_eq!(file.name(), "smart-commit-0.5b-v0.2-q4_k_m");
    assert_eq!(file.key(), "model.gguf");
    assert_eq!(file.size(), Some(8));
    assert_eq!(file.devices(), "vulkan and cpu");

    let ov = Local {
        path: dir.path().join("model.gguf"),
    };
    std::fs::create_dir(&ov.path).unwrap();
    assert_eq!(ov.name(), "model.gguf", "a directory keeps its name");
    assert_eq!(ov.key(), "model.dir");
    assert_eq!(ov.size(), None);
    assert_eq!(ov.devices(), "npu, intel-gpu and openvino-cpu");
}

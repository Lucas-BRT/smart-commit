use super::*;

use std::path::PathBuf;

use crate::backend::GenerationConfig;
use crate::backend::device::{self, Target};
use crate::backend::genai::Device;
use crate::backend::llamacpp::LlamaDevice;
use crate::models;
use crate::paths;
use crate::prompt::PromptOptions;
use crate::style::Convention;

fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    move |name| {
        vars.iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| (*v).to_owned())
    }
}

const HOME: &[(&str, &str)] = &[("HOME", "/home/me")];

fn resolve(
    global: Option<&str>,
    repo: Option<&str>,
    vars: &[(&str, &str)],
    flags: Vec<(&'static str, String)>,
) -> Result<Resolved, ConfigError> {
    let dir = tempfile::TempDir::new().unwrap();
    let write = |name: &str, text: Option<&str>| {
        text.map(|t| {
            let path = dir.path().join(name);
            std::fs::write(&path, t).unwrap();
            path
        })
    };
    let lookup = env(vars);
    Resolved::resolve(&Sources {
        global: write("global.toml", global),
        repo: write("repo.toml", repo),
        env: &lookup,
        flags,
    })
}

#[test]
fn defaults_match_the_built_in_options() {
    let settings = resolve(None, None, HOME, vec![]).unwrap().settings();
    assert_eq!(settings.prompt_options(), PromptOptions::default());
    assert_eq!(settings.generation_config(), GenerationConfig::default());
    assert_eq!(settings.backend, BackendKind::Auto);
    assert_eq!(settings.device, DeviceSetting::Auto);
    assert_eq!(settings.model.id, models::DEFAULT);
    assert_eq!(settings.gguf, paths::default_gguf_file(env(HOME)));
    assert_eq!(
        settings.llama_server,
        paths::default_llama_server(env(HOME))
    );
    assert_eq!(settings.history_depth, crate::context::HISTORY_DEPTH);
    assert_eq!(settings.model_dir, paths::default_model_dir(env(HOME)));
}

#[test]
fn the_model_is_kept_loaded_for_ten_minutes_unless_set() {
    let settings = resolve(None, None, HOME, vec![]).unwrap().settings();
    assert_eq!(settings.keep_loaded, std::time::Duration::from_secs(600));
    let off = [("HOME", "/home/me"), ("SMART_COMMIT_KEEP_LOADED", "0")];
    let settings = resolve(None, None, &off, vec![]).unwrap().settings();
    assert!(settings.keep_loaded.is_zero());
}

#[test]
fn layers_override_in_order() {
    let global = "backend = \"mock\"\n[model]\ndevice = \"gpu\"\n[style]\ndefault_convention = \"free\"\nhistory_depth = 10\n";
    let repo = "[style]\ndefault_convention = \"gitmoji\"\n";
    let vars = [("HOME", "/home/me"), ("SMART_COMMIT_DEVICE", "cpu")];
    let resolved = resolve(
        Some(global),
        Some(repo),
        &vars,
        vec![("backend", "openvino".into())],
    )
    .unwrap();
    let origin = |name| resolved.get(name).unwrap().1.clone();
    assert!(matches!(origin("style.history_depth"), Origin::Global(_)));
    assert!(matches!(
        origin("style.default_convention"),
        Origin::Repo(_)
    ));
    assert_eq!(origin("model.device"), Origin::Env("SMART_COMMIT_DEVICE"));
    assert_eq!(origin("backend"), Origin::Flag);
    assert_eq!(origin("style.convention"), Origin::Default);

    let settings = resolved.settings();
    assert_eq!(settings.backend, BackendKind::Openvino);
    assert_eq!(settings.device, DeviceSetting::Fixed(Target::Cpu));
    assert_eq!(settings.default_convention, Convention::Gitmoji);
    assert_eq!(settings.history_depth, 10);
}

#[test]
fn keys_earlier_versions_had_are_read_and_ignored() {
    // `style.language` asked for Portuguese; a file that still has it must keep working.
    let text =
        "[style]\nlanguage = \"portuguese\"\ndefault_language = \"english\"\nhistory_depth = 7\n";
    let path = PathBuf::from("config.toml");
    let values = parse_file(text, &path, &Origin::Global(path.clone()), None).unwrap();
    let names: Vec<_> = values.iter().map(|(key, _)| key.name).collect();
    assert_eq!(names, ["style.history_depth"]);
    // They are not keys: setting or reading one is still an unknown key.
    assert!(matches!(
        key("style.language"),
        Err(ConfigError::UnknownKey { .. })
    ));
}

#[test]
fn unknown_keys_are_errors() {
    let err = resolve(Some("[model]\ndevise = \"cpu\"\n"), None, HOME, vec![]).unwrap_err();
    assert!(matches!(&err, ConfigError::UnknownKey { key, .. } if key == "model.devise"));
    assert!(
        err.to_string()
            .contains("known keys: backend, model.device")
    );
    assert!(resolve(Some("[style.extra]\nx = 1\n"), None, HOME, vec![]).is_err());
}

#[test]
fn invalid_values_are_errors() {
    for (text, problem) in [
        (
            "[model]\ndevice = \"tpu\"\n",
            "expected one of auto, npu, intel-gpu, vulkan, cpu, openvino-cpu, got \"tpu\"",
        ),
        ("[model]\ndevice = 3\n", "expected a string, got 3"),
        (
            "[style]\nhistory_depth = \"30\"\n",
            "expected a whole number, got \"30\"",
        ),
        (
            "[style]\nhistory_depth = 0\n",
            "expected a whole number from 1 to 500, got \"0\"",
        ),
        (
            "[model]\ndir = \"models/here\"\n",
            "expected an absolute path or one starting with ~/",
        ),
        ("model = 1\n", "unknown key `model`"),
    ] {
        let err = resolve(Some(text), None, HOME, vec![]).unwrap_err();
        assert!(err.to_string().contains(problem), "{text:?}: {err}");
    }
    let err = resolve(
        None,
        None,
        &[("HOME", "/h"), ("SMART_COMMIT_BACKEND", "gpt")],
        vec![],
    )
    .unwrap_err();
    assert!(
        err.to_string()
            .contains("environment variable SMART_COMMIT_BACKEND"),
        "{err}"
    );
}

#[test]
fn syntax_errors_name_the_file() {
    let err = resolve(Some("[model\n"), None, HOME, vec![]).unwrap_err();
    assert!(matches!(err, ConfigError::Syntax { path, .. } if path.ends_with("global.toml")));
}

#[test]
fn machine_keys_are_refused_in_the_repository_file() {
    let err = resolve(None, Some("[model]\ndir = \"/tmp/evil\"\n"), HOME, vec![]).unwrap_err();
    assert!(
        matches!(&err, ConfigError::UserOnly { key, .. } if key == "model.dir"),
        "{err}"
    );
    assert!(resolve(None, Some("backend = \"mock\"\n"), HOME, vec![]).is_err());
}

#[test]
fn paths_expand_home() {
    let resolved = resolve(Some("[model]\ndir = \"~/models/q\"\n"), None, HOME, vec![]).unwrap();
    assert_eq!(
        resolved.settings().model_dir,
        Some(PathBuf::from("/home/me/models/q"))
    );
}

#[test]
fn empty_environment_variables_are_ignored() {
    let vars = [("HOME", "/h"), ("SMART_COMMIT_DEVICE", "")];
    let resolved = resolve(None, None, &vars, vec![]).unwrap();
    assert_eq!(resolved.get("model.device").unwrap().1, Origin::Default);
}

#[test]
fn without_home_paths_have_no_default() {
    let settings = resolve(None, None, &[], vec![]).unwrap().settings();
    assert_eq!(settings.model_dir, None);
    assert!(
        settings
            .genai_options(Device::Npu)
            .unwrap_err()
            .contains("model.dir")
    );
    assert!(
        settings
            .llamacpp_options(Target::Cpu, false)
            .unwrap_err()
            .contains("model.llama_server")
    );
}

#[test]
fn threads_batch_is_one_per_core_unless_set() {
    let options = |global: Option<&str>| {
        resolve(global, None, HOME, vec![])
            .unwrap()
            .settings()
            .llamacpp_options(Target::Cpu, false)
            .unwrap()
            .threads_batch
    };
    assert!(options(None) >= 1);
    assert_eq!(options(Some("[model]\nthreads_batch = 3\n")), 3);
}

#[test]
fn llamacpp_runs_on_the_cpu_or_the_configured_vulkan_gpu() {
    let global = "[model]\nvulkan_device = 2\n";
    let settings = resolve(Some(global), None, HOME, vec![])
        .unwrap()
        .settings();
    let on = |target| settings.llamacpp_options(target, false).unwrap().device;
    assert_eq!(on(Target::Cpu), LlamaDevice::Cpu);
    assert_eq!(on(Target::Vulkan), LlamaDevice::Vulkan(2));
}

#[test]
fn fast_load_follows_the_benchmark_unless_set() {
    let settings = |vars: &[(&str, &str)]| {
        resolve(None, None, vars, vec![])
            .unwrap()
            .settings()
            .fast_load
    };
    assert_eq!(settings(&[]), FastLoad::Auto);
    assert!(FastLoad::Auto.enabled(|| true));
    assert!(!FastLoad::Auto.enabled(|| false));

    let on = settings(&[("SMART_COMMIT_FAST_LOAD", "on")]);
    assert_eq!(on, FastLoad::On);
    assert!(on.enabled(|| panic!("the benchmark is not asked")));
    let off = settings(&[("SMART_COMMIT_FAST_LOAD", "off")]);
    assert!(!off.enabled(|| panic!("the benchmark is not asked")));
    assert!(resolve(None, None, &[("SMART_COMMIT_FAST_LOAD", "yes")], vec![]).is_err());

    let repo = "[model]\nfast_load = \"on\"\n";
    assert!(
        resolve(None, Some(repo), &[], vec![]).is_err(),
        "about the machine"
    );
    let options = resolve(None, None, HOME, vec![])
        .unwrap()
        .settings()
        .llamacpp_options(Target::Cpu, true)
        .unwrap();
    assert!(options.fast_load);
}

#[test]
fn every_device_name_is_accepted() {
    for target in device::DEFAULT_ORDER {
        let vars = [("SMART_COMMIT_DEVICE", target.name())];
        let settings = resolve(None, None, &vars, vec![]).unwrap().settings();
        assert_eq!(settings.device, DeviceSetting::Fixed(target));
    }
    let vars = [("SMART_COMMIT_DEVICE", "tpu")];
    assert!(resolve(None, None, &vars, vec![]).is_err());
}

#[test]
fn the_old_gpu_name_still_works_with_a_warning() {
    let global = "[model]\ndevice = \"gpu\"\n";
    let resolved = resolve(Some(global), None, HOME, vec![]).unwrap();
    assert_eq!(
        resolved.settings().device,
        DeviceSetting::Fixed(Target::IntelGpu)
    );
    let warnings = resolved.renamed_values();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].starts_with("model.device = gpu (global ")
            && warnings[0].ends_with(
                "is the old name of intel-gpu; \
                     `smart-commit config set model.device intel-gpu` renames it"
            ),
        "{}",
        warnings[0]
    );
    let current = resolve(None, None, &[("SMART_COMMIT_DEVICE", "intel-gpu")], vec![]);
    assert!(current.unwrap().renamed_values().is_empty());
}

#[test]
fn npu_chunk_must_divide_the_prompt_length() {
    let global = "[model]\nmax_prompt_tokens = 1536\nnpu_prefill_chunk = 1024\n";
    let settings = resolve(Some(global), None, HOME, vec![])
        .unwrap()
        .settings();
    let err = settings.genai_options(Device::Npu).unwrap_err();
    assert!(err.contains("must be a multiple"), "{err}");
    // Only the NPU cares; and a chunk at least as large is just static.
    assert!(settings.genai_options(Device::Gpu).is_ok());
    let global = "[model]\nmax_prompt_tokens = 512\nnpu_prefill_chunk = 1024\n";
    let settings = resolve(Some(global), None, HOME, vec![])
        .unwrap()
        .settings();
    assert!(settings.genai_options(Device::Npu).is_ok());
    assert_eq!(settings.npu.prefill_chunk, 1024);
    assert!(settings.npu.best_perf);
}

#[test]
fn every_key_is_documented() {
    let page = include_str!("../../../docs/configuration.md");
    for key in KEYS {
        assert!(
            page.contains(&format!("| `{}` |", key.name)),
            "{} is missing from docs/configuration.md",
            key.name
        );
        if let Some(var) = key.env {
            assert!(
                page.contains(var),
                "{var} is missing from docs/configuration.md"
            );
        }
    }
}

#[test]
fn set_and_unset_keep_comments() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("sub/config.toml");
    let origin = Origin::Global(path.clone());
    set_in_file(&path, &origin, "model.device", "cpu", None).unwrap();
    let with_comment = format!("# mine\n{}", std::fs::read_to_string(&path).unwrap());
    std::fs::write(&path, with_comment).unwrap();
    set_in_file(&path, &origin, "style.history_depth", "12", None).unwrap();
    set_in_file(&path, &origin, "backend", "mock", None).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    // Top-level keys must precede tables in TOML, so `backend` goes first;
    // the comment stays with the table it was above.
    assert!(text.contains("# mine\n[model]\n"), "{text}");
    let values = parse_file(&text, &path, &origin, None).unwrap();
    let names: Vec<_> = values
        .iter()
        .map(|(k, v)| (k.name, v.to_string()))
        .collect();
    assert!(names.contains(&("model.device", "cpu".into())));
    assert!(names.contains(&("style.history_depth", "12".into())));
    assert!(names.contains(&("backend", "mock".into())));

    assert!(unset_in_file(&path, "model.device").unwrap());
    assert!(!unset_in_file(&path, "model.device").unwrap());
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("# mine\n[model]\n") && !text.contains("device"),
        "{text}"
    );
}

#[test]
fn set_aside_invalid_removes_only_values_this_version_cannot_read() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "# mine\n[model]\ndevice = \"vulkan\"\nname = \"smart-commit-9b-v9\"\n\
             max_new_tokens = 5000\n[style]\ndefault_convention = \"free\"\n",
    )
    .unwrap();
    let removed = set_aside_invalid(&path, None).unwrap();
    let messages: Vec<_> = removed.iter().map(ToString::to_string).collect();
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(messages[0].contains("`model.name`") && messages[0].contains("\"smart-commit-9b-v9\""));
    assert!(messages[1].contains("`model.max_new_tokens`"));
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        text,
        "# mine\n[model]\ndevice = \"vulkan\"\n[style]\ndefault_convention = \"free\"\n"
    );
    assert!(set_aside_invalid(&path, None).unwrap().is_empty());
}

#[test]
fn set_aside_invalid_leaves_other_errors_and_missing_files() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    assert!(set_aside_invalid(&path, None).unwrap().is_empty());
    assert!(!path.exists());
    for text in ["[model]\ndevise = \"cpu\"\n", "[model\n"] {
        std::fs::write(&path, text).unwrap();
        assert!(set_aside_invalid(&path, None).is_err(), "{text}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    }
}

#[test]
fn a_write_that_fails_leaves_the_file_as_it_was() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    let origin = Origin::Global(path.clone());
    set_in_file(&path, &origin, "model.device", "cpu", None).unwrap();
    let before = std::fs::read_to_string(&path).unwrap();
    // The place the new text is written first cannot be a file.
    std::fs::create_dir(dir.path().join("config.toml.tmp")).unwrap();
    assert!(set_in_file(&path, &origin, "style.history_depth", "12", None).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
}

#[test]
fn a_saved_file_leaves_nothing_beside_it() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    let origin = Origin::Global(path.clone());
    set_in_file(&path, &origin, "model.device", "cpu", None).unwrap();
    set_in_file(&path, &origin, "style.history_depth", "12", None).unwrap();
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, ["config.toml"]);
}

#[test]
fn a_symlinked_config_is_written_through_not_replaced() {
    let dir = tempfile::TempDir::new().unwrap();
    let real = dir.path().join("dotfiles/config.toml");
    std::fs::create_dir_all(real.parent().unwrap()).unwrap();
    std::fs::write(&real, "").unwrap();
    let link = dir.path().join("config.toml");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let origin = Origin::Global(link.clone());
    set_in_file(&link, &origin, "model.device", "cpu", None).unwrap();
    assert!(std::fs::symlink_metadata(&link).unwrap().is_symlink());
    assert!(std::fs::read_to_string(&real).unwrap().contains("device"));
}

#[test]
fn set_uncomments_the_template_line() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    let origin = Origin::Global(path.clone());
    let defaults = resolve(None, None, HOME, vec![]).unwrap();
    std::fs::write(&path, template(false, &defaults)).unwrap();

    set_in_file(&path, &origin, "model.device", "cpu", None).unwrap();
    set_in_file(&path, &origin, "backend", "mock", None).unwrap();
    set_in_file(&path, &origin, "style.history_depth", "12", None).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("# Environment: SMART_COMMIT_DEVICE.\ndevice = \"cpu\"\n"),
        "{text}"
    );
    assert!(text.contains("# Environment: SMART_COMMIT_BACKEND.\nbackend = \"mock\"\n"));
    assert!(text.contains("history_depth = 12\n"));
    assert!(
        !text.contains("# device = "),
        "the commented line was replaced"
    );
    // `max_prompt_tokens` has its own template line; it must stay put.
    assert!(text.contains("# max_prompt_tokens = 1024"));

    let values = parse_file(&text, &path, &origin, None).unwrap();
    assert_eq!(values.len(), 3);
    // Changing it again edits the now real key; an old name is written
    // under its new one.
    set_in_file(&path, &origin, "model.device", "gpu", None).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("SMART_COMMIT_DEVICE.\ndevice = \"intel-gpu\""),
        "{text}"
    );
}

#[test]
fn set_validates_before_writing() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    let global = Origin::Global(path.clone());
    assert!(set_in_file(&path, &global, "model.device", "tpu", None).is_err());
    assert!(set_in_file(&path, &global, "nope", "1", None).is_err());
    let repo = Origin::Repo(path.clone());
    assert!(matches!(
        set_in_file(&path, &repo, "model.dir", "/m", None),
        Err(ConfigError::UserOnly { .. })
    ));
    assert!(!path.exists(), "nothing was written");
}

#[test]
fn templates_parse_to_the_defaults_and_list_every_key() {
    let defaults = resolve(None, None, HOME, vec![]).unwrap();
    for for_repo in [false, true] {
        let template = template(for_repo, &defaults);
        let path = PathBuf::from("/t.toml");
        let origin = if for_repo {
            Origin::Repo(path.clone())
        } else {
            Origin::Global(path.clone())
        };
        // All commented out: no values.
        assert_eq!(parse_file(&template, &path, &origin, None).unwrap(), vec![]);
        // Uncommented, every line is valid and equals the default.
        let uncommented: String = template
            .lines()
            .map(|l| match l.strip_prefix("# ") {
                Some(rest) if rest.contains(" = ") && !rest.starts_with(['#', ' ']) => rest,
                _ => l,
            })
            .collect::<Vec<_>>()
            .join("\n");
        let values = parse_file(&uncommented, &path, &origin, Some("/home/me")).unwrap();
        let expected = KEYS
            .iter()
            .filter(|k| !(for_repo && k.scope == Scope::User))
            .count();
        assert_eq!(values.len(), expected, "{uncommented}");
        for (key, value) in values {
            assert_eq!(&value, &defaults.get(key.name).unwrap().0, "{}", key.name);
        }
    }
}

#[test]
fn the_type_is_never_asked_unless_set_and_below_half() {
    let settings = resolve(None, None, HOME, vec![]).unwrap().settings();
    assert_eq!(settings.ask_type, AskType::Off);
    assert_eq!(settings.ask_type_below, 50);
    assert!((settings.ask_type_below() - 0.5).abs() < f64::EPSILON);
}

#[test]
fn asking_for_the_type_is_set_by_file_or_environment() {
    let file = "[model]\nask_type = \"low_confidence\"\nask_type_below = 65\n";
    let settings = resolve(Some(file), None, HOME, vec![]).unwrap().settings();
    assert_eq!(settings.ask_type, AskType::LowConfidence);
    assert!((settings.ask_type_below() - 0.65).abs() < 1e-9);

    let vars = [
        ("HOME", "/home/me"),
        ("SMART_COMMIT_ASK_TYPE", "low_confidence"),
        ("SMART_COMMIT_ASK_TYPE_BELOW", "30"),
    ];
    let settings = resolve(Some("[model]\nask_type_below = 90\n"), None, &vars, vec![])
        .unwrap()
        .settings();
    assert_eq!(settings.ask_type, AskType::LowConfidence);
    assert_eq!(settings.ask_type_below, 30, "the environment wins");
}

#[test]
fn the_confidence_to_ask_below_is_a_percentage_between_the_extremes() {
    for value in ["0", "100"] {
        let file = format!("[model]\nask_type_below = {value}\n");
        let err = resolve(Some(&file), None, HOME, vec![]).unwrap_err();
        assert!(
            err.to_string().contains("a whole number from 1 to 99"),
            "{value}: {err}"
        );
    }
    let err = resolve(Some("[model]\nask_type = \"always\"\n"), None, HOME, vec![]).unwrap_err();
    assert!(
        err.to_string()
            .contains("expected one of off, low_confidence"),
        "{err}"
    );
}

#[test]
fn asking_for_the_type_is_a_personal_setting_not_the_repositorys() {
    let err = resolve(
        None,
        Some("[model]\nask_type = \"low_confidence\"\n"),
        HOME,
        vec![],
    )
    .unwrap_err();
    assert!(
        matches!(&err, ConfigError::UserOnly { key, .. } if key == "model.ask_type"),
        "{err}"
    );
}

#[test]
fn a_kept_model_may_take_a_sixth_of_the_ram_unless_set() {
    let settings = resolve(None, None, HOME, vec![]).unwrap().settings();
    assert_eq!(settings.keep_loaded_max_ram, 15);
    let file = "[model]\nkeep_loaded_max_ram = 40\n";
    let settings = resolve(Some(file), None, HOME, vec![]).unwrap().settings();
    assert_eq!(settings.keep_loaded_max_ram, 40);
    let vars = [
        ("HOME", "/home/me"),
        ("SMART_COMMIT_KEEP_LOADED_MAX_RAM", "25"),
    ];
    let settings = resolve(Some(file), None, &vars, vec![]).unwrap().settings();
    assert_eq!(settings.keep_loaded_max_ram, 25, "the environment wins");
}

#[test]
fn the_share_of_ram_is_a_percentage_from_one_to_a_hundred() {
    for value in ["0", "101"] {
        let file = format!("[model]\nkeep_loaded_max_ram = {value}\n");
        let err = resolve(Some(&file), None, HOME, vec![]).unwrap_err();
        assert!(
            err.to_string().contains("a whole number from 1 to 100"),
            "{value}: {err}"
        );
    }
    let repo = "[model]\nkeep_loaded_max_ram = 90\n";
    let err = resolve(None, Some(repo), HOME, vec![]).unwrap_err();
    assert!(
        matches!(&err, ConfigError::UserOnly { key, .. } if key == "model.keep_loaded_max_ram"),
        "{err}"
    );
}

#[test]
fn only_a_keep_loaded_someone_set_counts_as_asked_for() {
    let settings = resolve(None, None, HOME, vec![]).unwrap().settings();
    assert!(!settings.keep_loaded_explicit);
    assert_eq!(settings.keep_loaded, std::time::Duration::from_secs(600));
    let file = "[model]\nkeep_loaded = 10\n";
    let settings = resolve(Some(file), None, HOME, vec![]).unwrap().settings();
    assert!(settings.keep_loaded_explicit, "even to the default's value");
    let vars = [("HOME", "/home/me"), ("SMART_COMMIT_KEEP_LOADED", "0")];
    let settings = resolve(None, None, &vars, vec![]).unwrap().settings();
    assert!(settings.keep_loaded_explicit);
    assert!(settings.keep_loaded.is_zero());
}

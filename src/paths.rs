//! Where smart-commit keeps models and caches, following the XDG base
//! directory spec.
//!
//! Functions take the environment as a lookup so they can be tested
//! without touching the process environment.

use std::path::PathBuf;

/// `$XDG_DATA_HOME/smart-commit`: the models, llama.cpp and the benchmark
/// (and, with install.sh's default prefix, the release's own files).
pub fn data_dir(env: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    Some(xdg(&env, "XDG_DATA_HOME", ".local/share")?.join("smart-commit"))
}

/// `$XDG_CACHE_HOME/smart-commit`, every cache of smart-commit.
pub fn cache_root(env: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    Some(xdg(&env, "XDG_CACHE_HOME", ".cache")?.join("smart-commit"))
}

/// `$XDG_CONFIG_HOME/smart-commit`, the global configuration's directory.
pub fn config_dir(env: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    Some(xdg(&env, "XDG_CONFIG_HOME", ".config")?.join("smart-commit"))
}

/// `$XDG_DATA_HOME/smart-commit/models`, where models are installed.
pub fn default_models_dir(env: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    Some(xdg(&env, "XDG_DATA_HOME", ".local/share")?.join("smart-commit/models"))
}

fn default_model() -> &'static crate::models::Model {
    crate::models::find(crate::models::DEFAULT).expect("the default model is registered")
}

/// The default model's OpenVINO directory, in [`default_models_dir`].
pub fn default_model_dir(env: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    Some(default_models_dir(env)?.join(default_model().openvino?))
}

/// The default model's GGUF file, in [`default_models_dir`].
pub fn default_gguf_file(env: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    Some(default_models_dir(env)?.join(default_model().gguf.file))
}

/// `$XDG_DATA_HOME/smart-commit/llama.cpp/llama-<build>/llama-server`, as
/// `smart-commit setup` installs it.
pub fn default_llama_server(env: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    Some(
        xdg(&env, "XDG_DATA_HOME", ".local/share")?
            .join("smart-commit/llama.cpp")
            .join(format!(
                "llama-{}",
                crate::backend::llamacpp::LLAMA_CPP_BUILD
            ))
            .join("llama-server"),
    )
}

/// `$XDG_DATA_HOME/smart-commit/bench.json`, the last `smart-commit bench`.
pub fn default_bench_file(env: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    Some(xdg(&env, "XDG_DATA_HOME", ".local/share")?.join("smart-commit/bench.json"))
}

/// `$XDG_DATA_HOME/smart-commit/local-models`, the models from disk used
/// before, one path per line.
pub fn default_local_models_file(env: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    Some(xdg(&env, "XDG_DATA_HOME", ".local/share")?.join("smart-commit/local-models"))
}

/// `$XDG_CACHE_HOME/smart-commit/openvino`, where compiled NPU blobs go.
pub fn default_cache_dir(env: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    Some(xdg(&env, "XDG_CACHE_HOME", ".cache")?.join("smart-commit/openvino"))
}

/// `$XDG_CONFIG_HOME/smart-commit/config.toml`, the global config.
pub fn default_config_file(env: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    Some(xdg(&env, "XDG_CONFIG_HOME", ".config")?.join("smart-commit/config.toml"))
}

/// `$XDG_RUNTIME_DIR/smart-commit`, where models kept loaded listen
/// (`model.keep_loaded`): private to the user and emptied at logout. No
/// fallback: without it, models are not kept loaded.
pub fn runtime_dir(env: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let dir = env("XDG_RUNTIME_DIR").map(PathBuf::from)?;
    dir.is_absolute().then(|| dir.join("smart-commit"))
}

/// The XDG variable if set to an absolute path (relative ones are invalid
/// per the spec), else `$HOME/<fallback>`.
fn xdg(env: &impl Fn(&str) -> Option<String>, var: &str, fallback: &str) -> Option<PathBuf> {
    if let Some(dir) = env(var).map(PathBuf::from).filter(|p| p.is_absolute()) {
        return Some(dir);
    }
    let home = env("HOME").filter(|h| !h.is_empty())?;
    Some(PathBuf::from(home).join(fallback))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            vars.iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| (*v).to_owned())
        }
    }

    #[test]
    fn runtime_dir_needs_xdg_runtime_dir() {
        assert_eq!(
            runtime_dir(env(&[("XDG_RUNTIME_DIR", "/run/user/1000")])).unwrap(),
            PathBuf::from("/run/user/1000/smart-commit")
        );
        assert_eq!(runtime_dir(env(&[("HOME", "/home/me")])), None);
        assert_eq!(runtime_dir(env(&[("XDG_RUNTIME_DIR", "run")])), None);
    }

    #[test]
    fn falls_back_to_home() {
        let vars = [("HOME", "/home/me")];
        assert_eq!(
            default_model_dir(env(&vars)).unwrap(),
            PathBuf::from(
                "/home/me/.local/share/smart-commit/models/qwen2.5-coder-1.5b-instruct-int4-sym-g128"
            )
        );
        assert_eq!(
            default_cache_dir(env(&vars)).unwrap(),
            PathBuf::from("/home/me/.cache/smart-commit/openvino")
        );
        assert_eq!(
            default_config_file(env(&vars)).unwrap(),
            PathBuf::from("/home/me/.config/smart-commit/config.toml")
        );
        assert_eq!(
            default_gguf_file(env(&vars)).unwrap(),
            PathBuf::from(
                "/home/me/.local/share/smart-commit/models/qwen2.5-coder-1.5b-instruct-q4_k_m.gguf"
            )
        );
        assert_eq!(
            default_bench_file(env(&vars)).unwrap(),
            PathBuf::from("/home/me/.local/share/smart-commit/bench.json")
        );
        assert_eq!(
            default_local_models_file(env(&vars)).unwrap(),
            PathBuf::from("/home/me/.local/share/smart-commit/local-models")
        );
        assert_eq!(
            default_llama_server(env(&vars)).unwrap(),
            PathBuf::from("/home/me/.local/share/smart-commit/llama.cpp/llama-b11381/llama-server")
        );
    }

    #[test]
    fn honours_xdg_variables() {
        let vars = [
            ("HOME", "/home/me"),
            ("XDG_DATA_HOME", "/data"),
            ("XDG_CACHE_HOME", "/cache"),
            ("XDG_CONFIG_HOME", "/config"),
        ];
        assert_eq!(
            default_config_file(env(&vars)).unwrap(),
            PathBuf::from("/config/smart-commit/config.toml")
        );
        assert_eq!(
            default_models_dir(env(&vars)).unwrap(),
            PathBuf::from("/data/smart-commit/models")
        );
        assert_eq!(
            default_cache_dir(env(&vars)).unwrap(),
            PathBuf::from("/cache/smart-commit/openvino")
        );
    }

    #[test]
    fn relative_xdg_paths_are_ignored() {
        let vars = [("HOME", "/home/me"), ("XDG_CACHE_HOME", "cache")];
        assert_eq!(
            default_cache_dir(env(&vars)).unwrap(),
            PathBuf::from("/home/me/.cache/smart-commit/openvino")
        );
    }

    #[test]
    fn no_home_means_no_default() {
        assert_eq!(default_model_dir(env(&[])), None);
        assert_eq!(default_cache_dir(env(&[("HOME", "")])), None);
    }
}

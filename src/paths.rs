//! Where npu-commit keeps models and caches, following the XDG base
//! directory spec.
//!
//! Functions take the environment as a lookup so they can be tested
//! without touching the process environment.

use std::path::PathBuf;

/// The model `scripts/export-model.sh` exports.
pub const DEFAULT_MODEL: &str = "qwen2.5-coder-1.5b-instruct-int4-sym-g128";

/// `$XDG_DATA_HOME/npu-commit/models/<DEFAULT_MODEL>`.
pub fn default_model_dir(env: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    Some(
        xdg(&env, "XDG_DATA_HOME", ".local/share")?
            .join("npu-commit/models")
            .join(DEFAULT_MODEL),
    )
}

/// `$XDG_CACHE_HOME/npu-commit/openvino`, where compiled NPU blobs go.
pub fn default_cache_dir(env: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    Some(xdg(&env, "XDG_CACHE_HOME", ".cache")?.join("npu-commit/openvino"))
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
    fn falls_back_to_home() {
        let vars = [("HOME", "/home/me")];
        assert_eq!(
            default_model_dir(env(&vars)).unwrap(),
            PathBuf::from("/home/me/.local/share/npu-commit/models").join(DEFAULT_MODEL)
        );
        assert_eq!(
            default_cache_dir(env(&vars)).unwrap(),
            PathBuf::from("/home/me/.cache/npu-commit/openvino")
        );
    }

    #[test]
    fn honours_xdg_variables() {
        let vars = [
            ("HOME", "/home/me"),
            ("XDG_DATA_HOME", "/data"),
            ("XDG_CACHE_HOME", "/cache"),
        ];
        assert_eq!(
            default_model_dir(env(&vars)).unwrap(),
            PathBuf::from("/data/npu-commit/models").join(DEFAULT_MODEL)
        );
        assert_eq!(
            default_cache_dir(env(&vars)).unwrap(),
            PathBuf::from("/cache/npu-commit/openvino")
        );
    }

    #[test]
    fn relative_xdg_paths_are_ignored() {
        let vars = [("HOME", "/home/me"), ("XDG_CACHE_HOME", "cache")];
        assert_eq!(
            default_cache_dir(env(&vars)).unwrap(),
            PathBuf::from("/home/me/.cache/npu-commit/openvino")
        );
    }

    #[test]
    fn no_home_means_no_default() {
        assert_eq!(default_model_dir(env(&[])), None);
        assert_eq!(default_cache_dir(env(&[("HOME", "")])), None);
    }
}

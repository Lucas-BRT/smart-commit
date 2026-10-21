//! The models smart-commit recommends, and installing them.
//!
//! Each model is downloaded from Hugging Face, pinned to a revision and
//! checked against the SHA-256 of the file at that revision: other
//! publishers' models from their own repositories (never re-hosted), and the
//! project's fine-tunes from [`OWN_ACCOUNT`]. Any other GGUF file can be used
//! from disk instead (`model.gguf`), unverified.
//!
//! The OpenVINO devices (`npu`, `intel-gpu`, `openvino-cpu`) need the model
//! exported for OpenVINO, which only models with a pinned export have.

use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::prompt::PromptRules;

/// Where models are downloaded from.
pub const HUGGING_FACE: &str = "https://huggingface.co";

/// The Hugging Face account the project's own fine-tunes are published
/// under, one repository per model and version (AGENTS.md).
pub const OWN_ACCOUNT: &str = "LucasBRT";

/// The model used unless `model.name` says otherwise.
pub const DEFAULT: &str = "qwen2.5-coder-1.5b";

/// The ids of [`REGISTRY`], in its order (`model.name`'s choices).
pub const IDS: &[&str] = &[
    "qwen2.5-coder-1.5b",
    "qwen2.5-coder-0.5b",
    "qwen2.5-coder-7b",
    "smart-commit-1.5b-v0.1",
    "smart-commit-0.5b-v0.1",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Checked on the fixture prompts and the hardware tests.
    Validated,
    /// Loads and answers; its messages have not been evaluated.
    Experimental,
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Validated => "validated",
            Self::Experimental => "experimental",
        })
    }
}

/// A GGUF file in a Hugging Face repository, for the llama.cpp devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gguf {
    pub repo: &'static str,
    /// A commit of the repository, never a branch.
    pub revision: &'static str,
    pub file: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

impl Gguf {
    pub fn url(&self, base: &str) -> String {
        format!(
            "{}/{}/resolve/{}/{}",
            base.trim_end_matches('/'),
            self.repo,
            self.revision,
            self.file
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Model {
    pub id: &'static str,
    pub name: &'static str,
    pub license: &'static str,
    pub status: Status,
    pub summary: &'static str,
    pub gguf: Gguf,
    /// The directory `scripts/export-model.sh` writes, for the OpenVINO
    /// devices; `None` when the model has no pinned export yet.
    pub openvino: Option<&'static str>,
    /// The instructions the model needs: [`PromptRules::Omitted`] only for
    /// a model trained without them (training H6).
    pub rules: PromptRules,
}

pub const REGISTRY: &[Model] = &[
    Model {
        id: "qwen2.5-coder-1.5b",
        name: "Qwen2.5-Coder-1.5B-Instruct",
        license: "Apache-2.0",
        status: Status::Validated,
        summary: "the default: fast on every device, code-focused",
        gguf: Gguf {
            repo: "Qwen/Qwen2.5-Coder-1.5B-Instruct-GGUF",
            revision: "f86cb2c1fa58255f8052cc32aeede1b7482d4361",
            file: "qwen2.5-coder-1.5b-instruct-q4_k_m.gguf",
            size: 1_117_320_768,
            sha256: "cc324af070c2ecbfd324a30884d2f951a7ff756aba85cb811a6ec436933bb046",
        },
        openvino: Some("qwen2.5-coder-1.5b-instruct-int4-sym-g128"),
        rules: PromptRules::Full,
    },
    Model {
        id: "qwen2.5-coder-0.5b",
        name: "Qwen2.5-Coder-0.5B-Instruct",
        license: "Apache-2.0",
        status: Status::Experimental,
        summary: "the smallest: faster, for slow CPUs; weaker messages",
        gguf: Gguf {
            repo: "Qwen/Qwen2.5-Coder-0.5B-Instruct-GGUF",
            revision: "ebb2015119c907b064c512bf053e945850b5875f",
            file: "qwen2.5-coder-0.5b-instruct-q4_k_m.gguf",
            size: 491_400_064,
            sha256: "1d9614638d18024d0fbb36575a15f1302a3adf044df10345688ec4f6e1c4ff32",
        },
        openvino: None,
        rules: PromptRules::Full,
    },
    Model {
        id: "qwen2.5-coder-7b",
        name: "Qwen2.5-Coder-7B-Instruct",
        license: "Apache-2.0",
        status: Status::Experimental,
        summary: "the largest: better messages, but slow to load (8 s on a GPU)",
        gguf: Gguf {
            repo: "Qwen/Qwen2.5-Coder-7B-Instruct-GGUF",
            revision: "13fb94bfda8c8cf22497dc57b78f391a9acb426a",
            file: "qwen2.5-coder-7b-instruct-q4_k_m.gguf",
            size: 4_683_073_536,
            sha256: "509287f78cb4d4cf6b3843734733b914b2c158e43e22a7f4bf5e963800894d3c",
        },
        openvino: None,
        rules: PromptRules::Full,
    },
    Model {
        id: "smart-commit-1.5b-v0.1",
        name: "smart-commit 1.5B v0.1",
        license: "Apache-2.0",
        status: Status::Experimental,
        summary: "Qwen2.5-Coder-1.5B fine-tuned for smart-commit: the right commit types",
        gguf: Gguf {
            repo: "LucasBRT/smart-commit-1.5b-v0.1",
            revision: "9147da25797e21b63815f9ad80a1e79547198a37",
            file: "smart-commit-1.5b-v0.1-q4_k_m.gguf",
            size: 986_048_192,
            sha256: "7b8f456ff62c050e43ebdb90593960990e490e1dd7c2635652a82929a040d9f7",
        },
        openvino: None,
        rules: PromptRules::Full,
    },
    Model {
        id: "smart-commit-0.5b-v0.1",
        name: "smart-commit 0.5B v0.1",
        license: "Apache-2.0",
        status: Status::Experimental,
        summary: "Qwen2.5-Coder-0.5B fine-tuned: as good as the 1.5B on types, faster",
        gguf: Gguf {
            repo: "LucasBRT/smart-commit-0.5b-v0.1",
            revision: "829633003d096d47256a8dd12fe666ea29f89cb1",
            file: "smart-commit-0.5b-v0.1-q4_k_m.gguf",
            size: 397_807_616,
            sha256: "83b9acfc91e67cba1a2b669a0afe59ddc1b3ce5909617355b88d3a2ff6e66c11",
        },
        openvino: None,
        rules: PromptRules::Full,
    },
];

pub fn find(id: &str) -> Option<&'static Model> {
    REGISTRY.iter().find(|m| m.id == id)
}

/// The size of a download, for people.
pub fn size(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1e9)
    } else {
        format!("{} MB", bytes / 1_000_000)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("cannot download {url}: {problem}")]
    Download { url: String, problem: String },
    #[error("{path} is not the file pinned for {id}: SHA-256 {actual}, expected {expected}")]
    Checksum {
        id: &'static str,
        path: String,
        actual: String,
        expected: &'static str,
    },
    #[error("{0} is not a GGUF file")]
    NotGguf(String),
    #[error("{0}")]
    Io(String),
}

fn io(what: impl fmt::Display) -> impl FnOnce(std::io::Error) -> ModelError {
    move |err| ModelError::Io(format!("{what}: {err}"))
}

/// The SHA-256 of a file, in hex.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// `<file>.sha256`, which the llama.cpp backend reads to name the model in
/// traces.
pub fn sidecar(file: &Path) -> PathBuf {
    PathBuf::from(format!("{}.sha256", file.display()))
}

/// Whether `file` is the pinned GGUF: its sidecar names the pinned hash and
/// its size matches (hashing a few GB on every check would be slow).
pub fn is_installed(gguf: &Gguf, file: &Path) -> bool {
    let recorded = std::fs::read_to_string(sidecar(file)).unwrap_or_default();
    recorded.split_whitespace().next() == Some(gguf.sha256)
        && std::fs::metadata(file).is_ok_and(|m| m.len() == gguf.size)
}

/// The instructions the GGUF at `file` needs: those of the recommended
/// model it was checked to be, or all of them for any other file, since
/// only a model trained without them can do without.
pub fn prompt_rules(file: &Path) -> PromptRules {
    REGISTRY
        .iter()
        .find(|model| is_installed(&model.gguf, file))
        .map_or(PromptRules::Full, |model| model.rules)
}

/// Downloads `model`'s GGUF into `dir` from `base` (normally
/// [`HUGGING_FACE`]) unless it is already there, and checks it. `progress`
/// is called with the bytes downloaded so far, about twice a second.
pub fn install_gguf(
    model: &'static Model,
    dir: &Path,
    base: &str,
    mut progress: impl FnMut(u64),
) -> Result<PathBuf, ModelError> {
    let gguf = &model.gguf;
    let file = dir.join(gguf.file);
    if is_installed(gguf, &file) {
        return Ok(file);
    }
    std::fs::create_dir_all(dir).map_err(io(dir.display()))?;
    let partial = dir.join(format!("{}.partial", gguf.file));
    let url = gguf.url(base);
    download(&url, &partial, &mut progress)?;
    let actual = sha256_file(&partial).map_err(io(partial.display()))?;
    if actual != gguf.sha256 {
        let _ = std::fs::remove_file(&partial);
        return Err(ModelError::Checksum {
            id: model.id,
            path: url,
            actual,
            expected: gguf.sha256,
        });
    }
    std::fs::rename(&partial, &file).map_err(io(file.display()))?;
    std::fs::write(sidecar(&file), format!("{}  {}\n", gguf.sha256, gguf.file))
        .map_err(io(file.display()))?;
    Ok(file)
}

/// Downloads with curl, which every supported system has, reporting the
/// size of the partial file while it runs. A partial file from an
/// interrupted download is resumed.
fn download(url: &str, dest: &Path, progress: &mut dyn FnMut(u64)) -> Result<(), ModelError> {
    let failed = |problem: String| ModelError::Download {
        url: url.to_owned(),
        problem,
    };
    let mut child = Command::new("curl")
        .args(["-fsSL", "--retry", "3", "-C", "-", "-o"])
        .arg(dest)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| failed(format!("cannot run curl: {err}")))?;
    let status = loop {
        progress(std::fs::metadata(dest).map_or(0, |m| m.len()));
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(500)),
            Err(err) => return Err(failed(err.to_string())),
        }
    };
    progress(std::fs::metadata(dest).map_or(0, |m| m.len()));
    if !status.success() {
        let mut stderr = String::new();
        if let Some(mut pipe) = child.stderr.take() {
            let _ = pipe.read_to_string(&mut stderr);
        }
        return Err(failed(
            stderr
                .trim()
                .lines()
                .last()
                .unwrap_or("curl failed")
                .to_owned(),
        ));
    }
    Ok(())
}

/// Checks that `path` is a GGUF file (by its magic number), for a model
/// chosen from disk.
pub fn check_gguf(path: &Path) -> Result<(), ModelError> {
    let mut magic = [0u8; 4];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut magic))
        .map_err(io(path.display()))?;
    if &magic == b"GGUF" {
        Ok(())
    } else {
        Err(ModelError::NotGguf(path.display().to_string()))
    }
}

/// How many models from disk [`remember_local`] keeps.
pub const LOCAL_LIMIT: usize = 10;

/// A model from disk, used before: a GGUF file (`model.gguf`) or an
/// OpenVINO model directory (`model.dir`), unverified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Local {
    pub path: PathBuf,
}

impl Local {
    /// The setting it is used through.
    pub fn key(&self) -> &'static str {
        if self.path.is_dir() {
            "model.dir"
        } else {
            "model.gguf"
        }
    }

    /// Its name in lists: the file name without `.gguf`, or the directory's.
    pub fn name(&self) -> String {
        let name = self
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string());
        match name.strip_suffix(".gguf") {
            Some(stem) if !self.path.is_dir() => stem.to_owned(),
            _ => name,
        }
    }

    /// The file's size; none for a directory.
    pub fn size(&self) -> Option<u64> {
        std::fs::metadata(&self.path)
            .ok()
            .filter(|m| m.is_file())
            .map(|m| m.len())
    }

    /// The devices that run it.
    pub fn devices(&self) -> &'static str {
        if self.path.is_dir() {
            "npu, intel-gpu and openvino-cpu"
        } else {
            "vulkan and cpu"
        }
    }
}

/// The models from disk recorded in `file` (one path per line, most recent
/// first) that still exist; none if it cannot be read.
pub fn local_models(file: &Path) -> Vec<Local> {
    let text = std::fs::read_to_string(file).unwrap_or_default();
    let mut paths: Vec<PathBuf> = Vec::new();
    for line in text.lines().filter(|l| !l.is_empty()) {
        let path = PathBuf::from(line);
        if path.exists() && !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths.into_iter().map(|path| Local { path }).collect()
}

/// Records `path` first in `file`, without the paths that no longer exist,
/// and keeps the [`LOCAL_LIMIT`] most recent.
pub fn remember_local(file: &Path, path: &Path) -> std::io::Result<()> {
    let mut paths = vec![path.to_path_buf()];
    paths.extend(
        local_models(file)
            .into_iter()
            .map(|local| local.path)
            .filter(|p| p != path),
    );
    paths.truncate(LOCAL_LIMIT);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text: String = paths.iter().map(|p| format!("{}\n", p.display())).collect();
    std::fs::write(file, text)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn the_default_model_is_the_one_the_scripts_install() {
        let model = find(DEFAULT).unwrap();
        let script = include_str!("../scripts/setup-llamacpp.sh");
        assert!(script.contains(&format!("GGUF_NAME={}\n", model.gguf.file)));
        assert!(script.contains(&format!("GGUF_SHA256={}\n", model.gguf.sha256)));
        assert!(script.contains(model.gguf.revision));
        let export = include_str!("../scripts/export-model.sh");
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
}

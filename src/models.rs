//! The models smart-commit recommends, and installing them.
//!
//! Models are never served by this project: each one is downloaded from its
//! publisher's Hugging Face repository, pinned to a revision and checked
//! against the SHA-256 of the file at that revision. Any other GGUF file can
//! be used from disk instead (`model.gguf`), unverified.
//!
//! The OpenVINO devices (`npu`, `intel-gpu`, `openvino-cpu`) need the model
//! exported for OpenVINO, which only models with a pinned export have.

use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use sha2::{Digest, Sha256};

/// Where models are downloaded from.
pub const HUGGING_FACE: &str = "https://huggingface.co";

/// The model used unless `model.name` says otherwise.
pub const DEFAULT: &str = "qwen2.5-coder-1.5b";

/// The ids of [`REGISTRY`], in its order (`model.name`'s choices).
pub const IDS: &[&str] = &[
    "qwen2.5-coder-1.5b",
    "qwen2.5-coder-0.5b",
    "qwen2.5-coder-7b",
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

#[cfg(test)]
mod tests {
    use super::*;

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

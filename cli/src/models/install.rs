use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use sha2::{Digest, Sha256};

use super::registry::*;
use crate::prompt::PromptRules;

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
    Ok(hex(&hasher.finalize()))
}

/// Bytes as lowercase hex.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The SHA-256 of some bytes, in hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
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
    fetch(model.id, &gguf.url(base), gguf.sha256, &file, &mut progress)?;
    std::fs::write(sidecar(&file), format!("{}  {}\n", gguf.sha256, gguf.file))
        .map_err(io(file.display()))?;
    Ok(file)
}

/// Downloads `url` to `dest`, through a `.partial` file that only becomes
/// `dest` when its SHA-256 is `expected`; `label` names what it is in the
/// error. `progress` is called with the bytes downloaded so far.
pub fn fetch(
    label: &'static str,
    url: &str,
    expected: &'static str,
    dest: &Path,
    mut progress: impl FnMut(u64),
) -> Result<(), ModelError> {
    let partial = PathBuf::from(format!("{}.partial", dest.display()));
    download(url, &partial, &mut progress)?;
    let actual = sha256_file(&partial).map_err(io(partial.display()))?;
    if actual != expected {
        let _ = std::fs::remove_file(&partial);
        return Err(ModelError::Checksum {
            id: label,
            path: url.to_owned(),
            actual,
            expected,
        });
    }
    std::fs::rename(&partial, dest).map_err(io(dest.display()))
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

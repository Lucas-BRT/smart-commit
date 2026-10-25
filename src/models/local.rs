use std::path::{Path, PathBuf};

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

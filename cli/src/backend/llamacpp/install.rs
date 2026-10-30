//! Installing llama.cpp: the Vulkan build `smart-commit setup` pins, which
//! runs the `cpu` device on any x86_64 machine and the `vulkan` device on
//! any GPU with a Vulkan driver. Nothing here needs sudo.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::LLAMA_CPP_BUILD;
use crate::models::{ModelError, fetch};

/// The SHA-256 of the pinned release archive.
pub const ARCHIVE_SHA256: &str = "6f93bf4138e9ad6a380ff605be1fd5108a66ec1977218eb74613f90634e18c64";

/// The size of the pinned release archive, in bytes.
pub const ARCHIVE_SIZE: u64 = 31_615_869;

/// Where the pinned release is downloaded from.
pub fn archive_url() -> String {
    format!(
        "https://github.com/ggml-org/llama.cpp/releases/download/{LLAMA_CPP_BUILD}/\
         llama-{LLAMA_CPP_BUILD}-bin-ubuntu-vulkan-x64.tar.gz"
    )
}

/// The server of the build in `dir` (`.../smart-commit/llama.cpp`).
pub fn server_in(dir: &Path) -> PathBuf {
    dir.join(format!("llama-{LLAMA_CPP_BUILD}/llama-server"))
}

/// Downloads the archive at `url`, checks it against `sha256`, unpacks it in
/// `dir` and returns the server; a build already there is left alone.
pub fn install(
    dir: &Path,
    url: &str,
    sha256: &'static str,
    progress: impl FnMut(u64),
) -> Result<PathBuf, ModelError> {
    let server = server_in(dir);
    if server.is_file() {
        return Ok(server);
    }
    let io = |what: &Path| {
        let what = what.display().to_string();
        move |err: std::io::Error| ModelError::Io(format!("{what}: {err}"))
    };
    std::fs::create_dir_all(dir).map_err(io(dir))?;
    let archive = dir.join(format!("llama-{LLAMA_CPP_BUILD}.tar.gz"));
    fetch("llama.cpp", url, sha256, &archive, progress)?;
    // Unpacked beside the build and renamed into place, so that an
    // interrupted `tar` never leaves a `llama-server` without its libraries
    // that the check above would take for an installed build.
    let build = dir.join(format!("llama-{LLAMA_CPP_BUILD}"));
    let unpacking = dir.join(".unpacking");
    let _ = std::fs::remove_dir_all(&unpacking);
    std::fs::create_dir_all(&unpacking).map_err(io(&unpacking))?;
    let status = Command::new("tar")
        .arg("-xzf")
        .arg(&archive)
        .arg("-C")
        .arg(&unpacking)
        .stdin(Stdio::null())
        .status()
        .map_err(io(&archive));
    let _ = std::fs::remove_file(&archive);
    let status = status.inspect_err(|_| {
        let _ = std::fs::remove_dir_all(&unpacking);
    })?;
    let unpacked = unpacking.join(format!("llama-{LLAMA_CPP_BUILD}"));
    let problem = if !status.success() {
        Some(format!("cannot unpack {url}: tar {status}"))
    } else if !unpacked.join("llama-server").is_file() {
        Some(format!(
            "{} has no {}",
            url,
            server.strip_prefix(dir).unwrap_or(&server).display()
        ))
    } else {
        None
    };
    if let Some(problem) = problem {
        let _ = std::fs::remove_dir_all(&unpacking);
        return Err(ModelError::Io(problem));
    }
    // A half-built directory of an earlier attempt would make the rename fail.
    let _ = std::fs::remove_dir_all(&build);
    let moved = std::fs::rename(&unpacked, &build).map_err(io(&build));
    let _ = std::fs::remove_dir_all(&unpacking);
    moved?;
    Ok(server)
}

/// What llama-server lists as the devices it can use besides the CPU.
pub fn list_devices(server: &Path) -> String {
    let output = Command::new(server)
        .arg("--list-devices")
        .stdin(Stdio::null())
        .output();
    let Ok(output) = output else {
        return String::new();
    };
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    text.lines()
        .skip_while(|l| !l.contains("Available devices"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A release archive holding a `llama-server`, and its hash.
    fn archive(dir: &Path, with_server: bool) -> (PathBuf, &'static str) {
        let root = dir.join("stage");
        let inner = root.join(format!("llama-{LLAMA_CPP_BUILD}"));
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(
            inner.join(if with_server { "llama-server" } else { "other" }),
            "#!/bin/sh\n",
        )
        .unwrap();
        let tarball = dir.join("release.tar.gz");
        let status = Command::new("tar")
            .arg("-czf")
            .arg(&tarball)
            .arg("-C")
            .arg(&root)
            .arg(format!("llama-{LLAMA_CPP_BUILD}"))
            .status()
            .unwrap();
        assert!(status.success());
        let sha = crate::models::sha256_hex(&std::fs::read(&tarball).unwrap());
        (tarball, Box::leak(sha.into_boxed_str()))
    }

    fn url(path: &Path) -> String {
        format!("file://{}", path.display())
    }

    #[test]
    fn the_pinned_release_is_the_build_the_code_expects() {
        assert_eq!(
            archive_url(),
            "https://github.com/ggml-org/llama.cpp/releases/download/b11381/llama-b11381-bin-ubuntu-vulkan-x64.tar.gz"
        );
        assert_eq!(ARCHIVE_SHA256.len(), 64);
        assert_eq!(
            server_in(Path::new("/d")),
            Path::new("/d/llama-b11381/llama-server")
        );
    }

    #[test]
    fn a_checked_archive_is_unpacked_and_removed() {
        let dir = tempfile::TempDir::new().unwrap();
        let (tarball, sha) = archive(dir.path(), true);
        let target = dir.path().join("llama.cpp");
        let mut seen = 0;
        let server = install(&target, &url(&tarball), sha, |bytes| seen = bytes).unwrap();
        assert!(server.is_file());
        assert!(seen > 0);
        assert!(
            !target
                .join(format!("llama-{LLAMA_CPP_BUILD}.tar.gz"))
                .exists()
        );
        // Already there: nothing is downloaded, so a bad URL is not a problem.
        assert_eq!(
            install(&target, "file:///nonexistent", sha, |_| {}).unwrap(),
            server
        );
    }

    #[test]
    fn another_archive_is_refused_and_leaves_nothing() {
        let dir = tempfile::TempDir::new().unwrap();
        let (tarball, _) = archive(dir.path(), true);
        let target = dir.path().join("llama.cpp");
        let err = install(&target, &url(&tarball), ARCHIVE_SHA256, |_| {}).unwrap_err();
        assert!(
            err.to_string()
                .contains("is not the file pinned for llama.cpp"),
            "{err}"
        );
        assert!(!server_in(&target).exists());
        assert!(
            !target
                .join(format!("llama-{LLAMA_CPP_BUILD}.tar.gz.partial"))
                .exists()
        );
    }

    #[test]
    fn an_interrupted_unpacking_leaves_no_server_behind() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().join("stage");
        let inner = root.join(format!("llama-{LLAMA_CPP_BUILD}"));
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(inner.join("llama-server"), "#!/bin/sh\n").unwrap();
        // Incompressible, so that cutting the end leaves the server unpacked.
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let noise: Vec<u8> = (0..400_000)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 24) as u8
            })
            .collect();
        std::fs::write(inner.join("libggml.so"), noise).unwrap();
        let tarball = dir.path().join("release.tar.gz");
        let status = Command::new("tar")
            .arg("-czf")
            .arg(&tarball)
            .arg("-C")
            .arg(&root)
            .arg(format!("llama-{LLAMA_CPP_BUILD}/llama-server"))
            .arg(format!("llama-{LLAMA_CPP_BUILD}/libggml.so"))
            .status()
            .unwrap();
        assert!(status.success());
        let mut bytes = std::fs::read(&tarball).unwrap();
        bytes.truncate(bytes.len() - 200_000);
        std::fs::write(&tarball, &bytes).unwrap();
        let sha: &'static str = Box::leak(crate::models::sha256_hex(&bytes).into_boxed_str());

        let target = dir.path().join("llama.cpp");
        let err = install(&target, &url(&tarball), sha, |_| {}).unwrap_err();
        assert!(err.to_string().contains("cannot unpack"), "{err}");
        // Nothing the next run could take for an installed build.
        assert!(!server_in(&target).exists());
        assert!(!target.join(format!("llama-{LLAMA_CPP_BUILD}")).exists());
        assert!(!target.join(".unpacking").exists());
    }

    #[test]
    fn a_half_built_directory_of_an_earlier_attempt_is_replaced() {
        let dir = tempfile::TempDir::new().unwrap();
        let (tarball, sha) = archive(dir.path(), true);
        let target = dir.path().join("llama.cpp");
        let build = target.join(format!("llama-{LLAMA_CPP_BUILD}"));
        std::fs::create_dir_all(&build).unwrap();
        std::fs::write(build.join("stale"), "").unwrap();
        std::fs::create_dir_all(target.join(".unpacking/old")).unwrap();
        let server = install(&target, &url(&tarball), sha, |_| {}).unwrap();
        assert!(server.is_file());
        assert!(!build.join("stale").exists());
        assert!(!target.join(".unpacking").exists());
    }

    #[test]
    fn an_archive_without_the_server_is_an_error() {
        let dir = tempfile::TempDir::new().unwrap();
        let (tarball, sha) = archive(dir.path(), false);
        let err = install(&dir.path().join("llama.cpp"), &url(&tarball), sha, |_| {}).unwrap_err();
        assert!(
            err.to_string().contains("has no llama-b11381/llama-server"),
            "{err}"
        );
    }

    #[test]
    fn a_missing_download_is_reported() {
        let dir = tempfile::TempDir::new().unwrap();
        let err = install(
            &dir.path().join("l"),
            "file:///nonexistent/x.tar.gz",
            ARCHIVE_SHA256,
            |_| {},
        )
        .unwrap_err();
        assert!(err.to_string().contains("cannot download"), "{err}");
    }

    #[test]
    fn the_devices_are_what_the_server_lists_from_its_header_on() {
        let dir = tempfile::TempDir::new().unwrap();
        let server = dir.path().join("llama-server");
        std::fs::write(
            &server,
            "#!/bin/sh\necho 'loading' >&2\necho 'Available devices:'\necho '  Vulkan0: GPU (8192 MiB)'\n",
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&server, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        // Running a script just written can meet "text file busy" while another
        // test's child process still holds the descriptor: try again.
        let mut devices = String::new();
        for _ in 0..20 {
            devices = list_devices(&server);
            if !devices.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert_eq!(devices, "Available devices:\n  Vulkan0: GPU (8192 MiB)");
        assert_eq!(list_devices(&dir.path().join("missing")), "");
    }
}

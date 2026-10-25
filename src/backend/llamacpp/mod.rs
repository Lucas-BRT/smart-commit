//! A second backend: llama.cpp's server, with Vulkan for the GPUs OpenVINO
//! cannot drive (AMD and NVIDIA, with proprietary or open-source drivers;
//! it runs on Intel GPUs too), and on the CPU of any x86_64 machine, which
//! needs no OpenVINO. The release build picks the CPU code for the machine
//! (Intel or AMD) at run time.
//!
//! Each generation session starts a private `llama-server`, listening on a
//! UNIX socket in a fresh temporary directory (nothing is exposed on the
//! network), and stops it when the backend is dropped; with
//! `model.keep_loaded`, [`crate::backend::server`] keeps one running between commits
//! instead. The HTTP client is a
//! minimal HTTP/1.1 one over that socket, so no network library is needed.

mod http;
pub mod install;
#[cfg(test)]
mod tests;

pub use http::{http, parse_http_response};

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::backend::{Backend, BackendError, GenerationConfig, Metrics};
use crate::prompt::Prompt;

/// The llama.cpp release `smart-commit setup` installs.
pub const LLAMA_CPP_BUILD: &str = "b11381";

/// UNIX socket paths are limited to 108 bytes on Linux.
const MAX_SOCKET_PATH: usize = 100;

/// Where llama.cpp runs the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LlamaDevice {
    /// Every layer on the CPU.
    Cpu,
    /// Every layer on this Vulkan device (`VulkanN`, as
    /// `llama-server --list-devices` numbers them).
    Vulkan(usize),
}

impl fmt::Display for LlamaDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cpu => f.write_str("CPU"),
            Self::Vulkan(n) => write!(f, "Vulkan{n}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlamaCppOptions {
    pub server: PathBuf,
    /// A GGUF model; `<model>.sha256` next to it, if present, identifies it.
    pub model: PathBuf,
    pub device: LlamaDevice,
    pub max_prompt_tokens: usize,
    pub max_new_tokens: usize,
    /// Threads reading the prompt on the CPU, which is most of the time per
    /// message there. Decoding keeps llama.cpp's default: changing it
    /// changed replies, and replies are short.
    pub threads_batch: usize,
    pub startup_timeout: Duration,
    /// Skips what llama-server does at every start that a small model with a
    /// fixed context rarely needs: repacking the weights for the CPU's vector
    /// instructions (a copy of most of the model) and fitting the model to the
    /// GPUs' free memory. `smart-commit bench` measures whether it pays off.
    pub fast_load: bool,
}

/// The flags of [`LlamaCppOptions::fast_load`].
pub const FAST_LOAD_FLAGS: [&str; 3] = ["--no-repack", "--fit", "off"];

pub struct LlamaCppBackend {
    server: Option<Mutex<Child>>,
    dir: Option<PathBuf>,
    socket: PathBuf,
    description: String,
    load_ms: f32,
    last_metrics: Mutex<Option<Metrics>>,
}

impl fmt::Debug for LlamaCppBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LlamaCppBackend")
            .field("socket", &self.socket)
            .field("description", &self.description)
            .finish_non_exhaustive()
    }
}

/// The error of the model or the server not being where `options` say.
pub fn missing_file(options: &LlamaCppOptions) -> Option<BackendError> {
    let unavailable = BackendError::Unavailable;
    if !options.model.is_file() {
        return Some(unavailable(format!(
            "no GGUF model at {}; `smart-commit setup` downloads it",
            options.model.display()
        )));
    }
    if !options.server.is_file() {
        return Some(unavailable(format!(
            "no llama-server at {}; `smart-commit setup` installs it",
            options.server.display()
        )));
    }
    None
}

impl LlamaCppBackend {
    /// Starts `llama-server` and waits until the model is loaded.
    pub fn start(options: &LlamaCppOptions) -> Result<Self, BackendError> {
        Self::start_in(options, None)
    }

    /// Like [`Self::start`], with the socket and log in `dir` instead of a
    /// new temporary directory; `dir` is removed when the backend is
    /// dropped, like the temporary one.
    pub fn start_in(options: &LlamaCppOptions, dir: Option<&Path>) -> Result<Self, BackendError> {
        Self::start_with(options, dir, &[])
    }

    /// Like [`Self::start_in`], with `extra` llama-server flags after
    /// smart-commit's own, to try others than it passes (`bench --prompts`).
    pub fn start_with(
        options: &LlamaCppOptions,
        dir: Option<&Path>,
        extra: &[String],
    ) -> Result<Self, BackendError> {
        let unavailable = BackendError::Unavailable;
        if let Some(missing) = missing_file(options) {
            return Err(missing);
        }
        let dir = match dir {
            Some(dir) => private_dir_at(dir).map(|()| dir.to_owned()),
            None => private_dir(),
        }
        .map_err(|e| unavailable(format!("cannot create a socket dir: {e}")))?;
        let socket = dir.join("server.sock");
        let log_path = dir.join("server.log");
        let log = std::fs::File::create(&log_path)
            .map_err(|e| unavailable(format!("cannot create {}: {e}", log_path.display())))?;

        let started = Instant::now();
        let child = Command::new(&options.server)
            .args(server_args(options, &socket))
            .args(extra)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log)
            .spawn();
        let child = match child {
            Ok(child) => child,
            Err(err) => {
                let _ = std::fs::remove_dir_all(&dir);
                return Err(unavailable(format!(
                    "cannot start {}: {err}",
                    options.server.display()
                )));
            }
        };
        let mut backend = Self {
            server: Some(Mutex::new(child)),
            dir: Some(dir),
            socket,
            description: describe(options),
            load_ms: 0.0,
            last_metrics: Mutex::new(None),
        };
        backend.wait_until_ready(options.startup_timeout, &log_path)?;
        backend.load_ms = started.elapsed().as_secs_f32() * 1000.0;
        Ok(backend)
    }

    /// Starts with [`LlamaCppOptions::fast_load`] and, if that fails (without
    /// the memory fit, a GPU short of memory cannot load the model), starts
    /// again without it. Returns a note when it had to.
    pub fn start_or_load_normally(
        options: &LlamaCppOptions,
    ) -> Result<(Self, Option<String>), BackendError> {
        Self::start_or_load_normally_in(options, None)
    }

    /// Like [`Self::start_or_load_normally`], in `dir` as [`Self::start_in`].
    pub fn start_or_load_normally_in(
        options: &LlamaCppOptions,
        dir: Option<&Path>,
    ) -> Result<(Self, Option<String>), BackendError> {
        match Self::start_in(options, dir) {
            Ok(backend) => Ok((backend, None)),
            Err(fast) if options.fast_load => {
                let normal = LlamaCppOptions {
                    fast_load: false,
                    ..options.clone()
                };
                let backend = Self::start_in(&normal, dir)?;
                let reason = fast.to_string();
                Ok((
                    backend,
                    Some(format!(
                        "fast load failed on {} ({}); loaded normally",
                        options.device,
                        reason.lines().next().unwrap_or(&reason)
                    )),
                ))
            }
            Err(err) => Err(err),
        }
    }

    /// Talks to a server that is already running, without managing it.
    #[doc(hidden)]
    pub fn connect(socket: PathBuf, description: String) -> Self {
        Self {
            server: None,
            dir: None,
            socket,
            description,
            load_ms: 0.0,
            last_metrics: Mutex::new(None),
        }
    }

    /// Whether the server this backend started is still running; a backend
    /// that only connected cannot tell, and says yes.
    pub fn is_running(&self) -> bool {
        self.server
            .as_ref()
            .is_none_or(|server| matches!(server.lock().expect("server lock").try_wait(), Ok(None)))
    }

    fn wait_until_ready(&self, timeout: Duration, log: &Path) -> Result<(), BackendError> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(server) = &self.server
                && let Ok(Some(status)) = server.lock().expect("server lock").try_wait()
            {
                return Err(BackendError::Unavailable(format!(
                    "llama-server exited with {status} while loading the model: {}",
                    log_tail(log)
                )));
            }
            if let Ok((200, _)) = http(&self.socket, "GET", "/health", None) {
                return Ok(());
            }
            if Instant::now() > deadline {
                return Err(BackendError::Unavailable(format!(
                    "llama-server was not ready after {}s: {}",
                    timeout.as_secs(),
                    log_tail(log)
                )));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for LlamaCppBackend {
    fn drop(&mut self) {
        if let Some(server) = &self.server {
            let mut child = server.lock().expect("server lock");
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(dir) = &self.dir {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

impl Backend for LlamaCppBackend {
    fn describe(&self) -> String {
        self.description.clone()
    }

    fn generate(&self, prompt: &Prompt, config: &GenerationConfig) -> Result<String, BackendError> {
        let body = chat_request(prompt, config).to_string();
        let (status, response) = http(&self.socket, "POST", "/v1/chat/completions", Some(&body))
            .map_err(|e| BackendError::Generation(format!("llama-server request failed: {e}")))?;
        if status != 200 {
            return Err(BackendError::Generation(format!(
                "llama-server answered {status}: {}",
                response.trim()
            )));
        }
        let (text, mut metrics) = parse_chat_response(&response)?;
        metrics.load_ms = self.load_ms;
        *self.last_metrics.lock().expect("metrics lock") = Some(metrics);
        Ok(text)
    }

    fn last_metrics(&self) -> Option<Metrics> {
        self.last_metrics.lock().expect("metrics lock").clone()
    }
}

/// Everything explicit: all layers on the chosen GPU, a context that fits
/// exactly our prompt budget and reply, one slot, no web UI, no network.
/// The context the server is started with: the prompt budget, half again
/// as room for a prompt whose tokens were underestimated (at most 1.25
/// times on the prompts measured, see [`crate::diff::estimate_tokens`]),
/// and the reply. A
/// longer prompt is read a little longer instead of failing; the KV cache
/// it costs is a few megabytes for the small models.
pub fn context_size(options: &LlamaCppOptions) -> usize {
    options.max_prompt_tokens + options.max_prompt_tokens / 2 + options.max_new_tokens
}

pub fn server_args(options: &LlamaCppOptions, socket: &Path) -> Vec<String> {
    let context = context_size(options);
    let (layers, device) = match options.device {
        // `none` keeps llama.cpp from offloading anything to a GPU it finds.
        LlamaDevice::Cpu => ("0", "none".to_owned()),
        LlamaDevice::Vulkan(_) => ("999", options.device.to_string()),
    };
    [
        "--model",
        &options.model.to_string_lossy(),
        // A path ending in `.sock` makes llama-server listen on a UNIX socket.
        "--host",
        &socket.to_string_lossy(),
        "--n-gpu-layers",
        layers,
        "--device",
        &device,
        "--ctx-size",
        &context.to_string(),
        "--parallel",
        "1",
        "--threads-batch",
        &options.threads_batch.to_string(),
        "--no-webui",
        "--offline",
    ]
    .iter()
    .chain(if options.fast_load {
        FAST_LOAD_FLAGS.as_slice()
    } else {
        &[]
    })
    .map(|s| (*s).to_owned())
    .collect()
}

/// Greedy and seeded, without prompt caching (which could change batching
/// between calls), like the OpenVINO backend.
pub fn chat_request(prompt: &Prompt, config: &GenerationConfig) -> Value {
    json!({
        "messages": [
            {"role": "system", "content": prompt.system},
            {"role": "user", "content": prompt.user},
        ],
        "max_tokens": config.max_new_tokens,
        "temperature": 0,
        "top_k": 1,
        "seed": 0,
        "cache_prompt": false,
    })
}

pub fn parse_chat_response(body: &str) -> Result<(String, Metrics), BackendError> {
    let invalid =
        |what: &str| BackendError::Generation(format!("unexpected llama-server reply: {what}"));
    let value: Value = serde_json::from_str(body).map_err(|e| invalid(&e.to_string()))?;
    let text = value
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("no choices[0].message.content"))?
        .to_owned();
    let timing = |key: &str| {
        value
            .pointer(&format!("/timings/{key}"))
            .and_then(Value::as_f64)
    };
    let count = |key: &str| timing(key).map_or(0, |n| n as usize);
    let prompt_ms = timing("prompt_ms").unwrap_or(0.0) as f32;
    let predicted_ms = timing("predicted_ms").unwrap_or(0.0) as f32;
    Ok((
        text,
        Metrics {
            input_tokens: count("prompt_n"),
            output_tokens: count("predicted_n"),
            load_ms: 0.0,
            time_to_first_token_ms: prompt_ms,
            time_per_output_token_ms: timing("predicted_per_token_ms").unwrap_or(0.0) as f32,
            generate_ms: prompt_ms + predicted_ms,
        },
    ))
}

/// `dir`, created if needed and readable by this user only.
fn private_dir_at(dir: &Path) -> std::io::Result<()> {
    if dir.as_os_str().len() > MAX_SOCKET_PATH - 12 {
        return Err(std::io::Error::other(format!(
            "{} is too long for a socket path",
            dir.display()
        )));
    }
    std::fs::create_dir_all(dir)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

/// A new directory under the temporary directory, short enough for the
/// socket path limit.
fn private_dir() -> std::io::Result<PathBuf> {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let mut base = std::env::temp_dir();
    if base.as_os_str().len() > MAX_SOCKET_PATH - 40 {
        base = PathBuf::from("/tmp");
    }
    let name = format!(
        "smart-commit-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let dir = base.join(name);
    std::fs::create_dir(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(dir)
}

fn log_tail(path: &Path) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let tail = lines[lines.len().saturating_sub(3)..].join(" | ");
    if tail.is_empty() {
        "no output".to_owned()
    } else {
        tail
    }
}

/// Server build, device, model and the model's recorded SHA-256.
pub fn describe(options: &LlamaCppOptions) -> String {
    let name = |p: &Path| {
        p.file_name().map_or_else(
            || p.to_string_lossy().into_owned(),
            |n| n.to_string_lossy().into_owned(),
        )
    };
    let build = options
        .server
        .parent()
        .map_or_else(|| "unknown".to_owned(), name);
    let sidecar = PathBuf::from(format!("{}.sha256", options.model.display()));
    let identity = std::fs::read_to_string(sidecar)
        .ok()
        .and_then(|s| {
            s.split_whitespace()
                .next()
                .map(|h| h.chars().take(16).collect::<String>())
        })
        .unwrap_or_else(|| "unverified".to_owned());
    // Threads only matter where llama.cpp computes on the CPU.
    let threads = match options.device {
        LlamaDevice::Cpu => format!(" threads-batch={}", options.threads_batch),
        LlamaDevice::Vulkan(_) => String::new(),
    };
    format!(
        "llama.cpp server={build} device={} model={} sha256={identity} ctx={}{threads}{}",
        options.device,
        name(&options.model),
        context_size(options),
        if options.fast_load { " load=fast" } else { "" }
    )
}

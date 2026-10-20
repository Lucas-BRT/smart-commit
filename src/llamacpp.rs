//! A second backend: llama.cpp's server, with Vulkan for the GPUs OpenVINO
//! cannot drive (AMD and NVIDIA, with proprietary or open-source drivers;
//! it runs on Intel GPUs too), and on the CPU of any x86_64 machine, which
//! needs no OpenVINO. The release build picks the CPU code for the machine
//! (Intel or AMD) at run time.
//!
//! Each generation session starts a private `llama-server`, listening on a
//! UNIX socket in a fresh temporary directory (nothing is exposed on the
//! network), and stops it when the backend is dropped; with
//! `model.keep_loaded`, [`crate::server`] keeps one running between commits
//! instead. The HTTP client is a
//! minimal HTTP/1.1 one over that socket, so no network library is needed.

use std::fmt;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
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

impl LlamaCppBackend {
    /// Starts `llama-server` and waits until the model is loaded.
    pub fn start(options: &LlamaCppOptions) -> Result<Self, BackendError> {
        Self::start_in(options, None)
    }

    /// Like [`Self::start`], with the socket and log in `dir` instead of a
    /// new temporary directory; `dir` is removed when the backend is
    /// dropped, like the temporary one.
    pub fn start_in(options: &LlamaCppOptions, dir: Option<&Path>) -> Result<Self, BackendError> {
        let unavailable = BackendError::Unavailable;
        if !options.model.is_file() {
            return Err(unavailable(format!(
                "no GGUF model at {}; `smart-commit setup` downloads it",
                options.model.display()
            )));
        }
        if !options.server.is_file() {
            return Err(unavailable(format!(
                "no llama-server at {}; `smart-commit setup` installs it",
                options.server.display()
            )));
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
/// as room for a prompt whose tokens were underestimated (the budget counts
/// about 3 characters per token, and hashes, base64 or long numbers take
/// up to twice as many tokens: Qwen splits every digit), and the reply. A
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

/// One HTTP/1.1 request over the UNIX socket; returns status and body.
pub fn http(
    socket: &Path,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> std::io::Result<(u16, String)> {
    let mut stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(Duration::from_secs(300)))?;
    let body = body.unwrap_or("");
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)?;
    parse_http_response(&raw).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// Parses a complete HTTP/1.1 response, with a Content-Length or chunked
/// body (or one that ends with the connection).
pub fn parse_http_response(raw: &[u8]) -> Result<(u16, String), String> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("no end of headers")?;
    let head = std::str::from_utf8(&raw[..split]).map_err(|_| "headers are not UTF-8")?;
    let mut body = &raw[split + 4..];
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or("no status code")?;
    let header = |name: &str| {
        head.split("\r\n").skip(1).find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim()
                .eq_ignore_ascii_case(name)
                .then(|| v.trim().to_owned())
        })
    };
    let decoded;
    if header("transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked")) {
        decoded = dechunk(body)?;
        body = &decoded;
    } else if let Some(length) = header("content-length").and_then(|v| v.parse::<usize>().ok()) {
        body = body
            .get(..length)
            .ok_or("body shorter than Content-Length")?;
    }
    let text = String::from_utf8(body.to_vec()).map_err(|_| "body is not UTF-8")?;
    Ok((status, text))
}

fn dechunk(mut data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let line_end = data
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or("truncated chunk size")?;
        let size_text = std::str::from_utf8(&data[..line_end]).map_err(|_| "bad chunk size")?;
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|_| format!("bad chunk size {size_text:?}"))?;
        data = &data[line_end + 2..];
        if size == 0 {
            return Ok(out);
        }
        out.extend_from_slice(data.get(..size).ok_or("truncated chunk")?);
        data = data.get(size + 2..).ok_or("truncated chunk")?;
    }
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

#[cfg(test)]
mod tests {
    use std::os::unix::net::UnixListener;

    use super::*;

    fn options(dir: &Path) -> LlamaCppOptions {
        LlamaCppOptions {
            server: dir.join("llama-b11381/llama-server"),
            model: dir.join("model.gguf"),
            device: LlamaDevice::Vulkan(1),
            max_prompt_tokens: 1024,
            max_new_tokens: 128,
            threads_batch: 6,
            startup_timeout: Duration::from_secs(5),
            fast_load: false,
        }
    }

    fn prompt() -> Prompt {
        Prompt {
            system: "sys".into(),
            user: "diff".into(),
        }
    }

    #[test]
    fn server_runs_on_the_chosen_gpu_with_our_context() {
        let args = server_args(&options(Path::new("/d")), Path::new("/tmp/x/server.sock"));
        let joined = args.join(" ");
        assert!(joined.contains("--model /d/model.gguf"));
        assert!(joined.contains("--host /tmp/x/server.sock"));
        assert!(joined.contains("--device Vulkan1"));
        assert!(joined.contains("--ctx-size 1664"));
        assert!(joined.contains("--n-gpu-layers 999"));
        assert!(joined.contains("--offline"));
        assert!(joined.contains("--threads-batch 6"));
        assert!(
            !joined.contains("--threads "),
            "decoding keeps the default: {joined}"
        );
    }

    #[test]
    fn on_the_cpu_nothing_is_offloaded() {
        let options = LlamaCppOptions {
            device: LlamaDevice::Cpu,
            ..options(Path::new("/d"))
        };
        let joined = server_args(&options, Path::new("/tmp/x/server.sock")).join(" ");
        assert!(joined.contains("--n-gpu-layers 0"), "{joined}");
        assert!(joined.contains("--device none"), "{joined}");
        assert!(!joined.contains("Vulkan"), "{joined}");
        assert!(joined.contains("--ctx-size 1664"), "{joined}");
        assert_eq!(
            describe(&options),
            "llama.cpp server=llama-b11381 device=CPU model=model.gguf sha256=unverified ctx=1664 \
             threads-batch=6"
        );
    }

    #[test]
    fn fast_load_skips_the_repack_and_the_memory_fit() {
        let normal = options(Path::new("/d"));
        let joined = server_args(&normal, Path::new("/tmp/x/server.sock")).join(" ");
        assert!(!joined.contains("repack"), "{joined}");
        assert!(!joined.contains("--fit"), "{joined}");

        let fast = LlamaCppOptions {
            fast_load: true,
            ..normal
        };
        let args = server_args(&fast, Path::new("/tmp/x/server.sock"));
        assert!(
            args.ends_with(&FAST_LOAD_FLAGS.map(String::from)),
            "{args:?}"
        );
        assert!(describe(&fast).ends_with(" ctx=1664 load=fast"));
        assert!(describe(&options(Path::new("/d"))).ends_with(" ctx=1664"));
    }

    #[test]
    fn a_failed_fast_load_is_retried_normally() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut options = options(dir.path());
        std::fs::write(&options.model, "gguf").unwrap();
        let calls = dir.path().join("calls");
        let server = dir.path().join("fake-server");
        std::fs::write(
            &server,
            format!(
                "#!/bin/sh\necho \"$*\" >>'{}'\necho 'error: out of device memory' >&2\nexit 3\n",
                calls.display()
            ),
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&server, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        options.server = server;
        options.fast_load = true;
        let err = LlamaCppBackend::start_or_load_normally(&options)
            .unwrap_err()
            .to_string();
        assert!(err.contains("out of device memory"), "{err}");
        let calls = std::fs::read_to_string(&calls).unwrap();
        let calls: Vec<&str> = calls.lines().collect();
        assert_eq!(calls.len(), 2, "{calls:?}");
        assert!(calls[0].ends_with("--no-repack --fit off"), "{calls:?}");
        assert!(!calls[1].contains("--no-repack"), "{calls:?}");

        // Without fast load there is nothing to fall back to.
        std::fs::remove_file(dir.path().join("calls")).unwrap();
        options.fast_load = false;
        assert!(LlamaCppBackend::start_or_load_normally(&options).is_err());
        let calls = std::fs::read_to_string(dir.path().join("calls")).unwrap();
        assert_eq!(calls.lines().count(), 1);
    }

    #[test]
    fn requests_are_greedy_and_seeded() {
        let request = chat_request(&prompt(), &GenerationConfig { max_new_tokens: 96 });
        assert_eq!(request["temperature"], 0);
        assert_eq!(request["top_k"], 1);
        assert_eq!(request["seed"], 0);
        assert_eq!(request["max_tokens"], 96);
        assert_eq!(request["cache_prompt"], false);
        assert_eq!(request["messages"][0]["role"], "system");
        assert_eq!(request["messages"][1]["content"], "diff");
    }

    #[test]
    fn parses_replies_with_timings() {
        let body = r#"{"choices":[{"message":{"role":"assistant","content":"Add support for renames"}}],
            "timings":{"prompt_n":142,"prompt_ms":290.4,"predicted_n":6,"predicted_ms":302.1,"predicted_per_token_ms":60.4}}"#;
        let (text, metrics) = parse_chat_response(body).unwrap();
        assert_eq!(text, "Add support for renames");
        assert_eq!((metrics.input_tokens, metrics.output_tokens), (142, 6));
        assert!((metrics.generate_ms - 592.5).abs() < 0.01);
        assert!(parse_chat_response(r#"{"error":{"message":"x"}}"#).is_err());
        assert!(parse_chat_response("not json").is_err());
    }

    #[test]
    fn parses_http_responses() {
        let fixed = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello trailing";
        assert_eq!(parse_http_response(fixed).unwrap(), (200, "hello".into()));
        let chunked = b"HTTP/1.1 503 Busy\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nwiki\r\n5;x=1\r\npedia\r\n0\r\n\r\n";
        assert_eq!(
            parse_http_response(chunked).unwrap(),
            (503, "wikipedia".into())
        );
        let until_close = b"HTTP/1.1 200 OK\r\n\r\n{}";
        assert_eq!(
            parse_http_response(until_close).unwrap(),
            (200, "{}".into())
        );
        assert!(parse_http_response(b"HTTP/1.1 200 OK\r\n").is_err());
        assert!(parse_http_response(b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\nshort").is_err());
        assert!(
            parse_http_response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n")
                .is_err()
        );
    }

    /// A fake llama-server: answers each connection with the next canned
    /// response and records the requests.
    fn fake_server(socket: &Path, responses: Vec<String>) -> std::thread::JoinHandle<Vec<String>> {
        let listener = UnixListener::bind(socket).unwrap();
        std::thread::spawn(move || {
            let mut requests = Vec::new();
            for response in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = vec![0u8; 65536];
                let mut request = Vec::new();
                loop {
                    let n = stream.read(&mut buf).unwrap();
                    request.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&request);
                    if let Some(end) = text.find("\r\n\r\n") {
                        let length = text[..end]
                            .lines()
                            .find_map(|l| l.strip_prefix("Content-Length: "))
                            .and_then(|v| v.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        if request.len() >= end + 4 + length {
                            break;
                        }
                    }
                    if n == 0 {
                        break;
                    }
                }
                requests.push(String::from_utf8_lossy(&request).into_owned());
                stream.write_all(response.as_bytes()).unwrap();
            }
            requests
        })
    }

    fn reply(status: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
    }

    #[test]
    fn the_context_fits_prompts_whose_tokens_were_underestimated() {
        // Found evaluating without the fixed rules: a diff of SSH keys
        // estimated at 905 tokens was 1292, over the 1280 of a context
        // sized for the budget; across eval-v1 the estimate was at worst
        // 1.45 times short.
        let options = options(Path::new("/d"));
        assert_eq!(options.max_prompt_tokens, 1024);
        assert!(context_size(&options) >= 1292 + options.max_new_tokens);
        assert!(context_size(&options) >= 1024 * 145 / 100 + options.max_new_tokens);
    }

    #[test]
    fn generates_through_the_socket() {
        let dir = tempfile::TempDir::new().unwrap();
        let socket = dir.path().join("server.sock");
        let body = r#"{"choices":[{"message":{"content":"feat: add it"}}],"timings":{"prompt_n":10,"predicted_n":3}}"#;
        let server = fake_server(
            &socket,
            vec![
                reply("200 OK", body),
                reply("500 Internal Server Error", "boom"),
            ],
        );
        let backend = LlamaCppBackend::connect(socket, "fake".into());

        let text = backend
            .generate(&prompt(), &GenerationConfig::default())
            .unwrap();
        assert_eq!(text, "feat: add it");
        assert_eq!(backend.last_metrics().unwrap().input_tokens, 10);
        let err = backend
            .generate(&prompt(), &GenerationConfig::default())
            .unwrap_err();
        assert_eq!(
            err,
            BackendError::Generation("llama-server answered 500: boom".into())
        );

        let requests = server.join().unwrap();
        assert!(requests[0].starts_with("POST /v1/chat/completions HTTP/1.1\r\n"));
        assert!(
            requests[0].contains(r#""temperature":0"#),
            "{}",
            requests[0]
        );
    }

    #[test]
    fn missing_files_are_reported_before_starting() {
        let dir = tempfile::TempDir::new().unwrap();
        let options = options(dir.path());
        let err = LlamaCppBackend::start(&options).unwrap_err();
        assert!(err.to_string().contains("no GGUF model at"), "{err}");
        std::fs::write(&options.model, "gguf").unwrap();
        let err = LlamaCppBackend::start(&options).unwrap_err();
        assert!(err.to_string().contains("no llama-server at"), "{err}");
    }

    #[test]
    fn a_server_that_dies_is_reported_with_its_output() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut options = options(dir.path());
        std::fs::write(&options.model, "gguf").unwrap();
        let server = dir.path().join("fake-server");
        std::fs::write(
            &server,
            "#!/bin/sh\necho 'error: failed to load model' >&2\nexit 3\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&server, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        options.server = server;
        let err = LlamaCppBackend::start(&options).unwrap_err().to_string();
        assert!(err.contains("llama-server exited"), "{err}");
        assert!(err.contains("failed to load model"), "{err}");
    }

    #[test]
    fn description_names_build_device_and_model() {
        let dir = tempfile::TempDir::new().unwrap();
        let options = options(dir.path());
        assert_eq!(
            describe(&options),
            "llama.cpp server=llama-b11381 device=Vulkan1 model=model.gguf sha256=unverified ctx=1664"
        );
        std::fs::write(
            dir.path().join("model.gguf.sha256"),
            "cc324af070c2ecbfd324a30884d2f951  model.gguf\n",
        )
        .unwrap();
        assert!(describe(&options).contains("sha256=cc324af070c2ecbf"));
    }

    #[test]
    fn setup_script_installs_what_the_code_expects() {
        let script = include_str!("../scripts/setup-llamacpp.sh");
        assert!(script.contains(&format!("\nLLAMA_BUILD={LLAMA_CPP_BUILD}\n")));
    }

    #[test]
    fn socket_dirs_are_private_and_short() {
        let dir = private_dir().unwrap();
        assert!(dir.join("server.sock").as_os_str().len() < 108);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        std::fs::remove_dir(dir).unwrap();
    }
}

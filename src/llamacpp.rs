//! A second backend: llama.cpp's server with Vulkan, for the GPUs OpenVINO
//! cannot drive (AMD and NVIDIA, with proprietary or open-source drivers);
//! it runs on Intel GPUs too.
//!
//! Each generation session starts a private `llama-server`, listening on a
//! UNIX socket in a fresh temporary directory (nothing is exposed on the
//! network), and stops it when the backend is dropped. The HTTP client is a
//! minimal HTTP/1.1 one over that socket, so no network library is needed.

use std::fmt;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::backend::{Backend, BackendError, GenerationConfig, Metrics};
use crate::prompt::Prompt;

/// The llama.cpp release `smart-commit setup` installs.
pub const LLAMA_CPP_BUILD: &str = "b11381";
/// The GGUF model `smart-commit setup` downloads.
pub const DEFAULT_GGUF: &str = "qwen2.5-coder-1.5b-instruct-q4_k_m.gguf";

/// UNIX socket paths are limited to 108 bytes on Linux.
const MAX_SOCKET_PATH: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlamaCppOptions {
    pub server: PathBuf,
    /// A GGUF model; `<model>.sha256` next to it, if present, identifies it.
    pub model: PathBuf,
    /// Which Vulkan device (`VulkanN`, as `llama-server --list-devices`
    /// numbers them).
    pub vulkan_device: usize,
    pub max_prompt_tokens: usize,
    pub max_new_tokens: usize,
    pub startup_timeout: Duration,
}

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
        let dir =
            private_dir().map_err(|e| unavailable(format!("cannot create a socket dir: {e}")))?;
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
pub fn server_args(options: &LlamaCppOptions, socket: &Path) -> Vec<String> {
    let context = options.max_prompt_tokens + options.max_new_tokens;
    [
        "--model",
        &options.model.to_string_lossy(),
        // A path ending in `.sock` makes llama-server listen on a UNIX socket.
        "--host",
        &socket.to_string_lossy(),
        "--n-gpu-layers",
        "999",
        "--device",
        &format!("Vulkan{}", options.vulkan_device),
        "--ctx-size",
        &context.to_string(),
        "--parallel",
        "1",
        "--no-webui",
        "--offline",
    ]
    .iter()
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
fn describe(options: &LlamaCppOptions) -> String {
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
    format!(
        "llama.cpp server={build} device=Vulkan{} model={} sha256={identity} ctx={}",
        options.vulkan_device,
        name(&options.model),
        options.max_prompt_tokens + options.max_new_tokens
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
            vulkan_device: 1,
            max_prompt_tokens: 1024,
            max_new_tokens: 128,
            startup_timeout: Duration::from_secs(5),
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
        assert!(joined.contains("--ctx-size 1152"));
        assert!(joined.contains("--n-gpu-layers 999"));
        assert!(joined.contains("--offline"));
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
            "llama.cpp server=llama-b11381 device=Vulkan1 model=model.gguf sha256=unverified ctx=1152"
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
        let script = include_str!("../scripts/setup-gpu.sh");
        assert!(script.contains(&format!("\nLLAMA_BUILD={LLAMA_CPP_BUILD}\n")));
        assert!(script.contains(&format!("\nGGUF_NAME={DEFAULT_GGUF}\n")));
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

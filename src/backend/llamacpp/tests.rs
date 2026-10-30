use std::io::{Read, Write};
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
fn a_program_still_open_for_writing_is_started_when_it_is_free() {
    use std::io::Write as _;
    use std::os::unix::fs::PermissionsExt as _;
    // A program just written, with a process forked meanwhile that still holds
    // the file open for writing: the first start fails with "text file busy"
    // (the flake of the tests that write a script and run it at once).
    let dir = tempfile::TempDir::new().unwrap();
    let script = dir.path().join("program");
    let mut open = std::fs::File::create(&script).unwrap();
    open.write_all(b"#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let err = Command::new(&script).spawn().unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::ExecutableFileBusy, "{err}");
    let closer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(80));
        drop(open);
    });
    let mut child = spawn_when_free(&mut Command::new(&script)).unwrap();
    assert!(child.wait().unwrap().success());
    closer.join().unwrap();
    // Any other error is the first one, at once.
    let err = spawn_when_free(&mut Command::new(dir.path().join("absent"))).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
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
fn extra_flags_reach_the_server_after_ours() {
    let dir = tempfile::TempDir::new().unwrap();
    let mut options = options(dir.path());
    std::fs::write(&options.model, "gguf").unwrap();
    let server = dir.path().join("fake-server");
    std::fs::write(&server, "#!/bin/sh\necho \"flags: $*\" >&2\nexit 3\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&server, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    options.server = server;
    let extra = ["-b".to_owned(), "1024".to_owned()];
    let err = LlamaCppBackend::start_with(&options, None, &extra)
        .unwrap_err()
        .to_string();
    assert!(err.contains("--offline -b 1024"), "{err}");
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

#[test]
fn a_reply_can_be_begun_for_the_model() {
    let request =
        chat_request_starting_with(&prompt(), &GenerationConfig { max_new_tokens: 96 }, "fix");
    let messages = request["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[2]["role"], "assistant");
    assert_eq!(messages[2]["content"], "fix");
    // Everything else is as greedy and seeded as a plain request.
    assert_eq!(request["temperature"], 0);
    assert_eq!(request["max_tokens"], 96);
    assert_eq!(request["cache_prompt"], false);
}

#[test]
fn the_type_is_asked_with_one_token_and_its_alternatives() {
    let request = type_request(&prompt());
    assert_eq!(request["max_tokens"], 1);
    assert_eq!(request["logprobs"], true);
    assert_eq!(request["top_logprobs"], 12);
    assert_eq!(request["temperature"], 0);
    assert_eq!(request["messages"].as_array().unwrap().len(), 2);
}

#[test]
fn types_are_read_from_the_first_tokens_candidates() {
    let body = r#"{"choices":[{"logprobs":{"content":[{"token":"feat","logprob":-0.654,
        "top_logprobs":[
          {"token":"feat","logprob":-0.654},{"token":"fix","logprob":-0.894},
          {"token":"ch","logprob":-3.453},{"token":"ref","logprob":-3.697},
          {"token":"Fix","logprob":-6.878},{"token":"(","logprob":-9.0}]}]}}]}"#;
    let choices = parse_type_choices(body).unwrap();
    let names: Vec<&str> = choices.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["feat", "fix", "chore", "refactor"]);
    let total: f64 = choices.iter().map(|c| c.probability).sum();
    assert!((total - 1.0).abs() < 1e-9);
    assert!(choices[0].probability > choices[1].probability);

    let err = parse_type_choices(r#"{"choices":[{"message":{"content":"feat"}}]}"#).unwrap_err();
    assert!(err.to_string().contains("no log probabilities"), "{err}");
    assert!(parse_type_choices("not json").is_err());
}

#[test]
fn types_and_a_chosen_start_go_through_the_socket() {
    let dir = tempfile::TempDir::new().unwrap();
    let socket = dir.path().join("server.sock");
    let choices = r#"{"choices":[{"logprobs":{"content":[{"token":"feat","logprob":-0.7,
        "top_logprobs":[{"token":"feat","logprob":-0.7},{"token":"fix","logprob":-0.9}]}]}}]}"#;
    let reply_body = r#"{"choices":[{"message":{"content":"fix(core): stop the crash"}}],
        "timings":{"prompt_n":10,"predicted_n":6}}"#;
    let server = fake_server(
        &socket,
        vec![reply("200 OK", choices), reply("200 OK", reply_body)],
    );
    let backend = LlamaCppBackend::connect(socket, "fake".into());

    let types = backend.type_choices(&prompt()).unwrap();
    assert_eq!(types[0].name, "feat");
    assert_eq!(types[1].name, "fix");
    let text = backend
        .generate_starting_with(&prompt(), "fix", &GenerationConfig::default())
        .unwrap();
    assert_eq!(text, "fix(core): stop the crash");
    assert_eq!(backend.last_metrics().unwrap().output_tokens, 6);

    let requests = server.join().unwrap();
    assert!(
        requests[0].contains(r#""logprobs":true"#),
        "{}",
        requests[0]
    );
    // The last message is the assistant's, begun with the type (the order
    // of an object's keys depends on the features serde_json is built with).
    let body: Value = serde_json::from_str(requests[1].split("\r\n\r\n").nth(1).unwrap()).unwrap();
    let last = body["messages"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last, json!({"role": "assistant", "content": "fix"}));
}

#[test]
fn a_frozen_server_is_not_healthy_and_does_not_hang_the_check() {
    let dir = tempfile::TempDir::new().unwrap();
    let socket = dir.path().join("server.sock");
    // Accepts the connection and never replies, like a hung GPU driver.
    let listener = UnixListener::bind(&socket).unwrap();
    let held = std::thread::spawn(move || listener.accept().map(|(stream, _)| stream));
    let started = std::time::Instant::now();
    assert!(!healthy_within(&socket, Duration::from_millis(200)));
    assert!(started.elapsed() >= Duration::from_millis(150));
    assert!(started.elapsed() < Duration::from_secs(5));
    drop(held);
}

#[test]
fn a_server_that_answers_is_healthy_and_a_missing_one_is_not() {
    let dir = tempfile::TempDir::new().unwrap();
    let socket = dir.path().join("server.sock");
    assert!(!healthy_within(&socket, HEALTH_TIMEOUT));
    let listener = UnixListener::bind(&socket).unwrap();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0; 1024];
        let _ = stream.read(&mut buf);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}")
            .unwrap();
    });
    assert!(healthy_within(&socket, HEALTH_TIMEOUT));
}

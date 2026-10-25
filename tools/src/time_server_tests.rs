use std::sync::Mutex;

use super::{ServerTiming, parse_prompt_file, read_prompts, time_prompts};
use smart_commit::backend::{Backend, BackendError, GenerationConfig, Metrics};
use smart_commit::prompt::Prompt;

const PRINTED: &str = "===== system =====\nRules.\n\n===== user =====\nfile | +1 -0\n\n--- file\n+x\n===== ~12 of 1024 tokens =====\n";

fn prompt() -> Prompt {
    Prompt {
        system: "Rules.\n".into(),
        user: "file | +1 -0\n\n--- file\n+x".into(),
    }
}

#[test]
fn a_printed_prompt_is_read_back() {
    assert_eq!(parse_prompt_file(PRINTED), Some(prompt()));
}

#[test]
fn text_that_is_not_a_printed_prompt_is_refused() {
    assert_eq!(parse_prompt_file(""), None);
    assert_eq!(parse_prompt_file("===== system =====\nonly\n"), None);
}

#[test]
fn prompts_are_read_in_name_order_from_txt_files() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(dir.path().join("b.txt"), PRINTED).unwrap();
    std::fs::write(dir.path().join("a.txt"), PRINTED).unwrap();
    std::fs::write(dir.path().join("c.md"), "ignored").unwrap();
    let prompts = read_prompts(dir.path()).unwrap();
    let names: Vec<&str> = prompts.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["a.txt", "b.txt"]);
}

#[test]
fn a_directory_without_prompts_or_with_other_text_is_an_error() {
    let dir = tempfile::TempDir::new().unwrap();
    assert!(read_prompts(dir.path()).unwrap_err().contains("no *.txt"));
    std::fs::write(dir.path().join("x.txt"), "hello").unwrap();
    assert!(
        read_prompts(dir.path())
            .unwrap_err()
            .contains("not the output of")
    );
    assert!(read_prompts(&dir.path().join("missing")).is_err());
}

/// Replies with the nth timing, as llama-server reports them.
struct Timed {
    replies: Mutex<Vec<(&'static str, Metrics)>>,
    last: Mutex<Option<Metrics>>,
}

fn metrics(input: usize, prefill: f32, output: usize, decode: f32) -> Metrics {
    Metrics {
        input_tokens: input,
        output_tokens: output,
        load_ms: 0.0,
        time_to_first_token_ms: prefill,
        time_per_output_token_ms: 0.0,
        generate_ms: prefill + decode,
    }
}

impl Backend for Timed {
    fn describe(&self) -> String {
        "timed".into()
    }

    fn generate(&self, _: &Prompt, _: &GenerationConfig) -> Result<String, BackendError> {
        let (reply, metrics) = self.replies.lock().unwrap().remove(0);
        *self.last.lock().unwrap() = Some(metrics);
        Ok(reply.to_owned())
    }

    fn last_metrics(&self) -> Option<Metrics> {
        self.last.lock().unwrap().clone()
    }
}

fn timed(replies: Vec<(&'static str, Metrics)>) -> Timed {
    Timed {
        replies: Mutex::new(replies),
        last: Mutex::new(None),
    }
}

#[test]
fn prefill_and_decode_are_told_apart() {
    let backend = timed(vec![(
        "  feat: add x\nbody\n",
        metrics(300, 1000.0, 12, 500.0),
    )]);
    let prompts = [("a.txt".to_owned(), prompt())];
    let timings = time_prompts(&backend, &prompts, &GenerationConfig::default()).unwrap();
    assert_eq!(timings[0].input_tokens, 300);
    assert_eq!(timings[0].prefill_ms, 1000.0);
    assert_eq!(timings[0].decode_ms, 500.0);
    assert_eq!(timings[0].first_line, "feat: add x");
}

#[test]
fn a_backend_without_timings_is_an_error() {
    let backend = smart_commit::backend::MockBackend::new(["feat: x"]);
    let prompts = [("a.txt".to_owned(), prompt())];
    assert!(time_prompts(&backend, &prompts, &GenerationConfig::default()).is_err());
}

#[test]
fn the_report_has_the_mean_and_a_line_per_prompt() {
    let backend = timed(vec![
        ("feat: a", metrics(300, 1000.0, 12, 400.0)),
        ("fix: b", metrics(500, 3000.0, 10, 600.0)),
    ]);
    let prompts = [
        ("a.txt".to_owned(), prompt()),
        ("b.txt".to_owned(), prompt()),
    ];
    let timing = ServerTiming {
        load_ms: 300.0,
        prompts: time_prompts(&backend, &prompts, &GenerationConfig::default()).unwrap(),
    };
    let text = timing.render("cpu");
    assert_eq!(
        text.lines().next().unwrap(),
        "== cpu: load 300 ms, prefill 2000 ms, decode 500 ms, total/msg 2800 ms"
    );
    assert!(text.contains("a.txt          in= 300   1000ms out= 12    400ms  feat: a"));
    assert!(text.contains("b.txt          in= 500   3000ms out= 10    600ms  fix: b"));
}

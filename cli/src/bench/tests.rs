use std::time::Instant;

use super::*;
use crate::backend::MockBackend;
use crate::backend::{Backend, BackendError, GenerationConfig};

fn entry(target: Target, per_message_ms: Option<f64>) -> Entry {
    Entry {
        target,
        backend: per_message_ms.map(|_| "b".to_owned()),
        measurement: per_message_ms.map(|ms| Measurement {
            load_ms: ms / 2.0,
            generate_ms: ms / 2.0,
            per_message_ms: ms,
            cpu_cores: 1.25,
            replies: vec![],
        }),
        error: per_message_ms
            .is_none()
            .then(|| "backend unavailable: no runtime\nmore".to_owned()),
        fast_load: None,
    }
}

fn report_of(entries: Vec<Entry>) -> Report {
    Report {
        format: FORMAT,
        tool_version: "0".into(),
        machine: Machine {
            cpu: "cpu".into(),
            kernel: "k".into(),
        },
        entries,
    }
}

#[test]
fn embedded_prompts_are_sized_like_real_ones() {
    let prompts = prompts();
    assert_eq!(prompts.len(), 4);
    assert!(
        prompts
            .iter()
            .all(|p| !p.system.is_empty() && !p.user.is_empty())
    );
    // From about 300 tokens to the 1024-token budget: the fixtures of
    // the other tests are under 300, which underestimates prefill.
    let tokens: Vec<usize> = prompts
        .iter()
        .map(|p| crate::diff::estimate_tokens(&p.system) + crate::diff::estimate_tokens(&p.user))
        .collect();
    assert!(tokens.windows(2).all(|w| w[0] < w[1]), "{tokens:?}");
    assert!(tokens[0] >= 300 && tokens[3] >= 850, "{tokens:?}");
}

#[test]
fn ranking_is_fastest_first_and_skips_failures() {
    let report = report_of(vec![
        entry(Target::IntelGpu, None),
        entry(Target::Npu, Some(5100.0)),
        entry(Target::Vulkan, Some(2400.0)),
        entry(Target::Cpu, Some(4000.0)),
    ]);
    assert_eq!(report.ranking(), [Target::Vulkan, Target::Cpu, Target::Npu]);
    assert_eq!(report.fastest(), Some(Target::Vulkan));
    assert_eq!(
        report_of(vec![entry(Target::IntelGpu, None)]).fastest(),
        None
    );
}

#[test]
fn table_marks_the_selection_and_explains_failures() {
    let report = report_of(vec![
        entry(Target::IntelGpu, Some(1800.0)),
        entry(Target::Npu, None),
        entry(Target::OpenvinoCpu, Some(2100.0)),
    ]);
    assert_eq!(
        report.table(Some(Target::IntelGpu)),
        "  device       per message     load  generate     CPU\n  \
         intel-gpu          1.8 s    0.9 s     0.9 s    1.2x  <- selected\n  \
         npu          unavailable: backend unavailable: no runtime\n  \
         openvino-cpu       2.1 s    1.1 s     1.1 s    1.2x\n"
    );
}

#[test]
fn reports_round_trip_and_other_formats_are_ignored() {
    let report = report_of(vec![entry(Target::Cpu, Some(4000.0))]);
    assert_eq!(Report::from_json(&report.to_json()).unwrap(), report);
    // Reports of 0.3 and earlier name devices that changed meaning.
    let old = report.to_json().replace("\"format\": 2", "\"format\": 1");
    assert_eq!(Report::from_json(&old), None);
    assert_eq!(Report::from_json("{"), None);
}

#[test]
fn measure_warms_up_then_times_every_prompt() {
    let mut loads = 0;
    let entry = measure(
        Target::Cpu,
        &prompts(),
        &GenerationConfig::default(),
        |_| {
            loads += 1;
            Ok(Box::new(MockBackend::new([
                "feat: a", "feat: b", "c", "d", "e", "f", "g",
            ])))
        },
    );
    assert_eq!(loads, 2, "one warm-up load and one timed load");
    let m = entry.measurement.unwrap();
    // One reply per embedded prompt.
    assert_eq!(m.replies, ["feat: a", "feat: b", "c", "d"]);
    assert!((m.per_message_ms - (m.load_ms + m.generate_ms)).abs() < 1e-9);
    assert_eq!(entry.backend.as_deref(), Some("mock"));
}

#[test]
fn measure_reports_load_failures() {
    let entry = measure(
        Target::Npu,
        &prompts(),
        &GenerationConfig::default(),
        |_| Err(BackendError::Unavailable("no NPU".into())),
    );
    assert_eq!(entry.error.as_deref(), Some("backend unavailable: no NPU"));
    assert!(entry.measurement.is_none());
}

#[test]
fn fast_load_needs_a_real_gain() {
    let on = FastLoadTrial::new(Some(2800.0), Some(1900.0));
    assert!(on.enabled);
    assert_eq!(
        on.note(Target::Cpu),
        "fast load turned on for cpu (--no-repack --fit off): 2.8 s -> 1.9 s per message"
    );
    // 4% is within the noise.
    let small = FastLoadTrial::new(Some(2500.0), Some(2400.0));
    assert!(!small.enabled);
    assert_eq!(
        small.note(Target::Vulkan),
        "fast load left off for vulkan: 2.5 s -> 2.4 s per message, under 5% faster"
    );
    assert!(
        FastLoadTrial::new(Some(2000.0), Some(1900.0)).enabled,
        "exactly 5%"
    );
    assert!(!FastLoadTrial::new(Some(2000.0), Some(2100.0)).enabled);

    let failed = FastLoadTrial::new(Some(2500.0), None);
    assert!(!failed.enabled);
    assert_eq!(
        failed.note(Target::Vulkan),
        "fast load left off for vulkan: it failed to load"
    );
    let only = FastLoadTrial::new(None, Some(2500.0));
    assert!(only.enabled);
    assert!(only.note(Target::Cpu).ends_with("it only loads that way"));
    assert!(!FastLoadTrial::new(None, None).enabled);
}

/// A backend that sleeps for `load_ms` when created, like a slow load.
fn slow_load(load_ms: u64) -> Result<Box<dyn Backend>, BackendError> {
    std::thread::sleep(std::time::Duration::from_millis(load_ms));
    Ok(Box::new(MockBackend::new(["feat: a"; 5])))
}

#[test]
fn measure_load_modes_keeps_the_faster_mode() {
    let mut modes = Vec::new();
    let entry = measure_load_modes(
        Target::Cpu,
        &prompts(),
        &GenerationConfig::default(),
        |_, fast| {
            modes.push(fast);
            slow_load(if fast { 10 } else { 300 })
        },
    );
    assert_eq!(
        modes,
        [false, false, true, true],
        "warm-up and timed load each"
    );
    let mut phases = Vec::new();
    measure_load_modes_with(
        Target::Cpu,
        &prompts(),
        &GenerationConfig::default(),
        |_, _| slow_load(0),
        |p| phases.push(p),
    );
    assert_eq!(
        phases,
        ["measuring, normal load", "measuring, with fast load"]
    );
    let trial = entry.fast_load.unwrap();
    assert!(trial.enabled);
    let m = entry.measurement.unwrap();
    assert_eq!(Some(m.per_message_ms), trial.fast_ms);
    assert!(m.load_ms < 300.0);

    let entry = measure_load_modes(
        Target::Vulkan,
        &prompts(),
        &GenerationConfig::default(),
        |_, fast| {
            if fast {
                Err(BackendError::Unavailable("out of device memory".into()))
            } else {
                slow_load(0)
            }
        },
    );
    assert!(!entry.fast_load.as_ref().unwrap().enabled);
    assert!(entry.measurement.is_some(), "the normal load is kept");
    assert_eq!(entry.error, None);
}

#[test]
fn reports_know_where_fast_load_is_on() {
    let mut cpu = entry(Target::Cpu, Some(1900.0));
    cpu.fast_load = Some(FastLoadTrial::new(Some(2800.0), Some(1900.0)));
    let mut vulkan = entry(Target::Vulkan, Some(2500.0));
    vulkan.fast_load = Some(FastLoadTrial::new(Some(2500.0), Some(2400.0)));
    let report = report_of(vec![entry(Target::Npu, Some(5100.0)), cpu, vulkan]);
    assert!(report.fast_load(Target::Cpu));
    assert!(!report.fast_load(Target::Vulkan));
    assert!(!report.fast_load(Target::Npu));
    assert!(!report.fast_load(Target::IntelGpu), "not measured");
    assert_eq!(report.fast_load_notes().len(), 2);
    // Nothing to say about a device that failed in both modes.
    let mut failed = entry(Target::Cpu, None);
    failed.fast_load = Some(FastLoadTrial::new(None, None));
    assert!(report_of(vec![failed]).fast_load_notes().is_empty());
    assert_eq!(Report::from_json(&report.to_json()).unwrap(), report);
    // Devices without a trial do not write the field.
    let npu = report_of(vec![entry(Target::Npu, Some(5100.0))]).to_json();
    assert!(!npu.contains("fast_load"), "{npu}");
}

#[test]
fn cpu_time_grows_with_work() {
    let before = cpu_time_ms();
    let mut x = 0u64;
    let start = Instant::now();
    while start.elapsed().as_millis() < 60 {
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
    }
    std::hint::black_box(x);
    assert!(cpu_time_ms() > before);
}

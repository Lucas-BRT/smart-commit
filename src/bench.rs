//! Measures every device on fixed prompts, so `model.device = "auto"` can
//! pick the fastest one on this machine.
//!
//! The prompts are recorded in `tests/fixtures/traces/bench-*.json`, embedded
//! in the binary, so every machine is measured on the same inputs. They are
//! sized like real prompts (the quartiles and 90th percentile of eval-v1's),
//! since reading the prompt is most of the time on the CPU. Each device
//! is loaded once to warm the compilation caches, then loaded again and
//! timed: a commit pays one load plus one generation. The llama.cpp devices
//! are measured twice, with and without fast load, and keep the faster.

use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::backend::{Backend, BackendError, GenerationConfig};
use crate::device::Target;
use crate::prompt::Prompt;
use crate::trace::Trace;

/// Bumped on incompatible changes to the saved report. 2: the devices of
/// 0.4 (`cpu` moved to llama.cpp, `gpu` became `intel-gpu`).
pub const FORMAT: u32 = 2;

/// Fast load must save at least this share of the time per message, more
/// than the run-to-run noise, to be turned on.
pub const FAST_LOAD_MIN_GAIN: f64 = 0.05;

const FIXTURES: &[&str] = &[
    include_str!("../tests/fixtures/traces/bench-p25.json"),
    include_str!("../tests/fixtures/traces/bench-p50.json"),
    include_str!("../tests/fixtures/traces/bench-p75.json"),
    include_str!("../tests/fixtures/traces/bench-p90.json"),
];

/// Every prompt of the embedded fixtures, in a fixed order.
pub fn prompts() -> Vec<Prompt> {
    FIXTURES
        .iter()
        .flat_map(|json| {
            Trace::from_json(json)
                .expect("embedded fixtures are valid")
                .generation
                .attempts
                .into_iter()
                .map(|a| a.prompt)
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Measurement {
    /// Loading the model with warm caches.
    pub load_ms: f64,
    /// Mean time to generate one message.
    pub generate_ms: f64,
    /// What a commit costs: one load and one generation.
    pub per_message_ms: f64,
    /// Mean busy CPU cores while measuring (CPU time / wall time).
    pub cpu_cores: f64,
    pub replies: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub target: Target,
    /// The backend's own description, when it loaded.
    pub backend: Option<String>,
    pub measurement: Option<Measurement>,
    pub error: Option<String>,
    /// For llama.cpp devices: both load modes, `measurement` being the one
    /// chosen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fast_load: Option<FastLoadTrial>,
}

/// Time per message with and without fast load; `None` when that mode
/// failed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FastLoadTrial {
    pub normal_ms: Option<f64>,
    pub fast_ms: Option<f64>,
    pub enabled: bool,
}

impl FastLoadTrial {
    /// Picks fast load when it alone worked, or saved at least
    /// [`FAST_LOAD_MIN_GAIN`].
    pub fn new(normal_ms: Option<f64>, fast_ms: Option<f64>) -> Self {
        let enabled = match (normal_ms, fast_ms) {
            (Some(normal), Some(fast)) => fast <= normal * (1.0 - FAST_LOAD_MIN_GAIN),
            (None, Some(_)) => true,
            _ => false,
        };
        Self {
            normal_ms,
            fast_ms,
            enabled,
        }
    }

    /// What the benchmark decided, for the user.
    pub fn note(&self, target: Target) -> String {
        let s = |ms: f64| format!("{:.1} s", ms / 1000.0);
        let flags = "--no-repack --fit off";
        match (self.normal_ms, self.fast_ms, self.enabled) {
            (Some(normal), Some(fast), true) => format!(
                "fast load turned on for {target} ({flags}): {} -> {} per message",
                s(normal),
                s(fast)
            ),
            (None, Some(_), _) => {
                format!("fast load turned on for {target} ({flags}): it only loads that way")
            }
            (Some(normal), Some(fast), false) => format!(
                "fast load left off for {target}: {} -> {} per message, under {:.0}% faster",
                s(normal),
                s(fast),
                FAST_LOAD_MIN_GAIN * 100.0
            ),
            _ => format!("fast load left off for {target}: it failed to load"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub format: u32,
    pub tool_version: String,
    pub machine: Machine,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Machine {
    pub cpu: String,
    pub kernel: String,
}

impl Machine {
    pub fn current() -> Self {
        let cpu = std::fs::read_to_string("/proc/cpuinfo")
            .ok()
            .and_then(|info| {
                info.lines()
                    .find_map(|l| l.strip_prefix("model name"))
                    .map(|l| l.trim_start_matches([' ', '\t', ':']).trim().to_owned())
            })
            .unwrap_or_else(|| "unknown".to_owned());
        let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .map(|k| k.trim().to_owned())
            .unwrap_or_else(|_| "unknown".to_owned());
        Self { cpu, kernel }
    }
}

impl Report {
    /// Devices that worked, fastest first; ties keep the measuring order.
    pub fn ranking(&self) -> Vec<Target> {
        let mut ok: Vec<(&Entry, f64)> = self
            .entries
            .iter()
            .filter_map(|e| e.measurement.as_ref().map(|m| (e, m.per_message_ms)))
            .collect();
        ok.sort_by(|a, b| a.1.total_cmp(&b.1));
        ok.into_iter().map(|(e, _)| e.target).collect()
    }

    pub fn fastest(&self) -> Option<Target> {
        self.ranking().first().copied()
    }

    pub fn entry(&self, target: Target) -> Option<&Entry> {
        self.entries.iter().find(|e| e.target == target)
    }

    /// Whether the benchmark turned fast load on for `target`.
    pub fn fast_load(&self, target: Target) -> bool {
        self.entry(target)
            .and_then(|e| e.fast_load.as_ref())
            .is_some_and(|f| f.enabled)
    }

    /// One line per device that worked, measured in both load modes.
    pub fn fast_load_notes(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|e| e.measurement.is_some())
            .filter_map(|e| e.fast_load.as_ref().map(|f| f.note(e.target)))
            .collect()
    }

    pub fn to_json(&self) -> String {
        let mut json = serde_json::to_string_pretty(self).expect("reports serialize");
        json.push('\n');
        json
    }

    /// `None` for a report of another format: it is simply ignored.
    pub fn from_json(json: &str) -> Option<Self> {
        serde_json::from_str::<Self>(json)
            .ok()
            .filter(|r| r.format == FORMAT)
    }

    /// The table shown to the user, marking `selected`.
    pub fn table(&self, selected: Option<Target>) -> String {
        let mut out = format!(
            "  {:<12} {:>11} {:>8} {:>9} {:>7}\n",
            "device", "per message", "load", "generate", "CPU"
        );
        for entry in &self.entries {
            let mark = if Some(entry.target) == selected {
                "  <- selected"
            } else {
                ""
            };
            match (&entry.measurement, &entry.error) {
                (Some(m), _) => out.push_str(&format!(
                    "  {:<12} {:>11} {:>8} {:>9} {:>7}{mark}\n",
                    entry.target.name(),
                    seconds(m.per_message_ms),
                    seconds(m.load_ms),
                    seconds(m.generate_ms),
                    format!("{:.1}x", m.cpu_cores),
                )),
                (None, error) => out.push_str(&format!(
                    "  {:<12} unavailable: {}\n",
                    entry.target.name(),
                    first_line(error.as_deref().unwrap_or("unknown error"))
                )),
            }
        }
        out
    }
}

fn seconds(ms: f64) -> String {
    format!("{:.1} s", ms / 1000.0)
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or(text)
}

/// CPU time used by this process and its waited-for children (such as
/// llama-server), in milliseconds; from `/proc/self/stat`.
pub fn cpu_time_ms() -> f64 {
    let Ok(stat) = std::fs::read_to_string("/proc/self/stat") else {
        return 0.0;
    };
    // Fields after the parenthesised command name; utime is field 14.
    let Some(rest) = stat.rsplit_once(')').map(|(_, r)| r) else {
        return 0.0;
    };
    let fields: Vec<f64> = rest
        .split_whitespace()
        .skip(11)
        .take(4)
        .filter_map(|f| f.parse().ok())
        .collect();
    // Clock ticks; 100 per second on every Linux build that matters.
    fields.iter().sum::<f64>() * 10.0
}

/// Measures one device: a warm-up load, then a timed load and every prompt.
pub fn measure(
    target: Target,
    prompts: &[Prompt],
    config: &GenerationConfig,
    mut load: impl FnMut(Target) -> Result<Box<dyn Backend>, BackendError>,
) -> Entry {
    let failed = |error: String| Entry {
        target,
        backend: None,
        measurement: None,
        error: Some(error),
        fast_load: None,
    };
    // Warm-up: compiles and caches the model, which only the first run pays.
    match load(target) {
        Ok(backend) => drop(backend),
        Err(err) => return failed(err.to_string()),
    }

    let cpu_start = cpu_time_ms();
    let wall_start = Instant::now();
    let backend = match load(target) {
        Ok(backend) => backend,
        Err(err) => return failed(err.to_string()),
    };
    let load_ms = wall_start.elapsed().as_secs_f64() * 1000.0;
    let description = backend.describe();
    let mut replies = Vec::new();
    let generate_start = Instant::now();
    for prompt in prompts {
        match backend.generate(prompt, config) {
            Ok(reply) => replies.push(reply),
            Err(err) => return failed(err.to_string()),
        }
    }
    let generate_ms = generate_start.elapsed().as_secs_f64() * 1000.0 / prompts.len().max(1) as f64;
    drop(backend);
    let wall_ms = wall_start.elapsed().as_secs_f64() * 1000.0;
    let cpu_ms = cpu_time_ms() - cpu_start;
    Entry {
        target,
        backend: Some(description),
        measurement: Some(Measurement {
            load_ms,
            generate_ms,
            per_message_ms: load_ms + generate_ms,
            cpu_cores: if wall_ms > 0.0 { cpu_ms / wall_ms } else { 0.0 },
            replies,
        }),
        error: None,
        fast_load: None,
    }
}

/// Measures a llama.cpp device without and with fast load (`load`'s second
/// argument) and keeps the mode [`FastLoadTrial`] picks.
pub fn measure_load_modes(
    target: Target,
    prompts: &[Prompt],
    config: &GenerationConfig,
    load: impl FnMut(Target, bool) -> Result<Box<dyn Backend>, BackendError>,
) -> Entry {
    measure_load_modes_with(target, prompts, config, load, |_| {})
}

/// Like [`measure_load_modes`], telling `phase` which mode it measures.
pub fn measure_load_modes_with(
    target: Target,
    prompts: &[Prompt],
    config: &GenerationConfig,
    mut load: impl FnMut(Target, bool) -> Result<Box<dyn Backend>, BackendError>,
    mut phase: impl FnMut(&'static str),
) -> Entry {
    phase("measuring, normal load");
    let normal = measure(target, prompts, config, |t| load(t, false));
    phase("measuring, with fast load");
    let fast = measure(target, prompts, config, |t| load(t, true));
    let per_message = |e: &Entry| e.measurement.as_ref().map(|m| m.per_message_ms);
    let trial = FastLoadTrial::new(per_message(&normal), per_message(&fast));
    let chosen = if trial.enabled { fast } else { normal };
    Entry {
        fast_load: Some(trial),
        ..chosen
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::MockBackend;

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
            .map(|p| {
                crate::diff::estimate_tokens(&p.system) + crate::diff::estimate_tokens(&p.user)
            })
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
}

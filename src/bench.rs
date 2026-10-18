//! Measures every device on fixed prompts, so `model.device = "auto"` can
//! pick the fastest one on this machine.
//!
//! The prompts are the ones recorded in `tests/fixtures/traces`, embedded in
//! the binary, so every machine is measured on the same inputs. Each device
//! is loaded once to warm the compilation caches, then loaded again and
//! timed: a commit pays one load plus one generation.

use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::backend::{Backend, BackendError, GenerationConfig};
use crate::device::Target;
use crate::prompt::Prompt;
use crate::trace::Trace;

/// Bumped on incompatible changes to the saved report.
pub const FORMAT: u32 = 1;

const FIXTURES: &[&str] = &[
    include_str!("../tests/fixtures/traces/conventional-pt-retry.json"),
    include_str!("../tests/fixtures/traces/free-en-chatty.json"),
    include_str!("../tests/fixtures/traces/gitmoji-fallback.json"),
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
            "  {:<8} {:>11} {:>8} {:>9} {:>7}\n",
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
                    "  {:<8} {:>11} {:>8} {:>9} {:>7}{mark}\n",
                    entry.target.name(),
                    seconds(m.per_message_ms),
                    seconds(m.load_ms),
                    seconds(m.generate_ms),
                    format!("{:.1}x", m.cpu_cores),
                )),
                (None, error) => out.push_str(&format!(
                    "  {:<8} unavailable: {}\n",
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
    fn embedded_prompts_are_the_fixtures() {
        let prompts = prompts();
        assert_eq!(prompts.len(), 5);
        assert!(
            prompts
                .iter()
                .all(|p| !p.system.is_empty() && !p.user.is_empty())
        );
    }

    #[test]
    fn ranking_is_fastest_first_and_skips_failures() {
        let report = report_of(vec![
            entry(Target::Gpu, None),
            entry(Target::Npu, Some(5100.0)),
            entry(Target::Vulkan, Some(2400.0)),
            entry(Target::Cpu, Some(4000.0)),
        ]);
        assert_eq!(report.ranking(), [Target::Vulkan, Target::Cpu, Target::Npu]);
        assert_eq!(report.fastest(), Some(Target::Vulkan));
        assert_eq!(report_of(vec![entry(Target::Gpu, None)]).fastest(), None);
    }

    #[test]
    fn table_marks_the_selection_and_explains_failures() {
        let report = report_of(vec![
            entry(Target::Gpu, Some(1800.0)),
            entry(Target::Npu, None),
        ]);
        assert_eq!(
            report.table(Some(Target::Gpu)),
            "  device   per message     load  generate     CPU\n  \
             gpu            1.8 s    0.9 s     0.9 s    1.2x  <- selected\n  \
             npu      unavailable: backend unavailable: no runtime\n"
        );
    }

    #[test]
    fn reports_round_trip_and_other_formats_are_ignored() {
        let report = report_of(vec![entry(Target::Cpu, Some(4000.0))]);
        assert_eq!(Report::from_json(&report.to_json()).unwrap(), report);
        let other = report.to_json().replace("\"format\": 1", "\"format\": 2");
        assert_eq!(Report::from_json(&other), None);
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
        assert_eq!(m.replies, ["feat: a", "feat: b", "c", "d", "e"]);
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

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

mod measure;
mod report;
#[cfg(test)]
mod tests;

pub use measure::{cpu_time_ms, measure, measure_load_modes, measure_load_modes_with};
pub use report::{Machine, Report};

use serde::{Deserialize, Serialize};

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
    include_str!("../../tests/fixtures/traces/bench-p25.json"),
    include_str!("../../tests/fixtures/traces/bench-p50.json"),
    include_str!("../../tests/fixtures/traces/bench-p75.json"),
    include_str!("../../tests/fixtures/traces/bench-p90.json"),
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

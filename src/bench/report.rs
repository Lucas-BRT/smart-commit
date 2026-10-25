use serde::{Deserialize, Serialize};

use super::*;
use crate::device::Target;

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

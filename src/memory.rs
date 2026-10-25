//! How much of the machine's RAM a model kept loaded may take.
//!
//! A llama-server kept after a commit holds its model's memory until it is
//! idle for `model.keep_loaded` minutes. On a machine with little RAM that
//! is a lot, so a model is kept only if it fits with room to spare, and a
//! kept one stops when the machine runs short or it grows past its share.

use std::fmt;
use std::path::Path;

/// What a llama-server takes over its model file, beyond the weights:
/// buffers for the context and the batch, and the libraries. Measured on
/// the CPU (`docs/measurements.md#keeping-the-model-loaded`): about 195 MB
/// for the 0.5B model and 250 MB for the 1.5B.
pub const OVERHEAD_BYTES: u64 = 256 * 1024 * 1024;

/// RAM that must stay available, as a share of the total, after a model is
/// loaded to be kept, and while it is kept.
pub const HEADROOM_PERCENT: u64 = 10;

/// The machine's RAM, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Memory {
    pub total: u64,
    /// What can be used without swapping (`MemAvailable`).
    pub available: u64,
}

impl Memory {
    /// From the text of `/proc/meminfo`: `None` without both `MemTotal`
    /// and `MemAvailable` (very old kernels), or with a total of zero.
    pub fn parse(meminfo: &str) -> Option<Self> {
        let field = |name: &str| {
            meminfo.lines().find_map(|line| {
                let (key, rest) = line.split_once(':')?;
                (key == name).then(|| {
                    let kilobytes: u64 = rest.trim().trim_end_matches("kB").trim().parse().ok()?;
                    Some(kilobytes * 1024)
                })?
            })
        };
        let (total, available) = (field("MemTotal")?, field("MemAvailable")?);
        (total > 0).then_some(Self { total, available })
    }

    /// Reads it from `meminfo`, which is `/proc/meminfo` on a real machine.
    pub fn read(meminfo: &Path) -> Option<Self> {
        Self::parse(&std::fs::read_to_string(meminfo).ok()?)
    }
}

/// The RAM a llama-server is expected to take for a model file of
/// `model_bytes`: the weights and [`OVERHEAD_BYTES`].
pub fn expected_use(model_bytes: u64) -> u64 {
    model_bytes.saturating_add(OVERHEAD_BYTES)
}

/// Why a model is not kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// It would take more than `max_percent` of the RAM.
    TooBig { percent: u64 },
    /// Loading it would leave less than [`HEADROOM_PERCENT`] of the RAM
    /// available.
    MachineBusy { left_percent: u64 },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooBig { percent } => write!(f, "it would take {percent}% of the RAM"),
            Self::MachineBusy { left_percent } => {
                write!(
                    f,
                    "loading it would leave {left_percent}% of the RAM available"
                )
            }
        }
    }
}

/// Whether a model that takes `expected` bytes may be kept loaded on
/// `memory`: at most `max_percent` of the total, and loading it leaves
/// [`HEADROOM_PERCENT`] of the total available.
pub fn may_keep(expected: u64, memory: &Memory, max_percent: u64) -> Result<(), Refusal> {
    let percent = |bytes: u64| bytes.saturating_mul(100) / memory.total;
    if expected.saturating_mul(100) > memory.total.saturating_mul(max_percent) {
        return Err(Refusal::TooBig {
            percent: percent(expected).max(max_percent + 1),
        });
    }
    let left = memory.available.saturating_sub(expected);
    if left.saturating_mul(100) < memory.total.saturating_mul(HEADROOM_PERCENT) {
        return Err(Refusal::MachineBusy {
            left_percent: percent(left),
        });
    }
    Ok(())
}

/// Why a model that is kept must stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pressure {
    /// The machine has less than [`HEADROOM_PERCENT`] of its RAM available.
    Low { available_percent: u64 },
    /// The server's own memory passed `max_percent` of the total.
    Grown { percent: u64 },
}

impl fmt::Display for Pressure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Low { available_percent } => {
                write!(f, "only {available_percent}% of the RAM is available")
            }
            Self::Grown { percent } => write!(f, "the server takes {percent}% of the RAM"),
        }
    }
}

/// Whether a kept server using `resident` bytes should stop: the machine
/// runs short, or the server passed its share (the estimate was short).
pub fn pressure(resident: Option<u64>, memory: &Memory, max_percent: u64) -> Option<Pressure> {
    let percent = |bytes: u64| bytes.saturating_mul(100) / memory.total;
    if memory.available.saturating_mul(100) < memory.total.saturating_mul(HEADROOM_PERCENT) {
        return Some(Pressure::Low {
            available_percent: percent(memory.available),
        });
    }
    let resident = resident?;
    (resident.saturating_mul(100) > memory.total.saturating_mul(max_percent)).then(|| {
        Pressure::Grown {
            percent: percent(resident),
        }
    })
}

/// The resident memory of a process, from the text of its
/// `/proc/PID/status` (`VmRSS`), in bytes.
pub fn resident(status: &str) -> Option<u64> {
    let line = status.lines().find_map(|l| l.strip_prefix("VmRSS:"))?;
    let kilobytes: u64 = line.trim().trim_end_matches("kB").trim().parse().ok()?;
    Some(kilobytes * 1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * MIB;

    fn machine(total_gib: u64, available_gib: u64) -> Memory {
        Memory {
            total: total_gib * GIB,
            available: available_gib * GIB,
        }
    }

    #[test]
    fn meminfo_gives_the_total_and_what_is_available() {
        let text = "MemTotal:       16070744 kB\nMemFree:          504236 kB\n\
                    MemAvailable:    7340032 kB\nBuffers:          338008 kB\n";
        let memory = Memory::parse(text).unwrap();
        assert_eq!(memory.total, 16_070_744 * 1024);
        assert_eq!(memory.available, 7_340_032 * 1024);
    }

    #[test]
    fn a_meminfo_missing_a_field_or_empty_tells_nothing() {
        assert_eq!(Memory::parse("MemTotal: 1024 kB\n"), None);
        assert_eq!(Memory::parse("MemAvailable: 1024 kB\n"), None);
        assert_eq!(Memory::parse("MemTotal: 0 kB\nMemAvailable: 0 kB\n"), None);
        assert_eq!(
            Memory::parse("MemTotal: lots kB\nMemAvailable: 5 kB\n"),
            None
        );
        assert_eq!(Memory::parse(""), None);
        assert_eq!(Memory::read(Path::new("/nonexistent/meminfo")), None);
    }

    #[test]
    fn a_model_takes_its_file_and_the_servers_overhead() {
        assert_eq!(expected_use(941 * MIB), 941 * MIB + OVERHEAD_BYTES);
        assert_eq!(expected_use(u64::MAX), u64::MAX);
    }

    #[test]
    fn a_small_model_is_kept_on_a_machine_with_room() {
        // The 0.5B (380 MB) and the 1.5B (941 MB) on 16 GiB: 4% and 7%.
        for model in [380 * MIB, 941 * MIB] {
            assert_eq!(may_keep(expected_use(model), &machine(16, 8), 15), Ok(()));
        }
    }

    #[test]
    fn a_model_that_is_too_large_a_share_of_the_ram_is_not_kept() {
        // The 7B (4.7 GB) on 16 GiB is 31%.
        let seven_b = expected_use(4_700 * MIB);
        assert!(matches!(
            may_keep(seven_b, &machine(16, 12), 15),
            Err(Refusal::TooBig { percent: 30..=32 })
        ));
        // The 1.5B on a 4 GiB machine is 29%.
        assert!(matches!(
            may_keep(expected_use(941 * MIB), &machine(4, 3), 15),
            Err(Refusal::TooBig { percent: 28..=30 })
        ));
        // A larger share allowed keeps it.
        assert_eq!(
            may_keep(expected_use(941 * MIB), &machine(4, 3), 30),
            Ok(())
        );
    }

    #[test]
    fn the_share_allowed_is_inclusive() {
        // Exactly 15% of 20 GiB is 3 GiB.
        assert_eq!(may_keep(3 * GIB, &machine(20, 15), 15), Ok(()));
        assert!(may_keep(3 * GIB + 1, &machine(20, 15), 15).is_err());
        // 100 allows anything that fits.
        assert_eq!(may_keep(8 * GIB, &machine(16, 15), 100), Ok(()));
    }

    #[test]
    fn a_busy_machine_does_not_keep_a_model_that_would_leave_it_short() {
        // 1.2 GB taken of 2 GiB available on 16 GiB leaves 5%.
        let busy = machine(16, 2);
        assert!(matches!(
            may_keep(expected_use(941 * MIB), &busy, 15),
            Err(Refusal::MachineBusy {
                left_percent: 3..=6
            })
        ));
        // The same model with 4 GiB available leaves 22%.
        assert_eq!(
            may_keep(expected_use(941 * MIB), &machine(16, 4), 15),
            Ok(())
        );
    }

    #[test]
    fn a_kept_server_stops_when_the_machine_runs_short() {
        let low = Memory {
            total: 16 * GIB,
            available: GIB,
        };
        assert!(matches!(
            pressure(Some(GIB), &low, 15),
            Some(Pressure::Low {
                available_percent: 6
            })
        ));
        // Even when the server's own size is unknown.
        assert!(matches!(
            pressure(None, &low, 15),
            Some(Pressure::Low { .. })
        ));
        assert_eq!(pressure(Some(GIB), &machine(16, 8), 15), None);
        assert_eq!(pressure(None, &machine(16, 8), 15), None);
    }

    #[test]
    fn a_kept_server_stops_when_it_passes_its_share() {
        // 3 GiB of 16 is 18%, over 15.
        assert!(matches!(
            pressure(Some(3 * GIB), &machine(16, 10), 15),
            Some(Pressure::Grown { percent: 18 })
        ));
        assert_eq!(pressure(Some(2 * GIB), &machine(16, 10), 15), None);
    }

    #[test]
    fn a_processs_resident_memory_is_read_from_its_status() {
        let status =
            "Name:\tllama-server\nVmPeak:\t 2000000 kB\nVmRSS:\t 1196516 kB\nThreads:\t7\n";
        assert_eq!(resident(status), Some(1_196_516 * 1024));
        assert_eq!(resident("Name:\tx\n"), None);
        assert_eq!(resident("VmRSS: many kB\n"), None);
    }
}

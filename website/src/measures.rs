//! The numbers of `docs/measurements.md` that the pages draw. The page never
//! invents one: `tests/routes.rs` finds each of them again in that file.
//! Times and sizes stay as the document writes them (`2.8 s`, `1,169 MB`),
//! so a number can be compared with its source as text.

/// The machines the numbers come from, as `docs/measurements.md` names them.
pub const RYZEN: &str = "Ryzen 5 5500 with a GeForce RTX 3050 8 GB";
pub const CORE_ULTRA: &str = "Core Ultra 5 225H (Arc iGPU, NPU)";

/// Per message end to end on the Ryzen 5 5500 with the RTX 3050: loading the
/// model at every commit, and keeping it loaded from the second commit.
pub struct Kept {
    /// The model and the device, as the document writes them.
    pub label: &'static str,
    pub cold: &'static str,
    pub kept: &'static str,
}

pub const KEPT: &[Kept] = &[
    Kept {
        label: "`qwen2.5-coder-1.5b`, `vulkan`",
        cold: "2.8s",
        kept: "0.20s",
    },
    Kept {
        label: "`qwen2.5-coder-1.5b`, `cpu`",
        cold: "2.6s",
        kept: "2.0s",
    },
    Kept {
        label: "`smart-commit-0.5b-v0.1`, `vulkan`",
        cold: "1.4s",
        kept: "0.14s",
    },
    Kept {
        label: "`smart-commit-0.5b-v0.1`, `cpu`",
        cold: "1.3s",
        kept: "0.8s",
    },
    Kept {
        label: "`qwen2.5-coder-7b`, `vulkan`",
        cold: "7.5s",
        kept: "0.57s",
    },
];

/// The default model on a device, in the table above.
pub fn default_model_on(device: &str) -> &'static Kept {
    let label = format!("`qwen2.5-coder-1.5b`, `{device}`");
    KEPT.iter()
        .find(|row| row.label == label)
        .unwrap_or_else(|| panic!("no measurement of the default model on {device}"))
}

/// What a server keeps in RAM on the CPU.
pub struct Resident {
    pub model: &'static str,
    pub file: &'static str,
    pub with_fast_load: &'static str,
    pub without_fast_load: &'static str,
}

pub const RESIDENT: &[Resident] = &[
    Resident {
        model: "`smart-commit-0.5b-v0.1`",
        file: "380 MB",
        with_fast_load: "564 MB",
        without_fast_load: "593 MB",
    },
    Resident {
        model: "`smart-commit-1.5b-v0.1`",
        file: "941 MB",
        with_fast_load: "1,169 MB",
        without_fast_load: "1,773 MB",
    },
];

/// Per message on the Core Ultra 5 225H, end to end through the CLI.
pub struct DeviceTime {
    /// The device, as the document writes it.
    pub device: &'static str,
    pub time: &'static str,
}

pub const BY_DEVICE: &[DeviceTime] = &[
    DeviceTime {
        device: "intel-gpu",
        time: "1.8s",
    },
    DeviceTime {
        device: "vulkan (iGPU through Mesa ANV)",
        time: "2.5s",
    },
    DeviceTime {
        device: "openvino-cpu",
        time: "4.0s",
    },
    DeviceTime {
        device: "npu",
        time: "5.1s",
    },
];

/// The time of a device on the Core Ultra, as the document writes it.
pub fn by_device(name: &str) -> &'static str {
    BY_DEVICE
        .iter()
        .find(|row| row.device == name)
        .map_or("", |row| row.time)
}

/// Per message on the Ryzen 5 5500, from `smart-commit bench`.
pub struct Bench {
    pub device: &'static str,
    pub total: &'static str,
    pub load: &'static str,
    pub generate: &'static str,
    pub without_fast_load: &'static str,
}

pub const BENCH_RYZEN: &[Bench] = &[
    Bench {
        device: "vulkan (RTX 3050)",
        total: "2.4s",
        load: "2.1s",
        generate: "0.3s",
        without_fast_load: "2.7s",
    },
    Bench {
        device: "cpu (llama.cpp)",
        total: "4.0s",
        load: "0.7s",
        generate: "3.3s",
        without_fast_load: "5.1s",
    },
];

/// Per message by model on the Ryzen 5 5500.
pub struct ModelTime {
    /// The model, as the document writes it.
    pub model: &'static str,
    pub cpu: &'static str,
    pub vulkan: &'static str,
}

pub const BY_MODEL: &[ModelTime] = &[
    ModelTime {
        model: "`qwen2.5-coder-1.5b` (default)",
        cpu: "4.0s",
        vulkan: "2.4s",
    },
    ModelTime {
        model: "`qwen2.5-coder-0.5b`",
        cpu: "3.1s",
        vulkan: "1.5s",
    },
    ModelTime {
        model: "`smart-commit-1.5b-v0.1`",
        cpu: "3.8s",
        vulkan: "2.3s",
    },
    ModelTime {
        model: "`smart-commit-0.5b-v0.1`",
        cpu: "1.9s",
        vulkan: "1.2s",
    },
];

/// Where the time goes on the Core Ultra 5 225H, in milliseconds.
pub struct Breakdown {
    pub configuration: &'static str,
    pub load: &'static str,
    pub prefill: &'static str,
    pub decode: &'static str,
    pub total: &'static str,
}

pub const WHERE: &[Breakdown] = &[
    Breakdown {
        configuration: "`cpu` as shipped (`--no-repack --fit off`)",
        load: "369",
        prefill: "6992",
        decode: "512",
        total: "7873",
    },
    Breakdown {
        configuration: "+ `-tb 14`",
        load: "369",
        prefill: "3369",
        decode: "517",
        total: "4255",
    },
    Breakdown {
        configuration: "+ `-tb 14 -b 1024 -ub 1024`",
        load: "389",
        prefill: "2998",
        decode: "518",
        total: "3905",
    },
    Breakdown {
        configuration: "+ the above, `model.max_prompt_tokens = 512`",
        load: "369",
        prefill: "1664",
        decode: "577",
        total: "2610",
    },
    Breakdown {
        configuration: "`vulkan` as shipped (Intel Arc iGPU)",
        load: "658",
        prefill: "1095",
        decode: "735",
        total: "2487",
    },
    Breakdown {
        configuration: "`vulkan`, `model.max_prompt_tokens = 512`",
        load: "650",
        prefill: "534",
        decode: "739",
        total: "1923",
    },
];

/// How often the type the model picks is the author's, by its confidence.
pub struct Band {
    pub range: &'static str,
    pub commits: &'static str,
    pub right: &'static str,
}

pub const CONFIDENCE: &[Band] = &[
    Band {
        range: "below 50%",
        commits: "67",
        right: "0.493",
    },
    Band {
        range: "50 to 70%",
        commits: "68",
        right: "0.632",
    },
    Band {
        range: "70 to 90%",
        commits: "80",
        right: "0.825",
    },
    Band {
        range: "90% or more",
        commits: "35",
        right: "0.943",
    },
];

/// A time written as the document writes it (`2.8 s`, `0.20 s`) in seconds.
pub fn secs(text: &str) -> f64 {
    text.trim_end_matches('s').trim().parse().unwrap_or(0.0)
}

/// A number written with a thousands comma (`1,169 MB`), without its unit.
pub fn number(text: &str) -> f64 {
    text.split(' ')
        .next()
        .unwrap_or("")
        .replace(',', "")
        .parse()
        .unwrap_or(0.0)
}

/// A label as the document writes it, without the Markdown backticks.
pub fn plain(label: &str) -> String {
    label.replace('`', "")
}

/// The first commit after a pause pays the load; the next four do not.
/// Five commits in a row, in seconds, without and with the model kept loaded.
pub fn five_commits(cold: f64, kept: f64) -> ([f64; 5], [f64; 5]) {
    ([cold; 5], [cold, kept, kept, kept, kept])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_and_numbers_are_read_as_written() {
        assert_eq!(secs("2.8s"), 2.8);
        assert_eq!(secs("0.20s"), 0.2);
        assert_eq!(number("1,169 MB"), 1169.0);
        assert_eq!(number("380 MB"), 380.0);
        assert_eq!(secs("nonsense"), 0.0);
    }

    #[test]
    fn five_commits_pay_the_load_once_with_the_model_kept() {
        let (cold, kept) = five_commits(2.8, 0.2);
        assert_eq!(cold.iter().sum::<f64>(), 14.0);
        assert!((kept.iter().sum::<f64>() - 3.6).abs() < 1e-9);
    }

    #[test]
    fn a_device_is_found_by_name() {
        assert_eq!(by_device("npu"), "5.1s");
        assert_eq!(by_device("nothing"), "");
    }

    #[test]
    fn the_default_model_is_found_on_each_device() {
        assert_eq!(default_model_on("vulkan").kept, "0.20s");
        assert_eq!(default_model_on("cpu").kept, "2.0s");
    }

    #[test]
    fn labels_lose_their_backticks() {
        assert_eq!(plain("`a`, `b`"), "a, b");
    }
}

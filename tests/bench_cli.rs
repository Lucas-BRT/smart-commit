//! `smart-commit bench` and `smart-commit device`, without hardware: devices
//! are unavailable (no models), and saved reports are written by hand.

use std::fs;
use std::path::Path;
use std::process::Output;

use smart_commit::backend::device::Target;
use smart_commit::bench::{self, Entry, FastLoadTrial, Machine, Measurement, Report};
use tempfile::TempDir;

mod common;
use common::{TestRepo, smart_commit};

struct Home {
    config: TempDir,
    data: TempDir,
}

impl Home {
    fn new() -> Self {
        Self {
            config: TempDir::new().unwrap(),
            data: TempDir::new().unwrap(),
        }
    }

    fn run(&self, args: &[&str], dir: &Path) -> Output {
        smart_commit(
            args,
            dir,
            &[
                ("XDG_CONFIG_HOME", self.config.path().to_str().unwrap()),
                ("XDG_DATA_HOME", self.data.path().to_str().unwrap()),
            ],
        )
    }

    fn bench_file(&self) -> std::path::PathBuf {
        self.data.path().join("smart-commit/bench.json")
    }

    /// Saves a report where `per_message` is None for unavailable devices.
    fn save_report(&self, results: &[(Target, Option<f64>)]) {
        let entries = results
            .iter()
            .map(|&(target, ms)| Entry {
                target,
                backend: ms.map(|_| format!("{target} backend")),
                measurement: ms.map(|ms| Measurement {
                    load_ms: ms / 2.0,
                    generate_ms: ms / 2.0,
                    per_message_ms: ms,
                    cpu_cores: 1.0,
                    replies: vec![],
                }),
                error: ms
                    .is_none()
                    .then(|| "backend unavailable: not set up".to_owned()),
                fast_load: None,
            })
            .collect();
        let report = Report {
            format: bench::FORMAT,
            tool_version: "test".into(),
            machine: Machine {
                cpu: "Test CPU".into(),
                kernel: "6.0".into(),
            },
            entries,
        };
        fs::create_dir_all(self.bench_file().parent().unwrap()).unwrap();
        fs::write(self.bench_file(), report.to_json()).unwrap();
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn bench_without_models_reports_every_device_and_fails() {
    let home = Home::new();
    let dir = TempDir::new().unwrap();
    let output = home.run(&["bench"], dir.path());
    assert_eq!(output.status.code(), Some(1));
    let out = stdout(&output);
    for device in ["intel-gpu", "npu", "vulkan", "cpu", "openvino-cpu"] {
        assert!(
            out.contains(&format!("  {device:<12} unavailable: ")),
            "{out}"
        );
    }
    assert!(stderr(&output).contains("measuring intel-gpu..."));
    assert!(stderr(&output).contains("error: no device could run the model"));
    let saved = Report::from_json(&fs::read_to_string(home.bench_file()).unwrap()).unwrap();
    assert_eq!(saved.entries.len(), 5);
    assert_eq!(saved.fastest(), None);
}

#[test]
fn bench_json_prints_the_report() {
    let home = Home::new();
    let dir = TempDir::new().unwrap();
    let output = home.run(&["bench", "--json"], dir.path());
    let report = Report::from_json(&stdout(&output)).unwrap();
    assert_eq!(
        report.entries.iter().map(|e| e.target).collect::<Vec<_>>(),
        [
            Target::IntelGpu,
            Target::Npu,
            Target::Vulkan,
            Target::Cpu,
            Target::OpenvinoCpu
        ]
    );
}

#[test]
fn device_without_a_benchmark_says_how_to_run_one() {
    let home = Home::new();
    let dir = TempDir::new().unwrap();
    let out = stdout(&home.run(&["device"], dir.path()));
    assert!(out.contains("No benchmark yet"), "{out}");
    assert!(out.contains("model.device comes from: default."), "{out}");
}

#[test]
fn device_shows_the_fastest_as_selected() {
    let home = Home::new();
    let dir = TempDir::new().unwrap();
    home.save_report(&[
        (Target::IntelGpu, Some(1800.0)),
        (Target::Npu, Some(5100.0)),
        (Target::Vulkan, None),
        (Target::Cpu, Some(4000.0)),
    ]);
    let out = stdout(&home.run(&["device"], dir.path()));
    assert!(out.contains("Last benchmark (Test CPU)"), "{out}");
    assert!(
        out.contains("1.8 s") && out.contains("<- selected"),
        "{out}"
    );
    assert!(
        out.contains("vulkan       unavailable: backend unavailable: not set up"),
        "{out}"
    );
    assert!(
        out.contains(
            "smart-commit will use intel-gpu on this machine (~1.8 s per message): the fastest"
        ),
        "{out}"
    );
}

#[test]
fn device_use_changes_the_global_config_and_shows_the_estimate() {
    let home = Home::new();
    let dir = TempDir::new().unwrap();
    home.save_report(&[
        (Target::IntelGpu, Some(1800.0)),
        (Target::Npu, Some(5100.0)),
        (Target::Vulkan, None),
    ]);

    let output = home.run(&["device", "use", "npu"], dir.path());
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("model.device = npu"));
    assert!(stdout(&output).contains("~5.1 s per message"));
    let out = stdout(&home.run(&["config", "get", "model.device"], dir.path()));
    assert_eq!(out, "npu\n");
    let shown = stdout(&home.run(&["device"], dir.path()));
    assert!(
        shown.contains("will use npu on this machine (~5.1 s per message): set in model.device"),
        "{shown}"
    );

    let output = home.run(&["device", "use", "vulkan"], dir.path());
    assert!(stderr(&output).contains("note: vulkan was unavailable in the last benchmark"));

    let output = home.run(&["device", "use", "auto"], dir.path());
    assert!(
        stdout(&output).contains("will use intel-gpu"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn the_old_gpu_name_in_the_config_still_works_with_a_warning() {
    let home = Home::new();
    let dir = TempDir::new().unwrap();
    let config = home.config.path().join("smart-commit/config.toml");
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    fs::write(&config, "[model]\ndevice = \"gpu\"\n").unwrap();
    home.save_report(&[(Target::IntelGpu, Some(1800.0))]);
    let output = home.run(&["device"], dir.path());
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stdout(&output).contains("will use intel-gpu on this machine (~1.8 s per message)"),
        "{}",
        stdout(&output)
    );
    let err = stderr(&output);
    assert!(
        err.starts_with("warning: model.device = gpu (global ")
            && err.contains("`smart-commit config set model.device intel-gpu` renames it"),
        "{err}"
    );

    // `device use` with the old name writes the new one.
    home.run(&["device", "use", "gpu"], dir.path());
    assert_eq!(
        fs::read_to_string(&config).unwrap(),
        "[model]\ndevice = \"intel-gpu\"\n"
    );
    assert_eq!(stderr(&home.run(&["device"], dir.path())), "");
}

#[test]
fn auto_tries_devices_in_benchmark_order() {
    let home = Home::new();
    let repo = TestRepo::new();
    repo.commit("feat: init");
    repo.write("a.rs", "fn a() {}\n");
    repo.git(&["add", "a.rs"]);
    home.save_report(&[
        (Target::IntelGpu, None),
        (Target::Cpu, Some(4000.0)),
        (Target::Vulkan, Some(2400.0)),
    ]);
    let err = stderr(&home.run(&[], repo.path()));
    let order: Vec<&str> = err
        .lines()
        .filter_map(|l| l.trim_start().split_once(':').map(|(d, _)| d))
        .filter(|d| Target::from_name(d).is_some())
        .collect();
    assert_eq!(
        order,
        ["vulkan", "cpu", "intel-gpu", "npu", "openvino-cpu"],
        "{err}"
    );
}

#[test]
fn device_says_where_the_benchmark_turned_fast_load_on() {
    let home = Home::new();
    let dir = TempDir::new().unwrap();
    home.save_report(&[
        (Target::Npu, None),
        (Target::Vulkan, Some(2300.0)),
        (Target::Cpu, Some(1900.0)),
    ]);
    let mut report = Report::from_json(&fs::read_to_string(home.bench_file()).unwrap()).unwrap();
    report.entries[1].fast_load = Some(FastLoadTrial::new(Some(2400.0), Some(2300.0)));
    report.entries[2].fast_load = Some(FastLoadTrial::new(Some(2800.0), Some(1900.0)));
    fs::write(home.bench_file(), report.to_json()).unwrap();

    let out = stdout(&home.run(&["device"], dir.path()));
    assert!(
        out.contains(
            "fast load turned on for cpu (--no-repack --fit off): 2.8 s -> 1.9 s per message"
        ),
        "{out}"
    );
    assert!(
        out.contains("fast load left off for vulkan: 2.4 s -> 2.3 s per message, under 5% faster"),
        "{out}"
    );
    assert!(!out.contains("overrides"), "{out}");

    home.run(&["config", "set", "model.fast_load", "off"], dir.path());
    let out = stdout(&home.run(&["device"], dir.path()));
    assert!(
        out.contains("model.fast_load = off overrides this: always off."),
        "{out}"
    );
}

#[test]
fn prompts_need_a_llama_cpp_device() {
    let home = Home::new();
    let dir = TempDir::new().unwrap();
    let output = home.run(
        &["bench", "--prompts", dir.path().to_str().unwrap()],
        dir.path(),
    );
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("--device vulkan or cpu"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn prompts_without_prompt_files_are_reported() {
    let home = Home::new();
    let dir = TempDir::new().unwrap();
    let output = home.run(
        &[
            "bench",
            "--prompts",
            dir.path().to_str().unwrap(),
            "--device",
            "cpu",
        ],
        dir.path(),
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no *.txt prompts"));
}

#[test]
fn the_options_of_prompts_need_prompts_and_json_conflicts_with_it() {
    let home = Home::new();
    let dir = TempDir::new().unwrap();
    for args in [
        vec!["bench", "--label", "x"],
        vec!["bench", "--json", "--prompts", "."],
    ] {
        let output = home.run(&args, dir.path());
        assert_eq!(output.status.code(), Some(2), "{args:?}");
    }
}

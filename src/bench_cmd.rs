//! `smart-commit bench` and `smart-commit device`: measure the devices of this
//! machine, show which one is used, and change it.

use std::error::Error;
use std::path::PathBuf;

use clap::Subcommand;
use smart_commit::bench::{self, Machine, Report};
use smart_commit::config::{self, DeviceSetting, Origin};
use smart_commit::device::{DEFAULT_ORDER, Target};
use smart_commit::paths;

use crate::DeviceArg;

#[derive(Debug, Subcommand)]
pub enum DeviceCommand {
    /// Use a device from now on (global config), or `auto` for the fastest.
    Use { device: DeviceArg },
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

fn bench_file() -> Result<PathBuf, Box<dyn Error>> {
    Ok(paths::default_bench_file(env).ok_or("HOME is not set")?)
}

/// The last benchmark, if there is a readable one.
pub fn last_report() -> Option<Report> {
    let text = std::fs::read_to_string(bench_file().ok()?).ok()?;
    Report::from_json(&text)
}

const HOW_TO_CHANGE: &str = "To use another device: smart-commit device use <npu|gpu|cpu|vulkan>, \
                             or `auto` for the fastest.";

pub fn run_bench(json: bool) -> Result<(), Box<dyn Error>> {
    let settings = crate::resolve(None, Vec::new())?.settings();
    let prompts = bench::prompts();
    let config = settings.generation_config();
    let mut entries = Vec::new();
    for target in DEFAULT_ORDER {
        eprintln!("measuring {target}...");
        entries.push(bench::measure(target, &prompts, &config, |t| {
            crate::load_on(&settings, t)
        }));
    }
    let report = Report {
        format: bench::FORMAT,
        tool_version: env!("CARGO_PKG_VERSION").to_owned(),
        machine: Machine::current(),
        entries,
    };
    let path = bench_file()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, report.to_json())?;
    if json {
        print!("{}", report.to_json());
        return Ok(());
    }
    let selected = effective(&report, settings.device);
    println!(
        "Benchmark on {}: {} prompts, after one warm-up load.\n",
        report.machine.cpu,
        prompts.len()
    );
    print!("{}", report.table(selected));
    println!();
    match selected {
        Some(target) => println!("{}", using_message(&report, target, settings.device)),
        None => {
            return Err(
                "no device could run the model; `smart-commit setup` installs \
                        the drivers and models"
                    .into(),
            );
        }
    }
    println!("{HOW_TO_CHANGE}");
    println!("Saved to {}.", path.display());
    Ok(())
}

/// The device that will run: the configured one, or the fastest for auto.
fn effective(report: &Report, device: DeviceSetting) -> Option<Target> {
    match device {
        DeviceSetting::Auto => report.fastest(),
        DeviceSetting::Npu => Some(Target::Npu),
        DeviceSetting::Gpu => Some(Target::Gpu),
        DeviceSetting::Cpu => Some(Target::Cpu),
        DeviceSetting::Vulkan => Some(Target::Vulkan),
    }
}

fn using_message(report: &Report, target: Target, device: DeviceSetting) -> String {
    let estimate = report
        .entry(target)
        .and_then(|e| e.measurement.as_ref())
        .map(|m| format!(" (~{:.1} s per message)", m.per_message_ms / 1000.0))
        .unwrap_or_else(|| " (unavailable in the benchmark)".to_owned());
    let why = match device {
        DeviceSetting::Auto => "the fastest, model.device = auto",
        _ => "set in model.device",
    };
    format!("smart-commit will use {target} on this machine{estimate}: {why}.")
}

pub fn run_device(command: Option<DeviceCommand>) -> Result<(), Box<dyn Error>> {
    match command {
        None => show_device(),
        Some(DeviceCommand::Use { device }) => use_device(device),
    }
}

fn show_device() -> Result<(), Box<dyn Error>> {
    let resolved = crate::resolve(None, Vec::new())?;
    let settings = resolved.settings();
    let origin = resolved
        .get("model.device")
        .map(|(_, o)| o.to_string())
        .unwrap_or_default();
    match last_report() {
        Some(report) => {
            println!("Last benchmark ({}):\n", report.machine.cpu);
            let selected = effective(&report, settings.device);
            print!("{}", report.table(selected));
            println!();
            if let Some(target) = selected {
                println!("{}", using_message(&report, target, settings.device));
            }
        }
        None => println!(
            "No benchmark yet: `smart-commit bench` measures the devices of this machine.\n\
             Until then, auto uses the first that works of gpu, npu, vulkan, cpu."
        ),
    }
    println!("model.device comes from: {origin}.");
    println!("{HOW_TO_CHANGE}");
    Ok(())
}

fn use_device(device: DeviceArg) -> Result<(), Box<dyn Error>> {
    let path = paths::default_config_file(env).ok_or("HOME is not set")?;
    let origin = Origin::Global(path.clone());
    config::set_in_file(&path, &origin, "model.device", device.name(), None)?;
    println!("model.device = {}  ({origin})", device.name());
    if env("SMART_COMMIT_DEVICE").is_some_and(|v| !v.is_empty()) {
        eprintln!("note: SMART_COMMIT_DEVICE is set and overrides the config.");
    }
    let Some(report) = last_report() else {
        println!("Run `smart-commit bench` to compare the devices of this machine.");
        return Ok(());
    };
    match device.target() {
        None => {
            if let Some(target) = report.fastest() {
                println!("{}", using_message(&report, target, DeviceSetting::Auto));
            }
        }
        Some(target) => {
            let entry = report.entry(target);
            if let Some(m) = entry.and_then(|e| e.measurement.as_ref()) {
                println!(
                    "~{:.1} s per message on this machine, in the last benchmark.",
                    m.per_message_ms / 1000.0
                );
            } else if let Some(error) = entry.and_then(|e| e.error.as_deref()) {
                eprintln!(
                    "note: {target} was unavailable in the last benchmark: {}",
                    error.lines().next().unwrap_or(error)
                );
            }
        }
    }
    Ok(())
}

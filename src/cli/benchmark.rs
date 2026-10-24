//! `smart-commit bench` and `smart-commit device`: measure the devices of this
//! machine, show which one is used, and change it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use clap::Subcommand;
use smart_commit::bench::{self, Entry, Machine, Report};
use smart_commit::config::{self, BackendKind, DeviceSetting, FastLoad, Origin, Settings};
use smart_commit::device::{DEFAULT_ORDER, Target};
use smart_commit::paths;
use smart_commit::tui::bench::{Action, BenchView};
use smart_commit::tui::{self, Screen};
use std::error::Error;

use super::args::DeviceArg;
use super::load::{Retry, load_with};
use super::settings::{env, resolve, stop_kept_after, warn_renamed};

#[derive(Debug, Subcommand)]
pub(super) enum DeviceCommand {
    /// Use a device from now on (global config), or `auto` for the fastest.
    Use { device: DeviceArg },
}

fn bench_file() -> Result<PathBuf, Box<dyn Error>> {
    Ok(paths::default_bench_file(env).ok_or("HOME is not set")?)
}

/// The last benchmark, if there is a readable one.
pub(super) fn last_report() -> Option<Report> {
    let text = std::fs::read_to_string(bench_file().ok()?).ok()?;
    Report::from_json(&text)
}

const HOW_TO_CHANGE: &str = "To use another device: smart-commit device use \
                             <npu|intel-gpu|vulkan|cpu|openvino-cpu>, or `auto` for the fastest.";

/// What the measuring thread reports.
enum Progress {
    Start(Target, &'static str),
    Done(Entry),
}

/// Measures every device, telling `progress` what it does, unless `stop`
/// is set before a device: then `None`, and nothing is left running.
fn measure_all(
    settings: &Settings,
    stop: &AtomicBool,
    mut progress: impl FnMut(Progress),
) -> Option<Report> {
    // A model kept loaded holds memory the measured loads need.
    if let Some(runtime) = paths::runtime_dir(env) {
        smart_commit::server::stop(&runtime);
    }
    let prompts = bench::prompts();
    let config = settings.generation_config();
    let mut entries = Vec::new();
    for target in DEFAULT_ORDER {
        if stop.load(Ordering::Relaxed) {
            return None;
        }
        progress(Progress::Start(target, "measuring"));
        let entry = match target.backend() {
            BackendKind::LlamaCpp => bench::measure_load_modes_with(
                target,
                &prompts,
                &config,
                |t, fast| load_with(settings, t, fast, Retry::Never),
                |phase| progress(Progress::Start(target, phase)),
            ),
            _ => bench::measure(target, &prompts, &config, |t| {
                load_with(settings, t, false, Retry::Never)
            }),
        };
        progress(Progress::Done(entry.clone()));
        entries.push(entry);
    }
    Some(Report {
        format: bench::FORMAT,
        tool_version: env!("CARGO_PKG_VERSION").to_owned(),
        machine: Machine::current(),
        entries,
    })
}

fn save(report: &Report) -> Result<PathBuf, Box<dyn Error>> {
    let path = bench_file()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, report.to_json())?;
    Ok(path)
}

pub(super) fn run_bench(json: bool) -> Result<(), Box<dyn Error>> {
    let resolved = resolve(None, Vec::new())?;
    warn_renamed(&resolved);
    let settings = resolved.settings();
    if !json && tui::interactive() {
        return bench_screen(settings);
    }
    let report = measure_all(&settings, &AtomicBool::new(false), |p| {
        if let Progress::Start(target, "measuring") = p {
            eprintln!("measuring {target}...");
        }
    })
    .expect("never stopped");
    let path = save(&report)?;
    if json {
        print!("{}", report.to_json());
        return Ok(());
    }
    print_summary(&report, &settings, &path)
}

/// The benchmark in a terminal: devices as they are measured, then a
/// choice of device; the summary is printed afterwards, so it stays on
/// screen.
fn bench_screen(settings: Settings) -> Result<(), Box<dyn Error>> {
    let mut screen = Screen::new()?;
    let benchmarked = bench_in(&mut screen, &settings);
    drop(screen);
    let Benchmarked {
        report,
        path,
        choice,
    } = benchmarked?;
    let settings = match apply(choice)? {
        Some((line, settings)) => {
            println!("{line}");
            settings
        }
        None => settings,
    };
    print_summary(&report, &settings, &path)
}

/// A finished benchmark, saved, and the device the user chose, if any.
pub(super) struct Benchmarked {
    pub report: Report,
    pub path: PathBuf,
    pub choice: Option<String>,
}

/// Measures every device on `screen`, live, saves the report and lets the
/// user choose a device. Ctrl+C stops after the device being measured and
/// saves nothing.
pub(super) fn bench_in(
    screen: &mut Screen,
    settings: &Settings,
) -> Result<Benchmarked, Box<dyn Error>> {
    let auto = settings.device == DeviceSetting::Auto;
    let mut view = BenchView::measuring(
        format!("Benchmark on {}", Machine::current().cpu),
        &DEFAULT_ORDER,
        auto,
    );
    let stop = Arc::new(AtomicBool::new(false));
    let (sender, progress) = mpsc::channel();
    let worker = {
        let (settings, stop) = (settings.clone(), stop.clone());
        std::thread::spawn(move || {
            measure_all(&settings, &stop, |p| {
                let _ = sender.send(p);
            })
        })
    };
    let report = loop {
        while let Ok(p) = progress.try_recv() {
            match p {
                Progress::Start(target, what) => view.start(target, what),
                Progress::Done(entry) => view.finish(entry),
            }
        }
        if worker.is_finished() {
            match worker.join().map_err(|_| "the benchmark crashed")? {
                Some(report) => break report,
                None => return Err("benchmark stopped; nothing was saved".into()),
            }
        }
        view.tick();
        screen.draw(|frame| view.render(frame))?;
        // Ctrl+C stops after the device being measured, so no server is
        // left running.
        if let Some(key) = screen.key_within(Duration::from_millis(150))?
            && view.on_key(key) == Action::Quit
        {
            stop.store(true, Ordering::Relaxed);
            view.stopping();
        }
    };
    let path = save(&report)?;
    view.done(
        effective(&report, settings.device),
        fast_load_lines(&report, settings.fast_load),
    );
    let choice = choose(screen, &mut view)?;
    Ok(Benchmarked {
        report,
        path,
        choice,
    })
}

/// Waits for the user to pick a device, or none.
fn choose(screen: &mut Screen, view: &mut BenchView) -> Result<Option<String>, Box<dyn Error>> {
    loop {
        screen.draw(|frame| view.render(frame))?;
        match view.on_key(screen.key()?) {
            Action::None => {}
            Action::Quit => return Ok(None),
            Action::Use(target) => return Ok(Some(target.name().to_owned())),
            Action::Auto => return Ok(Some("auto".to_owned())),
        }
    }
}

/// Sets model.device to what the screen chose, and returns the settings
/// after it.
/// Sets model.device to what the screen chose; returns what it did and the
/// settings after it.
pub(super) fn apply(choice: Option<String>) -> Result<Option<(String, Settings)>, Box<dyn Error>> {
    let Some(name) = choice else {
        return Ok(None);
    };
    let path = paths::default_config_file(env).ok_or("HOME is not set")?;
    let origin = Origin::Global(path.clone());
    config::set_in_file(&path, &origin, "model.device", &name, None)?;
    Ok(Some((
        format!("model.device = {name}  ({origin})"),
        resolve(None, Vec::new())?.settings(),
    )))
}

fn print_summary(report: &Report, settings: &Settings, path: &Path) -> Result<(), Box<dyn Error>> {
    let selected = effective(report, settings.device);
    println!(
        "Benchmark on {}: {} prompts, after one warm-up load.\n",
        report.machine.cpu,
        bench::prompts().len()
    );
    print!("{}", report.table(selected));
    println!();
    print_fast_load(report, settings.fast_load);
    match selected {
        Some(target) => println!("{}", using_message(report, target, settings.device)),
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

/// What the benchmark decided about fast load, and whether model.fast_load
/// overrides it.
fn print_fast_load(report: &Report, setting: FastLoad) {
    let lines = fast_load_lines(report, setting);
    if lines.is_empty() {
        return;
    }
    for line in lines {
        println!("{line}");
    }
    println!();
}

/// The benchmark's fast load notes, and the setting that overrides them.
fn fast_load_lines(report: &Report, setting: FastLoad) -> Vec<String> {
    let mut lines = report.fast_load_notes();
    if lines.is_empty() {
        return lines;
    }
    match setting {
        FastLoad::Auto => {}
        FastLoad::On => lines.push("model.fast_load = on overrides this: always on.".to_owned()),
        FastLoad::Off => lines.push("model.fast_load = off overrides this: always off.".to_owned()),
    }
    lines
}

/// The device that will run: the configured one, or the fastest for auto.
fn effective(report: &Report, device: DeviceSetting) -> Option<Target> {
    match device {
        DeviceSetting::Auto => report.fastest(),
        DeviceSetting::Fixed(target) => Some(target),
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

pub(super) fn run_device(command: Option<DeviceCommand>) -> Result<(), Box<dyn Error>> {
    match command {
        None => show_device(),
        Some(DeviceCommand::Use { device }) => use_device(device),
    }
}

fn show_device() -> Result<(), Box<dyn Error>> {
    let resolved = resolve(None, Vec::new())?;
    warn_renamed(&resolved);
    let settings = resolved.settings();
    if tui::interactive()
        && let Some(report) = last_report()
    {
        let mut view = BenchView::from_report(
            format!("Last benchmark ({})", report.machine.cpu),
            &report,
            effective(&report, settings.device),
            settings.device == DeviceSetting::Auto,
            fast_load_lines(&report, settings.fast_load),
        );
        let mut screen = Screen::new()?;
        let choice = choose(&mut screen, &mut view);
        drop(screen);
        match apply(choice?)? {
            Some((line, _)) => println!("{line}"),
            None => return Ok(()),
        }
    }
    let resolved = resolve(None, Vec::new())?;
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
            print_fast_load(&report, settings.fast_load);
            if let Some(target) = selected {
                println!("{}", using_message(&report, target, settings.device));
            }
        }
        None => println!(
            "No benchmark yet: `smart-commit bench` measures the devices of this machine.\n\
             Until then, auto uses the first that works of {}.",
            DEFAULT_ORDER.map(Target::name).join(", ")
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
    if let Some(note) = stop_kept_after("model.device") {
        println!("{note}");
    }
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

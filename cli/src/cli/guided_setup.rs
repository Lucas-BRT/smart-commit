//! `smart-commit setup`: in a terminal, a full-screen setup (the hardware,
//! the plan, each step's output, the benchmark); with `--yes` or without a
//! terminal, the same plan as text.

use std::io::{BufRead, BufReader, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Context as _, Result};
use smart_commit::backend::llamacpp;
use smart_commit::config;
use smart_commit::hardware::{Hardware, Npu};
use smart_commit::models::{self, Model};
use smart_commit::paths;
use smart_commit::setup::{self, Facts, StepKind};
use smart_commit::tui::setup::{Action, SetupView, State};
use smart_commit::tui::{self, Screen};

use super::model::{self, Chosen};
use super::settings::{env, hardware_sources, resolve};

pub(super) fn run(yes: bool, no_drivers: bool, no_bench: bool, llamacpp_only: bool) -> Result<()> {
    set_aside_invalid_settings()?;
    if llamacpp_only {
        let server = install_llamacpp(&mut |_| {})?;
        println!("llama.cpp: {}", server.display());
        return Ok(());
    }
    let exe = std::env::current_exe()?.canonicalize()?;
    let scripts = scripts_dir(&exe)?;
    if !yes && tui::interactive() {
        return interactive(&scripts, no_drivers, no_bench);
    }
    plain(&scripts, yes, no_drivers, no_bench)
}

/// The llama.cpp build this release pins, in the data directory (from
/// `SMART_COMMIT_LLAMACPP_URL`, for tests; the pinned SHA-256 still applies).
fn install_llamacpp(progress: &mut dyn FnMut(u64)) -> Result<PathBuf> {
    let server = paths::default_llama_server(env).context("HOME is not set")?;
    let dir = server
        .parent()
        .and_then(Path::parent)
        .context("the llama.cpp directory has no parent")?;
    let url = env("SMART_COMMIT_LLAMACPP_URL").unwrap_or_else(llamacpp::install::archive_url);
    Ok(llamacpp::install::install(
        dir,
        &url,
        llamacpp::install::ARCHIVE_SHA256,
        progress,
    )?)
}

/// `setup` without the full screen, for `--yes` and for output that is not a
/// terminal: the hardware, the plan, then each step that is not done yet,
/// asking before what needs sudo.
fn plain(scripts: &Path, yes: bool, no_drivers: bool, no_bench: bool) -> Result<()> {
    let hw = Hardware::detect(&hardware_sources());
    let model = resolve(None, Vec::new())?.settings().model;
    let facts = facts(scripts, &hw, model, !no_drivers, !no_bench)?;
    let steps = setup::plan(&hw, &facts);
    println!("\n==> Hardware\n{}", hw.inventory().trim_end());
    println!("\n==> Plan");
    for step in &steps {
        let state = if step.installed {
            " (already set up)"
        } else {
            ""
        };
        let sudo = if step.sudo { " (sudo)" } else { "" };
        println!("  {}{sudo}: {}{state}", step.title, step.detail);
    }
    if let Some(why) = setup::not_needed(&hw) {
        println!("  {why}");
    }
    let mut failed = false;
    for step in steps.iter().filter(|s| !s.installed) {
        match step.kind {
            StepKind::LlamaCpp => {
                println!("\n==> llama.cpp");
                match install_llamacpp(&mut |_| {}) {
                    Ok(server) => {
                        println!("  {}", server.display());
                        let devices = llamacpp::install::list_devices(&server);
                        if !devices.is_empty() {
                            println!("  devices seen by llama.cpp (besides the CPU):\n{devices}");
                        }
                    }
                    Err(err) => {
                        failed = true;
                        println!(
                            "  Could not install llama.cpp ({err:#}); run `smart-commit setup` again."
                        );
                    }
                }
            }
            StepKind::Model => {
                println!("\n==> Model {}", model.id);
                match plain_model(model) {
                    Ok(file) => {
                        println!("  checked against the pinned SHA-256: {}", file.display())
                    }
                    Err(err) => {
                        failed = true;
                        println!(
                            "  Could not install {} ({err:#}); run `smart-commit setup` again.",
                            model.id
                        );
                    }
                }
            }
            StepKind::GpuDrivers => {
                println!("\n==> GPU drivers");
                if ask("Set up the GPUs as planned above (may use sudo)?", yes) {
                    failed |= !run_script(
                        &scripts.join("setup-gpu.sh"),
                        if yes { &["--yes"] } else { &[] },
                    );
                }
            }
            StepKind::NpuStack => {
                if ask(
                    "Install the Intel NPU driver and OpenVINO (uses sudo)?",
                    yes,
                ) {
                    failed |= !run_script(&scripts.join("setup-npu.sh"), &[]);
                }
            }
            StepKind::Openvino => {
                if ask("Install OpenVINO, for the Intel GPU (uses sudo)?", yes) {
                    failed |= !run_script(&scripts.join("setup-npu.sh"), &["--openvino-only"]);
                }
            }
            StepKind::ExportModel => {
                println!("\n==> OpenVINO model");
                if ask(
                    "Export the model for OpenVINO here (downloads it from Hugging Face; \
                     about 10 GB of disk while it runs, a few minutes)?",
                    yes,
                ) && !run_script(&scripts.join("export-model.sh"), &[])
                {
                    failed = true;
                    println!("  Could not export the model; run `smart-commit setup` again.");
                }
            }
            StepKind::Bench if render_group_pending() => {
                println!("\n==> Benchmark postponed");
                println!(
                    "  You were just added to the render group, which only applies after\n  \
                     logging out and back in. Then run: smart-commit bench"
                );
            }
            StepKind::Bench => {
                println!("\n==> Benchmark");
                if let Err(err) = super::benchmark::run_bench(false) {
                    failed = true;
                    println!("  Nothing could run the model yet ({err:#}); see above.");
                }
            }
        }
    }
    for step in steps
        .iter()
        .filter(|s| s.installed && s.kind == StepKind::NpuStack)
    {
        println!("\n==> {}: already set up", step.title);
    }
    println!("\n==> Done");
    if !failed {
        print!("{NEXT_STEPS}");
    }
    Ok(())
}

/// What to do once the setup worked.
const NEXT_STEPS: &str = "\nNext:\n  \
    git add -p                 choose what goes into the commit\n  \
    smart-commit               print a message for what is staged\n  \
    smart-commit --commit      review it, then commit\n  \
    smart-commit hook install  fill in every `git commit` of this repository\n";

/// Downloads `model` into the models directory, as `setup` does for the
/// one in use.
fn plain_model(model: &'static Model) -> Result<PathBuf> {
    let dir = paths::default_models_dir(env).context("HOME is not set")?;
    let base = env("SMART_COMMIT_HF_BASE").unwrap_or_else(|| models::HUGGING_FACE.to_owned());
    println!(
        "  {} ({}, {}) from huggingface.co/{}",
        model.name,
        models::size(model.gguf.size),
        model.license,
        model.gguf.repo
    );
    Ok(models::install_gguf(model, &dir, &base, |_| {})?)
}

/// Runs a setup script with the terminal, and whether it succeeded.
fn run_script(script: &Path, args: &[&str]) -> bool {
    Command::new(script)
        .args(args)
        .status()
        .map_err(|err| println!("  cannot run {}: {err}", script.display()))
        .is_ok_and(|status| status.success())
}

/// `question`: yes with `--yes`; no without a terminal to ask on.
fn ask(question: &str, yes: bool) -> bool {
    if yes {
        return true;
    }
    if !std::io::stdin().is_terminal() {
        println!("{question} [skipped: no terminal; run `smart-commit setup` again to do it]");
        return false;
    }
    print!("{question} [y/N] ");
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer).is_ok()
        && matches!(answer.trim(), "y" | "Y" | "yes" | "s" | "S" | "sim")
}

/// Setup is how a broken install gets fixed, so a global setting this
/// version cannot read (a model a newer build added, say) must not stop it:
/// it is removed with a warning naming the old value, and the default
/// applies until something else is chosen.
fn set_aside_invalid_settings() -> Result<()> {
    let Some(path) = paths::default_config_file(env) else {
        return Ok(());
    };
    for removed in config::set_aside_invalid(&path, env("HOME").as_deref())? {
        tracing::warn!(
            "{removed}; removed, so the default applies \
             (`smart-commit model` or `smart-commit config set` choose again)"
        );
    }
    Ok(())
}

/// The scripts next to the binary: installed, or in a checkout.
fn scripts_dir(exe: &Path) -> Result<PathBuf> {
    let bin = exe.parent().context("the binary has no directory")?;
    [
        bin.join("../share/smart-commit/scripts"),
        bin.join("../../scripts"),
    ]
    .into_iter()
    .find(|p| p.join("setup-gpu.sh").is_file())
    .context("the setup scripts are not next to the binary; reinstall smart-commit")
}

/// What is already on this machine.
fn facts(
    scripts: &Path,
    hw: &Hardware,
    model: &'static Model,
    drivers: bool,
    bench: bool,
) -> Result<Facts> {
    let settings = resolve(None, Vec::new())?.settings();
    let models_dir = paths::default_models_dir(env).context("HOME is not set")?;
    let quiet = |script: &str, args: &[&str]| {
        Command::new(scripts.join(script))
            .args(args)
            .stdin(Stdio::null())
            .output()
            .is_ok_and(|o| o.status.success())
    };
    let gpu_plan = if hw.gpus.is_empty() {
        Vec::new()
    } else {
        // What setup-gpu.sh would do, and what it warns about (a GPU with no
        // kernel driver, say), which the plan should say too.
        Command::new(scripts.join("setup-gpu.sh"))
            .arg("--dry-run")
            .stdin(Stdio::null())
            .output()
            .map(|o| {
                let steps = String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .filter_map(|l| l.strip_prefix("==> ").map(str::to_owned))
                    .collect::<Vec<_>>();
                let warnings = String::from_utf8_lossy(&o.stderr)
                    .lines()
                    .filter(|l| l.starts_with("warning: "))
                    .map(str::to_owned)
                    .collect::<Vec<_>>();
                [steps, warnings].concat()
            })
            .unwrap_or_default()
    };
    Ok(Facts {
        model,
        llama_installed: settings.llama_server.as_ref().is_some_and(|p| p.is_file()),
        model_installed: models::is_installed(&model.gguf, &models_dir.join(model.gguf.file)),
        npu_ready: hw.npu == Npu::Intel && quiet("check-npu.sh", &[]),
        exported: model.openvino.is_some() && quiet("export-model.sh", &["--check"]),
        gpu_plan,
        drivers,
        bench,
    })
}

fn interactive(scripts: &Path, no_drivers: bool, no_bench: bool) -> Result<()> {
    let hw = Hardware::detect(&hardware_sources());
    let mut model = resolve(None, Vec::new())?.settings().model;
    let plan_view = |model| -> Result<SetupView> {
        let facts = facts(scripts, &hw, model, !no_drivers, !no_bench)?;
        Ok(SetupView::new(
            &hw.inventory(),
            setup::not_needed(&hw),
            setup::plan(&hw, &facts),
        ))
    };
    let mut view = plan_view(model)?;
    let mut screen = Screen::new()?;
    let mut notes = Vec::new();
    loop {
        screen.draw(|frame| view.render(frame))?;
        match view.on_key(screen.key()?) {
            Action::None => {}
            Action::Quit => {
                drop(screen);
                eprintln!("Setup cancelled: nothing was changed.");
                return Ok(());
            }
            Action::ChangeModel => match model::choose(&mut screen)? {
                Some(Chosen::Model(chosen)) => {
                    let changed = chosen.id != model.id;
                    model = chosen;
                    view = plan_view(model)?;
                    // Installed or not, it must still become the one in use.
                    if changed {
                        view.enable(StepKind::Model);
                    }
                }
                Some(Chosen::File(path)) => {
                    notes.extend(model::select_path(&path)?);
                    view.remove(StepKind::Model);
                }
                None => {}
            },
            Action::Start => break,
        }
    }

    for kind in view.enabled() {
        if view.stopping() {
            view.set(kind, State::Skipped("stopped".to_owned()));
            continue;
        }
        view.set(kind, State::Running);
        screen.draw(|frame| view.render(frame))?;
        let result = match kind {
            StepKind::LlamaCpp => install_llamacpp_step(&mut screen, &mut view),
            StepKind::Model => install_model(&mut screen, &mut view, model),
            StepKind::GpuDrivers => {
                with_sudo(&mut screen, &scripts.join("setup-gpu.sh"), &["--yes"])
            }
            StepKind::NpuStack => with_sudo(&mut screen, &scripts.join("setup-npu.sh"), &[]),
            StepKind::Openvino => with_sudo(
                &mut screen,
                &scripts.join("setup-npu.sh"),
                &["--openvino-only"],
            ),
            StepKind::ExportModel => piped(
                &mut screen,
                &mut view,
                &scripts.join("export-model.sh"),
                &[],
            ),
            StepKind::Bench if render_group_pending() => Err(
                "you were just added to the render group: log out and back in, then run \
                 `smart-commit bench`"
                    .to_owned(),
            ),
            StepKind::Bench => bench(&mut screen),
        };
        let state = match result {
            Ok(note) => State::Done(note),
            Err(why) if kind == StepKind::Bench && why.contains("render group") => {
                State::Skipped(why)
            }
            Err(why) => State::Failed(why),
        };
        view.set(kind, state);
    }
    for note in &notes {
        view.log(note);
    }
    view.finish("Setup finished. Press Enter.".to_owned());
    loop {
        screen.draw(|frame| view.render(frame))?;
        if view.on_key(screen.key()?) == Action::Quit {
            break;
        }
    }
    drop(screen);
    let failed = view
        .results()
        .iter()
        .any(|(_, state)| matches!(state, State::Failed(_)));
    println!("Setup:");
    for (title, state) in view.results() {
        let (word, why) = match state {
            State::Done(note) => ("done", note),
            State::Failed(why) => ("failed", why),
            State::Skipped(why) => ("skipped", why),
            State::Pending | State::Running => continue,
        };
        let why = if why.is_empty() {
            String::new()
        } else {
            format!(": {why}")
        };
        println!("  {word:<8} {title}{why}");
    }
    for note in notes {
        println!("{note}");
    }
    if !failed {
        print!("{NEXT_STEPS}");
    }
    Ok(())
}

/// Runs `script` with its output in the setup's log.
fn piped(
    screen: &mut Screen,
    view: &mut SetupView,
    script: &Path,
    args: &[&str],
) -> Result<String, String> {
    let mut child = Command::new(script)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("cannot run {}: {err}", script.display()))?;
    let (sender, lines) = mpsc::channel::<String>();
    let readers: Vec<_> = [
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    .map(|pipe| {
        let sender = sender.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(pipe).split(b'\n').map_while(Result::ok) {
                let _ = sender.send(String::from_utf8_lossy(&line).into_owned());
            }
        })
    })
    .collect();
    drop(sender);
    let status = loop {
        while let Ok(line) = lines.try_recv() {
            view.log(&line);
        }
        if let Ok(Some(status)) = child.try_wait() {
            break status;
        }
        screen
            .draw(|frame| view.render(frame))
            .map_err(|e| format!("{e:#}"))?;
        if let Some(key) = screen
            .key_within(Duration::from_millis(100))
            .map_err(|e| format!("{e:#}"))?
        {
            view.on_key(key);
        }
    };
    for reader in readers {
        let _ = reader.join();
    }
    while let Ok(line) = lines.try_recv() {
        view.log(&line);
    }
    if status.success() {
        Ok(String::new())
    } else {
        Err(status.to_string())
    }
}

/// Runs `script` outside the full screen, where sudo can ask for the
/// password, and waits for Enter before coming back.
fn with_sudo(screen: &mut Screen, script: &Path, args: &[&str]) -> Result<String, String> {
    let status = screen
        .suspend(|| {
            println!("\n==> {} {}", script.display(), args.join(" "));
            let status = Command::new(script).args(args).status();
            println!("\nPress Enter to go back to the setup.");
            let _ = std::io::stdin().lock().read_line(&mut String::new());
            status
        })
        .map_err(|e| format!("{e:#}"))?
        .map_err(|err| format!("cannot run {}: {err}", script.display()))?;
    if status.success() {
        Ok(String::new())
    } else {
        Err(status.to_string())
    }
}

/// Installs llama.cpp with a progress bar.
fn install_llamacpp_step(screen: &mut Screen, view: &mut SetupView) -> Result<String, String> {
    view.log(&format!(
        "llama.cpp {} ({}) from github.com/ggml-org/llama.cpp",
        llamacpp::LLAMA_CPP_BUILD,
        models::size(llamacpp::install::ARCHIVE_SIZE)
    ));
    let (sender, progress) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        install_llamacpp(&mut |bytes| {
            let _ = sender.send(bytes);
        })
        .map_err(|e| format!("{e:#}"))
    });
    while !worker.is_finished() {
        while let Ok(bytes) = progress.try_recv() {
            view.progress(
                format!(
                    "llama.cpp: {} of {}",
                    models::size(bytes),
                    models::size(llamacpp::install::ARCHIVE_SIZE)
                ),
                bytes as f64 / llamacpp::install::ARCHIVE_SIZE as f64,
            );
        }
        screen
            .draw(|frame| view.render(frame))
            .map_err(|e| format!("{e:#}"))?;
        if let Some(key) = screen
            .key_within(Duration::from_millis(150))
            .map_err(|e| format!("{e:#}"))?
        {
            view.on_key(key);
        }
    }
    let server = worker
        .join()
        .map_err(|_| "the download crashed".to_owned())??;
    view.log(&format!("installed: {}", server.display()));
    let devices = llamacpp::install::list_devices(&server);
    for line in devices.lines() {
        view.log(line);
    }
    Ok(String::new())
}

/// Downloads `model` with a progress bar, then makes it the one in use.
fn install_model(
    screen: &mut Screen,
    view: &mut SetupView,
    model: &'static Model,
) -> Result<String, String> {
    let dir = paths::default_models_dir(env).ok_or("HOME is not set")?;
    let base = env("SMART_COMMIT_HF_BASE").unwrap_or_else(|| models::HUGGING_FACE.to_owned());
    view.log(&format!(
        "{} ({}, {}) from huggingface.co/{}",
        model.name,
        models::size(model.gguf.size),
        model.license,
        model.gguf.repo
    ));
    let (sender, progress) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        models::install_gguf(model, &dir, &base, |bytes| {
            let _ = sender.send(bytes);
        })
    });
    while !worker.is_finished() {
        while let Ok(bytes) = progress.try_recv() {
            view.progress(
                format!(
                    "{}: {} of {}",
                    model.name,
                    models::size(bytes),
                    models::size(model.gguf.size)
                ),
                bytes as f64 / model.gguf.size.max(1) as f64,
            );
        }
        screen
            .draw(|frame| view.render(frame))
            .map_err(|e| format!("{e:#}"))?;
        if let Some(key) = screen
            .key_within(Duration::from_millis(150))
            .map_err(|e| format!("{e:#}"))?
        {
            view.on_key(key);
        }
    }
    let file = worker
        .join()
        .map_err(|_| "the download crashed".to_owned())?
        .map_err(|err| err.to_string())?;
    view.log(&format!(
        "checked against the pinned SHA-256: {}",
        file.display()
    ));
    for line in model::select(model).map_err(|e| format!("{e:#}"))? {
        view.log(&line);
    }
    Ok(String::new())
}

/// The live benchmark, then back to the setup.
fn bench(screen: &mut Screen) -> Result<String, String> {
    let settings = resolve(None, Vec::new())
        .map_err(|e| format!("{e:#}"))?
        .settings();
    let benchmarked =
        super::benchmark::bench_in(screen, &settings).map_err(|e| format!("{e:#}"))?;
    let settings =
        match super::benchmark::apply(benchmarked.choice).map_err(|e| format!("{e:#}"))? {
            Some((_, settings)) => settings,
            None => settings,
        };
    let target = match settings.device {
        smart_commit::config::DeviceSetting::Fixed(target) => Some(target),
        smart_commit::config::DeviceSetting::Auto => benchmarked.report.fastest(),
    };
    match target.and_then(|t| benchmarked.report.entry(t).map(|e| (t, e))) {
        Some((target, entry)) => Ok(match &entry.measurement {
            Some(m) => format!(
                "({target}, ~{:.1} s per message)",
                m.per_message_ms / 1000.0
            ),
            None => format!("({target})"),
        }),
        None => Err("no device could run the model".to_owned()),
    }
}

/// Added to the render group by setup-npu.sh, which only applies to new
/// logins: the NPU cannot be used until then.
fn render_group_pending() -> bool {
    let output = |program: &str, args: &[&str]| {
        Command::new(program)
            .args(args)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default()
    };
    let current = output("id", &["-nG"]);
    let user = env("USER").unwrap_or_default();
    let listed = output("getent", &["group", "render"]);
    !current.split_whitespace().any(|g| g == "render")
        && listed
            .trim()
            .rsplit(':')
            .next()
            .is_some_and(|members| members.split(',').any(|m| !user.is_empty() && m == user))
}

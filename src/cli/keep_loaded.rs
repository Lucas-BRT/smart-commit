//! `smart-commit server`: the models kept loaded between commits.

use clap::Subcommand;
use smart_commit::{paths, server};
use std::error::Error;

use super::settings::env;

#[derive(Debug, Subcommand)]
pub(super) enum ServerCommand {
    /// List the models kept loaded, and how long each has been idle.
    Status,
    /// Stop every model kept loaded.
    Stop,
}

pub(super) fn run(command: ServerCommand) -> Result<(), Box<dyn Error>> {
    let Some(runtime) = paths::runtime_dir(env) else {
        println!("No model is kept loaded: $XDG_RUNTIME_DIR is not set.");
        return Ok(());
    };
    match command {
        ServerCommand::Status => {
            let kept = server::status(&runtime);
            if kept.is_empty() {
                println!("No model is kept loaded (model.keep_loaded).");
            }
            for k in kept {
                let name = k.spec.options.model.file_name().unwrap_or_default();
                println!(
                    "{} on {}: idle {} s, stops after {} s idle",
                    name.to_string_lossy(),
                    k.spec.options.device,
                    k.idle.as_secs(),
                    k.spec.keep.as_secs()
                );
            }
        }
        ServerCommand::Stop => println!("Stopped {} model(s).", server::stop(&runtime)),
    }
    Ok(())
}

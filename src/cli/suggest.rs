//! `smart-commit` without a subcommand: write the message for what is
//! staged, and optionally commit it.

use std::time::Instant;

use anyhow::Result;
use smart_commit::generate::Source;
use smart_commit::git::Repo;
use smart_commit::message::{CommitMessage, SanitizeError};
use smart_commit::tui::review::{self, Review};
use smart_commit::tui::{self, Screen};

use super::args::{BackendArgs, GenerateArgs};
use super::session::Session;

pub(super) fn run(args: &GenerateArgs) -> Result<()> {
    let repo = Repo::discover(&args.path)?;
    if args.commit && tui::interactive() {
        return review_and_commit(&repo, &args.backend);
    }
    let message = generate_message(&repo, &args.backend)?.to_string();
    if args.commit {
        repo.commit_in_editor(&message)?;
    } else {
        println!("{message}");
    }
    Ok(())
}

/// `--commit` in a terminal: shows the message, then commits it, opens the
/// editor, asks the model for another one or gives up.
fn review_and_commit(repo: &Repo, args: &BackendArgs) -> Result<()> {
    let started = Instant::now();
    let session = Session::start(repo, args)?;
    let generation = session.generate(&[])?;
    let mut review = Review::new(
        &generation,
        session.running(),
        started.elapsed().as_secs_f32(),
        &session.context.staged,
    );
    let mut screen = Screen::new()?;
    let action = loop {
        screen.draw(|frame| review.render(frame))?;
        match review.on_key(screen.key()?) {
            review::Action::None => {}
            review::Action::Another => {
                review.working();
                screen.draw(|frame| review.render(frame))?;
                let started = Instant::now();
                let generation = session.generate(&review.turned_down)?;
                review.replace(&generation, started.elapsed().as_secs_f32());
            }
            action => break action,
        }
    };
    drop(screen);
    let message = review.message.to_string();
    match action {
        review::Action::Commit => repo.commit(&message)?,
        review::Action::Edit => repo.commit_in_editor(&message)?,
        _ => eprintln!("Cancelled: nothing was committed."),
    }
    Ok(())
}

/// Generates a message for what is staged in `repo`, writing the trace if
/// asked and warning when the fallback had to be used.
pub(super) fn generate_message(repo: &Repo, args: &BackendArgs) -> Result<CommitMessage> {
    let generation = Session::start(repo, args)?.generate(&[])?;
    if generation.source == Source::Fallback {
        let copied = generation
            .attempts
            .iter()
            .all(|a| matches!(a.rejected, Some(SanitizeError::CopiedHistory(_))));
        if copied {
            tracing::warn!(
                "the model only repeated earlier commit messages; \
                 using a message built from the file list"
            );
        } else {
            tracing::warn!(
                "the model's output was unusable after {} attempts; \
                 using a message built from the file list",
                generation.attempts.len()
            );
        }
    }
    Ok(generation.message)
}

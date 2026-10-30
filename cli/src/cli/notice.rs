//! The notice of a newer release, after the commands a person runs by hand
//! (see `smart_commit::update_notice`).

use std::io::IsTerminal as _;
use std::time::{SystemTime, UNIX_EPOCH};

use smart_commit::{paths, update_notice};

use super::settings::{env, resolve};
use super::ui::note;

/// Prints the notice if `update.check` is on, stderr is a terminal and a day
/// has passed since the last request. Never fails: a command's result does
/// not depend on it.
pub(super) fn print() {
    // `SMART_COMMIT_NOTICE_TTY` lets tests stand in for a terminal.
    let terminal = std::io::stderr().is_terminal() || env("SMART_COMMIT_NOTICE_TTY").is_some();
    if !terminal || !resolve(None, vec![]).is_ok_and(|resolved| resolved.update_check()) {
        return;
    }
    let Some(cache) = paths::cache_root(env).map(|dir| dir.join(update_notice::CACHE_FILE)) else {
        return;
    };
    let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return;
    };
    let current = env!("CARGO_PKG_VERSION");
    let fetch = || update_notice::fetch_latest(current);
    if let Some(line) = update_notice::check(&cache, current, now.as_secs(), fetch) {
        note!("{line}");
    }
}

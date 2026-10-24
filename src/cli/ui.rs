//! What a command tells the person running it, apart from its output.
//!
//! Notes are part of a command's result (what it chose, what overrides what),
//! not diagnostics, so they are always printed. Problems are `tracing`
//! warnings and errors, and the pipeline's decisions are `tracing` debug
//! events (see `logging`).

/// Prints `note: ...` on stderr.
macro_rules! note {
    ($($arg:tt)*) => {
        eprintln!("note: {}", format_args!($($arg)*))
    };
}

pub(super) use note;

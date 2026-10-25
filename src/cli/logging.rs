//! Diagnostics on stderr: `error:` and `warning:` lines, plus `debug:` ones
//! with `-v`. (`note:` lines are not diagnostics: see `ui`.)

use std::fmt;

use tracing::{Event, Level, Subscriber};
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::registry::LookupSpan;

/// One line per event, `level: message`, as the commands always printed.
struct Plain;

impl<S, N> FormatEvent<S, N> for Plain
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let meta = event.metadata();
        match *meta.level() {
            Level::ERROR => write!(writer, "error: ")?,
            Level::WARN => write!(writer, "warning: ")?,
            _ => write!(writer, "debug: {}: ", meta.target())?,
        }
        ctx.field_format().format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

/// Shows warnings and errors; `-v` adds what the pipeline decided, `-vv` more.
pub(super) fn init(verbose: u8) {
    let level = match verbose {
        0 => Level::WARN,
        1 => Level::DEBUG,
        _ => Level::TRACE,
    };
    // A second call (tests in one process) keeps the first subscriber.
    let _ = tracing_subscriber::fmt()
        .event_format(Plain)
        .with_max_level(level)
        .with_writer(std::io::stderr)
        .try_init();
}

//! Infers the commit message style of a repository from its history.
//!
//! Everything here is deterministic and works on plain commit subjects, so
//! it can be tested without git or an NPU.

#[allow(clippy::module_inception)]
#[cfg(test)]
mod tests;

mod detect;
mod gitmoji;
mod language;
mod parse;
mod ticket;
mod types;

pub use detect::{detect, scope_areas};
pub use gitmoji::is_gitmoji;
pub use language::{language_of, words};
pub use parse::parse_conventional;
pub use ticket::{find_ticket_fn as find_ticket, ticket_span_fn as ticket_span};
pub use types::{Convention, ConventionalHeader, Language, Style, TicketStyle};

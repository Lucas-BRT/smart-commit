//! Ticket detection and classification.

use super::TicketStyle;
use super::language::description_of;

pub fn find_ticket(text: &str) -> Option<&str> {
    ticket_span(text).map(|(start, end)| &text[start..end])
}

fn ticket_span(text: &str) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    (0..bytes.len()).find_map(|start| {
        let at_boundary = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
        if !at_boundary || !bytes[start].is_ascii_uppercase() {
            return None;
        }
        let project_end = start
            + bytes[start..]
                .iter()
                .take_while(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
                .count();
        if bytes.get(project_end) != Some(&b'-') {
            return None;
        }
        let number_start = project_end + 1;
        let end = number_start
            + bytes[number_start..]
                .iter()
                .take_while(|b| b.is_ascii_digit())
                .count();
        let followed_by_alnum = bytes.get(end).is_some_and(u8::is_ascii_alphanumeric);
        (end > number_start && !followed_by_alnum).then_some((start, end))
    })
}

pub fn ticket_span_fn(text: &str) -> Option<(usize, usize)> {
    ticket_span(text)
}

pub fn find_ticket_fn(text: &str) -> Option<&str> {
    find_ticket(text)
}

pub fn detect_tickets_for_subjects(subjects: &[&str]) -> TicketStyle {
    // Position 1 covers `[ABC-123]` and `(ABC-123)`.
    let starts_with_ticket = |text: &str| ticket_span(text).is_some_and(|(start, _)| start <= 1);
    let citing: Vec<&str> = subjects
        .iter()
        .copied()
        .filter(|s| find_ticket(s).is_some())
        .collect();
    const TICKET_THRESHOLD: f64 = 0.5;
    if subjects.is_empty() || (citing.len() as f64 / subjects.len() as f64) < TICKET_THRESHOLD {
        return TicketStyle::None;
    }
    let prefixed = citing
        .iter()
        .filter(|s| starts_with_ticket(s) || starts_with_ticket(description_of(s)))
        .count();
    if prefixed * 2 > citing.len() {
        TicketStyle::Prefix
    } else {
        TicketStyle::Mention
    }
}

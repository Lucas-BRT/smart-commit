//! Gitmoji detection.

fn is_shortcode_char(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-' | '+')
}

/// Covers the blocks gitmoji draws from; not a complete emoji table.
fn is_emoji(c: char) -> bool {
    matches!(
        c as u32,
        0x1F000..=0x1FAFF   // pictographs, emoticons, transport, symbols
            | 0x2300..=0x23FF   // misc technical (⏪, ⌛)
            | 0x2600..=0x27BF   // misc symbols, dingbats (♻, ✨, ✅)
            | 0x2B00..=0x2BFF   // arrows (⬆, ⬇)
    )
}

/// Whether the subject starts with an emoji or a `:shortcode:`.
pub fn is_gitmoji(subject: &str) -> bool {
    if let Some(rest) = subject.strip_prefix(':') {
        return rest
            .find(':')
            .is_some_and(|end| end > 0 && rest[..end].chars().all(is_shortcode_char));
    }
    subject.chars().next().is_some_and(is_emoji)
}

pub fn is_emoji_char(c: char) -> bool {
    is_emoji(c)
}

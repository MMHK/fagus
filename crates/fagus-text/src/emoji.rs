//! Emoji sequences, which font fallback keeps together as Chrome does: an emoji with the
//! variation selectors, skin tone modifiers, keycap mark and tags that follow it, emoji joined
//! by zero width joiners, and pairs of regional indicators (flags).

use unicode_properties::EmojiStatus;
use unicode_properties::emoji::UnicodeEmoji;

/// How an emoji sequence asks to be shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presentation {
    /// In color, as variation selector 16 asks: a color font comes before the fonts of the text.
    Emoji,
    /// In color by default (an emoji whose default presentation is emoji, or a sequence): among
    /// the fonts beyond those of the text, color fonts come first.
    DefaultEmoji,
    /// As text, by default or as variation selector 15 asks: among the fonts beyond those of the
    /// text, fonts without color come first.
    Text,
}

/// Whether a character is a variation selector, which selects a form of the character before
/// it and has no glyph of its own.
pub fn is_variation_selector(character: char) -> bool {
    matches!(character, '\u{FE00}'..='\u{FE0F}' | '\u{E0100}'..='\u{E01EF}')
}

/// Whether a character may start an emoji sequence: an emoji other than ASCII, or a keycap base
/// (`#`, `*` or a digit) that a variation selector or the keycap mark follows.
fn starts_sequence(character: char, next: Option<char>) -> bool {
    if character.is_ascii() {
        matches!(character, '#' | '*' | '0'..='9') && matches!(next, Some('\u{FE0F}' | '\u{20E3}'))
    } else {
        character.is_emoji_char()
    }
}

/// The length in bytes of the emoji sequence at the start of `text`, if one starts there: a
/// single emoji counts, other characters do not.
pub fn sequence_len(text: &str) -> Option<usize> {
    let mut chars = text.char_indices().peekable();
    let (_, first) = chars.next()?;
    if !starts_sequence(first, chars.peek().map(|(_, next)| *next)) {
        return None;
    }
    let regional = unicode_properties::emoji::is_regional_indicator(first);
    let mut flag_done = !regional;
    let mut end = first.len_utf8();
    while let Some(&(offset, character)) = chars.peek() {
        let joins = match character {
            '\u{200D}' => {
                // A joiner takes the emoji after it into the sequence.
                chars.next();
                end = offset + character.len_utf8();
                match chars.peek() {
                    Some(&(next_offset, next)) if !next.is_whitespace() => {
                        chars.next();
                        end = next_offset + next.len_utf8();
                    }
                    _ => {}
                }
                continue;
            }
            '\u{20E3}' | '\u{1F3FB}'..='\u{1F3FF}' | '\u{E0020}'..='\u{E007F}' => true,
            _ if is_variation_selector(character) => true,
            _ if !flag_done && unicode_properties::emoji::is_regional_indicator(character) => {
                flag_done = true;
                true
            }
            _ => false,
        };
        if !joins {
            break;
        }
        chars.next();
        end = offset + character.len_utf8();
    }
    Some(end)
}

/// How an emoji sequence asks to be shown (see [`sequence_len`]).
pub fn presentation(sequence: &str) -> Presentation {
    if sequence.contains('\u{FE0F}') {
        return Presentation::Emoji;
    }
    if sequence.contains('\u{FE0E}') {
        return Presentation::Text;
    }
    let mut chars = sequence.chars();
    let first = chars.next().unwrap_or(' ');
    if chars.next().is_some()
        || matches!(
            first.emoji_status(),
            EmojiStatus::EmojiPresentation
                | EmojiStatus::EmojiPresentationAndModifierBase
                | EmojiStatus::EmojiPresentationAndEmojiComponent
                | EmojiStatus::EmojiPresentationAndModifierAndEmojiComponent
        )
    {
        Presentation::DefaultEmoji
    } else {
        Presentation::Text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_emoji_sequences() {
        let len = |text: &str| sequence_len(text);
        assert_eq!(len("a😀"), None);
        assert_eq!(len("😀b"), Some(4));
        assert_eq!(len("👍🏽!"), Some(8));
        assert_eq!(len("👨\u{200D}👩\u{200D}👧 x"), Some(18));
        assert_eq!(len("❤\u{FE0F}x"), Some(6));
        assert_eq!(len("1\u{FE0F}\u{20E3}2"), Some(7));
        assert_eq!(len("12"), None);
        assert_eq!(len("🇩🇪🇫🇷"), Some(8));
        assert_eq!(len("©"), Some(2));
    }

    #[test]
    fn tells_the_presentation() {
        assert_eq!(presentation("❤\u{FE0F}"), Presentation::Emoji);
        assert_eq!(presentation("😀\u{FE0E}"), Presentation::Text);
        assert_eq!(presentation("😀"), Presentation::DefaultEmoji);
        assert_eq!(presentation("👍🏽"), Presentation::DefaultEmoji);
        assert_eq!(presentation("❤"), Presentation::Text);
        assert_eq!(presentation("©"), Presentation::Text);
    }
}

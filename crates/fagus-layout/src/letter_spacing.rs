//! Where `letter-spacing` goes, as Blink applies it (`ShapeResultSpacing::ComputeSpacing`): after
//! each grapheme cluster, but not after the letters of a cursive script such as Arabic, whose
//! letters join (CSS Text 4, §8.2.1), nor after characters that take no room.

use fagus_text::cursive_script;

/// Whether each byte of `text` is in a run of a cursive script, as Blink segments text by
/// script: a character of no script of its own (a space, a European digit, punctuation, a mark)
/// takes the script of the last character before it that has one, where `before` gives that
/// of the text before `text` if none of `text` does; at the start, without one, that of the
/// first character after it that has one.
pub(crate) fn cursive_bytes(text: &str, before: Option<bool>) -> Vec<bool> {
    let mut bytes = vec![false; text.len()];
    let mut current = before;
    // Characters before the first with a script of its own take that one's: it is looked for
    // once, not from each of them (thousands of zero width spaces).
    let mut first = None;
    for (offset, character) in text.char_indices() {
        let own = cursive_script(character);
        if own.is_some() {
            current = own;
        }
        let cursive = current.unwrap_or_else(|| {
            *first.get_or_insert_with(|| text[offset..].chars().find_map(cursive_script).unwrap_or(false))
        });
        bytes[offset..offset + character.len_utf8()].fill(cursive);
    }
    bytes
}

/// Whether the last character of `text` that has a script of its own is of a cursive one.
pub(crate) fn ends_cursive(text: &str) -> Option<bool> {
    text.chars().rev().find_map(cursive_script)
}

/// Whether letter spacing goes after `character`, in a run of a cursive script or not: not
/// after the letters of a cursive script, though after its spaces, and never after a character
/// that is set without width (controls, zero width spaces and joiners, bidi marks).
pub(crate) fn spaced(character: char, cursive: bool) -> bool {
    if is_zero_width(character) {
        return false;
    }
    !cursive || matches!(character, ' ' | '\t' | '\n' | '\u{A0}')
}

/// Blink's `TreatAsZeroWidthSpace`: a form feed, a carriage return, the object replacement
/// character, zero width joiners and the default ignorable characters (zero width spaces, bidi
/// controls and isolates, the word joiner, invisible operators, variation selectors …). Other
/// controls are spaced: Chrome draws them with a glyph.
fn is_zero_width(character: char) -> bool {
    matches!(character, '\u{C}' | '\r' | '\u{FFFC}') || crate::inline::is_default_ignorable(character)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_characters_without_a_script_by_the_text_around_them() {
        // Digits and spaces after Arabic are in its run; before it, they take its script too.
        let text = "12 بت 34";
        let bytes = cursive_bytes(text, None);
        assert!(bytes.iter().all(|&cursive| cursive));
        let text = "ab 12";
        assert!(cursive_bytes(text, None).iter().all(|&cursive| !cursive));
        // The text before decides for a run without a script of its own.
        assert!(cursive_bytes("12", Some(true)).iter().all(|&cursive| cursive));
        assert!(cursive_bytes("12", None).iter().all(|&cursive| !cursive));
        // Latin after Arabic starts a run of its own.
        let text = "ب x";
        let bytes = cursive_bytes(text, None);
        assert!(bytes[0] && bytes[2]);
        assert!(!bytes[3]);
        assert_eq!(ends_cursive("abc ب 12"), Some(true));
        assert_eq!(ends_cursive("12"), None);
    }

    #[test]
    fn spaces_the_spaces_of_cursive_scripts_but_not_their_letters() {
        assert!(!spaced('ب', true));
        assert!(spaced(' ', true));
        assert!(spaced('\u{A0}', true));
        assert!(spaced('א', false));
        assert!(!spaced('\u{200C}', false));
        assert!(!spaced('\u{200F}', false));
        // Bidi isolates, invisible operators and the word joiner neither; a line tabulation is.
        for character in [
            '\u{2066}', '\u{2069}', '\u{2060}', '\u{2062}', '\u{FEFF}', '\u{C}',
        ] {
            assert!(!spaced(character, false), "{character:?}");
        }
        assert!(spaced('\u{B}', false));
    }
}

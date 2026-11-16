//! The fonts that Chrome on macOS falls back to for characters that the fonts of the text and
//! the standard font lack: Core Text's cascade list from the text's font, which depends on the
//! kind of that font (a serif face cascades to Songti SC for Chinese characters and to Times New
//! Roman for Hebrew, a sans-serif one to PingFang SC and Lucida Grande …) and on the character.
//!
//! The tables in `cascade_data.rs` come from Chrome: every assigned symbol and two letters of
//! each block of Unicode, printed with Times, a Latin-only web font, Courier, Menlo and Comic Sans
//! MS as the text's font, and the families Chrome embedded put in an order that agrees with which
//! of them has which character.

/// The kind of a face, which decides the cascade list that Core Text falls back along.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Cascade {
    Serif,
    #[default]
    SansSerif,
    Monospace,
    /// Menlo, what `monospace` stands for on macOS, which cascades to Monaco first.
    Menlo,
    /// Handwriting and display faces (Comic Sans MS, Apple Chancery, Marker Felt …).
    Script,
}

/// The families of macOS whose cascade list is the serif one; other faces, also web fonts,
/// take the sans-serif one.
const SERIF: [&str; 28] = [
    "American Typewriter",
    "AppleMyungjo",
    "Athelas",
    "Baskerville",
    "Big Caslon",
    "Bodoni 72",
    "Bodoni 72 Oldstyle",
    "Bodoni 72 Smallcaps",
    "Charter",
    "Cochin",
    "Devanagari MT",
    "Didot",
    "Georgia",
    "Hiragino Mincho Pro",
    "Hiragino Mincho ProN",
    "Hoefler Text",
    "Iowan Old Style",
    "Mshtakan",
    "New York",
    "PT Serif",
    "PT Serif Caption",
    "Palatino",
    "Rockwell",
    "STSong",
    "Songti SC",
    "Songti TC",
    "Times",
    "Times New Roman",
];

/// The fixed-pitch families of macOS, which take the monospace cascade list (Menlo its own).
const MONOSPACE: [&str; 6] = [
    "Andale Mono",
    "Courier",
    "Courier New",
    "Menlo",
    "Monaco",
    "PT Mono",
];

/// The handwriting and display families of macOS that take the script cascade list.
const SCRIPT: [&str; 7] = [
    "Apple Chancery",
    "Chalkboard",
    "Chalkboard SE",
    "Comic Sans MS",
    "Marker Felt",
    "Noteworthy",
    "Zapfino",
];

impl Cascade {
    /// The cascade of a face by its own family name.
    pub(crate) fn of(family: &str) -> Self {
        let named = |names: &[&str]| names.iter().any(|name| name.eq_ignore_ascii_case(family));
        if family.eq_ignore_ascii_case("Menlo") {
            Self::Menlo
        } else if named(&SERIF) {
            Self::Serif
        } else if named(&MONOSPACE) {
            Self::Monospace
        } else if named(&SCRIPT) {
            Self::Script
        } else {
            Self::SansSerif
        }
    }
}

include!("cascade_data.rs");

/// The families that Core Text falls back to for `character` from a face of a kind, in order;
/// the first one installed that has the character is taken.
pub(crate) fn families(character: char, cascade: Cascade) -> &'static [&'static str] {
    // The C0 controls (a line tabulation, a form feed …), which Hiragino maps to a blank glyph.
    if ('\u{1}'..='\u{1F}').contains(&character) {
        return match cascade {
            Cascade::Serif | Cascade::Script => &["Hiragino Mincho ProN"],
            _ => &["Hiragino Sans"],
        };
    }
    let ranges: &[(u32, u32, u16)] = match cascade {
        Cascade::Serif => &SERIF_RANGES,
        Cascade::SansSerif => &SANS_SERIF_RANGES,
        Cascade::Monospace => &MONOSPACE_RANGES,
        Cascade::Menlo => &MENLO_RANGES,
        Cascade::Script => &SCRIPT_RANGES,
    };
    let code = character as u32;
    let index = ranges.partition_point(|(_, end, _)| *end < code);
    match ranges.get(index) {
        Some(&(start, _, list)) if start <= code => LISTS.get(usize::from(list)).copied().unwrap_or(&[]),
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menlo_falls_back_to_monaco_first() {
        assert_eq!(Cascade::of("Menlo"), Cascade::Menlo);
        assert_eq!(Cascade::of("Courier"), Cascade::Monospace);
        // ∴ and ∵, which Menlo lacks, come from Monaco in Chrome.
        for character in ['∴', '∵'] {
            assert_eq!(families(character, Cascade::Menlo).first(), Some(&"Monaco"));
        }
    }

    #[test]
    fn controls_cascade_to_hiragino() {
        // As Chrome draws a line tabulation in Menlo, Courier and Helvetica, and in Georgia and
        // Comic Sans MS.
        for cascade in [Cascade::Menlo, Cascade::Monospace, Cascade::SansSerif] {
            assert_eq!(families('\u{B}', cascade), ["Hiragino Sans"]);
        }
        for cascade in [Cascade::Serif, Cascade::Script] {
            assert_eq!(families('\u{C}', cascade), ["Hiragino Mincho ProN"]);
        }
    }

    #[test]
    fn sans_serif_latin_signs_cascade_to_helvetica() {
        // PingFang SC and Apple SD Gothic Neo lack `×`, `®` and `ł`: Chrome takes Helvetica.
        for character in ['×', '®', '€', 'ł'] {
            assert_eq!(
                families(character, Cascade::SansSerif).first(),
                Some(&"Helvetica")
            );
        }
        assert_eq!(families('a', Cascade::SansSerif), [""; 0]);
    }

    #[test]
    fn serif_currency_signs_fall_back_as_in_chrome() {
        // ₹ in Times comes from Songti SC, the other signs Times lacks from Helvetica and Geneva.
        let list = families('₹', Cascade::Serif);
        let at = |family| list.iter().position(|name| *name == family).expect(family);
        assert!(at("Songti SC") < at("Helvetica"), "{list:?}");
        assert!(at("Helvetica") < at("Geneva"), "{list:?}");
        assert_eq!(Cascade::of("Devanagari MT"), Cascade::Serif);
    }
}

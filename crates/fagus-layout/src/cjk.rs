//! The characters that Chrome counts as CJK ideographs or symbols (`IsCjkIdeographOrSymbol`),
//! around which `text-justify: auto` may add space on both sides. Besides the ideographs, kana
//! and CJK punctuation these are symbols that East Asian text uses among Latin ones (№, ‰, †,
//! ★, ✓, ■, circled numbers), emoji presented as emoji by default and the pictographs of emoji
//! sequences.
//!
//! The ranges and the list of single characters are those of Chromium's
//! `character_property_data.h` and `character_property_data_generator.cc`, under Chromium's
//! BSD license (`CHROMIUM-LICENSE`).

use unicode_properties::EmojiStatus;
use unicode_properties::emoji::UnicodeEmoji;

/// The ranges of CJK ideographs and symbols, inclusive and in order.
const RANGES: [(u32, u32); 82] = [
    (0x2156, 0x215A),
    (0x2160, 0x216B),
    (0x2170, 0x217B),
    (0x23BE, 0x23CC),
    (0x2460, 0x2492),
    (0x249C, 0x24FF),
    (0x25CE, 0x25D3),
    (0x25E2, 0x25E6),
    (0x2600, 0x2603),
    (0x2660, 0x266F),
    (0x2672, 0x267D),
    (0x2776, 0x277F),
    (0x2E80, 0x2FDF),
    (0x2FF0, 0x302D),
    (0x3031, 0x312F),
    (0x3190, 0x31BF),
    (0x31C0, 0x31EF),
    (0x3200, 0x33FF),
    (0x3400, 0x4DBF),
    (0x4DC0, 0x4DFF),
    (0x4E00, 0x9FFF),
    (0xF860, 0xF862),
    (0xF900, 0xFAFF),
    (0xFE30, 0xFE6F),
    (0xFF00, 0xFF0C),
    (0xFF0E, 0xFF1A),
    (0xFF1F, 0xFFEF),
    (0x16FE0, 0x16FFF),
    (0x17000, 0x187FF),
    (0x18800, 0x18AFF),
    (0x1B000, 0x1B0FF),
    (0x1B100, 0x1B12F),
    (0x1B170, 0x1B2FF),
    (0x1F110, 0x1F129),
    (0x1F130, 0x1F149),
    (0x1F150, 0x1F169),
    (0x1F170, 0x1F189),
    (0x1F202, 0x1F219),
    (0x1F21B, 0x1F22E),
    (0x1F230, 0x1F231),
    (0x1F23B, 0x1F24F),
    (0x1F252, 0x1F2FF),
    (0x1F321, 0x1F32A),
    (0x1F394, 0x1F39F),
    (0x1F3CD, 0x1F3CE),
    (0x1F3D4, 0x1F3DF),
    (0x1F3F1, 0x1F3F2),
    (0x1F3F5, 0x1F3F7),
    (0x1F4FD, 0x1F4FE),
    (0x1F53E, 0x1F54A),
    (0x1F568, 0x1F573),
    (0x1F576, 0x1F579),
    (0x1F57B, 0x1F58F),
    (0x1F591, 0x1F594),
    (0x1F597, 0x1F5A3),
    (0x1F5A5, 0x1F5E7),
    (0x1F5E9, 0x1F5FA),
    (0x1F650, 0x1F67F),
    (0x1F6C6, 0x1F6CB),
    (0x1F6CD, 0x1F6CF),
    (0x1F6D3, 0x1F6D4),
    (0x1F6D9, 0x1F6DB),
    (0x1F6E0, 0x1F6EA),
    (0x1F6ED, 0x1F6F3),
    (0x1F6FD, 0x1F6FF),
    (0x1F900, 0x1F90B),
    (0x1FAC9, 0x1FACC),
    (0x20000, 0x2FFFF),
    // Pictographs of RGI emoji ZWJ and modifier sequences that are text by default (Unicode
    // 16 and 17), which Chrome marks as symbols too.
    (0x2194, 0x2195),
    (0x261D, 0x261D),
    (0x2620, 0x2620),
    (0x2640, 0x2640),
    (0x2642, 0x2642),
    (0x2695, 0x2696),
    (0x26A7, 0x26A7),
    (0x26D3, 0x26D3),
    (0x26F9, 0x26F9),
    (0x2708, 0x2708),
    (0x270C, 0x270D),
    (0x2744, 0x2744),
    (0x2764, 0x2764),
    (0x27A1, 0x27A1),
];

/// More pictographs of emoji sequences, beyond the Basic Multilingual Plane.
const SEQUENCE_PICTOGRAPHS: [u32; 9] = [
    0x1F32B, 0x1F3CB, 0x1F3CC, 0x1F3F3, 0x1F441, 0x1F574, 0x1F575, 0x1F590, 0x1F5E8,
];

/// Single characters: Mandarin tone marks and symbols that CJK fonts carry, in order.
const SINGLES: [u32; 77] = [
    0x2C7, 0x2CA, 0x2CB, 0x2D9, 0x2020, 0x2021, 0x2030, 0x203B, 0x203C, 0x2042, 0x2047, 0x2048, 0x2049,
    0x2051, 0x20DD, 0x20DE, 0x2100, 0x2103, 0x2105, 0x2109, 0x210A, 0x2113, 0x2116, 0x2121, 0x212B, 0x213B,
    0x2150, 0x2151, 0x2152, 0x217F, 0x2189, 0x2307, 0x2312, 0x23CE, 0x2423, 0x25A0, 0x25A1, 0x25A2, 0x25AA,
    0x25AB, 0x25B1, 0x25B2, 0x25B3, 0x25B6, 0x25B7, 0x25BC, 0x25BD, 0x25C0, 0x25C1, 0x25C6, 0x25C7, 0x25C9,
    0x25CB, 0x25CC, 0x25EF, 0x2605, 0x2606, 0x260E, 0x2616, 0x2617, 0x26A0, 0x2713, 0x271A, 0x273F, 0x2740,
    0x2756, 0x2763, 0x2B1A, 0xFE10, 0xFE11, 0xFE12, 0xFE19, 0xFF1D, 0x1F100, 0x1F200, 0x1F237, 0x1F32C,
];

/// More single emoji of Chromium's list.
const SINGLE_EMOJI: [u32; 6] = [0x1F336, 0x1F37D, 0x1F43F, 0x1F54F, 0x1F93B, 0x1F946];

/// Whether a character is a CJK ideograph or symbol as Chrome's `IsCjkIdeographOrSymbol` tells:
/// justification may space it out on both sides.
pub(crate) fn is_cjk_ideograph_or_symbol(character: char) -> bool {
    let code = u32::from(character);
    if code < 0x2C7 {
        return false;
    }
    RANGES.iter().any(|&(start, end)| (start..=end).contains(&code))
        || SINGLES.binary_search(&code).is_ok()
        || SEQUENCE_PICTOGRAPHS.contains(&code)
        || SINGLE_EMOJI.contains(&code)
        || matches!(
            character.emoji_status(),
            EmojiStatus::EmojiPresentation
                | EmojiStatus::EmojiPresentationAndModifierBase
                | EmojiStatus::EmojiPresentationAndEmojiComponent
                | EmojiStatus::EmojiPresentationAndModifierAndEmojiComponent
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_ideographs_kana_and_the_symbols_chrome_lists() {
        for character in [
            '漢', 'か', 'カ', '、', '①', '№', '‰', '†', '★', '✓', '■', '🙂', '🇩', '❤',
        ] {
            assert!(is_cjk_ideograph_or_symbol(character), "{character}");
        }
    }

    #[test]
    fn leaves_out_latin_cyrillic_hangul_and_other_symbols() {
        for character in [
            'a', 'é', 'Ж', 'Ω', '한', '€', '©', '→', '•', '–', '\u{3030}', '\u{302E}',
        ] {
            assert!(!is_cjk_ideograph_or_symbol(character), "{character}");
        }
    }

    #[test]
    fn lists_its_single_characters_in_order() {
        assert!(SINGLES.windows(2).all(|pair| pair[0] < pair[1]));
    }
}

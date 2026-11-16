//! Line break opportunities as browsers find them.
//!
//! Between two printable ASCII characters, Blink does not follow the Unicode line breaking
//! algorithm but a table of its own, kept for compatibility with older browsers: a line breaks
//! after spaces, before an opening bracket that follows some punctuation, and after `-` and `?`,
//! but not after `/` or `.` in a URL, nor after `!` or `|` before a letter. Where a character
//! beyond ASCII is involved, the Unicode line breaking algorithm (UAX #14) decides.

use unicode_linebreak::BreakClass;
use unicode_properties::emoji::UnicodeEmoji;

/// A position where a line may or must break.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BreakOpportunity {
    /// Byte offset in the text: the line breaks before this offset.
    pub offset: usize,
    /// Whether the line must break here (e.g. after a line feed).
    pub mandatory: bool,
}

/// How strictly lines break around the punctuation and symbols of Chinese and Japanese text
/// (CSS `line-break`), as Chrome tailors the Unicode line breaking algorithm for each value.
/// The `Cjk` values are those for text in Chinese or Japanese (`lang`), where `normal` and
/// `loose` (and `auto` in Chinese) break around more characters than elsewhere.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Strictness {
    #[default]
    Auto,
    Loose,
    LooseCjk,
    Normal,
    NormalCjk,
    Strict,
    /// A line may break between any two grapheme clusters.
    Anywhere,
}

/// The break opportunities of `text`, in order. The end of the text is always included.
pub fn break_opportunities(text: &str) -> Vec<BreakOpportunity> {
    break_opportunities_with(text, Strictness::Auto)
}

/// The break opportunities of `text` with `line-break` at `strictness`, in order. The end of
/// the text is always included.
pub fn break_opportunities_with(text: &str, strictness: Strictness) -> Vec<BreakOpportunity> {
    if strictness == Strictness::Anywhere {
        return anywhere(text);
    }
    let bytes = text.as_bytes();
    let mut opportunities = Vec::new();
    // After an ASCII character: by Blink's rules, unless a printable one meets one beyond ASCII.
    for offset in 1..bytes.len() {
        let (last, next) = (bytes[offset - 1], bytes[offset]);
        // Nothing breaks after a letter or digit in Blink's table, the most common case.
        if last.is_ascii_alphanumeric()
            || !last.is_ascii()
            || is_printable_ascii(char::from(last)) && !next.is_ascii()
        {
            continue;
        }
        let (last, next) = (char::from(last), char::from(next));
        // Mandatory breaks after a line feed or a carriage return (UAX #14, LB4 and LB5). A line
        // tabulation or form feed is no break at all in Blink's table, though UAX #14 forces one.
        if last == '\n' || last == '\r' && next != '\n' {
            opportunities.push(BreakOpportunity {
                offset,
                mandatory: true,
            });
        } else if if next.is_ascii() {
            breaks_between(offset.checked_sub(2).map(|at| char::from(bytes[at])), last, next)
        } else {
            is_breakable_space(last) && !text[offset..].starts_with(is_mark)
        } {
            opportunities.push(BreakOpportunity {
                offset,
                mandatory: false,
            });
        }
    }
    // Elsewhere, where a character beyond ASCII is involved, the Unicode line breaker decides.
    // Only the words with such characters need it: no rule of UAX #14 looks across spaces into a
    // word, and breaks after spaces follow Blink's rule.
    if !text.is_ascii() {
        let from_ascii = opportunities.len();
        let mut word_start = 0;
        for (index, &byte) in bytes
            .iter()
            .enumerate()
            .chain(std::iter::once((bytes.len(), &b' ')))
        {
            if !is_breakable_space(char::from(byte)) {
                continue;
            }
            let word = &text[word_start..index];
            let start = word_start;
            word_start = index + 1;
            if word.is_ascii() || joins_letters(word) && !word.chars().any(is_aksara_script) {
                continue;
            }
            let tailored = tailored(word, strictness);
            let breaks = unicode_linebreak::linebreaks(tailored.as_deref().unwrap_or(word))
                .chain(complex_context_breaks(word))
                .chain(aksara_breaks(word))
                .chain(hyphen_breaks_after_hebrew(word))
                .chain(loose_breaks_before(word, strictness));
            for (offset, _) in breaks {
                let offset = start + offset;
                if offset == start || offset >= index {
                    continue;
                }
                let (last, next) = (bytes[offset - 1], bytes[offset]);
                if last.is_ascii() && (next.is_ascii() || !is_printable_ascii(char::from(last))) {
                    continue;
                }
                // After a line or paragraph separator (or a next line) Blink takes the break
                // that UAX #14 forces as one that a line may take: only line feeds force one.
                opportunities.push(BreakOpportunity {
                    offset,
                    mandatory: false,
                });
            }
        }
        if opportunities.len() > from_ascii {
            opportunities.sort_unstable_by_key(|opportunity| opportunity.offset);
            opportunities.dedup_by_key(|opportunity| opportunity.offset);
        }
    }
    opportunities.push(BreakOpportunity {
        offset: text.len(),
        mandatory: false,
    });
    opportunities
}

/// ICU's line breaker with its dictionaries, made once.
fn icu_line_segmenter() -> &'static icu_segmenter::LineSegmenterBorrowed<'static> {
    static SEGMENTER: std::sync::OnceLock<icu_segmenter::LineSegmenterBorrowed<'static>> =
        std::sync::OnceLock::new();
    SEGMENTER.get_or_init(|| {
        icu_segmenter::LineSegmenter::new_dictionary(icu_segmenter::options::LineBreakOptions::default())
    })
}

/// The break opportunities inside the runs of `word` in scripts that UAX #14 leaves to an
/// analysis of the context (class SA: Thai, Lao, Khmer, Myanmar), which have no spaces between
/// words: between the words of a run, as ICU's dictionaries find them for Chrome.
fn complex_context_breaks(word: &str) -> Vec<(usize, unicode_linebreak::BreakOpportunity)> {
    let complex =
        |character: char| unicode_linebreak::break_property(character as u32) == BreakClass::ComplexContext;
    if !word.chars().any(complex) {
        return Vec::new();
    }
    icu_line_segmenter()
        .segment_str(word)
        .filter(|&offset| {
            offset > 0
                && offset < word.len()
                && word[..offset].chars().next_back().is_some_and(complex)
                && word[offset..].chars().next().is_some_and(complex)
        })
        .map(|offset| (offset, unicode_linebreak::BreakOpportunity::Allowed))
        .collect()
}

/// Whether `character` is of a Brahmic script whose letters Unicode 15.1 gave the classes of
/// orthographic syllables (AK, AP, AS, VI, VF, VB), which `unicode-linebreak` does not know.
fn is_aksara_script(character: char) -> bool {
    matches!(
        u32::from(character),
        0x1900..=0x194F // Limbu
            | 0x1A00..=0x1A1F // Buginese
            | 0x1B00..=0x1B7F // Balinese
            | 0x1B80..=0x1BBF // Sundanese
            | 0x1BC0..=0x1BFF // Batak
            | 0x1C00..=0x1C4F // Lepcha
            | 0xA800..=0xA82F // Syloti Nagri
            | 0xA880..=0xA8DF // Saurashtra
            | 0xA900..=0xA95F // Kayah Li, Rejang
            | 0xA980..=0xA9DF // Javanese
            | 0xAA00..=0xAA5F // Cham
            | 0x11000..=0x1124F // Brahmi to Khojki
            | 0x11280..=0x1137F // Multani to Grantha
            | 0x11400..=0x114DF // Newa, Tirhuta
            | 0x11580..=0x116CF // Siddham, Modi, Takri
            | 0x11800..=0x1184F // Dogra
            | 0x11900..=0x11AAF // Dives Akuru to Soyombo
            | 0x11C00..=0x11CBF // Bhaiksuki, Marchen
            | 0x11D00..=0x11DAF // Masaram Gondi, Gunjala Gondi
            | 0x11EE0..=0x11EFF // Makasar
            | 0x11F00..=0x11F5F // Kawi
    )
}

/// The break opportunities inside the runs of `word` in Brahmic scripts that Unicode 15.1
/// breaks between orthographic syllables (UAX #14, `LB28a`), as ICU does for Chrome.
fn aksara_breaks(word: &str) -> Vec<(usize, unicode_linebreak::BreakOpportunity)> {
    if !word.chars().any(is_aksara_script) {
        return Vec::new();
    }
    icu_line_segmenter()
        .segment_str(word)
        .filter(|&offset| {
            offset > 0
                && offset < word.len()
                && word[..offset].chars().next_back().is_some_and(is_aksara_script)
                && word[offset..].chars().next().is_some_and(is_aksara_script)
        })
        .map(|offset| (offset, unicode_linebreak::BreakOpportunity::Allowed))
        .collect()
}

/// `word` with the characters the line breaker should see otherwise, as Chrome tailors the
/// Unicode line breaking algorithm for `line-break` (CSS Text 3, §5.3): small kana and the
/// prolonged sound mark (class CJ) break like other kana (class ID) but with `strict`; with
/// `normal` and `loose` the wave dash and the katakana-hiragana double hyphen do too, and with
/// `loose` the characters of [`breaks_loosely`]. `None` where nothing changes.
fn tailored(word: &str, strictness: Strictness) -> Option<String> {
    let like_kana = |character: char| {
        // Only characters three bytes long in UTF-8, as is the kana they become: offsets stay.
        character.len_utf8() == 3
            && match strictness {
                Strictness::Strict => false,
                Strictness::Auto | Strictness::Normal | Strictness::Anywhere => {
                    is_conditional_starter(character)
                }
                Strictness::NormalCjk => {
                    is_conditional_starter(character) || matches!(character, '\u{301C}' | '\u{30A0}')
                }
                Strictness::Loose => is_conditional_starter(character) || breaks_loosely(character, false),
                Strictness::LooseCjk => {
                    is_conditional_starter(character)
                        || matches!(character, '\u{301C}' | '\u{30A0}')
                        || breaks_loosely(character, true)
                }
            }
    };
    word.chars().any(like_kana).then(|| {
        word.chars()
            .map(|character| {
                if like_kana(character) {
                    '\u{3042}'
                } else {
                    character
                }
            })
            .collect()
    })
}

/// Whether `character` is a small kana or the prolonged sound mark (class CJ).
fn is_conditional_starter(character: char) -> bool {
    unicode_linebreak::break_property(character as u32) == BreakClass::ConditionalJapaneseStarter
}

/// The characters that `line-break: loose` lets a line break before, and after, beside
/// Chinese and Japanese characters, as in Chrome: iteration marks and inseparable characters,
/// and in Chinese and Japanese text (`cjk`) hyphens, centered punctuation, exclamation and
/// question marks, some postfix symbols and the fullwidth currency signs.
fn breaks_loosely(character: char, cjk: bool) -> bool {
    let everywhere = matches!(
        character,
        // Iteration marks.
        '\u{3005}' | '\u{303B}' | '\u{309D}' | '\u{309E}' | '\u{30FD}' | '\u{30FE}'
        // Inseparable characters.
        | '\u{2024}' | '\u{2025}' | '\u{2026}' | '\u{22EF}' | '\u{FE19}'
    );
    everywhere
        || cjk
            && matches!(
                character,
                // Hyphens.
                |'\u{2010}'| '\u{2013}'
        // Centered punctuation.
        | '\u{30FB}' | '\u{FF1A}' | '\u{FF1B}' | '\u{FF65}'
        // Exclamation and question marks.
        | '\u{FF01}' | '\u{FF1F}' | '\u{203C}' | '\u{2047}' | '\u{2048}' | '\u{2049}'
        // Postfix symbols.
        | '\u{2030}' | '\u{2032}' | '\u{2033}' | '\u{2035}' | '\u{2103}' | '\u{FF05}' | '\u{FFE0}'
        // Prefix symbols.
        | '\u{2116}' | '\u{FF04}' | '\u{FFE1}' | '\u{FFE5}' | '\u{FFE6}'
            )
}

/// The breaks that `line-break: loose` adds before the degree sign after a Chinese or Japanese
/// character, which [`tailored`] cannot turn into kana (it is two bytes long in UTF-8).
fn loose_breaks_before(
    word: &str,
    strictness: Strictness,
) -> impl Iterator<Item = (usize, unicode_linebreak::BreakOpportunity)> + '_ {
    let ideographic = |character: char| {
        matches!(
            unicode_linebreak::break_property(character as u32),
            BreakClass::Ideographic
                | BreakClass::ConditionalJapaneseStarter
                | BreakClass::HangulLvSyllable
                | BreakClass::HangulLvtSyllable
        )
    };
    let characters: Vec<(usize, char)> = if strictness == Strictness::LooseCjk {
        word.char_indices().collect()
    } else {
        Vec::new()
    };
    (1..characters.len()).filter_map(move |index| {
        let [(_, before), (offset, next)] = [characters[index - 1], characters[index]];
        (next == '\u{00B0}' && (ideographic(before) || breaks_loosely(before, true)))
            .then_some((offset, unicode_linebreak::BreakOpportunity::Allowed))
    })
}

/// The break opportunities of `text` with `line-break: anywhere`: between any two grapheme
/// clusters (as far as combining marks, joiners and variation selectors keep characters
/// together), with the forced breaks of the text.
fn anywhere(text: &str) -> Vec<BreakOpportunity> {
    let mut opportunities: Vec<BreakOpportunity> = break_opportunities_with(text, Strictness::Auto)
        .into_iter()
        .filter(|opportunity| opportunity.mandatory)
        .collect();
    let mut last: Option<char> = None;
    for (offset, character) in text.char_indices() {
        let joins = matches!(
            unicode_linebreak::break_property(character as u32),
            BreakClass::CombiningMark | BreakClass::ZeroWidthJoiner
        ) || matches!(character, '\u{FE00}'..='\u{FE0F}' | '\u{E0100}'..='\u{E01EF}')
            // A joiner keeps an emoji after it in the cluster (UAX #29, GB11), not other
            // characters (`B\u{200D}C` breaks after the joiner, as in Chrome).
            || last == Some('\u{200D}') && !character.is_ascii() && character.is_emoji_char()
            || last == Some('\r') && character == '\n';
        if let Some(previous) = last
            && !joins
            && !matches!(previous, '\n' | '\r')
        {
            opportunities.push(BreakOpportunity {
                offset,
                mandatory: false,
            });
        }
        last = Some(character);
    }
    opportunities.sort_unstable_by_key(|opportunity| opportunity.offset);
    opportunities.dedup_by_key(|opportunity| opportunity.offset);
    opportunities.push(BreakOpportunity {
        offset: text.len(),
        mandatory: false,
    });
    opportunities.dedup_by_key(|opportunity| opportunity.offset);
    opportunities
}

/// The breaks after a hyphen between Hebrew letters (`שלום-עולם`), which the Unicode line
/// breaking algorithm allows since Unicode 15.1 (`LB21a` only keeps a hyphen after a Hebrew
/// letter with what follows if that is not one) and Chrome finds; the line breaker this crate
/// uses keeps them together.
fn hyphen_breaks_after_hebrew(
    word: &str,
) -> impl Iterator<Item = (usize, unicode_linebreak::BreakOpportunity)> + '_ {
    let class = |character: char| unicode_linebreak::break_property(character as u32);
    let characters: Vec<(usize, char)> = word.char_indices().collect();
    (2..characters.len()).filter_map(move |index| {
        let [(_, before), (_, hyphen), (offset, next)] =
            [characters[index - 2], characters[index - 1], characters[index]];
        (class(before) == BreakClass::HebrewLetter
            && matches!(hyphen, '-' | '\u{2010}')
            && class(next) == BreakClass::HebrewLetter)
            .then_some((offset, unicode_linebreak::BreakOpportunity::Allowed))
    })
}

/// The break opportunities of Japanese `text` with `word-break: auto-phrase` and `line-break` at
/// `strictness`, as ICU finds them with `lw=phrase`: letters and digits of any script stay
/// together, as with `keep-all`, but for the boundaries of phrases (see
/// [`crate::phrase_breaks`]). The end of the text is always included.
pub fn phrase_opportunities_with(text: &str, strictness: Strictness) -> Vec<BreakOpportunity> {
    let mut opportunities = break_opportunities_with(text, strictness);
    opportunities.retain(|opportunity| {
        opportunity.mandatory || opportunity.offset == text.len() || !keeps_all_at(text, opportunity.offset)
    });
    let phrases = crate::phrase_breaks(text);
    if phrases.is_empty() {
        return opportunities;
    }
    opportunities.extend(phrases.into_iter().map(|offset| BreakOpportunity {
        offset,
        mandatory: false,
    }));
    opportunities.sort_by_key(|opportunity| (opportunity.offset, !opportunity.mandatory));
    opportunities.dedup_by_key(|opportunity| opportunity.offset);
    opportunities
}

/// Whether `word-break: keep-all` keeps the characters before and after byte `at` of `text`
/// together: letters and numbers of any script but those of complex context (as Blink's
/// `ShouldKeepAfterKeepAll` tells, looking past a combining mark before `at`).
pub fn keeps_all_at(text: &str, at: usize) -> bool {
    let mut before = text[..at].chars().rev();
    let (Some(last), Some(next)) = (before.next(), text[at..].chars().next()) else {
        return false;
    };
    let class = |character: char| unicode_linebreak::break_property(character as u32);
    let last = if class(last) == BreakClass::CombiningMark {
        before.next().unwrap_or(last)
    } else {
        last
    };
    let keeps =
        |character: char| character.is_alphanumeric() && class(character) != BreakClass::ComplexContext;
    keeps(last) && keeps(next)
}

/// Whether `word-break: break-all` lets a line break at byte `at` of `text`, between the
/// characters around it.
pub fn breaks_all_at(text: &str, at: usize) -> bool {
    let start = text[..at]
        .char_indices()
        .rev()
        .take(2)
        .last()
        .map_or(at, |(offset, _)| offset);
    let end = text[at..].chars().next().map_or(at, |next| at + next.len_utf8());
    break_all_opportunities(&text[start..end]).contains(&(at - start))
}

/// Whether every pair in `word` with a character beyond ASCII is one of letters, digits and
/// combining marks, between which UAX #14 never breaks (LB9, LB23, LB28): most words with umlauts
/// need no Unicode line breaker.
fn joins_letters(word: &str) -> bool {
    let joins = |character: char| {
        character.is_ascii_alphanumeric()
            || !character.is_ascii()
                && matches!(
                    unicode_linebreak::break_property(character as u32),
                    BreakClass::Alphabetic
                        | BreakClass::HebrewLetter
                        | BreakClass::Numeric
                        | BreakClass::CombiningMark
                )
    };
    let mut characters = word.chars();
    let Some(mut last) = characters.next() else {
        return true;
    };
    for next in characters {
        if !(last.is_ascii() && next.is_ascii() || joins(last) && joins(next)) {
            return false;
        }
        last = next;
    }
    true
}

/// Whether a line may break between two ASCII characters (Blink's `LazyLineBreakIterator`): after
/// a run of spaces, else by Blink's table.
fn breaks_between(before_last: Option<char>, last: char, next: char) -> bool {
    if is_breakable_space(next) {
        false
    } else if is_breakable_space(last) {
        true
    } else {
        is_printable_ascii(last) && is_printable_ascii(next) && breaks_between_ascii(before_last, last, next)
    }
}

/// Whether `character` is a combining mark, which stays with a space it follows: the space is
/// its base, which it is shaped on.
fn is_mark(character: char) -> bool {
    use unicode_properties::{GeneralCategoryGroup, UnicodeGeneralCategory};
    character >= '\u{300}' && character.general_category_group() == GeneralCategoryGroup::Mark
}

fn is_breakable_space(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\n')
}

fn is_printable_ascii(character: char) -> bool {
    ('!'..='~').contains(&character)
}

/// Blink's table for two printable ASCII characters (`kAsciiLineBreakTable`).
fn breaks_between_ascii(before_last: Option<char>, last: char, next: char) -> bool {
    match last {
        // A hyphen after a letter or digit breaks before a digit ("RE-2026", "1234-5678" in
        // long URLs); one that may be a minus sign does not ("(-1").
        '-' if next.is_ascii_digit() => {
            before_last.is_some_and(|character| character.is_ascii_alphanumeric())
        }
        '-' => !"!$),./:;?]}".contains(next),
        '?' => !"!\"'),./:;?]}".contains(next),
        '!' | '"' | '#' | '%' | '&' | ')' | '*' | '+' | ',' | '.' | ':' | ';' | '=' | '>' | '\\' | ']'
        | '|' | '}' | '~' => "(<[{".contains(next),
        _ => false,
    }
}

/// The positions inside `text` where `word-break: break-all` lets a line break in addition to
/// its break opportunities: between the letters and digits of words and around them, by the
/// classes of UAX #14 as Blink pairs them (CSS Text 3, §5.2).
pub fn break_all_opportunities(text: &str) -> Vec<usize> {
    let mut positions = Vec::new();
    let mut last: Option<(BreakClass, bool)> = None;
    for (offset, character) in text.char_indices() {
        // Blink treats `+` as a letter here, as Internet Explorer did.
        let class = if character == '+' {
            BreakClass::Alphabetic
        } else {
            unicode_linebreak::break_property(character as u32)
        };
        if last.is_some_and(|last| breaks_all_between(last, (class, character.is_ascii()))) {
            positions.push(offset);
        }
        if class != BreakClass::CombiningMark {
            last = Some((class, character.is_ascii()));
        }
    }
    positions
}

/// Blink's table of additional breaks for `word-break: break-all`
/// (`kBreakAllLineBreakClassTable`), for the classes of alphabetic scripts, with whether each
/// character is ASCII: as measured in Chrome, it breaks before `-` and `|` but not before a dash
/// or a thin space beyond ASCII, nor after them.
fn breaks_all_between(
    (last, last_ascii): (BreakClass, bool),
    (next, next_ascii): (BreakClass, bool),
) -> bool {
    use BreakClass::{
        After, Alphabetic, Ambiguous, CloseParenthesis, ClosePunctuation, ComplexContext, Exclamation,
        HebrewLetter, Hyphen, InfixSeparator, Numeric, OpenPunctuation, Postfix, Prefix, Symbol,
    };
    let letters = matches!(
        next,
        Alphabetic | Ambiguous | HebrewLetter | Numeric | OpenPunctuation | ComplexContext
    );
    let hyphens = next == Hyphen || next == After && next_ascii;
    match last {
        Alphabetic | Ambiguous | HebrewLetter | Numeric | Symbol | ComplexContext | ClosePunctuation
        | CloseParenthesis => letters || hyphens || next == Prefix,
        After => letters || hyphens && last_ascii || next == Prefix,
        InfixSeparator => letters || hyphens,
        Exclamation | Postfix => letters || hyphens || matches!(next, Prefix | Postfix),
        Prefix => next == Postfix,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offsets(text: &str) -> Vec<usize> {
        break_opportunities(text).iter().map(|b| b.offset).collect()
    }

    #[test]
    fn breaks_javanese_between_orthographic_syllables() {
        // Unicode 15.1 (UAX #14, LB28a), as ICU breaks it for Chrome: between syllables, not
        // inside them (a consonant with its vowel sign or virama).
        let text = "\u{A9CB}\u{A9B1}\u{A9A7}\u{A9BC}\u{A9A4}\u{A9C0}\u{A9B2}\u{A9B8}";
        let breaks = offsets(text);
        assert!(breaks.len() > 2, "{breaks:?}");
        for offset in &breaks[..breaks.len() - 1] {
            let next = text[*offset..].chars().next().expect("a character");
            assert!(
                !matches!(next, '\u{A9B3}'..='\u{A9C0}'),
                "{breaks:?} before a sign at {offset}"
            );
        }
        // Latin words of letters alone still skip the Unicode line breaker.
        assert_eq!(offsets("word"), [4]);
    }

    #[test]
    fn breaks_thai_between_words_found_in_a_dictionary() {
        // As ICU breaks it for Chrome: ภาษา|ไทย|ง่าย|นิด|เดียว, and after the space.
        let text = "ภาษาไทยง่ายนิดเดียว ok";
        let words: Vec<&str> = {
            let mut start = 0;
            offsets(text)
                .into_iter()
                .map(|end| {
                    let word = &text[start..end];
                    start = end;
                    word
                })
                .collect()
        };
        assert_eq!(words, ["ภาษา", "ไทย", "ง่าย", "นิด", "เดียว ", "ok"]);
        // Latin text next to Thai keeps its own breaks.
        assert_eq!(offsets("abc"), [3]);
    }

    #[test]
    fn keeps_a_mark_with_the_space_it_is_on() {
        // A combining acute on a space: the line breaks after it, not between the two.
        assert_eq!(offsets("a \u{301}b"), [5]);
        assert_eq!(offsets("a \u{301} b"), [5, 6]);
    }

    #[test]
    fn breaks_after_spaces_and_hyphens() {
        // As in Chrome, a hyphen after a letter breaks before a number too.
        assert_eq!(offsets("Rechnung Nr. RE-2026"), [9, 13, 16, 20]);
        assert_eq!(offsets("Kunden-Portal"), [7, 13]);
        // A minus sign stays with its number.
        assert_eq!(offsets("Saldo (-12,50)"), [6, 14]);
    }

    #[test]
    fn breaks_ascii_like_chrome() {
        // No break after a slash or a dot: URLs and "und/oder" stay whole.
        assert_eq!(offsets("https://example.com/rechnungen"), [30]);
        assert_eq!(offsets("und/oder"), [8]);
        // Before an opening bracket after punctuation, and after a question mark.
        assert_eq!(offsets("a,(b) wo?hin"), [2, 6, 9, 12]);
        // Beyond ASCII, UAX #14 decides: after an en dash, not inside a word with umlauts.
        assert_eq!(offsets("Mo–Fr Mützenträger"), [5, 8, 22]);
    }

    #[test]
    fn tailors_the_unicode_rules_like_chrome() {
        // Before small kana and the prolonged sound mark, as between other kana.
        assert_eq!(offsets("アップ"), [3, 6, 9]);
        assert_eq!(offsets("あーい"), [3, 6, 9]);
        // After a hyphen between Hebrew letters.
        assert_eq!(offsets("שלום-עולם"), [9, 17]);
    }

    fn strict_offsets(text: &str, strictness: Strictness) -> Vec<usize> {
        break_opportunities_with(text, strictness)
            .iter()
            .map(|b| b.offset)
            .collect()
    }

    #[test]
    fn breaks_by_line_break_like_chrome() {
        // `strict` keeps small kana with the character before.
        assert_eq!(strict_offsets("アップ", Strictness::Strict), [6, 9]);
        // `loose` breaks before iteration marks, in any language.
        assert_eq!(offsets("時々"), [6]);
        assert_eq!(strict_offsets("時々", Strictness::Loose), [3, 6]);
        // Before fullwidth exclamation marks and the degree sign only in Chinese and Japanese.
        assert_eq!(strict_offsets("ああ！あ", Strictness::Loose), [3, 9, 12]);
        assert_eq!(strict_offsets("ああ！あ", Strictness::LooseCjk), [3, 6, 9, 12]);
        assert_eq!(strict_offsets("あ°C", Strictness::LooseCjk), [3, 6]);
        // `normal` breaks before the wave dash there.
        assert_eq!(strict_offsets("あ〜あ", Strictness::Normal), [6, 9]);
        assert_eq!(strict_offsets("あ〜あ", Strictness::NormalCjk), [3, 6, 9]);
        // `anywhere` between any two characters but combining marks.
        assert_eq!(strict_offsets("ab c", Strictness::Anywhere), [1, 2, 3, 4]);
        assert_eq!(strict_offsets("a\u{301}b", Strictness::Anywhere), [3, 4]);
        assert!(break_opportunities_with("a\nb", Strictness::Anywhere)[1].mandatory);
    }

    #[test]
    fn breaks_anywhere_after_joiners_but_before_joined_emoji() {
        // A zero width joiner stays with the letter before it, not with the one after it, as
        // in Chrome (`AB\u{200D}CD` in 2ch: `AB\u{200D}` and `CD`).
        assert_eq!(strict_offsets("ab\u{200D}cd", Strictness::Anywhere), [1, 5, 6, 7]);
        // An emoji ZWJ sequence stays whole (UAX #29, GB11).
        assert_eq!(
            strict_offsets("\u{1F468}\u{200D}\u{1F469}x", Strictness::Anywhere),
            [11, 12]
        );
    }

    #[test]
    fn keeps_and_breaks_all_between_two_characters() {
        assert!(keeps_all_at("日本語", 3));
        assert!(!keeps_all_at("日本、語", 9));
        assert!(breaks_all_at("abcd", 2));
        assert!(!breaks_all_at("ab.", 2));
    }

    #[test]
    fn breaks_all_between_letters() {
        assert_eq!(break_all_opportunities("ab1"), [1, 2]);
        // Not before a dot or a closing bracket, but after the dot.
        assert_eq!(break_all_opportunities("a.b)"), [2]);
        // `+` counts as a letter; nothing breaks after an opening bracket.
        assert_eq!(break_all_opportunities("(a+b"), [2, 3]);
    }

    #[test]
    fn finds_the_same_breaks_beside_characters_beyond_ascii() {
        for text in [
            "Rechnung Nr. RE-2026 (-12,50) wo?hin a,(b)",
            "a\nb\r\nc\rd",
            "  x  ",
        ] {
            // A character beyond ASCII at the end changes nothing before it.
            let mut slow: Vec<BreakOpportunity> = break_opportunities(&format!("{text}ä"))
                .into_iter()
                .filter(|opportunity| opportunity.offset < text.len())
                .collect();
            slow.push(BreakOpportunity {
                offset: text.len(),
                mandatory: false,
            });
            assert_eq!(break_opportunities(text), slow, "{text:?}");
        }
    }

    #[test]
    fn breaks_japanese_between_phrases() {
        let offsets = |text: &str| -> Vec<usize> {
            phrase_opportunities_with(text, Strictness::NormalCjk)
                .iter()
                .map(|opportunity| opportunity.offset)
                .collect()
        };
        // After へ, not between the other kana and kanji; not before the full stop.
        assert_eq!(offsets("東京へ行きましょう。"), [9, 30]);
        // Latin words keep their own breaks.
        assert_eq!(offsets("a b"), [2, 3]);
    }

    #[test]
    fn forces_breaks_only_after_line_feeds() {
        // A line tabulation and a form feed join the letters around them, as in Blink's table.
        assert_eq!(offsets("a\x0Bb\x0Cc"), [5]);
        // After a line or paragraph separator a line may break, but need not.
        let breaks = break_opportunities("a\u{2028}b\u{2029}c");
        assert_eq!(
            breaks
                .iter()
                .map(|opportunity| opportunity.offset)
                .collect::<Vec<_>>(),
            [4, 8, 9]
        );
        assert!(breaks.iter().all(|opportunity| !opportunity.mandatory));
    }

    #[test]
    fn marks_forced_breaks() {
        let breaks = break_opportunities("a\nb");
        assert_eq!(
            breaks[0],
            BreakOpportunity {
                offset: 2,
                mandatory: true
            }
        );
        assert!(!breaks[1].mandatory);
    }
}

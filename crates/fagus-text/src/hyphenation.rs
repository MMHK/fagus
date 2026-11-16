//! Hyphenation as Chrome hyphenates on Linux, Windows, `ChromeOS` and Android, the same on every
//! operating system Fagus runs on.
//!
//! Chrome on those systems hyphenates with Blink's `HyphenationMinikin`
//! (`third_party/blink/renderer/platform/text/hyphenation/`), which runs Android's Minikin
//! hyphenator (Liang's algorithm, as Blink copies it in `hyphenator_aosp.cc`, Apache License 2.0,
//! Copyright 2015 The Android Open Source Project) over the `.hyb` dictionaries of Chromium's
//! `third_party/hyphenation-patterns`. This module ports both (Blink's part under its BSD
//! license, `CHROMIUM-LICENSE`) and embeds those dictionaries, Brotli-compressed, in
//! `hyphenation/` (their licenses are in `hyphenation/LICENSE.txt`). Chrome on macOS hyphenates
//! with Core Foundation instead, which breaks words elsewhere; Fagus follows Linux everywhere.
//!
//! Offsets inside a word are counted in UTF-16 code units, as in Blink, and converted to byte
//! offsets of the text at the end.

use std::sync::OnceLock;

use unicode_linebreak::BreakClass;
use unicode_properties::{GeneralCategory, GeneralCategoryGroup, UnicodeGeneralCategory};

/// The dictionaries, by the locale of their file (`hyph-<locale>.hyb`), with their size once
/// decompressed.
const DICTIONARIES: [(&str, &[u8], usize); 48] = [
    ("af", include_bytes!("../hyphenation/af.hyb.br"), 72640),
    ("as", include_bytes!("../hyphenation/as.hyb.br"), 703),
    ("be", include_bytes!("../hyphenation/be.hyb.br"), 6098),
    ("bg", include_bytes!("../hyphenation/bg.hyb.br"), 3467),
    ("bn", include_bytes!("../hyphenation/bn.hyb.br"), 703),
    ("cs", include_bytes!("../hyphenation/cs.hyb.br"), 64245),
    ("cu", include_bytes!("../hyphenation/cu.hyb.br"), 52842),
    ("cy", include_bytes!("../hyphenation/cy.hyb.br"), 35913),
    ("da", include_bytes!("../hyphenation/da.hyb.br"), 6967),
    (
        "de-1901",
        include_bytes!("../hyphenation/de-1901.hyb.br"),
        121_393,
    ),
    (
        "de-1996",
        include_bytes!("../hyphenation/de-1996.hyb.br"),
        120_412,
    ),
    (
        "de-ch-1901",
        include_bytes!("../hyphenation/de-ch-1901.hyb.br"),
        120_218,
    ),
    ("el", include_bytes!("../hyphenation/el.hyb.br"), 4219),
    ("en-gb", include_bytes!("../hyphenation/en-gb.hyb.br"), 46607),
    ("en-us", include_bytes!("../hyphenation/en-us.hyb.br"), 59802),
    ("es", include_bytes!("../hyphenation/es.hyb.br"), 14995),
    ("et", include_bytes!("../hyphenation/et.hyb.br"), 21421),
    ("eu", include_bytes!("../hyphenation/eu.hyb.br"), 665),
    ("fr", include_bytes!("../hyphenation/fr.hyb.br"), 8165),
    ("ga", include_bytes!("../hyphenation/ga.hyb.br"), 35824),
    ("gl", include_bytes!("../hyphenation/gl.hyb.br"), 9289),
    ("gu", include_bytes!("../hyphenation/gu.hyb.br"), 655),
    ("hi", include_bytes!("../hyphenation/hi.hyb.br"), 687),
    ("hr", include_bytes!("../hyphenation/hr.hyb.br"), 3031),
    ("it", include_bytes!("../hyphenation/it.hyb.br"), 2512),
    ("ka", include_bytes!("../hyphenation/ka.hyb.br"), 9996),
    ("kn", include_bytes!("../hyphenation/kn.hyb.br"), 711),
    ("la", include_bytes!("../hyphenation/la.hyb.br"), 1839),
    ("lt", include_bytes!("../hyphenation/lt.hyb.br"), 7774),
    ("ml", include_bytes!("../hyphenation/ml.hyb.br"), 776),
    ("mn-cyrl", include_bytes!("../hyphenation/mn-cyrl.hyb.br"), 5142),
    ("mr", include_bytes!("../hyphenation/mr.hyb.br"), 687),
    ("mul-ethi", include_bytes!("../hyphenation/mul-ethi.hyb.br"), 3740),
    ("nb", include_bytes!("../hyphenation/nb.hyb.br"), 145_263),
    ("nl", include_bytes!("../hyphenation/nl.hyb.br"), 77080),
    ("nn", include_bytes!("../hyphenation/nn.hyb.br"), 145_263),
    ("or", include_bytes!("../hyphenation/or.hyb.br"), 647),
    ("pa", include_bytes!("../hyphenation/pa.hyb.br"), 607),
    ("pt", include_bytes!("../hyphenation/pt.hyb.br"), 1414),
    ("sk", include_bytes!("../hyphenation/sk.hyb.br"), 64103),
    ("sl", include_bytes!("../hyphenation/sl.hyb.br"), 6631),
    ("sq", include_bytes!("../hyphenation/sq.hyb.br"), 2013),
    ("sv", include_bytes!("../hyphenation/sv.hyb.br"), 72119),
    ("ta", include_bytes!("../hyphenation/ta.hyb.br"), 554),
    ("te", include_bytes!("../hyphenation/te.hyb.br"), 703),
    ("tk", include_bytes!("../hyphenation/tk.hyb.br"), 2712),
    ("uk", include_bytes!("../hyphenation/uk.hyb.br"), 19417),
    ("und-ethi", include_bytes!("../hyphenation/und-ethi.hyb.br"), 3484),
];

/// The dictionaries once decompressed, on first use; `None` for one that cannot be read.
static LOADED: [OnceLock<Option<Box<[u8]>>>; DICTIONARIES.len()] =
    [const { OnceLock::new() }; DICTIONARIES.len()];

/// The locales whose dictionary is another one, from CLDR as Android compiles it
/// (`HyphenationMinikin::CreateLocaleFallbackMap`): the dictionary, and for `en` alone the one
/// that the locale itself takes (other English locales fall back to British English).
const FALLBACKS: [(&str, &str, Option<&str>); 33] = [
    ("en-as", "en-us", None),
    ("en-gu", "en-us", None),
    ("en-mh", "en-us", None),
    ("en-mp", "en-us", None),
    ("en-pr", "en-us", None),
    ("en-um", "en-us", None),
    ("en-vi", "en-us", None),
    ("en", "en-gb", Some("en-us")),
    ("de", "de-1996", None),
    ("de-li-1901", "de-ch-1901", None),
    ("no", "nb", None),
    ("mn", "mn-cyrl", None),
    ("am", "und-ethi", None),
    ("byn", "und-ethi", None),
    ("gez", "und-ethi", None),
    ("ti", "und-ethi", None),
    ("wal", "und-ethi", None),
    ("und-beng", "bn", None),
    ("und-deva", "hi", None),
    ("und-gujr", "gu", None),
    ("und-guru", "pa", None),
    ("und-knda", "kn", None),
    ("und-mlym", "ml", None),
    ("und-orya", "or", None),
    ("und-taml", "ta", None),
    ("und-telu", "te", None),
    ("de-1901", "de-1901", None),
    ("de-1996", "de-1996", None),
    ("de-ch-1901", "de-ch-1901", None),
    ("en-gb", "en-gb", None),
    ("en-us", "en-us", None),
    ("mn-cyrl", "mn-cyrl", None),
    ("und-ethi", "und-ethi", None),
];

/// `HyphenationMinikin::MapLocale`: the locale of the dictionary for `locale`, from the
/// fallbacks of the locale and of its prefixes (`de-AT` → `de` → `de-1996`), or the locale
/// itself where none has one.
fn map_locale(locale: &str) -> &str {
    let mut mapped = locale;
    loop {
        if let Some((_, to, exact)) = FALLBACKS
            .iter()
            .find(|(from, ..)| from.eq_ignore_ascii_case(mapped))
        {
            return match exact {
                Some(exact) if mapped.len() == locale.len() => exact,
                _ => to,
            };
        }
        match mapped.rfind('-') {
            Some(cut) if cut > 0 => mapped = &mapped[..cut],
            _ => return mapped,
        }
    }
}

/// The dictionary for the BCP 47 tag `lang`, as `Hyphenation::PlatformGetHyphenation` finds
/// it: it maps the locale until it maps to itself, and opens `hyph-<locale>.hyb` in ASCII
/// lowercase; with whether the locale is English (whose capitalized words stay whole).
fn dictionary(lang: &str) -> Option<(Dictionary<'static>, bool)> {
    let mut locale = lang;
    loop {
        let mapped = map_locale(locale);
        if mapped.eq_ignore_ascii_case(locale) {
            break;
        }
        locale = mapped;
    }
    let index = DICTIONARIES
        .iter()
        .position(|(name, ..)| name.eq_ignore_ascii_case(locale))?;
    let data = LOADED[index]
        .get_or_init(|| {
            let (_, compressed, size) = DICTIONARIES[index];
            decompress(compressed, size).map(Vec::into_boxed_slice)
        })
        .as_deref()?;
    let english = locale
        .get(..2)
        .is_some_and(|start| start.eq_ignore_ascii_case("en"));
    Some((Dictionary::new(data)?, english))
}

/// The limits of `hyphenate-limit-chars` as Blink takes them (`Hyphenation::SetLimits`): the
/// fewest characters of a word that is hyphenated, before a hyphen and after it, from the
/// values of the property with 0 for `auto` (5, 2 and 2). A limit before the hyphen is the
/// limit after it too where that is `auto`, and words shorter than both together stay whole.
fn limits([word, before, after]: [u32; 3]) -> (usize, usize, usize) {
    let size = |limit: u32| usize::try_from(limit).unwrap_or(usize::MAX);
    let (before, after) = match (size(before), size(after)) {
        (0, 0) => (2, 2),
        (0, after) => (2, after),
        (before, 0) => (before, before),
        limits => limits,
    };
    let word = if word == 0 { 5 } else { size(word) };
    (word.max(before.saturating_add(after)), before, after)
}

/// Where the words of `text` may be hyphenated, as byte offsets, for the language of the BCP 47
/// tag `lang` (`de`, `de-DE`, `en-GB` …), with the limits of `hyphenate-limit-chars` (0 for
/// `auto`: words of five characters or more, two before a hyphen and two after it). Words are
/// the text between the line break opportunities of UAX #14; each is hyphenated with
/// [`hyphenate_word`].
pub fn hyphenation_points_limited(text: &str, lang: &str, limits: [u32; 3]) -> Vec<usize> {
    let mut points = Vec::new();
    let mut start = 0;
    let ends = crate::break_opportunities(text)
        .into_iter()
        .map(|opportunity| opportunity.offset)
        .chain(std::iter::once(text.len()));
    for end in ends {
        if end > start {
            points.extend(
                hyphenate_word(&text[start..end], lang, limits)
                    .into_iter()
                    .map(|point| start + point),
            );
            start = end;
        }
    }
    points
}

/// Where the word `word` (the text between two line break opportunities) may be hyphenated, as
/// byte offsets from its start in ascending order, for the language of the BCP 47 tag `lang`,
/// with the limits of `hyphenate-limit-chars` (see [`hyphenation_points_limited`]).
///
/// As Blink's `HyphenationMinikin::HyphenLocations`: spaces, opening punctuation and quotation
/// marks before the word and spaces, punctuation and control characters after it are left
/// out; a word shorter than the word limit, or an English one that starts with a capital
/// letter, stays whole. Minikin then hyphenates words of 5 to 62 UTF-16 code units whose
/// characters are all in the dictionary's alphabet (so not those with digits or apostrophes),
/// never before the second character or after the third last, and other words only after
/// their soft hyphens.
pub fn hyphenate_word(word: &str, lang: &str, limits: [u32; 3]) -> Vec<usize> {
    let Some((dictionary, english)) = dictionary(lang) else {
        return Vec::new();
    };
    let (min_word, before, after) = self::limits(limits);
    let trimmed = word.trim_start_matches(skips_leading);
    let leading = word.len() - trimmed.len();
    let trimmed = trimmed.trim_end_matches(skips_trailing);
    let units: Vec<u16> = trimmed.encode_utf16().collect();
    if units.len() < min_word
        || english
            && units
                .first()
                .and_then(|&first| char::from_u32(u32::from(first)))
                .is_some_and(|first| first.general_category() == GeneralCategory::UppercaseLetter)
    {
        return Vec::new();
    }
    let hyphens = dictionary.hyphenate(&units);
    // The byte offset of each UTF-16 code unit that starts a character.
    let mut offsets = vec![None; units.len()];
    let mut unit = 0;
    for (offset, character) in trimmed.char_indices() {
        offsets[unit] = Some(leading + offset);
        unit += character.len_utf16();
    }
    let last = units.len().saturating_sub(after);
    (before..=last)
        .filter(|&index| hyphens.get(index).is_some_and(|&hyphen| hyphen != 0))
        .filter_map(|index| offsets.get(index).copied().flatten())
        .collect()
}

/// Whether a character before a word is left out of it: spaces (`TreatAsSpace`), and opening
/// punctuation and quotation marks (line breaking classes OP and QU).
fn skips_leading(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\n' | '\u{A0}')
        || matches!(
            unicode_linebreak::break_property(u32::from(character)),
            BreakClass::OpenPunctuation | BreakClass::Quotation
        )
}

/// Whether a character after a word is left out of it: space separators, punctuation and
/// control characters.
fn skips_trailing(character: char) -> bool {
    let category = character.general_category();
    category == GeneralCategory::SpaceSeparator
        || category == GeneralCategory::Control
        || character.general_category_group() == GeneralCategoryGroup::Punctuation
}

/// Minikin never hyphenates before the second character of a word or after its third last.
const MIN_PREFIX: usize = 2;
const MIN_SUFFIX: usize = 3;
/// The longest word Minikin hyphenates with its patterns, with the codes for its start and end,
/// in UTF-16 code units.
const MAX_HYPHENATED_SIZE: usize = 64;
const SOFT_HYPHEN: u16 = 0xAD;
/// The first four bytes of a `.hyb` file.
const MAGIC: u32 = 0x62ad_7968;

/// A `.hyb` dictionary (Minikin's `doc/hyb_file_format.md`): an alphabet that maps the
/// characters of words, in either case, to codes; a trie of the patterns over those codes; and
/// the patterns' values.
#[derive(Clone, Copy)]
struct Dictionary<'a> {
    data: &'a [u8],
    alphabet: usize,
    trie: usize,
    patterns: usize,
}

impl<'a> Dictionary<'a> {
    /// The dictionary in `data`, if it starts as one.
    fn new(data: &'a [u8]) -> Option<Self> {
        let header = |index: usize| read_u32(data, index * 4).and_then(|value| usize::try_from(value).ok());
        (read_u32(data, 0)? == MAGIC).then_some(())?;
        Some(Self {
            data,
            alphabet: header(2)?,
            trie: header(3)?,
            patterns: header(4)?,
        })
    }

    /// The `u32` at byte `offset` of the dictionary, 0 past its end.
    fn u32(&self, offset: usize) -> u32 {
        read_u32(self.data, offset).unwrap_or(0)
    }

    /// `Hyphenator::hyphenate`: one value for each code unit of `word`, 1 where a hyphen may
    /// go before it.
    fn hyphenate(&self, word: &[u16]) -> Vec<u8> {
        let mut result = vec![0; word.len()];
        if word.len() >= MIN_PREFIX + MIN_SUFFIX
            && word.len() + 2 <= MAX_HYPHENATED_SIZE
            && let Some(codes) = self.codes(word)
        {
            self.hyphenate_codes(&mut result, &codes);
            return result;
        }
        // Words with characters outside the alphabet break only after their soft hyphens.
        for (value, &unit) in result.iter_mut().skip(1).zip(word) {
            *value = u8::from(unit == SOFT_HYPHEN);
        }
        result
    }

    /// `Hyphenator::alphabetLookup`: the codes of the characters of `word`, between the code 0
    /// for its start and end, or `None` where one is not in the alphabet.
    fn codes(&self, word: &[u16]) -> Option<Vec<u16>> {
        let mut codes = Vec::with_capacity(word.len() + 2);
        codes.push(0);
        match self.u32(self.alphabet) {
            0 => {
                let min = self.u32(self.alphabet + 4);
                let max = self.u32(self.alphabet + 8);
                for &unit in word {
                    let unit = u32::from(unit);
                    if unit < min || unit >= max {
                        return None;
                    }
                    let index = self.alphabet + 12 + usize::try_from(unit - min).ok()?;
                    match self.data.get(index) {
                        Some(&code) if code != 0 => codes.push(u16::from(code)),
                        _ => return None,
                    }
                }
            }
            1 => {
                let count = usize::try_from(self.u32(self.alphabet + 4)).ok()?;
                let entry = |index: usize| self.u32(self.alphabet + 8 + index * 4);
                for &unit in word {
                    let key = u32::from(unit) << 11;
                    // The first entry not below the key (`std::lower_bound`).
                    let (mut low, mut high) = (0, count);
                    while low < high {
                        let middle = low + (high - low) / 2;
                        if entry(middle) < key {
                            low = middle + 1;
                        } else {
                            high = middle;
                        }
                    }
                    if low == count || entry(low) >> 11 != u32::from(unit) {
                        return None;
                    }
                    codes.push(u16::try_from(entry(low) & 0x7ff).ok()?);
                }
            }
            _ => return None,
        }
        codes.push(0);
        Some(codes)
    }

    /// `Hyphenator::hyphenateFromCodes`: matches every pattern of the trie at every position of
    /// the codes of a word, keeps the highest value at each place between characters, and
    /// marks the places with odd values (those of exceptions are 10 and 11).
    fn hyphenate_codes(&self, result: &mut [u8], codes: &[u16]) {
        let trie = self.trie;
        let char_mask = self.u32(trie + 4);
        let link_shift = self.u32(trie + 8);
        let link_mask = self.u32(trie + 12);
        let pattern_shift = self.u32(trie + 16);
        let entries = usize::try_from(self.u32(trie + 20)).unwrap_or(0);
        let entry = |index: usize| {
            if index < entries {
                self.u32(trie + 24 + index * 4)
            } else {
                0
            }
        };
        let pattern_count = usize::try_from(self.u32(self.patterns + 4)).unwrap_or(0);
        let buffer = self.patterns + usize::try_from(self.u32(self.patterns + 8)).unwrap_or(0);
        let length = codes.len();
        let max_offset = length - MIN_SUFFIX - 1;
        for start in 0..length - 1 {
            let mut node = 0;
            for (end, &code) in codes.iter().enumerate().skip(start) {
                let code = u32::from(code);
                let link = entry(node + code as usize);
                if link & char_mask != code {
                    break;
                }
                node = ((link & link_mask) >> link_shift) as usize;
                let pattern = (entry(node) >> pattern_shift) as usize;
                if pattern == 0 || pattern >= pattern_count {
                    continue;
                }
                let pattern = self.u32(self.patterns + 16 + pattern * 4);
                let pattern_length = (pattern >> 26) as usize;
                let pattern_shift = ((pattern >> 20) & 0x3f) as usize;
                let values = buffer + (pattern & 0xf_ffff) as usize;
                let Some(values) = self.data.get(values..values + pattern_length) else {
                    continue;
                };
                // The pattern's values end where its match ends, less the zeros it leaves out
                // there; they may start one place before the word, so places are counted from
                // there.
                let Some(first) = (end + 2).checked_sub(pattern_length + pattern_shift) else {
                    continue;
                };
                for (place, &value) in (first..).zip(values) {
                    if (MIN_PREFIX + 1..max_offset + 1).contains(&place) {
                        result[place - 1] = result[place - 1].max(value);
                    }
                }
            }
        }
        for value in result.iter_mut().take(max_offset).skip(MIN_PREFIX) {
            *value &= 1;
        }
    }
}

/// The little-endian `u32` at byte `offset` of `data`.
fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    let bytes = data.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Decompresses Brotli data of `size` bytes.
fn decompress(compressed: &[u8], size: usize) -> Option<Vec<u8>> {
    use brotli_decompressor::{BrotliDecompressStream, BrotliResult, BrotliState};
    let mut output = vec![0; size];
    let (mut available_in, mut input_offset) = (compressed.len(), 0);
    let (mut available_out, mut output_offset, mut total_out) = (size, 0, 0);
    let mut state = BrotliState::new(Heap, Heap, Heap);
    let result = BrotliDecompressStream(
        &mut available_in,
        &mut input_offset,
        compressed,
        &mut available_out,
        &mut output_offset,
        &mut output,
        &mut total_out,
        &mut state,
    );
    (matches!(result, BrotliResult::ResultSuccess) && output_offset == size).then_some(output)
}

/// Memory for the Brotli decoder from the global allocator.
struct Heap;

/// A block of memory of [`Heap`].
#[derive(Default)]
struct Block<T>(Box<[T]>);

impl<T> brotli_decompressor::SliceWrapper<T> for Block<T> {
    fn slice(&self) -> &[T] {
        &self.0
    }
}

impl<T> brotli_decompressor::SliceWrapperMut<T> for Block<T> {
    fn slice_mut(&mut self) -> &mut [T] {
        &mut self.0
    }
}

impl<T: Clone + Default> brotli_decompressor::Allocator<T> for Heap {
    type AllocatedMemory = Block<T>;
    fn alloc_cell(&mut self, len: usize) -> Block<T> {
        Block(vec![T::default(); len].into_boxed_slice())
    }
    fn free_cell(&mut self, _block: Block<T>) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The words of a fixture with their hyphenation points as byte offsets.
    fn fixture(text: &str) -> impl Iterator<Item = (String, Vec<usize>)> + '_ {
        text.lines().filter(|line| !line.starts_with('#')).map(|line| {
            let mut points = Vec::new();
            let mut word = String::new();
            for part in line.split('-') {
                if !word.is_empty() {
                    points.push(word.len());
                }
                word.push_str(part);
            }
            (word, points)
        })
    }

    #[test]
    fn hyphenates_word_lists_as_chrome_on_linux() {
        // Words of public-domain texts, hyphenated by a faithful re-implementation of Minikin
        // over Chromium's `.hyb` files (outside this crate).
        let lists = [
            ("de", include_str!("../tests/hyphenation/de.txt")),
            ("de-1901", include_str!("../tests/hyphenation/de-1901.txt")),
            ("de-CH-1901", include_str!("../tests/hyphenation/de-ch-1901.txt")),
            ("en-US", include_str!("../tests/hyphenation/en-us.txt")),
            ("en-GB", include_str!("../tests/hyphenation/en-gb.txt")),
            ("fr", include_str!("../tests/hyphenation/fr.txt")),
            ("it", include_str!("../tests/hyphenation/it.txt")),
            ("es", include_str!("../tests/hyphenation/es.txt")),
            ("nl", include_str!("../tests/hyphenation/nl.txt")),
            ("pt", include_str!("../tests/hyphenation/pt.txt")),
            ("cs", include_str!("../tests/hyphenation/cs.txt")),
            ("da", include_str!("../tests/hyphenation/da.txt")),
            ("sv", include_str!("../tests/hyphenation/sv.txt")),
            ("nb", include_str!("../tests/hyphenation/nb.txt")),
            ("nn", include_str!("../tests/hyphenation/nn.txt")),
        ];
        for (lang, list) in lists {
            let mut count = 0;
            for (word, points) in fixture(list) {
                assert_eq!(hyphenate_word(&word, lang, [0; 3]), points, "{word} in {lang}");
                count += 1;
            }
            assert!(count >= 600, "{lang}: {count} words");
        }
    }

    #[test]
    fn maps_locales_to_dictionaries_as_blink() {
        // Blink's `HyphenationTest.MapLocale`.
        for (locale, mapped) in [
            ("de-de", "de-1996"),
            ("de-de-xyz", "de-1996"),
            ("de-li", "de-1996"),
            ("de-li-1901", "de-ch-1901"),
            ("en", "en-us"),
            ("en-gu", "en-us"),
            ("en-gu-xyz", "en-us"),
            ("en-xyz", "en-gb"),
            ("en-xyz-xyz", "en-gb"),
            ("fr-ca", "fr"),
            ("fr-fr", "fr"),
            ("fr-fr-xyz", "fr"),
            ("mn-xyz", "mn-cyrl"),
            ("und-Deva-xyz", "hi"),
            ("en-us", "en-us"),
            ("fr", "fr"),
        ] {
            assert_eq!(map_locale(locale), mapped, "{locale}");
        }
        // Australian and Canadian English take the British dictionary, plain English the
        // American one; Norwegian is Bokmål; languages without a dictionary stay whole.
        assert_eq!(hyphenate_word("organisation", "en-AU", [0; 3]), [2, 5, 8]);
        assert_eq!(hyphenate_word("organisation", "en", [0; 3]), [2, 5, 6, 8]);
        assert_eq!(
            hyphenate_word("forsikringsselskap", "no", [0; 3]),
            hyphenate_word("forsikringsselskap", "nb", [0; 3])
        );
        assert!(hyphenate_word("forsikringsselskap", "no", [0; 3]).len() > 2);
        for lang in ["pl", "hu", "ru", "xx", "", "de_DE"] {
            assert_eq!(
                hyphenate_word("konstytucyjny", lang, [0; 3]),
                [0_usize; 0],
                "{lang}"
            );
        }
    }

    #[test]
    fn hyphenates_as_blinks_own_tests() {
        // Blink's `HyphenationTest` for Minikin (`English`, `German`, `LeadingSpaces`,
        // `NonLetters`, `CapitalizedWords`).
        assert_eq!(hyphenate_word("hyphenation", "en-us", [0; 3]), [2, 6, 7]);
        assert_eq!(hyphenate_word("konsonantien", "de-1996", [0; 3]), [3, 5, 8]);
        // After "Bäch", byte 5.
        assert_eq!(hyphenate_word("Bächlein", "de-1996", [0; 3]), [5]);
        assert_eq!(hyphenate_word(" principle", "en-us", [0; 3]), [5, 7]);
        assert_eq!(hyphenate_word("   principle", "en-us", [0; 3]), [7, 9]);
        assert_eq!(hyphenate_word("   ", "en-us", [0; 3]), [0_usize; 0]);
        assert_eq!(hyphenate_word("**********", "en-us", [0; 3]), [0_usize; 0]);
        assert_eq!(hyphenate_word("Hyphenation", "en-us", [0; 3]), [0_usize; 0]);
        assert_eq!(hyphenate_word("Konsonantien", "de-1996", [0; 3]), [3, 5, 8]);
        // Capitalized words stay whole in every English locale, in any case of the tag.
        assert_eq!(hyphenate_word("Keyboard", "EN-us", [0; 3]), [0_usize; 0]);
        assert_eq!(hyphenate_word("Keyboard", "en-AU", [0; 3]), [0_usize; 0]);
        assert_eq!(hyphenate_word("keyboard", "en-US", [0; 3]), [3]);
    }

    #[test]
    fn hyphenates_within_the_limits_of_hyphenate_limit_chars() {
        // Blink's `HyphenationTest.Limits`; `hyphenate-limit-chars` gives the word limit first.
        let example = |limits| hyphenate_word("example", "en-us", limits);
        assert_eq!(example([0, 0, 0]), [2, 4]);
        assert_eq!(example([0, 2, 3]), [2, 4]);
        assert_eq!(example([0, 3, 0]), [4]);
        assert_eq!(example([0, 0, 4]), [2]);
        assert_eq!(example([0, 3, 4]), [0_usize; 0]);
        assert_eq!(example([7, 0, 0]), [2, 4]);
        assert_eq!(example([8, 0, 0]), [0_usize; 0]);
        // Minikin itself never hyphenates before the second letter or after the third last.
        assert_eq!(example([0, 1, 1]), [2, 4]);
        assert_eq!(hyphenate_word("Rechnungsnummer", "de", [0; 3]), [4, 9, 12]);
        assert_eq!(hyphenate_word("Rechnungsnummer", "de", [0, 5, 0]), [9]);
        assert_eq!(hyphenate_word("Rechnung", "de", [9, 0, 0]), [0_usize; 0]);
        assert_eq!(hyphenate_word("Rechnung", "de", [0, 4, 5]), [0_usize; 0]);
    }

    #[test]
    fn leaves_punctuation_around_words_out() {
        // Blink's `HyphenationTest.WordToHyphenate`, and what is left out before and after.
        assert_eq!(hyphenate_word(" ( principle. ).", "en-us", [0; 3]), [7, 9]);
        assert_eq!(hyphenate_word("„Rechnungsnummer“,", "de", [0; 3]), [7, 12, 15]);
        assert_eq!(
            hyphenate_word("«anticonstitutionnellement»", "fr", [0; 3]).len(),
            8
        );
        // A word of four letters stays whole, with the punctuation that makes it five.
        assert_eq!(hyphenate_word("(Haus)", "de", [0; 3]), [0_usize; 0]);
    }

    #[test]
    fn leaves_words_with_other_characters_whole() {
        // Characters outside the dictionary's alphabet (digits, apostrophes but in French,
        // hyphens) leave the whole word unhyphenated, and so does a word of more than 62 UTF-16
        // code units.
        for word in [
            "RE2026Rechnung",
            "Rechnung2026",
            "l’organisation",
            "don't-understand",
        ] {
            assert_eq!(hyphenate_word(word, "de", [0; 3]), [0_usize; 0], "{word}");
        }
        assert_eq!(hyphenate_word("l’organisation", "fr", [0; 3]), [6, 8, 10, 12]);
        assert_eq!(hyphenate_word("doesn't", "en", [0; 3]), [0_usize; 0]);
        let long = "donaudampfschifffahrtsgesellschaftskapitänswitwenrentenversicherung";
        assert_eq!(hyphenate_word(&long[..60], "de", [0; 3]).len(), 15);
        assert_eq!(hyphenate_word(long, "de", [0; 3]), [0_usize; 0]);
        // A word with a soft hyphen in it breaks after it alone.
        assert_eq!(hyphenate_word("Rech\u{AD}nungsnummer", "de", [0; 3]), [6]);
    }

    #[test]
    fn finds_the_hyphenation_points_of_the_words_of_a_text() {
        let text = "Rechnungsnummer und Umsatzsteuer, Umsatz-steuernummer";
        let points = hyphenation_points_limited(text, "de-DE", [0; 3]);
        let parts: Vec<&str> = std::iter::once(0)
            .chain(points.iter().copied())
            .zip(points.iter().copied().chain(std::iter::once(text.len())))
            .map(|(start, end)| &text[start..end])
            .collect();
        assert_eq!(
            parts,
            [
                "Rech",
                "nungs",
                "num",
                "mer und Um",
                "satz",
                "steuer, Um",
                "satz-steu",
                "er",
                "num",
                "mer"
            ]
        );
    }
}

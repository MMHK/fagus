//! Phrases of Japanese text, where `word-break: auto-phrase` lets lines break: Chrome asks ICU
//! for line breaks with `lw=phrase`, whose rules keep kana and kanji together like letters of a
//! word, and whose `MlBreakEngine` finds the boundaries of phrases inside each run of them with
//! the model of `BudouX` (scores for the characters, pairs and triples around each place, which
//! add up to a break where their sum is positive).

include!("phrase_data.rs");

/// Whether ICU's engine for Chinese and Japanese takes a character: kanji, kana and the
/// prolonged sound marks (`[[:Han:][:Hiragana:][:Katakana:]ーｰﾞﾟ]`).
pub fn is_phrase_letter(character: char) -> bool {
    matches!(character,
        // Hiragana.
        '\u{3041}'..='\u{3096}' | '\u{309D}'..='\u{309F}' | '\u{1B001}'..='\u{1B11F}'
        | '\u{1B132}' | '\u{1B150}'..='\u{1B152}' | '\u{1F200}'
        // Katakana, and the prolonged sound marks and voicing marks.
        | '\u{30A1}'..='\u{30FA}' | '\u{30FC}'..='\u{30FF}' | '\u{31F0}'..='\u{31FF}'
        | '\u{32D0}'..='\u{32FE}' | '\u{3300}'..='\u{3357}' | '\u{FF66}'..='\u{FF9F}'
        | '\u{1AFF0}'..='\u{1AFFE}' | '\u{1B000}' | '\u{1B120}'..='\u{1B122}' | '\u{1B155}'
        | '\u{1B164}'..='\u{1B167}'
        // Han.
        | '\u{2E80}'..='\u{2E99}' | '\u{2E9B}'..='\u{2EF3}' | '\u{2F00}'..='\u{2FD5}' | '\u{3005}'
        | '\u{3007}' | '\u{3021}'..='\u{3029}' | '\u{3038}'..='\u{303B}' | '\u{3400}'..='\u{4DBF}'
        | '\u{4E00}'..='\u{9FFF}' | '\u{F900}'..='\u{FA6D}' | '\u{FA70}'..='\u{FAD9}'
        | '\u{16FE2}' | '\u{16FE3}' | '\u{16FF0}' | '\u{16FF1}' | '\u{20000}'..='\u{2A6DF}'
        | '\u{2A700}'..='\u{2EE5D}' | '\u{2F800}'..='\u{2FA1D}' | '\u{30000}'..='\u{323AF}')
}

/// The score of a feature in one of the model's tables, 0 for features it does not have.
fn score(table: &[(&str, i32)], feature: &str) -> i32 {
    table
        .binary_search_by(|(key, _)| key.as_bytes().cmp(feature.as_bytes()))
        .map_or(0, |index| table[index].1)
}

/// The boundaries of the phrases inside `run` (kanji and kana), as character counts from its
/// start: where the scores of the characters, pairs and triples around a place add up to more
/// than half of all scores.
fn boundaries(run: &[char]) -> Vec<usize> {
    let tables = [&UW1[..], &UW2, &UW3, &UW4, &UW5, &UW6];
    let pairs = [&BW1[..], &BW2, &BW3];
    let triples = [&TW1[..], &TW2, &TW3, &TW4];
    let at = |index: isize| {
        usize::try_from(index)
            .ok()
            .and_then(|index| run.get(index).copied())
    };
    let mut found = Vec::new();
    let mut feature = String::new();
    for index in 0..run.len().saturating_sub(1) {
        let index = isize::try_from(index).unwrap_or(isize::MAX);
        let mut sum = -(TOTAL / 2);
        for (offset, table) in (-2..).zip(tables) {
            if let Some(character) = at(index + offset) {
                feature.clear();
                feature.push(character);
                sum += score(table, &feature);
            }
        }
        for (offset, table) in (-1..).zip(pairs) {
            if let (Some(first), Some(second)) = (at(index + offset), at(index + offset + 1)) {
                feature.clear();
                feature.extend([first, second]);
                sum += score(table, &feature);
            }
        }
        for (offset, table) in (-2..).zip(triples) {
            if let (Some(first), Some(second), Some(third)) =
                (at(index + offset), at(index + offset + 1), at(index + offset + 2))
            {
                feature.clear();
                feature.extend([first, second, third]);
                sum += score(table, &feature);
            }
        }
        if sum > 0 {
            found.push(usize::try_from(index).unwrap_or_default() + 1);
        }
    }
    found
}

/// The phrase boundaries in `text` (byte offsets), as ICU's `MlBreakEngine` finds them for
/// `lw=phrase`: inside each run of kanji and kana; at its start after closing punctuation; at
/// its end before a digit, an opening bracket or quote, or a letter.
pub fn phrase_breaks(text: &str) -> Vec<usize> {
    use unicode_properties::{GeneralCategory, UnicodeGeneralCategory};
    let closing = |character: char| {
        matches!(
            character.general_category(),
            GeneralCategory::ConnectorPunctuation
                | GeneralCategory::DashPunctuation
                | GeneralCategory::ClosePunctuation
                | GeneralCategory::FinalPunctuation
                | GeneralCategory::OtherPunctuation
        )
    };
    let opening = |character: char| {
        character.is_alphabetic()
            || matches!(
                character.general_category(),
                GeneralCategory::DecimalNumber
                    | GeneralCategory::InitialPunctuation
                    | GeneralCategory::OpenPunctuation
            )
    };
    let characters: Vec<(usize, char)> = text.char_indices().collect();
    let mut breaks = Vec::new();
    let mut index = 0;
    while index < characters.len() {
        if !is_phrase_letter(characters[index].1) {
            index += 1;
            continue;
        }
        let start = index;
        while index < characters.len() && is_phrase_letter(characters[index].1) {
            index += 1;
        }
        let run: Vec<char> = characters[start..index]
            .iter()
            .map(|&(_, character)| character)
            .collect();
        if start > 0 && closing(characters[start - 1].1) {
            breaks.push(characters[start].0);
        }
        breaks.extend(boundaries(&run).into_iter().map(|at| characters[start + at].0));
        if let Some(&(end, next)) = characters.get(index)
            && opening(next)
        {
            breaks.push(end);
        }
    }
    breaks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phrases(text: &str) -> Vec<&str> {
        let mut start = 0;
        let mut out = Vec::new();
        for at in phrase_breaks(text).into_iter().chain([text.len()]) {
            out.push(&text[start..at]);
            start = at;
        }
        out
    }

    #[test]
    fn finds_japanese_phrases_as_icu_does() {
        assert_eq!(phrases("東京へ行きましょう。"), ["東京へ", "行きましょう。"]);
        assert_eq!(phrases("楽しいドライブ。"), ["楽しい", "ドライブ。"]);
        assert_eq!(phrases("一生懸命働きます"), ["一生懸命", "働きます"]);
        // A single word stays whole; at the end of a run before a letter it breaks.
        assert_eq!(phrases("ドライブ"), ["ドライブ"]);
        assert_eq!(phrases("東京Tokyo"), ["東京", "Tokyo"]);
        // After closing punctuation a run starts a phrase.
        assert_eq!(phrases("x、東京"), ["x、", "東京"]);
    }

    #[test]
    fn has_sorted_tables() {
        for table in [
            &UW1[..],
            &UW2,
            &UW3,
            &UW4,
            &UW5,
            &UW6,
            &BW1,
            &BW2,
            &BW3,
            &TW1,
            &TW2,
            &TW3,
            &TW4,
        ] {
            assert!(
                table
                    .windows(2)
                    .all(|pair| pair[0].0.as_bytes() < pair[1].0.as_bytes())
            );
        }
    }
}

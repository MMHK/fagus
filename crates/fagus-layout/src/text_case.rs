//! Case mapping for `text-transform` by the language of the text (CSS Text 3, §2.1.1), as
//! Chrome maps case with ICU: Turkish and Azeri dotted and dotless i, and Greek capitals without
//! accents.

/// The languages whose case mapping differs from the default one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CaseLanguage {
    Default,
    /// Turkish and Azeri: `i` and `I` pair with the dotted `İ` and the dotless `ı`.
    Turkic,
    /// Greek: capitals drop their accents (ICU's `GreekUpper`).
    Greek,
}

impl CaseLanguage {
    /// The case language of a BCP 47 language tag (`lang`).
    pub(crate) fn of(lang: Option<&str>) -> Self {
        let primary = lang
            .and_then(|lang| lang.split(['-', '_']).next())
            .unwrap_or_default()
            .to_ascii_lowercase();
        match primary.as_str() {
            "tr" | "az" => Self::Turkic,
            "el" => Self::Greek,
            _ => Self::Default,
        }
    }
}

/// `text` in capitals, in the case mapping of `language`.
pub(crate) fn uppercase(text: &str, language: CaseLanguage) -> String {
    match language {
        CaseLanguage::Default => text.to_uppercase(),
        CaseLanguage::Turkic => text
            .chars()
            .map(|character| match character {
                'i' => "\u{130}".to_owned(),
                other => other.to_uppercase().collect(),
            })
            .collect(),
        CaseLanguage::Greek => greek_uppercase(text),
    }
}

/// `text` in small letters, in the case mapping of `language`.
pub(crate) fn lowercase(text: &str, language: CaseLanguage) -> String {
    if language != CaseLanguage::Turkic {
        return text.to_lowercase();
    }
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '\u{130}' => result.push('i'),
            // `I` with a combining dot above is a dotted `i`.
            'I' if chars.peek() == Some(&'\u{307}') => {
                chars.next();
                result.push('i');
            }
            'I' => result.push('\u{131}'),
            other => result.extend(other.to_lowercase()),
        }
    }
    result
}

/// `text` with the first letter of each word in titlecase, as Chrome capitalizes (Blink's
/// `Capitalize`): the simple titlecase mapping of the character that starts each word, in any
/// language, where words end at spaces and punctuation but not at an apostrophe between letters,
/// and characters beyond the Basic Multilingual Plane stay as they are (Blink maps UTF-16 code
/// units). `before` is the text before it in the paragraph, whose last word it may go on.
pub(crate) fn capitalize(text: &str, before: Before) -> String {
    let mut result = String::with_capacity(text.len());
    let mut before = before;
    for character in text.chars() {
        let starts = is_word(character)
            && !before.last.is_some_and(is_word)
            && !(before.last.is_some_and(joins_letters)
                && before.second.is_some_and(char::is_alphabetic)
                && character.is_alphabetic());
        if starts {
            result.push(simple_titlecase(character));
        } else {
            result.push(character);
        }
        before.push(character);
    }
    result
}

/// The last two characters before a text that decide where its first word starts, without
/// marks and other characters that do not end a word.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Before {
    last: Option<char>,
    second: Option<char>,
}

impl Before {
    /// Takes in a character that follows.
    pub(crate) fn push(&mut self, character: char) {
        if !is_extend(character) {
            self.second = self.last;
            self.last = Some(character);
        }
    }

    /// Takes in a text that follows: its last two characters that end a word decide, so it is
    /// read from its end.
    pub(crate) fn push_str(&mut self, text: &str) {
        let mut ends = text.chars().rev().filter(|&character| !is_extend(character));
        match (ends.next(), ends.next()) {
            (Some(last), Some(second)) => (self.last, self.second) = (Some(last), Some(second)),
            (Some(last), None) => (self.last, self.second) = (Some(last), self.last),
            _ => {}
        }
    }
}

/// Whether a character is part of a word (letters, digits, connector punctuation), where a word
/// may start; Chinese and Japanese characters are words of their own.
fn is_word(character: char) -> bool {
    (character.is_alphanumeric() && !is_ideographic(character))
        || matches!(
            character,
            '_' | '\u{203F}' | '\u{2040}' | '\u{2054}' | '\u{FE33}' | '\u{FE34}' | '\u{FE4D}'
                ..='\u{FE4F}' | '\u{FF3F}'
        )
}

/// Whether a character is a Chinese or Japanese character, which Chrome's word breaker keeps
/// apart from letters.
fn is_ideographic(character: char) -> bool {
    matches!(
        character,
        '\u{3040}'..='\u{30FF}' | '\u{31F0}'..='\u{31FF}' | '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}'
            | '\u{F900}'..='\u{FAFF}' | '\u{FF66}'..='\u{FF9F}' | '\u{20000}'..='\u{3FFFF}'
    )
}

/// Whether a character joins the letters on both sides of it into one word (an apostrophe, a
/// middle dot), as `don't`.
fn joins_letters(character: char) -> bool {
    matches!(character, '\'' | '\u{2019}' | '\u{B7}' | '\u{2027}')
}

/// Whether a character goes on the word before it: a combining mark, a soft hyphen or a joiner.
fn is_extend(character: char) -> bool {
    matches!(character, '\u{AD}' | '\u{200C}' | '\u{200D}') || crate::inline::is_mark(character)
}

/// The simple titlecase mapping of a character (one character, as ICU's `u_totitle`): the
/// titlecase forms of the digraphs and of Greek letters with a subscript iota, else the capital
/// where it is one character. Characters beyond the Basic Multilingual Plane stay.
fn simple_titlecase(character: char) -> char {
    let code = u32::from(character);
    let mapped = match code {
        0x1C4..=0x1C6 => 0x1C5,
        0x1C7..=0x1C9 => 0x1C8,
        0x1CA..=0x1CC => 0x1CB,
        0x1F1..=0x1F3 => 0x1F2,
        0x1F80..=0x1FAF => code | 8,
        0x1FB3 | 0x1FBC => 0x1FBC,
        0x1FC3 | 0x1FCC => 0x1FCC,
        0x1FF3 | 0x1FFC => 0x1FFC,
        0x1_0000.. => code,
        _ => {
            let mut upper = character.to_uppercase();
            return match (upper.next(), upper.next()) {
                (Some(upper), None) => upper,
                _ => character,
            };
        }
    };
    char::from_u32(mapped).unwrap_or(character)
}

/// What a Greek letter is made of: its capital without marks, whether it is a vowel, and
/// whether it carries an accent (tonos) or a dialytika.
#[derive(Clone, Copy)]
struct GreekLetter {
    capital: char,
    vowel: bool,
    accent: bool,
    dialytika: bool,
}

/// The parts of a monotonic Greek letter, `None` for other characters.
fn greek_letter(character: char) -> Option<GreekLetter> {
    let (capital, accent, dialytika) = match character {
        '\u{386}' | '\u{3AC}' => ('Α', true, false),
        '\u{388}' | '\u{3AD}' => ('Ε', true, false),
        '\u{389}' | '\u{3AE}' => ('Η', true, false),
        '\u{38A}' | '\u{3AF}' => ('Ι', true, false),
        '\u{38C}' | '\u{3CC}' => ('Ο', true, false),
        '\u{38E}' | '\u{3CD}' => ('Υ', true, false),
        '\u{38F}' | '\u{3CE}' => ('Ω', true, false),
        '\u{390}' => ('Ι', true, true),
        '\u{3B0}' => ('Υ', true, true),
        '\u{3AA}' | '\u{3CA}' => ('Ι', false, true),
        '\u{3AB}' | '\u{3CB}' => ('Υ', false, true),
        '\u{391}'..='\u{3A9}' | '\u{3B1}'..='\u{3C9}' => {
            let capital = character.to_uppercase().next().unwrap_or(character);
            (capital, false, false)
        }
        _ => return None,
    };
    Some(GreekLetter {
        capital,
        vowel: matches!(capital, 'Α' | 'Ε' | 'Η' | 'Ι' | 'Ο' | 'Υ' | 'Ω'),
        accent,
        dialytika,
    })
}

/// Greek text in capitals as ICU's `GreekUpper` sets them (Unicode TR #21 and ICU's notes on
/// Greek uppercasing): accents (tonos, varia, perispomeni) and breathings go, a dialytika stays,
/// an `ι` or `υ` after a vowel that lost its accent gains one (`τσάι` → `ΤΣΑΪ`), and the
/// disjunctive `ή` standing alone keeps its tonos. Polytonic letters are left to the default
/// mapping.
fn greek_uppercase(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut result = String::with_capacity(text.len());
    // Whether the previous letter was a vowel that lost an accent and had no dialytika.
    let mut after_accented_vowel = false;
    // Whether the previous character is a letter (for the disjunctive `ή`).
    let mut after_letter = false;
    for (index, &character) in chars.iter().enumerate() {
        // Combining marks after Greek letters: accents and breathings go, a dialytika stays.
        if matches!(
            character,
            '\u{300}' | '\u{301}' | '\u{342}' | '\u{313}' | '\u{314}' | '\u{343}'
        ) && after_letter
        {
            continue;
        }
        if character == '\u{344}' && after_letter {
            result.push('\u{308}');
            continue;
        }
        let Some(letter) = greek_letter(character) else {
            result.extend(character.to_uppercase());
            after_accented_vowel = false;
            after_letter = character.is_alphabetic();
            continue;
        };
        let accent = letter.accent
            || chars
                .get(index + 1)
                .is_some_and(|next| matches!(next, '\u{301}' | '\u{300}' | '\u{342}'));
        let next_is_letter = chars
            .iter()
            .skip(index + 1)
            .find(|next| !is_greek_mark(**next))
            .is_some_and(|next| next.is_alphabetic());
        if letter.capital == 'Η' && accent && !letter.dialytika && !after_letter && !next_is_letter {
            result.push('\u{389}');
        } else if letter.dialytika || (after_accented_vowel && matches!(letter.capital, 'Ι' | 'Υ')) {
            result.push(if letter.capital == 'Ι' {
                '\u{3AA}'
            } else {
                '\u{3AB}'
            });
        } else {
            result.push(letter.capital);
        }
        after_accented_vowel = letter.vowel && accent && !letter.dialytika;
        after_letter = true;
    }
    result
}

/// Whether a character is a combining mark that Greek letters carry.
fn is_greek_mark(character: char) -> bool {
    matches!(character, '\u{300}'..='\u{345}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_in_a_text_as_its_characters_one_by_one() {
        for start in ["", "a", "ab", "x\u{301}"] {
            for text in [
                "",
                "a",
                "ab",
                "don't",
                "e\u{301}",
                "\u{301}",
                "\u{301}\u{302}",
                "a\u{AD}",
                "x y z\u{200D}",
            ] {
                let mut whole = Before::default();
                whole.push_str(start);
                let mut each = whole;
                whole.push_str(text);
                text.chars().for_each(|character| each.push(character));
                assert_eq!(whole, each, "{start:?} then {text:?}");
            }
        }
    }

    #[test]
    fn greek_capitals_drop_accents() {
        let upper = |text| uppercase(text, CaseLanguage::of(Some("el")));
        assert_eq!(upper("Προϊόν"), "ΠΡΟΪΟΝ");
        assert_eq!(upper("Συσκευασία"), "ΣΥΣΚΕΥΑΣΙΑ");
        assert_eq!(upper("Όροι πώλησης"), "ΟΡΟΙ ΠΩΛΗΣΗΣ");
        // An iota after a vowel that lost its accent is pronounced apart: it gains a dialytika.
        assert_eq!(upper("τσάι"), "ΤΣΑΪ");
        assert_eq!(upper("ευφυΐα"), "ΕΥΦΥΪΑ");
        // The disjunctive "or" keeps its tonos.
        assert_eq!(upper("ή"), "Ή");
        assert_eq!(upper("ήταν"), "ΗΤΑΝ");
        // Combining accents go too.
        assert_eq!(upper("α\u{301}λφα"), "ΑΛΦΑ");
        assert_eq!(upper("Price €"), "PRICE €");
        // Other languages keep the accents.
        assert_eq!(uppercase("Προϊόν", CaseLanguage::of(Some("en"))), "ΠΡΟΪΌΝ");
    }

    #[test]
    fn turkish_has_dotted_and_dotless_i() {
        let turkish = CaseLanguage::of(Some("tr-TR"));
        assert_eq!(uppercase("istanbul", turkish), "İSTANBUL");
        assert_eq!(lowercase("ISPARTA İZMİR", turkish), "ısparta izmir");
        assert_eq!(uppercase("istanbul", CaseLanguage::of(None)), "ISTANBUL");
    }

    #[test]
    fn capitalizes_words_as_chrome() {
        let capitalize = |text| capitalize(text, Before::default());
        // Words end at punctuation, but not at an apostrophe between letters, nor at an
        // underscore or between digits and letters.
        assert_eq!(
            capitalize("(hello) don't x-ray foo.bar foo_bar 3am a1b2 ab:cd l’homme ’tis"),
            "(Hello) Don't X-Ray Foo.Bar Foo_bar 3am A1b2 Ab:Cd L’homme ’Tis"
        );
        // The simple titlecase mapping, in any language: digraphs take their titlecase form,
        // letters without a one-letter capital stay.
        assert_eq!(capitalize("ǆungla ǉ ßeta ﬀoo ᾳbc ᾀx"), "ǅungla ǈ ßeta ﬀoo ᾼbc ᾈx");
        assert_eq!(capitalize("istanbul ılık άλφα"), "Istanbul Ilık Άλφα");
        // Letters beyond the Basic Multilingual Plane stay, circled letters are letters.
        assert_eq!(capitalize("\u{10428}\u{10428} ⓑⓑ"), "\u{10428}\u{10428} Ⓑⓑ");
        // Chinese and Japanese characters are words of their own; marks and soft hyphens go on
        // the word.
        assert_eq!(
            capitalize("東京abc e\u{301}lan ab\u{AD}cd"),
            "東京Abc E\u{301}lan Ab\u{AD}cd"
        );
        // A text goes on the word before it.
        let mut before = Before::default();
        before.push_str("foo");
        assert_eq!(super::capitalize("bar baz", before), "bar Baz");
    }
}

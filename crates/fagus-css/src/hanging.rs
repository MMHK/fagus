//! `hanging-punctuation` (CSS Text 3, §8.2.1), which lightningcss does not parse:
//! `none | [ first || [ force-end | allow-end ] || last ]`.

use lightningcss::properties::custom::{Token, TokenList, TokenOrValue};

/// Which punctuation at the end of a line hangs (`force-end`, `allow-end`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HangingEnd {
    /// None but the closing bracket or quote of `last`.
    #[default]
    None,
    /// A stop or comma at the end of a line hangs.
    Force,
    /// A stop or comma at the end of a line hangs where it does not fit otherwise.
    Allow,
}

/// The computed value of `hanging-punctuation`: `none` where nothing is set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HangingPunctuation {
    /// An opening bracket, quote or ideographic space at the start of the first formatted line
    /// hangs.
    pub first: bool,
    /// A stop or comma at the end of a line hangs, or may.
    pub end: HangingEnd,
    /// A closing bracket or quote at the end of the last formatted line hangs.
    pub last: bool,
}

impl HangingPunctuation {
    /// Whether no punctuation hangs (`none`).
    pub fn is_none(self) -> bool {
        self == Self::default()
    }

    /// Parses the keywords of a value, in any case: `None` where they are not a valid value.
    pub fn parse<'w>(words: impl IntoIterator<Item = &'w str>) -> Option<Self> {
        let words: Vec<String> = words.into_iter().map(str::to_ascii_lowercase).collect();
        if let [word] = words.as_slice()
            && word == "none"
        {
            return Some(Self::default());
        }
        let mut value = Self::default();
        let mut end = false;
        for word in &words {
            match word.as_str() {
                "first" if !value.first => value.first = true,
                "last" if !value.last => value.last = true,
                "force-end" if !end => (end, value.end) = (true, HangingEnd::Force),
                "allow-end" if !end => (end, value.end) = (true, HangingEnd::Allow),
                _ => return None,
            }
        }
        (!words.is_empty()).then_some(value)
    }

    /// Parses a value of identifiers and white space.
    pub fn from_tokens(tokens: &TokenList<'_>) -> Option<Self> {
        let mut words = Vec::new();
        for token in &tokens.0 {
            match token {
                TokenOrValue::Token(Token::WhiteSpace(_)) => {}
                TokenOrValue::Token(Token::Ident(ident)) => words.push(ident.as_ref()),
                _ => return None,
            }
        }
        Self::parse(words)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_grammar_of_css_text_3() {
        let parse = |value: &str| HangingPunctuation::parse(value.split_ascii_whitespace());
        assert_eq!(parse("none"), Some(HangingPunctuation::default()));
        assert_eq!(
            parse("last FIRST allow-end"),
            Some(HangingPunctuation {
                first: true,
                end: HangingEnd::Allow,
                last: true,
            })
        );
        assert_eq!(
            parse("force-end"),
            Some(HangingPunctuation {
                end: HangingEnd::Force,
                ..HangingPunctuation::default()
            })
        );
        for invalid in [
            "",
            "none first",
            "first first",
            "force-end allow-end",
            "end",
            "first,",
        ] {
            assert_eq!(parse(invalid), None, "{invalid}");
        }
    }
}

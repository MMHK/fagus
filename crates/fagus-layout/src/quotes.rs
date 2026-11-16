//! The quotes of `open-quote` and `close-quote` (CSS Generated Content 3, §4.3): those of
//! `quotes`, or for `auto` those of the language, as Chrome takes them from CLDR.

/// The quotes of languages, by their tag or its first part, as Chrome 141 prints them: the
/// outer pair, then the inner one. Others take “ ” and ‘ ’.
const LANGUAGES: &[(&str, [&str; 4])] = &[
    ("de", ["„", "“", "‚", "‘"]),
    ("cs", ["„", "“", "‚", "‘"]),
    ("sk", ["„", "“", "‚", "‘"]),
    ("sl", ["„", "“", "‚", "‘"]),
    ("hr", ["„", "“", "‚", "‘"]),
    ("et", ["„", "“", "‚", "‘"]),
    ("fr", ["«", "»", "«", "»"]),
    ("fr-ch", ["«", "»", "‹", "›"]),
    ("fr-ca", ["«", "»", "”", "“"]),
    ("it", ["«", "»", "“", "”"]),
    ("ca", ["«", "»", "“", "”"]),
    ("el", ["«", "»", "“", "”"]),
    ("pt-pt", ["«", "»", "“", "”"]),
    ("nl", ["‘", "’", "‘", "’"]),
    ("pl", ["„", "”", "«", "»"]),
    ("ro", ["„", "”", "«", "»"]),
    ("hu", ["„", "”", "»", "«"]),
    ("sr", ["„", "”", "’", "’"]),
    ("bg", ["„", "“", "„", "“"]),
    ("lt", ["„", "“", "„", "“"]),
    ("ru", ["«", "»", "„", "“"]),
    ("uk", ["«", "»", "„", "“"]),
    ("sv", ["”", "”", "’", "’"]),
    ("fi", ["”", "”", "’", "’"]),
    ("he", ["”", "”", "’", "’"]),
    ("nb", ["«", "»", "‘", "’"]),
    ("nn", ["«", "»", "‘", "’"]),
    ("no", ["«", "»", "‘", "’"]),
    ("ar", ["”", "“", "’", "‘"]),
    ("ur", ["”", "“", "’", "‘"]),
    ("fa", ["«", "»", "‹", "›"]),
    ("am", ["«", "»", "‹", "›"]),
    ("ja", ["「", "」", "『", "』"]),
];

/// The quotes of `open-quote` and `close-quote` for the language `lang`.
fn language(lang: Option<&str>) -> [&'static str; 4] {
    let lang = lang.unwrap_or_default().to_ascii_lowercase().replace('_', "-");
    // The whole tag, then shorter ones: `de-CH-1996`, `de-CH`, `de`.
    let mut tag = lang.as_str();
    loop {
        if let Some((_, quotes)) = LANGUAGES.iter().find(|(name, _)| *name == tag) {
            return *quotes;
        }
        match tag.rfind('-') {
            Some(at) => tag = &tag[..at],
            None => return ["“", "”", "‘", "’"],
        }
    }
}

/// The text of an opening (`open`) or closing quote at `depth` (from 0, the outermost): of
/// `quotes`, or of the language `lang` for `auto`; quotes deeper than the pairs given take the
/// last.
pub(crate) fn quote(
    quotes: Option<&[(String, String)]>,
    lang: Option<&str>,
    depth: usize,
    open: bool,
) -> String {
    if let Some(pairs) = quotes {
        return pairs
            .get(depth.min(pairs.len().saturating_sub(1)))
            .map(|(opening, closing)| if open { opening.clone() } else { closing.clone() })
            .unwrap_or_default();
    }
    let [outer_open, outer_close, inner_open, inner_close] = language(lang);
    let (opening, closing) = if depth == 0 {
        (outer_open, outer_close)
    } else {
        (inner_open, inner_close)
    };
    (if open { opening } else { closing }).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_as_the_language_or_quotes_says() {
        let pair = |lang: &str, depth| {
            (
                quote(None, Some(lang), depth, true),
                quote(None, Some(lang), depth, false),
            )
        };
        assert_eq!(pair("de", 0), ("„".to_owned(), "“".to_owned()));
        assert_eq!(pair("de-CH-1996", 2), ("‚".to_owned(), "‘".to_owned()));
        assert_eq!(pair("fr-CH", 1), ("‹".to_owned(), "›".to_owned()));
        assert_eq!(pair("en-GB", 0), ("“".to_owned(), "”".to_owned()));
        assert_eq!(pair("xx", 1), ("‘".to_owned(), "’".to_owned()));
        let own = [("<".to_owned(), ">".to_owned()), ("(".to_owned(), ")".to_owned())];
        assert_eq!(quote(Some(&own), Some("de"), 5, true), "(");
        assert_eq!(quote(Some(&[]), Some("de"), 0, true), "");
    }
}

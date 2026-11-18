//! Page selectors of `@page` rules (CSS Paged Media 3, §4.2), with the `:nth()` pseudo-class of
//! CSS Generated Content for Paged Media 3 (§3.1), which lightningcss does not parse.
//!
//! Before a stylesheet is parsed, [`encode_nth_pages`] writes each `@page` selector with `:nth()`
//! as a page name that carries the arguments, which lightningcss takes; [`PageSelector::of`]
//! reads them back.

use std::borrow::Cow;
use std::fmt::Write;

use cssparser::{ParseError, Parser, ParserInput, Token};
use lightningcss::rules::page::{PagePseudoClass, PageSelector as ParsedSelector};

/// Separates the page name from the arguments of each `:nth()` in the names that
/// [`encode_nth_pages`] writes: a character of the private use area, which no page name holds.
const NTH: char = '\u{E000}';

/// A page selector: a page name and pseudo-classes, all of which a page must match.
#[derive(Clone, Debug, PartialEq)]
pub struct PageSelector {
    pub name: Option<String>,
    /// `:first`, `:left`, `:right`, `:blank` (and `:last`, which lightningcss takes and no page
    /// matches).
    pub pseudo_classes: Vec<PagePseudoClass>,
    pub nth: Vec<PageNth>,
}

/// An `:nth(An+B)` or `:nth(An+B of <custom-ident>)` pseudo-class.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageNth {
    pub a: i32,
    pub b: i32,
    /// The name of the page groups whose pages it counts; `None` for the pages of the document.
    pub of: Option<String>,
}

impl PageNth {
    /// Whether the page with the 1-based `index` matches: `An+B = index` for some `n >= 0`.
    pub fn matches(&self, index: usize) -> bool {
        let Ok(index) = i64::try_from(index) else {
            return false;
        };
        let (a, offset) = (i64::from(self.a), index - i64::from(self.b));
        if a == 0 {
            offset == 0
        } else {
            offset % a == 0 && offset / a >= 0
        }
    }
}

impl PageSelector {
    /// The selector that lightningcss parsed, with the `:nth()` pseudo-classes that
    /// [`encode_nth_pages`] put into its name.
    pub fn of(parsed: &ParsedSelector<'_>) -> Self {
        let mut name = None;
        let mut nth = Vec::new();
        if let Some(parsed_name) = &parsed.name {
            let mut parts = parsed_name.split(NTH);
            name = parts.next().filter(|name| !name.is_empty()).map(str::to_owned);
            nth.extend(parts.filter_map(decode_nth));
        }
        Self {
            name,
            pseudo_classes: parsed.pseudo_classes.clone(),
            nth,
        }
    }

    /// Whether the selector names pages: a page name, or the page groups of one.
    pub fn names_pages(&self) -> bool {
        self.name.is_some() || self.nth.iter().any(|nth| nth.of.is_some())
    }
}

/// The arguments of one `:nth()` as [`encode_nth_pages`] writes them: `A,B` or `A,B,name`.
fn decode_nth(encoded: &str) -> Option<PageNth> {
    let mut parts = encoded.splitn(3, ',');
    let a = parts.next()?.parse().ok()?;
    let b = parts.next()?.parse().ok()?;
    let of = parts.next().map(str::to_owned);
    Some(PageNth { a, b, of })
}

/// `css` with the selectors of `@page` rules that use `:nth()` written as page names that carry
/// its arguments (a selector `chapter:nth(2n+1 of chapter):left` becomes a page name of
/// `chapter`, `2,1` and `chapter`, then `:left`), which lightningcss parses; [`PageSelector::of`]
/// reads them back. Selectors that are not valid stay as they are, and lightningcss drops their
/// rule. The lines stay where they are, for the positions of diagnostics.
pub fn encode_nth_pages(css: &str) -> Cow<'_, str> {
    let contains = |needle: &str| {
        css.as_bytes()
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
    };
    if !contains("@page") || !contains("nth(") {
        return Cow::Borrowed(css);
    }
    let mut input = ParserInput::new(css);
    let mut parser = Parser::new(&mut input);
    let mut edits = Vec::new();
    scan(&mut parser, &mut edits);
    if edits.is_empty() {
        return Cow::Borrowed(css);
    }
    let mut out = String::with_capacity(css.len() + 64);
    let mut at = 0;
    for (start, end, text) in edits {
        out.push_str(&css[at..start]);
        out.push_str(&text);
        // As many line breaks as the selectors spanned, so that the lines after keep their numbers.
        out.extend(std::iter::repeat_n('\n', css[start..end].matches('\n').count()));
        at = end;
    }
    out.push_str(&css[at..]);
    Cow::Owned(out)
}

/// One part of the prelude of an `@page` rule.
enum Part {
    Space,
    Comma,
    Colon,
    Ident(String),
    /// `nth(…)` with its arguments, `None` if they are not valid; other functions are `None`.
    Function(Option<PageNth>),
    Other,
}

/// Collects the edits of the `@page` preludes with `:nth()` in the rules `parser` reads, and in
/// the blocks inside them: the byte range of each prelude and the text that replaces it.
fn scan(parser: &mut Parser<'_, '_>, edits: &mut Vec<(usize, usize, String)>) {
    loop {
        let token = match parser.next_including_whitespace_and_comments() {
            Ok(token) => token.clone(),
            Err(_) => return,
        };
        match token {
            Token::AtKeyword(name) if name.eq_ignore_ascii_case("page") => {
                if let Some(edit) = page_prelude(parser) {
                    edits.push(edit);
                }
            }
            Token::CurlyBracketBlock => {
                let _ = parser.parse_nested_block(|inner| {
                    scan(inner, edits);
                    Ok::<(), ParseError<'_, ()>>(())
                });
            }
            _ => {}
        }
    }
}

/// The edit of the prelude of an `@page` rule that `parser` is at, if it holds valid selectors
/// with `:nth()`. Leaves `parser` before the block of the rule.
fn page_prelude(parser: &mut Parser<'_, '_>) -> Option<(usize, usize, String)> {
    let start = parser.position().byte_index();
    let mut parts = Vec::new();
    let end = loop {
        let state = parser.state();
        let token = match parser.next_including_whitespace_and_comments() {
            Ok(Token::CurlyBracketBlock | Token::Semicolon) | Err(_) => {
                parser.reset(&state);
                break state.position().byte_index();
            }
            Ok(token) => token.clone(),
        };
        parts.push(match token {
            Token::WhiteSpace(_) | Token::Comment(_) => Part::Space,
            Token::Comma => Part::Comma,
            Token::Colon => Part::Colon,
            Token::Ident(name) => Part::Ident(name.to_string()),
            Token::Function(name) => Part::Function(
                parser
                    .parse_nested_block(nth_arguments)
                    .ok()
                    .filter(|_| name.eq_ignore_ascii_case("nth")),
            ),
            _ => Part::Other,
        });
    };
    let selectors = selectors(&parts)?;
    if selectors.iter().all(|selector| selector.nth.is_empty()) {
        return None;
    }
    let text: Vec<String> = selectors.iter().map(encoded).collect();
    Some((start, end, format!(" {} ", text.join(", "))))
}

/// The arguments of `:nth()`: `An+B`, optionally `of` and a page name.
fn nth_arguments<'i>(input: &mut Parser<'i, '_>) -> Result<PageNth, ParseError<'i, ()>> {
    let (a, b) = cssparser::parse_nth(input)?;
    let of = if input.try_parse(|input| input.expect_ident_matching("of")).is_ok() {
        let name = input.expect_ident()?.to_string();
        // A <custom-ident> excludes the CSS-wide keywords and `default`.
        if ["initial", "inherit", "unset", "revert", "revert-layer", "default"]
            .iter()
            .any(|keyword| name.eq_ignore_ascii_case(keyword))
        {
            return Err(input.new_custom_error(()));
        }
        Some(name)
    } else {
        None
    };
    input.expect_exhausted()?;
    Ok(PageNth { a, b, of })
}

/// The selectors of a prelude, `None` if one is not valid: a list separated by commas of a page
/// name and pseudo-classes, with no white space inside a selector.
fn selectors(parts: &[Part]) -> Option<Vec<PageSelector>> {
    parts
        .split(|part| matches!(part, Part::Comma))
        .map(|parts| {
            let first = parts.iter().position(|part| !matches!(part, Part::Space))?;
            let last = parts.iter().rposition(|part| !matches!(part, Part::Space))?;
            let mut parts = parts[first..=last].iter();
            let mut selector = PageSelector {
                name: None,
                pseudo_classes: Vec::new(),
                nth: Vec::new(),
            };
            let mut next = parts.next();
            if let Some(Part::Ident(name)) = next {
                selector.name = Some(name.clone());
                next = parts.next();
            }
            while let Some(part) = next {
                let Part::Colon = part else {
                    return None;
                };
                match parts.next()? {
                    Part::Ident(pseudo) => {
                        let pseudo = match pseudo.to_ascii_lowercase().as_str() {
                            "first" => PagePseudoClass::First,
                            "left" => PagePseudoClass::Left,
                            "right" => PagePseudoClass::Right,
                            "blank" => PagePseudoClass::Blank,
                            "last" => PagePseudoClass::Last,
                            _ => return None,
                        };
                        selector.pseudo_classes.push(pseudo);
                    }
                    Part::Function(Some(nth)) => selector.nth.push(nth.clone()),
                    _ => return None,
                }
                next = parts.next();
            }
            Some(selector)
        })
        .collect()
}

/// A selector written as lightningcss takes it, with its `:nth()` pseudo-classes in its name.
fn encoded(selector: &PageSelector) -> String {
    let mut out = String::new();
    if selector.name.is_some() || !selector.nth.is_empty() {
        let mut name = selector.name.clone().unwrap_or_default();
        for nth in &selector.nth {
            name.push(NTH);
            let _ = write!(name, "{},{}", nth.a, nth.b);
            if let Some(of) = &nth.of {
                name.push(',');
                name.push_str(of);
            }
        }
        // (Writing to a string does not fail.)
        let _ = cssparser::serialize_identifier(&name, &mut out);
    }
    for pseudo in &selector.pseudo_classes {
        out.push_str(match pseudo {
            PagePseudoClass::First => ":first",
            PagePseudoClass::Left => ":left",
            PagePseudoClass::Right => ":right",
            PagePseudoClass::Blank => ":blank",
            PagePseudoClass::Last => ":last",
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use lightningcss::rules::CssRule;
    use lightningcss::stylesheet::{ParserOptions, StyleSheet};

    use super::*;

    /// The selectors of the `@page` rules of `css` after [`encode_nth_pages`].
    fn page_selectors(css: &str) -> Vec<Vec<PageSelector>> {
        let encoded = encode_nth_pages(css);
        let sheet = StyleSheet::parse(&encoded, ParserOptions::default()).expect("parses");
        let mut found = Vec::new();
        let mut rules: Vec<&CssRule<'_>> = sheet.rules.0.iter().collect();
        while let Some(rule) = rules.pop() {
            match rule {
                CssRule::Page(page) => found.push(page.selectors.iter().map(PageSelector::of).collect()),
                CssRule::Media(media) => rules.extend(&media.rules.0),
                _ => {}
            }
        }
        found.reverse();
        found
    }

    fn nth(a: i32, b: i32, of: Option<&str>) -> PageNth {
        PageNth {
            a,
            b,
            of: of.map(str::to_owned),
        }
    }

    #[test]
    fn reads_nth_page_selectors() {
        let found = page_selectors(
            "@page :nth(1) { margin: 1cm }
             @page chapter:NTH( 2n + 1 of chapter ):left, :first { margin: 2cm }
             @media print { @page :nth(odd of Sec\\ tion) { margin: 3cm } }
             @page wide { margin: 4cm }",
        );
        assert_eq!(found.len(), 4, "{found:?}");
        assert_eq!(
            found[0],
            [PageSelector {
                name: None,
                pseudo_classes: Vec::new(),
                nth: vec![nth(0, 1, None)],
            }]
        );
        assert_eq!(
            found[1],
            [
                PageSelector {
                    name: Some("chapter".to_owned()),
                    pseudo_classes: vec![PagePseudoClass::Left],
                    nth: vec![nth(2, 1, Some("chapter"))],
                },
                PageSelector {
                    name: None,
                    pseudo_classes: vec![PagePseudoClass::First],
                    nth: Vec::new(),
                }
            ]
        );
        assert_eq!(found[2][0].nth, [nth(2, 1, Some("Sec tion"))]);
        assert_eq!(found[3][0].name.as_deref(), Some("wide"));
    }

    #[test]
    fn leaves_invalid_nth_selectors_to_the_parser() {
        for css in [
            "@page :nth(x) { margin: 1cm }",
            "@page :nth(1 of) { margin: 1cm }",
            "@page :nth(1 of a b) { margin: 1cm }",
            "@page :nth(1 of default) { margin: 1cm }",
            "@page :nth(1) :left { margin: 1cm }",
            "@page :nth(1):other { margin: 1cm }",
            "@page :nth(1), { margin: 1cm }",
        ] {
            assert_eq!(encode_nth_pages(css), css, "{css}");
        }
    }

    #[test]
    fn keeps_the_lines_of_the_stylesheet() {
        let css = "@page :nth(1),\n  :nth(2 of a) {\n  margin: 1cm }\np { color: red }";
        let encoded = encode_nth_pages(css);
        assert_ne!(encoded, css);
        assert_eq!(encoded.lines().count(), css.lines().count());
        assert!(
            encoded.ends_with("\n  margin: 1cm }\np { color: red }"),
            "{encoded}"
        );
    }

    #[test]
    fn matches_an_plus_b() {
        let indexes = |nth: PageNth| (1..=9).filter(|&index| nth.matches(index)).collect::<Vec<_>>();
        assert_eq!(indexes(nth(0, 3, None)), [3]);
        assert_eq!(indexes(nth(2, 0, None)), [2, 4, 6, 8]);
        assert_eq!(indexes(nth(2, 1, None)), [1, 3, 5, 7, 9]);
        assert_eq!(indexes(nth(3, -1, None)), [2, 5, 8]);
        assert_eq!(indexes(nth(-1, 3, None)), [1, 2, 3]);
        assert_eq!(indexes(nth(0, 0, None)), Vec::<usize>::new());
    }
}

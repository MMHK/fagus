//! Values of CSS Generated Content for Paged Media that lightningcss does not parse.

use std::borrow::Cow;

use lightningcss::properties::custom::{Token, TokenList, TokenOrValue};
use lightningcss::rules::page::PageMarginRule;

/// The custom property that [`encode_footnote_areas`] puts first in the declarations of each
/// `@footnote` rule: its name holds a character of the private use area, which no property of a
/// stylesheet has.
const FOOTNOTE_AREA: &str = "--\u{E001}";

/// Writes each `@footnote` rule inside an `@page` rule (CSS Generated Content for Paged Media 3,
/// §2.2) as a margin rule that lightningcss takes, `@bottom-center`, with a declaration that
/// tells it apart ([`is_footnote_area`]): lightningcss drops at-rules in `@page` that are not
/// margin rules. The lines and the rest of the text keep their places.
pub fn encode_footnote_areas(css: &str) -> Cow<'_, str> {
    let found = css
        .as_bytes()
        .windows(9)
        .any(|window| window.eq_ignore_ascii_case(b"@footnote"));
    if !found {
        return Cow::Borrowed(css);
    }
    let mut input = cssparser::ParserInput::new(css);
    let mut parser = cssparser::Parser::new(&mut input);
    // The byte ranges of the `@footnote` keywords, and where their blocks start.
    let mut edits = Vec::new();
    scan_footnotes(&mut parser, false, &mut edits);
    if edits.is_empty() {
        return Cow::Borrowed(css);
    }
    let mut out = String::with_capacity(css.len() + 32 * edits.len());
    let mut at = 0;
    for (start, end, block) in edits {
        out.push_str(&css[at..start]);
        out.push_str("@bottom-center");
        out.push_str(&css[end..block]);
        out.push_str(FOOTNOTE_AREA);
        out.push_str(":1;");
        at = block;
    }
    out.push_str(&css[at..]);
    Cow::Owned(out)
}

/// Collects the edits of [`encode_footnote_areas`] in the rules `parser` reads: the `@footnote`
/// rules directly in an `@page` rule (`in_page`), and those of the blocks inside.
fn scan_footnotes(
    parser: &mut cssparser::Parser<'_, '_>,
    in_page: bool,
    edits: &mut Vec<(usize, usize, usize)>,
) {
    use cssparser::Token;
    let mut page = false;
    let mut footnote: Option<(usize, usize)> = None;
    loop {
        let start = parser.position().byte_index();
        let token = match parser.next_including_whitespace_and_comments() {
            Ok(token) => token.clone(),
            Err(_) => return,
        };
        match token {
            Token::AtKeyword(name) if name.eq_ignore_ascii_case("page") => {
                page = true;
                footnote = None;
            }
            Token::AtKeyword(name) if in_page && name.eq_ignore_ascii_case("footnote") => {
                footnote = Some((start, parser.position().byte_index()));
            }
            Token::CurlyBracketBlock => {
                let block = parser.position().byte_index();
                if let Some((start, end)) = footnote.take() {
                    edits.push((start, end, block));
                }
                let inner_page = std::mem::take(&mut page);
                let _ = parser.parse_nested_block(|inner| {
                    scan_footnotes(inner, inner_page, edits);
                    Ok::<(), cssparser::ParseError<'_, ()>>(())
                });
            }
            Token::Semicolon => {
                page = false;
                footnote = None;
            }
            Token::WhiteSpace(_) | Token::Comment(_) => {}
            // Only white space may come between `@footnote` and its block.
            _ => footnote = None,
        }
    }
}

/// Whether a margin rule is an `@footnote` rule that [`encode_footnote_areas`] wrote.
pub fn is_footnote_area(rule: &PageMarginRule<'_>) -> bool {
    use lightningcss::properties::Property;
    use lightningcss::properties::custom::CustomPropertyName;
    rule.declarations.declarations.iter().any(|property| {
        matches!(property, Property::Custom(custom)
            if matches!(&custom.name, CustomPropertyName::Custom(name) if name.0.as_ref() == FOOTNOTE_AREA))
    })
}

/// The name in `position: running(name)` (CSS GCPM 3, §2.1).
pub fn running_name(tokens: &TokenList<'_>) -> Option<String> {
    let [TokenOrValue::Function(function)] = significant(tokens).as_slice() else {
        return None;
    };
    if !function.name.0.eq_ignore_ascii_case("running") {
        return None;
    }
    match significant(&function.arguments).as_slice() {
        [TokenOrValue::Token(Token::Ident(name))] => Some(name.to_string()),
        _ => None,
    }
}

/// Whether a value of `content` (its tokens other than white space) uses `element()` in a way
/// CSS Generated Content for Paged Media 3 does not allow, which makes the value invalid: with
/// other values ("it cannot be combined with other possible values for the content property",
/// §1.2.1), several times, or with arguments other than
/// `<custom-ident> [, first | start | last | first-except]?`.
pub fn misused_element(tokens: &[&TokenOrValue<'_>]) -> bool {
    let is_element = |token: &TokenOrValue<'_>| matches!(token, TokenOrValue::Function(function) if function.name.0.eq_ignore_ascii_case("element"));
    if !tokens.iter().any(|token| is_element(token)) {
        return false;
    }
    let [TokenOrValue::Function(function)] = tokens else {
        return true;
    };
    let custom_ident = |name: &str| {
        !["initial", "inherit", "unset", "revert", "revert-layer", "default"]
            .iter()
            .any(|keyword| name.eq_ignore_ascii_case(keyword))
    };
    match significant(&function.arguments).as_slice() {
        [TokenOrValue::Token(Token::Ident(name))] => !custom_ident(name),
        [
            TokenOrValue::Token(Token::Ident(name)),
            TokenOrValue::Token(Token::Comma),
            TokenOrValue::Token(Token::Ident(which)),
        ] => {
            !custom_ident(name)
                || !["first", "start", "last", "first-except"]
                    .iter()
                    .any(|keyword| which.eq_ignore_ascii_case(keyword))
        }
        _ => true,
    }
}

/// [`misused_element`] for a whole value of `content`.
pub fn content_misuses_element(tokens: &TokenList<'_>) -> bool {
    misused_element(&significant(tokens))
}

/// The line names of `subgrid` in `grid-template-columns` or `grid-template-rows`, which
/// lightningcss does not parse: one list per `[…]`, in order, with `repeat(n, …)` written out;
/// `None` if the value is not `subgrid`.
pub fn subgrid_line_names(tokens: &TokenList<'_>) -> Option<Vec<Vec<String>>> {
    subgrid_lines(tokens).map(|lines| lines.before)
}

/// The line names of a `subgrid` value (CSS Grid 2, §9.3): the lists before an automatic
/// repetition (`repeat(auto-fill, …)`), its lists, and those after it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SubgridLines {
    pub before: Vec<Vec<String>>,
    pub repeated: Option<Vec<Vec<String>>>,
    pub after: Vec<Vec<String>>,
}

/// The most lists a `repeat(n, …)` writes out.
const MAX_REPEATED_LISTS: usize = 10_000;

/// The line names of a `subgrid` value (see [`SubgridLines`]); `None` if the value is not
/// `subgrid` or not valid.
pub fn subgrid_lines(tokens: &TokenList<'_>) -> Option<SubgridLines> {
    let tokens = significant(tokens);
    let (first, rest) = tokens.split_first()?;
    match first {
        TokenOrValue::Token(Token::Ident(ident)) if ident.eq_ignore_ascii_case("subgrid") => {}
        _ => return None,
    }
    let mut lines = SubgridLines::default();
    let mut current: Option<Vec<String>> = None;
    for token in rest {
        let target = if lines.repeated.is_some() {
            &mut lines.after
        } else {
            &mut lines.before
        };
        match (token, &mut current) {
            (TokenOrValue::Token(Token::SquareBracketBlock), None) => current = Some(Vec::new()),
            (TokenOrValue::Token(Token::Ident(name)), Some(names)) => names.push(name.to_string()),
            (TokenOrValue::Token(Token::CloseSquareBracket), Some(_)) => target.extend(current.take()),
            (TokenOrValue::Function(function), None) if function.name.0.eq_ignore_ascii_case("repeat") => {
                let arguments = significant(&function.arguments);
                let (count, lists) = match arguments.as_slice() {
                    [count, TokenOrValue::Token(Token::Comma), lists @ ..] => (*count, lists),
                    _ => return None,
                };
                let lists = name_lists(lists)?;
                match count {
                    TokenOrValue::Token(Token::Ident(ident)) if ident.eq_ignore_ascii_case("auto-fill") => {
                        if lines.repeated.is_some() || lists.is_empty() {
                            return None;
                        }
                        lines.repeated = Some(lists);
                    }
                    TokenOrValue::Token(Token::Number {
                        int_value: Some(count),
                        ..
                    }) if *count > 0 => {
                        let count = usize::try_from(*count).ok()?;
                        let limit = MAX_REPEATED_LISTS / lists.len().max(1);
                        for _ in 0..count.min(limit) {
                            target.extend(lists.iter().cloned());
                        }
                    }
                    _ => return None,
                }
            }
            _ => return None,
        }
    }
    current.is_none().then_some(lines)
}

/// The rows and columns of a `grid` or `grid-template` shorthand of the form `rows / columns`
/// where either is `subgrid` (CSS Grid 2, §7.4), which lightningcss does not parse; `None`
/// for other values.
pub fn subgrid_shorthand_sides<'i>(tokens: &TokenList<'i>) -> Option<(TokenList<'i>, TokenList<'i>)> {
    let mut sides: Vec<Vec<TokenOrValue<'i>>> = vec![Vec::new()];
    for token in &tokens.0 {
        match token {
            TokenOrValue::Token(Token::Delim('/')) => sides.push(Vec::new()),
            other => sides.last_mut()?.push(other.clone()),
        }
    }
    let [rows, columns] = <[Vec<TokenOrValue<'i>>; 2]>::try_from(sides).ok()?;
    let (rows, columns) = (TokenList(rows), TokenList(columns));
    (subgrid_lines(&rows).is_some() || subgrid_lines(&columns).is_some()).then_some((rows, columns))
}

/// The lists of names `[…]` of a `repeat()` in a `subgrid` value.
fn name_lists(tokens: &[&TokenOrValue<'_>]) -> Option<Vec<Vec<String>>> {
    let mut lists = Vec::new();
    let mut current: Option<Vec<String>> = None;
    for token in tokens {
        match (token, &mut current) {
            (TokenOrValue::Token(Token::SquareBracketBlock), None) => current = Some(Vec::new()),
            (TokenOrValue::Token(Token::Ident(name)), Some(names)) => names.push(name.to_string()),
            (TokenOrValue::Token(Token::CloseSquareBracket), Some(_)) => lists.extend(current.take()),
            _ => return None,
        }
    }
    current.is_none().then_some(lists)
}

fn significant<'a, 'i>(tokens: &'a TokenList<'i>) -> Vec<&'a TokenOrValue<'i>> {
    tokens
        .0
        .iter()
        .filter(|token| !matches!(token, TokenOrValue::Token(Token::WhiteSpace(_))))
        .collect()
}

#[cfg(test)]
mod tests {
    use lightningcss::properties::Property;
    use lightningcss::stylesheet::ParserOptions;

    use super::*;

    fn position(value: &str) -> Option<String> {
        let property = Property::parse_string("position".into(), value, ParserOptions::default()).ok()?;
        match property {
            Property::Unparsed(unparsed) => running_name(&unparsed.value),
            _ => None,
        }
    }

    #[test]
    fn writes_footnote_rules_as_margin_rules() {
        let css = "@page { margin: 1cm; @footnote { border-top: 1px solid } }\n@media print { @page :left { @FOOTNOTE\n{ padding: 2px } } }\n.a { --x: \"@footnote {\" }\n@footnote { color: red }";
        let encoded = encode_footnote_areas(css);
        assert_eq!(
            encoded,
            "@page { margin: 1cm; @bottom-center {--\u{E001}:1; border-top: 1px solid } }\n@media print { @page :left { @bottom-center\n{--\u{E001}:1; padding: 2px } } }\n.a { --x: \"@footnote {\" }\n@footnote { color: red }"
        );
        assert_eq!(encode_footnote_areas("p { color: red }"), "p { color: red }");
        let sheet =
            lightningcss::stylesheet::StyleSheet::parse(&encoded, ParserOptions::default()).expect("parses");
        let lightningcss::rules::CssRule::Page(page) = &sheet.rules.0[0] else {
            panic!("page rule");
        };
        assert_eq!(page.rules.len(), 1);
        assert!(is_footnote_area(&page.rules[0]));
        assert_eq!(page.rules[0].declarations.declarations.len(), 2);
    }

    #[test]
    fn reads_running_names() {
        assert_eq!(position("running(header)").as_deref(), Some("header"));
        assert_eq!(position(" running( footer ) ").as_deref(), Some("footer"));
        assert_eq!(position("running(a, b)"), None);
        assert_eq!(position("running()"), None);
        assert_eq!(position("absolute"), None);
    }
}

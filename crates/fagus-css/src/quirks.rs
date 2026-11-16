//! Lengths and colors in the author's style sheets and `style` attributes as Chrome reads them.
//!
//! In quirks mode (Quirks Mode, §3.5 and §3.6) a color may leave out its `#` (`color: ff0000`)
//! and a length its unit (`width: 100`, in pixels) in the properties that took them before CSS
//! required them; the CSS is written again with the `#` and the `px`. Elsewhere, and in standards
//! mode everywhere, a number is no length, though lightningcss reads it as one in pixels: the
//! declaration is made invalid, so that it is left out as in Chrome. So is a declaration with a
//! negative number, length or percentage (`padding: -8px 8px`) written out in a property that
//! takes none: Chrome rejects it at parse time, where lightningcss accepts it. A negative value
//! that a function computes (`calc(-8px)`) is valid and clamped later.

use cssparser::{Parser, ParserInput, Token};

/// The properties whose colors may leave out the `#` (the hashless hex color quirk).
const HASHLESS_COLOR: &[&str] = &[
    "background-color",
    "border-color",
    "border-top-color",
    "border-right-color",
    "border-bottom-color",
    "border-left-color",
    "color",
];

/// The properties whose lengths may leave out the unit in quirks mode (the unitless length
/// quirk). None of them takes a number otherwise.
const UNITLESS_LENGTH: &[&str] = &[
    "background-position",
    "border-spacing",
    "border-top-width",
    "border-right-width",
    "border-bottom-width",
    "border-left-width",
    "border-width",
    "bottom",
    "font-size",
    "height",
    "left",
    "letter-spacing",
    "margin",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "max-height",
    "max-width",
    "min-height",
    "min-width",
    "padding",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "right",
    "text-indent",
    "top",
    "vertical-align",
    "width",
    "word-spacing",
];

/// Other properties that take lengths and never a number but `0` (which lightningcss would read
/// as pixels), shorthands among them.
const LENGTH_ONLY: &[&str] = &[
    "background-position-x",
    "background-position-y",
    "background-size",
    "block-size",
    "border",
    "border-top",
    "border-right",
    "border-bottom",
    "border-left",
    "border-block-width",
    "border-block-start-width",
    "border-block-end-width",
    "border-inline-width",
    "border-inline-start-width",
    "border-inline-end-width",
    "border-radius",
    "border-top-left-radius",
    "border-top-right-radius",
    "border-bottom-left-radius",
    "border-bottom-right-radius",
    "column-gap",
    "column-rule-width",
    "column-width",
    "flex-basis",
    "gap",
    "grid-gap",
    "inline-size",
    "inset",
    "inset-block",
    "inset-block-start",
    "inset-block-end",
    "inset-inline",
    "inset-inline-start",
    "inset-inline-end",
    "margin-block",
    "margin-block-start",
    "margin-block-end",
    "margin-inline",
    "margin-inline-start",
    "margin-inline-end",
    "max-block-size",
    "max-inline-size",
    "min-block-size",
    "min-inline-size",
    "outline",
    "outline-offset",
    "outline-width",
    "padding-block",
    "padding-block-start",
    "padding-block-end",
    "padding-inline",
    "padding-inline-start",
    "padding-inline-end",
    "row-gap",
    "text-decoration-thickness",
    "text-underline-offset",
];

/// `css` (a style sheet, or the declarations of a `style` attribute) with its colors and lengths
/// written as Chrome reads them in a document in quirks mode or not (see the module), and the
/// percentages that positions compare kept apart (see [`crate::negative_basis`]); `None` if
/// nothing changes.
pub fn author_css(css: &str, quirks: bool) -> Option<String> {
    if crate::nests_too_deep(css) {
        return None;
    }
    let mut input = ParserInput::new(css);
    let mut parser = Parser::new(&mut input);
    let mut edits = Vec::new();
    walk(&mut parser, &mut edits, (css, quirks), true);
    if edits.is_empty() {
        return None;
    }
    edits.sort_by_key(|&(offset, _)| offset);
    let mut out = String::with_capacity(css.len() + edits.len() * 2);
    let mut at = 0;
    for (offset, insert) in edits {
        out.push_str(&css[at..offset]);
        out.push_str(insert);
        at = offset;
    }
    out.push_str(&css[at..]);
    Some(out)
}

/// A token of a statement at the top level of a block: its kind and where its text is.
struct Piece {
    kind: Kind,
    start: usize,
    end: usize,
    /// Whether the token is a negative number, dimension or percentage.
    negative: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Space,
    Ident,
    Colon,
    Number,
    Dimension,
    Percentage,
    /// A function with its arguments.
    Function,
    Other,
}

/// Walks the statements of a style sheet or block: rules, whose blocks it walks in turn, and
/// declarations (where `declarations` says a block may hold them), noting what to insert where.
fn walk(
    parser: &mut Parser<'_, '_>,
    edits: &mut Vec<(usize, &'static str)>,
    (css, quirks): (&str, bool),
    declarations: bool,
) {
    let mut statement: Vec<Piece> = Vec::new();
    loop {
        let start = parser.position().byte_index();
        let mut negative = false;
        let kind = match parser.next_including_whitespace_and_comments() {
            Err(_) => break,
            Ok(Token::WhiteSpace(_) | Token::Comment(_)) => Kind::Space,
            Ok(Token::Ident(_)) => Kind::Ident,
            Ok(Token::Colon) => Kind::Colon,
            Ok(Token::Number { value, .. }) => {
                negative = *value < 0.0;
                Kind::Number
            }
            Ok(Token::Dimension { value, .. }) => {
                negative = *value < 0.0;
                Kind::Dimension
            }
            Ok(Token::Percentage { unit_value, .. }) => {
                negative = *unit_value < 0.0;
                Kind::Percentage
            }
            Ok(Token::Semicolon) => {
                if declarations {
                    declaration(&statement, edits, (css, quirks));
                }
                statement.clear();
                continue;
            }
            Ok(Token::Function(_)) => {
                // Its arguments belong to the piece.
                let _ = parser.parse_nested_block(|_| Ok::<(), cssparser::ParseError<'_, ()>>(()));
                Kind::Function
            }
            Ok(Token::CurlyBracketBlock) => {
                // A rule: the statement was its prelude. Its block holds declarations, or
                // rules (inside at-rules such as `@media`), or both (nesting).
                statement.clear();
                let _ = parser.parse_nested_block(|block| {
                    walk(block, edits, (css, quirks), true);
                    Ok::<(), cssparser::ParseError<'_, ()>>(())
                });
                continue;
            }
            Ok(_) => Kind::Other,
        };
        let end = parser.position().byte_index();
        statement.push(Piece {
            kind,
            start,
            end,
            negative,
        });
    }
    if declarations {
        declaration(&statement, edits, (css, quirks));
    }
}

/// Notes the `#` and `px` that the value of a declaration (`name: value`, in `pieces`) leaves out
/// where its property takes values without them in quirks mode, and makes numbers where a
/// length belongs, and negative values where the property takes none, invalid elsewhere.
fn declaration(pieces: &[Piece], edits: &mut Vec<(usize, &'static str)>, (css, quirks): (&str, bool)) {
    let mut tokens = pieces.iter().filter(|piece| piece.kind != Kind::Space);
    let (Some(name), Some(colon)) = (tokens.next(), tokens.next()) else {
        return;
    };
    if name.kind != Kind::Ident || colon.kind != Kind::Colon {
        return;
    }
    let property = css[name.start..name.end].to_ascii_lowercase();
    let quirky_length = UNITLESS_LENGTH.contains(&property.as_str());
    let colors = quirks && HASHLESS_COLOR.contains(&property.as_str());
    let lengths = quirky_length || LENGTH_ONLY.contains(&property.as_str());
    let positive = !property.starts_with("--") && crate::repair::forbids_negative(&property);
    if crate::negative_basis::applies(&property) {
        let mut previous = Kind::Colon;
        for token in tokens.clone() {
            if token.kind == Kind::Function {
                let offset = previous == Kind::Ident;
                crate::negative_basis::protect(&property, css, (token.start, token.end), offset, edits);
            }
            previous = token.kind;
        }
    }
    // `flex: <grow> <shrink> <basis>`: a number after both flex factors is a basis without its
    // unit, which only `0` may leave out.
    let mut numbers = 0;
    let flex = property == "flex";
    if !colors && !lengths && !positive {
        return;
    }
    for token in tokens {
        if flex && token.kind == Kind::Number {
            numbers += 1;
            if numbers == 3
                && css[token.start..token.end]
                    .parse::<f32>()
                    .is_ok_and(|value| value != 0.0)
            {
                edits.push((token.end, "__"));
                continue;
            }
        }
        let text = &css[token.start..token.end];
        if positive && token.negative {
            // `-8px__`, `-5%__` and `-2__` are no values of any property.
            edits.push((token.end, "__"));
        } else if colors && is_hashless_color(token.kind, text) {
            edits.push((token.start, "#"));
        } else if lengths && token.kind == Kind::Number && text.parse::<f32>().is_ok_and(|value| value != 0.0)
        {
            // A unit no parser knows makes the declaration invalid.
            edits.push((token.end, if quirks && quirky_length { "px" } else { "__" }));
        }
    }
}

/// Whether a token is a color without its `#`: three or six hexadecimal digits written as a name,
/// as digits, or as digits followed by letters (Quirks Mode, §3.5).
fn is_hashless_color(kind: Kind, text: &str) -> bool {
    if !matches!(text.len(), 3 | 6) || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return false;
    }
    match kind {
        Kind::Ident => !text.as_bytes()[0].is_ascii_digit(),
        Kind::Number => text.bytes().all(|byte| byte.is_ascii_digit()),
        // Digits and then a unit: `00ff00` is the number 0 in the unit `ff00`.
        Kind::Dimension => text.as_bytes()[0].is_ascii_digit(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_hashless_colors_and_unitless_lengths_out_in_quirks_mode() {
        let quirky = |css: &str| author_css(css, true);
        assert_eq!(
            quirky("p { color: ff0000; background-color: 00ff00; border-color: f00 123456 }").as_deref(),
            Some("p { color: #ff0000; background-color: #00ff00; border-color: #f00 #123456 }")
        );
        assert_eq!(
            quirky("div { width: 100; margin: 0 10 -5; font-size: 30; top: 7.5 }").as_deref(),
            Some("div { width: 100px; margin: 0 10px -5px; font-size: 30px; top: 7.5px }")
        );
        // Inside at-rules, and in the declarations of a `style` attribute.
        assert_eq!(
            quirky("@media print { .a { height: 10 } }").as_deref(),
            Some("@media print { .a { height: 10px } }")
        );
        assert_eq!(
            quirky("color: abc; width: 5").as_deref(),
            Some("color: #abc; width: 5px")
        );
        // Not in other properties, shorthands, functions, nor for numbers with an exponent.
        assert_eq!(
            quirky("p { line-height: 40; background: 00ff00; color: 1e2 }"),
            None
        );
        assert_eq!(quirky("p { width: calc(100); color: red; color: ff00 }"), None);
        assert_eq!(quirky("a:hover { color: blue }"), None);
        assert_eq!(quirky("p { gap: 10 }").as_deref(), Some("p { gap: 10__ }"));
    }

    #[test]
    fn makes_unitless_flex_bases_and_negative_flex_factors_invalid() {
        let standard = |css: &str| author_css(css, false);
        assert_eq!(
            standard("p { flex: 0 0 4 }").as_deref(),
            Some("p { flex: 0 0 4__ }")
        );
        assert_eq!(standard("p { flex: 1 1 0; flex: 2 3; flex: 1 0 10px }"), None);
        assert_eq!(
            standard("p { flex-shrink: -2; flex-grow: -1; flex: -1 }").as_deref(),
            Some("p { flex-shrink: -2__; flex-grow: -1__; flex: -1__ }")
        );
    }

    #[test]
    fn makes_numbers_where_lengths_belong_invalid_in_standards_mode() {
        let standard = |css: &str| author_css(css, false);
        assert_eq!(
            standard("p { width: 100; margin: 0 10; border: 1 solid }").as_deref(),
            Some("p { width: 100__; margin: 0 10__; border: 1__ solid }")
        );
        assert_eq!(
            standard("p { color: ff0000; line-height: 1.5; width: 0; flex: 1 }"),
            None
        );
    }

    #[test]
    fn makes_literal_negatives_invalid_where_a_property_takes_none() {
        let standard = |css: &str| author_css(css, false);
        assert_eq!(
            standard("p { padding: -8px 8px; width: -5%; line-height: -2; border-width: 1px -.5em }")
                .as_deref(),
            Some("p { padding: -8px__ 8px; width: -5%__; line-height: -2__; border-width: 1px -.5em__ }")
        );
        assert_eq!(
            author_css("padding-left: -1", true).as_deref(),
            Some("padding-left: -1__")
        );
        // Not in functions, whose results are clamped, nor in properties that take negative
        // values, nor in custom properties.
        assert_eq!(
            standard(
                "p { padding: calc(-8px) max(-1px, 2px); margin: -8px; text-indent: -1em; --gap-size: -4px }"
            ),
            None
        );
    }
}

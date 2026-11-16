//! Functions that are resolved for each element when its values are computed, as `var()` is
//! (CSS Values 5): lightningcss keeps declarations with them as tokens, and Fagus keeps those
//! declarations instead of dropping them as invalid.

use lightningcss::properties::custom::{Function, Token, TokenList, TokenOrValue};

/// Whether `function` is resolved at computed-value time: `if()`, `attr()` with a type or a
/// unit, `sibling-index()`, `sibling-count()`, `progress()`, `contrast-color()`, `round()`
/// of a number alone, which lightningcss does not take, and `image(<color>)`.
pub fn is_computed_function(function: &Function<'_>) -> bool {
    let name = function.name.0.as_ref();
    if [
        "if",
        "sibling-index",
        "sibling-count",
        "progress",
        "contrast-color",
    ]
    .iter()
    .any(|known| name.eq_ignore_ascii_case(known))
    {
        return true;
    }
    if name.eq_ignore_ascii_case("attr") {
        return is_typed_attr(&function.arguments);
    }
    // `image()` of a color alone: Chrome takes no images in it.
    if name.eq_ignore_ascii_case("image") {
        return !has_top_level_comma(&function.arguments)
            && !function.arguments.0.iter().any(|token| {
                matches!(
                    token,
                    TokenOrValue::Url(_) | TokenOrValue::Token(Token::String(_) | Token::UnquotedUrl(_))
                )
            });
    }
    if is_unparsed_color(function) || divides_by_type(function) {
        return true;
    }
    name.eq_ignore_ascii_case("round") && !has_top_level_comma(&function.arguments)
}

/// The functions of colors whose relative form (`hsl(from red calc(h + 120) s l)`) lightningcss
/// may not parse.
pub const COLOR_FUNCTIONS: [&str; 10] = [
    "rgb", "rgba", "hsl", "hsla", "hwb", "lab", "lch", "oklab", "oklch", "color",
];

/// Whether a function is a color that lightningcss left unparsed but Chrome takes: a relative
/// color (`hsl(from …)`, with `calc()` on channel keywords as numbers), `color-mix()` (in a
/// predefined space such as `display-p3`, or of such colors) or a color in the
/// `display-p3-linear` space (`color(display-p3-linear 0 1 0)`). Fagus computes them.
pub fn is_unparsed_color(function: &Function<'_>) -> bool {
    let name = function.name.0.as_ref();
    if name.eq_ignore_ascii_case("color-mix") {
        return true;
    }
    let first = function.arguments.0.iter().find(|token| !token.is_whitespace());
    if name.eq_ignore_ascii_case("color")
        && matches!(first, Some(TokenOrValue::Token(Token::Ident(space))) if space.eq_ignore_ascii_case("display-p3-linear"))
    {
        return true;
    }
    COLOR_FUNCTIONS
        .iter()
        .any(|known| name.eq_ignore_ascii_case(known))
        && matches!(first, Some(TokenOrValue::Token(Token::Ident(from))) if from.eq_ignore_ascii_case("from"))
}

/// The math functions of CSS Values 4 (and `progress()`).
pub const MATH_FUNCTIONS: [&str; 22] = [
    "calc", "min", "max", "clamp", "round", "mod", "rem", "abs", "sign", "pow", "sqrt", "hypot", "log",
    "exp", "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "progress",
];

/// Whether a math function divides by a typed value somewhere (`calc(50px / 10px)`, `(100vw -
/// 2rem) / 1px`), which lightningcss does not parse: Fagus evaluates it.
pub fn divides_by_type(function: &Function<'_>) -> bool {
    MATH_FUNCTIONS
        .iter()
        .any(|known| function.name.0.eq_ignore_ascii_case(known))
        && has_typed_division(&function.arguments.0)
}

fn has_typed_division(tokens: &[TokenOrValue<'_>]) -> bool {
    let significant: Vec<&TokenOrValue<'_>> = tokens.iter().filter(|token| !token.is_whitespace()).collect();
    for (index, token) in significant.iter().enumerate() {
        match token {
            TokenOrValue::Token(Token::Delim('/')) => {
                let divisor = match significant.get(index + 1) {
                    Some(TokenOrValue::Token(Token::ParenthesisBlock)) => {
                        let mut depth = 0_i32;
                        let rest = &significant[index + 1..];
                        let end = rest
                            .iter()
                            .position(|token| {
                                depth += match token {
                                    TokenOrValue::Token(Token::ParenthesisBlock) => 1,
                                    TokenOrValue::Token(Token::CloseParenthesis) => -1,
                                    _ => 0,
                                };
                                depth == 0
                            })
                            .unwrap_or(rest.len());
                        rest[..end].iter().any(|token| is_typed(token))
                    }
                    Some(token) => is_typed(token),
                    None => false,
                };
                if divisor {
                    return true;
                }
            }
            TokenOrValue::Function(function) if has_typed_division(&function.arguments.0) => return true,
            _ => {}
        }
    }
    false
}

/// Whether a token is a typed value (a dimension or a percentage), or a function of one.
fn is_typed(token: &TokenOrValue<'_>) -> bool {
    match token {
        TokenOrValue::Length(_)
        | TokenOrValue::Angle(_)
        | TokenOrValue::Time(_)
        | TokenOrValue::Resolution(_)
        | TokenOrValue::Token(Token::Dimension { .. } | Token::Percentage { .. }) => true,
        TokenOrValue::Function(function) => function.arguments.0.iter().any(is_typed),
        _ => false,
    }
}

/// Whether the arguments of `attr()` give a type (`type(<length>)`, `raw-string`) or a unit
/// after the attribute's name, which the `attr()` of CSS 2 (in `content`) does not have.
pub fn is_typed_attr(arguments: &TokenList<'_>) -> bool {
    let mut significant = arguments
        .0
        .iter()
        .filter(|token| !matches!(token, TokenOrValue::Token(Token::WhiteSpace(_))));
    let _name = significant.next();
    matches!(
        significant.next(),
        Some(TokenOrValue::Function(_) | TokenOrValue::Token(Token::Ident(_) | Token::Delim('%')))
    )
}

fn has_top_level_comma(arguments: &TokenList<'_>) -> bool {
    let mut depth = 0_usize;
    arguments.0.iter().any(|token| match token {
        TokenOrValue::Token(
            Token::ParenthesisBlock | Token::SquareBracketBlock | Token::CurlyBracketBlock,
        ) => {
            depth += 1;
            false
        }
        TokenOrValue::Token(
            Token::CloseParenthesis | Token::CloseSquareBracket | Token::CloseCurlyBracket,
        ) => {
            depth = depth.saturating_sub(1);
            false
        }
        TokenOrValue::Token(Token::Comma) => depth == 0,
        _ => false,
    })
}

/// Whether tokens contain `var()`, `env()`, a color lightningcss leaves unresolved or a function
/// of [`is_computed_function`]: their declarations can only be checked once those are resolved.
pub fn has_computed_values(tokens: &TokenList<'_>) -> bool {
    tokens.0.iter().any(|token| match token {
        TokenOrValue::Var(_) | TokenOrValue::Env(_) | TokenOrValue::UnresolvedColor(_) => true,
        TokenOrValue::Function(function) => {
            is_computed_function(function) || has_computed_values(&function.arguments)
        }
        _ => false,
    })
}

/// Whether tokens contain `anchor()` or `anchor-size()` (CSS Anchor Positioning 1), which refer
/// to the boxes of other elements and are resolved in layout.
pub fn has_anchor_functions(tokens: &TokenList<'_>) -> bool {
    tokens.0.iter().any(|token| match token {
        TokenOrValue::Function(function) => {
            let name = function.name.0.as_ref();
            name.eq_ignore_ascii_case("anchor")
                || name.eq_ignore_ascii_case("anchor-size")
                || has_anchor_functions(&function.arguments)
        }
        _ => false,
    })
}

/// Whether an unparsed declaration is one of anchor positioning that lightningcss does not parse:
/// an inset, size or margin with `anchor()` or `anchor-size()`, or `justify-self` or
/// `align-self` of `anchor-center`.
pub fn is_anchored(unparsed: &lightningcss::properties::custom::UnparsedProperty<'_>) -> bool {
    use lightningcss::properties::PropertyId;
    match &unparsed.property_id {
        PropertyId::JustifySelf | PropertyId::AlignSelf(_) | PropertyId::PlaceSelf => unparsed
            .value
            .0
            .iter()
            .any(|token| matches!(token, TokenOrValue::Token(Token::Ident(ident)) if ident.eq_ignore_ascii_case("anchor-center"))),
        PropertyId::Top
        | PropertyId::Right
        | PropertyId::Bottom
        | PropertyId::Left
        | PropertyId::Inset
        | PropertyId::InsetBlock
        | PropertyId::InsetInline
        | PropertyId::InsetBlockStart
        | PropertyId::InsetBlockEnd
        | PropertyId::InsetInlineStart
        | PropertyId::InsetInlineEnd
        | PropertyId::Width
        | PropertyId::Height
        | PropertyId::MinWidth
        | PropertyId::MinHeight
        | PropertyId::MaxWidth
        | PropertyId::MaxHeight
        | PropertyId::BlockSize
        | PropertyId::InlineSize
        | PropertyId::Margin
        | PropertyId::MarginTop
        | PropertyId::MarginRight
        | PropertyId::MarginBottom
        | PropertyId::MarginLeft => has_anchor_functions(&unparsed.value),
        _ => false,
    }
}

/// Whether an unparsed declaration is a size of `calc-size()` (CSS Values 5), which lightningcss
/// does not parse and layout resolves.
pub fn is_calc_size(unparsed: &lightningcss::properties::custom::UnparsedProperty<'_>) -> bool {
    use lightningcss::properties::PropertyId;
    matches!(
        unparsed.property_id,
        PropertyId::Width
            | PropertyId::Height
            | PropertyId::MinWidth
            | PropertyId::MinHeight
            | PropertyId::MaxWidth
            | PropertyId::MaxHeight
            | PropertyId::InlineSize
            | PropertyId::BlockSize
    ) && matches!(
        unparsed.value.0.iter().filter(|token| !token.is_whitespace()).collect::<Vec<_>>().as_slice(),
        [TokenOrValue::Function(function)] if function.name.0.eq_ignore_ascii_case("calc-size")
    )
}

/// Whether `css` is a value of `syntax` (of `@property`, or of `attr()`'s `type()`), all of it.
pub fn matches_syntax(syntax: &lightningcss::values::syntax::SyntaxString, css: &str) -> bool {
    crate::guarded(|| {
        let mut input = cssparser::ParserInput::new(css);
        let mut parser = cssparser::Parser::new(&mut input);
        syntax.parse_value(&mut parser).is_ok() && parser.expect_exhausted().is_ok()
    })
    .unwrap_or(false)
}

/// The `path()` shape of a `clip-path` that lightningcss keeps as tokens (CSS Shapes 1, §3.1):
/// whether it fills by the even-odd rule, and its SVG path data. `None` for other values.
pub fn clip_path_function<'t>(
    unparsed: &'t lightningcss::properties::custom::UnparsedProperty<'_>,
) -> Option<(bool, &'t str)> {
    use lightningcss::properties::PropertyId;
    if !matches!(unparsed.property_id, PropertyId::ClipPath(_)) {
        return None;
    }
    let [TokenOrValue::Function(function)] = unparsed
        .value
        .0
        .iter()
        .filter(|token| !token.is_whitespace())
        .collect::<Vec<_>>()
        .as_slice()
    else {
        return None;
    };
    if !function.name.0.eq_ignore_ascii_case("path") {
        return None;
    }
    let arguments: Vec<_> = function
        .arguments
        .0
        .iter()
        .filter(|token| !token.is_whitespace())
        .collect();
    let (even_odd, data) = match arguments.as_slice() {
        [TokenOrValue::Token(Token::String(data))] => (false, data),
        [
            TokenOrValue::Token(Token::Ident(rule)),
            TokenOrValue::Token(Token::Comma),
            TokenOrValue::Token(Token::String(data)),
        ] if rule.eq_ignore_ascii_case("evenodd") || rule.eq_ignore_ascii_case("nonzero") => {
            (rule.eq_ignore_ascii_case("evenodd"), data)
        }
        _ => return None,
    };
    Some((even_odd, data.as_ref()))
}

/// Whether a `clip-path` that lightningcss keeps as tokens is a shape of CSS Shapes 2 that it
/// does not parse, which the style reads: one `xywh()` (four lengths or percentages), `rect()`
/// (four of them or `auto`), each with an optional ` round …`, or `shape()`, with an optional
/// box keyword. Other values are not valid, as in Chrome.
pub fn is_newer_clip_shape(unparsed: &lightningcss::properties::custom::UnparsedProperty<'_>) -> bool {
    use lightningcss::properties::PropertyId;
    const BOXES: [&str; 7] = [
        "border-box",
        "padding-box",
        "content-box",
        "margin-box",
        "fill-box",
        "stroke-box",
        "view-box",
    ];
    if !matches!(unparsed.property_id, PropertyId::ClipPath(_)) {
        return false;
    }
    let (mut shapes, mut boxes) = (0, 0);
    for token in unparsed.value.0.iter().filter(|token| !token.is_whitespace()) {
        match token {
            TokenOrValue::Function(function) if function.name.0.eq_ignore_ascii_case("shape") => shapes += 1,
            TokenOrValue::Function(function)
                if function.name.0.eq_ignore_ascii_case("xywh")
                    || function.name.0.eq_ignore_ascii_case("rect") =>
            {
                if !four_edges(&function.arguments, function.name.0.eq_ignore_ascii_case("rect")) {
                    return false;
                }
                shapes += 1;
            }
            TokenOrValue::Token(Token::Ident(ident))
                if BOXES.iter().any(|name| ident.eq_ignore_ascii_case(name)) =>
            {
                boxes += 1;
            }
            _ => return false,
        }
    }
    shapes == 1 && boxes <= 1
}

/// Whether the arguments of `xywh()` or `rect()` (`auto` allowed, for `rect()`) are four lengths
/// or percentages, before an optional ` round …`.
fn four_edges(arguments: &TokenList<'_>, auto: bool) -> bool {
    let mut count = 0;
    for token in arguments.0.iter().filter(|token| !token.is_whitespace()) {
        match token {
            TokenOrValue::Token(Token::Ident(ident)) if ident.eq_ignore_ascii_case("round") => break,
            TokenOrValue::Token(Token::Ident(ident)) if auto && ident.eq_ignore_ascii_case("auto") => {
                count += 1;
            }
            TokenOrValue::Length(_)
            | TokenOrValue::Token(Token::Percentage { .. } | Token::Dimension { .. }) => count += 1,
            TokenOrValue::Token(Token::Number { value, .. }) if *value == 0.0 => count += 1,
            TokenOrValue::Function(function)
                if ["calc", "min", "max", "clamp"]
                    .iter()
                    .any(|name| function.name.0.eq_ignore_ascii_case(name)) =>
            {
                count += 1;
            }
            _ => return false,
        }
    }
    count == 4
}

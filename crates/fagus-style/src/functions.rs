//! Functions that are resolved for each element at computed-value time, alongside `var()`
//! (CSS Values 5): `if()`, `attr()` with a type or a unit, `sibling-index()`,
//! `sibling-count()`, `progress()`, `contrast-color()`, `round()` of a number alone, and the
//! environment variables of `env()` that Chrome defines.

use fagus_css::MediaEnvironment;
use fagus_dom::{Document, NodeId};
use lightningcss::properties::custom::{
    EnvironmentVariable, EnvironmentVariableName, Token, TokenList, TokenOrValue,
};
use lightningcss::values::length::LengthValue;

/// What the functions of a value refer to besides custom properties.
#[derive(Clone, Copy, Default)]
pub struct Functions<'a> {
    /// The element whose value is computed (the originating element of a pseudo-element): its
    /// attributes for `attr()`, its place among its siblings for `sibling-index()`.
    pub element: Option<(&'a Document, NodeId)>,
    /// The page that `media()` in `if()` asks about.
    pub media: Option<&'a MediaEnvironment>,
    /// Whether math functions (`sibling-index()`, `progress()`, …) are resolved: in the values
    /// of properties, not in those of custom properties, which the elements that use them
    /// resolve for themselves.
    pub math: bool,
    /// What lengths come to in pixels (without the element's zoom), for math functions that
    /// Fagus evaluates (typed division, `progress()`): absolute lengths alone where it is
    /// `None`.
    pub lengths: crate::math::Lengths<'a>,
}

/// The significant tokens of a list: without white space.
fn significant<'t>(tokens: &'t [TokenOrValue<'static>]) -> Vec<&'t TokenOrValue<'static>> {
    tokens
        .iter()
        .filter(|token| !matches!(token, TokenOrValue::Token(Token::WhiteSpace(_))))
        .collect()
}

/// How a token changes the depth of nested blocks.
fn nesting(token: &TokenOrValue<'_>) -> isize {
    match token {
        TokenOrValue::Token(
            Token::ParenthesisBlock | Token::SquareBracketBlock | Token::CurlyBracketBlock,
        ) => 1,
        TokenOrValue::Token(
            Token::CloseParenthesis | Token::CloseSquareBracket | Token::CloseCurlyBracket,
        ) => -1,
        _ => 0,
    }
}

/// `tokens` split at the separators outside of blocks.
fn split<'t>(
    tokens: &'t [TokenOrValue<'static>],
    separator: impl Fn(&TokenOrValue<'static>) -> bool,
) -> Vec<&'t [TokenOrValue<'static>]> {
    let mut parts = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    for (index, token) in tokens.iter().enumerate() {
        depth += nesting(token);
        if depth == 0 && separator(token) {
            parts.push(&tokens[start..index]);
            start = index + 1;
        }
    }
    parts.push(&tokens[start..]);
    parts
}

/// `tokens` split at the first separator outside of blocks.
fn split_once<'t>(
    tokens: &'t [TokenOrValue<'static>],
    separator: impl Fn(&TokenOrValue<'static>) -> bool,
) -> Option<(&'t [TokenOrValue<'static>], &'t [TokenOrValue<'static>])> {
    let mut depth = 0;
    for (index, token) in tokens.iter().enumerate() {
        depth += nesting(token);
        if depth == 0 && separator(token) {
            return Some((&tokens[..index], &tokens[index + 1..]));
        }
    }
    None
}

fn is_comma(token: &TokenOrValue<'_>) -> bool {
    matches!(token, TokenOrValue::Token(Token::Comma))
}

/// The value of an environment variable that Chrome defines when it prints: no safe areas and
/// no virtual keyboard, text at its preferred scale. `None` for the others (the title bar of an
/// installed app, the segments of a folded screen, author-defined ones), whose fallback applies.
pub fn environment(variable: &EnvironmentVariable<'_>) -> Option<TokenOrValue<'static>> {
    let name = match &variable.name {
        EnvironmentVariableName::UA(ua) => ua.as_str(),
        EnvironmentVariableName::Unknown(ident) => ident.0.as_ref(),
        EnvironmentVariableName::Custom(_) => return None,
    };
    let name = name.to_ascii_lowercase();
    if name.starts_with("safe-area-inset-")
        || name.starts_with("safe-area-max-inset-")
        || name.starts_with("keyboard-inset-")
    {
        Some(TokenOrValue::Length(LengthValue::Px(0.0)))
    } else if name == "preferred-text-scale" {
        Some(number(1.0))
    } else {
        None
    }
}

fn number(value: f32) -> TokenOrValue<'static> {
    TokenOrValue::Token(Token::Number {
        has_sign: false,
        value,
        // Whole numbers keep their integer value, as the tokenizer gives them.
        #[expect(clippy::cast_possible_truncation, reason = "checked to be whole and in range")]
        int_value: (value.fract() == 0.0 && value.abs() < 1e9).then_some(value as i32),
    })
}

/// What `attr()` gives.
pub enum Attr<'t> {
    /// The attribute's value, as the tokens of its type.
    Value(TokenList<'static>),
    /// The fallback's tokens, where the attribute is missing or not of its type.
    Fallback(&'t [TokenOrValue<'static>]),
    /// Neither: the declaration is invalid at computed-value time.
    Invalid,
}

/// Resolves `attr(<name> <type>?, <fallback>?)` with a type (`type(<length>)`, `raw-string`)
/// or a unit (`px`, `%`, …) for an element (CSS Values 5, §7.7).
pub fn attr<'t>(arguments: &'t TokenList<'static>, element: Option<(&Document, NodeId)>) -> Attr<'t> {
    let (head, fallback) = match split_once(&arguments.0, is_comma) {
        Some((head, fallback)) => (head, Some(fallback)),
        None => (&arguments.0[..], None),
    };
    let otherwise = || fallback.map_or(Attr::Invalid, Attr::Fallback);
    let head = significant(head);
    let Some(TokenOrValue::Token(Token::Ident(name))) = head.first() else {
        return Attr::Invalid;
    };
    let value = element.and_then(|(document, node)| {
        let data = document.element(node)?;
        if data.is_html() {
            data.attribute(&name.to_ascii_lowercase())
        } else {
            data.attribute(name.as_ref())
        }
    });
    let Some(value) = value else {
        return otherwise();
    };
    let parsed = match head.get(1) {
        // `type(<syntax>)`: the value must be of the syntax.
        Some(TokenOrValue::Function(function)) if function.name.0.eq_ignore_ascii_case("type") => {
            let syntax = crate::variables::to_css(&function.arguments)
                .and_then(|css| lightningcss::values::syntax::SyntaxString::parse_string(&css).ok());
            syntax
                .filter(|syntax| fagus_css::matches_syntax(syntax, value))
                .and_then(|_| crate::variables::value_tokens(value))
        }
        Some(TokenOrValue::Token(Token::Ident(kind))) if kind.eq_ignore_ascii_case("raw-string") => {
            crate::variables::value_tokens(value)
        }
        // A unit: the value is a number of it.
        Some(TokenOrValue::Token(Token::Ident(unit))) => value
            .trim()
            .parse::<f32>()
            .ok()
            .filter(|number| number.is_finite())
            .and_then(|number| crate::variables::value_tokens(&format!("{number}{unit}"))),
        Some(TokenOrValue::Token(Token::Delim('%'))) => value
            .trim()
            .parse::<f32>()
            .ok()
            .filter(|number| number.is_finite())
            .and_then(|number| crate::variables::value_tokens(&format!("{number}%"))),
        _ => None,
    };
    parsed.map_or_else(otherwise, Attr::Value)
}

/// What a condition of `if()` comes to: as in media queries, it may be unknown, and `not`
/// unknown is unknown.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Truth {
    True,
    False,
    Unknown,
}

impl Truth {
    fn of(holds: bool) -> Self {
        if holds { Self::True } else { Self::False }
    }

    fn not(self) -> Self {
        match self {
            Self::True => Self::False,
            Self::False => Self::True,
            Self::Unknown => Self::Unknown,
        }
    }
}

/// A test of `if()`.
#[derive(Clone, Copy)]
pub enum Test<'t> {
    Media(&'t TokenList<'static>),
    Supports(&'t TokenList<'static>),
    Style(&'t TokenList<'static>),
}

/// The value of the first branch of `if()` whose condition holds (CSS Values 5, §11), `None`
/// where none does: the declaration is then invalid at computed-value time.
pub fn choose<'t>(
    arguments: &'t TokenList<'static>,
    test: &dyn Fn(Test<'_>) -> Truth,
) -> Option<&'t [TokenOrValue<'static>]> {
    for branch in split(&arguments.0, |token| {
        matches!(token, TokenOrValue::Token(Token::Semicolon))
    }) {
        if significant(branch).is_empty() {
            continue;
        }
        let (condition, value) =
            split_once(branch, |token| matches!(token, TokenOrValue::Token(Token::Colon)))?;
        let condition = significant(condition);
        let holds = match condition.as_slice() {
            [TokenOrValue::Token(Token::Ident(ident))] if ident.eq_ignore_ascii_case("else") => Truth::True,
            tokens => {
                let mut position = 0;
                let holds = expression(tokens, &mut position, test)?;
                if position != tokens.len() {
                    return None;
                }
                holds
            }
        };
        if holds == Truth::True {
            return Some(value);
        }
    }
    None
}

/// A boolean expression of tests: `not` one, several joined by `and` or by `or`, in
/// parentheses or not; `None` where it does not parse.
fn expression(
    tokens: &[&TokenOrValue<'static>],
    position: &mut usize,
    test: &dyn Fn(Test<'_>) -> Truth,
) -> Option<Truth> {
    let keyword = |token: Option<&&TokenOrValue<'static>>, word: &str| matches!(token, Some(TokenOrValue::Token(Token::Ident(ident))) if ident.eq_ignore_ascii_case(word));
    if keyword(tokens.get(*position), "not") {
        *position += 1;
        return Some(primary(tokens, position, test)?.not());
    }
    let mut result = primary(tokens, position, test)?;
    let joiner = if keyword(tokens.get(*position), "and") {
        "and"
    } else if keyword(tokens.get(*position), "or") {
        "or"
    } else {
        return Some(result);
    };
    while keyword(tokens.get(*position), joiner) {
        *position += 1;
        let next = primary(tokens, position, test)?;
        result = match (joiner, result, next) {
            ("and", Truth::False, _) | ("and", _, Truth::False) => Truth::False,
            ("or", Truth::True, _) | ("or", _, Truth::True) => Truth::True,
            (_, Truth::True | Truth::False, Truth::True | Truth::False) => Truth::of(joiner == "and"),
            _ => Truth::Unknown,
        };
    }
    Some(result)
}

/// A test, or an expression in parentheses.
fn primary(
    tokens: &[&TokenOrValue<'static>],
    position: &mut usize,
    test: &dyn Fn(Test<'_>) -> Truth,
) -> Option<Truth> {
    match tokens.get(*position)? {
        TokenOrValue::Function(function) => {
            *position += 1;
            let name = function.name.0.as_ref();
            let arguments = &function.arguments;
            if name.eq_ignore_ascii_case("media") {
                Some(test(Test::Media(arguments)))
            } else if name.eq_ignore_ascii_case("supports") {
                Some(test(Test::Supports(arguments)))
            } else if name.eq_ignore_ascii_case("style") {
                Some(test(Test::Style(arguments)))
            } else {
                None
            }
        }
        TokenOrValue::Token(Token::ParenthesisBlock) => {
            *position += 1;
            let result = expression(tokens, position, test)?;
            match tokens.get(*position)? {
                TokenOrValue::Token(Token::CloseParenthesis) => {
                    *position += 1;
                    Some(result)
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// The position of an element among the element children of its parent, from 1, and how many
/// they are.
pub fn sibling(document: &Document, node: NodeId) -> (usize, usize) {
    let mut index = 1;
    let mut previous = node;
    while let Some(sibling) = document.previous_sibling_element(previous) {
        index += 1;
        previous = sibling;
    }
    let mut count = index;
    let mut next = node;
    while let Some(sibling) = document.next_sibling_element(next) {
        count += 1;
        next = sibling;
    }
    (index, count)
}

/// `progress(<value>, <start>, <end>)`: where the value lies from the start to the end, from 0
/// to 1, of arguments of one type (math expressions, lengths relative to the font too).
pub fn progress(
    arguments: &TokenList<'static>,
    lengths: crate::math::Lengths<'_>,
) -> Option<TokenOrValue<'static>> {
    let function = lightningcss::properties::custom::Function {
        name: lightningcss::values::ident::Ident("progress".into()),
        arguments: arguments.clone(),
    };
    crate::math::evaluate_function(&function, lengths)?.token()
}

/// `contrast-color(<color>)`: white or black, whichever contrasts more with the color, white
/// where both do alike (CSS Color 5, §5).
pub fn contrast_color(arguments: &TokenList<'static>) -> Option<TokenOrValue<'static>> {
    let rgba = crate::compute::color(&crate::color_functions::token_color(&arguments.0)?)?;
    let linear = |channel: f32| {
        if channel <= 0.040_45 {
            channel / 12.92
        } else {
            libm::powf((channel + 0.055) / 1.055, 2.4)
        }
    };
    let luminance = 0.2126 * linear(rgba.r) + 0.7152 * linear(rgba.g) + 0.0722 * linear(rgba.b);
    let with_white = 1.05 / (luminance + 0.05);
    let with_black = (luminance + 0.05) / 0.05;
    let name = if with_white >= with_black {
        "white"
    } else {
        "black"
    };
    Some(TokenOrValue::Token(Token::Ident(name.into())))
}

/// `image(<color>)` (CSS Images 4, §2.2): an image of no size in the color, as the gradient
/// `linear-gradient(<color>, <color>)` draws it; `None` for other arguments, which Chrome does
/// not take.
pub fn image(arguments: &TokenList<'static>) -> Option<TokenOrValue<'static>> {
    let color = TokenOrValue::Color(crate::color_functions::token_color(&arguments.0)?);
    Some(TokenOrValue::Function(
        lightningcss::properties::custom::Function {
            name: lightningcss::values::ident::Ident("linear-gradient".into()),
            arguments: TokenList(vec![color.clone(), TokenOrValue::Token(Token::Comma), color]),
        },
    ))
}

/// Whether the two values of a `light-dark()` are images (CSS Color 5, §5.2: `light-dark()`
/// of two images is an image), not colors: whether one is a gradient, `url()`, `image()` or
/// `image-set()`.
pub fn is_image_pair(light: &TokenList<'_>, dark: &TokenList<'_>) -> bool {
    let is_image = |tokens: &TokenList<'_>| {
        tokens.0.iter().any(|token| match token {
            TokenOrValue::Url(_) => true,
            TokenOrValue::Function(function) => {
                let name = function.name.0.to_ascii_lowercase();
                name.ends_with("gradient") || name == "image" || name.ends_with("image-set") || name == "url"
            }
            TokenOrValue::Token(Token::Ident(ident)) => ident.eq_ignore_ascii_case("none"),
            _ => false,
        })
    };
    is_image(light) || is_image(dark)
}

/// The arguments of `round(<number>)`, which lightningcss only takes with the step: 1.
pub fn round_arguments(arguments: &TokenList<'static>) -> TokenList<'static> {
    let mut tokens = arguments.0.clone();
    tokens.push(TokenOrValue::Token(Token::Comma));
    tokens.push(number(1.0));
    TokenList(tokens)
}

/// Whether `test` holds for an element with the custom properties `lookup` gives: unknown for
/// `media()` of a media type (`print`), which takes conditions alone.
pub fn test<'v>(
    test: Test<'_>,
    lookup: &impl Fn(&str) -> Option<&'v TokenList<'static>>,
    media: Option<&MediaEnvironment>,
) -> Truth {
    Truth::of(match test {
        Test::Media(arguments) => {
            if matches!(
                significant(&arguments.0).as_slice(),
                [TokenOrValue::Token(Token::Ident(_))]
            ) {
                return Truth::Unknown;
            }
            media.is_some_and(|media| {
                crate::variables::to_css(arguments).is_some_and(|css| media.matches_condition(&css))
            })
        }
        Test::Supports(arguments) => {
            crate::variables::to_css(arguments).is_some_and(|css| fagus_css::supports(css.trim()))
        }
        Test::Style(arguments) => {
            let text = |tokens: &TokenList<'static>| {
                crate::variables::to_css(tokens).map(|css| css.trim().to_owned())
            };
            match split_once(&arguments.0, |token| {
                matches!(token, TokenOrValue::Token(Token::Colon))
            }) {
                Some((name, value)) => custom_name(name)
                    .and_then(|name| lookup(&name))
                    .zip(crate::variables::substitute(&TokenList(value.to_vec()), lookup))
                    .is_some_and(|(actual, expected)| {
                        text(actual).is_some() && text(actual) == text(&expected)
                    }),
                None => custom_name(&arguments.0).is_some_and(|name| lookup(&name).is_some()),
            }
        }
    })
}

/// The name of a custom property alone among tokens.
fn custom_name(tokens: &[TokenOrValue<'static>]) -> Option<String> {
    match significant(tokens).as_slice() {
        [TokenOrValue::DashedIdent(name)] => Some(name.0.to_string()),
        [TokenOrValue::Token(Token::Ident(name))] if name.starts_with("--") => Some(name.to_string()),
        _ => None,
    }
}

//! Gradients that lightningcss rejects although browsers accept them: color interpolation methods
//! (`linear-gradient(to right in oklab, …)`, CSS Images 4) and unitless zero angles in conic
//! gradients (`conic-gradient(red 0 25%, …)`).
//!
//! Fagus repairs the tokens, lets lightningcss parse the result, and keeps the interpolation
//! methods on the side.

use lightningcss::properties::custom::{Function, Token, TokenList, TokenOrValue};
use lightningcss::values::angle::Angle;

/// A color interpolation method: the color space and, for polar spaces, the hue method.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InterpolationMethod {
    pub space: String,
    pub hue: Option<String>,
}

fn is_gradient(function: &Function<'_>) -> bool {
    let name = function.name.0.to_ascii_lowercase();
    name.ends_with("gradient") && !name.starts_with('-')
}

fn is_conic(function: &Function<'_>) -> bool {
    function.name.0.to_ascii_lowercase().ends_with("conic-gradient")
}

fn is_unitless_zero(token: &TokenOrValue<'_>) -> bool {
    matches!(token, TokenOrValue::Token(Token::Number { value, .. }) if *value == 0.0)
}

/// Whether `tokens` contain a gradient that lightningcss rejects but [`repair`] fixes.
pub fn needs_repair(tokens: &TokenList<'_>) -> bool {
    tokens.0.iter().any(|token| match token {
        TokenOrValue::Function(function) if is_gradient(function) => {
            method_range(&function.arguments).is_some()
                || (is_conic(function) && function.arguments.0.iter().any(is_unitless_zero))
                || needs_repair(&function.arguments)
        }
        TokenOrValue::Function(function) => needs_repair(&function.arguments),
        _ => false,
    })
}

/// Removes the color interpolation methods of all gradients in `tokens` and gives unitless zero
/// angles in conic gradients a unit. Returns the tokens and the method of every gradient, in
/// order (`None` where a gradient has none).
pub fn repair(tokens: &TokenList<'static>) -> (TokenList<'static>, Vec<Option<InterpolationMethod>>) {
    let mut methods = Vec::new();
    let stripped = strip(tokens, &mut methods);
    (stripped, methods)
}

fn strip(tokens: &TokenList<'static>, methods: &mut Vec<Option<InterpolationMethod>>) -> TokenList<'static> {
    let mut result = Vec::with_capacity(tokens.0.len());
    for token in &tokens.0 {
        match token {
            TokenOrValue::Function(function) if is_gradient(function) => {
                let mut arguments = function.arguments.0.clone();
                let method = method_range(&function.arguments).map(|(start, end, method)| {
                    arguments.drain(start..end);
                    method
                });
                if method.is_some() {
                    // A first argument that held nothing but the method goes away with its comma.
                    let comma = arguments
                        .iter()
                        .position(|token| matches!(token, TokenOrValue::Token(Token::Comma)));
                    if let Some(comma) = comma
                        && arguments[..comma].iter().all(is_space)
                    {
                        let next = arguments[comma + 1..]
                            .iter()
                            .position(|token| !is_space(token))
                            .map_or(arguments.len(), |offset| comma + 1 + offset);
                        arguments.drain(..next);
                    }
                }
                if is_conic(function) {
                    for token in &mut arguments {
                        if is_unitless_zero(token) {
                            *token = TokenOrValue::Angle(Angle::Deg(0.0));
                        }
                    }
                }
                methods.push(method);
                result.push(TokenOrValue::Function(Function {
                    name: function.name.clone(),
                    arguments: TokenList(arguments),
                }));
            }
            TokenOrValue::Function(function) => result.push(TokenOrValue::Function(Function {
                name: function.name.clone(),
                arguments: strip(&function.arguments, methods),
            })),
            other => result.push(other.clone()),
        }
    }
    TokenList(result)
}

fn is_space(token: &TokenOrValue<'_>) -> bool {
    matches!(token, TokenOrValue::Token(Token::WhiteSpace(_)))
}

fn ident<'a>(token: &'a TokenOrValue<'_>) -> Option<&'a str> {
    match token {
        TokenOrValue::Token(Token::Ident(ident)) => Some(ident.as_ref()),
        _ => None,
    }
}

/// The token range of the interpolation method in the first argument of a gradient:
/// `in <color-space> [<hue-method> hue]`.
fn method_range(arguments: &TokenList<'_>) -> Option<(usize, usize, InterpolationMethod)> {
    let tokens = &arguments.0;
    let first_comma = tokens
        .iter()
        .position(|token| matches!(token, TokenOrValue::Token(Token::Comma)))
        .unwrap_or(tokens.len());
    let start = tokens[..first_comma]
        .iter()
        .position(|token| ident(token).is_some_and(|name| name.eq_ignore_ascii_case("in")))?;
    let mut words = Vec::new();
    let mut end = start + 1;
    while end < first_comma {
        let token = &tokens[end];
        if let Some(word) = ident(token) {
            if words.len() == 1 && !is_hue_method(word) && !word.eq_ignore_ascii_case("hue") {
                break;
            }
            words.push(word.to_ascii_lowercase());
            end += 1;
            if words.len() == 3 {
                break;
            }
        } else if is_space(token) {
            end += 1;
        } else {
            break;
        }
    }
    let space = words.first()?.clone();
    let hue = (words.len() == 3 && words[2] == "hue").then(|| words[1].clone());
    // Take the white space before `in` along.
    let start = if start > 0 && is_space(&tokens[start - 1]) {
        start - 1
    } else {
        start
    };
    Some((start, end, InterpolationMethod { space, hue }))
}

fn is_hue_method(word: &str) -> bool {
    ["shorter", "longer", "increasing", "decreasing"]
        .iter()
        .any(|method| word.eq_ignore_ascii_case(method))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightningcss::properties::custom::CustomPropertyName;
    use lightningcss::properties::{Property, PropertyId};
    use lightningcss::stylesheet::{ParserOptions, PrinterOptions};
    use lightningcss::traits::IntoOwned;

    fn tokens(css: &str) -> TokenList<'static> {
        match Property::parse_string(PropertyId::from("--t"), css, ParserOptions::default())
            .expect("tokens")
            .into_owned()
        {
            Property::Custom(custom) => custom.value,
            _ => unreachable!(),
        }
    }

    fn css(tokens: TokenList<'static>) -> String {
        Property::Custom(lightningcss::properties::custom::CustomProperty {
            name: CustomPropertyName::Custom(lightningcss::values::ident::DashedIdent("--t".into())),
            value: tokens,
        })
        .value_to_css_string(PrinterOptions::default())
        .expect("css")
    }

    #[test]
    fn strips_interpolation_methods() {
        let input = tokens(
            "linear-gradient(to right in oklch longer hue, red, blue), radial-gradient(in srgb, red, blue), linear-gradient(red, blue)",
        );
        assert!(needs_repair(&input));
        let (stripped, methods) = repair(&input);
        assert_eq!(
            css(stripped),
            "linear-gradient(to right, red, blue), radial-gradient(red, blue), linear-gradient(red, blue)"
        );
        assert_eq!(
            methods,
            vec![
                Some(InterpolationMethod {
                    space: "oklch".into(),
                    hue: Some("longer".into())
                }),
                Some(InterpolationMethod {
                    space: "srgb".into(),
                    hue: None
                }),
                None
            ]
        );
        assert!(!needs_repair(&tokens("linear-gradient(red, blue)")));
    }

    #[test]
    fn gives_conic_zero_angles_a_unit() {
        let input = tokens("conic-gradient(red 0 25%, blue 0)");
        assert!(needs_repair(&input));
        assert_eq!(css(repair(&input).0), "conic-gradient(red 0deg 25%, blue 0deg)");
    }
}

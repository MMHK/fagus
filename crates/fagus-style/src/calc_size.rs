//! `calc-size()` (CSS Values 5, §8): a size computed from a basis that layout resolves (`auto`,
//! `fit-content`, `min-content`, `max-content`, `stretch`, a length or `any`), with the
//! keyword `size` for it in a calculation. The basis becomes the property's own value, and the
//! calculations wait for layout, which applies them to the size the basis gives.

use std::sync::Arc;

use lightningcss::properties::PropertyId;
use lightningcss::properties::custom::{Function, Token, TokenList, TokenOrValue};
use lightningcss::values::length::LengthValue;

use crate::compute::Context;
use crate::style::ComputedValues;
use crate::values::{LengthPercentage, MaxSize, Size};

/// The calculations of `calc-size()` in a box's sizes, innermost first: `width`, `height`,
/// `min-width`, `min-height`, `max-width`, `max-height`.
pub type CalcSizes = [Option<Arc<[TokenList<'static>]>>; 6];

/// Whether tokens are a `calc-size()` alone.
pub fn is_calc_size(tokens: &TokenList<'_>) -> bool {
    matches!(
        tokens.0.iter().filter(|token| !token.is_whitespace()).collect::<Vec<_>>().as_slice(),
        [TokenOrValue::Function(function)] if function.name.0.eq_ignore_ascii_case("calc-size")
    )
}

/// The basis of a `calc-size()`.
enum Basis {
    Keyword(Size),
    /// A length or `any` (zero): the first calculation gives the size.
    Calculated,
}

/// The basis and the calculations of a `calc-size()`, innermost first.
fn parse(
    function: &Function<'static>,
    context: &Context,
    chain: &mut Vec<TokenList<'static>>,
) -> Option<Basis> {
    let arguments = &function.arguments.0;
    let comma = arguments
        .iter()
        .position(|token| matches!(token, TokenOrValue::Token(Token::Comma)))?;
    let basis: Vec<&TokenOrValue<'static>> = arguments[..comma]
        .iter()
        .filter(|token| !token.is_whitespace())
        .collect();
    let calculation = pixels(&TokenList(arguments[comma + 1..].to_vec()), context);
    let basis = match basis.as_slice() {
        [TokenOrValue::Token(Token::Ident(keyword))] => match keyword.to_ascii_lowercase().as_str() {
            "auto" => Basis::Keyword(Size::Auto),
            "min-content" => Basis::Keyword(Size::MinContent),
            "max-content" => Basis::Keyword(Size::MaxContent),
            "fit-content" => Basis::Keyword(Size::FitContent(None)),
            "stretch" | "-webkit-fill-available" => Basis::Keyword(Size::Stretch),
            "any" => Basis::Calculated,
            _ => return None,
        },
        [TokenOrValue::Function(inner)] if inner.name.0.eq_ignore_ascii_case("calc-size") => {
            parse(inner, context, chain)?
        }
        tokens => {
            chain.push(pixels(
                &TokenList(tokens.iter().map(|token| (*token).clone()).collect()),
                context,
            ));
            Basis::Calculated
        }
    };
    chain.push(calculation);
    Some(basis)
}

/// Tokens with their lengths in pixels (zoomed, as computed values are).
fn pixels(tokens: &TokenList<'static>, context: &Context) -> TokenList<'static> {
    TokenList(
        tokens
            .0
            .iter()
            .map(|token| match token {
                TokenOrValue::Length(length) => {
                    TokenOrValue::Length(LengthValue::Px(context.length_value(length)))
                }
                TokenOrValue::Function(function) => TokenOrValue::Function(Function {
                    name: function.name.clone(),
                    arguments: pixels(&function.arguments, context),
                }),
                other => other.clone(),
            })
            .collect(),
    )
}

/// Applies a sizing property of `calc-size()`: its basis as the property's value, its
/// calculations kept for layout; `false` for others.
pub(crate) fn apply(
    id: &PropertyId<'_>,
    tokens: &TokenList<'static>,
    style: &mut ComputedValues,
    context: &Context,
) -> bool {
    let index = match id {
        PropertyId::Width => 0,
        PropertyId::Height => 1,
        PropertyId::MinWidth => 2,
        PropertyId::MinHeight => 3,
        PropertyId::MaxWidth => 4,
        PropertyId::MaxHeight => 5,
        _ => return false,
    };
    let Some(TokenOrValue::Function(function)) = tokens.0.iter().find(|token| !token.is_whitespace()) else {
        return false;
    };
    let mut chain = Vec::new();
    let Some(basis) = parse(function, context, &mut chain) else {
        return false;
    };
    let size = match basis {
        Basis::Keyword(size) => size,
        // The first calculation gives the size from nothing.
        Basis::Calculated => Size::Length(LengthPercentage::Px(0.0)),
    };
    match index {
        0 => style.width = size,
        1 => style.height = size,
        2 => style.min_width = size,
        3 => style.min_height = size,
        _ => {
            let max = match size {
                Size::Length(length) => MaxSize::Length(length),
                Size::MinContent => MaxSize::MinContent,
                Size::MaxContent => MaxSize::MaxContent,
                Size::FitContent(limit) => MaxSize::FitContent(limit),
                Size::Stretch => MaxSize::Stretch,
                Size::Auto => MaxSize::None,
            };
            if index == 4 {
                style.max_width = max;
            } else {
                style.max_height = max;
            }
        }
    }
    let sizes = Arc::make_mut(style.calc_size.get_or_insert_with(Default::default));
    sizes[index] = Some(Arc::from(chain));
    true
}

/// The size that the calculations of a `calc-size()` make of the size `size` its basis gives,
/// with percentages of `basis`; `None` where they are invalid.
pub fn evaluate(chain: &[TokenList<'static>], size: f32, basis: f32) -> Option<f32> {
    let mut size = size;
    for calculation in chain {
        let tokens = substitute(calculation, size, basis);
        let quantity = crate::math::evaluate(&tokens.0, None)?;
        if quantity.powers != [1, 0, 0, 0] && !(quantity.powers == [0; 4] && quantity.value == 0.0) {
            return None;
        }
        #[expect(clippy::cast_possible_truncation, reason = "lengths are single precision")]
        {
            size = quantity.value as f32;
        }
    }
    Some(size.max(0.0))
}

/// Tokens with `size` and percentages as pixels.
fn substitute(tokens: &TokenList<'static>, size: f32, basis: f32) -> TokenList<'static> {
    TokenList(
        tokens
            .0
            .iter()
            .map(|token| match token {
                TokenOrValue::Token(Token::Ident(ident)) if ident.eq_ignore_ascii_case("size") => {
                    TokenOrValue::Length(LengthValue::Px(size))
                }
                TokenOrValue::Token(Token::Percentage { unit_value, .. }) => {
                    TokenOrValue::Length(LengthValue::Px(unit_value * basis))
                }
                TokenOrValue::Function(function) => TokenOrValue::Function(Function {
                    name: function.name.clone(),
                    arguments: substitute(&function.arguments, size, basis),
                }),
                other => other.clone(),
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(css: &str) -> TokenList<'static> {
        crate::variables::value_tokens(css).unwrap_or_else(|| TokenList(Vec::new()))
    }

    #[test]
    fn calculates_from_the_basis_size() {
        assert_eq!(evaluate(&[tokens("size * 0.5")], 400.0, 400.0), Some(200.0));
        assert_eq!(
            evaluate(&[tokens("round(up, size, 50px)")], 72.0, 400.0),
            Some(100.0)
        );
        // Nested: the inner calculation first.
        assert_eq!(
            evaluate(&[tokens("size + 10px"), tokens("size * 2")], 49.0, 400.0),
            Some(118.0)
        );
        // A percentage basis, as the first calculation, then `size` of it.
        assert_eq!(
            evaluate(&[tokens("50%"), tokens("size + 10px")], 0.0, 400.0),
            Some(210.0)
        );
        // Negative sizes are none.
        assert_eq!(evaluate(&[tokens("size - 100px")], 40.0, 400.0), Some(0.0));
        assert!(is_calc_size(&tokens("calc-size(auto, size)")));
        assert!(!is_calc_size(&tokens("calc(1px + 2px)")));
    }
}

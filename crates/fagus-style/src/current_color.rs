//! `background-color` values that depend on `currentcolor` (`currentcolor` itself, or a
//! `color-mix()` or relative color of it). Their computed value keeps `currentcolor`, so an
//! element that inherits one resolves it with its own `color`, as Chrome does (CSS Color 4,
//! §6.4; CSS Color 5, §4.1).

use std::sync::Arc;

use lightningcss::properties::custom::{Token, TokenList, TokenOrValue};
use lightningcss::values::color::CssColor;

use crate::values::Rgba;

/// Whether tokens refer to `currentcolor`, inside functions too.
pub fn refers_to_current(tokens: &[TokenOrValue<'_>]) -> bool {
    tokens.iter().any(|token| match token {
        TokenOrValue::Color(CssColor::CurrentColor) => true,
        TokenOrValue::Token(Token::Ident(ident)) => ident.eq_ignore_ascii_case("currentcolor"),
        TokenOrValue::Function(function) => refers_to_current(&function.arguments.0),
        _ => false,
    })
}

/// The color of a `background` shorthand (the last token of its last layer that is a color),
/// or of a `background-color` (all its tokens), as tokens, when it refers to `currentcolor`.
pub fn background_color(tokens: TokenList<'static>, shorthand: bool) -> Option<Arc<TokenList<'static>>> {
    if !refers_to_current(&tokens.0) {
        return None;
    }
    if !shorthand {
        return Some(Arc::new(tokens));
    }
    let last_layer = tokens
        .0
        .rsplit(|token| matches!(token, TokenOrValue::Token(Token::Comma)))
        .next()?;
    let color = last_layer.iter().rev().find(|token| match token {
        TokenOrValue::Color(_) => true,
        TokenOrValue::Token(Token::Ident(ident)) => ident.eq_ignore_ascii_case("currentcolor"),
        TokenOrValue::Function(function) => fagus_css::is_unparsed_color(function),
        _ => false,
    })?;
    refers_to_current(std::slice::from_ref(color)).then(|| Arc::new(TokenList(vec![color.clone()])))
}

/// The color a value kept by [`background_color`] comes to with the `currentcolor` that
/// [`crate::compute::set_current_color`] set; `None` where it is invalid.
pub fn resolve(tokens: &TokenList<'static>) -> Option<Rgba> {
    crate::compute::color(&crate::color_functions::token_color(&tokens.0)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(css: &str) -> TokenList<'static> {
        crate::variables::value_tokens(css).expect("tokens")
    }

    #[test]
    fn keeps_colors_that_refer_to_currentcolor() {
        assert!(background_color(tokens("rgb(from currentColor r g b)"), false).is_some());
        assert!(background_color(tokens("color-mix(in srgb, red, green)"), false).is_none());
        let kept = background_color(
            tokens("url(a.png), no-repeat color-mix(in srgb, currentcolor, red)"),
            true,
        )
        .expect("the color of the last layer");
        assert_eq!(kept.0.len(), 1);
    }

    #[test]
    fn resolves_with_the_current_color() {
        crate::compute::set_current_color(Some(Rgba::from_u8(0, 128, 0, 1.0)));
        let kept = background_color(tokens("rgb(from currentColor r g b)"), false).expect("kept");
        assert_eq!(resolve(&kept), Some(Rgba::from_u8(0, 128, 0, 1.0)));
        crate::compute::set_current_color(Some(Rgba::from_u8(0, 0, 255, 1.0)));
        assert_eq!(resolve(&kept), Some(Rgba::from_u8(0, 0, 255, 1.0)));
        crate::compute::set_current_color(None);
        assert_eq!(resolve(&kept), None);
    }
}

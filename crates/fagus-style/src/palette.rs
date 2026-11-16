//! `font-palette` values that lightningcss keeps as tokens: the keywords and `palette-mix()`.

use std::sync::Arc;

use lightningcss::properties::custom::{Token, TokenOrValue};

use crate::values::FontPalette;

/// Parses `normal`, `light`, `dark`, a dashed identifier or `palette-mix(in <space>, <palette>
/// [<percentage>], <palette> [<percentage>])`; `None` where the value is not valid.
/// `inherited` is the palette of the parent, which `inherit` takes.
pub(crate) fn font_palette(tokens: &[TokenOrValue<'_>], inherited: &FontPalette) -> Option<FontPalette> {
    let significant: Vec<&TokenOrValue<'_>> = tokens.iter().filter(|token| !token.is_whitespace()).collect();
    let [token] = significant.as_slice() else {
        return None;
    };
    match token {
        TokenOrValue::Token(Token::Ident(ident)) => match ident.to_ascii_lowercase().as_str() {
            "normal" => Some(FontPalette::Normal),
            "light" => Some(FontPalette::Light),
            "dark" => Some(FontPalette::Dark),
            "inherit" => Some(inherited.clone()),
            name if name.starts_with("--") => Some(FontPalette::Named(ident.as_ref().into())),
            _ => None,
        },
        TokenOrValue::DashedIdent(ident) => Some(FontPalette::Named(ident.0.as_ref().into())),
        TokenOrValue::Function(function) if function.name.0.eq_ignore_ascii_case("palette-mix") => {
            palette_mix(&function.arguments.0, inherited)
        }
        _ => None,
    }
}

/// The arguments of `palette-mix()`: a color space, then two palettes with optional
/// percentages, mixed as `color-mix()` mixes (CSS Fonts 4, §9.2).
fn palette_mix(arguments: &[TokenOrValue<'_>], inherited: &FontPalette) -> Option<FontPalette> {
    let mut parts = arguments.split(|token| matches!(token, TokenOrValue::Token(Token::Comma)));
    let space = parts.next()?;
    let space: Vec<&TokenOrValue<'_>> = space.iter().filter(|token| !token.is_whitespace()).collect();
    if !matches!(space.first(), Some(TokenOrValue::Token(Token::Ident(word))) if word.eq_ignore_ascii_case("in"))
    {
        return None;
    }
    let side = |part: &[TokenOrValue<'_>]| -> Option<(FontPalette, Option<f32>)> {
        let mut percent = None;
        let mut rest = Vec::new();
        for token in part {
            match token {
                TokenOrValue::Token(Token::Percentage { unit_value, .. }) if percent.is_none() => {
                    percent = Some(*unit_value);
                }
                other => rest.push(other.clone()),
            }
        }
        Some((font_palette(&rest, inherited)?, percent))
    };
    let (first, first_share) = side(parts.next()?)?;
    let (second, second_share) = side(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    let (first_share, second_share) = match (first_share, second_share) {
        (None, None) => (0.5, 0.5),
        (Some(share), None) => (share, 1.0 - share),
        (None, Some(share)) => (1.0 - share, share),
        (Some(a), Some(b)) => (a, b),
    };
    let total = first_share + second_share;
    if !(0.0..=1.0).contains(&first_share) || !(0.0..=1.0).contains(&second_share) || total <= 0.0 {
        return None;
    }
    Some(FontPalette::Mix(Arc::new((first, second, second_share / total))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Option<FontPalette> {
        let tokens = crate::variables::value_tokens(text)?;
        font_palette(&tokens.0, &FontPalette::Light)
    }

    #[test]
    fn parses_font_palettes() {
        assert_eq!(parse("dark"), Some(FontPalette::Dark));
        assert_eq!(parse("--Mine"), Some(FontPalette::Named("--Mine".into())));
        assert_eq!(parse("inherit"), Some(FontPalette::Light));
        assert_eq!(parse("--a --b"), None);
        assert_eq!(
            parse("palette-mix(in srgb, --a 25%, dark)"),
            Some(FontPalette::Mix(Arc::new((
                FontPalette::Named("--a".into()),
                FontPalette::Dark,
                0.75
            ))))
        );
        assert_eq!(parse("palette-mix(--a, dark)"), None);
    }
}

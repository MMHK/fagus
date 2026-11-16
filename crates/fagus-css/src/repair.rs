//! Values that lightningcss rejects although browsers accept them, turned into the properties
//! they stand for, and values that it accepts although browsers reject them.

use lightningcss::declaration::DeclarationBlock;
use lightningcss::properties::custom::{CustomPropertyName, Token, TokenList, TokenOrValue};
use lightningcss::properties::{Property, PropertyId};
use lightningcss::stylesheet::{ParserOptions, PrinterOptions};
use lightningcss::traits::IntoOwned;
use smallvec::SmallVec;

/// Turns `box-shadow: none` and `text-shadow: none` (with any vendor prefix), which lightningcss
/// keeps as unparsed values because it parses these properties only as lists of shadows, into
/// empty lists of shadows, and the old names of the gap properties (`grid-gap`, `grid-row-gap`
/// and `grid-column-gap`, as in Pico CSS), which browsers still accept, into `gap`, `row-gap`
/// and `column-gap`, and gradients that lightningcss rejects (unitless zero angles in conic
/// gradients, interpolation methods; see [`crate::gradient`]) outside backgrounds, as in
/// `border-image: conic-gradient(green 0 0) 1`, into the property without them. Backgrounds
/// keep their tokens, which the style repairs keeping the interpolation methods. Other
/// properties stay as they are.
pub fn repair_property(property: &mut Property<'_>) {
    if let Property::Custom(custom) = property
        && let CustomPropertyName::Unknown(name) = &custom.name
    {
        if let Some(target) = alias(&name.0)
            && let Ok(css) = property.value_to_css_string(PrinterOptions::default())
            && let Some(Ok(parsed)) = crate::guarded(|| {
                Property::parse_string(PropertyId::from(target), &css, ParserOptions::default())
                    .map(IntoOwned::into_owned)
            })
        {
            *property = parsed;
        }
        return;
    }
    let Property::Unparsed(unparsed) = property else {
        return;
    };
    if !matches!(
        unparsed.property_id,
        PropertyId::Background | PropertyId::BackgroundImage
    ) && !crate::has_computed_values(&unparsed.value)
        && crate::gradient::needs_repair(&unparsed.value)
    {
        let (repaired, _) = crate::gradient::repair(&unparsed.value.clone().into_owned());
        let id = unparsed.property_id.clone();
        let css = Property::Custom(lightningcss::properties::custom::CustomProperty {
            name: CustomPropertyName::Unknown(lightningcss::values::ident::Ident("x".into())),
            value: repaired,
        })
        .value_to_css_string(PrinterOptions::default());
        if let Ok(css) = css
            && let Some(Ok(parsed)) = crate::guarded(|| {
                Property::parse_string(id.into_owned(), &css, ParserOptions::default())
                    .map(IntoOwned::into_owned)
            })
            && !matches!(parsed, Property::Unparsed(_))
        {
            *property = parsed;
        }
        return;
    }
    if unparsed.property_id == PropertyId::ColorScheme {
        if let Some(scheme) = color_scheme(&unparsed.value) {
            *property = Property::ColorScheme(scheme);
        }
        return;
    }
    // `grid-auto-flow: dense`, which lightningcss rejects without `row` or `column` after it:
    // `row dense` (CSS Grid 2, §7.7).
    if unparsed.property_id == PropertyId::GridAutoFlow {
        if is_dense(&unparsed.value) {
            use lightningcss::properties::grid::GridAutoFlow;
            *property = Property::GridAutoFlow(GridAutoFlow::Row | GridAutoFlow::Dense);
        }
        return;
    }
    if !is_none(&unparsed.value) {
        return;
    }
    match &unparsed.property_id {
        PropertyId::BoxShadow(prefix) => *property = Property::BoxShadow(SmallVec::new(), *prefix),
        PropertyId::TextShadow => *property = Property::TextShadow(SmallVec::new()),
        _ => {}
    }
}

/// Whether the value of a property, with its `var()` substituted (`tokens`), is negative where
/// the property takes no negative values: a padding, size, border width, gap, font size, line
/// height or radius, which lightningcss accepts but browsers reject. So `padding: var(--size-00)
/// var(--size-2)` with Open Props' `--size-00: -.5rem` sets no padding at all, as in Chrome.
/// Negative numbers inside functions (`calc()`, `round()`) are allowed: their results are
/// clamped.
pub fn has_forbidden_negative(property: &PropertyId<'_>, tokens: &TokenList<'_>) -> bool {
    forbids_negative(&property.name().to_ascii_lowercase())
        && tokens.0.iter().any(|token| match token {
            TokenOrValue::Token(
                Token::Number { value, .. }
                | Token::Dimension { value, .. }
                | Token::Percentage {
                    unit_value: value, ..
                },
            ) => *value < 0.0,
            TokenOrValue::Length(length) => length.to_unit_value().0 < 0.0,
            _ => false,
        })
}

/// Whether a property takes no negative lengths, percentages or numbers.
pub(crate) fn forbids_negative(name: &str) -> bool {
    name.starts_with("padding")
        || name.ends_with("width")
        || name.ends_with("height")
        || name.ends_with("-size")
        || name.ends_with("gap")
        || name.contains("radius")
        || matches!(name, "flex" | "flex-basis" | "flex-grow" | "flex-shrink")
        || name == "outline"
        || (name.starts_with("border") && !name.contains("color") && !name.contains("style"))
}

/// The property that an old name stands for.
fn alias(name: &str) -> Option<&'static str> {
    match name.to_ascii_lowercase().as_str() {
        "grid-gap" => Some("gap"),
        "grid-row-gap" => Some("row-gap"),
        "grid-column-gap" => Some("column-gap"),
        _ => None,
    }
}

/// Repairs every declaration of a block (see [`repair_property`]).
pub fn repair_block(block: &mut DeclarationBlock<'_>) {
    for property in block
        .declarations
        .iter_mut()
        .chain(block.important_declarations.iter_mut())
    {
        repair_property(property);
    }
}

/// A `color-scheme` that names schemes lightningcss does not know beside `light` and `dark`
/// (Infima's `color-scheme: auto`, `light foo`): browsers take them as custom identifiers of
/// schemes they do not support and ignore them (CSS Color Adjustment 1, §2.1). `normal` among
/// others, a repeated `only`, or `only` alone are invalid.
fn color_scheme(tokens: &TokenList<'_>) -> Option<lightningcss::properties::ui::ColorScheme> {
    use lightningcss::properties::ui::ColorScheme;
    let mut scheme = ColorScheme::empty();
    let mut names = 0;
    for token in &tokens.0 {
        match token {
            TokenOrValue::Token(Token::WhiteSpace(_)) => {}
            TokenOrValue::Token(Token::Ident(ident)) => match ident.to_ascii_lowercase().as_str() {
                "only" if !scheme.contains(ColorScheme::Only) => scheme |= ColorScheme::Only,
                "light" => (scheme, names) = (scheme | ColorScheme::Light, names + 1),
                "dark" => (scheme, names) = (scheme | ColorScheme::Dark, names + 1),
                "normal" | "only" | "default" | "initial" | "inherit" | "unset" | "revert"
                | "revert-layer" => {
                    return None;
                }
                _ => names += 1,
            },
            _ => return None,
        }
    }
    (names > 0).then_some(scheme)
}

/// Whether a value is the keyword `dense` alone, which `grid-auto-flow` takes as `row dense`.
pub fn is_dense(tokens: &TokenList<'_>) -> bool {
    let mut significant = tokens
        .0
        .iter()
        .filter(|token| !matches!(token, TokenOrValue::Token(Token::WhiteSpace(_))));
    matches!(significant.next(), Some(TokenOrValue::Token(Token::Ident(ident))) if ident.eq_ignore_ascii_case("dense"))
        && significant.next().is_none()
}

/// Whether a value is the keyword `none` alone.
fn is_none(tokens: &TokenList<'_>) -> bool {
    let mut significant = tokens
        .0
        .iter()
        .filter(|token| !matches!(token, TokenOrValue::Token(Token::WhiteSpace(_))));
    matches!(
        (significant.next(), significant.next()),
        (Some(TokenOrValue::Token(Token::Ident(ident))), None) if ident.eq_ignore_ascii_case("none")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightningcss::stylesheet::{ParserOptions, StyleAttribute};

    #[test]
    fn repairs_shadows_of_none() {
        let mut parsed = StyleAttribute::parse(
            "box-shadow: none; -webkit-box-shadow: NONE; text-shadow: none; box-shadow: none 1px",
            ParserOptions::default(),
        )
        .expect("parses");
        repair_block(&mut parsed.declarations);
        let declarations = &parsed.declarations.declarations;
        assert!(matches!(&declarations[0], Property::BoxShadow(list, _) if list.is_empty()));
        assert!(matches!(&declarations[1], Property::BoxShadow(list, prefix)
            if list.is_empty() && *prefix == lightningcss::vendor_prefix::VendorPrefix::WebKit));
        assert!(matches!(&declarations[2], Property::TextShadow(list) if list.is_empty()));
        assert!(
            matches!(&declarations[3], Property::Unparsed(_)),
            "not `none` alone"
        );
    }

    #[test]
    fn repairs_dense_auto_flow() {
        use lightningcss::properties::grid::GridAutoFlow;
        let mut parsed = StyleAttribute::parse(
            "grid-auto-flow: dense; grid-auto-flow: dense foo",
            ParserOptions::default(),
        )
        .expect("parses");
        repair_block(&mut parsed.declarations);
        let declarations = &parsed.declarations.declarations;
        assert!(matches!(&declarations[0], Property::GridAutoFlow(flow)
            if *flow == GridAutoFlow::Row | GridAutoFlow::Dense));
        assert!(
            matches!(&declarations[1], Property::Unparsed(_)),
            "not `dense` alone"
        );
    }

    #[test]
    fn repairs_color_schemes_with_unknown_names() {
        use lightningcss::properties::ui::ColorScheme;
        let mut parsed = StyleAttribute::parse(
            "color-scheme: auto; color-scheme: foo dark; color-scheme: only light bar; \
             color-scheme: normal foo; color-scheme: only foo only",
            ParserOptions::default(),
        )
        .expect("parses");
        repair_block(&mut parsed.declarations);
        let declarations = &parsed.declarations.declarations;
        assert!(matches!(&declarations[0], Property::ColorScheme(scheme) if scheme.is_empty()));
        assert!(matches!(&declarations[1], Property::ColorScheme(scheme) if *scheme == ColorScheme::Dark));
        assert!(matches!(&declarations[2], Property::ColorScheme(scheme)
            if *scheme == ColorScheme::Light | ColorScheme::Only));
        assert!(matches!(&declarations[3], Property::Unparsed(_)));
        assert!(matches!(&declarations[4], Property::Unparsed(_)));
    }

    #[test]
    fn repairs_gradients_outside_backgrounds() {
        let mut parsed = StyleAttribute::parse(
            "border-image: conic-gradient(green 0 0) 1 fill / 10px; \
             list-style-image: linear-gradient(in oklab, red, blue); \
             background-image: conic-gradient(red 0 0)",
            ParserOptions::default(),
        )
        .expect("parses");
        repair_block(&mut parsed.declarations);
        let declarations = &parsed.declarations.declarations;
        assert!(
            matches!(&declarations[0], Property::BorderImage(..)),
            "{:?}",
            declarations[0]
        );
        assert!(
            matches!(&declarations[1], Property::ListStyleImage(..)),
            "{:?}",
            declarations[1]
        );
        // Backgrounds keep their tokens for the style, which keeps interpolation methods.
        assert!(matches!(&declarations[2], Property::Unparsed(_)));
    }

    #[test]
    fn finds_negative_values_where_properties_take_none() {
        let negative = |name: &str, css: &str| {
            let id = PropertyId::from(name);
            let Property::Custom(custom) =
                Property::parse_string(PropertyId::from("--x"), css, ParserOptions::default())
                    .expect("parses")
            else {
                panic!("custom property")
            };
            has_forbidden_negative(&id, &custom.value)
        };
        assert!(negative("padding", "-.5rem .5rem"));
        assert!(negative("padding-inline-start", "-2px"));
        assert!(negative("width", "-8px"));
        assert!(negative("line-height", "-1"));
        assert!(negative("border-top", "-1px solid red"));
        assert!(negative("gap", "4px -10%"));
        assert!(!negative("margin", "-8px"));
        assert!(!negative("width", "calc(100% - 8px)"));
        assert!(!negative("width", "calc(-8px + 50%)"));
        assert!(!negative("padding", "-0px"));
        assert!(!negative("outline-offset", "-2px"));
        assert!(!negative("text-indent", "-1em"));
    }

    #[test]
    fn repairs_old_names_of_gaps() {
        let mut parsed = StyleAttribute::parse(
            "grid-gap: 1px 2px; grid-row-gap: var(--gap); GRID-COLUMN-GAP: 3px; grid-gaps: 1px",
            ParserOptions::default(),
        )
        .expect("parses");
        repair_block(&mut parsed.declarations);
        let ids: Vec<String> = parsed
            .declarations
            .declarations
            .iter()
            .map(|property| property.property_id().name().to_owned())
            .collect();
        assert_eq!(ids, ["gap", "row-gap", "column-gap", "grid-gaps"]);
        assert!(matches!(&parsed.declarations.declarations[0], Property::Gap(_)));
        assert!(
            matches!(&parsed.declarations.declarations[1], Property::Unparsed(_)),
            "var() stays"
        );
    }
}

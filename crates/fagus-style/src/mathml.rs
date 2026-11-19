//! The properties of `MathML` Core that lightningcss does not know: `math-style`, `math-shift`,
//! `math-depth`, and `font-size: math`, which scales the font size of scripts by the
//! inherited font's `MATH` table as Chrome does (CSS Fonts 4, §2.5; `MathML` Core, §3.1.3).

use lightningcss::properties::custom::{Token, TokenList, TokenOrValue};

use crate::style::ComputedValues;

/// The factor of each step of `math-depth` without a `MATH` table (`MathML` Core's default
/// `scriptPercentScaleDown`), and of two steps (`scriptScriptPercentScaleDown`).
const DEFAULT_SCALES: (f32, f32) = (0.71, 0.5041);

/// The tokens of a declaration without white space and comments.
fn significant<'t>(tokens: &'t TokenList<'static>) -> Vec<&'t TokenOrValue<'static>> {
    tokens
        .0
        .iter()
        .filter(|token| {
            !matches!(
                token,
                TokenOrValue::Token(Token::WhiteSpace(_) | Token::Comment(_))
            )
        })
        .collect()
}

/// The identifier a declaration consists of, in lower case.
pub(crate) fn keyword(tokens: &TokenList<'static>) -> Option<String> {
    match significant(tokens).as_slice() {
        [TokenOrValue::Token(Token::Ident(ident))] => Some(ident.to_ascii_lowercase()),
        _ => None,
    }
}

/// Whether a declaration of `font-size` is `math`.
pub(crate) fn is_math_font_size(tokens: &TokenList<'static>) -> bool {
    keyword(tokens).as_deref() == Some("math")
}

/// `math-style` or `math-shift`: whether the value is `compact` (`Some(true)`) or `normal`.
pub(crate) fn compact(tokens: &TokenList<'static>) -> Option<bool> {
    match keyword(tokens)?.as_str() {
        "compact" => Some(true),
        "normal" => Some(false),
        _ => None,
    }
}

/// The computed `math-depth` of a declaration: `auto-add` adds one to the inherited depth
/// where the inherited `math-style` is compact, `add(n)` adds `n`, and an integer is taken
/// as it is (CSS Fonts 4, §2.5.1).
pub(crate) fn depth(tokens: &TokenList<'static>, parent: &ComputedValues) -> Option<i32> {
    let tokens = significant(tokens);
    match tokens.as_slice() {
        [TokenOrValue::Token(Token::Ident(ident))] if ident.eq_ignore_ascii_case("auto-add") => {
            Some(parent.math_depth + i32::from(parent.math_compact))
        }
        [
            TokenOrValue::Token(Token::Number {
                int_value: Some(value),
                ..
            }),
        ] => Some(*value),
        [TokenOrValue::Token(Token::Function(name)), rest @ ..] if name.eq_ignore_ascii_case("add") => {
            let value = rest.iter().find_map(|token| match token {
                TokenOrValue::Token(Token::Number {
                    int_value: Some(value),
                    ..
                }) => Some(*value),
                _ => None,
            })?;
            Some(parent.math_depth.saturating_add(value))
        }
        [TokenOrValue::Function(function)] if function.name.eq_ignore_ascii_case("add") => {
            match significant(&function.arguments).as_slice() {
                [
                    TokenOrValue::Token(Token::Number {
                        int_value: Some(value),
                        ..
                    }),
                ] => Some(parent.math_depth.saturating_add(*value)),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The font size of `font-size: math` for an element of math depth `depth` whose parent has
/// font size `parent_size` and depth `parent_depth`, with the script scales of the parent's
/// first available font (`None` without a `MATH` table), as Blink's
/// `ComputeFontSizeFromMathDepth` computes it.
pub(crate) fn scaled_font_size(
    parent_size: f32,
    (parent_depth, depth): (i32, i32),
    scales: Option<(f32, f32)>,
) -> f32 {
    let (mut from, mut to) = (parent_depth, depth);
    if from == to {
        return parent_size;
    }
    let invert = from > to;
    if invert {
        std::mem::swap(&mut from, &mut to);
    }
    let mut steps = to - from;
    let mut scale = 1.0_f32;
    if let Some((script, script_script)) = scales {
        if from <= 0 && to >= 2 {
            scale *= script_script;
            steps -= 2;
        } else if from == 1 {
            scale *= script_script / script;
            steps -= 1;
        } else if to == 1 {
            scale *= script;
            steps -= 1;
        }
    }
    scale *= DEFAULT_SCALES.0.powi(steps);
    if invert {
        parent_size / scale
    } else {
        parent_size * scale
    }
}

/// The default scales, for tests.
#[cfg(test)]
pub(crate) const fn default_scales() -> (f32, f32) {
    DEFAULT_SCALES
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_font_size_by_math_depth() {
        let stix = Some((0.7, 0.55));
        assert!((scaled_font_size(20.0, (0, 1), stix) - 14.0).abs() < 1e-4);
        assert!((scaled_font_size(20.0, (0, 2), stix) - 11.0).abs() < 1e-4);
        assert!((scaled_font_size(14.0, (1, 2), stix) - 11.0).abs() < 1e-4);
        assert!((scaled_font_size(11.0, (2, 3), stix) - 11.0 * 0.71).abs() < 1e-4);
        assert!((scaled_font_size(14.0, (1, 0), stix) - 20.0).abs() < 1e-4);
        // Without a MATH table each step scales by 0.71.
        let (script, _) = default_scales();
        assert!((scaled_font_size(20.0, (0, 1), None) - 20.0 * script).abs() < 1e-4);
        assert!((scaled_font_size(20.0, (0, 2), None) - 20.0 * script * script).abs() < 1e-4);
    }
}

#[cfg(test)]
mod cascade_tests {
    use fagus_css::{MediaEnvironment, Origin, RuleSet, Stylesheet};
    use fagus_dom::{Document, NodeId};

    use crate::{StyledDocument, USER_AGENT_CSS, compute_styles};

    fn styles(html: &str) -> (Document, StyledDocument) {
        let document = Document::parse_html(html);
        let ua = Stylesheet::parse(USER_AGENT_CSS, Origin::UserAgent, "ua.css");
        assert!(ua.diagnostics().is_empty(), "{:#?}", ua.diagnostics());
        let environment = MediaEnvironment::default();
        let rules = RuleSet::new(&[&ua], &environment);
        let styled = compute_styles(&document, &rules, (environment.width, environment.height));
        (document, styled)
    }

    fn element(document: &Document, name: &str, nth: usize) -> NodeId {
        document
            .descendants(NodeId::DOCUMENT)
            .filter(|&node| {
                document
                    .element(node)
                    .is_some_and(|data| &*data.name.local == name)
            })
            .nth(nth)
            .expect("element")
    }

    #[test]
    fn styles_mathml_with_the_user_agent_sheet() {
        let (document, styled) = styles(
            "<p style='font-size: 20px'><math><msup><mi>x</mi><mn>2</mn></msup>\
             <mfrac><mn>1</mn><mn>2</mn></mfrac></math>\
             <math display=block><mi mathcolor=red>y</mi></math>",
        );
        let style = |name: &str, nth: usize| styled.style(element(&document, name, nth)).expect("style");
        let math = style("math", 0);
        assert_eq!(&*math.font_family, &["math".to_owned()]);
        assert!(math.math_compact);
        assert!(!style("math", 1).math_compact);
        assert_eq!(style("mi", 0).math_depth, 0);
        let script = style("mn", 0);
        assert_eq!(script.math_depth, 1);
        assert!(script.math_compact);
        assert!(
            (script.font_size - 20.0 * 0.71).abs() < 1e-3,
            "{}",
            script.font_size
        );
        // In an inline fraction the numerator is one level deeper, in a block one not.
        assert_eq!(style("mn", 1).math_depth, 1);
        assert!(style("mn", 2).math_shift_compact);
        assert!(style("mi", 1).color.r > 0.99 && style("mi", 1).color.g < 0.01);
    }
}

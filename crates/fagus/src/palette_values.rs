//! `@font-palette-values` rules (CSS Fonts 4, §9.2) read into the palettes of `fagus-text`.
//!
//! The descriptors are read from their serialized text, so that those lightningcss keeps as
//! tokens read as those it parses.

use fagus_css::lightningcss::rules::font_palette_values::FontPaletteValuesRule;
use fagus_css::lightningcss::stylesheet::PrinterOptions;
use fagus_css::lightningcss::traits::{Parse, ToCss};
use fagus_css::lightningcss::values::color::CssColor;
use fagus_text::{BasePalette, PaletteColor, PaletteValues};

/// The palette of a rule. A descriptor that is not valid is left out, the rest of the rule
/// holds.
pub(crate) fn read(rule: &FontPaletteValuesRule<'static>) -> PaletteValues {
    let mut values = PaletteValues {
        name: rule.name.0.as_ref().into(),
        families: Vec::new(),
        base: None,
        overrides: Vec::new(),
    };
    for property in &rule.properties {
        let Ok(text) = property.to_css_string(PrinterOptions::default()) else {
            continue;
        };
        let Some((name, value)) = text.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "font-family" => {
                if let Some(families) = families(value) {
                    values.families = families;
                }
            }
            "base-palette" => {
                if let Some(base) = base_palette(value) {
                    values.base = Some(base);
                }
            }
            "override-colors" => {
                if let Some(overrides) = override_colors(value) {
                    values.overrides = overrides;
                }
            }
            _ => {}
        }
    }
    values
}

/// Splits a value at its commas outside parentheses and strings.
fn split_commas(value: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut depth, mut quote, mut start) = (0_i32, None, 0);
    for (at, character) in value.char_indices() {
        match (quote, character) {
            (Some(open), _) if character == open => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(character),
            (None, '(') => depth += 1,
            (None, ')') => depth -= 1,
            (None, ',') if depth == 0 => {
                parts.push(value[start..at].trim());
                start = at + 1;
            }
            _ => {}
        }
    }
    parts.push(value[start..].trim());
    parts
}

/// A list of family names: strings, or identifiers joined by single spaces. Generic families
/// are not valid here.
fn families(value: &str) -> Option<Vec<String>> {
    split_commas(value)
        .into_iter()
        .map(|family| {
            let quoted = family
                .strip_prefix('"')
                .and_then(|rest| rest.strip_suffix('"'))
                .or_else(|| family.strip_prefix('\'').and_then(|rest| rest.strip_suffix('\'')));
            if let Some(name) = quoted {
                Some(name.replace("\\\"", "\""))
            } else {
                let words: Vec<&str> = family.split_whitespace().collect();
                let valid = !words.is_empty()
                    && words.iter().all(|word| {
                        word.chars().all(|character| {
                            character.is_alphanumeric()
                                || matches!(character, '-' | '_')
                                || !character.is_ascii()
                        }) && !word.starts_with(|character: char| character.is_ascii_digit())
                    });
                valid.then(|| words.join(" "))
            }
        })
        .collect()
}

fn base_palette(value: &str) -> Option<BasePalette> {
    match value.to_ascii_lowercase().as_str() {
        "light" => Some(BasePalette::Light),
        "dark" => Some(BasePalette::Dark),
        number => number.parse::<u16>().ok().map(BasePalette::Index),
    }
}

/// Pairs of an index and a color; the whole list is invalid where one is.
fn override_colors(value: &str) -> Option<Vec<(u32, PaletteColor)>> {
    split_commas(value)
        .into_iter()
        .map(|pair| {
            let (index, color) = pair.split_once(char::is_whitespace)?;
            let index = index.parse::<u32>().ok()?;
            Some((index, color_bytes(color.trim())?))
        })
        .collect()
}

/// An sRGB color as bytes; `currentcolor` and system colors are not valid in a palette.
fn color_bytes(text: &str) -> Option<PaletteColor> {
    let color = CssColor::parse_string(text).ok()?;
    if matches!(color, CssColor::CurrentColor | CssColor::System(_)) {
        return None;
    }
    let rgba = fagus_style::absolute_color(&color)?;
    #[expect(clippy::cast_possible_truncation, reason = "clamped to a byte")]
    let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    Some([byte(rgba.r), byte(rgba.g), byte(rgba.b), byte(rgba.a)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use fagus_css::{MediaEnvironment, Origin, RuleSet, Stylesheet};

    #[test]
    fn reads_font_palette_values_rules() {
        let sheet = Stylesheet::parse(
            r#"@font-palette-values --a {
                 font-family: "COLR test";
                 base-palette: 3;
                 override-colors: 9 #FF0000, 1 rgb(0 0 255);
               }
               @font-palette-values --b {
                 font-family: Colors;
                 base-palette: dark;
                 override-colors: 0 lch(from blue calc(0.5 * l) c h), 2 currentcolor;
               }"#,
            Origin::Author,
            "style",
        );
        let rules = RuleSet::new(&[&sheet], &MediaEnvironment::default());
        let palettes: Vec<PaletteValues> = rules.font_palette_values().iter().map(read).collect();
        assert_eq!(palettes.len(), 2);
        assert_eq!(&*palettes[0].name, "--a");
        assert_eq!(palettes[0].families, ["COLR test"]);
        assert_eq!(palettes[0].base, Some(BasePalette::Index(3)));
        assert_eq!(
            palettes[0].overrides,
            [(9, [255, 0, 0, 255]), (1, [0, 0, 255, 255])]
        );
        assert_eq!(palettes[1].base, Some(BasePalette::Dark));
        // A list with `currentcolor` is not valid.
        assert_eq!(palettes[1].overrides, []);
    }
}

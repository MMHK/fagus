//! What `@font-face` rules say: the descriptors of their faces and which of their sources Fagus
//! loads, as Chrome reads them.

use fagus_css::lightningcss::properties::custom::{Token, TokenOrValue};
use fagus_css::lightningcss::properties::font::{AbsoluteFontWeight, FontStretch, FontWeight};
use fagus_css::lightningcss::rules::font_face::{
    FontFaceProperty, FontFaceRule, FontFormat, FontStyle, FontTechnology, Source, UrlSource,
};
use fagus_css::lightningcss::stylesheet::PrinterOptions;
use fagus_css::lightningcss::traits::ToCss;
use fagus_css::lightningcss::values::percentage::Percentage;
use fagus_text::Descriptors;

/// The family, the descriptors and the sources of an `@font-face` rule; `None` for a rule
/// without a family.
pub(crate) fn read<'a>(rule: &'a FontFaceRule<'static>) -> Option<(Descriptors, Vec<&'a Source<'static>>)> {
    let mut family = None;
    let mut sources = Vec::new();
    let mut descriptors = Descriptors::default();
    for property in &rule.properties {
        match property {
            FontFaceProperty::FontFamily(name) => family = Some(unquoted(name)),
            FontFaceProperty::Source(list) => sources.extend(list.iter()),
            FontFaceProperty::FontWeight(range) => {
                descriptors.weight = Some(ordered(font_weight(&range.0), font_weight(&range.1)));
            }
            FontFaceProperty::UnicodeRange(ranges) => {
                descriptors.unicode_range = ranges.iter().map(|range| (range.start, range.end)).collect();
            }
            FontFaceProperty::FontStretch(range) => {
                descriptors.stretch = Some(ordered(font_stretch(&range.0), font_stretch(&range.1)));
            }
            FontFaceProperty::FontStyle(value) => {
                descriptors.style_auto = false;
                descriptors.slope = None;
                descriptors.style = match value {
                    FontStyle::Normal => fagus_text::FontStyle::Normal,
                    FontStyle::Italic => fagus_text::FontStyle::Italic,
                    FontStyle::Oblique(range) => {
                        let (low, high) = ordered(range.0.to_degrees(), range.1.to_degrees());
                        if low < high {
                            descriptors.slope = Some((low.max(-90.0), high.min(90.0)));
                        }
                        fagus_text::FontStyle::oblique(low)
                    }
                };
            }
            // `size-adjust` and the metric overrides, which lightningcss keeps as tokens.
            // `font-display` and the like do not matter for a document rendered once.
            FontFaceProperty::Custom(custom) => {
                let value = metric(&custom.value.0);
                let fraction = |current: Option<f32>| match value {
                    Some(Metric::Normal) => None,
                    Some(Metric::Fraction(fraction)) => Some(fraction),
                    None => current,
                };
                match custom.name.as_ref().to_ascii_lowercase().as_str() {
                    "size-adjust" => {
                        if let Some(Metric::Fraction(adjust)) = value {
                            descriptors.size_adjust = adjust;
                        }
                    }
                    "ascent-override" => {
                        descriptors.ascent_override = fraction(descriptors.ascent_override);
                    }
                    "descent-override" => {
                        descriptors.descent_override = fraction(descriptors.descent_override);
                    }
                    "line-gap-override" => {
                        descriptors.line_gap_override = fraction(descriptors.line_gap_override);
                    }
                    "font-feature-settings" => {
                        if let Some(features) = fagus_style::font_face_features(&custom.value) {
                            descriptors.features = features;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    descriptors.family = family?;
    Some((descriptors, sources))
}

/// The value of `size-adjust` or of a metric override.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Metric {
    /// `normal`: the font's own metric.
    Normal,
    /// A percentage (not negative), as a fraction.
    Fraction(f32),
}

/// A descriptor's value of a single percentage or `normal`; `None` for anything else, which
/// leaves the descriptor as it was.
fn metric(tokens: &[TokenOrValue<'_>]) -> Option<Metric> {
    let mut tokens = tokens.iter().filter(|token| !token.is_whitespace());
    match (tokens.next()?, tokens.next()) {
        (TokenOrValue::Token(Token::Percentage { unit_value, .. }), None) => Some(*unit_value)
            .filter(|value| value.is_finite() && *value >= 0.0)
            .map(Metric::Fraction),
        (TokenOrValue::Token(Token::Ident(ident)), None) if ident.eq_ignore_ascii_case("normal") => {
            Some(Metric::Normal)
        }
        _ => None,
    }
}

/// Whether Chrome loads a `url()` source: its `format()` is one it reads (`woff`, `woff2`,
/// `truetype`, `opentype`, `collection`, or one of them with `-variations`), its `tech()` lists
/// only technologies it supports, and without a format the URL does not end in `.eot`.
pub(crate) fn loads(source: &UrlSource<'_>) -> bool {
    let format = match &source.format {
        None => !source.url.url.to_ascii_lowercase().ends_with(".eot"),
        Some(
            FontFormat::WOFF
            | FontFormat::WOFF2
            | FontFormat::TrueType
            | FontFormat::OpenType
            | FontFormat::Collection,
        ) => true,
        Some(FontFormat::EmbeddedOpenType | FontFormat::SVG) => false,
        Some(FontFormat::String(format)) => matches!(
            format.to_ascii_lowercase().as_str(),
            "woff"
                | "woff2"
                | "truetype"
                | "opentype"
                | "collection"
                | "woff-variations"
                | "woff2-variations"
                | "truetype-variations"
                | "opentype-variations"
        ),
    };
    format
        && source.tech.iter().all(|tech| {
            !matches!(
                tech,
                FontTechnology::FeaturesGraphite | FontTechnology::ColorSVG | FontTechnology::Incremental
            )
        })
}

/// The name of a family or of a `local()` face, without quotes.
pub(crate) fn unquoted(name: &impl ToCss) -> String {
    name.to_css_string(PrinterOptions::default())
        .map(|name| {
            let name = name.trim();
            match name.strip_prefix('"').and_then(|name| name.strip_suffix('"')) {
                Some(quoted) => quoted.replace("\\\"", "\""),
                None => name
                    .strip_prefix('\'')
                    .and_then(|name| name.strip_suffix('\''))
                    .unwrap_or(name)
                    .to_owned(),
            }
        })
        .unwrap_or_default()
}

/// A range from two ends in either order (CSS Fonts 4, §4.4: reversed ends are swapped).
fn ordered(a: f32, b: f32) -> (f32, f32) {
    (a.min(b), a.max(b))
}

/// A width in percent of normal.
fn font_stretch(stretch: &FontStretch) -> f32 {
    let Percentage(value) = stretch.into();
    value * 100.0
}

fn font_weight(weight: &FontWeight) -> f32 {
    match weight {
        FontWeight::Absolute(AbsoluteFontWeight::Weight(weight)) => *weight,
        FontWeight::Absolute(AbsoluteFontWeight::Bold) => 700.0,
        FontWeight::Absolute(AbsoluteFontWeight::Normal) | FontWeight::Bolder | FontWeight::Lighter => 400.0,
    }
}

#[cfg(test)]
mod tests {
    use fagus_css::lightningcss::rules::font_face::Source;
    use fagus_css::{MediaEnvironment, Origin, RuleSet, Stylesheet};

    use super::{loads, read};

    fn rules(css: &str) -> RuleSet {
        let sheet = Stylesheet::parse(css, Origin::Author, "test");
        RuleSet::new(&[&sheet], &MediaEnvironment::default())
    }

    #[test]
    fn reads_size_adjust_and_metric_overrides() {
        let set = rules(
            "@font-face { font-family: 'Web Font'; src: url(a.woff2); size-adjust: 112.5%;
               ascent-override: 90%; descent-override: normal; line-gap-override: 0% }",
        );
        let (descriptors, sources) = read(&set.font_faces()[0].0).expect("a family");
        assert_eq!(descriptors.family, "Web Font");
        assert_eq!(sources.len(), 1);
        assert!((descriptors.size_adjust - 1.125).abs() < 1e-6);
        assert_eq!(descriptors.ascent_override, Some(0.9));
        assert_eq!(descriptors.descent_override, None);
        assert_eq!(descriptors.line_gap_override, Some(0.0));
        assert!(descriptors.style_auto);
    }

    #[test]
    fn reads_font_feature_settings() {
        let set = rules(
            "@font-face { font-family: A; src: url(a.ttf); font-feature-settings: 'liga' off, 'ss05', 'cv01' 3 }
             @font-face { font-family: B; src: url(b.ttf); font-feature-settings: 'toolong' }",
        );
        let (features, _) = read(&set.font_faces()[0].0).expect("a family");
        assert_eq!(features.features, [(*b"liga", 0), (*b"ss05", 1), (*b"cv01", 3)]);
        // An invalid value sets nothing.
        let (invalid, _) = read(&set.font_faces()[1].0).expect("a family");
        assert_eq!(invalid.features, Vec::<([u8; 4], u32)>::new());
    }

    #[test]
    fn reads_oblique_ranges() {
        let set = rules(
            "@font-face { font-family: A; src: url(a.ttf); font-style: oblique 15deg 0deg }
             @font-face { font-family: B; src: url(b.ttf); font-style: italic }",
        );
        let (oblique, _) = read(&set.font_faces()[0].0).expect("a family");
        assert_eq!(oblique.slope, Some((0.0, 15.0)));
        assert!(!oblique.style_auto);
        let (italic, _) = read(&set.font_faces()[1].0).expect("a family");
        assert_eq!(italic.style, fagus_text::FontStyle::Italic);
        assert_eq!(italic.slope, None);
    }

    #[test]
    fn skips_sources_of_formats_and_technologies_chrome_does_not_load() {
        let set = rules(
            "@font-face { font-family: A; src: url(a.eot), url(b.svg) format('svg'),
               url(c.ttf) format('x-unknown'), url(d.ttf) tech(incremental),
               url(e.woff2) format(woff2) tech(color-SVG), url(f.ttf) format('truetype-variations'),
               url(g.woff2) format('woff2') tech(variations, color-COLRv1), url(h.otf) format(opentype),
               url(i.otc) format(collection), local('Some Face') }",
        );
        let (_, sources) = read(&set.font_faces()[0].0).expect("a family");
        let loaded: Vec<bool> = sources
            .iter()
            .map(|source| match source {
                Source::Url(url) => loads(url),
                Source::Local(_) => true,
            })
            .collect();
        assert_eq!(
            loaded,
            [false, false, false, false, false, true, true, true, true, true]
        );
    }
}

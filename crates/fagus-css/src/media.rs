//! Evaluation of media queries for paged output, and of container queries for the sizes layout
//! finds.

use lightningcss::media_query::{
    MediaCondition, MediaFeatureComparison, MediaFeatureId, MediaFeatureName, MediaFeatureValue, MediaList,
    MediaQuery, MediaType, Operator, Qualifier, QueryFeature,
};
use lightningcss::rules::container::{ContainerCondition, ContainerSizeFeatureId, StyleQuery};
use lightningcss::values::length::{Length, LengthValue};
use lightningcss::values::resolution::Resolution;

/// The output medium that media queries are evaluated against.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MediaEnvironment {
    /// Width of the page area in CSS pixels.
    pub width: f32,
    /// Height of the page area in CSS pixels.
    pub height: f32,
}

impl Default for MediaEnvironment {
    /// An A4 page.
    fn default() -> Self {
        Self {
            width: 210.0 * 96.0 / 25.4,
            height: 297.0 * 96.0 / 25.4,
        }
    }
}

/// The initial font size that relative units in media queries refer to.
const MEDIA_FONT_SIZE: f32 = 16.0;

impl MediaEnvironment {
    /// Whether a media query list in CSS text (the prelude of `@media`) applies; one that does
    /// not parse does not.
    pub fn matches(&self, queries: &str) -> bool {
        self.parse_and_match(queries, false)
    }

    /// Whether a media query list in CSS text applies on a screen of this size: `screen`
    /// queries hold there, `print` ones do not. Chrome chooses the sources of `<picture>`
    /// elements so, as it loads a document, before it prints it.
    pub fn matches_on_screen(&self, queries: &str) -> bool {
        self.parse_and_match(queries, true)
    }

    fn parse_and_match(&self, queries: &str, screen: bool) -> bool {
        let queries = &*crate::flat_math(queries);
        crate::guarded(|| {
            let mut input = cssparser::ParserInput::new(queries);
            let mut parser = cssparser::Parser::new(&mut input);
            MediaList::parse(&mut parser, &lightningcss::stylesheet::ParserOptions::default()).is_ok_and(
                |list| {
                    parser.is_exhausted()
                        && (list.media_queries.is_empty()
                            || list
                                .media_queries
                                .iter()
                                .any(|query| self.matches_query(query, screen)))
                },
            )
        })
        .unwrap_or(false)
    }

    /// Whether a media list (e.g. of `@media`) applies. An empty list always applies.
    pub(crate) fn matches_list(&self, list: &MediaList<'_>) -> bool {
        list.media_queries.is_empty()
            || list
                .media_queries
                .iter()
                .any(|query| self.matches_query(query, false))
    }

    /// Whether a media list applies on a screen of this size and not in print: rules that lay a
    /// document out otherwise on the screen Chrome loads it on than on paper.
    pub(crate) fn matches_list_on_screen_only(&self, list: &MediaList<'_>) -> bool {
        !self.matches_list(list)
            && list
                .media_queries
                .iter()
                .any(|query| self.matches_query(query, true))
    }

    /// Whether a media condition holds, as `media()` in `if()` asks it (`width > 100px`,
    /// `(hover) and (color)`): a condition of features alone, without a media type.
    pub fn matches_condition(&self, css: &str) -> bool {
        self.condition_holds(css, false)
    }

    /// Whether a media condition holds on a screen of this size, as Chrome evaluates those of
    /// `sizes` attributes when it loads a document.
    pub fn matches_condition_on_screen(&self, css: &str) -> bool {
        self.condition_holds(css, true)
    }

    fn condition_holds(&self, css: &str, screen: bool) -> bool {
        use lightningcss::stylesheet::ParserOptions;
        use lightningcss::traits::ParseWithOptions;
        let css = &*crate::flat_math(css);
        let parse = |css: &str| {
            crate::guarded(|| {
                MediaCondition::parse_string_with_options(css, ParserOptions::default())
                    .ok()
                    .map(|condition| self.condition(&condition, screen))
            })
            .flatten()
        };
        parse(css).or_else(|| parse(&format!("({css})"))).unwrap_or(false)
    }

    fn matches_query(&self, query: &MediaQuery<'_>, screen: bool) -> bool {
        let type_matches = match query.media_type {
            MediaType::All => true,
            MediaType::Print => !screen,
            MediaType::Screen => screen,
            MediaType::Custom(_) => false,
        };
        let result = type_matches
            && query
                .condition
                .as_ref()
                .is_none_or(|condition| self.condition(condition, screen));
        if query.qualifier == Some(Qualifier::Not) {
            !result
        } else {
            result
        }
    }

    fn condition(&self, condition: &MediaCondition<'_>, screen: bool) -> bool {
        match condition {
            MediaCondition::Feature(feature) => self.feature(feature, screen),
            MediaCondition::Not(inner) => !self.condition(inner, screen),
            MediaCondition::Operation { operator, conditions } => match operator {
                Operator::And => conditions
                    .iter()
                    .all(|condition| self.condition(condition, screen)),
                Operator::Or => conditions
                    .iter()
                    .any(|condition| self.condition(condition, screen)),
            },
            MediaCondition::Unknown(_) => false,
        }
    }

    fn feature(&self, feature: &QueryFeature<'_, MediaFeatureId>, screen: bool) -> bool {
        match feature {
            QueryFeature::Plain { name, value } => {
                self.compare(name, MediaFeatureComparison::Equal, value, screen)
            }
            QueryFeature::Boolean { name } => match self.value(name, screen) {
                Some(Value::Number(number)) => number != 0.0,
                Some(Value::Ident(ident)) => ident != "none",
                None => false,
            },
            QueryFeature::Range {
                name,
                operator,
                value,
            } => self.compare(name, *operator, value, screen),
            QueryFeature::Interval {
                name,
                start,
                start_operator,
                end,
                end_operator,
            } => {
                // `start < name < end`: the start value is on the left, so the operator is reversed.
                self.compare(name, reverse(*start_operator), start, screen)
                    && self.compare(name, *end_operator, end, screen)
            }
        }
    }

    fn compare(
        &self,
        name: &MediaFeatureName<'_, MediaFeatureId>,
        operator: MediaFeatureComparison,
        expected: &MediaFeatureValue<'_>,
        screen: bool,
    ) -> bool {
        let Some(actual) = self.value(name, screen) else {
            return false;
        };
        compare_values(actual, operator, expected_value(expected))
    }

    /// The value of a media feature as Chrome 157 answers it when it prints (measured): for a
    /// desktop with a mouse, a light color scheme and no preferences.
    fn value(&self, name: &MediaFeatureName<'_, MediaFeatureId>, screen: bool) -> Option<Value> {
        let id = match name {
            MediaFeatureName::Standard(id) => id,
            // A laptop's screen is not folded.
            MediaFeatureName::Unknown(ident) if ident.0.eq_ignore_ascii_case("device-posture") => {
                return Some(Value::Ident("continuous".into()));
            }
            _ => return None,
        };
        Some(match id {
            MediaFeatureId::Width | MediaFeatureId::DeviceWidth => Value::Number(self.width),
            MediaFeatureId::Height | MediaFeatureId::DeviceHeight => Value::Number(self.height),
            MediaFeatureId::AspectRatio | MediaFeatureId::DeviceAspectRatio => {
                Value::Number(self.width / self.height)
            }
            MediaFeatureId::Orientation => Value::Ident(
                if self.height >= self.width {
                    "portrait"
                } else {
                    "landscape"
                }
                .into(),
            ),
            // Chrome prints at 300 dpi, 3.125 device pixels per CSS pixel; its screen has one.
            MediaFeatureId::Resolution => Value::Number(if screen { 96.0 } else { 300.0 }),
            MediaFeatureId::WebKitDevicePixelRatio | MediaFeatureId::MozDevicePixelRatio => {
                Value::Number(if screen { 1.0 } else { 300.0 / 96.0 })
            }
            MediaFeatureId::Color => Value::Number(8.0),
            MediaFeatureId::ColorIndex | MediaFeatureId::Monochrome | MediaFeatureId::Grid => {
                Value::Number(0.0)
            }
            MediaFeatureId::ColorGamut => Value::Ident("srgb".into()),
            MediaFeatureId::DynamicRange => Value::Ident("standard".into()),
            MediaFeatureId::OverflowBlock => Value::Ident("paged".into()),
            MediaFeatureId::DisplayMode => Value::Ident("browser".into()),
            MediaFeatureId::PrefersColorScheme => Value::Ident("light".into()),
            MediaFeatureId::PrefersReducedMotion
            | MediaFeatureId::PrefersContrast
            | MediaFeatureId::PrefersReducedTransparency => Value::Ident("no-preference".into()),
            MediaFeatureId::Hover | MediaFeatureId::AnyHover => Value::Ident("hover".into()),
            MediaFeatureId::Pointer | MediaFeatureId::AnyPointer => Value::Ident("fine".into()),
            // Fagus runs no scripts: `<noscript>` shows, as `scripting: none` says.
            MediaFeatureId::Scripting
            | MediaFeatureId::Update
            | MediaFeatureId::OverflowInline
            | MediaFeatureId::ForcedColors => Value::Ident("none".into()),
            // Features Chrome does not know: `video-dynamic-range`, `prefers-reduced-data`,
            // `inverted-colors`, `scan`.
            _ => return None,
        })
    }
}

enum Value {
    Number(f32),
    Ident(String),
}

/// The size of a query container as layout found it: its content box, the height only for a
/// container of `container-type: size`, and the font size its relative lengths refer to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContainerSize {
    pub width: f32,
    pub height: Option<f32>,
    pub font_size: f32,
}

/// Whether a container answers the condition of an `@container` rule: its size features by
/// the `size` layout found (a feature of a size the container does not know, its height for one
/// of `inline-size`, does not hold), its style queries by `style`. Scroll-state queries do not
/// hold in print.
pub fn container_matches(
    query: &crate::ruleset::ContainerQuery,
    size: Option<ContainerSize>,
    style: &dyn Fn(&StyleQuery<'static>) -> bool,
) -> bool {
    query
        .condition
        .as_ref()
        .is_none_or(|condition| container_condition(condition, size, style))
}

/// Whether a container condition asks for the size of its container: only size containers
/// (`container-type`) answer it.
pub fn asks_size(condition: &ContainerCondition<'_>) -> bool {
    match condition {
        ContainerCondition::Feature(_) => true,
        ContainerCondition::Not(inner) => asks_size(inner),
        ContainerCondition::Operation { conditions, .. } => conditions.iter().any(asks_size),
        ContainerCondition::Style(_)
        | ContainerCondition::ScrollState(_)
        | ContainerCondition::Unknown(_) => false,
    }
}

fn container_condition(
    condition: &ContainerCondition<'static>,
    size: Option<ContainerSize>,
    style: &dyn Fn(&StyleQuery<'static>) -> bool,
) -> bool {
    match condition {
        ContainerCondition::Feature(feature) => size.is_some_and(|size| container_feature(feature, size)),
        ContainerCondition::Not(inner) => !container_condition(inner, size, style),
        ContainerCondition::Operation { operator, conditions } => match operator {
            Operator::And => conditions
                .iter()
                .all(|condition| container_condition(condition, size, style)),
            Operator::Or => conditions
                .iter()
                .any(|condition| container_condition(condition, size, style)),
        },
        ContainerCondition::Style(query) => style(query),
        ContainerCondition::ScrollState(_) | ContainerCondition::Unknown(_) => false,
    }
}

fn container_feature(feature: &QueryFeature<'_, ContainerSizeFeatureId>, size: ContainerSize) -> bool {
    let value = |name: &MediaFeatureName<'_, ContainerSizeFeatureId>| -> Option<Value> {
        let MediaFeatureName::Standard(id) = name else {
            return None;
        };
        Some(match id {
            ContainerSizeFeatureId::Width | ContainerSizeFeatureId::InlineSize => Value::Number(size.width),
            ContainerSizeFeatureId::Height | ContainerSizeFeatureId::BlockSize => Value::Number(size.height?),
            ContainerSizeFeatureId::AspectRatio => Value::Number(size.width / size.height?),
            ContainerSizeFeatureId::Orientation => Value::Ident(
                if size.height? >= size.width {
                    "portrait"
                } else {
                    "landscape"
                }
                .into(),
            ),
        })
    };
    let compare = |name, operator, expected: &MediaFeatureValue<'_>| {
        let Some(actual) = value(name) else {
            return false;
        };
        let expected = match expected {
            MediaFeatureValue::Length(length) => {
                relative_length_px(length, size.font_size).map(Value::Number)
            }
            other => expected_value(other),
        };
        compare_values(actual, operator, expected)
    };
    match feature {
        QueryFeature::Plain { name, value } => compare(name, MediaFeatureComparison::Equal, value),
        QueryFeature::Boolean { name } => match value(name) {
            Some(Value::Number(number)) => number != 0.0,
            Some(Value::Ident(ident)) => ident != "none",
            None => false,
        },
        QueryFeature::Range {
            name,
            operator,
            value,
        } => compare(name, *operator, value),
        QueryFeature::Interval {
            name,
            start,
            start_operator,
            end,
            end_operator,
        } => compare(name, reverse(*start_operator), start) && compare(name, *end_operator, end),
    }
}

/// A length of a container query in pixels, its relative units relative to the container's font.
fn relative_length_px(length: &Length, font_size: f32) -> Option<f32> {
    match length {
        Length::Value(value) => value.to_px().or(match value {
            LengthValue::Em(em) => Some(em * font_size),
            LengthValue::Rem(rem) => Some(rem * MEDIA_FONT_SIZE),
            LengthValue::Ex(ex) | LengthValue::Ch(ex) => Some(ex * font_size / 2.0),
            _ => None,
        }),
        Length::Calc(_) => None,
    }
}

/// `actual` compared with `expected` by `operator`, as media features are.
fn compare_values(actual: Value, operator: MediaFeatureComparison, expected: Option<Value>) -> bool {
    match (actual, expected) {
        (Value::Number(actual), Some(Value::Number(expected))) => match operator {
            MediaFeatureComparison::Equal => (actual - expected).abs() < 0.01,
            MediaFeatureComparison::GreaterThan => actual > expected,
            MediaFeatureComparison::GreaterThanEqual => actual >= expected - 0.01,
            MediaFeatureComparison::LessThan => actual < expected,
            MediaFeatureComparison::LessThanEqual => actual <= expected + 0.01,
        },
        (Value::Ident(actual), Some(Value::Ident(expected))) => {
            operator == MediaFeatureComparison::Equal && actual.eq_ignore_ascii_case(&expected)
        }
        _ => false,
    }
}

fn expected_value(value: &MediaFeatureValue<'_>) -> Option<Value> {
    Some(match value {
        MediaFeatureValue::Length(length) => Value::Number(length_px(length)?),
        MediaFeatureValue::Number(number) => Value::Number(*number),
        MediaFeatureValue::Integer(integer) => Value::Number(*integer as f32),
        MediaFeatureValue::Boolean(boolean) => Value::Number(if *boolean { 1.0 } else { 0.0 }),
        MediaFeatureValue::Resolution(resolution) => Value::Number(match resolution {
            Resolution::Dpi(dpi) => *dpi,
            Resolution::Dpcm(dpcm) => dpcm * 2.54,
            Resolution::Dppx(dppx) => dppx * 96.0,
        }),
        MediaFeatureValue::Ratio(ratio) => Value::Number(ratio.0 / ratio.1),
        MediaFeatureValue::Ident(ident) => Value::Ident(ident.0.to_string()),
        MediaFeatureValue::Env(_) => return None,
    })
}

fn length_px(length: &Length) -> Option<f32> {
    match length {
        Length::Value(value) => value.to_px().or(match value {
            LengthValue::Em(em) | LengthValue::Rem(em) => Some(em * MEDIA_FONT_SIZE),
            LengthValue::Ex(ex) | LengthValue::Ch(ex) => Some(ex * MEDIA_FONT_SIZE / 2.0),
            _ => None,
        }),
        Length::Calc(_) => None,
    }
}

fn reverse(operator: MediaFeatureComparison) -> MediaFeatureComparison {
    match operator {
        MediaFeatureComparison::Equal => MediaFeatureComparison::Equal,
        MediaFeatureComparison::GreaterThan => MediaFeatureComparison::LessThan,
        MediaFeatureComparison::GreaterThanEqual => MediaFeatureComparison::LessThanEqual,
        MediaFeatureComparison::LessThan => MediaFeatureComparison::GreaterThan,
        MediaFeatureComparison::LessThanEqual => MediaFeatureComparison::GreaterThanEqual,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Origin, Stylesheet};
    use lightningcss::rules::CssRule;

    fn matches(query: &str) -> bool {
        let sheet = Stylesheet::parse(&format!("@media {query} {{}}"), Origin::Author, "test");
        let CssRule::Media(rule) = &sheet.sheet.rules.0[0] else {
            panic!("not a media rule: {query}")
        };
        MediaEnvironment::default().matches_list(&rule.query)
    }

    #[test]
    fn answers_container_queries() {
        let answers = |query: &str, width: f32, height: Option<f32>| {
            let sheet = Stylesheet::parse(&format!("@container {query} {{}}"), Origin::Author, "test");
            let CssRule::Container(rule) = &sheet.sheet.rules.0[0] else {
                panic!("not a container rule: {query}")
            };
            let query = crate::ContainerQuery {
                name: rule.name.as_ref().map(|name| name.0.0.to_string()),
                condition: rule.condition.clone(),
            };
            container_matches(
                &query,
                Some(ContainerSize {
                    width,
                    height,
                    font_size: 20.0,
                }),
                &|_| false,
            )
        };
        assert!(answers("(min-width: 400px)", 500.0, None));
        assert!(!answers("(min-width: 400px)", 300.0, None));
        assert!(answers("(300px <= width < 400px)", 350.0, None));
        assert!(answers("card (inline-size > 10em)", 201.0, None));
        assert!(!answers("(inline-size > 10em)", 199.0, None));
        // A container of its inline size alone cannot tell its height.
        assert!(!answers("(min-height: 10px)", 300.0, None));
        assert!(answers(
            "(orientation: landscape) and (min-height: 10px)",
            300.0,
            Some(100.0)
        ));
        assert!(answers("not (max-width: 100px)", 300.0, None));
        assert!(!answers("style(--card: wide)", 300.0, None));
    }

    #[test]
    fn answers_the_tests_of_if() {
        let environment = MediaEnvironment::default();
        assert!(environment.matches_condition("width > 100px"));
        assert!(environment.matches_condition("(hover) and (color)"));
        assert!(!environment.matches_condition("(max-width: 100px)"));
        assert!(crate::supports("display: grid"));
        assert!(crate::supports("(display: grid) and (color: red)"));
        assert!(!crate::supports("display: nonsense"));
        // Values with functions resolved for each element are supported, as in Chrome.
        assert!(crate::supports("width: attr(data-width type(<length>))"));
        assert!(crate::supports("width: calc(sibling-index() * 1px)"));
    }

    #[test]
    fn evaluates_print_media() {
        assert!(matches("print"));
        assert!(matches("all"));
        assert!(!matches("screen"));
        assert!(matches("not screen"));
        assert!(matches("(min-width: 500px)"));
        assert!(!matches("(min-width: 1000px)"));
        assert!(matches("(width > 40rem)"));
        assert!(matches("(400px < width < 1000px)"));
        assert!(matches("(orientation: portrait)"));
        assert!(matches("(hover: hover)"));
        assert!(matches("(pointer: fine)"));
        assert!(!matches("(prefers-reduced-motion: reduce)"));
        assert!(!matches("(video-dynamic-range: standard)"));
        assert!(matches("(device-posture: continuous)"));
        assert!(matches("(prefers-color-scheme: light)"));
        assert!(!matches("(prefers-color-scheme: dark)"));
        assert!(matches("print and (max-width: 210mm)"));
    }

    #[test]
    fn prints_at_300_dpi_on_a_screen_of_96() {
        // Chrome 157 prints at 300 dpi (3.125 dppx), measured with `@media` in print; the
        // window it loads the document in has one device pixel per CSS pixel.
        assert!(matches("(resolution: 300dpi)"));
        assert!(matches("(min-resolution: 2dppx)"));
        assert!(!matches("(min-resolution: 301dpi)"));
        assert!(matches("(-webkit-device-pixel-ratio: 3.125)"));
        assert!(matches("(-webkit-min-device-pixel-ratio: 3.1)"));
        let screen = MediaEnvironment::default();
        assert!(screen.matches_on_screen("screen and (resolution: 1dppx)"));
        assert!(!screen.matches_on_screen("print"));
        assert!(screen.matches_condition_on_screen("(max-resolution: 96dpi)"));
        assert!(!screen.matches_condition("(max-resolution: 96dpi)"));
    }
}

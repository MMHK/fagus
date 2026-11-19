//! Conversion of declared (parsed) values into computed values.

use std::sync::Arc;

use lightningcss::properties::align::{
    AlignContent, AlignItems, AlignSelf, ContentDistribution, ContentPosition, GapValue, JustifyContent,
    SelfPosition,
};
use lightningcss::properties::border::{BorderSideWidth, LineStyle};
use lightningcss::properties::display::{
    Display as CssDisplay, DisplayInside as CssInside, DisplayKeyword, DisplayOutside as CssOutside,
    Visibility as CssVisibility,
};
use lightningcss::properties::flex::{FlexDirection as CssFlexDirection, FlexWrap as CssFlexWrap};
use lightningcss::properties::font::{
    AbsoluteFontSize, AbsoluteFontWeight, FontFamily, FontSize, FontStyle as CssFontStyle, FontWeight,
    GenericFontFamily, LineHeight as CssLineHeight, RelativeFontSize, VerticalAlign as CssVerticalAlign,
    VerticalAlignKeyword,
};
use lightningcss::properties::grid::{TrackBreadth, TrackListItem, TrackSize as CssTrackSize, TrackSizing};
use lightningcss::properties::list::{
    ListStylePosition as CssListStylePosition, ListStyleType as CssListStyleType,
};
use lightningcss::properties::outline::OutlineStyle as CssOutlineStyle;
use lightningcss::properties::overflow::{OverflowKeyword, TextOverflow as CssTextOverflow};
use lightningcss::properties::position::{Position as CssPosition, ZIndex};
use lightningcss::properties::size::{BoxSizing as CssBoxSizing, MaxSize as CssMaxSize, Size as CssSize};
use lightningcss::properties::svg::SVGPaint;
use lightningcss::properties::text::{
    Direction as CssDirection, Spacing, TextAlign as CssTextAlign, TextAlignLast as CssTextAlignLast,
    TextDecorationLine as CssDecorationLine, TextDecorationStyle as CssDecorationStyle,
    TextDecorationThickness as CssDecorationThickness, TextJustify as CssTextJustify, TextTransformCase,
    UnicodeBidi as CssUnicodeBidi, WhiteSpace as CssWhiteSpace,
};
use lightningcss::properties::transform::{Rotate, Scale, Transform as CssTransform, Translate};
use lightningcss::properties::{Property, PropertyId};
use lightningcss::stylesheet::PrinterOptions;
use lightningcss::values::calc::{Calc, MathFunction};
use lightningcss::values::color::CssColor;
use lightningcss::values::length::{
    Length, LengthOrNumber, LengthPercentage as CssLengthPercentage, LengthPercentageOrAuto, LengthValue,
};
use lightningcss::values::percentage::DimensionPercentage;
use lightningcss::values::percentage::NumberOrPercentage;
use lightningcss::values::position::{HorizontalPositionKeyword, VerticalPositionKeyword};

use crate::background;
use crate::style::ComputedValues;
use crate::values::{
    Appearance, AutoRepeat, BoxShadow, DecorationThickness, EmphasisMark, EmphasisShape, FontSizeBasis,
    GridArea, GridAreas, GridLine, Hyphens, LineBreak, MAX_GRID_TRACKS, OutlineStyle, OverflowWrap, TabSize,
    TextJustify, TextOverflow, TextShadow, TrackList, TransformFunction, WordBreak,
};
use crate::values::{
    BorderStyle, BoxSizing, CalcNode, ContentAlignment, Direction, Display, DisplayInside, DisplayOutside,
    FlexDirection, FlexWrap, FontStyle, InternalDisplay, ItemAlignment, LengthPercentage,
    LengthPercentageAuto, LineHeight, ListStylePosition, ListStyleType, MaxSize, Overflow, Position, Rgba,
    SelfFlags, Size, TextAlign, TextDecorationStyle, TextTransform, TrackSize, UnicodeBidi, VerticalAlign,
    Visibility, WhiteSpace,
};

/// What relative units refer to while computing an element's values.
#[derive(Clone, Copy, Debug)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent facts about the element that units and sides depend on"
)]
pub(crate) struct Context {
    /// The element's own computed font size (for `em` in all properties except `font-size`).
    pub font_size: f32,
    /// The parent's computed font size, scaled to the element's zoom: what `em` in
    /// `font-size` refers to.
    pub parent_font_size: f32,
    /// Whether the parent's font size comes from an absolute length (see [`FontSizeBasis`]).
    pub parent_font_size_absolute: bool,
    /// The root element's font size without its zoom (for `rem`), which lengths multiply by
    /// the element's zoom as they do absolute lengths.
    pub root_font_size: f32,
    /// The metrics of the element's first available font, for `ex`, `ch`, `cap` and `ic`: the
    /// parent's while `font-size` and the font properties are computed.
    pub units: crate::values::FontUnits,
    /// The metrics of the root element's first available font, for `rex`, `rch`, `rcap` and
    /// `ric`.
    pub root_units: crate::values::FontUnits,
    /// The element's computed line height in pixels, for `lh`: the parent's while `font-size`
    /// and `line-height` are computed.
    pub line_height: f32,
    /// The root element's computed line height without its zoom, for `rlh`.
    pub root_line_height: f32,
    /// The element's effective zoom, by which absolute lengths, viewport units and
    /// root-relative lengths are multiplied (font-relative lengths refer to zoomed values
    /// already), as Blink computes them (`CSSToLengthConversionData::ZoomedComputedPixels`).
    pub zoom: f32,
    /// Width and height of the page area, for viewport units.
    pub viewport: (f32, f32),
    /// Width and height of the nearest query container's content box without its zoom, for
    /// container query units: the page area where there is none.
    pub container: (f32, f32),
    /// Whether the element's own `direction` is `rtl`, by which logical properties map to
    /// physical sides.
    pub rtl: bool,
    /// The element's own `writing-mode`, by which logical properties map to physical sides too.
    pub writing_mode: crate::values::WritingMode,
    /// Whether the element's text is set upright in vertical lines (`text-orientation:
    /// upright`), where `ch` refers to the vertical advance of the "0".
    pub upright: bool,
    /// Whether the parent's `direction` is `rtl`: that of the containing block, by which
    /// `float` and `clear` map `inline-start` and `inline-end` to sides.
    pub parent_rtl: bool,
    /// Whether the element is the root, which has no parent to match (`text-align:
    /// match-parent` is `start` there).
    pub root: bool,
}

/// The shortest decimal number that parses to `value`: the number as a stylesheet wrote it
/// (`0.64` for the single-precision 0.63999999), which Blink keeps as a double.
pub(crate) fn decimal(value: f32) -> f64 {
    let wide = f64::from(value);
    if value == 0.0 || !value.is_finite() {
        return wide;
    }
    let exponent = libm::log10(wide.abs()).floor() as i32;
    for digits in 1..=9 {
        // Powers of ten up to 1e22 are exact doubles, so the rounding below is exact too.
        let shift = digits - 1 - exponent;
        let power = 10f64.powi(shift.abs());
        let rounded = if shift >= 0 {
            (wide * power).round() / power
        } else {
            (wide / power).round() * power
        };
        if (rounded as f32).to_bits() == value.to_bits() {
            return rounded;
        }
    }
    wide
}

/// An absolute length in pixels, multiplied out in double precision from the number as
/// written, as Blink converts units (`25.4mm` is 96px, not 95.99999px).
pub(crate) fn absolute_px(value: &LengthValue) -> Option<f64> {
    let (number, factor) = match value {
        LengthValue::Px(v) => return Some(f64::from(*v)),
        LengthValue::In(v) => (v, 96.0),
        LengthValue::Cm(v) => (v, 96.0 / 2.54),
        LengthValue::Mm(v) => (v, 96.0 / 25.4),
        LengthValue::Q(v) => (v, 96.0 / 101.6),
        LengthValue::Pt(v) => (v, 96.0 / 72.0),
        LengthValue::Pc(v) => (v, 16.0),
        _ => return None,
    };
    Some(decimal(*number) * factor)
}

impl Context {
    pub fn length_value(&self, value: &LengthValue) -> f32 {
        self.length_value_with(value, self.font_size)
    }

    fn length_value_with(&self, value: &LengthValue, font_size: f32) -> f32 {
        if let Some(px) = absolute_px(value) {
            return (px * f64::from(self.zoom)) as f32;
        }
        let (width, height) = self.viewport;
        let root_font_size = self.root_font_size * self.zoom;
        let zoomed = match value {
            // In double precision from the number as written, as Blink multiplies the parsed
            // double by the font size (`.64rem` of a 20px root is 12.8px, not 12.799999px, whose
            // `.9375em` would truncate to 11.984375px instead of 12px).
            LengthValue::Em(v) => return (decimal(*v) * f64::from(font_size)) as f32,
            LengthValue::Rem(v) => return (decimal(*v) * f64::from(root_font_size)) as f32,
            LengthValue::Ex(v) => return v * font_size * self.units.ex,
            LengthValue::Ch(v) if self.upright => return v * self.units.upright_ch(font_size),
            LengthValue::Ch(v) => return v * font_size * self.units.ch,
            LengthValue::Cap(v) => return v * font_size * self.units.cap,
            LengthValue::Ic(v) => return v * font_size * self.units.ic,
            LengthValue::Lh(v) => return v * self.line_height,
            LengthValue::Rex(v) => return v * root_font_size * self.root_units.ex,
            LengthValue::Rch(v) => return v * root_font_size * self.root_units.ch,
            LengthValue::Rcap(v) => return v * root_font_size * self.root_units.cap,
            LengthValue::Ric(v) => return v * root_font_size * self.root_units.ic,
            LengthValue::Rlh(v) => return v * self.root_line_height * self.zoom,
            LengthValue::Vw(v) | LengthValue::Svw(v) | LengthValue::Lvw(v) | LengthValue::Dvw(v) => {
                v * width / 100.0
            }
            LengthValue::Vh(v) | LengthValue::Svh(v) | LengthValue::Lvh(v) | LengthValue::Dvh(v) => {
                v * height / 100.0
            }
            // The inline and block axes are the element's own, as in Blink.
            LengthValue::Vi(v) | LengthValue::Svi(v) | LengthValue::Lvi(v) | LengthValue::Dvi(v) => {
                v * if self.writing_mode.is_vertical() {
                    height
                } else {
                    width
                } / 100.0
            }
            LengthValue::Vb(v) | LengthValue::Svb(v) | LengthValue::Lvb(v) | LengthValue::Dvb(v) => {
                v * if self.writing_mode.is_vertical() {
                    width
                } else {
                    height
                } / 100.0
            }
            LengthValue::Vmin(v) | LengthValue::Svmin(v) | LengthValue::Lvmin(v) | LengthValue::Dvmin(v) => {
                v * width.min(height) / 100.0
            }
            LengthValue::Vmax(v) | LengthValue::Svmax(v) | LengthValue::Lvmax(v) | LengthValue::Dvmax(v) => {
                v * width.max(height) / 100.0
            }
            LengthValue::Cqw(v) | LengthValue::Cqi(v) => v * self.container.0 / 100.0,
            LengthValue::Cqh(v) | LengthValue::Cqb(v) => v * self.container.1 / 100.0,
            LengthValue::Cqmin(v) => v * self.container.0.min(self.container.1) / 100.0,
            LengthValue::Cqmax(v) => v * self.container.0.max(self.container.1) / 100.0,
            _ => 0.0,
        };
        zoomed * self.zoom
    }

    pub fn length(&self, length: &Length) -> f32 {
        crate::values::censored(match length {
            Length::Value(value) => self.length_value(value),
            Length::Calc(calc) => self.calc_length(calc),
        })
    }

    fn calc_length(&self, calc: &Calc<Length>) -> f32 {
        match calc {
            Calc::Value(length) => self.length(length),
            Calc::Number(number) => *number,
            Calc::Sum(a, b) => self.calc_length(a) + self.calc_length(b),
            Calc::Product(factor, node) => factor * self.calc_length(node),
            Calc::Function(function) => match &**function {
                MathFunction::Calc(node) => self.calc_length(node),
                MathFunction::Min(nodes) => nodes
                    .iter()
                    .map(|node| self.calc_length(node))
                    .fold(f32::INFINITY, f32::min),
                MathFunction::Max(nodes) => nodes
                    .iter()
                    .map(|node| self.calc_length(node))
                    .fold(f32::NEG_INFINITY, f32::max),
                MathFunction::Clamp(min, value, max) => self
                    .calc_length(value)
                    .min(self.calc_length(max))
                    .max(self.calc_length(min)),
                MathFunction::Abs(node) => self.calc_length(node).abs(),
                MathFunction::Round(strategy, value, step) => {
                    rounding(strategy).round(self.calc_length(value), self.calc_length(step))
                }
                MathFunction::Mod(a, b) => crate::values::css_mod(self.calc_length(a), self.calc_length(b)),
                MathFunction::Rem(a, b) => crate::values::css_rem(self.calc_length(a), self.calc_length(b)),
                MathFunction::Hypot(nodes) => nodes
                    .iter()
                    .map(|node| self.calc_length(node).powi(2))
                    .sum::<f32>()
                    .sqrt(),
                // A sign is a number, which a length cannot be.
                MathFunction::Sign(_) => 0.0,
            },
        }
    }

    pub fn length_percentage(&self, value: &CssLengthPercentage) -> LengthPercentage {
        LengthPercentage::from_node(self.calc_node(value, self.font_size))
    }

    /// For `font-size`: `em` and percentages refer to the parent's font size.
    fn font_length_percentage(&self, value: &CssLengthPercentage) -> f32 {
        self.calc_node(value, self.parent_font_size)
            .resolve(self.parent_font_size)
    }

    fn calc_node(&self, value: &CssLengthPercentage, font_size: f32) -> CalcNode {
        match value {
            DimensionPercentage::Dimension(length) => CalcNode::Px(self.length_value_with(length, font_size)),
            DimensionPercentage::Percentage(percentage) => CalcNode::Percent(percentage.0),
            DimensionPercentage::Calc(calc) => self.calc_tree(calc, font_size),
        }
    }

    fn calc_tree(&self, calc: &Calc<CssLengthPercentage>, font_size: f32) -> CalcNode {
        match calc {
            Calc::Value(value) => self.calc_node(value, font_size),
            Calc::Number(number) => CalcNode::Px(*number),
            Calc::Sum(a, b) => CalcNode::Sum(
                Box::new(self.calc_tree(a, font_size)),
                Box::new(self.calc_tree(b, font_size)),
            ),
            Calc::Product(factor, node) => {
                CalcNode::Product(*factor, Box::new(self.calc_tree(node, font_size)))
            }
            Calc::Function(function) => match &**function {
                MathFunction::Calc(node) => self.calc_tree(node, font_size),
                MathFunction::Min(nodes) => {
                    CalcNode::Min(nodes.iter().map(|node| self.calc_tree(node, font_size)).collect())
                }
                MathFunction::Max(nodes) => {
                    CalcNode::Max(nodes.iter().map(|node| self.calc_tree(node, font_size)).collect())
                }
                MathFunction::Clamp(min, value, max) => CalcNode::Clamp(
                    Box::new(self.calc_tree(min, font_size)),
                    Box::new(self.calc_tree(value, font_size)),
                    Box::new(self.calc_tree(max, font_size)),
                ),
                MathFunction::Abs(node) => CalcNode::Abs(Box::new(self.calc_tree(node, font_size))),
                MathFunction::Round(strategy, value, step) => CalcNode::Round(
                    rounding(strategy),
                    Box::new(self.calc_tree(value, font_size)),
                    Box::new(self.calc_tree(step, font_size)),
                ),
                MathFunction::Mod(a, b) => CalcNode::Mod(
                    Box::new(self.calc_tree(a, font_size)),
                    Box::new(self.calc_tree(b, font_size)),
                ),
                MathFunction::Rem(a, b) => CalcNode::Rem(
                    Box::new(self.calc_tree(a, font_size)),
                    Box::new(self.calc_tree(b, font_size)),
                ),
                MathFunction::Hypot(nodes) => {
                    CalcNode::Hypot(nodes.iter().map(|node| self.calc_tree(node, font_size)).collect())
                }
                MathFunction::Sign(_) => CalcNode::Px(0.0),
            },
        }
    }

    /// `filter`: its functions, and SVG filters (`url()`) by their URL.
    fn filters(
        &self,
        list: &lightningcss::properties::effects::FilterList<'_>,
    ) -> Arc<[crate::values::Filter]> {
        use crate::values::Filter;
        use lightningcss::properties::effects::{Filter as Css, FilterList};
        use lightningcss::values::percentage::NumberOrPercentage;
        let amount = |value: &NumberOrPercentage| match value {
            NumberOrPercentage::Number(number) => *number,
            NumberOrPercentage::Percentage(percentage) => percentage.0,
        };
        let FilterList::Filters(filters) = list else {
            return Arc::from([]);
        };
        filters
            .iter()
            .map(|filter| match filter {
                Css::Blur(length) => Filter::Blur(self.length(length).max(0.0)),
                Css::Brightness(value) => Filter::Brightness(amount(value)),
                Css::Contrast(value) => Filter::Contrast(amount(value)),
                Css::Grayscale(value) => Filter::Grayscale(amount(value)),
                Css::HueRotate(angle) => Filter::HueRotate(angle.to_degrees()),
                Css::Invert(value) => Filter::Invert(amount(value)),
                Css::Opacity(value) => Filter::Opacity(amount(value)),
                Css::Saturate(value) => Filter::Saturate(amount(value)),
                Css::Sepia(value) => Filter::Sepia(amount(value)),
                Css::DropShadow(shadow) => Filter::DropShadow {
                    x: self.length(&shadow.x_offset),
                    y: self.length(&shadow.y_offset),
                    blur: self.length(&shadow.blur).max(0.0),
                    color: color(&shadow.color),
                },
                Css::Url(url) => Filter::Url(Arc::from(url.url.as_ref())),
            })
            .collect()
    }

    /// `clip-path`: a basic shape with its reference box, a box, or an SVG `<clipPath>` by its
    /// URL.
    pub(crate) fn clip_path(
        &self,
        value: &lightningcss::properties::masking::ClipPath<'_>,
    ) -> Option<Arc<crate::values::ClipPath>> {
        use crate::values::{BoxArea, ClipPath, ClipShape, TransformBox};
        use lightningcss::properties::masking::{ClipPath as Css, GeometryBox};
        // `fill-box` is the content box of a box, `stroke-box` and `view-box` its border box.
        let reference = |geometry: &GeometryBox| match geometry {
            GeometryBox::PaddingBox => BoxArea::PaddingBox,
            GeometryBox::ContentBox | GeometryBox::FillBox => BoxArea::ContentBox,
            GeometryBox::MarginBox => BoxArea::MarginBox,
            _ => BoxArea::BorderBox,
        };
        let svg_reference = |geometry: &GeometryBox| match geometry {
            GeometryBox::ContentBox | GeometryBox::PaddingBox | GeometryBox::FillBox => TransformBox::FillBox,
            GeometryBox::ViewBox => TransformBox::ViewBox,
            _ => TransformBox::StrokeBox,
        };
        let (shape, geometry) = match value {
            Css::None => return None,
            Css::Url(url) => (
                ClipShape::Url(Arc::from(url.url.as_ref())),
                &GeometryBox::BorderBox,
            ),
            Css::Box(geometry) => (ClipShape::Box, geometry),
            Css::Shape { shape, reference_box } => (self.basic_shape(shape)?, reference_box),
        };
        Some(Arc::new(ClipPath {
            shape,
            reference: reference(geometry),
            svg_reference: svg_reference(geometry),
        }))
    }

    /// A basic shape (CSS Shapes 1, §3) of `clip-path` or `shape-outside`.
    pub(crate) fn basic_shape(
        &self,
        shape: &lightningcss::values::shape::BasicShape,
    ) -> Option<crate::values::ClipShape> {
        use crate::values::{ClipShape, Corners, ShapeRadius};
        use lightningcss::values::shape::{BasicShape, ShapeRadius as CssRadius};
        let radius = |radius: &CssRadius| match radius {
            CssRadius::LengthPercentage(length) => ShapeRadius::Length(self.length_percentage(length)),
            CssRadius::ClosestSide => ShapeRadius::ClosestSide,
            CssRadius::FarthestSide => ShapeRadius::FarthestSide,
        };
        Some(match shape {
            BasicShape::Inset(inset) => {
                let corner = |size: &lightningcss::values::size::Size2D<CssLengthPercentage>| {
                    (self.length_percentage(&size.0), self.length_percentage(&size.1))
                };
                ClipShape::Inset {
                    insets: [
                        self.length_percentage(&inset.rect.0),
                        self.length_percentage(&inset.rect.1),
                        self.length_percentage(&inset.rect.2),
                        self.length_percentage(&inset.rect.3),
                    ],
                    radii: Corners {
                        top_left: corner(&inset.radius.top_left),
                        top_right: corner(&inset.radius.top_right),
                        bottom_right: corner(&inset.radius.bottom_right),
                        bottom_left: corner(&inset.radius.bottom_left),
                    },
                }
            }
            BasicShape::Circle(circle) => ClipShape::Ellipse {
                radii: (radius(&circle.radius), radius(&circle.radius)),
                circle: true,
                center: background::position(&circle.position, self),
            },
            BasicShape::Ellipse(ellipse) => ClipShape::Ellipse {
                radii: (radius(&ellipse.radius_x), radius(&ellipse.radius_y)),
                circle: false,
                center: background::position(&ellipse.position, self),
            },
            BasicShape::Polygon(polygon) => {
                use lightningcss::traits::{Parse, ToCss};
                // The points are not public: read them back from their CSS.
                let mut points = Vec::new();
                for point in &polygon.points {
                    let css = point.to_css_string(PrinterOptions::default()).ok()?;
                    let lightningcss::values::size::Size2D(x, y) =
                        lightningcss::values::size::Size2D::<CssLengthPercentage>::parse_string(&css).ok()?;
                    points.push((self.length_percentage(&x), self.length_percentage(&y)));
                }
                ClipShape::Polygon {
                    even_odd: polygon.fill_rule == lightningcss::values::shape::FillRule::Evenodd,
                    points,
                }
            }
        })
    }

    pub fn length_percentage_auto(&self, value: &LengthPercentageOrAuto) -> LengthPercentageAuto {
        match value {
            LengthPercentageOrAuto::Auto => LengthPercentageAuto::Auto,
            LengthPercentageOrAuto::LengthPercentage(length) => {
                LengthPercentageAuto::Length(self.length_percentage(length))
            }
        }
    }

    fn size(&self, value: &CssSize) -> Size {
        match value {
            CssSize::Auto | CssSize::Contain => Size::Auto,
            CssSize::Stretch(_) => Size::Stretch,
            CssSize::LengthPercentage(length) => Size::Length(self.length_percentage(length)),
            CssSize::MinContent(_) => Size::MinContent,
            CssSize::MaxContent(_) => Size::MaxContent,
            CssSize::FitContent(_) => Size::FitContent(None),
            // Chrome does not take `fit-content(<length-percentage>)` in sizes, only in grid
            // tracks: the declaration is invalid there, which is as if it were auto.
            CssSize::FitContentFunction(_) => Size::Auto,
        }
    }

    fn max_size(&self, value: &CssMaxSize) -> MaxSize {
        match value {
            CssMaxSize::None | CssMaxSize::Contain | CssMaxSize::FitContentFunction(_) => MaxSize::None,
            CssMaxSize::Stretch(_) => MaxSize::Stretch,
            CssMaxSize::LengthPercentage(length) => MaxSize::Length(self.length_percentage(length)),
            CssMaxSize::MinContent(_) => MaxSize::MinContent,
            CssMaxSize::MaxContent(_) => MaxSize::MaxContent,
            CssMaxSize::FitContent(_) => MaxSize::FitContent(None),
        }
    }

    fn track_size(&self, value: &CssTrackSize) -> TrackSize {
        match value {
            CssTrackSize::TrackBreadth(breadth) => self.track_breadth(breadth),
            CssTrackSize::MinMax { min, max } => TrackSize::MinMax(
                Box::new(self.track_breadth(min)),
                Box::new(self.track_breadth(max)),
            ),
            CssTrackSize::FitContent(length) => TrackSize::FitContent(self.length_percentage(length)),
        }
    }

    fn track_breadth(&self, value: &TrackBreadth) -> TrackSize {
        match value {
            TrackBreadth::Length(length) => TrackSize::Length(self.length_percentage(length)),
            TrackBreadth::Flex(fraction) => TrackSize::Fraction(*fraction),
            TrackBreadth::MinContent => TrackSize::MinContent,
            TrackBreadth::MaxContent => TrackSize::MaxContent,
            TrackBreadth::Auto => TrackSize::Auto,
        }
    }

    fn track_list(&self, value: &TrackSizing<'_>) -> TrackList {
        use lightningcss::properties::grid::RepeatCount;
        let after_names =
            |idents: Option<&lightningcss::values::ident::CustomIdentList<'_>>| -> Arc<[(usize, Arc<str>)]> {
                idents
                    .into_iter()
                    .flatten()
                    .map(|ident| (0, Arc::from(ident.0.as_ref())))
                    .collect()
            };
        match value {
            TrackSizing::None => TrackList::default(),
            TrackSizing::TrackList(list) => {
                let mut tracks = Vec::new();
                let mut names: Vec<(usize, Arc<str>)> = Vec::new();
                // Line names lie before the track of the same index, the last after all tracks.
                let mut name =
                    |line: usize, idents: Option<&lightningcss::values::ident::CustomIdentList<'_>>| {
                        for ident in idents.into_iter().flatten() {
                            names.push((line, Arc::from(ident.0.as_ref())));
                        }
                    };
                let mut auto_repeat: Option<AutoRepeat> = None;
                for (index, item) in list.items.iter().enumerate() {
                    // The names right after an automatic repetition go with it.
                    let after_repeat = index > 0
                        && matches!(&list.items[index - 1], TrackListItem::TrackRepeat(repeat)
                            if matches!(repeat.count, RepeatCount::AutoFill | RepeatCount::AutoFit));
                    if after_repeat && let Some(repeat) = auto_repeat.as_mut() {
                        repeat.after = after_names(list.line_names.get(index));
                    } else {
                        name(tracks.len(), list.line_names.get(index));
                    }
                    match item {
                        TrackListItem::TrackSize(size) => tracks.push(self.track_size(size)),
                        TrackListItem::TrackRepeat(repeat) => {
                            let sizes = repeat.track_sizes.iter().map(|size| self.track_size(size));
                            match repeat.count {
                                RepeatCount::Number(count) => {
                                    for _ in 0..count.max(1) {
                                        if tracks.len() >= usize::from(MAX_GRID_TRACKS) {
                                            break;
                                        }
                                        for (track, size) in sizes.clone().enumerate() {
                                            name(tracks.len(), repeat.line_names.get(track));
                                            tracks.push(size);
                                        }
                                        name(tracks.len(), repeat.line_names.last());
                                    }
                                }
                                RepeatCount::AutoFill | RepeatCount::AutoFit => {
                                    let names = repeat
                                        .line_names
                                        .iter()
                                        .enumerate()
                                        .flat_map(|(line, idents)| {
                                            idents
                                                .iter()
                                                .map(move |ident| (line, Arc::from(ident.0.as_ref())))
                                        })
                                        .collect();
                                    auto_repeat = Some(AutoRepeat {
                                        at: tracks.len(),
                                        tracks: sizes.collect(),
                                        fit: repeat.count == RepeatCount::AutoFit,
                                        names,
                                        after: Arc::from([]),
                                        after_lists: 0,
                                    });
                                }
                            }
                        }
                    }
                }
                let last = list.items.len();
                let after_repeat = last > 0
                    && matches!(&list.items[last - 1], TrackListItem::TrackRepeat(repeat)
                        if matches!(repeat.count, RepeatCount::AutoFill | RepeatCount::AutoFit));
                if after_repeat && let Some(repeat) = auto_repeat.as_mut() {
                    repeat.after = after_names(list.line_names.get(last));
                } else {
                    name(tracks.len(), list.line_names.get(last));
                }
                TrackList {
                    tracks: Arc::from(tracks),
                    names: Arc::from(names),
                    auto_repeat,
                    subgrid: false,
                }
            }
        }
    }

    fn track_sizes(&self, sizes: &[CssTrackSize]) -> Arc<[TrackSize]> {
        sizes.iter().map(|size| self.track_size(size)).collect()
    }
}

/// The geometry properties of SVG elements that CSS sets beside `width` and `height`, which the
/// SVG renderer reads as attributes only (see [`svg_paint`]).
pub const SVG_GEOMETRY: [&str; 8] = ["x", "y", "cx", "cy", "r", "rx", "ry", "d"];

/// A declaration of an SVG painting property as CSS text for the SVG renderer, which reads
/// colors of CSS Color 3 only: colors in sRGB, `currentColor` kept for it to resolve.
pub(crate) fn svg_paint(property: &Property<'_>) -> Option<(&'static str, String)> {
    let text = || property.value_to_css_string(PrinterOptions::default()).ok();
    let paint = |value: &SVGPaint<'_>| match value {
        SVGPaint::Color(css) => Some(color(css).map_or_else(
            || "currentColor".to_owned(),
            |rgba| {
                let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round();
                format!(
                    "rgba({}, {}, {}, {})",
                    channel(rgba.r),
                    channel(rgba.g),
                    channel(rgba.b),
                    rgba.a.clamp(0.0, 1.0)
                )
            },
        )),
        _ => text(),
    };
    Some(match property {
        Property::Fill(value) => ("fill", paint(value)?),
        Property::Stroke(value) => ("stroke", paint(value)?),
        Property::FillRule(_) => ("fill-rule", text()?),
        Property::FillOpacity(_) => ("fill-opacity", text()?),
        Property::StrokeOpacity(_) => ("stroke-opacity", text()?),
        Property::StrokeWidth(_) => ("stroke-width", text()?),
        Property::StrokeLinecap(_) => ("stroke-linecap", text()?),
        Property::StrokeLinejoin(_) => ("stroke-linejoin", text()?),
        Property::StrokeMiterlimit(_) => ("stroke-miterlimit", text()?),
        Property::StrokeDasharray(_) => ("stroke-dasharray", text()?),
        Property::StrokeDashoffset(_) => ("stroke-dashoffset", text()?),
        // Of a `<mask>`, which the SVG renderer reads.
        Property::MaskType(_) => ("mask-type", text()?),
        // The geometry properties of SVG elements (SVG 2, §9.1 and §13), which lightningcss
        // keeps as tokens (`width` and `height` are CSS sizes).
        Property::Custom(custom)
            if let lightningcss::properties::custom::CustomPropertyName::Unknown(name) = &custom.name
                && let Some(geometry) = SVG_GEOMETRY
                    .iter()
                    .find(|known| name.0.eq_ignore_ascii_case(known)) =>
        {
            (geometry, text()?.trim().to_owned())
        }
        // `vector-effect`, which lightningcss keeps as tokens.
        Property::Custom(custom)
            if matches!(&custom.name, lightningcss::properties::custom::CustomPropertyName::Unknown(name)
                if name.0.eq_ignore_ascii_case("vector-effect")) =>
        {
            let value = text()?.trim().to_ascii_lowercase();
            if value != "none" && value != "non-scaling-stroke" {
                return None;
            }
            ("vector-effect", value)
        }
        _ => return None,
    })
}

/// Converts `text-emphasis-style`. A fill without a shape is a circle in horizontal text; an
/// empty string is no mark.
fn emphasis_mark(value: &lightningcss::properties::text::TextEmphasisStyle<'_>) -> Option<EmphasisMark> {
    use lightningcss::properties::text::{TextEmphasisFillMode, TextEmphasisShape, TextEmphasisStyle};
    match value {
        TextEmphasisStyle::None => None,
        TextEmphasisStyle::String(text) => {
            (!text.is_empty()).then(|| EmphasisMark::String(text.as_ref().into()))
        }
        TextEmphasisStyle::Keyword { fill, shape } => Some(EmphasisMark::Shape {
            shape: match shape {
                Some(TextEmphasisShape::Dot) => EmphasisShape::Dot,
                Some(TextEmphasisShape::Circle) | None => EmphasisShape::Circle,
                Some(TextEmphasisShape::DoubleCircle) => EmphasisShape::DoubleCircle,
                Some(TextEmphasisShape::Triangle) => EmphasisShape::Triangle,
                Some(TextEmphasisShape::Sesame) => EmphasisShape::Sesame,
            },
            filled: *fill == TextEmphasisFillMode::Filled,
        }),
    }
}

thread_local! {
    /// Whether the element whose style is being computed uses the dark color scheme: the colors
    /// of its declarations resolve `light-dark()` and system colors with it.
    static DARK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Sets the color scheme that [`color`] resolves colors in, until it is set again.
pub(crate) fn set_dark(dark: bool) {
    DARK.set(dark);
}

/// Whether the element whose style is being computed uses the dark color scheme.
pub(crate) fn is_dark() -> bool {
    DARK.get()
}

thread_local! {
    /// The color that `currentcolor` stands for in the color functions Fagus computes
    /// (`color-mix()`, relative colors) of the declarations being computed: the parent's
    /// `color` while `color` itself is computed, the element's own afterwards.
    static CURRENT: std::cell::Cell<Option<Rgba>> = const { std::cell::Cell::new(None) };
}

/// Sets the color that `currentcolor` resolves to in color functions, until it is set again.
pub(crate) fn set_current_color(color: Option<Rgba>) {
    CURRENT.set(color);
}

/// The color that `currentcolor` resolves to in color functions: `None` outside of the
/// computation of an element's style.
pub(crate) fn current_color() -> Option<Rgba> {
    CURRENT.get()
}

/// Converts a color. `None` stands for `currentColor`.
pub(crate) fn color(value: &CssColor) -> Option<Rgba> {
    match value {
        CssColor::CurrentColor => None,
        CssColor::LightDark(light, dark) => color(if DARK.get() { dark } else { light }),
        CssColor::System(system) => Some(system_color(*system, DARK.get())),
        // Colors of other spaces than sRGB keep their precision, and those outside of sRGB are
        // clipped to it channel by channel, as Chrome draws them (lightningcss would map them
        // into the gamut by reducing their chroma).
        CssColor::LAB(_) | CssColor::Predefined(_) => {
            let clamped = clamp_lightness(value);
            let srgb = lightningcss::values::color::SRGB::try_from(clamped.as_ref().unwrap_or(value)).ok()?;
            let channel = |value: f32| {
                if value.is_nan() {
                    0.0
                } else {
                    value.clamp(0.0, 1.0)
                }
            };
            Some(Rgba {
                r: channel(srgb.r),
                g: channel(srgb.g),
                b: channel(srgb.b),
                a: channel(srgb.alpha),
            })
        }
        other => match other.to_rgb() {
            Ok(CssColor::RGBA(rgba)) => Some(Rgba::from_u8(
                rgba.red,
                rgba.green,
                rgba.blue,
                f32::from(rgba.alpha) / 255.0,
            )),
            _ => Some(Rgba::BLACK),
        },
    }
}

/// A `lab()`, `lch()`, `oklab()` or `oklch()` color with its lightness clamped to its range
/// (0 to 100, or to 1 for the `ok` spaces) and its chroma to no less than 0, as Chrome clamps
/// them where it parses them; `None` for other colors and those already in range.
fn clamp_lightness(value: &CssColor) -> Option<CssColor> {
    use lightningcss::values::color::LABColor;
    let CssColor::LAB(lab) = value else {
        return None;
    };
    let clamped = match **lab {
        LABColor::LAB(mut color) => {
            color.l = color.l.clamp(0.0, 100.0);
            LABColor::LAB(color)
        }
        LABColor::LCH(mut color) => {
            color.l = color.l.clamp(0.0, 100.0);
            color.c = color.c.max(0.0);
            LABColor::LCH(color)
        }
        LABColor::OKLAB(mut color) => {
            color.l = color.l.clamp(0.0, 1.0);
            LABColor::OKLAB(color)
        }
        LABColor::OKLCH(mut color) => {
            color.l = color.l.clamp(0.0, 1.0);
            color.c = color.c.max(0.0);
            LABColor::OKLCH(color)
        }
    };
    (clamped != **lab).then(|| CssColor::LAB(Box::new(clamped)))
}

/// A system color as Chrome 157 on macOS prints it in the light or the dark color scheme
/// (measured; Blink's `LayoutTheme::DefaultSystemColor` and the macOS color provider).
fn system_color(system: lightningcss::values::color::SystemColor, dark: bool) -> Rgba {
    use lightningcss::values::color::SystemColor as S;
    const BLACK: [u8; 3] = [0, 0, 0];
    const WHITE: [u8; 3] = [0xff, 0xff, 0xff];
    let (light, dark_color) = match system {
        S::AccentColor => ([0x00, 0x75, 0xff], [0x00, 0x75, 0xff]),
        S::ActiveText => ([0xff, 0x00, 0x00], [0xff, 0x00, 0x00]),
        S::ButtonFace | S::ButtonHighlight | S::ButtonShadow | S::ThreeDFace => {
            ([0xef, 0xef, 0xef], [0x6b, 0x6b, 0x6b])
        }
        S::AccentColorText => (WHITE, WHITE),
        S::Canvas
        | S::ActiveCaption
        | S::AppWorkspace
        | S::Background
        | S::InactiveCaption
        | S::InfoBackground
        | S::Menu
        | S::Scrollbar
        | S::Window => (WHITE, [0x12, 0x12, 0x12]),
        S::Field => (WHITE, [0x3b, 0x3b, 0x3b]),
        S::GrayText | S::InactiveCaptionText => ([0x80, 0x80, 0x80], [0x80, 0x80, 0x80]),
        S::Highlight => ([0xb3, 0xd7, 0xfe], [0xc2, 0xdf, 0xff]),
        S::SelectedItem => ([0xb3, 0xd7, 0xff], [0x99, 0xc8, 0xff]),
        S::SelectedItemText => (BLACK, [0x3b, 0x3b, 0x3b]),
        S::HighlightText | S::MarkText => (BLACK, BLACK),
        S::LinkText => ([0x00, 0x00, 0xee], [0x9e, 0x9e, 0xff]),
        S::Mark => ([0xff, 0xff, 0x00], [0xff, 0xff, 0x00]),
        S::VisitedText => ([0x55, 0x1a, 0x8b], [0xd0, 0xad, 0xf0]),
        // `CanvasText`, `ButtonText`, `ButtonBorder`, `FieldText` and the deprecated colors
        // that stand for them.
        _ => (BLACK, WHITE),
    };
    let [r, g, b] = if dark { dark_color } else { light };
    Rgba::from_u8(r, g, b, 1.0)
}

fn rounding(strategy: &lightningcss::values::calc::RoundingStrategy) -> crate::values::Rounding {
    use lightningcss::values::calc::RoundingStrategy;
    match strategy {
        RoundingStrategy::Nearest => crate::values::Rounding::Nearest,
        RoundingStrategy::Up => crate::values::Rounding::Up,
        RoundingStrategy::Down => crate::values::Rounding::Down,
        RoundingStrategy::ToZero => crate::values::Rounding::ToZero,
    }
}

fn display(value: &CssDisplay) -> Display {
    match value {
        CssDisplay::Keyword(keyword) => match keyword {
            DisplayKeyword::None => Display::None,
            DisplayKeyword::Contents => Display::Contents,
            DisplayKeyword::TableRowGroup => Display::Internal(InternalDisplay::TableRowGroup),
            DisplayKeyword::TableHeaderGroup => Display::Internal(InternalDisplay::TableHeaderGroup),
            DisplayKeyword::TableFooterGroup => Display::Internal(InternalDisplay::TableFooterGroup),
            DisplayKeyword::TableRow => Display::Internal(InternalDisplay::TableRow),
            DisplayKeyword::TableCell => Display::Internal(InternalDisplay::TableCell),
            DisplayKeyword::TableColumnGroup => Display::Internal(InternalDisplay::TableColumnGroup),
            DisplayKeyword::TableColumn => Display::Internal(InternalDisplay::TableColumn),
            DisplayKeyword::TableCaption => Display::Internal(InternalDisplay::TableCaption),
            _ => Display::INLINE,
        },
        CssDisplay::Pair(pair) => {
            let outside = match pair.outside {
                CssOutside::Block => DisplayOutside::Block,
                _ => DisplayOutside::Inline,
            };
            let inside = match pair.inside {
                CssInside::Flow => DisplayInside::Flow,
                CssInside::FlowRoot => DisplayInside::FlowRoot,
                CssInside::Table => DisplayInside::Table,
                CssInside::Flex(_) | CssInside::Box(_) => DisplayInside::Flex,
                CssInside::Grid => DisplayInside::Grid,
                CssInside::Ruby => DisplayInside::Flow,
            };
            Display::Normal {
                outside,
                inside,
                list_item: pair.is_list_item,
            }
        }
    }
}

/// The part `display` gives a box in ruby layout.
fn ruby_display(value: &CssDisplay) -> crate::values::RubyDisplay {
    use crate::values::RubyDisplay;
    match value {
        CssDisplay::Keyword(DisplayKeyword::RubyText) => RubyDisplay::Text,
        CssDisplay::Pair(pair) if matches!(pair.inside, CssInside::Ruby) && !pair.is_list_item => {
            RubyDisplay::Container
        }
        _ => RubyDisplay::None,
    }
}

fn border_style(value: LineStyle) -> BorderStyle {
    match value {
        LineStyle::None => BorderStyle::None,
        LineStyle::Hidden => BorderStyle::Hidden,
        LineStyle::Solid => BorderStyle::Solid,
        LineStyle::Dashed => BorderStyle::Dashed,
        LineStyle::Dotted => BorderStyle::Dotted,
        LineStyle::Double => BorderStyle::Double,
        LineStyle::Groove => BorderStyle::Groove,
        LineStyle::Ridge => BorderStyle::Ridge,
        LineStyle::Inset => BorderStyle::Inset,
        LineStyle::Outset => BorderStyle::Outset,
    }
}

/// What `match-parent` computes to with the parent's `align`: that alignment, its `start` and
/// `end` on the sides the parent's direction gives them; `start` of the element's own direction
/// at the root, which has no parent (CSS Text 3, §6.1; WPT
/// `text-align-match-parent-root-rtl`).
fn match_parent(parent: &ComputedValues, align: TextAlign, root: bool) -> TextAlign {
    if root {
        TextAlign::Start
    } else {
        parent.physical_align(align)
    }
}

fn border_width(context: &Context, value: &BorderSideWidth) -> f32 {
    snap_border_width(match value {
        BorderSideWidth::Thin => context.zoom,
        BorderSideWidth::Medium => 3.0 * context.zoom,
        BorderSideWidth::Thick => 5.0 * context.zoom,
        BorderSideWidth::Length(length) => context.length(length),
    })
}

/// A border or rule width in whole pixels, as browsers compute it: a width below one pixel
/// becomes one, a larger one is cut to whole pixels (CSS Values 4, §6, "snap as a border width",
/// with a pixel as the device pixel).
pub(crate) fn snap_border_width(width: f32) -> f32 {
    if width <= 0.0 {
        0.0
    } else if width < 1.0 {
        1.0
    } else {
        // A width that comes out whole stays whole despite rounding errors.
        (width + 1e-4).floor()
    }
}

fn scrollbar(value: OverflowKeyword) -> crate::values::Scrollbar {
    match value {
        OverflowKeyword::Auto => crate::values::Scrollbar::Auto,
        OverflowKeyword::Scroll => crate::values::Scrollbar::Scroll,
        _ => crate::values::Scrollbar::None,
    }
}

fn overflow(value: OverflowKeyword) -> Overflow {
    match value {
        OverflowKeyword::Visible => Overflow::Visible,
        OverflowKeyword::Clip => Overflow::Clip,
        OverflowKeyword::Hidden | OverflowKeyword::Scroll | OverflowKeyword::Auto => Overflow::Hidden,
    }
}

/// A flex factor as used: not negative, NaN as zero, infinity as the largest finite factor.
fn flex_factor(value: f32) -> f32 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(0.0, f32::MAX)
    }
}

fn content_alignment_justify(value: &JustifyContent) -> ContentAlignment {
    match value {
        JustifyContent::Normal => ContentAlignment::Normal,
        JustifyContent::ContentDistribution(distribution) => distribution_alignment(*distribution),
        JustifyContent::ContentPosition { value, .. } => position_alignment(*value),
        JustifyContent::Left { .. } => ContentAlignment::Left,
        JustifyContent::Right { .. } => ContentAlignment::Right,
    }
}

fn content_alignment_align(value: &AlignContent) -> ContentAlignment {
    match value {
        AlignContent::Normal => ContentAlignment::Normal,
        AlignContent::ContentDistribution(distribution) => distribution_alignment(*distribution),
        AlignContent::ContentPosition { value, .. } => position_alignment(*value),
        AlignContent::BaselinePosition(_) => ContentAlignment::Start,
    }
}

fn distribution_alignment(value: ContentDistribution) -> ContentAlignment {
    match value {
        ContentDistribution::SpaceBetween => ContentAlignment::SpaceBetween,
        ContentDistribution::SpaceAround => ContentAlignment::SpaceAround,
        ContentDistribution::SpaceEvenly => ContentAlignment::SpaceEvenly,
        ContentDistribution::Stretch => ContentAlignment::Stretch,
    }
}

fn position_alignment(value: ContentPosition) -> ContentAlignment {
    match value {
        ContentPosition::Center => ContentAlignment::Center,
        ContentPosition::Start => ContentAlignment::Start,
        ContentPosition::End => ContentAlignment::End,
        ContentPosition::FlexStart => ContentAlignment::FlexStart,
        ContentPosition::FlexEnd => ContentAlignment::FlexEnd,
    }
}

fn grid_line(line: &lightningcss::properties::grid::GridLine<'_>) -> GridLine {
    use lightningcss::properties::grid::GridLine as Css;
    let limit = i32::from(MAX_GRID_TRACKS);
    match line {
        Css::Line { index, name: None } if *index != 0 => GridLine::Line((*index).clamp(-limit, limit)),
        Css::Span { index, name: None } if *index > 0 => {
            GridLine::Span(index.unsigned_abs().min(u32::from(MAX_GRID_TRACKS)))
        }
        Css::Area { name } => GridLine::Area(Arc::from(name.0.as_ref())),
        Css::Line {
            index,
            name: Some(name),
        } if *index != 0 => GridLine::Named(Arc::from(name.0.as_ref()), (*index).clamp(-limit, limit)),
        Css::Span {
            index,
            name: Some(name),
        } if *index > 0 => GridLine::SpanNamed(
            Arc::from(name.0.as_ref()),
            index.unsigned_abs().min(u32::from(MAX_GRID_TRACKS)),
        ),
        _ => GridLine::Auto,
    }
}

/// The named areas of `grid-template-areas`: each name's rows and columns, if they make a
/// rectangle (lightningcss checks that they do).
fn grid_areas(value: &lightningcss::properties::grid::GridTemplateAreas) -> Option<Arc<GridAreas>> {
    use lightningcss::properties::grid::GridTemplateAreas as Css;
    let Css::Areas { columns, areas } = value else {
        return None;
    };
    let columns = (*columns as usize).max(1);
    let mut named: Vec<GridArea> = Vec::new();
    for (index, name) in areas.iter().enumerate() {
        let Some(name) = name else { continue };
        let (row, column) = (index / columns, index % columns);
        match named.iter_mut().find(|area| &*area.name == name.as_str()) {
            Some(area) => {
                area.rows = (area.rows.0.min(row), area.rows.1.max(row + 1));
                area.columns = (area.columns.0.min(column), area.columns.1.max(column + 1));
            }
            None => named.push(GridArea {
                name: Arc::from(name.as_str()),
                rows: (row, row + 1),
                columns: (column, column + 1),
            }),
        }
    }
    Some(Arc::new(GridAreas {
        rows: areas.len().div_ceil(columns),
        columns,
        areas: named,
    }))
}

fn self_alignment(value: SelfPosition) -> ItemAlignment {
    match value {
        SelfPosition::Center => ItemAlignment::Center,
        SelfPosition::Start | SelfPosition::SelfStart | SelfPosition::FlexStart => ItemAlignment::Start,
        SelfPosition::End | SelfPosition::SelfEnd | SelfPosition::FlexEnd => ItemAlignment::End,
    }
}

/// The flags of a self-alignment position and its overflow alignment.
fn self_flags(
    value: SelfPosition,
    overflow: Option<&lightningcss::properties::align::OverflowPosition>,
) -> SelfFlags {
    SelfFlags {
        safe: matches!(
            overflow,
            Some(lightningcss::properties::align::OverflowPosition::Safe)
        ),
        flex: matches!(value, SelfPosition::FlexStart | SelfPosition::FlexEnd),
    }
}

fn gap(context: &Context, value: &GapValue) -> Option<LengthPercentage> {
    match value {
        GapValue::Normal => None,
        GapValue::LengthPercentage(length) => Some(context.length_percentage(length)),
    }
}

fn list_style_type(value: &CssListStyleType<'_>) -> ListStyleType {
    match value {
        CssListStyleType::None => ListStyleType::None,
        CssListStyleType::String(string) => ListStyleType::String(string.to_string()),
        CssListStyleType::CounterStyle(style) => {
            let mut name = String::new();
            let mut printer = lightningcss::printer::Printer::new(&mut name, PrinterOptions::default());
            let _ = lightningcss::traits::ToCss::to_css(style, &mut printer);
            crate::extra::list_style_by_name(&name)
        }
    }
}

/// The font sizes of the keywords from `xx-small` to `xxx-large`, for the default size of 16px
/// and for the default fixed size of 13px that `monospace` text has in Chrome.
const KEYWORD_SIZES: [[f32; 8]; 2] = [
    [9.0, 10.0, 13.0, 16.0, 18.0, 24.0, 32.0, 48.0],
    [9.0, 10.0, 12.0, 13.0, 16.0, 20.0, 26.0, 39.0],
];

/// The size of a `font-size` keyword (`xx-small` = 0), in `monospace` or another family.
pub(crate) fn keyword_font_size(keyword: u8, monospace: bool) -> f32 {
    KEYWORD_SIZES[usize::from(monospace)][usize::from(keyword.min(7))]
}

/// The computed font size from a declared `font-size` and where it comes from, for text in
/// `monospace` alone or not.
pub(crate) fn font_size(context: &Context, value: &FontSize, monospace: bool) -> (f32, FontSizeBasis) {
    let relative = FontSizeBasis {
        keyword: None,
        absolute: context.parent_font_size_absolute,
        monospace,
    };
    match value {
        FontSize::Length(length) => {
            let size = context.font_length_percentage(length);
            // Sizes in em or percent of the parent's keep its basis; other lengths are absolute.
            let parent_relative = matches!(
                length,
                DimensionPercentage::Percentage(_)
                    | DimensionPercentage::Dimension(
                        LengthValue::Em(_) | LengthValue::Ex(_) | LengthValue::Ch(_)
                    )
            );
            let basis = if parent_relative {
                relative
            } else {
                FontSizeBasis {
                    keyword: None,
                    absolute: true,
                    monospace,
                }
            };
            (size, basis)
        }
        FontSize::Absolute(size) => {
            let keyword = match size {
                AbsoluteFontSize::XXSmall => 0,
                AbsoluteFontSize::XSmall => 1,
                AbsoluteFontSize::Small => 2,
                AbsoluteFontSize::Medium => 3,
                AbsoluteFontSize::Large => 4,
                AbsoluteFontSize::XLarge => 5,
                AbsoluteFontSize::XXLarge => 6,
                AbsoluteFontSize::XXXLarge => 7,
            };
            (
                keyword_font_size(keyword, monospace) * context.zoom,
                FontSizeBasis {
                    keyword: Some(keyword),
                    absolute: false,
                    monospace,
                },
            )
        }
        FontSize::Relative(RelativeFontSize::Smaller) => (context.parent_font_size / 1.2, relative),
        FontSize::Relative(RelativeFontSize::Larger) => (context.parent_font_size * 1.2, relative),
    }
}

/// Whether a `font-family` list is the generic `monospace` alone, the family whose text Chrome
/// sizes from its default fixed font size.
pub(crate) fn is_monospace(families: &[FontFamily<'_>]) -> bool {
    matches!(families, [FontFamily::Generic(GenericFontFamily::Monospace)])
}

/// Applies one longhand declaration to `style`. `font-size` must be applied first, because other
/// properties resolve `em` against it.
/// A `letter-spacing` or `word-spacing` with a percentage (of the font size), as pixels and the
/// fraction of the font size: `calc(0.05em + 5%)` at 20px is `(1.0, 0.05)`.
fn spacing_percentage(property: &Property<'static>, context: &Context) -> Option<(f32, f32)> {
    use lightningcss::traits::Parse;
    let text = property.value_to_css_string(PrinterOptions::default()).ok()?;
    let value = CssLengthPercentage::parse_string(text.trim()).ok()?;
    let value = context.length_percentage(&value);
    let pixels = value.resolve(0.0);
    let fraction = (value.resolve(100.0) - pixels) / 100.0;
    (pixels.is_finite() && fraction.is_finite()).then_some((pixels, fraction))
}

pub(crate) fn apply(
    property: &Property<'static>,
    style: &mut ComputedValues,
    parent: &ComputedValues,
    context: &Context,
) {
    let lp = |value: &CssLengthPercentage| context.length_percentage(value);
    let lpa = |value: &LengthPercentageOrAuto| context.length_percentage_auto(value);
    match property {
        Property::Display(value) => {
            style.display = display(value);
            style.ruby_display = ruby_display(value);
            style.legacy_box.is_box = matches!(
                value,
                CssDisplay::Pair(pair)
                    if matches!(pair.inside, CssInside::Box(lightningcss::vendor_prefix::VendorPrefix::WebKit))
            );
        }
        Property::BoxFlex(value, _) => style.legacy_box.flex = *value,
        Property::BoxOrdinalGroup(value, _) => style.legacy_box.ordinal_group = (*value).max(1),
        Property::BoxPack(value, _) => {
            use crate::webkit_box::BoxPack;
            use lightningcss::properties::flex::BoxPack as Pack;
            style.legacy_box.pack = match value {
                Pack::Start => BoxPack::Start,
                Pack::End => BoxPack::End,
                Pack::Center => BoxPack::Center,
                Pack::Justify => BoxPack::Justify,
            };
        }
        Property::BoxAlign(value, _) => {
            use crate::webkit_box::BoxAlign;
            use lightningcss::properties::flex::BoxAlign as Align;
            style.legacy_box.align = match value {
                Align::Start => BoxAlign::Start,
                Align::End => BoxAlign::End,
                Align::Center => BoxAlign::Center,
                Align::Baseline => BoxAlign::Baseline,
                Align::Stretch => BoxAlign::Stretch,
            };
        }
        Property::BoxDirection(value, _) => {
            style.legacy_box.reverse = *value == lightningcss::properties::flex::BoxDirection::Reverse;
        }
        Property::TextRendering(value) => {
            style.text_rendering_speed =
                *value == lightningcss::properties::svg::TextRendering::OptimizeSpeed;
        }
        Property::ImageRendering(value) => {
            style.image_rendering = match value {
                lightningcss::properties::svg::ImageRendering::OptimizeSpeed => {
                    crate::values::ImageRendering::Pixelated
                }
                _ => crate::values::ImageRendering::Auto,
            };
        }
        // Sizes of `calc-size()`, which layout resolves.
        Property::Unparsed(unparsed) if fagus_css::is_calc_size(unparsed) => {
            let id = match unparsed.property_id {
                PropertyId::InlineSize if context.writing_mode.is_vertical() => PropertyId::Height,
                PropertyId::InlineSize => PropertyId::Width,
                PropertyId::BlockSize if context.writing_mode.is_vertical() => PropertyId::Width,
                PropertyId::BlockSize => PropertyId::Height,
                ref other => other.clone(),
            };
            crate::calc_size::apply(&id, &unparsed.value, style, context);
        }
        // `clip-path: path()`, which lightningcss does not know.
        Property::Unparsed(unparsed)
            if let Some((even_odd, data)) = fagus_css::clip_path_function(unparsed) =>
        {
            style.clip_path = Some(Arc::new(crate::values::ClipPath {
                shape: crate::values::ClipShape::Path {
                    even_odd,
                    data: Arc::from(data),
                },
                reference: crate::values::BoxArea::BorderBox,
                svg_reference: crate::values::TransformBox::StrokeBox,
            }));
        }
        // `clip-path: xywh()`, `rect()` and `shape()`, which lightningcss does not know either;
        // one that does not parse leaves the property as it was.
        Property::Unparsed(unparsed) if fagus_css::is_newer_clip_shape(unparsed) => {
            if let Some(clip_path) = crate::shape::newer_clip_path(&unparsed.value, context) {
                style.clip_path = Some(clip_path);
            }
        }
        // `flow-start` and `flow-end` of `align-content` and `justify-content`.
        Property::Unparsed(unparsed) if crate::block_align::apply_flow_content(unparsed, style) => {}
        // Anchor positioning, which layout resolves.
        Property::Unparsed(unparsed) if fagus_css::is_anchored(unparsed) => {
            crate::anchor::apply_unparsed(&unparsed.property_id, &unparsed.value, style, context);
        }
        // `pixelated` and `crisp-edges`, which lightningcss knows only for SVG.
        // `-webkit-baseline-middle`, which lightningcss does not know.
        Property::Unparsed(unparsed)
            if unparsed.property_id == PropertyId::VerticalAlign
                && unparsed.value.0.iter().any(|token| {
                    matches!(
                        token,
                        lightningcss::properties::custom::TokenOrValue::Token(
                            lightningcss::properties::custom::Token::Ident(ident)
                        ) if ident.eq_ignore_ascii_case("-webkit-baseline-middle")
                    )
                }) =>
        {
            style.vertical_align = VerticalAlign::BaselineMiddle;
        }
        // A string, which lightningcss does not know.
        Property::Unparsed(unparsed) if matches!(unparsed.property_id, PropertyId::TextOverflow(_)) => {
            if let Some(text) = text_overflow_string(&unparsed.value) {
                style.text_overflow = TextOverflow::String(text);
            }
        }
        // `flex-wrap: balance`, which lightningcss does not know.
        Property::Unparsed(unparsed)
            if matches!(unparsed.property_id, PropertyId::FlexWrap(_))
                && let Some(reverse) = fagus_css::flex_wrap_balance(&unparsed.value) =>
        {
            style.flex_wrap = if reverse {
                FlexWrap::WrapReverse
            } else {
                FlexWrap::Wrap
            };
            style.flex_balance = true;
        }
        // `auto-phrase`, which lightningcss does not know.
        Property::Unparsed(unparsed) if unparsed.property_id == PropertyId::WordBreak => {
            let mut idents = unparsed.value.0.iter().filter_map(|token| match token {
                lightningcss::properties::custom::TokenOrValue::Token(
                    lightningcss::properties::custom::Token::Ident(ident),
                ) => Some(ident.to_ascii_lowercase()),
                lightningcss::properties::custom::TokenOrValue::Token(
                    lightningcss::properties::custom::Token::WhiteSpace(_),
                ) => None,
                _ => Some(String::new()),
            });
            if let (Some("auto-phrase"), None) = (idents.next().as_deref(), idents.next()) {
                style.word_break = WordBreak::AutoPhrase;
            }
        }
        // lightningcss does not parse `preserve-3d`.
        Property::Unparsed(unparsed) if matches!(unparsed.property_id, PropertyId::TransformStyle(_)) => {
            style.preserve_3d = unparsed.value.0.iter().any(|token| {
                matches!(token, lightningcss::properties::custom::TokenOrValue::Token(
                    lightningcss::properties::custom::Token::Ident(ident),
                ) if ident.eq_ignore_ascii_case("preserve-3d"))
            });
        }
        // `math-auto`, which lightningcss does not know.
        Property::Unparsed(unparsed) if unparsed.property_id == PropertyId::TextTransform => {
            let mut idents = unparsed.value.0.iter().filter_map(|token| match token {
                lightningcss::properties::custom::TokenOrValue::Token(
                    lightningcss::properties::custom::Token::Ident(ident),
                ) => Some(ident.to_ascii_lowercase()),
                lightningcss::properties::custom::TokenOrValue::Token(
                    lightningcss::properties::custom::Token::WhiteSpace(_),
                ) => None,
                _ => Some(String::new()),
            });
            if let (Some("math-auto"), None) = (idents.next().as_deref(), idents.next()) {
                style.text_transform = TextTransform::MathAuto;
            }
        }
        Property::Unparsed(unparsed) if unparsed.property_id == PropertyId::ImageRendering => {
            let keyword = unparsed.value.0.iter().find_map(|token| match token {
                lightningcss::properties::custom::TokenOrValue::Token(
                    lightningcss::properties::custom::Token::Ident(ident),
                ) => Some(ident.to_ascii_lowercase()),
                _ => None,
            });
            match keyword.as_deref() {
                Some("pixelated" | "crisp-edges") => {
                    style.image_rendering = crate::values::ImageRendering::Pixelated;
                }
                Some("smooth" | "high-quality" | "auto") => {
                    style.image_rendering = crate::values::ImageRendering::Auto;
                }
                _ => {}
            }
        }
        Property::Unparsed(unparsed)
            if matches!(
                unparsed.property_id,
                PropertyId::GridTemplateColumns | PropertyId::GridTemplateRows
            ) =>
        {
            // `subgrid` with the names of its lines.
            if let Some(lines) = fagus_css::paged::subgrid_lines(&unparsed.value) {
                let numbered = |lists: &[Vec<String>]| -> Vec<(usize, Arc<str>)> {
                    lists
                        .iter()
                        .enumerate()
                        .flat_map(|(line, names)| {
                            names.iter().map(move |name| (line, Arc::from(name.as_str())))
                        })
                        .collect()
                };
                // `repeat(auto-fill, …)` repeats its lists as often as the lines the subgrid spans
                // leave room for.
                let auto_repeat = lines.repeated.as_ref().map(|repeated| AutoRepeat {
                    at: lines.before.len(),
                    tracks: repeated.iter().map(|_| TrackSize::Auto).collect(),
                    fit: false,
                    names: numbered(repeated).into(),
                    after: numbered(&lines.after).into(),
                    after_lists: lines.after.len(),
                });
                let list = TrackList {
                    names: numbered(&lines.before).into(),
                    subgrid: true,
                    auto_repeat,
                    ..TrackList::default()
                };
                if unparsed.property_id == PropertyId::GridTemplateColumns {
                    style.grid_template_columns = list;
                } else {
                    style.grid_template_rows = list;
                }
            }
        }
        // Sizing keywords, which lightningcss does not take in `flex-basis`: `content` sizes the
        // item by its content, like `max-content`.
        Property::Unparsed(unparsed) if matches!(unparsed.property_id, PropertyId::FlexBasis(_)) => {
            use lightningcss::properties::custom::{Token, TokenOrValue};
            let mut tokens = unparsed
                .value
                .0
                .iter()
                .filter(|token| !matches!(token, TokenOrValue::Token(Token::WhiteSpace(_))));
            let keyword = match (tokens.next(), tokens.next()) {
                (Some(TokenOrValue::Token(Token::Ident(ident))), None) => Some(ident.to_ascii_lowercase()),
                _ => None,
            };
            if let Some(basis) = keyword.as_deref().and_then(|keyword| match keyword {
                "content" | "max-content" => Some(Size::MaxContent),
                "min-content" => Some(Size::MinContent),
                "fit-content" => Some(Size::FitContent(None)),
                "stretch" | "-webkit-fill-available" => Some(Size::Stretch),
                _ => None,
            }) {
                style.flex_basis = basis;
            }
        }
        // `-webkit-match-parent`, and `-internal-center` of `th`: centred unless an ancestor
        // sets an alignment other than `start`, which the cell then takes (as Chrome does).
        // `-webkit-left`, `-webkit-center` and `-webkit-right` align text like `left`, `center`
        // and `right`, and block-level children too.
        Property::Unparsed(unparsed) if unparsed.property_id == PropertyId::TextAlign => {
            let keyword = unparsed.value.0.iter().find_map(|token| match token {
                lightningcss::properties::custom::TokenOrValue::Token(
                    lightningcss::properties::custom::Token::Ident(ident),
                ) => Some(ident.to_ascii_lowercase()),
                _ => None,
            });
            match keyword.as_deref() {
                Some("-webkit-match-parent") => {
                    style.text_align = match_parent(parent, parent.text_align, context.root);
                    style.align_blocks = false;
                }
                Some("-internal-center") => {
                    (style.text_align, style.align_blocks) = match parent.text_align {
                        TextAlign::Start => (TextAlign::Center, false),
                        align => (align, parent.align_blocks),
                    };
                }
                Some("-webkit-left") => (style.text_align, style.align_blocks) = (TextAlign::Left, true),
                Some("-webkit-center") => (style.text_align, style.align_blocks) = (TextAlign::Center, true),
                Some("-webkit-right") => (style.text_align, style.align_blocks) = (TextAlign::Right, true),
                _ => {}
            }
        }
        Property::Unparsed(unparsed) if unparsed.property_id == PropertyId::Position => {
            // `running(name)`: out of the flow, shown in the page margins (CSS GCPM 3, §2.1).
            style.position = Position::Static;
            style.running = fagus_css::paged::running_name(&unparsed.value).map(Into::into);
        }
        Property::Position(value) => {
            style.running = None;
            style.position = match value {
                CssPosition::Static => Position::Static,
                CssPosition::Relative => Position::Relative,
                CssPosition::Absolute => Position::Absolute,
                CssPosition::Fixed => Position::Fixed,
                CssPosition::Sticky(_) => Position::Sticky,
            };
        }
        Property::Top(value) => style.inset.top = lpa(value),
        Property::Right(value) => style.inset.right = lpa(value),
        Property::Bottom(value) => style.inset.bottom = lpa(value),
        Property::Left(value) => style.inset.left = lpa(value),
        Property::ZIndex(value) => {
            style.z_index = match value {
                ZIndex::Auto => None,
                ZIndex::Integer(integer) => Some(*integer),
            }
        }
        Property::Resize(value) => {
            style.resize = *value != lightningcss::properties::ui::Resize::None;
        }
        Property::BoxSizing(value, _) => {
            style.box_sizing = match value {
                CssBoxSizing::ContentBox => BoxSizing::ContentBox,
                CssBoxSizing::BorderBox => BoxSizing::BorderBox,
            }
        }
        Property::Width(value) => style.width = context.size(value),
        Property::Height(value) => style.height = context.size(value),
        Property::MinWidth(value) => style.min_width = context.size(value),
        Property::MinHeight(value) => style.min_height = context.size(value),
        Property::MaxWidth(value) => style.max_width = context.max_size(value),
        Property::MaxHeight(value) => style.max_height = context.max_size(value),
        Property::AspectRatio(value) => {
            style.aspect_ratio_auto = value.auto;
            // A ratio with a term that Blink's layout units (1/64 px) round to zero is degenerate,
            // as `auto` (WPT `css-sizing/aspect-ratio/zero-or-infinity-006`): one of `0/1` would
            // make a box infinitely tall.
            style.aspect_ratio = value
                .ratio
                .as_ref()
                .filter(|ratio| ratio.0 >= 1.0 / 128.0 && ratio.1 >= 1.0 / 128.0)
                .map(|ratio| ratio.0 / ratio.1);
        }
        Property::MarginTop(value) => style.margin.top = lpa(value),
        Property::MarginRight(value) => style.margin.right = lpa(value),
        Property::MarginBottom(value) => style.margin.bottom = lpa(value),
        Property::MarginLeft(value) => style.margin.left = lpa(value),
        // Logical properties, on the sides the element's writing mode and direction give them.
        Property::MarginBlockStart(value) => *logical(&mut style.margin, context, BLOCK_START) = lpa(value),
        Property::MarginBlockEnd(value) => *logical(&mut style.margin, context, BLOCK_END) = lpa(value),
        Property::MarginInlineStart(value) => *logical(&mut style.margin, context, INLINE_START) = lpa(value),
        Property::MarginInlineEnd(value) => *logical(&mut style.margin, context, INLINE_END) = lpa(value),
        Property::PaddingBlockStart(value) => {
            *logical(&mut style.padding, context, BLOCK_START) = padding(context, value);
        }
        Property::PaddingBlockEnd(value) => {
            *logical(&mut style.padding, context, BLOCK_END) = padding(context, value);
        }
        Property::PaddingInlineStart(value) => {
            *logical(&mut style.padding, context, INLINE_START) = padding(context, value);
        }
        Property::PaddingInlineEnd(value) => {
            *logical(&mut style.padding, context, INLINE_END) = padding(context, value);
        }
        Property::InsetBlockStart(value) => *logical(&mut style.inset, context, BLOCK_START) = lpa(value),
        Property::InsetBlockEnd(value) => *logical(&mut style.inset, context, BLOCK_END) = lpa(value),
        Property::InsetInlineStart(value) => *logical(&mut style.inset, context, INLINE_START) = lpa(value),
        Property::InsetInlineEnd(value) => *logical(&mut style.inset, context, INLINE_END) = lpa(value),
        Property::InlineSize(value) if context.writing_mode.is_vertical() => {
            style.height = context.size(value);
        }
        Property::BlockSize(value) if context.writing_mode.is_vertical() => style.width = context.size(value),
        Property::MinInlineSize(value) if context.writing_mode.is_vertical() => {
            style.min_height = context.size(value);
        }
        Property::MinBlockSize(value) if context.writing_mode.is_vertical() => {
            style.min_width = context.size(value);
        }
        Property::MaxInlineSize(value) if context.writing_mode.is_vertical() => {
            style.max_height = context.max_size(value);
        }
        Property::MaxBlockSize(value) if context.writing_mode.is_vertical() => {
            style.max_width = context.max_size(value);
        }
        Property::InlineSize(value) => style.width = context.size(value),
        Property::BlockSize(value) => style.height = context.size(value),
        Property::MinInlineSize(value) => style.min_width = context.size(value),
        Property::MinBlockSize(value) => style.min_height = context.size(value),
        Property::MaxInlineSize(value) => style.max_width = context.max_size(value),
        Property::MaxBlockSize(value) => style.max_height = context.max_size(value),
        Property::BorderBlockStartWidth(value) => {
            *logical(&mut style.border_width, context, BLOCK_START) = border_width(context, value);
        }
        Property::BorderBlockEndWidth(value) => {
            *logical(&mut style.border_width, context, BLOCK_END) = border_width(context, value);
        }
        Property::BorderInlineStartWidth(value) => {
            *logical(&mut style.border_width, context, INLINE_START) = border_width(context, value);
        }
        Property::BorderInlineEndWidth(value) => {
            *logical(&mut style.border_width, context, INLINE_END) = border_width(context, value);
        }
        Property::BorderBlockStartStyle(value) => {
            *logical(&mut style.border_style, context, BLOCK_START) = border_style(*value);
        }
        Property::BorderBlockEndStyle(value) => {
            *logical(&mut style.border_style, context, BLOCK_END) = border_style(*value);
        }
        Property::BorderInlineStartStyle(value) => {
            *logical(&mut style.border_style, context, INLINE_START) = border_style(*value);
        }
        Property::BorderInlineEndStyle(value) => {
            *logical(&mut style.border_style, context, INLINE_END) = border_style(*value);
        }
        Property::BorderBlockStartColor(value) => {
            *logical(&mut style.border_color, context, BLOCK_START) = color(value);
        }
        Property::BorderBlockEndColor(value) => {
            *logical(&mut style.border_color, context, BLOCK_END) = color(value);
        }
        Property::BorderInlineStartColor(value) => {
            *logical(&mut style.border_color, context, INLINE_START) = color(value);
        }
        Property::BorderInlineEndColor(value) => {
            *logical(&mut style.border_color, context, INLINE_END) = color(value);
        }
        // The start-start corner is where the block-start and the inline-start sides meet: the
        // top left one in horizontal left-to-right text, the top right one in right-to-left text.
        Property::BorderStartStartRadius(value) => {
            *corner(&mut style.border_radius, context, BLOCK_START, INLINE_START) =
                (lp(&value.0), lp(&value.1));
        }
        Property::BorderStartEndRadius(value) => {
            *corner(&mut style.border_radius, context, BLOCK_START, INLINE_END) =
                (lp(&value.0), lp(&value.1));
        }
        Property::BorderEndEndRadius(value) => {
            *corner(&mut style.border_radius, context, BLOCK_END, INLINE_END) = (lp(&value.0), lp(&value.1));
        }
        Property::BorderEndStartRadius(value) => {
            *corner(&mut style.border_radius, context, BLOCK_END, INLINE_START) =
                (lp(&value.0), lp(&value.1));
        }
        Property::PaddingTop(value) => style.padding.top = padding(context, value),
        Property::PaddingRight(value) => style.padding.right = padding(context, value),
        Property::PaddingBottom(value) => style.padding.bottom = padding(context, value),
        Property::PaddingLeft(value) => style.padding.left = padding(context, value),
        Property::BorderTopWidth(value) => style.border_width.top = border_width(context, value),
        Property::BorderRightWidth(value) => style.border_width.right = border_width(context, value),
        Property::BorderBottomWidth(value) => style.border_width.bottom = border_width(context, value),
        Property::BorderLeftWidth(value) => style.border_width.left = border_width(context, value),
        Property::BorderTopStyle(value) => style.border_style.top = border_style(*value),
        Property::BorderRightStyle(value) => style.border_style.right = border_style(*value),
        Property::BorderBottomStyle(value) => style.border_style.bottom = border_style(*value),
        Property::BorderLeftStyle(value) => style.border_style.left = border_style(*value),
        Property::BorderTopColor(value) => style.border_color.top = color(value),
        Property::BorderRightColor(value) => style.border_color.right = color(value),
        Property::BorderBottomColor(value) => style.border_color.bottom = color(value),
        Property::BorderLeftColor(value) => style.border_color.left = color(value),
        Property::BorderTopLeftRadius(value, _) => {
            style.border_radius.top_left = (lp(&value.0), lp(&value.1));
        }
        Property::BorderTopRightRadius(value, _) => {
            style.border_radius.top_right = (lp(&value.0), lp(&value.1));
        }
        Property::BorderBottomRightRadius(value, _) => {
            style.border_radius.bottom_right = (lp(&value.0), lp(&value.1));
        }
        Property::BorderBottomLeftRadius(value, _) => {
            style.border_radius.bottom_left = (lp(&value.0), lp(&value.1));
        }
        Property::OverflowX(value) => {
            style.overflow_x = overflow(*value);
            style.scrollbar_x = scrollbar(*value);
        }
        Property::OverflowY(value) => {
            style.overflow_y = overflow(*value);
            style.scrollbar_y = scrollbar(*value);
        }
        Property::Visibility(value) => {
            style.visibility = match value {
                CssVisibility::Visible => Visibility::Visible,
                CssVisibility::Hidden => Visibility::Hidden,
                CssVisibility::Collapse => Visibility::Collapse,
            }
        }
        Property::Appearance(value, _) => {
            style.appearance = match value {
                lightningcss::properties::ui::Appearance::None => Appearance::None,
                _ => Appearance::Auto,
            }
        }
        Property::ColorScheme(scheme) => {
            use lightningcss::properties::ui::ColorScheme;
            style.dark = scheme.contains(ColorScheme::Dark) && !scheme.contains(ColorScheme::Light);
        }
        Property::AccentColor(value) => {
            style.accent_color = match value {
                lightningcss::properties::ui::ColorOrAuto::Auto => None,
                lightningcss::properties::ui::ColorOrAuto::Color(value) => {
                    color(value).or(style.accent_color)
                }
            }
        }
        Property::Opacity(value) => style.opacity = value.0.clamp(0.0, 1.0),
        Property::ClipPath(value, _) => style.clip_path = context.clip_path(value),
        Property::Filter(list, _) => style.filter = context.filters(list),
        Property::BorderImageSource(image) => {
            border_image(style).source = background::image(image, context, None);
        }
        Property::BorderImageSlice(slice) => {
            use crate::values::ImageSlice;
            let side = |value: &NumberOrPercentage| match value {
                NumberOrPercentage::Number(number) => ImageSlice::Number(number.max(0.0)),
                NumberOrPercentage::Percentage(percentage) => ImageSlice::Percent(percentage.0.max(0.0)),
            };
            let offsets = &slice.offsets;
            let image = border_image(style);
            image.slice = [
                side(&offsets.0),
                side(&offsets.1),
                side(&offsets.2),
                side(&offsets.3),
            ];
            image.fill = slice.fill;
        }
        Property::BorderImageWidth(widths) => {
            use crate::values::BorderImageWidth;
            use lightningcss::properties::border_image::BorderImageSideWidth;
            let side = |value: &BorderImageSideWidth| match value {
                BorderImageSideWidth::Number(number) => BorderImageWidth::Number(number.max(0.0)),
                BorderImageSideWidth::LengthPercentage(length) => {
                    BorderImageWidth::Length(context.length_percentage(length))
                }
                BorderImageSideWidth::Auto => BorderImageWidth::Auto,
            };
            border_image(style).width = [side(&widths.0), side(&widths.1), side(&widths.2), side(&widths.3)];
        }
        Property::BorderImageOutset(outsets) => {
            use crate::values::BorderImageOutset;
            use lightningcss::values::length::LengthOrNumber;
            let side = |value: &LengthOrNumber| match value {
                LengthOrNumber::Number(number) => BorderImageOutset::Number(number.max(0.0)),
                LengthOrNumber::Length(length) => BorderImageOutset::Length(context.length(length).max(0.0)),
            };
            border_image(style).outset = [
                side(&outsets.0),
                side(&outsets.1),
                side(&outsets.2),
                side(&outsets.3),
            ];
        }
        Property::BorderImageRepeat(repeat) => {
            use crate::values::BorderImageRepeat;
            use lightningcss::properties::border_image::BorderImageRepeatKeyword;
            let keyword = |value: &BorderImageRepeatKeyword| match value {
                BorderImageRepeatKeyword::Stretch => BorderImageRepeat::Stretch,
                BorderImageRepeatKeyword::Repeat => BorderImageRepeat::Repeat,
                BorderImageRepeatKeyword::Round => BorderImageRepeat::Round,
                BorderImageRepeatKeyword::Space => BorderImageRepeat::Space,
            };
            border_image(style).repeat = (keyword(&repeat.horizontal), keyword(&repeat.vertical));
        }
        Property::MaskImage(images, _) => {
            mask_layers(style).images = images
                .iter()
                .map(|image| background::image(image, context, None))
                .collect();
        }
        Property::MaskMode(modes) => {
            use lightningcss::properties::masking::MaskMode as Css;
            mask_layers(style).modes = modes
                .iter()
                .map(|mode| match mode {
                    Css::Alpha => crate::values::MaskMode::Alpha,
                    Css::Luminance => crate::values::MaskMode::Luminance,
                    Css::MatchSource => crate::values::MaskMode::MatchSource,
                })
                .collect();
        }
        Property::MaskRepeat(repeats, _) => {
            mask_layers(style).repeats = repeats.iter().map(background::repeat).collect();
        }
        Property::MaskPositionX(positions) => {
            mask_layers(style).position_x = positions
                .iter()
                .map(|x| background::component(x, context, |side| *side == HorizontalPositionKeyword::Right))
                .collect();
        }
        Property::MaskPositionY(positions) => {
            mask_layers(style).position_y = positions
                .iter()
                .map(|y| background::component(y, context, |side| *side == VerticalPositionKeyword::Bottom))
                .collect();
        }
        Property::MaskPosition(positions, _) => {
            let (x, y): (Vec<_>, Vec<_>) = positions
                .iter()
                .map(|position| background::position(position, context))
                .unzip();
            let mask = mask_layers(style);
            mask.position_x = x.into();
            mask.position_y = y.into();
        }
        Property::MaskClip(clips, _) => {
            use lightningcss::properties::masking::MaskClip;
            let mask = mask_layers(style);
            mask.clips = clips
                .iter()
                .map(|clip| match clip {
                    MaskClip::GeometryBox(geometry) => Some(geometry_box(geometry)),
                    MaskClip::NoClip => None,
                })
                .collect();
            mask.svg_clips = clips
                .iter()
                .map(|clip| match clip {
                    MaskClip::GeometryBox(geometry) => Some(svg_geometry_box(geometry)),
                    MaskClip::NoClip => None,
                })
                .collect();
        }
        Property::MaskOrigin(origins, _) => {
            let mask = mask_layers(style);
            mask.origins = origins.iter().map(geometry_box).collect();
            mask.svg_origins = origins.iter().map(svg_geometry_box).collect();
        }
        Property::MaskSize(sizes, _) => {
            mask_layers(style).sizes = sizes.iter().map(|size| background::size(size, context)).collect();
        }
        Property::MaskComposite(composites) => {
            use crate::values::MaskComposite;
            use lightningcss::properties::masking::MaskComposite as Css;
            mask_layers(style).composites = composites
                .iter()
                .map(|composite| match composite {
                    Css::Add => MaskComposite::Add,
                    Css::Subtract => MaskComposite::Subtract,
                    Css::Intersect => MaskComposite::Intersect,
                    Css::Exclude => MaskComposite::Exclude,
                })
                .collect();
        }
        Property::WebKitMaskComposite(composites) => {
            use crate::values::MaskComposite;
            use lightningcss::properties::masking::WebKitMaskComposite as Css;
            // The operators of `-webkit-mask-composite` that `mask-composite` has.
            mask_layers(style).composites = composites
                .iter()
                .map(|composite| match composite {
                    Css::SourceIn | Css::DestinationIn => MaskComposite::Intersect,
                    Css::SourceOut | Css::DestinationOut => MaskComposite::Subtract,
                    Css::Xor => MaskComposite::Exclude,
                    _ => MaskComposite::Add,
                })
                .collect();
        }
        Property::MixBlendMode(value) => {
            use lightningcss::traits::ToCss;
            if let Some(mode) = value
                .to_css_string(PrinterOptions::default())
                .ok()
                .and_then(|keyword| crate::values::BlendMode::of(&keyword))
            {
                style.mix_blend_mode = mode;
            }
        }
        Property::Color(value) => style.color = color(value).unwrap_or(parent.color),
        Property::BackgroundColor(value) => style.background_color = color(value).unwrap_or(style.color),
        Property::FontFamily(families) => {
            style.font_family = families
                .iter()
                .map(|family| match family {
                    FontFamily::FamilyName(name) => family_name(name),
                    FontFamily::Generic(generic) => generic_family(*generic).to_owned(),
                })
                .collect();
        }
        Property::FontSize(_) => {}
        Property::FontWeight(value) => {
            style.font_weight = match value {
                FontWeight::Absolute(AbsoluteFontWeight::Weight(weight)) => *weight,
                FontWeight::Absolute(AbsoluteFontWeight::Normal) => 400.0,
                FontWeight::Absolute(AbsoluteFontWeight::Bold) => 700.0,
                FontWeight::Bolder => bolder(parent.font_weight),
                FontWeight::Lighter => lighter(parent.font_weight),
            }
        }
        Property::FontVariantCaps(value) => {
            use lightningcss::properties::font::FontVariantCaps as Caps;
            style.font_variant.caps = match value {
                Caps::Normal => crate::values::FontVariantCaps::Normal,
                Caps::SmallCaps => crate::values::FontVariantCaps::SmallCaps,
                Caps::AllSmallCaps => crate::values::FontVariantCaps::AllSmallCaps,
                Caps::PetiteCaps => crate::values::FontVariantCaps::PetiteCaps,
                Caps::AllPetiteCaps => crate::values::FontVariantCaps::AllPetiteCaps,
                Caps::Unicase => crate::values::FontVariantCaps::Unicase,
                Caps::TitlingCaps => crate::values::FontVariantCaps::TitlingCaps,
            };
        }
        Property::FontStretch(value) => {
            let lightningcss::values::percentage::Percentage(stretch) = value.into();
            style.font_stretch = stretch * 100.0;
        }
        Property::FontStyle(value) => {
            style.font_style = match value {
                CssFontStyle::Normal => FontStyle::Normal,
                CssFontStyle::Italic => FontStyle::Italic,
                CssFontStyle::Oblique(angle) => FontStyle::oblique(angle.to_degrees()),
            }
        }
        Property::LineHeight(value) => {
            style.line_height = match value {
                CssLineHeight::Normal => LineHeight::Normal,
                CssLineHeight::Number(number) => LineHeight::Number(*number),
                CssLineHeight::Length(length) => LineHeight::Px(lp(length).resolve(style.font_size)),
            }
        }
        Property::LetterSpacing(value) => {
            style.letter_spacing = match value {
                Spacing::Normal => 0.0,
                Spacing::Length(length) => context.length(length),
            };
            style.spacing_percentages[0] = None;
        }
        Property::WordSpacing(value) => {
            style.word_spacing = match value {
                Spacing::Normal => 0.0,
                Spacing::Length(length) => context.length(length),
            };
            style.spacing_percentages[1] = None;
        }
        // Percentages, which lightningcss does not parse for these.
        Property::Unparsed(unparsed)
            if matches!(
                unparsed.property_id,
                PropertyId::LetterSpacing | PropertyId::WordSpacing
            ) =>
        {
            if let Some(parts) = spacing_percentage(property, context) {
                let index = usize::from(unparsed.property_id == PropertyId::WordSpacing);
                style.spacing_percentages[index] = Some(parts);
            }
        }
        Property::Direction(value) => {
            style.direction = match value {
                CssDirection::Ltr => Direction::Ltr,
                CssDirection::Rtl => Direction::Rtl,
            }
        }
        Property::UnicodeBidi(value) => {
            style.unicode_bidi = match value {
                CssUnicodeBidi::Normal => UnicodeBidi::Normal,
                CssUnicodeBidi::Embed => UnicodeBidi::Embed,
                CssUnicodeBidi::Isolate => UnicodeBidi::Isolate,
                CssUnicodeBidi::BidiOverride => UnicodeBidi::BidiOverride,
                CssUnicodeBidi::IsolateOverride => UnicodeBidi::IsolateOverride,
                CssUnicodeBidi::Plaintext => UnicodeBidi::Plaintext,
            }
        }
        Property::TextAlign(value) => {
            style.align_blocks = false;
            style.text_align = match value {
                CssTextAlign::Start => TextAlign::Start,
                // The parent's `start` or `end`, on the side its direction gives them.
                CssTextAlign::MatchParent => match_parent(parent, parent.text_align, context.root),
                CssTextAlign::End => TextAlign::End,
                CssTextAlign::Left => TextAlign::Left,
                CssTextAlign::Right => TextAlign::Right,
                CssTextAlign::Center => TextAlign::Center,
                CssTextAlign::Justify => TextAlign::Justify,
                // Justified, the last line too.
                CssTextAlign::JustifyAll => {
                    style.text_align_last = Some(TextAlign::Justify);
                    TextAlign::Justify
                }
            }
        }
        Property::TabSize(value, _) => {
            style.tab_size = match value {
                LengthOrNumber::Number(spaces) => TabSize::Spaces(spaces.max(0.0)),
                LengthOrNumber::Length(length) => TabSize::Px(context.length(length).max(0.0)),
            }
        }
        Property::TextAlignLast(value, _) => {
            style.text_align_last = match value {
                CssTextAlignLast::Auto => None,
                CssTextAlignLast::Start => Some(TextAlign::Start),
                CssTextAlignLast::MatchParent => Some(match_parent(
                    parent,
                    parent.text_align_last.unwrap_or(TextAlign::Start),
                    context.root,
                )),
                CssTextAlignLast::End => Some(TextAlign::End),
                CssTextAlignLast::Left => Some(TextAlign::Left),
                CssTextAlignLast::Right => Some(TextAlign::Right),
                CssTextAlignLast::Center => Some(TextAlign::Center),
                CssTextAlignLast::Justify => Some(TextAlign::Justify),
            }
        }
        Property::TextJustify(value) => {
            style.text_justify = match value {
                CssTextJustify::Auto | CssTextJustify::InterWord => TextJustify::Auto,
                CssTextJustify::None => TextJustify::None,
                CssTextJustify::InterCharacter => TextJustify::InterCharacter,
            }
        }
        Property::TextIndent(value) => {
            style.text_indent = lp(&value.value);
            style.text_indent_each_line = value.each_line;
            style.text_indent_hanging = value.hanging;
        }
        Property::TextTransform(value) => {
            style.text_transform = match value.case {
                TextTransformCase::None => TextTransform::None,
                TextTransformCase::Uppercase => TextTransform::Uppercase,
                TextTransformCase::Lowercase => TextTransform::Lowercase,
                TextTransformCase::Capitalize => TextTransform::Capitalize,
            }
        }
        Property::WhiteSpace(value) => {
            style.white_space = match value {
                CssWhiteSpace::Normal => WhiteSpace::NORMAL,
                CssWhiteSpace::Pre => WhiteSpace::PRE,
                CssWhiteSpace::NoWrap => WhiteSpace::NO_WRAP,
                CssWhiteSpace::PreWrap => WhiteSpace::PRE_WRAP,
                CssWhiteSpace::BreakSpaces => WhiteSpace::BREAK_SPACES,
                CssWhiteSpace::PreLine => WhiteSpace::PRE_LINE,
            }
        }
        Property::VerticalAlign(value) => {
            style.vertical_align = match value {
                CssVerticalAlign::Keyword(keyword) => match keyword {
                    VerticalAlignKeyword::Baseline => VerticalAlign::Baseline,
                    VerticalAlignKeyword::Sub => VerticalAlign::Sub,
                    VerticalAlignKeyword::Super => VerticalAlign::Super,
                    VerticalAlignKeyword::Top => VerticalAlign::Top,
                    VerticalAlignKeyword::TextTop => VerticalAlign::TextTop,
                    VerticalAlignKeyword::Middle => VerticalAlign::Middle,
                    VerticalAlignKeyword::Bottom => VerticalAlign::Bottom,
                    VerticalAlignKeyword::TextBottom => VerticalAlign::TextBottom,
                },
                CssVerticalAlign::Length(length) => VerticalAlign::Length(lp(length)),
            }
        }
        Property::TextDecorationLine(value, _) => {
            style.text_decoration_line.underline = value.contains(CssDecorationLine::Underline);
            style.text_decoration_line.overline = value.contains(CssDecorationLine::Overline);
            style.text_decoration_line.line_through = value.contains(CssDecorationLine::LineThrough);
        }
        Property::TextDecorationStyle(value, _) => {
            style.text_decoration_style = match value {
                CssDecorationStyle::Solid => TextDecorationStyle::Solid,
                CssDecorationStyle::Double => TextDecorationStyle::Double,
                CssDecorationStyle::Dotted => TextDecorationStyle::Dotted,
                CssDecorationStyle::Dashed => TextDecorationStyle::Dashed,
                CssDecorationStyle::Wavy => TextDecorationStyle::Wavy,
            }
        }
        Property::TextDecorationColor(value, _) => style.text_decoration_color = color(value),
        Property::TextShadow(shadows) => {
            style.text_shadow = shadows
                .iter()
                .map(|shadow| TextShadow {
                    x: context.length(&shadow.x_offset),
                    y: context.length(&shadow.y_offset),
                    blur: context.length(&shadow.blur).max(0.0),
                    color: color(&shadow.color),
                })
                .collect();
        }
        Property::TextEmphasisStyle(value, _) => style.text_emphasis_style = emphasis_mark(value),
        Property::TextEmphasisColor(value, _) => style.text_emphasis_color = color(value),
        Property::TextEmphasisPosition(value, _) => {
            use lightningcss::properties::text::TextEmphasisPositionVertical;
            style.text_emphasis_position = match value.vertical {
                TextEmphasisPositionVertical::Over => crate::values::AnnotationSide::Over,
                TextEmphasisPositionVertical::Under => crate::values::AnnotationSide::Under,
            };
        }
        Property::OutlineWidth(value) => style.outline_width = border_width(context, value),
        Property::OutlineStyle(value) => {
            style.outline_style = match value {
                CssOutlineStyle::Auto => OutlineStyle::Auto,
                CssOutlineStyle::LineStyle(line) => OutlineStyle::Line(border_style(*line)),
            }
        }
        Property::OutlineColor(value) => style.outline_color = color(value),
        Property::ContainerType(value) => {
            use crate::values::ContainerType;
            use lightningcss::properties::contain::ContainerType as Css;
            style.container_type = match value {
                Css::InlineSize => ContainerType::InlineSize,
                Css::Size => ContainerType::Size,
                Css::Normal | Css::ScrollState => ContainerType::Normal,
            };
        }
        Property::ContainerName(names) => {
            use lightningcss::properties::contain::ContainerNameList;
            style.container_names = match names {
                ContainerNameList::None => Arc::from([]),
                ContainerNameList::Names(names) => names.iter().map(|name| name.0.to_string()).collect(),
            };
        }
        Property::BoxDecorationBreak(value, _) => {
            use crate::values::BoxDecorationBreak;
            style.box_decoration_break = match value {
                lightningcss::properties::text::BoxDecorationBreak::Slice => BoxDecorationBreak::Slice,
                lightningcss::properties::text::BoxDecorationBreak::Clone => BoxDecorationBreak::Clone,
            };
        }
        Property::FontPalette(reference) => {
            style.font_palette = crate::values::FontPalette::Named(reference.ident.0.as_ref().into());
        }
        // `normal`, `light`, `dark` and `palette-mix()`, which lightningcss does not know.
        Property::Unparsed(unparsed) if unparsed.property_id == PropertyId::FontPalette => {
            if let Some(palette) = crate::palette::font_palette(&unparsed.value.0, &style.font_palette) {
                style.font_palette = palette;
            }
        }
        Property::TextOverflow(value, _) => {
            style.text_overflow = match value {
                CssTextOverflow::Clip => TextOverflow::Clip,
                CssTextOverflow::Ellipsis => TextOverflow::Ellipsis,
            }
        }
        Property::BoxOrient(value, _) => {
            use lightningcss::properties::flex::BoxOrient as Orient;
            style.box_orient = match value {
                Orient::Vertical | Orient::BlockAxis => crate::values::BoxOrient::Vertical,
                Orient::Horizontal | Orient::InlineAxis => crate::values::BoxOrient::Horizontal,
            };
        }
        Property::TextDecorationSkipInk(value, _) => {
            use lightningcss::properties::text::TextDecorationSkipInk as Skip;
            style.text_decoration_skip_ink = match value {
                Skip::Auto => crate::values::SkipInk::Auto,
                Skip::None => crate::values::SkipInk::None,
                Skip::All => crate::values::SkipInk::All,
            };
        }
        Property::TextDecorationThickness(value) => {
            style.text_decoration_thickness = match value {
                CssDecorationThickness::Auto => DecorationThickness::Auto,
                CssDecorationThickness::FromFont => DecorationThickness::FromFont,
                // Percentages refer to 1em.
                CssDecorationThickness::LengthPercentage(length) => {
                    DecorationThickness::Px(context.length_percentage(length).resolve(context.font_size))
                }
            }
        }
        Property::ListStyleType(value) => style.list_style_type = list_style_type(value),
        Property::ListStyleImage(image) => style.list_style_image = background::image(image, context, None),
        Property::ListStylePosition(value) => {
            style.list_style_position = match value {
                CssListStylePosition::Inside => ListStylePosition::Inside,
                CssListStylePosition::Outside => ListStylePosition::Outside,
            }
        }
        Property::BorderSpacing(value) => {
            style.border_spacing = (context.length(&value.0), context.length(&value.1));
        }
        Property::FlexDirection(value, _) => {
            style.flex_direction = match value {
                CssFlexDirection::Row => FlexDirection::Row,
                CssFlexDirection::RowReverse => FlexDirection::RowReverse,
                CssFlexDirection::Column => FlexDirection::Column,
                CssFlexDirection::ColumnReverse => FlexDirection::ColumnReverse,
            }
        }
        Property::FlexWrap(value, _) => {
            style.flex_balance = false;
            style.flex_wrap = match value {
                CssFlexWrap::NoWrap => FlexWrap::NoWrap,
                CssFlexWrap::Wrap => FlexWrap::Wrap,
                CssFlexWrap::WrapReverse => FlexWrap::WrapReverse,
            }
        }
        // `calc(infinity)` is the largest factor there is, as in Blink.
        Property::FlexGrow(value, _) => style.flex_grow = flex_factor(*value),
        Property::FlexShrink(value, _) => style.flex_shrink = flex_factor(*value),
        Property::FlexBasis(value, _) => {
            style.flex_basis = match value {
                LengthPercentageOrAuto::Auto => Size::Auto,
                LengthPercentageOrAuto::LengthPercentage(length) => Size::Length(lp(length)),
            }
        }
        Property::Order(value, _) => style.order = *value,
        Property::JustifyContent(value, _) => {
            use lightningcss::properties::align::OverflowPosition;
            style.justify_content = content_alignment_justify(value);
            style.justify_content_safe = matches!(
                value,
                JustifyContent::ContentPosition {
                    overflow: Some(OverflowPosition::Safe),
                    ..
                } | JustifyContent::Left {
                    overflow: Some(OverflowPosition::Safe)
                } | JustifyContent::Right {
                    overflow: Some(OverflowPosition::Safe)
                }
            );
        }
        Property::AlignContent(value, _) => {
            style.align_content = content_alignment_align(value);
            style.align_content_unsafe = matches!(
                value,
                AlignContent::ContentPosition {
                    overflow: Some(lightningcss::properties::align::OverflowPosition::Unsafe),
                    ..
                }
            );
            style.align_content_safe = matches!(
                value,
                AlignContent::ContentPosition {
                    overflow: Some(lightningcss::properties::align::OverflowPosition::Safe),
                    ..
                }
            );
        }
        Property::AlignItems(value, _) => {
            style.align_items = match value {
                AlignItems::Normal => ItemAlignment::Normal,
                AlignItems::Stretch => ItemAlignment::Stretch,
                AlignItems::BaselinePosition(lightningcss::properties::align::BaselinePosition::Last) => {
                    ItemAlignment::LastBaseline
                }
                AlignItems::BaselinePosition(_) => ItemAlignment::Baseline,
                AlignItems::SelfPosition { value, .. } => self_alignment(*value),
            };
            style.align_items_flags = match value {
                AlignItems::SelfPosition { value, overflow } => self_flags(*value, overflow.as_ref()),
                _ => SelfFlags::default(),
            };
        }
        Property::AlignSelf(value, _) => {
            style.align_self = match value {
                AlignSelf::Auto => ItemAlignment::Auto,
                AlignSelf::Normal => ItemAlignment::Normal,
                AlignSelf::Stretch => ItemAlignment::Stretch,
                AlignSelf::BaselinePosition(lightningcss::properties::align::BaselinePosition::Last) => {
                    ItemAlignment::LastBaseline
                }
                AlignSelf::BaselinePosition(_) => ItemAlignment::Baseline,
                AlignSelf::SelfPosition { value, .. } => self_alignment(*value),
            };
            style.align_self_flags = match value {
                AlignSelf::SelfPosition { value, overflow } => self_flags(*value, overflow.as_ref()),
                _ => SelfFlags::default(),
            };
        }
        Property::RowGap(value) => style.row_gap = gap(context, value),
        Property::ColumnGap(value) => style.column_gap = gap(context, value),
        Property::GridTemplateColumns(value) => style.grid_template_columns = context.track_list(value),
        Property::GridTemplateRows(value) => style.grid_template_rows = context.track_list(value),
        Property::GridTemplateAreas(value) => style.grid_template_areas = grid_areas(value),
        Property::Hyphens(value, _) => {
            use lightningcss::properties::text::Hyphens as Css;
            style.hyphens = match value {
                Css::None => Hyphens::None,
                Css::Manual => Hyphens::Manual,
                Css::Auto => Hyphens::Auto,
            };
        }
        Property::OverflowWrap(value) | Property::WordWrap(value) => {
            use lightningcss::properties::text::OverflowWrap as Css;
            style.overflow_wrap = match value {
                Css::Normal => OverflowWrap::Normal,
                Css::BreakWord => OverflowWrap::BreakWord,
                Css::Anywhere => OverflowWrap::Anywhere,
            };
        }
        Property::LineBreak(value) => {
            use lightningcss::properties::text::LineBreak as Css;
            style.line_break = match value {
                Css::Auto => LineBreak::Auto,
                Css::Loose => LineBreak::Loose,
                Css::Normal => LineBreak::Normal,
                Css::Strict => LineBreak::Strict,
                Css::Anywhere => LineBreak::Anywhere,
            };
        }
        Property::WordBreak(value) => {
            use lightningcss::properties::text::WordBreak as Css;
            style.word_break = match value {
                Css::Normal => WordBreak::Normal,
                Css::BreakAll => WordBreak::BreakAll,
                Css::KeepAll => WordBreak::KeepAll,
                Css::BreakWord => WordBreak::BreakWord,
            };
        }
        Property::GridAutoRows(value) => style.grid_auto_rows = context.track_sizes(&value.0),
        Property::GridAutoColumns(value) => style.grid_auto_columns = context.track_sizes(&value.0),
        Property::GridAutoFlow(flow) => {
            use lightningcss::properties::grid::GridAutoFlow;
            style.grid_auto_flow_column = flow.contains(GridAutoFlow::Column);
            style.grid_auto_flow_dense = flow.contains(GridAutoFlow::Dense);
        }
        Property::GridRowStart(line) => style.grid_row_start = grid_line(line),
        Property::GridRowEnd(line) => style.grid_row_end = grid_line(line),
        Property::GridColumnStart(line) => style.grid_column_start = grid_line(line),
        Property::GridColumnEnd(line) => style.grid_column_end = grid_line(line),
        Property::JustifyItems(value) => {
            use lightningcss::properties::align::JustifyItems;
            style.justify_items_block = crate::block_align::justify_items(value);
            style.justify_items = match value {
                JustifyItems::Normal | JustifyItems::Legacy(_) => ItemAlignment::Normal,
                JustifyItems::Stretch => ItemAlignment::Stretch,
                JustifyItems::BaselinePosition(lightningcss::properties::align::BaselinePosition::Last) => {
                    ItemAlignment::LastBaseline
                }
                JustifyItems::BaselinePosition(_) => ItemAlignment::Baseline,
                JustifyItems::SelfPosition { value, .. } => self_alignment(*value),
                JustifyItems::Left { .. } => ItemAlignment::Start,
                JustifyItems::Right { .. } => ItemAlignment::End,
            }
        }
        Property::JustifySelf(value) => {
            use lightningcss::properties::align::JustifySelf;
            style.justify_self_block = crate::block_align::justify_self(value);
            style.justify_self = match value {
                JustifySelf::Auto => ItemAlignment::Auto,
                JustifySelf::Normal => ItemAlignment::Normal,
                JustifySelf::Stretch => ItemAlignment::Stretch,
                JustifySelf::BaselinePosition(lightningcss::properties::align::BaselinePosition::Last) => {
                    ItemAlignment::LastBaseline
                }
                JustifySelf::BaselinePosition(_) => ItemAlignment::Baseline,
                JustifySelf::SelfPosition { value, .. } => self_alignment(*value),
                JustifySelf::Left { .. } => ItemAlignment::Start,
                JustifySelf::Right { .. } => ItemAlignment::End,
            }
        }
        Property::BoxShadow(shadows, _) => {
            style.box_shadow = shadows
                .iter()
                .map(|shadow| BoxShadow {
                    color: color(&shadow.color),
                    x: context.length(&shadow.x_offset),
                    y: context.length(&shadow.y_offset),
                    blur: context.length(&shadow.blur).max(0.0),
                    spread: context.length(&shadow.spread),
                    inset: shadow.inset,
                })
                .collect();
        }
        Property::Transform(list, _) => {
            style.transform = list
                .0
                .iter()
                .flat_map(|function| transform(function, context))
                .collect();
        }
        Property::Translate(value) => {
            style.translate = match value {
                Translate::None => None,
                Translate::XYZ { x, y, z } => Some((
                    context.length_percentage(x),
                    context.length_percentage(y),
                    context.length(z),
                )),
            };
        }
        Property::Rotate(value) => {
            style.rotate = match value {
                Rotate::None => None,
                Rotate::XYZ { x, y, z, angle } => Some((*x, *y, *z, angle.to_degrees())),
            };
        }
        Property::Perspective(value, _) => {
            style.perspective = match value {
                lightningcss::properties::transform::Perspective::None => None,
                lightningcss::properties::transform::Perspective::Length(length) => {
                    Some(context.length(length).max(0.0))
                }
            };
        }
        Property::PerspectiveOrigin(position, _) => {
            style.perspective_origin = background::position(position, context);
        }
        Property::Scale(value) => {
            style.scale = match value {
                Scale::None => None,
                Scale::XYZ { x, y, .. } => Some((number(x), number(y))),
            };
        }
        Property::TransformOrigin(position, _) => {
            style.transform_origin = background::position(position, context);
        }
        Property::BackdropFilter(list, _) => {
            style.backdrop_filter = !matches!(list, lightningcss::properties::effects::FilterList::None);
        }
        Property::TransformStyle(value, _) => {
            style.preserve_3d = matches!(
                value,
                lightningcss::properties::transform::TransformStyle::Preserve3d
            );
        }
        Property::TransformBox(value) => {
            use crate::values::TransformBox;
            use lightningcss::properties::transform::TransformBox as Css;
            style.transform_box = match value {
                Css::ContentBox => TransformBox::ContentBox,
                Css::BorderBox => TransformBox::BorderBox,
                Css::FillBox => TransformBox::FillBox,
                Css::StrokeBox => TransformBox::StrokeBox,
                Css::ViewBox => TransformBox::ViewBox,
            };
        }
        Property::BackgroundImage(images) => {
            style.background_image = images
                .iter()
                .map(|image| background::image(image, context, None))
                .collect();
        }
        Property::BackgroundPositionX(positions) => {
            style.background_position_x = positions
                .iter()
                .map(|x| background::component(x, context, |side| *side == HorizontalPositionKeyword::Right))
                .collect();
        }
        Property::BackgroundPositionY(positions) => {
            style.background_position_y = positions
                .iter()
                .map(|y| background::component(y, context, |side| *side == VerticalPositionKeyword::Bottom))
                .collect();
        }
        Property::BackgroundSize(sizes) => {
            style.background_size = sizes.iter().map(|size| background::size(size, context)).collect();
        }
        Property::BackgroundRepeat(repeats) => {
            style.background_repeat = repeats.iter().map(background::repeat).collect();
        }
        Property::BackgroundOrigin(origins) => {
            style.background_origin = origins.iter().map(background::origin).collect();
        }
        Property::BackgroundAttachment(attachments) => {
            use lightningcss::properties::background::BackgroundAttachment;
            style.background_fixed = attachments
                .iter()
                .map(|attachment| *attachment == BackgroundAttachment::Fixed)
                .collect();
            let local = |attachment: &BackgroundAttachment| *attachment == BackgroundAttachment::Local;
            style.background_local = if attachments.iter().any(local) {
                attachments.iter().map(local).collect()
            } else {
                Arc::from([])
            };
        }
        Property::BackgroundClip(clips, _) => {
            style.background_clip = clips.iter().map(background::clip).collect();
            let text = |clip: &lightningcss::properties::background::BackgroundClip| {
                *clip == lightningcss::properties::background::BackgroundClip::Text
            };
            style.background_clip_text = if clips.iter().any(text) {
                clips.iter().map(text).collect()
            } else {
                Arc::from([])
            };
            style.background_clip_border_area = Arc::from([]);
        }
        // `border-area`, which lightningcss does not know.
        Property::Unparsed(unparsed) if matches!(unparsed.property_id, PropertyId::BackgroundClip(_)) => {
            use fagus_css::background_clip::{ClipBox, ClipLayer};
            if let Some(clips) = fagus_css::background_clip::layers(&unparsed.value) {
                let flags = |pick: fn(&ClipLayer) -> bool| -> Arc<[bool]> {
                    if clips.iter().any(pick) {
                        clips.iter().map(pick).collect()
                    } else {
                        Arc::from([])
                    }
                };
                style.background_clip_text = flags(|clip| clip.text);
                style.background_clip_border_area = flags(|clip| clip.border_area);
                style.background_clip = clips
                    .iter()
                    .map(|clip| match clip.area {
                        ClipBox::Border => crate::values::BoxArea::BorderBox,
                        ClipBox::Padding => crate::values::BoxArea::PaddingBox,
                        ClipBox::Content => crate::values::BoxArea::ContentBox,
                    })
                    .collect();
            }
        }
        Property::Custom(custom) => {
            if let lightningcss::properties::custom::CustomPropertyName::Unknown(name) = &custom.name {
                if name.0.as_ref() == background::IMAGE_TOKENS {
                    if let Some(images) = background::images_from_tokens(&custom.value, context) {
                        style.background_image = images.into();
                    }
                } else {
                    crate::extra::apply(&name.0, &custom.value, style, context);
                }
            }
        }
        _ => {}
    }
}

/// The border image of a style, made when a `border-image` property is first set.
fn border_image(style: &mut ComputedValues) -> &mut crate::values::BorderImage {
    Arc::make_mut(style.border_image.get_or_insert_with(Default::default))
}

/// The mask layers of a style, made when a mask property is first set.
fn mask_layers(style: &mut ComputedValues) -> &mut crate::values::MaskLayers {
    Arc::make_mut(style.mask.get_or_insert_with(Default::default))
}

/// The box of the box model that a box of CSS Masking stands for in HTML (CSS Masking 1, §6.1):
/// the content box for `fill-box`, the border box for the boxes of SVG and the margin box.
fn geometry_box(geometry: &lightningcss::properties::masking::GeometryBox) -> crate::values::BoxArea {
    use crate::values::BoxArea;
    use lightningcss::properties::masking::GeometryBox;
    match geometry {
        GeometryBox::PaddingBox => BoxArea::PaddingBox,
        GeometryBox::ContentBox | GeometryBox::FillBox => BoxArea::ContentBox,
        GeometryBox::BorderBox | GeometryBox::MarginBox | GeometryBox::StrokeBox | GeometryBox::ViewBox => {
            BoxArea::BorderBox
        }
    }
}

/// The box of an SVG element that a box of CSS Masking stands for (CSS Masking 1, §6.1): the
/// object bounding box for `content-box`, `padding-box` and `fill-box`, the nearest viewport for
/// `view-box`, and the stroke bounding box for the others.
fn svg_geometry_box(
    geometry: &lightningcss::properties::masking::GeometryBox,
) -> crate::values::TransformBox {
    use crate::values::TransformBox;
    use lightningcss::properties::masking::GeometryBox;
    match geometry {
        GeometryBox::PaddingBox | GeometryBox::ContentBox | GeometryBox::FillBox => TransformBox::FillBox,
        GeometryBox::ViewBox => TransformBox::ViewBox,
        GeometryBox::BorderBox | GeometryBox::MarginBox | GeometryBox::StrokeBox => TransformBox::StrokeBox,
    }
}

fn number(value: &NumberOrPercentage) -> f32 {
    match value {
        NumberOrPercentage::Number(number) => *number,
        NumberOrPercentage::Percentage(percentage) => percentage.0,
    }
}

/// `scale3d(x, y, z)`: a 2D scale where depth keeps its scale.
fn scale3d(x: f32, y: f32, z: f32) -> TransformFunction {
    if (z - 1.0).abs() < f32::EPSILON {
        return TransformFunction::Scale(x, y);
    }
    TransformFunction::Matrix3d([
        x, 0.0, 0.0, 0.0, 0.0, y, 0.0, 0.0, 0.0, 0.0, z, 0.0, 0.0, 0.0, 0.0, 1.0,
    ])
}

/// A transform function, as one or two of Fagus's.
fn transform(function: &CssTransform, context: &Context) -> Vec<TransformFunction> {
    let degrees = |angle: &lightningcss::values::angle::Angle| angle.to_degrees();
    let lp = |value: &CssLengthPercentage| context.length_percentage(value);
    vec![match function {
        CssTransform::Translate(x, y) => TransformFunction::Translate(lp(x), lp(y)),
        CssTransform::Translate3d(x, y, z) => {
            return vec![
                TransformFunction::Translate(lp(x), lp(y)),
                TransformFunction::TranslateZ(context.length(z)),
            ];
        }
        CssTransform::TranslateX(x) => TransformFunction::Translate(lp(x), LengthPercentage::ZERO),
        CssTransform::TranslateY(y) => TransformFunction::Translate(LengthPercentage::ZERO, lp(y)),
        CssTransform::TranslateZ(z) => TransformFunction::TranslateZ(context.length(z)),
        CssTransform::Scale(x, y) => TransformFunction::Scale(number(x), number(y)),
        // Scaling depth leaves the plane of the box as it is, but not a plane turned out of it
        // by a rotation before.
        CssTransform::Scale3d(x, y, z) => scale3d(number(x), number(y), number(z)),
        CssTransform::ScaleX(x) => TransformFunction::Scale(number(x), 1.0),
        CssTransform::ScaleY(y) => TransformFunction::Scale(1.0, number(y)),
        CssTransform::ScaleZ(z) => scale3d(1.0, 1.0, number(z)),
        CssTransform::Rotate(angle) | CssTransform::RotateZ(angle) => {
            TransformFunction::Rotate(degrees(angle))
        }
        CssTransform::RotateX(angle) => TransformFunction::Rotate3d(1.0, 0.0, 0.0, degrees(angle)),
        CssTransform::RotateY(angle) => TransformFunction::Rotate3d(0.0, 1.0, 0.0, degrees(angle)),
        CssTransform::Rotate3d(x, y, z, angle) => TransformFunction::Rotate3d(*x, *y, *z, degrees(angle)),
        CssTransform::Skew(x, y) => TransformFunction::Skew(degrees(x), degrees(y)),
        CssTransform::SkewX(x) => TransformFunction::Skew(degrees(x), 0.0),
        CssTransform::SkewY(y) => TransformFunction::Skew(0.0, degrees(y)),
        CssTransform::Matrix(matrix) => {
            TransformFunction::Matrix([matrix.a, matrix.b, matrix.c, matrix.d, matrix.e, matrix.f])
        }
        CssTransform::Matrix3d(m) => TransformFunction::Matrix3d([
            m.m11, m.m12, m.m13, m.m14, m.m21, m.m22, m.m23, m.m24, m.m31, m.m32, m.m33, m.m34, m.m41, m.m42,
            m.m43, m.m44,
        ]),
        CssTransform::Perspective(length) => TransformFunction::Perspective(context.length(length).max(0.0)),
    }]
}

/// A side of a box in its own writing mode: the block-start, block-end, inline-start or
/// inline-end side.
#[derive(Clone, Copy, PartialEq, Eq)]
struct LogicalSide {
    block: bool,
    start: bool,
}

const BLOCK_START: LogicalSide = LogicalSide {
    block: true,
    start: true,
};
const BLOCK_END: LogicalSide = LogicalSide {
    block: true,
    start: false,
};
const INLINE_START: LogicalSide = LogicalSide {
    block: false,
    start: true,
};
const INLINE_END: LogicalSide = LogicalSide {
    block: false,
    start: false,
};

/// The physical sides, in the order top, right, bottom, left.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PhysicalSide {
    Top,
    Right,
    Bottom,
    Left,
}

/// The physical side that a logical side stands for in the element's writing mode and
/// direction (CSS Writing Modes 4, §6.4).
fn physical(context: &Context, side: LogicalSide) -> PhysicalSide {
    physical_side(context.writing_mode, context.rtl, side)
}

/// The physical side that a logical side stands for in a writing mode and direction.
fn physical_side(mode: crate::values::WritingMode, rtl: bool, side: LogicalSide) -> PhysicalSide {
    use crate::values::WritingMode;
    use PhysicalSide::{Bottom, Left, Right, Top};
    if side.block {
        return match (mode, side.start) {
            (WritingMode::HorizontalTb, true) => Top,
            (WritingMode::HorizontalTb, false) => Bottom,
            (WritingMode::VerticalRl | WritingMode::SidewaysRl, true)
            | (WritingMode::VerticalLr | WritingMode::SidewaysLr, false) => Right,
            (WritingMode::VerticalRl | WritingMode::SidewaysRl, false)
            | (WritingMode::VerticalLr | WritingMode::SidewaysLr, true) => Left,
        };
    }
    // The inline axis starts at the left or the top, and at the other end in right-to-left
    // text; in `sideways-lr` it runs from the bottom up.
    let forward = side.start != rtl;
    match (mode, forward) {
        (WritingMode::HorizontalTb, true) => Left,
        (WritingMode::HorizontalTb, false) => Right,
        (WritingMode::SidewaysLr, true) => Bottom,
        (WritingMode::SidewaysLr, false) => Top,
        (_, true) => Top,
        (_, false) => Bottom,
    }
}

/// The side of `sides` that the logical side `side` stands for.
fn logical<'s, T>(sides: &'s mut crate::values::Sides<T>, context: &Context, side: LogicalSide) -> &'s mut T {
    match physical(context, side) {
        PhysicalSide::Top => &mut sides.top,
        PhysicalSide::Right => &mut sides.right,
        PhysicalSide::Bottom => &mut sides.bottom,
        PhysicalSide::Left => &mut sides.left,
    }
}

/// The physical property that a logical longhand stands for in a writing mode and direction
/// (`margin-inline-start` is `margin-left` in horizontal left-to-right text), `None` for other
/// properties. Chrome applies `inherit`, `initial` and `unset` of a logical property to that
/// physical property: the element inherits its parent's value of the same physical side
/// (Chrome 157, measured).
pub(crate) fn physical_id(
    id: &PropertyId<'_>,
    mode: crate::values::WritingMode,
    rtl: bool,
) -> Option<PropertyId<'static>> {
    use PhysicalSide::{Bottom, Left, Right, Top};
    let side = |logical: LogicalSide, [top, right, bottom, left]: [PropertyId<'static>; 4]| {
        Some(match physical_side(mode, rtl, logical) {
            Top => top,
            Right => right,
            Bottom => bottom,
            Left => left,
        })
    };
    let margin = || {
        [
            PropertyId::MarginTop,
            PropertyId::MarginRight,
            PropertyId::MarginBottom,
            PropertyId::MarginLeft,
        ]
    };
    let padding = || {
        [
            PropertyId::PaddingTop,
            PropertyId::PaddingRight,
            PropertyId::PaddingBottom,
            PropertyId::PaddingLeft,
        ]
    };
    let inset = || {
        [
            PropertyId::Top,
            PropertyId::Right,
            PropertyId::Bottom,
            PropertyId::Left,
        ]
    };
    let width = || {
        [
            PropertyId::BorderTopWidth,
            PropertyId::BorderRightWidth,
            PropertyId::BorderBottomWidth,
            PropertyId::BorderLeftWidth,
        ]
    };
    let style = || {
        [
            PropertyId::BorderTopStyle,
            PropertyId::BorderRightStyle,
            PropertyId::BorderBottomStyle,
            PropertyId::BorderLeftStyle,
        ]
    };
    let color = || {
        [
            PropertyId::BorderTopColor,
            PropertyId::BorderRightColor,
            PropertyId::BorderBottomColor,
            PropertyId::BorderLeftColor,
        ]
    };
    let vertical = mode.is_vertical();
    // A corner is where a block side and an inline side meet.
    let corner = |block: LogicalSide, inline: LogicalSide| {
        let sides = [physical_side(mode, rtl, block), physical_side(mode, rtl, inline)];
        let prefix = lightningcss::vendor_prefix::VendorPrefix::None;
        Some(match (sides.contains(&Top), sides.contains(&Left)) {
            (true, true) => PropertyId::BorderTopLeftRadius(prefix),
            (true, false) => PropertyId::BorderTopRightRadius(prefix),
            (false, false) => PropertyId::BorderBottomRightRadius(prefix),
            (false, true) => PropertyId::BorderBottomLeftRadius(prefix),
        })
    };
    match id {
        PropertyId::MarginBlockStart => side(BLOCK_START, margin()),
        PropertyId::MarginBlockEnd => side(BLOCK_END, margin()),
        PropertyId::MarginInlineStart => side(INLINE_START, margin()),
        PropertyId::MarginInlineEnd => side(INLINE_END, margin()),
        PropertyId::PaddingBlockStart => side(BLOCK_START, padding()),
        PropertyId::PaddingBlockEnd => side(BLOCK_END, padding()),
        PropertyId::PaddingInlineStart => side(INLINE_START, padding()),
        PropertyId::PaddingInlineEnd => side(INLINE_END, padding()),
        PropertyId::InsetBlockStart => side(BLOCK_START, inset()),
        PropertyId::InsetBlockEnd => side(BLOCK_END, inset()),
        PropertyId::InsetInlineStart => side(INLINE_START, inset()),
        PropertyId::InsetInlineEnd => side(INLINE_END, inset()),
        PropertyId::BorderBlockStartWidth => side(BLOCK_START, width()),
        PropertyId::BorderBlockEndWidth => side(BLOCK_END, width()),
        PropertyId::BorderInlineStartWidth => side(INLINE_START, width()),
        PropertyId::BorderInlineEndWidth => side(INLINE_END, width()),
        PropertyId::BorderBlockStartStyle => side(BLOCK_START, style()),
        PropertyId::BorderBlockEndStyle => side(BLOCK_END, style()),
        PropertyId::BorderInlineStartStyle => side(INLINE_START, style()),
        PropertyId::BorderInlineEndStyle => side(INLINE_END, style()),
        PropertyId::BorderBlockStartColor => side(BLOCK_START, color()),
        PropertyId::BorderBlockEndColor => side(BLOCK_END, color()),
        PropertyId::BorderInlineStartColor => side(INLINE_START, color()),
        PropertyId::BorderInlineEndColor => side(INLINE_END, color()),
        PropertyId::BorderStartStartRadius => corner(BLOCK_START, INLINE_START),
        PropertyId::BorderStartEndRadius => corner(BLOCK_START, INLINE_END),
        PropertyId::BorderEndStartRadius => corner(BLOCK_END, INLINE_START),
        PropertyId::BorderEndEndRadius => corner(BLOCK_END, INLINE_END),
        PropertyId::InlineSize => Some(if vertical {
            PropertyId::Height
        } else {
            PropertyId::Width
        }),
        PropertyId::BlockSize => Some(if vertical {
            PropertyId::Width
        } else {
            PropertyId::Height
        }),
        PropertyId::MinInlineSize => Some(if vertical {
            PropertyId::MinHeight
        } else {
            PropertyId::MinWidth
        }),
        PropertyId::MinBlockSize => Some(if vertical {
            PropertyId::MinWidth
        } else {
            PropertyId::MinHeight
        }),
        PropertyId::MaxInlineSize => Some(if vertical {
            PropertyId::MaxHeight
        } else {
            PropertyId::MaxWidth
        }),
        PropertyId::MaxBlockSize => Some(if vertical {
            PropertyId::MaxWidth
        } else {
            PropertyId::MaxHeight
        }),
        _ => None,
    }
}

/// The corner of `corners` where the logical sides `block` and `inline` meet.
fn corner<'c>(
    corners: &'c mut crate::values::Corners,
    context: &Context,
    block: LogicalSide,
    inline: LogicalSide,
) -> &'c mut (LengthPercentage, LengthPercentage) {
    use PhysicalSide::{Bottom, Left, Right, Top};
    let (first, second) = (physical(context, block), physical(context, inline));
    let top = first == Top || second == Top;
    let left = first == Left || second == Left;
    debug_assert!(top || first == Bottom || second == Bottom);
    debug_assert!(left || first == Right || second == Right);
    match (top, left) {
        (true, true) => &mut corners.top_left,
        (true, false) => &mut corners.top_right,
        (false, false) => &mut corners.bottom_right,
        (false, true) => &mut corners.bottom_left,
    }
}

/// A padding: negative values are invalid when written out (they never get here) and clamped
/// at zero when a calculation comes to them (`calc(-8px)`), as in Chrome.
fn padding(context: &Context, value: &LengthPercentageOrAuto) -> LengthPercentage {
    match value {
        LengthPercentageOrAuto::Auto => LengthPercentage::ZERO,
        LengthPercentageOrAuto::LengthPercentage(length) => context.length_percentage(length).non_negative(),
    }
}

/// The name of a family, without the quotes the serializer adds. A generic family's keyword
/// quoted (`"serif"`) is the name of a family, not the generic family, as in Chrome: it keeps
/// its quotes, which no installed family has.
fn family_name(name: &lightningcss::properties::font::FamilyName<'_>) -> String {
    let mut css = String::new();
    let mut printer = lightningcss::printer::Printer::new(&mut css, PrinterOptions::default());
    let _ = lightningcss::traits::ToCss::to_css(name, &mut printer);
    let trimmed = css.trim_matches(|character| character == '"' || character == '\'');
    let name = trimmed.replace("\\ ", " ");
    let generic = [
        "serif",
        "sans-serif",
        "monospace",
        "cursive",
        "fantasy",
        "system-ui",
        "ui-serif",
        "ui-sans-serif",
        "ui-monospace",
        "ui-rounded",
        "math",
    ]
    .iter()
    .any(|keyword| name.eq_ignore_ascii_case(keyword));
    if generic { format!("\"{name}\"") } else { name }
}

/// The name of a generic family as font matching knows it. The `ui-` families keep their
/// names: Chrome does not know them, so with the system's fonts they match nothing.
fn generic_family(family: GenericFontFamily) -> &'static str {
    match family {
        GenericFontFamily::Serif => "serif",
        GenericFontFamily::Monospace => "monospace",
        GenericFontFamily::Cursive => "cursive",
        GenericFontFamily::Fantasy => "fantasy",
        GenericFontFamily::SystemUI => "system-ui",
        GenericFontFamily::UISerif => "ui-serif",
        GenericFontFamily::UISansSerif => "ui-sans-serif",
        GenericFontFamily::UIMonospace => "ui-monospace",
        GenericFontFamily::UIRounded => "ui-rounded",
        GenericFontFamily::Math => "math",
        _ => "sans-serif",
    }
}

/// `bolder` (CSS Fonts 4, §2.2.1).
fn bolder(parent: f32) -> f32 {
    if parent < 350.0 {
        400.0
    } else if parent < 550.0 {
        700.0
    } else {
        parent.max(900.0)
    }
}

fn lighter(parent: f32) -> f32 {
    if parent < 550.0 {
        parent.min(100.0)
    } else if parent < 750.0 {
        400.0
    } else {
        700.0
    }
}

/// The string of a `text-overflow` of one, with each run of forced line breaks in it made one
/// space, as Blink draws it (`SuppressLineBreaks`); `None` for other values.
fn text_overflow_string(tokens: &lightningcss::properties::custom::TokenList<'_>) -> Option<Arc<str>> {
    use lightningcss::properties::custom::{Token, TokenOrValue};
    let mut significant = tokens.0.iter().filter(|token| !token.is_whitespace());
    let (Some(TokenOrValue::Token(Token::String(text))), None) = (significant.next(), significant.next())
    else {
        return None;
    };
    let forced = |character: char| {
        matches!(
            character,
            '\n' | '\u{B}' | '\u{C}' | '\u{85}' | '\u{2028}' | '\u{2029}'
        )
    };
    let mut out = String::with_capacity(text.len());
    let mut breaking = false;
    for character in text.chars() {
        if forced(character) {
            if !breaking {
                out.push(' ');
            }
            breaking = true;
        } else {
            out.push(character);
            breaking = false;
        }
    }
    Some(out.into())
}

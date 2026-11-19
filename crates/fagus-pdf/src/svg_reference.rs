//! SVG elements that CSS names by URL: a `<clipPath>` for `clip-path: url(#id)`, a `<mask>`
//! for `mask: url(#id)` and a `<filter>` for `filter: url(#id)` (CSS Masking 1, Filter Effects
//! 1). A clip path becomes one outline where it can; a mask, or a clip path that cannot, is
//! drawn as an SVG image of the box's size; a filter becomes the CSS filter functions that do
//! what its primitives do, where they can, and filters the box's pixels where they cannot (see
//! [`crate::filtered`]).

use std::sync::Arc;

use fagus_layout::{PathSegment, SvgClip, SvgReference};
use fagus_style::values::{Filter, Rgba};
use fagus_text::FontDb;
use usvg::filter::{ColorInterpolation, ColorMatrixKind, CompositeOperator, Input, Kind, TransferFunction};
use usvg::{Group, Node, Transform, Tree};

/// The element with the id `id` in `markup` (from `Document::svg_definition`) made ready for
/// the boxes that name it; `None` for an element that is no `<clipPath>`, `<mask>` or
/// `<filter>`, or a filter that usvg cannot read.
pub fn reference(markup: &str, id: &str, fonts: &FontDb) -> Option<SvgReference> {
    reference_in(markup, id, fonts, (1.0, 1.0))
}

/// [`reference`] for an element in an SVG whose viewport is `viewport` (width and height) large,
/// which the percentages of a clip path's lengths in user space refer to, as in Chrome.
pub fn reference_in(markup: &str, id: &str, fonts: &FontDb, viewport: (f32, f32)) -> Option<SvgReference> {
    let name = element_name(markup)?;
    match name {
        "clipPath" => Some(match clip(markup, id, fonts, viewport) {
            Some(clip) => SvgReference::Clip(clip),
            None => SvgReference::Markup {
                markup: Arc::from(markup),
                clip: true,
            },
        }),
        "mask" => Some(SvgReference::Markup {
            markup: Arc::from(markup),
            clip: false,
        }),
        "filter" => match filter(markup, id) {
            Some(filters) => Some(SvgReference::Filter(Arc::from(filters))),
            // Primitives without CSS functions filter the box's pixels, as in Chrome.
            None => parse_with(markup, id, "filter", 100.0, (100.0, 100.0), &FontDb::new())
                .is_some_and(|tree| !tree.filters().is_empty())
                .then(|| SvgReference::PixelFilter {
                    markup: Arc::from(markup),
                }),
        },
        _ => None,
    }
}

/// The name of the first element of `markup`.
fn element_name(markup: &str) -> Option<&str> {
    let rest = markup.trim_start().strip_prefix('<')?;
    let end = rest.find(|c: char| c.is_whitespace() || c == '>' || c == '/')?;
    Some(&rest[..end])
}

/// Parses an SVG document of the size `viewport` that draws a square of `size` with `property`
/// naming the element `id` of `markup`.
fn parse_with(
    markup: &str,
    id: &str,
    property: &str,
    size: f32,
    (width, height): (f32, f32),
    fonts: &FontDb,
) -> Option<Tree> {
    let document = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" \
         width=\"{width}\" height=\"{height}\"><defs>{markup}</defs>\
         <rect width=\"{size}\" height=\"{size}\" {property}=\"url(#{id})\"/></svg>"
    );
    if id.contains(['"', '<', '&']) {
        return None;
    }
    crate::svg::parse_tree(
        document.as_bytes(),
        &crate::svg::options(document.as_bytes(), fonts),
    )
    .ok()
}

/// A `<clipPath>` as one outline, if it is one: shapes of one fill rule, none clipped again.
/// Its coordinates are fractions of the box with `clipPathUnits="objectBoundingBox"`. Text is
/// left to the SVG drawn as a mask, where it stays text, as in Chrome (see
/// [`crate::clip_text`]).
fn clip(markup: &str, id: &str, fonts: &FontDb, viewport: (f32, f32)) -> Option<SvgClip> {
    if markup.contains("<text") {
        return None;
    }
    let tree = parse_with(markup, id, "clip-path", 1.0, viewport, fonts)?;
    let bounding_box = crate::svg::root_attribute(markup, "clipPathUnits")
        .is_some_and(|units| units.trim() == "objectBoundingBox");
    let Some(clip_path) = tree.clip_paths().first() else {
        // An empty clip path, which usvg drops with what it clips: nothing shows.
        return Some(SvgClip {
            segments: Vec::new(),
            even_odd: false,
            bounding_box,
        });
    };
    if clip_path.clip_path().is_some() {
        return None;
    }
    let mut outline = Outline::default();
    outline.group(clip_path.root(), clip_path.transform())?;
    Some(SvgClip {
        segments: outline.segments,
        even_odd: outline.even_odd == Some(true),
        bounding_box,
    })
}

/// The outlines of the shapes of a clip path, collected into one.
#[derive(Default)]
struct Outline {
    segments: Vec<PathSegment>,
    /// The fill rule of the shapes so far.
    even_odd: Option<bool>,
    /// How many shapes were collected.
    shapes: usize,
}

impl Outline {
    /// Adds the shapes of `group` under `transform`; `None` where they do not make one outline.
    fn group(&mut self, group: &Group, transform: Transform) -> Option<()> {
        let transform = transform.pre_concat(group.transform());
        if group.clip_path().is_some() || group.mask().is_some() || !group.filters().is_empty() {
            return None;
        }
        for child in group.children() {
            match child {
                Node::Group(group) => self.group(group, transform)?,
                Node::Path(path) => {
                    if !path.is_visible() {
                        continue;
                    }
                    let even_odd = path
                        .fill()
                        .is_some_and(|fill| fill.rule() == usvg::FillRule::EvenOdd);
                    // Shapes of the even-odd rule cut holes into each other where they overlap,
                    // and shapes of both rules cannot be one outline.
                    if self.even_odd.is_some_and(|known| known != even_odd) || (even_odd && self.shapes > 0) {
                        return None;
                    }
                    self.even_odd = Some(even_odd);
                    self.shapes += 1;
                    let data = path.data().clone().transform(transform)?;
                    self.path(&data);
                }
                Node::Text(text) => self.group(text.flattened(), transform)?,
                Node::Image(_) => {}
            }
        }
        Some(())
    }

    fn path(&mut self, path: &usvg::tiny_skia_path::Path) {
        use usvg::tiny_skia_path::PathSegment as Segment;
        let mut current = (0.0, 0.0);
        let mut start = (0.0, 0.0);
        for segment in path.segments() {
            match segment {
                Segment::MoveTo(point) => {
                    current = (point.x, point.y);
                    start = current;
                    self.segments.push(PathSegment::Move(point.x, point.y));
                }
                Segment::LineTo(point) => {
                    current = (point.x, point.y);
                    self.segments.push(PathSegment::Line(point.x, point.y));
                }
                Segment::QuadTo(control, end) => {
                    // The cubic curve of a quadratic one: its control points two thirds of
                    // the way to the quadratic control point.
                    let third = |from: f32, to: f32| from + (to - from) * 2.0 / 3.0;
                    self.segments.push(PathSegment::Cubic([
                        third(current.0, control.x),
                        third(current.1, control.y),
                        third(end.x, control.x),
                        third(end.y, control.y),
                        end.x,
                        end.y,
                    ]));
                    current = (end.x, end.y);
                }
                Segment::CubicTo(first, second, end) => {
                    self.segments.push(PathSegment::Cubic([
                        first.x, first.y, second.x, second.y, end.x, end.y,
                    ]));
                    current = (end.x, end.y);
                }
                Segment::Close => {
                    current = start;
                    self.segments.push(PathSegment::Close);
                }
            }
        }
    }
}

/// What a filter primitive yields, as far as CSS filter functions can express it.
#[derive(Clone, Debug)]
enum Value {
    /// What the element draws, through these functions.
    Graphic(Vec<Filter>),
    /// The element's alpha in one color, moved and blurred: a shadow.
    Shadow { x: f32, y: f32, blur: f32, color: Rgba },
    /// A color all over the filter region.
    Flood(Rgba),
}

/// The CSS filter functions that do what the `<filter>` with the id `id` in `markup` does;
/// `None` where it does something else.
fn filter(markup: &str, id: &str) -> Option<Vec<Filter>> {
    // The filter's lengths in user units, as the box's pixels, whatever its size.
    let tree = parse_with(markup, id, "filter", 100.0, (100.0, 100.0), &FontDb::new())?;
    let filter = tree.filters().first()?;
    let mut results: Vec<(&str, Value)> = Vec::new();
    for primitive in filter.primitives() {
        let input = |input: &Input| -> Option<Value> {
            match input {
                Input::SourceGraphic => Some(Value::Graphic(Vec::new())),
                Input::SourceAlpha => Some(Value::Shadow {
                    x: 0.0,
                    y: 0.0,
                    blur: 0.0,
                    color: Rgba::BLACK,
                }),
                Input::Reference(name) => results
                    .iter()
                    .rev()
                    .find(|(result, _)| result == name)
                    .map(|(_, value)| value.clone()),
            }
        };
        let linear = primitive.color_interpolation() == ColorInterpolation::LinearRGB;
        let value = match primitive.kind() {
            Kind::GaussianBlur(blur) => {
                let (x, y) = (blur.std_dev_x().get(), blur.std_dev_y().get());
                if (x - y).abs() > 1e-3 {
                    return None;
                }
                match input(blur.input())? {
                    Value::Graphic(mut filters) => {
                        if x > 0.0 {
                            filters.push(Filter::Blur(x));
                        }
                        Value::Graphic(filters)
                    }
                    Value::Shadow {
                        x: dx,
                        y: dy,
                        blur,
                        color,
                    } => Value::Shadow {
                        x: dx,
                        y: dy,
                        blur: libm::hypotf(blur, x),
                        color,
                    },
                    flood @ Value::Flood(_) => flood,
                }
            }
            Kind::Offset(offset) => match input(offset.input())? {
                Value::Shadow { x, y, blur, color } => Value::Shadow {
                    x: x + offset.dx(),
                    y: y + offset.dy(),
                    blur,
                    color,
                },
                graphic @ Value::Graphic(_) if offset.dx() == 0.0 && offset.dy() == 0.0 => graphic,
                flood @ Value::Flood(_) => flood,
                Value::Graphic(_) => return None,
            },
            Kind::Flood(flood) => Value::Flood(color(flood.color(), flood.opacity().get())),
            Kind::DropShadow(shadow) => {
                let Value::Graphic(mut filters) = input(shadow.input())? else {
                    return None;
                };
                filters.push(Filter::DropShadow {
                    x: shadow.dx(),
                    y: shadow.dy(),
                    blur: f32::midpoint(shadow.std_dev_x().get(), shadow.std_dev_y().get()),
                    color: Some(color(shadow.color(), shadow.opacity().get())),
                });
                Value::Graphic(filters)
            }
            Kind::ColorMatrix(matrix) => {
                let values = color_matrix(matrix.kind());
                match input(matrix.input())? {
                    Value::Graphic(mut filters) => {
                        filters.push(Filter::ColorMatrix {
                            matrix: values,
                            linear,
                        });
                        Value::Graphic(filters)
                    }
                    Value::Shadow { x, y, blur, color } => Value::Shadow {
                        x,
                        y,
                        blur,
                        color: recolor_shadow(&values, color, linear)?,
                    },
                    Value::Flood(flood) => Value::Flood(
                        Filter::ColorMatrix {
                            matrix: values,
                            linear,
                        }
                        .apply(flood),
                    ),
                }
            }
            Kind::ComponentTransfer(transfer) => {
                let values = transfer_matrix([
                    transfer.func_r(),
                    transfer.func_g(),
                    transfer.func_b(),
                    transfer.func_a(),
                ])?;
                let Value::Graphic(mut filters) = input(transfer.input())? else {
                    return None;
                };
                filters.push(Filter::ColorMatrix {
                    matrix: values,
                    linear,
                });
                Value::Graphic(filters)
            }
            Kind::Composite(composite) => {
                let (top, bottom) = (input(composite.input1())?, input(composite.input2())?);
                match composite.operator() {
                    CompositeOperator::In => in_alpha(top, bottom)?,
                    CompositeOperator::Over => over(top, bottom)?,
                    _ => return None,
                }
            }
            Kind::Blend(blend) if blend.mode() == usvg::BlendMode::Normal => {
                over(input(blend.input1())?, input(blend.input2())?)?
            }
            Kind::Merge(merge) => {
                let mut layers = merge.inputs().iter();
                let mut value = input(layers.next()?)?;
                for layer in layers {
                    value = over(input(layer)?, value)?;
                }
                value
            }
            _ => return None,
        };
        results.push((primitive.result(), value));
    }
    match results.pop()? {
        (_, Value::Graphic(filters)) => Some(filters),
        _ => None,
    }
}

/// A color of usvg with an opacity.
fn color(color: usvg::Color, opacity: f32) -> Rgba {
    Rgba::from_u8(color.red, color.green, color.blue, opacity.clamp(0.0, 1.0))
}

/// `top` drawn over `bottom`: a shadow under the graphic it is the shadow of.
fn over(top: Value, bottom: Value) -> Option<Value> {
    match (top, bottom) {
        (Value::Graphic(mut filters), Value::Shadow { x, y, blur, color })
            if filters.iter().all(Filter::is_color) =>
        {
            // Color functions keep the alpha that the shadow is cast by.
            filters.push(Filter::DropShadow {
                x,
                y,
                blur,
                color: Some(color),
            });
            Some(Value::Graphic(filters))
        }
        _ => None,
    }
}

/// `top` where `bottom` is: a flood within a shadow colors it; the graphic within its own alpha
/// is the graphic.
fn in_alpha(top: Value, bottom: Value) -> Option<Value> {
    match (top, bottom) {
        (Value::Flood(flood), Value::Shadow { x, y, blur, color }) => Some(Value::Shadow {
            x,
            y,
            blur,
            color: Rgba {
                a: flood.a * color.a,
                ..flood
            },
        }),
        (Value::Shadow { x, y, blur, color }, Value::Flood(flood)) => Some(Value::Shadow {
            x,
            y,
            blur,
            color: Rgba {
                a: color.a * flood.a,
                ..color
            },
        }),
        (
            graphic @ Value::Graphic(_),
            Value::Shadow {
                x: 0.0,
                y: 0.0,
                blur: 0.0,
                ..
            },
        ) => Some(graphic),
        _ => None,
    }
}

/// The color of a shadow after a color matrix: one that sets a color and scales the alpha.
fn recolor_shadow(matrix: &[f32; 20], color: Rgba, linear: bool) -> Option<Rgba> {
    // Each channel of the result must not depend on the shadow's alpha, but its alpha only on
    // that alpha, so that the matrix leaves one color with less or more alpha.
    let constant = |row: usize| matrix[row * 5..row * 5 + 4].iter().all(|value| *value == 0.0);
    if !(constant(0) && constant(1) && constant(2)) {
        // A matrix that keeps the colors of a black shadow black.
        let keeps_black = (0..3).all(|row| matrix[row * 5 + 3] == 0.0 && matrix[row * 5 + 4] == 0.0);
        if !(keeps_black && color.r == 0.0 && color.g == 0.0 && color.b == 0.0) {
            return None;
        }
    }
    if matrix[15..18].iter().any(|value| *value != 0.0) || matrix[19] != 0.0 {
        return None;
    }
    let channel = |row: usize, own: f32| {
        if constant(row) {
            let value = matrix[row * 5 + 4].clamp(0.0, 1.0);
            if linear { from_linear(value) } else { value }
        } else {
            own
        }
    };
    Some(Rgba {
        r: channel(0, color.r),
        g: channel(1, color.g),
        b: channel(2, color.b),
        a: (color.a * matrix[18]).clamp(0.0, 1.0),
    })
}

/// A channel in linear light in sRGB.
fn from_linear(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * libm::powf(c, 1.0 / 2.4) - 0.055
    }
}

/// The 4 × 5 matrix of an `feColorMatrix` (Filter Effects 1, §9.6).
fn color_matrix(kind: &ColorMatrixKind) -> [f32; 20] {
    let rgb = |m: [f32; 9]| {
        [
            m[0], m[1], m[2], 0.0, 0.0, m[3], m[4], m[5], 0.0, 0.0, m[6], m[7], m[8], 0.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0,
        ]
    };
    match kind {
        ColorMatrixKind::Matrix(values) => {
            let mut matrix = [0.0; 20];
            for (target, value) in matrix.iter_mut().zip(values) {
                *target = *value;
            }
            matrix
        }
        ColorMatrixKind::Saturate(amount) => {
            let s = amount.get();
            rgb([
                0.213 + 0.787 * s,
                0.715 - 0.715 * s,
                0.072 - 0.072 * s,
                0.213 - 0.213 * s,
                0.715 + 0.285 * s,
                0.072 - 0.072 * s,
                0.213 - 0.213 * s,
                0.715 - 0.715 * s,
                0.072 + 0.928 * s,
            ])
        }
        ColorMatrixKind::HueRotate(degrees) => {
            let (sin, cos) = libm::sincosf(degrees.to_radians());
            rgb([
                0.213 + cos * 0.787 - sin * 0.213,
                0.715 - cos * 0.715 - sin * 0.715,
                0.072 - cos * 0.072 + sin * 0.928,
                0.213 - cos * 0.213 + sin * 0.143,
                0.715 + cos * 0.285 + sin * 0.140,
                0.072 - cos * 0.072 - sin * 0.283,
                0.213 - cos * 0.213 - sin * 0.787,
                0.715 - cos * 0.715 + sin * 0.715,
                0.072 + cos * 0.928 + sin * 0.072,
            ])
        }
        ColorMatrixKind::LuminanceToAlpha => {
            let mut matrix = [0.0; 20];
            matrix[15] = 0.2125;
            matrix[16] = 0.7154;
            matrix[17] = 0.0721;
            matrix
        }
    }
}

/// The matrix of an `feComponentTransfer` whose functions are linear (or identities); `None`
/// for tables and gamma functions.
fn transfer_matrix(functions: [&TransferFunction; 4]) -> Option<[f32; 20]> {
    let mut matrix = [0.0; 20];
    for (channel, function) in functions.into_iter().enumerate() {
        let (slope, intercept) = match function {
            TransferFunction::Identity => (1.0, 0.0),
            TransferFunction::Linear { slope, intercept } => (*slope, *intercept),
            _ => return None,
        };
        matrix[channel * 5 + channel] = slope;
        matrix[channel * 5 + 4] = intercept;
    }
    Some(matrix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_clip_paths_as_outlines() {
        let fonts = FontDb::new();
        let circle = "<clipPath id=\"c\"><circle cx=\"10\" cy=\"10\" r=\"5\"/></clipPath>";
        let Some(SvgReference::Clip(clip)) = reference(circle, "c", &fonts) else {
            panic!("a clip path");
        };
        assert!(!clip.bounding_box && !clip.even_odd);
        assert!(
            matches!(clip.segments[0], PathSegment::Move(x, y) if (x - 15.0).abs() < 1e-3 && (y - 10.0).abs() < 1e-3)
        );
        let triangle = "<clipPath id=\"t\" clipPathUnits=\"objectBoundingBox\">\
                        <polygon transform=\"scale(0.5)\" points=\"0 0, 2 0, 1 2\" clip-rule=\"evenodd\"/></clipPath>";
        let Some(SvgReference::Clip(clip)) = reference(triangle, "t", &fonts) else {
            panic!("a clip path");
        };
        assert!(clip.bounding_box && clip.even_odd);
        assert_eq!(clip.segments[1], PathSegment::Line(1.0, 0.0));
        // Two shapes of the even-odd rule overlap: drawn, not one outline.
        let two = "<clipPath id=\"e\"><rect width=\"5\" height=\"5\" clip-rule=\"evenodd\"/>\
                   <rect x=\"2\" width=\"5\" height=\"5\" clip-rule=\"evenodd\"/></clipPath>";
        assert!(matches!(
            reference(two, "e", &fonts),
            Some(SvgReference::Markup { clip: true, .. })
        ));
        assert!(matches!(
            reference(
                "<mask id=\"m\"><rect width=\"5\" height=\"5\" fill=\"#fff\"/></mask>",
                "m",
                &fonts
            ),
            Some(SvgReference::Markup { clip: false, .. })
        ));
        assert_eq!(reference("<rect id=\"r\"/>", "r", &fonts), None);
        // Percentages in user space are of the viewport of the SVG the clip path is in.
        let half = "<clipPath id=\"h\"><rect x=\"50%\" width=\"50%\" height=\"100%\"/></clipPath>";
        let Some(SvgReference::Clip(clip)) = reference_in(half, "h", &fonts, (300.0, 150.0)) else {
            panic!("a clip path");
        };
        assert_eq!(clip.segments[0], PathSegment::Move(150.0, 0.0));
        assert_eq!(clip.segments[1], PathSegment::Line(300.0, 0.0));
        assert_eq!(clip.segments[2], PathSegment::Line(300.0, 150.0));
    }

    #[test]
    fn reads_filters_as_css_functions() {
        let fonts = FontDb::new();
        let filters = |markup: &str| match reference(markup, "f", &fonts) {
            Some(SvgReference::Filter(filters)) => Some(filters.to_vec()),
            _ => None,
        };
        assert_eq!(
            filters("<filter id=\"f\"><feGaussianBlur stdDeviation=\"3\"/></filter>"),
            Some(vec![Filter::Blur(3.0)])
        );
        // The shadow of SVG editors: the alpha moved, blurred and colored, under the graphic.
        assert_eq!(
            filters(
                "<filter id=\"f\"><feOffset in=\"SourceAlpha\" dx=\"5\" dy=\"4\" result=\"o\"/>\
                 <feGaussianBlur in=\"o\" stdDeviation=\"2\" result=\"b\"/><feFlood flood-color=\"#00ff00\" flood-opacity=\"0.5\"/>\
                 <feComposite in2=\"b\" operator=\"in\" result=\"s\"/><feMerge><feMergeNode in=\"s\"/>\
                 <feMergeNode in=\"SourceGraphic\"/></feMerge></filter>"
            ),
            Some(vec![Filter::DropShadow {
                x: 5.0,
                y: 4.0,
                blur: 2.0,
                color: Some(Rgba::from_u8(0, 255, 0, 0.5)),
            }])
        );
        let gray = filters("<filter id=\"f\"><feColorMatrix type=\"saturate\" values=\"0\"/></filter>");
        let Some([Filter::ColorMatrix { matrix, linear: true }]) = gray.as_deref() else {
            panic!("a color matrix");
        };
        assert!((matrix[0] - 0.213).abs() < 1e-6 && (matrix[18] - 1.0).abs() < 1e-6);
        // Noise has no CSS function: it filters pixels.
        assert_eq!(
            filters("<filter id=\"f\"><feTurbulence baseFrequency=\"0.05\"/></filter>"),
            None
        );
        assert!(matches!(
            reference(
                "<filter id=\"f\"><feTurbulence baseFrequency=\"0.05\"/></filter>",
                "f",
                &fonts
            ),
            Some(SvgReference::PixelFilter { .. })
        ));
    }
}

//! `border-shape` (CSS Borders 4, §7): a box whose border follows basic shapes. With one shape
//! (in its `half-border-box` by default), the border is a stroke along it as wide as the border
//! at the block start, in that side's color; with two (the outer one in the border box, the
//! inner one in the padding box by default), it fills the area between them in that color. The
//! background is clipped to the shape (the outer one), and `border-radius` does not apply.

use fagus_layout::PathSegment;
use fagus_style::ComputedStyle;
use fagus_style::values::{
    BorderShape, BorderStyle, BoxArea, ClipPath, ClipShape, Rgba, ShapeReference, TransformBox, WritingMode,
};

use crate::{ClipGeometry, Item, RectF};

/// How far a cubic Bézier's control points lie along the tangents of a quarter circle, for a
/// radius of 1.
const KAPPA: f32 = 0.552_284_8;

/// Paints the background and the border of a box with `border-shape`, its border box `rect`.
#[expect(
    clippy::too_many_arguments,
    reason = "what the background is painted with, as for `corner::paint_box`"
)]
pub(crate) fn paint_box(
    style: &ComputedStyle,
    background_style: &ComputedStyle,
    rect: RectF,
    shape: &BorderShape,
    placement: crate::background::Placement,
    images: &fagus_layout::Images,
    masks: crate::background::ClipMasks<'_>,
    items: &mut Vec<Item>,
) {
    let outer = outline(&shape.outer.0, reference(style, rect, shape.outer.1));
    let Some((outer_segments, outer_even_odd)) = outer else {
        return;
    };
    // Shadows go around the outer edge of a stroke, inset ones inside its inner edge or the
    // inner shape.
    let (stroke_width, ..) = block_start(style);
    let half = if shape.inner.is_none() {
        stroke_width / 2.0
    } else {
        0.0
    };
    let inner_outline = shape
        .inner
        .as_ref()
        .and_then(|(inner, inner_box)| outline(inner, reference(style, rect, *inner_box)))
        .map(|(segments, _)| segments);
    shadows(style, rect, &grown(&outer_segments, half), false, items);
    // The background within the shape, out to the outer edge of a stroke along it (which
    // `background-clip: border-area` shows), or within the inner one of two, as in Chrome
    // (measured).
    let (background_segments, background_even_odd) = match &shape.inner {
        Some((inner, inner_box)) => match outline(inner, reference(style, rect, *inner_box)) {
            Some(inner) => inner,
            None => (grown(&outer_segments, half), outer_even_odd),
        },
        None => (grown(&outer_segments, half), outer_even_odd),
    };
    items.push(Item::PushClipShape(ClipGeometry::Path {
        segments: background_segments,
        even_odd: background_even_odd,
    }));
    // The border follows the shape, not the box's edges: it hides no background there.
    let square = crate::background::Placement {
        radii: None,
        opaque_border: false,
        ..placement
    };
    crate::background::paint_clipped(background_style, square, images, masks, items);
    items.push(Item::Pop);
    let inside = inner_outline.unwrap_or_else(|| grown(&outer_segments, -half));
    shadows(style, rect, &inside, true, items);
    let (width, color, border_style) = block_start(style);
    match &shape.inner {
        // A stroke along the shape.
        None => {
            if width <= 0.0
                || color.a <= 0.0
                || matches!(border_style, BorderStyle::None | BorderStyle::Hidden)
            {
                return;
            }
            items.push(stroke(outer_segments, width, color));
        }
        // The area between the two shapes, whatever the border's style.
        Some((inner_shape, inner_box)) => {
            let Some((inner_segments, _)) = outline(inner_shape, reference(style, rect, *inner_box)) else {
                return;
            };
            if color.a <= 0.0 {
                return;
            }
            let Some(area) = bounds(&outer_segments) else {
                return;
            };
            let mut segments = outer_segments;
            segments.extend(inner_segments);
            items.push(Item::PushClipShape(ClipGeometry::Path {
                segments,
                even_odd: true,
            }));
            items.push(Item::Rect {
                rect: area,
                color,
                radii: None,
            });
            items.push(Item::Pop);
        }
    }
}

/// The outline that the overflow of a box with `border-shape` and the border box `rect` is
/// clipped to: its shape, the inner one of two.
pub(crate) fn clip_outline(
    style: &ComputedStyle,
    rect: RectF,
    shape: &BorderShape,
) -> Option<Vec<PathSegment>> {
    let (shape, reference_box) = shape.inner.as_ref().unwrap_or(&shape.outer);
    outline(shape, reference(style, rect, *reference_box)).map(|(segments, _)| segments)
}

/// Paints the outline of a box with `border-shape` and the border box `rect`, `width` wide in
/// `color`, `offset` outside its (outer) shape; false where the shape is no outline (or the
/// offset is negative).
pub(crate) fn paint_outline(
    style: &ComputedStyle,
    rect: RectF,
    shape: &BorderShape,
    (width, offset): (f32, f32),
    color: Rgba,
    items: &mut Vec<Item>,
) -> bool {
    if offset < 0.0 {
        return false;
    }
    let Some((segments, _)) = outline(&shape.outer.0, reference(style, rect, shape.outer.1)) else {
        return false;
    };
    if width <= 0.0 {
        return true;
    }
    items.push(crate::corner::outside_clip(segments.clone()));
    if offset == 0.0 {
        // A stroke twice as wide along the shape, outside it.
        items.push(stroke(segments, 2.0 * width, color));
    } else {
        // Where a stroke twice as wide as the offset and the outline together reaches and one
        // twice as wide as the offset does not, outside the shape.
        let Some(area) = bounds(&segments) else {
            items.push(Item::Pop);
            return true;
        };
        let reach = offset + width;
        let white = Rgba::WHITE;
        items.push(Item::PushMask(std::sync::Arc::new(crate::MaskPaint {
            items: vec![
                stroke(segments.clone(), 2.0 * reach, white),
                stroke(segments, 2.0 * offset, Rgba::BLACK),
            ],
            luminance: true,
        })));
        items.push(Item::Rect {
            rect: RectF {
                x: area.x - reach,
                y: area.y - reach,
                width: area.width + 2.0 * reach,
                height: area.height + 2.0 * reach,
            },
            color,
            radii: None,
        });
        items.push(Item::Pop);
    }
    items.push(Item::Pop);
    true
}

/// The box shadows (outer or `inset` ones) of a box with `border-shape`, its border box `rect`
/// and its outline `shape`: the outline moved to each shadow and stretched by its spread.
fn shadows(style: &ComputedStyle, rect: RectF, shape: &[PathSegment], inset: bool, items: &mut Vec<Item>) {
    let mut painted = Vec::new();
    crate::shadow::paint(style, rect, None, inset, &mut painted);
    for item in painted {
        let Item::BoxShadow(shadow) = &item else {
            items.push(item);
            continue;
        };
        // The shape itself, and the shape moved by the shadow's offset and grown by its spread
        // (the differences between the rectangles the shadow has for them).
        let Some(own) = bounds(shape) else {
            continue;
        };
        let center = |rect: RectF| (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        let ((from_x, from_y), (to_x, to_y)) = (center(shadow.area), center(shadow.shape));
        let (grow_x, grow_y) = (
            (shadow.shape.width - shadow.area.width) / 2.0,
            (shadow.shape.height - shadow.area.height) / 2.0,
        );
        let target = RectF {
            x: own.x + to_x - from_x - grow_x,
            y: own.y + to_y - from_y - grow_y,
            width: (own.width + 2.0 * grow_x).max(0.0),
            height: (own.height + 2.0 * grow_y).max(0.0),
        };
        let area = shape.to_vec();
        // A polygon's sides move apart by the spread (a stretched one's would not).
        let cast = if (grow_x - grow_y).abs() < 1e-3 && polygon(shape) {
            let moved = mapped(
                shape,
                own,
                RectF {
                    x: own.x + to_x - from_x,
                    y: own.y + to_y - from_y,
                    ..own
                },
            );
            grown(&moved, grow_x)
        } else {
            mapped(shape, own, target)
        };
        if shadow.blur > 0.0 {
            items.push(Item::BoxShadow(std::sync::Arc::new(crate::BoxShadowPaint {
                outlines: Some(std::sync::Arc::new(crate::ShadowOutlines { area, shape: cast })),
                ..(**shadow).clone()
            })));
            continue;
        }
        let bounds = if inset { shadow.area } else { shadow.shape };
        if inset {
            items.push(crate::corner::clip_to(area));
            items.push(crate::corner::outside_clip(cast));
        } else {
            items.push(crate::corner::outside_clip(area));
            items.push(crate::corner::clip_to(cast));
        }
        items.push(Item::Rect {
            rect: bounds,
            color: shadow.color,
            radii: None,
        });
        items.push(Item::Pop);
        items.push(Item::Pop);
    }
}

/// An outline grown by `by` on every side (shrunk, for a negative `by`): a polygon's sides moved
/// out along their normals, meeting at mitered corners; another outline stretched around the
/// middle of its bounds so that they grow by that much.
fn grown(segments: &[PathSegment], by: f32) -> Vec<PathSegment> {
    if by == 0.0 {
        return segments.to_vec();
    }
    if polygon(segments) {
        return offset_polygon(segments, by);
    }
    match bounds(segments) {
        Some(own) => mapped(
            segments,
            own,
            RectF {
                x: own.x - by,
                y: own.y - by,
                width: (own.width + 2.0 * by).max(0.0),
                height: (own.height + 2.0 * by).max(0.0),
            },
        ),
        None => segments.to_vec(),
    }
}

/// Whether an outline is of straight lines only.
fn polygon(segments: &[PathSegment]) -> bool {
    !segments.is_empty()
        && segments
            .iter()
            .all(|segment| !matches!(segment, PathSegment::Cubic(_)))
}

/// The outline of straight lines `segments` with each closed part's sides moved out by `by`
/// along their normals (in for a negative `by`), meeting where their lines cross.
fn offset_polygon(segments: &[PathSegment], by: f32) -> Vec<PathSegment> {
    // The closed parts, each as its points (a repeated point or a closing one left out).
    let mut parts: Vec<Vec<(f32, f32)>> = Vec::new();
    for segment in segments {
        match *segment {
            PathSegment::Move(x, y) => parts.push(vec![(x, y)]),
            PathSegment::Line(x, y) => {
                if let Some(part) = parts.last_mut()
                    && part
                        .last()
                        .is_none_or(|last| (last.0 - x).abs() > 1e-4 || (last.1 - y).abs() > 1e-4)
                {
                    part.push((x, y));
                }
            }
            PathSegment::Close | PathSegment::Cubic(_) => {}
        }
    }
    let mut out = Vec::new();
    for mut points in parts {
        if points.len() > 1 {
            let (first, last) = (points[0], points[points.len() - 1]);
            if (first.0 - last.0).abs() <= 1e-4 && (first.1 - last.1).abs() <= 1e-4 {
                points.pop();
            }
        }
        let count = points.len();
        if count < 3 {
            continue;
        }
        let area: f32 = (0..count)
            .map(|index| {
                let (a, b) = (points[index], points[(index + 1) % count]);
                a.0 * b.1 - a.1 * b.0
            })
            .sum();
        let outward = if area >= 0.0 { by } else { -by };
        // Each side's line moved out: a point on it and its direction.
        let sides: Vec<((f32, f32), (f32, f32))> = (0..count)
            .map(|index| {
                let (a, b) = (points[index], points[(index + 1) % count]);
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let length = dx.hypot(dy).max(f32::EPSILON);
                let normal = (dy / length * outward, -dx / length * outward);
                ((a.0 + normal.0, a.1 + normal.1), (dx, dy))
            })
            .collect();
        for index in 0..count {
            // Where the line of the side before the corner meets that of the side after it.
            let ((from, along), (to, onward)) = (sides[(index + count - 1) % count], sides[index]);
            let denominator = along.0 * onward.1 - along.1 * onward.0;
            let point = if denominator.abs() < 1e-6 {
                to
            } else {
                let at = ((to.0 - from.0) * onward.1 - (to.1 - from.1) * onward.0) / denominator;
                (from.0 + along.0 * at, from.1 + along.1 * at)
            };
            out.push(if index == 0 {
                PathSegment::Move(point.0, point.1)
            } else {
                PathSegment::Line(point.0, point.1)
            });
        }
        out.push(PathSegment::Close);
    }
    out
}

/// An outline laid out over `from`, moved and stretched over `to`.
fn mapped(segments: &[PathSegment], from: RectF, to: RectF) -> Vec<PathSegment> {
    let scale = |length: f32, target: f32| if length > 0.0 { target / length } else { 1.0 };
    let (sx, sy) = (scale(from.width, to.width), scale(from.height, to.height));
    let point = |x: f32, y: f32| (to.x + (x - from.x) * sx, to.y + (y - from.y) * sy);
    segments
        .iter()
        .map(|segment| match *segment {
            PathSegment::Move(x, y) => {
                let (x, y) = point(x, y);
                PathSegment::Move(x, y)
            }
            PathSegment::Line(x, y) => {
                let (x, y) = point(x, y);
                PathSegment::Line(x, y)
            }
            PathSegment::Cubic([x1, y1, x2, y2, x, y]) => {
                let (x1, y1) = point(x1, y1);
                let (x2, y2) = point(x2, y2);
                let (x, y) = point(x, y);
                PathSegment::Cubic([x1, y1, x2, y2, x, y])
            }
            PathSegment::Close => PathSegment::Close,
        })
        .collect()
}

/// The width, color and style of the border at the block start of `style`'s box (its top in
/// horizontal writing, its right in `vertical-rl`, its left in `vertical-lr`), or, where that
/// side has no border, at its inline start, block end or inline end, the first that has one,
/// as in Chrome (measured).
fn block_start(style: &ComputedStyle) -> (f32, Rgba, BorderStyle) {
    use fagus_style::values::Direction;
    // Top, right, bottom, left.
    let (block_start, inline_start) = match (style.writing_mode, style.direction) {
        (WritingMode::HorizontalTb, Direction::Ltr) => (0, 3),
        (WritingMode::HorizontalTb, Direction::Rtl) => (0, 1),
        (WritingMode::VerticalRl | WritingMode::SidewaysRl, Direction::Ltr) => (1, 0),
        (WritingMode::VerticalRl | WritingMode::SidewaysRl, Direction::Rtl) => (1, 2),
        (WritingMode::VerticalLr | WritingMode::SidewaysLr, Direction::Ltr) => (3, 0),
        (WritingMode::VerticalLr | WritingMode::SidewaysLr, Direction::Rtl) => (3, 2),
    };
    let side = |index: usize| {
        let (width, color, border_style) = match index {
            0 => (
                style.border_width.top,
                style.border_color.top,
                style.border_style.top,
            ),
            1 => (
                style.border_width.right,
                style.border_color.right,
                style.border_style.right,
            ),
            2 => (
                style.border_width.bottom,
                style.border_color.bottom,
                style.border_style.bottom,
            ),
            _ => (
                style.border_width.left,
                style.border_color.left,
                style.border_style.left,
            ),
        };
        (width, style.border_color(color), border_style)
    };
    let shown = |(width, _, border_style): &(f32, Rgba, BorderStyle)| {
        *width > 0.0 && !matches!(border_style, BorderStyle::None | BorderStyle::Hidden)
    };
    [
        block_start,
        inline_start,
        (block_start + 2) % 4,
        (inline_start + 2) % 4,
    ]
    .into_iter()
    .map(side)
    .find(shown)
    .unwrap_or_else(|| side(block_start))
}

/// The box a shape is laid out in, of a box with the border box `rect`.
fn reference(style: &ComputedStyle, rect: RectF, reference: ShapeReference) -> RectF {
    let widths = &style.border_width;
    let inset = |rect: RectF, [top, right, bottom, left]: [f32; 4]| RectF {
        x: rect.x + left,
        y: rect.y + top,
        width: (rect.width - left - right).max(0.0),
        height: (rect.height - top - bottom).max(0.0),
    };
    let borders = [widths.top, widths.right, widths.bottom, widths.left];
    match reference {
        ShapeReference::HalfBorderBox => inset(rect, borders.map(|width| width / 2.0)),
        ShapeReference::Box(BoxArea::BorderBox) => rect,
        // Its margins outside (`auto` ones are none), percentages of the box's own width.
        ShapeReference::Box(BoxArea::MarginBox) => {
            let basis = rect.width;
            let margin = |side: &fagus_style::values::LengthPercentageAuto| match side {
                fagus_style::values::LengthPercentageAuto::Length(length) => length.resolve(basis),
                fagus_style::values::LengthPercentageAuto::Auto => 0.0,
            };
            let margins = &style.margin;
            inset(
                rect,
                [
                    -margin(&margins.top),
                    -margin(&margins.right),
                    -margin(&margins.bottom),
                    -margin(&margins.left),
                ],
            )
        }
        ShapeReference::Box(BoxArea::PaddingBox) => inset(rect, borders),
        ShapeReference::Box(BoxArea::ContentBox) => {
            let basis = rect.width;
            let padding = &style.padding;
            let padded = inset(rect, borders);
            inset(
                padded,
                [
                    padding.top.resolve(basis),
                    padding.right.resolve(basis),
                    padding.bottom.resolve(basis),
                    padding.left.resolve(basis),
                ],
            )
        }
    }
}

/// The outline of `shape` laid out in `reference`, and whether it is filled by the even-odd
/// rule; `None` for a shape that is no outline (a box or an SVG element).
fn outline(shape: &ClipShape, reference: RectF) -> Option<(Vec<PathSegment>, bool)> {
    if matches!(shape, ClipShape::Box | ClipShape::Url(_)) {
        return None;
    }
    let clip = ClipPath {
        shape: shape.clone(),
        reference: BoxArea::BorderBox,
        svg_reference: TransformBox::BorderBox,
    };
    // Without the box's own border and rounded corners: the reference is the box.
    let plain = ComputedStyle {
        border_radius: fagus_style::values::Corners::default(),
        border_width: fagus_style::values::Sides::all(0.0),
        ..ComputedStyle::default()
    };
    Some(match crate::clip_item(&plain, &clip, reference) {
        Item::PushClip(rect, radii) => (rounded_rect(rect, radii), false),
        Item::PushClipShape(ClipGeometry::Ellipse { center, radii }) => (ellipse(center, radii), false),
        Item::PushClipShape(ClipGeometry::Polygon { points, even_odd }) => {
            let mut segments: Vec<PathSegment> = points
                .iter()
                .enumerate()
                .map(|(index, &(x, y))| {
                    if index == 0 {
                        PathSegment::Move(x, y)
                    } else {
                        PathSegment::Line(x, y)
                    }
                })
                .collect();
            segments.push(PathSegment::Close);
            (segments, even_odd)
        }
        Item::PushClipShape(ClipGeometry::Path { segments, even_odd }) => (segments, even_odd),
        _ => return None,
    })
}

/// An ellipse as four cubic Bézier curves, clockwise from its top.
fn ellipse((cx, cy): (f32, f32), (rx, ry): (f32, f32)) -> Vec<PathSegment> {
    let (kx, ky) = (rx * KAPPA, ry * KAPPA);
    vec![
        PathSegment::Move(cx, cy - ry),
        PathSegment::Cubic([cx + kx, cy - ry, cx + rx, cy - ky, cx + rx, cy]),
        PathSegment::Cubic([cx + rx, cy + ky, cx + kx, cy + ry, cx, cy + ry]),
        PathSegment::Cubic([cx - kx, cy + ry, cx - rx, cy + ky, cx - rx, cy]),
        PathSegment::Cubic([cx - rx, cy - ky, cx - kx, cy - ry, cx, cy - ry]),
        PathSegment::Close,
    ]
}

/// A rectangle with rounded corners (top left, top right, bottom right, bottom left), clockwise.
fn rounded_rect(rect: RectF, radii: Option<crate::Radii>) -> Vec<PathSegment> {
    let [(tlx, tly), (trx, try_), (brx, bry), (blx, bly)] = radii.unwrap_or_default();
    let (left, top, right, bottom) = (rect.x, rect.y, rect.x + rect.width, rect.y + rect.height);
    vec![
        PathSegment::Move(left + tlx, top),
        PathSegment::Line(right - trx, top),
        PathSegment::Cubic([
            right - trx + trx * KAPPA,
            top,
            right,
            top + try_ - try_ * KAPPA,
            right,
            top + try_,
        ]),
        PathSegment::Line(right, bottom - bry),
        PathSegment::Cubic([
            right,
            bottom - bry + bry * KAPPA,
            right - brx + brx * KAPPA,
            bottom,
            right - brx,
            bottom,
        ]),
        PathSegment::Line(left + blx, bottom),
        PathSegment::Cubic([
            left + blx - blx * KAPPA,
            bottom,
            left,
            bottom - bly + bly * KAPPA,
            left,
            bottom - bly,
        ]),
        PathSegment::Line(left, top + tly),
        PathSegment::Cubic([
            left,
            top + tly - tly * KAPPA,
            left + tlx - tlx * KAPPA,
            top,
            left + tlx,
            top,
        ]),
        PathSegment::Close,
    ]
}

/// The bounding box of an outline's points, control points included.
fn bounds(segments: &[PathSegment]) -> Option<RectF> {
    let mut points = segments.iter().flat_map(|segment| match *segment {
        PathSegment::Move(x, y) | PathSegment::Line(x, y) => vec![(x, y)],
        PathSegment::Cubic([x1, y1, x2, y2, x, y]) => vec![(x1, y1), (x2, y2), (x, y)],
        PathSegment::Close => Vec::new(),
    });
    let first = points.next()?;
    let (mut left, mut top, mut right, mut bottom) = (first.0, first.1, first.0, first.1);
    for (x, y) in points {
        left = left.min(x);
        top = top.min(y);
        right = right.max(x);
        bottom = bottom.max(y);
    }
    Some(RectF {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}

/// A solid stroke `width` wide in `color` along a closed outline.
fn stroke(segments: Vec<PathSegment>, width: f32, color: Rgba) -> Item {
    // Far enough around everything that the ring it is cut to cuts nothing.
    const FAR: RectF = RectF {
        x: -1.0e6,
        y: -1.0e6,
        width: 2.0e6,
        height: 2.0e6,
    };
    // A stroke much wider than the curves it follows are flat shows their flattening (by the
    // PDF reader, in lines along the middle) as corners along its edges, where the curves turn
    // fast: they are drawn as lines that each turn by a degree or two at most.
    let segments: Vec<PathSegment> = if width > 8.0 {
        let mut lines = Vec::with_capacity(segments.len() * 16);
        let mut current = (0.0, 0.0);
        for segment in segments {
            match segment {
                PathSegment::Move(x, y) | PathSegment::Line(x, y) => {
                    current = (x, y);
                    lines.push(segment);
                }
                PathSegment::Cubic([x1, y1, x2, y2, x, y]) => {
                    flatten(current, (x1, y1), (x2, y2), (x, y), 0, &mut lines);
                    current = (x, y);
                }
                PathSegment::Close => lines.push(segment),
            }
        }
        lines
    } else {
        segments
    };
    let none = RectF {
        width: 0.0,
        height: 0.0,
        ..FAR
    };
    Item::DashedBorder {
        outer: FAR,
        outer_radii: [(0.0, 0.0); 4],
        inner: none,
        inner_radii: [(0.0, 0.0); 4],
        center: none,
        center_radii: [(0.0, 0.0); 4],
        color,
        style: BorderStyle::Solid,
        width,
        // The PDF output strokes 2.2 times this (see `Item::DashedBorder`).
        stroke: width / 2.2,
        centerline: Some(segments.into()),
    }
}

/// Appends the cubic Bézier curve from `p0` through the control points `p1` and `p2` to `p3` as
/// lines that each turn by less than two degrees (halved as often as needed, to a limit).
fn flatten(
    p0: (f32, f32),
    p1: (f32, f32),
    p2: (f32, f32),
    p3: (f32, f32),
    depth: u32,
    out: &mut Vec<PathSegment>,
) {
    // How far the directions of two vectors turn (radians); none where one is a point.
    let turn = |a: (f32, f32), b: (f32, f32)| {
        let lengths = a.0.hypot(a.1) * b.0.hypot(b.1);
        if lengths <= f32::EPSILON {
            0.0
        } else {
            ((a.0 * b.0 + a.1 * b.1) / lengths).clamp(-1.0, 1.0).acos()
        }
    };
    let difference = |a: (f32, f32), b: (f32, f32)| (b.0 - a.0, b.1 - a.1);
    let (d1, d2, d3) = (difference(p0, p1), difference(p1, p2), difference(p2, p3));
    let turning = turn(d1, d2) + turn(d2, d3) + turn(d1, d3);
    if depth >= 10 || turning < 2.0_f32.to_radians() {
        out.push(PathSegment::Line(p3.0, p3.1));
        return;
    }
    // Halved at its middle (de Casteljau).
    let middle = |a: (f32, f32), b: (f32, f32)| (f32::midpoint(a.0, b.0), f32::midpoint(a.1, b.1));
    let (first, second, third) = (middle(p0, p1), middle(p1, p2), middle(p2, p3));
    let (before, after) = (middle(first, second), middle(second, third));
    let half = middle(before, after);
    flatten(p0, first, before, half, depth + 1, out);
    flatten(half, after, third, p3, depth + 1, out);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_polygon_sides_apart_and_strokes_with_the_first_border() {
        // A diamond grown by 2: each side 2 out along its normal, its corners √8 further out.
        let diamond = [
            PathSegment::Move(10.0, 0.0),
            PathSegment::Line(20.0, 10.0),
            PathSegment::Line(10.0, 20.0),
            PathSegment::Line(0.0, 10.0),
            PathSegment::Close,
        ];
        let grown = grown(&diamond, 2.0);
        let near = |a: f32, b: f32| (a - b).abs() < 1e-3;
        assert!(
            matches!(grown[0], PathSegment::Move(x, y) if near(x, 10.0) && near(y, -8.0_f32.sqrt())),
            "{grown:?}"
        );
        assert!(
            matches!(grown[1], PathSegment::Line(x, y) if near(x, 20.0 + 8.0_f32.sqrt()) && near(y, 10.0))
        );
        // Vertical writing without a border at the block start (right) strokes with the inline
        // start's (top).
        let mut style = ComputedStyle {
            writing_mode: WritingMode::VerticalRl,
            ..ComputedStyle::default()
        };
        style.border_width.top = 18.0;
        style.border_style.top = BorderStyle::Solid;
        style.border_width.left = 10.0;
        style.border_style.left = BorderStyle::Solid;
        assert!(near(block_start(&style).0, 18.0));
        style.border_width.right = 6.0;
        style.border_style.right = BorderStyle::Solid;
        assert!(near(block_start(&style).0, 6.0));
    }

    #[test]
    fn lays_out_shapes_in_their_boxes() {
        let style = ComputedStyle {
            border_width: fagus_style::values::Sides::all(10.0),
            ..ComputedStyle::default()
        };
        let rect = RectF {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 120.0,
        };
        assert_eq!(
            reference(&style, rect, ShapeReference::HalfBorderBox),
            RectF {
                x: 5.0,
                y: 5.0,
                width: 110.0,
                height: 110.0
            }
        );
        assert_eq!(
            reference(&style, rect, ShapeReference::Box(BoxArea::PaddingBox)).x,
            10.0
        );
        // A circle of half the box: from the top of its reference box, clockwise.
        let circle = ClipShape::Ellipse {
            radii: (
                fagus_style::values::ShapeRadius::Length(fagus_style::values::LengthPercentage::Px(50.0)),
                fagus_style::values::ShapeRadius::Length(fagus_style::values::LengthPercentage::Px(50.0)),
            ),
            circle: true,
            center: (
                fagus_style::values::LengthPercentage::Percent(0.5),
                fagus_style::values::LengthPercentage::Percent(0.5),
            ),
        };
        let (segments, even_odd) = outline(&circle, rect).expect("an outline");
        assert!(!even_odd);
        assert_eq!(segments[0], PathSegment::Move(60.0, 10.0));
        assert_eq!(bounds(&segments).map(|area| area.width), Some(100.0));
        assert!(outline(&ClipShape::Box, rect).is_none());
        // Moved and stretched over another box (a shadow's).
        let moved = mapped(
            &segments,
            rect,
            RectF {
                x: 10.0,
                y: 0.0,
                width: 240.0,
                height: 120.0,
            },
        );
        assert_eq!(moved[0], PathSegment::Move(130.0, 10.0));
        assert!(matches!(
            stroke(segments, 10.0, Rgba::BLACK),
            Item::DashedBorder {
                centerline: Some(_),
                ..
            }
        ));
    }
}

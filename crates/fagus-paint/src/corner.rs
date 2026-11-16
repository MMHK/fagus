//! Corners of other shapes than round (`corner-shape`, CSS Borders 4): the outline of a box
//! whose corners are superellipses, and the inner edge of its border, which keeps the border's
//! thickness along the curve, as Chrome draws them.
//!
//! The geometry follows Blink's `contoured_rect.cc` and `path_builder.cc` (the corner quads, the
//! Bézier approximation of a superellipse and the border-aligned inner corners), under their
//! BSD license (`CHROMIUM-LICENSE`).
//!
//! Boxes with such corners are painted with clips to these outlines: the background with the
//! outline of its clip area, the border filled in the ring between the outer outline and the
//! inner one, sharp shadows and `overflow` clips likewise.

use fagus_layout::PathSegment;
use fagus_style::ComputedStyle;
use fagus_style::values::Rgba;

use std::sync::Arc;

use crate::{BoxShadowPaint, ClipGeometry, Item, Radii, RectF, ShadowOutlines};

/// The curvature of `round`, which needs no outline of its own.
const ROUND: f32 = 2.0;
const STRAIGHT: f32 = 1000.0;
const NOTCH: f32 = 1.0 / STRAIGHT;

type Point = (f32, f32);

fn sub(a: Point, b: Point) -> Point {
    (a.0 - b.0, a.1 - b.1)
}

fn add(a: Point, b: Point) -> Point {
    (a.0 + b.0, a.1 + b.1)
}

fn scale(a: Point, factor: f32) -> Point {
    (a.0 * factor, a.1 * factor)
}

fn length(a: Point) -> f32 {
    a.0.hypot(a.1)
}

fn normalize(a: Point) -> Point {
    let length = length(a);
    if length > 0.0 {
        scale(a, 1.0 / length)
    } else {
        (0.0, 0.0)
    }
}

fn dot(a: Point, b: Point) -> f32 {
    a.0 * b.0 + a.1 * b.1
}

fn cross(a: Point, b: Point) -> f32 {
    a.0 * b.1 - a.1 * b.0
}

/// Where the lines through `a0`–`a1` and `b0`–`b1` meet.
fn intersection(a0: Point, a1: Point, b0: Point, b1: Point) -> Option<Point> {
    let a = sub(a1, a0);
    let b = sub(b1, b0);
    let denominator = cross(a, b);
    if denominator.abs() < 1e-6 {
        return None;
    }
    Some(add(a0, scale(a, cross(sub(b0, a0), b) / denominator)))
}

/// Where the segment `a0`–`a1` meets the line through `b0`–`b1`, or the segment `b0`–`b1` the
/// line through `a0`–`a1`.
fn segment_intersection(a0: Point, a1: Point, b0: Point, b1: Point) -> Option<Point> {
    let a = sub(a1, a0);
    let b = sub(b1, b0);
    let denominator = cross(a, b);
    if denominator.abs() < 1e-6 {
        return None;
    }
    let offset = sub(b0, a0);
    let a_scale = cross(offset, b) / denominator;
    if (0.0..=1.0).contains(&a_scale) {
        return Some(add(a0, scale(a, a_scale)));
    }
    let b_scale = cross(offset, a) / denominator;
    if (0.0..=1.0).contains(&b_scale) {
        return Some(add(b0, scale(b, b_scale)));
    }
    None
}

/// A corner: the quad of its start, its outer point, its end and its center, clockwise along
/// the outline, and its curvature.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Corner {
    start: Point,
    outer: Point,
    end: Point,
    center: Point,
    curvature: f32,
}

impl Corner {
    /// The corner `index` (0 top right, 1 bottom right, 2 bottom left, 3 top left) of a box with
    /// a radius `(x, y)` there.
    fn of(rect: RectF, index: usize, (rx, ry): (f32, f32), curvature: f32) -> Self {
        let (left, top, right, bottom) = (rect.x, rect.y, rect.x + rect.width, rect.y + rect.height);
        let quad = match index {
            0 => [
                (right - rx, top),
                (right, top),
                (right, top + ry),
                (right - rx, top + ry),
            ],
            1 => [
                (right, bottom - ry),
                (right, bottom),
                (right - rx, bottom),
                (right - rx, bottom - ry),
            ],
            2 => [
                (left + rx, bottom),
                (left, bottom),
                (left, bottom - ry),
                (left + rx, bottom - ry),
            ],
            _ => [
                (left, top + ry),
                (left, top),
                (left + rx, top),
                (left + rx, top + ry),
            ],
        };
        Self::new(quad, curvature)
    }

    fn new([start, outer, end, center]: [Point; 4], curvature: f32) -> Self {
        Self {
            start,
            outer,
            end,
            center,
            curvature: curvature.clamp(NOTCH, STRAIGHT),
        }
    }

    fn v1(&self) -> Point {
        sub(self.outer, self.start)
    }

    fn v2(&self) -> Point {
        sub(self.end, self.outer)
    }

    fn v3(&self) -> Point {
        sub(self.center, self.end)
    }

    fn v4(&self) -> Point {
        sub(self.start, self.center)
    }

    fn is_empty(&self) -> bool {
        length(self.v1()) == 0.0 || length(self.v2()) == 0.0
    }

    fn inverse(&self) -> Self {
        Self::new(
            [self.start, self.center, self.end, self.outer],
            1.0 / self.curvature,
        )
    }

    fn map(&self, (x, y): Point) -> Point {
        add(add(self.center, scale(self.v1(), x)), scale(self.v4(), y))
    }

    /// The corner of an inner edge (this corner) made to keep the thickness of the border
    /// between it and `origin`, the corner of the outer edge (Blink's `AlignedToOrigin`).
    fn aligned_to(&self, origin: &Self, edge_inset_start: f32, edge_inset_end: f32) -> Self {
        if origin.is_empty() || self == origin {
            return *self;
        }
        let start_radius = length(origin.v2());
        let end_radius = length(origin.v3());
        let start_inset = if edge_inset_start >= 0.0 && length(self.v2()) == 0.0 {
            edge_inset_start
        } else {
            start_radius - length(self.v2())
        };
        let end_inset = if edge_inset_end >= 0.0 && length(self.v3()) == 0.0 {
            edge_inset_end
        } else {
            end_radius - length(self.v3())
        };
        let parameter = origin.curvature.log2();
        let exponent = parameter.abs().exp2();
        let convex_half_corner = 0.5_f32.powf(1.0 / exponent);
        let half_corner = if parameter < 0.0 {
            1.0 - convex_half_corner
        } else {
            convex_half_corner
        };
        let sqrt2 = std::f32::consts::SQRT_2;
        let control_point_x = (half_corner / (sqrt2 - 1.0) - 1.0 / sqrt2).clamp(0.0, 1.0);
        let inset_difference = (end_inset - start_inset).clamp(-start_radius, end_radius);
        let mut start_control_point_x = control_point_x;
        let mut end_control_point_x = control_point_x;
        if inset_difference != 0.0 {
            let bevel_normal_delta = (start_radius * start_radius + end_radius * end_radius
                - inset_difference * inset_difference)
                .max(0.0)
                .sqrt();
            let bevel_normal_x = end_radius * inset_difference + start_radius * bevel_normal_delta;
            let bevel_normal_y = -start_radius * inset_difference + end_radius * bevel_normal_delta;
            let bevel_control_point_x =
                start_radius * bevel_normal_y / (start_radius * bevel_normal_y + end_radius * bevel_normal_x);
            start_control_point_x = if parameter < 0.0 {
                bevel_control_point_x * (2.0 * control_point_x)
            } else {
                1.0 - (1.0 - bevel_control_point_x) * (2.0 * (1.0 - control_point_x))
            };
            end_control_point_x = 2.0 * control_point_x - start_control_point_x;
        }
        let unmapped_start_normal = normalize((
            (1.0 - start_control_point_x) * start_radius,
            start_control_point_x * end_radius,
        ));
        let unmapped_end_normal = normalize((
            end_control_point_x * start_radius,
            (1.0 - end_control_point_x) * end_radius,
        ));
        let normalized_v3 = normalize(origin.v3());
        let normalized_v2 = normalize(origin.v2());
        let start_normal = add(
            scale(normalized_v3, unmapped_start_normal.0),
            scale(normalized_v2, unmapped_start_normal.1),
        );
        let end_normal = add(
            scale(normalized_v3, unmapped_end_normal.0),
            scale(normalized_v2, unmapped_end_normal.1),
        );
        let original_outer = sub(
            sub(self.outer, scale(normalized_v3, end_inset)),
            scale(normalized_v2, start_inset),
        );
        let mut adjusted_start = add(
            add(original_outer, scale(normalized_v3, end_radius)),
            scale(start_normal, start_inset),
        );
        let mut adjusted_end = add(
            add(original_outer, scale(normalized_v2, start_radius)),
            scale(end_normal, end_inset),
        );
        let start_tangent = (-start_normal.1, start_normal.0);
        let end_tangent = (-end_normal.1, end_normal.0);
        if parameter >= 0.0 && start_inset < 0.0 {
            adjusted_start = intersection(
                adjusted_start,
                sub(adjusted_start, start_tangent),
                add(self.outer, normalized_v3),
                self.outer,
            )
            .unwrap_or(adjusted_start);
        }
        if parameter >= 0.0 && end_inset < 0.0 {
            adjusted_end = intersection(
                adjusted_end,
                add(adjusted_end, end_tangent),
                add(self.outer, normalized_v2),
                self.outer,
            )
            .unwrap_or(adjusted_end);
        }
        let adjusted_height = dot(sub(adjusted_end, adjusted_start), normalized_v2);
        let adjusted_outer = sub(adjusted_end, scale(normalized_v2, adjusted_height));
        let adjusted_center = add(adjusted_start, scale(normalized_v2, adjusted_height));
        if parameter <= -1.0 || parameter >= 0.0 {
            return Self::new(
                [adjusted_start, adjusted_outer, adjusted_end, adjusted_center],
                origin.curvature,
            );
        }
        let tangent_intersection = intersection(
            adjusted_start,
            sub(adjusted_start, start_tangent),
            adjusted_end,
            add(adjusted_end, end_tangent),
        )
        .unwrap_or(adjusted_start);
        Self::new(
            [
                adjusted_start,
                tangent_intersection,
                adjusted_end,
                add(adjusted_start, sub(adjusted_end, tangent_intersection)),
            ],
            ROUND,
        )
    }
}

/// Two control points and the end of a cubic Bézier curve along a convex superellipse of
/// `curvature`, from (0, 1) to its point at 45 degrees, in the unit square of its corner
/// (Blink's `ApproximateSuperellipseHalfCornerAsBezierCurve`).
fn half_corner_curve(curvature: f32) -> [Point; 3] {
    const P: [f64; 7] = [
        1.243_092_094_272_424_8,
        2.010_479_023_614_843,
        0.329_229_011_794_437_53,
        0.282_302_314_221_207_3,
        1.347_370_426_105_542_1,
        2.914_946_863_794_981_4,
        0.910_650_710_291_708_6,
    ];
    let s = f64::from(curvature.log2());
    let slope = P[0] + (P[6] - P[0]) * 0.5 * (1.0 + (P[5] * (s - P[1])).tanh());
    let base = 1.0 / (1.0 + (-slope * (0.0 - P[1])).exp());
    let logistic = 1.0 / (1.0 + (-slope * (s - P[1])).exp());
    let a = (logistic - base) / (1.0 - base);
    let b = P[2] * (-P[3] * s.powf(P[4])).exp();
    let half_corner = 0.5_f64.powf(1.0 / f64::from(curvature));
    #[expect(clippy::cast_possible_truncation, reason = "geometry in single precision")]
    let point = |x: f64, y: f64| (x as f32, y as f32);
    [
        point(a, 1.0),
        point(half_corner - b, half_corner + b),
        point(half_corner, half_corner),
    ]
}

/// Adds a corner to an outline: a line to its start, then its curve to its end (Blink's
/// `AddCurvedCorner`).
fn add_corner(path: &mut Vec<PathSegment>, corner: &Corner) {
    if corner.curvature < 1.0 {
        add_corner(path, &corner.inverse());
        return;
    }
    path.push(PathSegment::Line(corner.start.0, corner.start.1));
    if corner.curvature >= STRAIGHT || corner.is_empty() {
        path.push(PathSegment::Line(corner.outer.0, corner.outer.1));
        path.push(PathSegment::Line(corner.end.0, corner.end.1));
    } else if (corner.curvature - 1.0).abs() < f32::EPSILON {
        path.push(PathSegment::Line(corner.end.0, corner.end.1));
    } else if (corner.curvature - ROUND).abs() < f32::EPSILON {
        // A quarter ellipse, as a cubic curve.
        const KAPPA: f32 = 0.552_284_8;
        let first = add(corner.start, scale(corner.v1(), KAPPA));
        let second = add(corner.end, scale(sub(corner.outer, corner.end), KAPPA));
        path.push(PathSegment::Cubic([
            first.0,
            first.1,
            second.0,
            second.1,
            corner.end.0,
            corner.end.1,
        ]));
    } else {
        let [p1, p2, p3] = half_corner_curve(corner.curvature);
        let transpose = |(x, y): Point| (y, x);
        let (first, second, half) = (corner.map(p1), corner.map(p2), corner.map(p3));
        path.push(PathSegment::Cubic([
            first.0, first.1, second.0, second.1, half.0, half.1,
        ]));
        let (third, fourth) = (corner.map(transpose(p2)), corner.map(transpose(p1)));
        path.push(PathSegment::Cubic([
            third.0,
            third.1,
            fourth.0,
            fourth.1,
            corner.end.0,
            corner.end.1,
        ]));
    }
}

/// Whether a style's corners have other shapes than round where they are rounded.
pub(crate) fn is_shaped(style: &ComputedStyle, radii: Option<&Radii>) -> bool {
    // The radii in the order of the shapes.
    radii.is_some_and(|radii| {
        ordered(radii)
            .iter()
            .zip(shapes(style))
            .any(|((x, y), shape)| *x > 0.0 && *y > 0.0 && (shape - ROUND).abs() > f32::EPSILON)
    })
}

/// The curvatures of the corners in the order of the outline: top right, bottom right, bottom
/// left, top left.
fn shapes(style: &ComputedStyle) -> [f32; 4] {
    let [top_left, top_right, bottom_right, bottom_left] = style.corner_shape;
    [top_right, bottom_right, bottom_left, top_left]
}

/// The radii in the order of the outline.
fn ordered(radii: &Radii) -> [(f32, f32); 4] {
    [radii[1], radii[2], radii[3], radii[0]]
}

/// The outline of a box `rect` with corners of `radii` and the shapes of `style`.
pub(crate) fn outline(rect: RectF, radii: &Radii, style: &ComputedStyle) -> Vec<PathSegment> {
    let shapes = shapes(style);
    let radii = ordered(radii);
    let corners: Vec<Corner> = (0..4)
        .map(|index| Corner::of(rect, index, radii[index], shapes[index]))
        .collect();
    let mut path = vec![PathSegment::Move(corners[0].start.0, corners[0].start.1)];
    for corner in &corners {
        add_corner(&mut path, corner);
    }
    path.push(PathSegment::Close);
    path
}

/// The inner edge of a border `insets` (top, right, bottom, left) wide inside the outline of
/// `rect` (see [`outline`]): the box inset, less a region cut from each corner where the border
/// keeps its thickness along the curve (Blink's border-aligned corner clip-out paths).
pub(crate) struct InnerEdge {
    pub(crate) rect: RectF,
    /// The regions cut from the corners, each a closed outline turning as [`FAR_RECT`] does.
    pub(crate) cuts: Vec<Vec<PathSegment>>,
}

pub(crate) fn inner_edge(rect: RectF, radii: &Radii, insets: [f32; 4], style: &ComputedStyle) -> InnerEdge {
    let (inner, corners) = inset_corners(rect, radii, insets, style);
    let cuts = corners
        .iter()
        .filter_map(|(origin, target, inset_start, inset_end)| {
            let mut cut = corner_path(origin, target, *inset_start, *inset_end)?;
            cut.push(PathSegment::Line(target.outer.0, target.outer.1));
            cut.push(PathSegment::Close);
            Some(oriented(cut, true))
        })
        .collect();
    InnerEdge { rect: inner, cuts }
}

/// The box `rect` inset by `insets` (top, right, bottom, left; negative ones outset it), and
/// its corners in the order of the outline, each with the corner of the outline of `rect` it is
/// aligned to and its insets along the edges before and after it.
fn inset_corners(
    rect: RectF,
    radii: &Radii,
    insets: [f32; 4],
    style: &ComputedStyle,
) -> (RectF, [(Corner, Corner, f32, f32); 4]) {
    let [top, right, bottom, left] = insets;
    let inner = RectF {
        x: rect.x + left,
        y: rect.y + top,
        width: (rect.width - left - right).max(0.0),
        height: (rect.height - top - bottom).max(0.0),
    };
    let shapes = shapes(style);
    let outer_radii = ordered(radii);
    // The radii of the inset box, as for round corners: a rounded corner grows by an outset,
    // a square one stays square.
    let shrink = |radius: f32, inset: f32| {
        if radius > 0.0 {
            (radius - inset).max(0.0)
        } else {
            0.0
        }
    };
    let inner_radii = [
        (shrink(outer_radii[0].0, right), shrink(outer_radii[0].1, top)),
        (shrink(outer_radii[1].0, right), shrink(outer_radii[1].1, bottom)),
        (shrink(outer_radii[2].0, left), shrink(outer_radii[2].1, bottom)),
        (shrink(outer_radii[3].0, left), shrink(outer_radii[3].1, top)),
    ];
    let edge_insets = [(top, right), (right, bottom), (bottom, left), (left, top)];
    let corners = std::array::from_fn(|index| {
        (
            Corner::of(rect, index, outer_radii[index], shapes[index]),
            Corner::of(inner, index, inner_radii[index], shapes[index]),
            edge_insets[index].0,
            edge_insets[index].1,
        )
    });
    (inner, corners)
}

/// The outline of the box `rect` inset by `insets` (top, right, bottom, left; negative ones
/// outset it) with its corners aligned to those of the outline of `rect` (see [`outline`]), so
/// that the band between the two keeps its width along the curves, as Blink outlines a box
/// with shaped corners and outsets (the middle of a border, an outline): one closed outline,
/// clockwise from the start of the top right corner.
pub(crate) fn contour(
    rect: RectF,
    radii: &Radii,
    insets: [f32; 4],
    style: &ComputedStyle,
) -> Vec<PathSegment> {
    let (inner, corners) = inset_corners(rect, radii, insets, style);
    // The ends of an inset corner's curve lie a little outside the inset box (Blink cuts them
    // off the box): they are moved into it.
    let clamp = |x: f32, y: f32| {
        (
            x.clamp(inner.x, inner.x + inner.width),
            y.clamp(inner.y, inner.y + inner.height),
        )
    };
    let mut path: Vec<PathSegment> = Vec::new();
    for (origin, target, inset_start, inset_end) in &corners {
        let segments = corner_path(origin, target, *inset_start, *inset_end)
            .unwrap_or_else(|| vec![PathSegment::Move(target.outer.0, target.outer.1)]);
        for segment in segments {
            path.push(match segment {
                PathSegment::Move(x, y) | PathSegment::Line(x, y) => {
                    let (x, y) = clamp(x, y);
                    if path.is_empty() {
                        PathSegment::Move(x, y)
                    } else {
                        PathSegment::Line(x, y)
                    }
                }
                PathSegment::Cubic([x1, y1, x2, y2, x, y]) => {
                    let (x, y) = clamp(x, y);
                    PathSegment::Cubic([x1, y1, x2, y2, x, y])
                }
                PathSegment::Close => PathSegment::Close,
            });
        }
    }
    path.push(PathSegment::Close);
    path
}

/// A rectangle far around everything on a page, turning clockwise on the page.
const FAR_RECT: [PathSegment; 5] = [
    PathSegment::Move(-1e6, -1e6),
    PathSegment::Line(1e6, -1e6),
    PathSegment::Line(1e6, 1e6),
    PathSegment::Line(-1e6, 1e6),
    PathSegment::Close,
];

/// Whether a closed outline turns clockwise on the page (by the signed area of its points,
/// control points included).
fn clockwise(path: &[PathSegment]) -> bool {
    let mut points = Vec::new();
    for segment in path {
        match *segment {
            PathSegment::Move(x, y) | PathSegment::Line(x, y) => points.push((x, y)),
            PathSegment::Cubic([x1, y1, x2, y2, x, y]) => points.extend([(x1, y1), (x2, y2), (x, y)]),
            PathSegment::Close => {}
        }
    }
    let area: f32 = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(a, b)| cross(*a, *b))
        .sum();
    area >= 0.0
}

/// A closed outline turning clockwise, or counterclockwise.
fn oriented(path: Vec<PathSegment>, clockwise_wanted: bool) -> Vec<PathSegment> {
    if clockwise(&path) == clockwise_wanted {
        return path;
    }
    // The segments backwards: each ends where the one before it started.
    let mut starts = Vec::new();
    let mut current = (0.0, 0.0);
    for segment in &path {
        starts.push(current);
        current = match *segment {
            PathSegment::Move(x, y) | PathSegment::Line(x, y) => (x, y),
            PathSegment::Cubic([.., x, y]) => (x, y),
            PathSegment::Close => current,
        };
    }
    let mut reversed = vec![PathSegment::Move(current.0, current.1)];
    for (segment, start) in path.iter().zip(starts).rev() {
        match *segment {
            PathSegment::Line(..) => reversed.push(PathSegment::Line(start.0, start.1)),
            PathSegment::Cubic([x1, y1, x2, y2, ..]) => {
                reversed.push(PathSegment::Cubic([x2, y2, x1, y1, start.0, start.1]));
            }
            PathSegment::Move(..) | PathSegment::Close => {}
        }
    }
    reversed.push(PathSegment::Close);
    reversed
}

fn rect_path(rect: RectF) -> Vec<PathSegment> {
    let (x1, y1, x2, y2) = (rect.x, rect.y, rect.x + rect.width, rect.y + rect.height);
    vec![
        PathSegment::Move(x1, y1),
        PathSegment::Line(x2, y1),
        PathSegment::Line(x2, y2),
        PathSegment::Line(x1, y2),
        PathSegment::Close,
    ]
}

impl InnerEdge {
    /// The clips to the area inside the edge (two to pop).
    pub(crate) fn clips_inside(self, items: &mut Vec<Item>) {
        items.push(Item::PushClip(self.rect, None));
        let mut segments = FAR_RECT.to_vec();
        segments.extend(self.cuts.into_iter().flatten());
        items.push(Item::PushClipShape(ClipGeometry::Path {
            segments,
            even_odd: true,
        }));
    }

    /// What clips to the area inside the edge, as one item to pop: the inset box less the cuts.
    /// A concave corner's cut reaches out of the box into the border, and the cuts of large
    /// corners overlap, so no outline of the box and the cuts filled by a rule takes them out
    /// once (and the edges where such an outline cancels itself show as lines): a mask of
    /// luminance, white over the box and black in each cut, as Blink clips out each cut.
    pub(crate) fn clip(self) -> Item {
        if self.cuts.is_empty() {
            return Item::PushClip(self.rect, None);
        }
        let mut items = vec![Item::Rect {
            rect: self.rect,
            color: Rgba::WHITE,
            radii: None,
        }];
        // Black reaching past the box, so that only the cut's edge is blurred where it meets
        // the box's.
        let reach = self.rect.width.max(self.rect.height).max(1.0);
        let over = RectF {
            x: self.rect.x - reach,
            y: self.rect.y - reach,
            width: self.rect.width + 2.0 * reach,
            height: self.rect.height + 2.0 * reach,
        };
        // A cut's sides along the box's edges meet at the box's corner: that corner is moved
        // out (across the box's edges, where the mask is black anyway), so that the cut's edge
        // and the box's are not blurred twice.
        let (left, top) = (self.rect.x, self.rect.y);
        let (right, bottom) = (left + self.rect.width, top + self.rect.height);
        let on = |a: f32, b: f32| (a - b).abs() < 1e-4;
        let out = |(x, y): Point| {
            let corner_x = on(x, left) || on(x, right);
            let corner_y = on(y, top) || on(y, bottom);
            if corner_x && corner_y {
                (
                    if on(x, left) { x - reach } else { x + reach },
                    if on(y, top) { y - reach } else { y + reach },
                )
            } else {
                (x, y)
            }
        };
        for cut in self.cuts {
            let segments = cut
                .into_iter()
                .map(|segment| match segment {
                    PathSegment::Move(x, y) => {
                        let (x, y) = out((x, y));
                        PathSegment::Move(x, y)
                    }
                    PathSegment::Line(x, y) => {
                        let (x, y) = out((x, y));
                        PathSegment::Line(x, y)
                    }
                    other => other,
                })
                .collect();
            items.push(Item::PushClipShape(ClipGeometry::Path {
                segments,
                even_odd: false,
            }));
            items.push(Item::Rect {
                rect: over,
                color: Rgba::BLACK,
                radii: None,
            });
            items.push(Item::Pop);
        }
        Item::PushMask(Arc::new(crate::MaskPaint {
            items,
            luminance: true,
        }))
    }

    /// The clip to what lies outside the edge: outside the inset box, or in a cut.
    pub(crate) fn clip_outside(self) -> Item {
        let mut segments = FAR_RECT.to_vec();
        segments.extend(oriented(rect_path(self.rect), false));
        segments.extend(self.cuts.into_iter().flatten());
        Item::PushClipShape(ClipGeometry::Path {
            segments,
            even_odd: false,
        })
    }
}

/// The edge of the region cut from the corner `target` of the inner box, where the border with
/// the corner `origin` of the outer one keeps its thickness: from where it leaves the box's
/// edge before the corner to where it meets the edge after it (Blink's `CornerClipOutPath`
/// without the corner's outer point); `None` where nothing is cut.
fn corner_path(
    origin: &Corner,
    target: &Corner,
    edge_inset_start: f32,
    edge_inset_end: f32,
) -> Option<Vec<PathSegment>> {
    let start_radius = length(origin.v2());
    let end_radius = length(origin.v3());
    if start_radius == 0.0 || end_radius == 0.0 || origin.curvature >= STRAIGHT {
        return None;
    }
    let start_inset = if edge_inset_start >= 0.0 && length(target.v2()) == 0.0 {
        edge_inset_start
    } else {
        start_radius - length(target.v2())
    };
    let end_inset = if edge_inset_end >= 0.0 && length(target.v3()) == 0.0 {
        edge_inset_end
    } else {
        end_radius - length(target.v3())
    };
    let parameter = origin.curvature.log2();
    let inset_difference = (end_inset - start_inset).clamp(-start_radius, end_radius);
    if parameter <= 0.0
        && ((inset_difference - -start_radius).abs() < f32::EPSILON
            || (inset_difference - end_radius).abs() < f32::EPSILON)
    {
        return None;
    }
    let aligned = target.aligned_to(origin, edge_inset_start, edge_inset_end);
    let normalized_v3 = normalize(origin.v3());
    let normalized_v2 = normalize(origin.v2());
    let original_outer = sub(
        sub(target.outer, scale(normalized_v3, end_inset)),
        scale(normalized_v2, start_inset),
    );
    let original_start = add(original_outer, scale(normalized_v3, end_radius));
    let original_end = add(original_outer, scale(normalized_v2, start_radius));
    let mut miter_start = aligned.start;
    let mut miter_end = aligned.end;
    if parameter < 0.0 && start_inset < 0.0 {
        let start_normal = normalize(scale(sub(aligned.start, original_start), 1.0 / start_inset));
        let start_tangent = (-start_normal.1, start_normal.0);
        miter_start = intersection(
            aligned.start,
            sub(aligned.start, start_tangent),
            add(target.outer, normalized_v3),
            target.outer,
        )
        .unwrap_or(aligned.start);
    }
    if parameter < 0.0 && end_inset < 0.0 {
        let end_normal = normalize(scale(sub(aligned.end, original_end), 1.0 / end_inset));
        let end_tangent = (-end_normal.1, end_normal.0);
        miter_end = intersection(
            aligned.end,
            add(aligned.end, end_tangent),
            add(target.outer, normalized_v2),
            target.outer,
        )
        .unwrap_or(aligned.end);
    }
    let self_intersection = (parameter < 0.0
        && start_inset < 0.0
        && end_inset < 0.0
        && (-end_inset >= start_radius || -start_inset >= end_radius))
        .then(|| segment_intersection(miter_start, aligned.start, miter_end, aligned.end))
        .flatten();
    let mut path = vec![PathSegment::Move(miter_start.0, miter_start.1)];
    if let Some(point) = self_intersection {
        path.push(PathSegment::Line(point.0, point.1));
    } else if origin.curvature <= NOTCH {
        path.push(PathSegment::Line(aligned.center.0, aligned.center.1));
    } else {
        add_corner(&mut path, &aligned);
    }
    path.push(PathSegment::Line(miter_end.0, miter_end.1));
    Some(path)
}

/// Paints the background, shadows and border of a box with shaped corners (see
/// [`is_shaped`]): as for rounded corners, but each clipped to its outline.
#[expect(clippy::too_many_arguments, reason = "what painting a box takes")]
pub(crate) fn paint_box(
    style: &ComputedStyle,
    background_style: &ComputedStyle,
    rect: RectF,
    radii: &Radii,
    placement: crate::background::Placement,
    images: &fagus_layout::Images,
    masks: crate::background::ClipMasks<'_>,
    borders: impl FnOnce(&mut Vec<Item>),
    items: &mut Vec<Item>,
) {
    use fagus_style::values::BoxArea;
    let widths = [
        style.border_width.top,
        style.border_width.right,
        style.border_width.bottom,
        style.border_width.left,
    ];
    let outer = outline(rect, radii, style);
    shadows(style, rect, radii, false, items);
    // The background, clipped as its bottom layer is.
    let clip = match background_style
        .background_clip
        .last()
        .copied()
        .unwrap_or(BoxArea::BorderBox)
    {
        _ if placement.covers => BoxArea::BorderBox,
        BoxArea::BorderBox if placement.opaque_border => BoxArea::PaddingBox,
        clip => clip,
    };
    let pops = match clip {
        BoxArea::BorderBox | BoxArea::MarginBox => {
            items.push(clip_to(outer.clone()));
            1
        }
        BoxArea::PaddingBox | BoxArea::ContentBox => {
            let mut insets = widths;
            if clip == BoxArea::ContentBox {
                let basis = rect.width;
                let padding = &style.padding;
                insets[0] += padding.top.resolve(basis);
                insets[1] += padding.right.resolve(basis);
                insets[2] += padding.bottom.resolve(basis);
                insets[3] += padding.left.resolve(basis);
            }
            inner_edge(rect, radii, insets, style).clips_inside(items);
            2
        }
    };
    let square = crate::background::Placement {
        radii: None,
        ..placement
    };
    crate::background::paint_clipped(background_style, square, images, masks, items);
    for _ in 0..pops {
        items.push(Item::Pop);
    }
    shadows(style, rect, radii, true, items);
    // The border, square, in the ring between the outlines.
    if widths.iter().any(|width| *width > 0.0) {
        items.push(clip_to(outer));
        items.push(inner_edge(rect, radii, widths, style).clip_outside());
        borders(items);
        items.push(Item::Pop);
        items.push(Item::Pop);
    }
}

/// The mask of a background clipped to the border area of a box with shaped corners (see
/// [`crate::background::border_area`]): the ring between the shaped outline and inner edge,
/// whole where every side's style fills its part of it, else the square border, opaque, cut
/// to the ring, as the border is.
pub(crate) fn border_area(
    style: &ComputedStyle,
    rect: RectF,
    radii: &Radii,
    sides: fagus_layout::VisibleSides,
) -> Arc<crate::MaskPaint> {
    use fagus_style::values::BorderStyle;
    let widths = [
        style.border_width.top,
        style.border_width.right,
        style.border_width.bottom,
        style.border_width.left,
    ];
    let styles = [
        style.border_style.top,
        style.border_style.right,
        style.border_style.bottom,
        style.border_style.left,
    ];
    let fills = (0..4).all(|side| {
        widths[side] <= 0.0
            || !matches!(
                styles[side],
                BorderStyle::Dashed | BorderStyle::Dotted | BorderStyle::Double
            )
    });
    let mut items = vec![
        clip_to(outline(rect, radii, style)),
        inner_edge(rect, radii, widths, style).clip_outside(),
    ];
    if fills && sides == fagus_layout::VisibleSides::ALL {
        items.push(Item::Rect {
            rect,
            color: Rgba::BLACK,
            radii: None,
        });
    } else {
        let square = crate::background::border_area(style, rect, None, sides);
        items.extend(square.items.iter().cloned());
    }
    items.extend([Item::Pop, Item::Pop]);
    Arc::new(crate::MaskPaint {
        items,
        luminance: false,
    })
}

/// The solid, dashed and dotted sides of a border with shaped corners, to be clipped to its
/// ring: solid ones one fill where all have one color, dashes and dots one stroke along the
/// middle of the border where all sides have one style, width and color, as Chrome strokes
/// them (Blink's `DrawCurvedDashedDottedBoxSide`); else each side in the wedge from its outer
/// edge towards the box's middle, split from the outer corners along the diagonals to the
/// inner ones, as Chrome splits them. `None` where a side has another style, which
/// [`crate::paint_borders`] draws square.
pub(crate) fn styled_border(
    style: &ComputedStyle,
    rect: RectF,
    radii: &Radii,
    items: &mut Vec<Item>,
) -> Option<()> {
    use fagus_style::values::BorderStyle;
    let widths = [
        style.border_width.top,
        style.border_width.right,
        style.border_width.bottom,
        style.border_width.left,
    ];
    let styles = [
        style.border_style.top,
        style.border_style.right,
        style.border_style.bottom,
        style.border_style.left,
    ];
    let colors = crate::border_colors(style);
    let shown =
        |side: usize| widths[side] > 0.0 && !matches!(styles[side], BorderStyle::None | BorderStyle::Hidden);
    if (0..4).any(|side| {
        shown(side)
            && !matches!(
                styles[side],
                BorderStyle::Solid | BorderStyle::Dashed | BorderStyle::Dotted
            )
    }) {
        return None;
    }
    // The middle of the border, cut to whole pixels, along which dashes and dots go.
    let centerline = || -> Arc<[PathSegment]> {
        contour(rect, radii, widths.map(|width| (width / 2.0).trunc()), style).into()
    };
    let dashes = |side: usize, stroke: f32| Item::DashedBorder {
        outer: rect,
        outer_radii: [(0.0, 0.0); 4],
        inner: RectF {
            width: 0.0,
            height: 0.0,
            ..rect
        },
        inner_radii: [(0.0, 0.0); 4],
        center: rect,
        center_radii: [(0.0, 0.0); 4],
        color: colors[side],
        style: styles[side],
        width: widths[side],
        stroke,
        centerline: Some(centerline()),
    };
    if (0..4).all(|side| {
        shown(side)
            && colors[side] == colors[0]
            && styles[side] == styles[0]
            && (styles[0] == BorderStyle::Solid || widths[side].to_bits() == widths[0].to_bits())
    }) {
        items.push(if styles[0] == BorderStyle::Solid {
            Item::Rect {
                rect,
                color: colors[0],
                radii: None,
            }
        } else {
            dashes(0, widths[0])
        });
        return Some(());
    }
    let [top, right, bottom, left] = widths;
    let (x1, y1, x2, y2) = (rect.x, rect.y, rect.x + rect.width, rect.y + rect.height);
    // How far the wedges reach in: past the deepest corner.
    let deepest = radii.iter().map(|(x, y)| x.max(*y)).fold(0.0_f32, f32::max);
    let thinnest = widths
        .iter()
        .copied()
        .filter(|width| *width > 0.0)
        .fold(f32::INFINITY, f32::min);
    let reach = (deepest / thinnest).max(1.0) * 2.0;
    let toward = |corner: Point, inner: Point| add(corner, scale(sub(inner, corner), reach));
    let corners = [(x1, y1), (x2, y1), (x2, y2), (x1, y2)];
    let inner = [
        (x1 + left, y1 + top),
        (x2 - right, y1 + top),
        (x2 - right, y2 - bottom),
        (x1 + left, y2 - bottom),
    ];
    for side in 0..4 {
        if !shown(side) {
            continue;
        }
        let (a, b) = (side, (side + 1) % 4);
        let points = [
            corners[a],
            corners[b],
            toward(corners[b], inner[b]),
            toward(corners[a], inner[a]),
        ];
        if styles[side] == BorderStyle::Solid {
            items.push(Item::BorderSide {
                points,
                color: colors[side],
                style: BorderStyle::Solid,
                width: widths[side],
            });
            continue;
        }
        // A stroke as wide as the widest of the side and the sides next to it.
        let stroke = widths[side]
            .max(widths[(side + 1) % 4])
            .max(widths[(side + 3) % 4]);
        items.push(Item::PushClipShape(ClipGeometry::Polygon {
            points: points.to_vec(),
            even_odd: false,
        }));
        items.push(dashes(side, stroke));
        items.push(Item::Pop);
    }
    Some(())
}

/// Paints the outline of a box with shaped corners (see [`is_shaped`]) and border box `rect`:
/// the band from `offset` to `offset + width` outside the box, its corners aligned to the
/// box's, as Chrome draws it; dashes and dots along its middle.
#[expect(clippy::too_many_arguments, reason = "what an outline takes")]
pub(crate) fn paint_outline(
    style: &ComputedStyle,
    rect: RectF,
    radii: &Radii,
    line: fagus_style::values::BorderStyle,
    width: f32,
    offset: f32,
    color: Rgba,
    items: &mut Vec<Item>,
) {
    use fagus_style::values::BorderStyle;
    let outer = -(offset + width);
    let bounds = RectF {
        x: rect.x + outer,
        y: rect.y + outer,
        width: (rect.width - 2.0 * outer).max(0.0),
        height: (rect.height - 2.0 * outer).max(0.0),
    };
    items.push(clip_to(contour(rect, radii, [outer; 4], style)));
    items.push(outside_clip(contour(rect, radii, [-offset; 4], style)));
    items.push(match line {
        BorderStyle::Dashed | BorderStyle::Dotted => Item::DashedBorder {
            outer: bounds,
            outer_radii: [(0.0, 0.0); 4],
            inner: RectF {
                width: 0.0,
                height: 0.0,
                ..bounds
            },
            inner_radii: [(0.0, 0.0); 4],
            center: bounds,
            center_radii: [(0.0, 0.0); 4],
            color,
            style: line,
            width,
            stroke: width,
            centerline: Some(contour(rect, radii, [outer + (width / 2.0).trunc(); 4], style).into()),
        },
        _ => Item::Rect {
            rect: bounds,
            color,
            radii: None,
        },
    });
    items.push(Item::Pop);
    items.push(Item::Pop);
}

/// Paints the shadows of a box with shaped corners: sharp ones clipped to the shapes, blurred
/// ones with the shapes' outlines.
fn shadows(style: &ComputedStyle, rect: RectF, radii: &Radii, inset: bool, items: &mut Vec<Item>) {
    let mut painted = Vec::new();
    crate::shadow::paint(style, rect, Some(*radii), inset, &mut painted);
    for item in painted {
        let Item::BoxShadow(shadow) = &item else {
            items.push(item);
            continue;
        };
        let shape_of = |area: RectF, radii: Option<Radii>| match radii {
            Some(radii) => outline(area, &radii, style),
            None => outline(area, &[(0.0, 0.0); 4], style),
        };
        let area = shape_of(shadow.area, shadow.area_radii);
        let shape = shape_of(shadow.shape, shadow.shape_radii);
        if shadow.blur > 0.0 {
            items.push(Item::BoxShadow(Arc::new(BoxShadowPaint {
                outlines: Some(Arc::new(ShadowOutlines { area, shape })),
                ..(**shadow).clone()
            })));
            continue;
        }
        let bounds = if inset { shadow.area } else { shadow.shape };
        if inset {
            items.push(clip_to(area));
            items.push(outside_clip(shape));
        } else {
            items.push(outside_clip(area));
            items.push(clip_to(shape));
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

/// A clip to an outline.
pub(crate) fn clip_to(segments: Vec<PathSegment>) -> Item {
    Item::PushClipShape(ClipGeometry::Path {
        segments,
        even_odd: false,
    })
}

/// A clip to what lies outside an outline (near it: within a million pixels).
pub(crate) fn outside_clip(outline: Vec<PathSegment>) -> Item {
    const FAR: f32 = 1e6;
    let mut segments = vec![
        PathSegment::Move(-FAR, -FAR),
        PathSegment::Line(FAR, -FAR),
        PathSegment::Line(FAR, FAR),
        PathSegment::Line(-FAR, FAR),
        PathSegment::Close,
    ];
    segments.extend(outline);
    Item::PushClipShape(ClipGeometry::Path {
        segments,
        even_odd: true,
    })
}

/// The factor that scales the `radii` of a box `width` by `height` with concave corners
/// (`scoop`, `notch`) so that the hulls of opposite corners do not overlap (Blink's
/// `RadiiConstraintFactorForOppositeCorners`); 1 where they do not, and for boxes whose corners
/// are all convex.
pub(crate) fn opposite_scale(width: f32, height: f32, radii: &Radii, style: &ComputedStyle) -> f32 {
    let rect = RectF {
        x: 0.0,
        y: 0.0,
        width,
        height,
    };
    let radii = ordered(radii);
    // A corner without a radius counts as round.
    let shapes = shapes(style);
    let curvatures: [f32; 4] = std::array::from_fn(|index| {
        if radii[index].0 > 0.0 && radii[index].1 > 0.0 {
            shapes[index]
        } else {
            ROUND
        }
    });
    if curvatures.iter().all(|curvature| *curvature >= 1.0) {
        return 1.0;
    }
    let corners: [Corner; 4] =
        std::array::from_fn(|index| Corner::of(rect, index, radii[index], curvatures[index]));
    // Top left with bottom right, bottom left with top right.
    opposite_factor(&corners[3], &corners[1])
        .min(opposite_factor(&corners[2], &corners[0]))
        .min(1.0)
}

/// The bounding box of a corner's start and end: left, top, right, bottom.
fn bounds(corner: &Corner) -> [f32; 4] {
    [
        corner.start.0.min(corner.end.0),
        corner.start.1.min(corner.end.1),
        corner.start.0.max(corner.end.0),
        corner.start.1.max(corner.end.1),
    ]
}

/// The hull of a corner's curve: its start, where the tangent at its middle meets the lines
/// from its start and end to its center, and its end (Blink's `ComputeHullQuad`).
fn hull(corner: &Corner) -> [Point; 4] {
    let half = 0.5_f32.powf(1.0 / corner.curvature);
    let middle = corner.map((half, half));
    let direction = sub(middle, corner.outer);
    let along = add(middle, (-direction.1, direction.0));
    let meet = |from: Point| line_intersection(middle, along, from, corner.center).unwrap_or(corner.center);
    [corner.start, meet(corner.start), meet(corner.end), corner.end]
}

/// Where the lines (not segments) through `a0`–`a1` and `b0`–`b1` meet.
fn line_intersection(a0: Point, a1: Point, b0: Point, b1: Point) -> Option<Point> {
    let (a, b) = (sub(a1, a0), sub(b1, b0));
    let denominator = cross(a, b);
    if denominator.abs() < 1e-9 {
        return None;
    }
    Some(add(a0, scale(a, cross(sub(b0, a0), b) / denominator)))
}

/// `quad` scaled by `factor` around `origin`.
fn scaled_quad(quad: &[Point; 4], origin: Point, factor: f32) -> [Point; 4] {
    quad.map(|point| add(origin, scale(sub(point, origin), factor)))
}

/// Whether two quads (convex, or flat) overlap or touch: no axis of their edges separates them.
fn quads_intersect(a: &[Point; 4], b: &[Point; 4]) -> bool {
    let separated_along = |quad: &[Point; 4]| {
        (0..4).any(|index| {
            let edge = sub(quad[(index + 1) % 4], quad[index]);
            let axis = (-edge.1, edge.0);
            if length(axis) == 0.0 {
                return false;
            }
            let project = |points: &[Point; 4]| {
                points
                    .iter()
                    .fold((f32::INFINITY, f32::NEG_INFINITY), |(low, high), point| {
                        let at = dot(*point, axis);
                        (low.min(at), high.max(at))
                    })
            };
            let (a_low, a_high) = project(a);
            let (b_low, b_high) = project(b);
            a_high < b_low || b_high < a_low
        })
    };
    !separated_along(a) && !separated_along(b)
}

/// The factor for two opposite corners (see [`opposite_scale`]): where their hulls overlap,
/// the largest scale (to 0.05) at which, scaled around their outer points, they do not.
fn opposite_factor(a: &Corner, b: &Corner) -> f32 {
    let ([a_left, a_top, a_right, a_bottom], [b_left, b_top, b_right, b_bottom]) = (bounds(a), bounds(b));
    if a_right <= b_left || b_right <= a_left || a_bottom <= b_top || b_bottom <= a_top {
        return 1.0;
    }
    let (hull_a, hull_b) = (hull(a), hull(b));
    let near = 1.0 - 5.0 * f32::EPSILON;
    if !quads_intersect(
        &scaled_quad(&hull_a, a.outer, near),
        &scaled_quad(&hull_b, b.outer, near),
    ) {
        return 1.0;
    }
    let (mut low, mut high) = (0.0_f32, 1.0_f32);
    while high - low > 0.05 {
        let middle = f32::midpoint(low, high);
        if quads_intersect(
            &scaled_quad(&hull_a, a.outer, middle),
            &scaled_quad(&hull_b, b.outer, middle),
        ) {
            high = middle;
        } else {
            low = middle;
        }
    }
    low
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_boxes_with_one_shaped_corner() {
        // A scoop at the top left alone, the only corner with a radius.
        let scoop = ComputedStyle {
            corner_shape: [0.5, 2.0, 2.0, 2.0],
            ..ComputedStyle::default()
        };
        let radii = [(40.0, 40.0), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0)];
        assert!(is_shaped(&scoop, Some(&radii)));
        let round_there = [(0.0, 0.0), (40.0, 40.0), (0.0, 0.0), (0.0, 0.0)];
        assert!(!is_shaped(&scoop, Some(&round_there)));
    }

    #[test]
    fn scales_concave_opposite_corners_apart() {
        // Two scoops of 220 pixels in a box of 240 meet in the middle: scaled down (Chrome 157).
        let scoop = ComputedStyle {
            corner_shape: [0.5, 2.0, 0.5, 2.0],
            ..ComputedStyle::default()
        };
        let radii = [(220.0, 220.0), (0.0, 0.0), (220.0, 220.0), (0.0, 0.0)];
        let factor = opposite_scale(240.0, 240.0, &radii, &scoop);
        assert!(factor < 0.9 && factor > 0.4, "{factor}");
        // Convex corners keep their radii; so do small concave ones.
        assert!((opposite_scale(240.0, 240.0, &radii, &ComputedStyle::default()) - 1.0).abs() < 1e-6);
        let small = [(20.0, 20.0), (0.0, 0.0), (20.0, 20.0), (0.0, 0.0)];
        assert!((opposite_scale(240.0, 240.0, &small, &scoop) - 1.0).abs() < 1e-6);
    }

    fn style(shape: f32) -> ComputedStyle {
        ComputedStyle {
            corner_shape: [shape; 4],
            ..ComputedStyle::default()
        }
    }

    fn points(path: &[PathSegment]) -> Vec<Point> {
        path.iter()
            .filter_map(|segment| match *segment {
                PathSegment::Move(x, y) | PathSegment::Line(x, y) => Some((x, y)),
                PathSegment::Cubic([.., x, y]) => Some((x, y)),
                PathSegment::Close => None,
            })
            .collect()
    }

    const BOX: RectF = RectF {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 60.0,
    };

    #[test]
    fn bevels_cut_the_corners_straight() {
        let path = outline(BOX, &[(20.0, 20.0); 4], &style(1.0));
        assert_eq!(
            points(&path),
            vec![
                (80.0, 0.0),
                (80.0, 0.0),
                (100.0, 20.0),
                (100.0, 40.0),
                (80.0, 60.0),
                (20.0, 60.0),
                (0.0, 40.0),
                (0.0, 20.0),
                (20.0, 0.0),
            ]
        );
        assert!(
            !path
                .iter()
                .any(|segment| matches!(segment, PathSegment::Cubic(_)))
        );
    }

    #[test]
    fn notches_go_close_to_the_corner_center() {
        let path = outline(BOX, &[(20.0, 10.0); 4], &style(0.0));
        let points = points(&path);
        // The top right corner: from its start in to near its center, then out to its end.
        assert_eq!(points[1], (80.0, 0.0));
        assert!(
            (points[2].0 - 80.0).abs() < 0.05 && (points[2].1 - 10.0).abs() < 0.05,
            "{points:?}"
        );
        assert_eq!(points[3], (100.0, 10.0));
    }

    #[test]
    fn squircles_pass_their_half_corner_point() {
        let path = outline(BOX, &[(20.0, 20.0); 4], &style(4.0));
        let PathSegment::Cubic([.., x, y]) = path[2] else {
            panic!("a curve: {path:?}");
        };
        // 0.5^(1/4) of the way from the corner's center to its outer point.
        let half = 0.5_f32.powf(0.25) * 20.0;
        assert!(
            (x - (80.0 + half)).abs() < 1e-3 && (y - (20.0 - half)).abs() < 1e-3,
            "{x} {y}"
        );
    }

    #[test]
    fn clips_out_overlapping_cuts_whole() {
        // Corners of half the box: the scoop's cut (top right) reaches into the notch's
        // (bottom right), and both reach out of the inset box into the border.
        let shapes = ComputedStyle {
            corner_shape: [1.0, 0.5, NOTCH, STRAIGHT],
            ..ComputedStyle::default()
        };
        let rect = RectF {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 200.0,
        };
        let Item::PushMask(mask) = inner_edge(rect, &[(100.0, 100.0); 4], [20.0; 4], &shapes).clip() else {
            panic!("a mask");
        };
        assert!(mask.luminance);
        let inset = RectF {
            x: 20.0,
            y: 20.0,
            width: 160.0,
            height: 160.0,
        };
        assert_eq!(
            mask.items[0],
            Item::Rect {
                rect: inset,
                color: Rgba::WHITE,
                radii: None
            }
        );
        // Each cut black over the box, as it is: the scoop's curve starts in the border.
        assert_eq!(mask.items.len(), 1 + 3 * 3);
        for cut in mask.items[1..].chunks(3) {
            assert!(matches!(
                &cut[0],
                Item::PushClipShape(ClipGeometry::Path { even_odd: false, .. })
            ));
            assert!(matches!(cut[1], Item::Rect { color, .. } if color == Rgba::BLACK));
            assert_eq!(cut[2], Item::Pop);
        }
        let Item::PushClipShape(ClipGeometry::Path { segments, .. }) = &mask.items[1] else {
            panic!("a cut");
        };
        assert!(points(segments).contains(&(200.0, 120.0)), "{segments:?}");
        // Square corners cut nothing.
        assert_eq!(
            inner_edge(rect, &[(10.0, 10.0); 4], [20.0; 4], &style(STRAIGHT)).clip(),
            Item::PushClip(inset, None)
        );
    }

    #[test]
    fn inner_bevel_keeps_the_border_thickness() {
        let inner = inner_edge(BOX, &[(20.0, 20.0); 4], [4.0; 4], &style(1.0));
        assert_eq!(
            inner.rect,
            RectF {
                x: 4.0,
                y: 4.0,
                width: 92.0,
                height: 52.0
            }
        );
        assert_eq!(inner.cuts.len(), 4);
        assert!(inner.cuts.iter().all(|cut| clockwise(cut)));
        let points = points(&inner.cuts[0]);
        // The inner top right bevel runs parallel to the outer one, 4px in: the cut has two
        // distinct points on that line (and the inner box's corner).
        let distance = |(x, y): Point| (x - y - 80.0).abs() / std::f32::consts::SQRT_2;
        let mut on_line: Vec<Point> = points
            .iter()
            .copied()
            .filter(|point| (distance(*point) - 4.0).abs() < 0.01)
            .collect();
        on_line.dedup();
        assert!(on_line.len() >= 2, "{points:?}");
        assert!(points.contains(&(96.0, 4.0)), "{points:?}");
    }

    #[test]
    fn outsets_keep_bevels_parallel() {
        // An outline 4px out of a box with bevels of 30px: the top edge starts where a line
        // 4px out of the bevel meets it, as Chrome draws it.
        let rect = RectF {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 80.0,
        };
        let path = contour(rect, &[(30.0, 30.0); 4], [-4.0; 4], &style(1.0));
        let found = points(&path);
        assert!(matches!(path.last(), Some(PathSegment::Close)));
        let expected = 90.0 + 4.0 * (std::f32::consts::SQRT_2 - 1.0);
        assert!(
            found
                .iter()
                .any(|(x, y)| (x - expected).abs() < 0.01 && (y + 4.0).abs() < 0.01),
            "{found:?}"
        );
        // No outset: the outline itself.
        assert_eq!(
            points(&contour(BOX, &[(20.0, 20.0); 4], [0.0; 4], &style(1.0)))[..3],
            points(&outline(BOX, &[(20.0, 20.0); 4], &style(1.0)))[..3]
        );
    }

    #[test]
    fn strokes_dashes_along_the_shape() {
        use fagus_style::values::{BorderStyle, Sides};
        let dashed = ComputedStyle {
            border_width: Sides::all(6.0),
            border_style: Sides::all(BorderStyle::Dashed),
            ..style(1.0)
        };
        let mut items = Vec::new();
        assert!(styled_border(&dashed, BOX, &[(20.0, 20.0); 4], &mut items).is_some());
        let [
            Item::DashedBorder {
                centerline: Some(centerline),
                stroke,
                ..
            },
        ] = items.as_slice()
        else {
            panic!("one stroke along the middle: {items:?}");
        };
        assert_eq!(*stroke, 6.0);
        // Along the middle of the border, 3px in, from the top right bevel.
        let (x, y) = points(centerline)[0];
        assert!((x - 78.0).abs() < 1.0 && (y - 3.0).abs() < 1e-3, "{x} {y}");
        // A side of another color strokes the middle in its wedge.
        let mixed = ComputedStyle {
            border_color: Sides {
                top: Some(Rgba::WHITE),
                ..Sides::all(None)
            },
            ..dashed
        };
        let mut items = Vec::new();
        assert!(styled_border(&mixed, BOX, &[(20.0, 20.0); 4], &mut items).is_some());
        assert_eq!(
            items
                .iter()
                .filter(|item| matches!(item, Item::DashedBorder { .. }))
                .count(),
            4
        );
        // Double borders are drawn square.
        let double = ComputedStyle {
            border_style: Sides::all(BorderStyle::Double),
            ..mixed
        };
        assert!(styled_border(&double, BOX, &[(20.0, 20.0); 4], &mut Vec::new()).is_none());
    }

    #[test]
    fn blurred_shadows_and_outlines_take_the_shape() {
        use fagus_style::values::{BorderStyle, BoxShadow, Rgba};
        let shadowed = ComputedStyle {
            box_shadow: Arc::from([BoxShadow {
                x: 0.0,
                y: 0.0,
                blur: 10.0,
                spread: 0.0,
                color: Some(Rgba::BLACK),
                inset: false,
            }]),
            ..style(1.0)
        };
        let mut items = Vec::new();
        shadows(&shadowed, BOX, &[(20.0, 20.0); 4], false, &mut items);
        let [Item::BoxShadow(shadow)] = items.as_slice() else {
            panic!("one shadow: {items:?}");
        };
        let outlines = shadow.outlines.as_ref().expect("the shape's outlines");
        assert_eq!(outlines.shape, outline(BOX, &[(20.0, 20.0); 4], &shadowed));
        // An outline 2px wide and 3px away follows the bevels.
        let mut items = Vec::new();
        paint_outline(
            &style(1.0),
            BOX,
            &[(20.0, 20.0); 4],
            BorderStyle::Solid,
            2.0,
            3.0,
            Rgba::BLACK,
            &mut items,
        );
        assert!(matches!(
            items.as_slice(),
            [
                Item::PushClipShape(_),
                Item::PushClipShape(_),
                Item::Rect { .. },
                Item::Pop,
                Item::Pop
            ]
        ));
        let Item::Rect { rect, .. } = &items[2] else {
            unreachable!()
        };
        assert_eq!((rect.x, rect.width), (-5.0, 110.0));
    }
}

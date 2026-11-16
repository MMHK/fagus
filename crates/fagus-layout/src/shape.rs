//! The float area of `shape-outside` (CSS Shapes 1): the shape that the lines beside a float
//! wrap around, instead of its margin box.
//!
//! A line box takes the extent of the shape over its whole height, as Chrome measures it. The
//! geometry follows Blink's shapes (`core/layout/shapes`, under the license below):
//! ellipses and rounded rectangles grow by `shape-margin` in their radii, polygons by offset
//! edges and circles about their vertices, and images by a disc about each row of their
//! pixels.

// Copyright (C) 2012 Adobe Systems Incorporated. All rights reserved.
//
// Redistribution and use in source and binary forms, with or without modification, are
// permitted provided that the following conditions are met:
//
// 1. Redistributions of source code must retain the above copyright notice, this list of
//    conditions and the following disclaimer.
// 2. Redistributions in binary form must reproduce the above copyright notice, this list of
//    conditions and the following disclaimer in the documentation and/or other materials
//    provided with the distribution.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS
// OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF
// MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE
// COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
// EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE
// GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND
// ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
// NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED
// OF THE POSSIBILITY OF SUCH DAMAGE.

use fagus_style::ComputedStyle;
use fagus_style::values::{
    ClipShape, Corners, Image, LengthPercentage, ShapeBox, ShapeOutside, ShapeRadius, WritingMode,
};

use crate::PathSegment;

/// The alpha channel of an image, for `shape-outside: url(…)`: one byte per pixel, row by row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlphaMask {
    pub width: u32,
    pub height: u32,
    pub alpha: Vec<u8>,
}

/// The float area of a float, in coordinates whose origin is the top left corner of its
/// margin box.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FloatShape {
    geometry: Geometry,
    /// `shape-margin`, in pixels.
    margin: f32,
    /// Where the reference box starts in the margin box.
    origin: (f32, f32),
    /// The width of the margin box, to which the excluded part of a line is clamped.
    width: f32,
}

#[derive(Clone, Debug, PartialEq)]
enum Geometry {
    Ellipse {
        center: (f32, f32),
        radii: (f32, f32),
    },
    /// A rectangle (x, y, width, height) with the radii of its corners: top left, top right,
    /// bottom right, bottom left.
    RoundedRect {
        rect: [f32; 4],
        radii: [(f32, f32); 4],
    },
    Polygon(Vec<(f32, f32)>),
    /// The outlines of `path()`, cut into lines: rasterized in the end (see [`rasterized`]).
    Outlines(Vec<Vec<(f32, f32)>>),
    /// The rows of pixels of an image that are inside the shape, from row `top` on: the start
    /// and end of each (end exclusive), with the shape margin added.
    Raster {
        top: i32,
        rows: Vec<Option<(i32, i32)>>,
    },
}

/// The sizes of a float's box: margin box size, and its margins, borders and padding (top,
/// right, bottom, left), in pixels.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FloatBox {
    pub(crate) size: (f32, f32),
    pub(crate) margin: [f32; 4],
    pub(crate) border: [f32; 4],
    pub(crate) padding: [f32; 4],
    /// Whether the float is a replaced element, whose `shape-outside` image fills its content
    /// box rather than taking its natural size.
    pub(crate) replaced: bool,
}

impl FloatShape {
    /// The float area of a float with `style` and box `float`, whose containing block is
    /// `containing_width` wide; `image` is the alpha of its `shape-outside` image, if it has
    /// one and it loaded. `None` where the float area is the margin box. The float is laid out
    /// in writing mode `mode`, where its box and its style are turned: the shape, given on the
    /// page, turns with them.
    pub(crate) fn new(
        style: &ComputedStyle,
        float: FloatBox,
        containing_width: f32,
        image: Option<&AlphaMask>,
        mode: WritingMode,
    ) -> Option<Self> {
        let value = style.shape_outside.as_deref()?;
        let margin = style.shape_margin.resolve(containing_width).max(0.0);
        let inset = |reference: ShapeBox| -> [f32; 4] {
            let side = |index: usize| match reference {
                ShapeBox::MarginBox => 0.0,
                ShapeBox::BorderBox => float.margin[index],
                ShapeBox::PaddingBox => float.margin[index] + float.border[index],
                ShapeBox::ContentBox => float.margin[index] + float.border[index] + float.padding[index],
            };
            [side(0), side(1), side(2), side(3)]
        };
        let (reference, geometry) = match value {
            ShapeOutside::Shape { shape, reference } => {
                let inset = inset(*reference);
                let size = (
                    (float.size.0 - inset[1] - inset[3]).max(0.0),
                    (float.size.1 - inset[0] - inset[2]).max(0.0),
                );
                let geometry = if mode.is_vertical() {
                    let page = (size.1, size.0);
                    turned(basic_shape(shape, page, &style.border_radius), mode, page)
                } else {
                    basic_shape(shape, size, &style.border_radius)
                };
                // Paths are drawn into pixels whose rows the float area takes, as in Chrome
                // (Blink's `Shape::CreateRasterShapeFromPath`).
                let geometry = match geometry {
                    Geometry::Outlines(outlines) => rasterized(&outlines, margin),
                    other => other,
                };
                (inset, geometry)
            }
            ShapeOutside::Image(source) => {
                let inset = inset(ShapeBox::ContentBox);
                let size = (
                    (float.size.0 - inset[1] - inset[3]).max(0.0),
                    (float.size.1 - inset[0] - inset[2]).max(0.0),
                );
                // A gradient is drawn in the content box (Blink's image size is that of the
                // reference box), on the page.
                let drawn;
                let image = match source {
                    Image::Gradient(gradient) => {
                        let page = if mode.is_vertical() {
                            (size.1, size.0)
                        } else {
                            size
                        };
                        drawn = crate::shape_gradient::alpha(gradient, page.0, page.1, style.color.a)?;
                        &drawn
                    }
                    _ => image?,
                };
                // The image of a replaced element where it draws it, from its border box
                // (which Chrome takes as from the content box); others' at its natural size,
                // from the top left corner of the box on the page.
                #[allow(clippy::cast_precision_loss, reason = "image sizes are far below 2^24")]
                let natural = (image.width as f32, image.height as f32);
                let turned_image;
                let (image, rect) = if mode.is_vertical() {
                    turned_image = turned_mask(image, mode);
                    let page = (size.1, size.0);
                    let rect = if float.replaced {
                        [
                            float.border[3] + float.padding[3],
                            float.border[0] + float.padding[0],
                            size.0,
                            size.1,
                        ]
                    } else {
                        turned_rect([0.0, 0.0, natural.0, natural.1], mode, page)
                    };
                    (&turned_image, rect)
                } else if float.replaced {
                    (
                        image,
                        [
                            float.border[3] + float.padding[3],
                            float.border[0] + float.padding[0],
                            size.0,
                            size.1,
                        ],
                    )
                } else {
                    (image, [0.0, 0.0, natural.0, natural.1])
                };
                let threshold = style.shape_image_threshold;
                (
                    inset,
                    raster(image, (rect, size.1), threshold, (inset[0], float.size.1), margin),
                )
            }
        };
        Some(Self {
            geometry,
            margin,
            origin: (reference[3], reference[0]),
            width: float.size.0,
        })
    }

    /// The part of a line from `top` to `top + height` (from the top of the margin box) that
    /// the float area takes: from where to where, from the left of the margin box. `None` if
    /// the line passes beside it freely.
    pub(crate) fn excluded(&self, top: f32, height: f32) -> Option<(f32, f32)> {
        let y1 = top - self.origin.1;
        let y2 = y1 + height;
        let (start, end) = match &self.geometry {
            Geometry::Ellipse { center, radii } => ellipse(*center, *radii, self.margin, y1, y2)?,
            Geometry::RoundedRect { rect, radii } => rounded_rect(*rect, *radii, self.margin, y1, y2)?,
            Geometry::Polygon(points) => polygon(points, self.margin, y1, y2)?,
            Geometry::Raster { top, rows } => raster_interval(*top, rows, y1, y2)?,
            Geometry::Outlines(_) => return None,
        };
        let start = (start + self.origin.0).max(0.0);
        let end = (end + self.origin.0).min(self.width);
        Some((start, end.max(start)))
    }
}

/// A point of a box of `size` on the page in the coordinates of writing mode `mode`, where
/// lines run along x: in vertical modes the y of the page, and x stacks the lines.
fn turned_point((x, y): (f32, f32), mode: WritingMode, (width, height): (f32, f32)) -> (f32, f32) {
    match mode {
        WritingMode::HorizontalTb => (x, y),
        WritingMode::VerticalRl | WritingMode::SidewaysRl => (y, width - x),
        WritingMode::VerticalLr => (y, x),
        WritingMode::SidewaysLr => (height - y, x),
    }
}

/// A rectangle (x, y, width, height) of a box of `size` on the page in the coordinates of
/// writing mode `mode`.
fn turned_rect([x, y, width, height]: [f32; 4], mode: WritingMode, size: (f32, f32)) -> [f32; 4] {
    let (x1, y1) = turned_point((x, y), mode, size);
    let (x2, y2) = turned_point((x + width, y + height), mode, size);
    [x1.min(x2), y1.min(y2), (x2 - x1).abs(), (y2 - y1).abs()]
}

/// The geometry of a shape in a box of `size` on the page, in the coordinates of writing mode
/// `mode`.
fn turned(geometry: Geometry, mode: WritingMode, size: (f32, f32)) -> Geometry {
    let point = |point| turned_point(point, mode, size);
    match geometry {
        Geometry::Ellipse { center, radii } => Geometry::Ellipse {
            center: point(center),
            radii: (radii.1, radii.0),
        },
        Geometry::RoundedRect { rect, radii } => {
            let [x, y, width, height] = rect;
            let turned = turned_rect(rect, mode, size);
            // Each corner goes where its point goes, its radii across.
            let mut corners = [(0.0, 0.0); 4];
            let points = [(x, y), (x + width, y), (x + width, y + height), (x, y + height)];
            for (corner, radius) in points.into_iter().zip(radii) {
                let (cx, cy) = point(corner);
                let right = cx > turned[0] + turned[2] / 2.0;
                let bottom = cy > turned[1] + turned[3] / 2.0;
                let index = match (right, bottom) {
                    (false, false) => 0,
                    (true, false) => 1,
                    (true, true) => 2,
                    (false, true) => 3,
                };
                corners[index] = (radius.1, radius.0);
            }
            Geometry::RoundedRect {
                rect: turned,
                radii: corners,
            }
        }
        Geometry::Polygon(points) => Geometry::Polygon(points.into_iter().map(point).collect()),
        Geometry::Outlines(outlines) => Geometry::Outlines(
            outlines
                .into_iter()
                .map(|points| points.into_iter().map(point).collect())
                .collect(),
        ),
        raster @ Geometry::Raster { .. } => raster,
    }
}

/// The alpha of an image turned as the coordinates of writing mode `mode` turn the page.
fn turned_mask(image: &AlphaMask, mode: WritingMode) -> AlphaMask {
    let (width, height) = (image.width, image.height);
    let mut alpha = Vec::with_capacity(image.alpha.len());
    // A pixel of the turned image (`column`, `row`) is the one of the image whose middle turns
    // to its middle.
    for row in 0..width {
        for column in 0..height {
            let (x, y) = match mode {
                WritingMode::HorizontalTb => (column, row),
                WritingMode::VerticalRl | WritingMode::SidewaysRl => (width - 1 - row, column),
                WritingMode::VerticalLr => (row, column),
                WritingMode::SidewaysLr => (row, height - 1 - column),
            };
            alpha.push(image.alpha.get((y * width + x) as usize).copied().unwrap_or(0));
        }
    }
    AlphaMask {
        width: height,
        height: width,
        alpha,
    }
}

/// The geometry of a basic shape in a reference box of `size`; `ClipShape::Box` is the box
/// with the corners of `border_radius`, resolved against it.
fn basic_shape(shape: &ClipShape, size: (f32, f32), border_radius: &Corners) -> Geometry {
    let (width, height) = size;
    match shape {
        // `shape-outside` names no SVG elements: the box stands in.
        ClipShape::Box | ClipShape::Url(_) => {
            rounded([0.0, 0.0, width, height], corner_radii(border_radius, size))
        }
        ClipShape::Path { data, .. } => Geometry::Outlines(outlines(data)),
        ClipShape::Inset { insets, radii } => {
            let top = insets[0].resolve(height);
            let right = insets[1].resolve(width);
            let bottom = insets[2].resolve(height);
            let left = insets[3].resolve(width);
            rounded(
                [
                    left,
                    top,
                    (width - left - right).max(0.0),
                    (height - top - bottom).max(0.0),
                ],
                corner_radii(radii, size),
            )
        }
        ClipShape::Ellipse {
            radii,
            circle,
            center,
        } => {
            let center = (center.0.resolve(width), center.1.resolve(height));
            let side = |radius: &ShapeRadius, horizontal: bool| {
                let (near, far) = if horizontal {
                    (
                        center.0.abs().min((width - center.0).abs()),
                        center.0.abs().max((width - center.0).abs()),
                    )
                } else {
                    (
                        center.1.abs().min((height - center.1).abs()),
                        center.1.abs().max((height - center.1).abs()),
                    )
                };
                match radius {
                    ShapeRadius::ClosestSide => near,
                    ShapeRadius::FarthestSide => far,
                    ShapeRadius::Length(length) => length.resolve(if horizontal { width } else { height }),
                }
            };
            let radii = if *circle {
                // A circle's sides are the nearest or farthest of all four; a percentage
                // refers to the normalized diagonal (CSS Shapes 1, §3.1).
                let radius = match &radii.0 {
                    ShapeRadius::ClosestSide => side(&radii.0, true).min(side(&radii.0, false)),
                    ShapeRadius::FarthestSide => side(&radii.0, true).max(side(&radii.0, false)),
                    ShapeRadius::Length(length) => {
                        length.resolve((width * width + height * height).sqrt() / std::f32::consts::SQRT_2)
                    }
                };
                (radius, radius)
            } else {
                (side(&radii.0, true), side(&radii.1, false))
            };
            Geometry::Ellipse {
                center,
                radii: (radii.0.max(0.0), radii.1.max(0.0)),
            }
        }
        ClipShape::Polygon { points, .. } => Geometry::Polygon(
            points
                .iter()
                .map(|(x, y)| (x.resolve(width), y.resolve(height)))
                .collect(),
        ),
    }
}

/// The radii of corners for a box of `size`.
fn corner_radii(corners: &Corners, size: (f32, f32)) -> [(f32, f32); 4] {
    let corner = |(x, y): &(LengthPercentage, LengthPercentage)| {
        (x.resolve(size.0).max(0.0), y.resolve(size.1).max(0.0))
    };
    [
        corner(&corners.top_left),
        corner(&corners.top_right),
        corner(&corners.bottom_right),
        corner(&corners.bottom_left),
    ]
}

/// A rounded rectangle whose radii are scaled down together where they do not fit.
fn rounded(rect: [f32; 4], mut radii: [(f32, f32); 4]) -> Geometry {
    let [top_left, top_right, bottom_right, bottom_left] = radii;
    let horizontal = (top_left.0 + top_right.0).max(bottom_left.0 + bottom_right.0);
    let vertical = (top_left.1 + bottom_left.1).max(top_right.1 + bottom_right.1);
    let mut factor: f32 = 1.0;
    if horizontal > rect[2] {
        factor = factor.min(rect[2] / horizontal);
    }
    if vertical > rect[3] {
        factor = factor.min(rect[3] / vertical);
    }
    for radius in &mut radii {
        *radius = (radius.0 * factor, radius.1 * factor);
    }
    Geometry::RoundedRect { rect, radii }
}

/// Whether the band from `y1` to `y2` overlaps the box of a shape from `top` to `bottom`: a band
/// that only touches it does not, an empty one at its top does (Blink's
/// `LineOverlapsBoundingBox`).
fn overlaps((top, bottom): (f32, f32), y1: f32, y2: f32) -> bool {
    (y1 < bottom && y2 > top) || (y1 >= y2 && (y1 - top).abs() < f32::EPSILON)
}

/// The x extent of an ellipse grown by `margin` in its radii over the band from `y1` to `y2`.
fn ellipse(center: (f32, f32), radii: (f32, f32), margin: f32, y1: f32, y2: f32) -> Option<(f32, f32)> {
    let (rx, ry) = (radii.0 + margin, radii.1 + margin);
    if rx <= 0.0 || ry <= 0.0 {
        return None;
    }
    if !overlaps((center.1 - ry, center.1 + ry), y1, y2) {
        return None;
    }
    let mut x = rx;
    if y1 > center.1 || y2 < center.1 {
        let dy = if y1 > center.1 {
            y1 - center.1
        } else {
            y2 - center.1
        };
        x = rx * (1.0 - (dy * dy) / (ry * ry)).max(0.0).sqrt();
    }
    Some((center.0 - x, center.0 + x))
}

/// The x extent of a rounded rectangle grown by `margin` (its radii too) over the band from
/// `y1` to `y2`.
fn rounded_rect(rect: [f32; 4], radii: [(f32, f32); 4], margin: f32, y1: f32, y2: f32) -> Option<(f32, f32)> {
    let [x, y, width, height] = rect;
    let (x, y, width, height) = (
        x - margin,
        y - margin,
        width + 2.0 * margin,
        height + 2.0 * margin,
    );
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let radii = if margin > 0.0 {
        radii.map(|(rx, ry)| (rx + margin, ry + margin))
    } else {
        radii
    };
    let (right, bottom) = (x + width, y + height);
    // The band misses the box.
    if !overlaps((y, bottom), y1, y2) {
        return None;
    }
    if radii.iter().all(|&(rx, ry)| rx <= 0.0 || ry <= 0.0) {
        return Some((x, right));
    }
    let [top_left, top_right, bottom_right, bottom_left] = radii;
    let top_corner_max_y = (y + top_left.1).max(y + top_right.1);
    let bottom_corner_min_y = (bottom - bottom_left.1).min(bottom - bottom_right.1);
    if top_corner_max_y <= bottom_corner_min_y && y1 <= top_corner_max_y && y2 >= bottom_corner_min_y {
        return Some((x, right));
    }
    let (mut start, mut end) = (right, x);
    if y1 <= y + top_left.1 && y2 >= bottom - bottom_left.1 {
        start = x;
    }
    if y1 <= y + top_right.1 && y2 >= bottom - bottom_right.1 {
        end = right;
    }
    // The intercepts of the curved edges at a height.
    let intercepts = |at: f32| -> Option<(f32, f32)> {
        if at < y || at > bottom {
            return None;
        }
        let curve = |(rx, ry): (f32, f32), dy: f32| {
            if rx <= 0.0 || ry <= 0.0 {
                return rx;
            }
            rx * (1.0 - (dy / ry).powi(2)).max(0.0).sqrt()
        };
        let min = if top_left.0 > 0.0 && top_left.1 > 0.0 && at < y + top_left.1 {
            x + top_left.0 - curve(top_left, y + top_left.1 - at)
        } else if bottom_left.0 > 0.0 && bottom_left.1 > 0.0 && at >= bottom - bottom_left.1 {
            x + bottom_left.0 - curve(bottom_left, at - (bottom - bottom_left.1))
        } else {
            x
        };
        let max = if top_right.0 > 0.0 && top_right.1 > 0.0 && at <= y + top_right.1 {
            right - top_right.0 + curve(top_right, y + top_right.1 - at)
        } else if bottom_right.0 > 0.0 && bottom_right.1 > 0.0 && at >= bottom - bottom_right.1 {
            right - bottom_right.0 + curve(bottom_right, at - (bottom - bottom_right.1))
        } else {
            right
        };
        Some((min, max))
    };
    for at in [y1, y2] {
        if let Some((min, max)) = intercepts(at) {
            start = start.min(min);
            end = end.max(max);
        }
    }
    (end >= start).then_some((start, end))
}

/// The x extent over the band from `y1` to `y2` of a polygon grown by `margin`.
#[allow(clippy::float_cmp, reason = "horizontal edges are those with equal ends")]
fn polygon(points: &[(f32, f32)], margin: f32, y1: f32, y2: f32) -> Option<(f32, f32)> {
    if points.len() < 3 {
        return None;
    }
    let (min_y, max_y) = points.iter().fold((f32::MAX, f32::MIN), |(min, max), point| {
        (min.min(point.1), max.max(point.1))
    });
    if y2 < min_y - margin || y1 > max_y + margin || y2 < y1 {
        return None;
    }
    let mut interval: Option<(f32, f32)> = None;
    let mut unite = |range: Option<(f32, f32)>| {
        if let Some((a, b)) = range {
            interval = Some(interval.map_or((a, b), |(start, end)| (start.min(a), end.max(b))));
        }
    };
    for index in 0..points.len() {
        let v1 = points[index];
        let v2 = points[(index + 1) % points.len()];
        let (edge_min, edge_max) = (v1.1.min(v2.1), v1.1.max(v2.1));
        // Edges that do not reach the band, and horizontal ones, add nothing.
        if edge_max < y1 - margin || edge_min > y2 + margin || edge_max == edge_min {
            continue;
        }
        if margin == 0.0 {
            unite(clipped_edge(v1, v2, y1, y2));
        } else {
            let normal = outward_normal(v1, v2);
            for sign in [1.0, -1.0] {
                let offset = (normal.0 * margin * sign, normal.1 * margin * sign);
                unite(clipped_edge(
                    (v1.0 + offset.0, v1.1 + offset.1),
                    (v2.0 + offset.0, v2.1 + offset.1),
                    y1,
                    y2,
                ));
            }
            unite(clipped_circle(v1, margin, y1, y2));
            unite(clipped_circle(v2, margin, y1, y2));
        }
    }
    interval
}

/// The outward normal of a polygon edge from `v1` to `v2`, of unit length.
fn outward_normal(v1: (f32, f32), v2: (f32, f32)) -> (f32, f32) {
    let delta = (v2.0 - v1.0, v2.1 - v1.1);
    let inward = if delta.0 == 0.0 {
        (if delta.1 > 0.0 { -1.0 } else { 1.0 }, 0.0)
    } else if delta.1 == 0.0 {
        (0.0, if delta.0 > 0.0 { 1.0 } else { -1.0 })
    } else {
        let length = (delta.0 * delta.0 + delta.1 * delta.1).sqrt();
        (-delta.1 / length, delta.0 / length)
    };
    (-inward.0, -inward.1)
}

/// The x range of the part of the edge from `v1` to `v2` within the band from `y1` to `y2`.
#[allow(
    clippy::float_cmp,
    reason = "a band that only touches an end of the edge misses it"
)]
fn clipped_edge(v1: (f32, f32), v2: (f32, f32), y1: f32, y2: f32) -> Option<(f32, f32)> {
    let (low, high) = if v1.1 < v2.1 { (v1, v2) } else { (v2, v1) };
    let (min_y, max_y) = (low.1, high.1);
    if y2 < min_y || y1 > max_y || (y1 == max_y && min_y <= y1) || (y2 == min_y && max_y >= y2) {
        return None;
    }
    let intercept = |y: f32| {
        if v1.0 == v2.0 || v1.1 == v2.1 {
            return v1.0.min(v2.0);
        }
        v1.0 + (y - v1.1) * (v2.0 - v1.0) / (v2.1 - v1.1)
    };
    let x1 = if min_y < y1 { intercept(y1) } else { low.0 };
    let x2 = if max_y > y2 { intercept(y2) } else { high.0 };
    Some((x1.min(x2), x1.max(x2)))
}

/// The x range of a circle within the band from `y1` to `y2`.
fn clipped_circle(center: (f32, f32), radius: f32, y1: f32, y2: f32) -> Option<(f32, f32)> {
    if y1 >= center.1 + radius || y2 <= center.1 - radius {
        return None;
    }
    if center.1 >= y1 && center.1 <= y2 {
        return Some((center.0 - radius, center.0 + radius));
    }
    let dy = if y2 < center.1 { y2 } else { y1 } - center.1;
    let x = radius * (1.0 - (dy * dy) / (radius * radius)).max(0.0).sqrt();
    Some((center.0 - x, center.0 + x))
}

/// The rows of an image's shape: the image, drawn in `rect` (x, y, width, height) of a content
/// box `height` tall and `inset` from the top of a margin box `margin_box` tall, marks where
/// its alpha is above `threshold`; grown by `margin`. Only rows of the content box and the
/// margin box count, as in Chrome.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn raster(
    image: &AlphaMask,
    (rect, height): ([f32; 4], f32),
    threshold: f32,
    (inset, margin_box): (f32, f32),
    margin: f32,
) -> Geometry {
    let [x, y, width, image_height] = rect.map(|value| value.round() as i32);
    let margin_top = -(inset.round() as i32);
    let margin_bottom = margin_top + margin_box.round() as i32;
    let first = margin_top.max(0).max(y);
    let last = (height.floor() as i32).min(y + image_height).min(margin_bottom);
    let level = (threshold * 255.0) as u8;
    let mut rows = vec![None; usize::try_from(margin_bottom - margin_top).unwrap_or(0)];
    if image.width > 0 && image.height > 0 && width > 0 && image_height > 0 {
        for row in first..last {
            // The pixel of the image under the middle of each pixel it is drawn in.
            let source_y = ((((row - y) as f32 + 0.5) * image.height as f32 / image_height as f32) as u32)
                .min(image.height - 1);
            let mut start = i32::MAX;
            let mut end = i32::MIN;
            for column in 0..width {
                let source_x =
                    (((column as f32 + 0.5) * image.width as f32 / width as f32) as u32).min(image.width - 1);
                let alpha = image.alpha[(source_y * image.width + source_x) as usize];
                if alpha > level {
                    start = start.min(x + column);
                    end = x + column + 1;
                }
            }
            if start < end {
                rows[(row - margin_top) as usize] = Some((start, end));
            }
        }
    }
    let rows = if margin > 0.0 {
        grow_rows(&rows, margin.ceil() as i32)
    } else {
        rows
    };
    Geometry::Raster {
        top: margin_top,
        rows,
    }
}

/// The outlines of path data `data` (in px from the reference box's corner), with each curve
/// cut into lines; empty where it does not parse.
fn outlines(data: &str) -> Vec<Vec<(f32, f32)>> {
    /// Lines a curve is cut into.
    const STEPS: u16 = 16;
    let mut outlines: Vec<Vec<(f32, f32)>> = Vec::new();
    let mut current: Vec<(f32, f32)> = Vec::new();
    for segment in crate::path_data::parse(data, (0.0, 0.0)).unwrap_or_default() {
        match segment {
            PathSegment::Move(x, y) => {
                if current.len() > 2 {
                    outlines.push(std::mem::take(&mut current));
                }
                current = vec![(x, y)];
            }
            PathSegment::Line(x, y) => current.push((x, y)),
            PathSegment::Cubic([x1, y1, x2, y2, x3, y3]) => {
                let (x0, y0) = current.last().copied().unwrap_or((0.0, 0.0));
                for step in 1..=STEPS {
                    // The Bernstein weights of the four points at this step.
                    let along = f32::from(step) / f32::from(STEPS);
                    let rest = 1.0 - along;
                    let weights = [
                        rest * rest * rest,
                        3.0 * rest * rest * along,
                        3.0 * rest * along * along,
                        along * along * along,
                    ];
                    let mix = |values: [f32; 4]| -> f32 {
                        values
                            .iter()
                            .zip(weights)
                            .map(|(value, weight)| value * weight)
                            .sum()
                    };
                    current.push((mix([x0, x1, x2, x3]), mix([y0, y1, y2, y3])));
                }
            }
            PathSegment::Close => {
                // What follows a close starts where the subpath started.
                let start = current.first().copied();
                if current.len() > 2 {
                    outlines.push(std::mem::take(&mut current));
                }
                current = start.into_iter().collect();
            }
        }
    }
    if current.len() > 2 {
        outlines.push(current);
    }
    outlines
}

/// The rows of pixels that outlines cover, grown by `margin`: as Chrome draws a path into
/// pixels from the reference box's corner to the right and bottom of its bounds (no further up
/// or left) and takes every pixel it touches.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn rasterized(outlines: &[Vec<(f32, f32)>], margin: f32) -> Geometry {
    let (right, bottom) = outlines
        .iter()
        .flatten()
        .fold((0.0f32, 0.0f32), |(right, bottom), point| {
            (right.max(point.0), bottom.max(point.1))
        });
    // As large as Blink draws shapes of images at most (`IsValidRasterShapeSize`).
    let (width, height) = (
        right.ceil().min(16384.0) as i32,
        bottom.ceil().min(16384.0) as i32,
    );
    let mut rows = vec![None; usize::try_from(height).unwrap_or(0)];
    for (row, slot) in rows.iter_mut().enumerate() {
        let y = row as f32;
        let mut extent: Option<(f32, f32)> = None;
        for points in outlines {
            if let Some((start, end)) = polygon(points, 0.0, y, y + 1.0) {
                extent = Some(extent.map_or((start, end), |(a, b)| (a.min(start), b.max(end))));
            }
        }
        if let Some((start, end)) = extent {
            let (start, end) = (start.floor().max(0.0) as i32, (end.ceil() as i32).min(width));
            if start < end {
                *slot = Some((start, end));
            }
        }
    }
    let rows = if margin > 0.0 {
        grow_rows(&rows, margin.ceil() as i32)
    } else {
        rows
    };
    Geometry::Raster { top: 0, rows }
}

/// Rows of a shape grown by a disc of `radius` about each: rows within the radius of one reach
/// as far as the disc does there.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn grow_rows(rows: &[Option<(i32, i32)>], radius: i32) -> Vec<Option<(i32, i32)>> {
    let reach: Vec<i32> = (0..=radius)
        .map(|dy| f64::from(radius * radius - dy * dy).sqrt() as i32)
        .collect();
    let mut grown = rows.to_vec();
    let count = rows.len() as i32;
    for (index, row) in rows.iter().enumerate() {
        let Some((start, end)) = *row else {
            continue;
        };
        let index = index as i32;
        for other in (index - radius).max(0)..(index + radius + 1).min(count) {
            let dx = reach[(other - index).unsigned_abs() as usize];
            let slot = &mut grown[other as usize];
            *slot = Some(slot.map_or((start - dx, end + dx), |(a, b)| {
                (a.min(start - dx), b.max(end + dx))
            }));
        }
    }
    grown
}

/// The x extent of the rows of an image's shape over the band from `y1` to `y2`.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
fn raster_interval(top: i32, rows: &[Option<(i32, i32)>], y1: f32, y2: f32) -> Option<(f32, f32)> {
    let (y1, y2) = (y1 as i32, y2 as i32);
    let bottom = top + i32::try_from(rows.len()).unwrap_or(0);
    if y2 < top || y1 >= bottom {
        return None;
    }
    let (y1, y2) = (y1.max(top), y2.min(bottom));
    let mut interval: Option<(i32, i32)> = None;
    for y in y1..y2.max(y1 + 1).min(bottom) {
        if let Some((start, end)) = rows[(y - top) as usize] {
            interval = Some(interval.map_or((start, end), |(a, b)| (a.min(start), b.max(end))));
        }
    }
    interval.map(|(start, end)| (start as f32, end as f32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_the_extent_of_an_ellipse_over_the_band() {
        let shape = |y1, y2| ellipse((50.0, 50.0), (50.0, 50.0), 0.0, y1, y2);
        assert_eq!(shape(40.0, 60.0), Some((0.0, 100.0)), "across the middle");
        let (start, end) = shape(0.0, 10.0).expect("top band");
        assert!(
            (start - 20.0).abs() < 0.01 && (end - 80.0).abs() < 0.01,
            "{start} {end}"
        );
        assert_eq!(shape(100.0, 110.0), None, "below");
        let grown = ellipse((50.0, 50.0), (50.0, 50.0), 10.0, 40.0, 60.0);
        assert_eq!(grown, Some((-10.0, 110.0)));
    }

    #[test]
    fn follows_polygon_edges_and_their_margin() {
        let triangle = [(0.0, 0.0), (100.0, 100.0), (0.0, 100.0)];
        assert_eq!(polygon(&triangle, 0.0, 10.0, 20.0), Some((0.0, 20.0)));
        let (_, end) = polygon(&triangle, 10.0, 10.0, 20.0).expect("grown");
        assert!(end > 30.0, "{end}");
    }

    #[test]
    fn rounds_corners_of_boxes() {
        let rect = [0.0, 0.0, 100.0, 100.0];
        let radii = [(50.0, 50.0); 4];
        assert_eq!(rounded_rect(rect, radii, 0.0, 45.0, 55.0), Some((0.0, 100.0)));
        let (start, end) = rounded_rect(rect, radii, 0.0, 0.0, 10.0).expect("top");
        assert!(
            (start - 20.0).abs() < 0.01 && (end - 80.0).abs() < 0.01,
            "{start} {end}"
        );
        assert_eq!(
            rounded_rect(rect, [(0.0, 0.0); 4], 5.0, 0.0, 10.0),
            Some((-5.0, 105.0))
        );
    }

    #[test]
    fn grows_image_rows_by_a_disc() {
        let rows = [None, Some((10, 20)), None, None];
        let grown = grow_rows(&rows, 2);
        assert_eq!(grown[1], Some((8, 22)));
        assert_eq!(grown[3], Some((10, 20)), "two rows away, the disc ends");
        assert_eq!(raster_interval(0, &grown, 0.0, 4.0), Some((8.0, 22.0)));
    }

    #[test]
    fn turns_shapes_into_vertical_writing_modes() {
        // A box 100 wide and 140 high on the page is 140 long and 100 across in vertical lines,
        // its right side their start in `vertical-rl`.
        let size = (100.0, 140.0);
        let triangle = Geometry::Polygon(vec![(0.0, 0.0), (100.0, 0.0), (0.0, 140.0)]);
        assert_eq!(
            turned(triangle, WritingMode::VerticalRl, size),
            Geometry::Polygon(vec![(0.0, 100.0), (0.0, 0.0), (140.0, 100.0)])
        );
        let rect = Geometry::RoundedRect {
            rect: [0.0, 0.0, 80.0, 80.0],
            radii: [(10.0, 20.0), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0)],
        };
        let Geometry::RoundedRect { rect, radii } = turned(rect, WritingMode::VerticalRl, size) else {
            panic!("a rounded rectangle");
        };
        assert_eq!(rect, [0.0, 20.0, 80.0, 80.0]);
        // The top left corner on the page is the bottom left one of the lines.
        assert_eq!(radii[3], (20.0, 10.0));
        assert_eq!(
            turned_point((10.0, 20.0), WritingMode::VerticalLr, size),
            (20.0, 10.0)
        );
        assert_eq!(
            turned_point((10.0, 20.0), WritingMode::SidewaysLr, size),
            (120.0, 10.0)
        );
        let mask = AlphaMask {
            width: 2,
            height: 1,
            alpha: vec![255, 0],
        };
        assert_eq!(turned_mask(&mask, WritingMode::VerticalRl).alpha, vec![0, 255]);
    }

    #[test]
    fn leaves_lines_that_touch_a_box_free() {
        let rect = [0.0, 40.0, 100.0, 40.0];
        assert_eq!(rounded_rect(rect, [(0.0, 0.0); 4], 0.0, 20.0, 40.0), None);
        assert_eq!(rounded_rect(rect, [(0.0, 0.0); 4], 0.0, 80.0, 100.0), None);
        assert_eq!(
            rounded_rect(rect, [(0.0, 0.0); 4], 0.0, 30.0, 50.0),
            Some((0.0, 100.0))
        );
        assert_eq!(ellipse((50.0, 60.0), (50.0, 20.0), 0.0, 20.0, 40.0), None);
    }
}

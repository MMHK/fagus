//! The alpha channel of a gradient, for `shape-outside: linear-gradient(…)` and the other
//! gradients (CSS Shapes 1, §6.1): the float area is where the gradient, drawn in the float's
//! content box, is more opaque than `shape-image-threshold`.

use fagus_style::values::{
    Gradient, GradientKind, GradientStop, LinearDirection, RadialShape, RadialSize, StopPosition,
};

use crate::shape::AlphaMask;

/// Positions beyond any float (`calc(1e39%)`) are as far as can be.
const MAX_STOP: f32 = 1e9;

/// A resolved stop: a color stop's position and alpha, or a hint's position.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Stop {
    Color(f32, f32),
    Hint(f32),
}

/// The alpha of `gradient` drawn into a box of `width` × `height` pixels, sampled in the middle
/// of each pixel; `current` is the alpha of `currentColor`. `None` for a box of no pixels or of
/// too many.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "the size is checked to be of at most 16M pixels"
)]
pub(crate) fn alpha(gradient: &Gradient, width: f32, height: f32, current: f32) -> Option<AlphaMask> {
    let (columns, rows) = (width.round(), height.round());
    if !(columns >= 1.0 && rows >= 1.0 && columns * rows <= 16.0 * 1024.0 * 1024.0) {
        return None;
    }
    let (width, height) = (columns, rows);
    // Where each point lies along the gradient, with the length its stops resolve against.
    let (length, place): (f32, Box<dyn Fn(f32, f32) -> f32>) = match &gradient.kind {
        GradientKind::Linear(direction) => {
            let (dx, dy) = match *direction {
                LinearDirection::Angle(degrees) => {
                    let (sin, cos) = libm::sincosf(degrees.to_radians());
                    (sin, -cos)
                }
                LinearDirection::Corner { right, bottom } => {
                    let norm = libm::hypotf(width, height).max(f32::EPSILON);
                    let sx = if right { 1.0 } else { -1.0 };
                    let sy = if bottom { 1.0 } else { -1.0 };
                    (sx * height / norm, sy * width / norm)
                }
            };
            let length = (width * dx).abs() + (height * dy).abs();
            let center = (width / 2.0, height / 2.0);
            (
                length,
                Box::new(move |x, y| (x - center.0) * dx + (y - center.1) * dy + length / 2.0),
            )
        }
        GradientKind::Radial { shape, position } => {
            let center = (position.0.resolve(width), position.1.resolve(height));
            let (rx, ry) = radii(shape, center, width, height);
            if rx <= 0.0 || ry <= 0.0 {
                return Some(AlphaMask {
                    width: columns as u32,
                    height: rows as u32,
                    alpha: vec![0; (columns * rows) as usize],
                });
            }
            (
                rx,
                Box::new(move |x, y| libm::hypotf((x - center.0) / rx, (y - center.1) / ry) * rx),
            )
        }
        GradientKind::Conic { from, position } => {
            let center = (position.0.resolve(width), position.1.resolve(height));
            let from = *from;
            (
                360.0,
                Box::new(move |x, y| {
                    let angle = libm::atan2f(x - center.0, center.1 - y).to_degrees();
                    (angle - from).rem_euclid(360.0)
                }),
            )
        }
    };
    let stops = resolve_stops(&gradient.stops, length, current);
    let colors: Vec<f32> = stops
        .iter()
        .filter_map(|stop| match stop {
            Stop::Color(at, _) => Some(*at),
            Stop::Hint(_) => None,
        })
        .collect();
    let (first, last) = (*colors.first()?, *colors.last()?);
    let mut alpha = Vec::with_capacity((columns * rows) as usize);
    for row in 0..rows as u32 {
        for column in 0..columns as u32 {
            let mut at = place(column as f32 + 0.5, row as f32 + 0.5);
            if gradient.repeating && last - first > f32::EPSILON {
                at = first + (at - first).rem_euclid(last - first);
            }
            alpha.push((alpha_at(&stops, at).clamp(0.0, 1.0) * 255.0).round() as u8);
        }
    }
    Some(AlphaMask {
        width: columns as u32,
        height: rows as u32,
        alpha,
    })
}

/// The alpha of resolved stops at `at`: interpolated linearly between color stops (alpha
/// interpolates linearly with premultiplied colors in every color space), along the curve of a
/// hint between them (CSS Images 4, §3.5.3).
fn alpha_at(stops: &[Stop], at: f32) -> f32 {
    let mut before: Option<(f32, f32)> = None;
    let mut hint = None;
    for stop in stops {
        match *stop {
            Stop::Hint(position) => hint = Some(position),
            Stop::Color(position, alpha) => {
                if position > at || (before.is_none() && position >= at) {
                    let Some((start, from)) = before else {
                        return alpha;
                    };
                    let span = position - start;
                    if span <= 0.0 {
                        return alpha;
                    }
                    let mut weight = (at - start) / span;
                    if let Some(hint) = hint {
                        let middle = ((hint - start) / span).clamp(0.0, 1.0);
                        weight = if middle <= 0.0 {
                            1.0
                        } else if middle >= 1.0 {
                            0.0
                        } else {
                            libm::powf(weight, libm::logf(0.5f32) / libm::logf(middle))
                        };
                    }
                    return from + (alpha - from) * weight;
                }
                before = Some((position, alpha));
                hint = None;
            }
        }
    }
    before.map_or(0.0, |(_, alpha)| alpha)
}

/// Resolves the positions of a gradient's stops on a gradient line of `length` (CSS Images 3,
/// §3.4.3): the first and last color stops default to its ends, positions never go back and
/// stops without one spread evenly between their neighbors.
#[allow(clippy::cast_precision_loss, reason = "stop counts are small")]
fn resolve_stops(stops: &[GradientStop], length: f32, current: f32) -> Vec<Stop> {
    let position = |position: &StopPosition| {
        let at = match position {
            StopPosition::Length(length_percentage) => length_percentage.resolve(length),
            StopPosition::Angle(degrees) => *degrees,
        };
        if at.is_nan() {
            0.0
        } else {
            at.clamp(-MAX_STOP, MAX_STOP)
        }
    };
    let count = stops
        .iter()
        .filter(|stop| matches!(stop, GradientStop::Color { .. }))
        .count();
    let mut resolved: Vec<(Option<f32>, Option<f32>)> = Vec::with_capacity(stops.len());
    // The alpha a stop that misses components goes on with beyond its position.
    let mut beyond: Vec<Option<f32>> = Vec::with_capacity(stops.len());
    let mut color_index = 0;
    for stop in stops {
        match stop {
            GradientStop::Color {
                color,
                position: stop_position,
                after,
                ..
            } => {
                let mut at = stop_position.as_ref().map(position);
                if at.is_none() && color_index == 0 {
                    at = Some(0.0);
                }
                if at.is_none() && color_index + 1 == count {
                    at = Some(length);
                }
                resolved.push((at, Some(color.map_or(current, |color| color.a))));
                beyond.push(after.map(|after| after.a));
                color_index += 1;
            }
            GradientStop::Hint(hint) => {
                resolved.push((Some(position(hint)), None));
                beyond.push(None);
            }
        }
    }
    let mut highest = f32::NEG_INFINITY;
    for entry in &mut resolved {
        if let Some(at) = &mut entry.0 {
            *at = at.max(highest);
            highest = *at;
        }
    }
    let mut index = 0;
    while index < resolved.len() {
        if resolved[index].0.is_some() {
            index += 1;
            continue;
        }
        let start = index;
        while index < resolved.len() && resolved[index].0.is_none() {
            index += 1;
        }
        let before = start
            .checked_sub(1)
            .and_then(|previous| resolved[previous].0)
            .unwrap_or(0.0);
        let after = resolved.get(index).and_then(|entry| entry.0).unwrap_or(before);
        let steps = (index - start + 1) as f32;
        for (offset, entry) in resolved[start..index].iter_mut().enumerate() {
            entry.0 = Some(before + (after - before) * (offset + 1) as f32 / steps);
        }
    }
    resolved
        .into_iter()
        .zip(beyond)
        .flat_map(|((at, alpha), beyond)| {
            let at = at.unwrap_or(0.0);
            let stop = alpha.map_or(Stop::Hint(at), |alpha| Stop::Color(at, alpha));
            std::iter::once(stop).chain(beyond.map(|alpha| Stop::Color(at, alpha)))
        })
        .collect()
}

/// The radii of a radial gradient's ending shape (CSS Images 3, §3.2.2).
fn radii(shape: &RadialShape, center: (f32, f32), width: f32, height: f32) -> (f32, f32) {
    let (cx, cy) = center;
    let horizontal = [cx.abs(), (width - cx).abs()];
    let vertical = [cy.abs(), (height - cy).abs()];
    let closest = (horizontal[0].min(horizontal[1]), vertical[0].min(vertical[1]));
    let farthest = (horizontal[0].max(horizontal[1]), vertical[0].max(vertical[1]));
    let corners = [(0.0, 0.0), (width, 0.0), (0.0, height), (width, height)]
        .map(|(x, y): (f32, f32)| libm::hypotf(x - cx, y - cy));
    let closest_corner = corners.iter().copied().fold(f32::INFINITY, f32::min);
    let farthest_corner = corners.iter().copied().fold(0.0, f32::max);
    match shape {
        RadialShape::Circle(size) => {
            let radius = match size {
                RadialSize::ClosestSide => closest.0.min(closest.1),
                RadialSize::FarthestSide => farthest.0.max(farthest.1),
                RadialSize::ClosestCorner => closest_corner,
                RadialSize::FarthestCorner => farthest_corner,
                RadialSize::Radii(radius, _) => radius.resolve(width),
            };
            (radius, radius)
        }
        RadialShape::Ellipse(size) => match size {
            RadialSize::ClosestSide => closest,
            RadialSize::FarthestSide => farthest,
            RadialSize::ClosestCorner => (
                closest.0 * std::f32::consts::SQRT_2,
                closest.1 * std::f32::consts::SQRT_2,
            ),
            RadialSize::FarthestCorner => (
                farthest.0 * std::f32::consts::SQRT_2,
                farthest.1 * std::f32::consts::SQRT_2,
            ),
            RadialSize::Radii(x, y) => (x.resolve(width), y.resolve(height)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fagus_style::values::{LengthPercentage, Rgba};

    fn stop(alpha: f32, at: Option<f32>) -> GradientStop {
        GradientStop::Color {
            color: Some(Rgba {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: alpha,
            }),
            legacy: true,
            position: at.map(|at| StopPosition::Length(LengthPercentage::Percent(at))),
            after: None,
        }
    }

    #[test]
    fn draws_the_alpha_of_a_linear_gradient_with_a_hard_stop() {
        let gradient = Gradient {
            kind: GradientKind::Linear(LinearDirection::Angle(90.0)),
            repeating: false,
            stops: vec![stop(1.0, None), stop(0.5, Some(0.5)), stop(0.0, Some(0.5))],
            interpolation: None,
        };
        let mask = alpha(&gradient, 4.0, 2.0, 1.0).expect("mask");
        assert_eq!((mask.width, mask.height), (4, 2));
        // 1 at the start to 0.5 halfway, transparent after it.
        assert_eq!(mask.alpha[..4], [223, 159, 0, 0]);
        assert_eq!(mask.alpha[4..], mask.alpha[..4]);
    }

    #[test]
    fn follows_hints_and_repeats() {
        let stops = [Stop::Color(0.0, 0.0), Stop::Hint(25.0), Stop::Color(100.0, 1.0)];
        assert!(
            (alpha_at(&stops, 25.0) - 0.5).abs() < 0.001,
            "half way at the hint"
        );
        let gradient = Gradient {
            kind: GradientKind::Linear(LinearDirection::Angle(180.0)),
            repeating: true,
            stops: vec![
                stop(1.0, Some(0.0)),
                stop(1.0, Some(0.25)),
                stop(0.0, Some(0.25)),
                stop(0.0, Some(0.5)),
            ],
            interpolation: None,
        };
        let mask = alpha(&gradient, 1.0, 8.0, 1.0).expect("mask");
        assert_eq!(mask.alpha, [255, 255, 0, 0, 255, 255, 0, 0]);
    }
}

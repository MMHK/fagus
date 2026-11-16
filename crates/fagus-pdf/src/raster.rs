//! Images Fagus computes itself: blurred shadows, and conic gradients where PDF/A rules out the
//! PostScript function they need as vector shadings.

use fagus_paint::RectF;
use fagus_style::values::Rgba;
use krilla::image::{BitsPerComponent, CustomImage, Image, ImageColorspace};

use crate::channel;

/// The largest side of a computed image in pixels.
pub(crate) const MAX_PIXELS: f32 = 1536.0;

/// An RGB image with an alpha channel.
#[derive(Clone, Hash)]
pub(crate) struct RgbaImage {
    pub(crate) color: Vec<u8>,
    /// Empty for an opaque image.
    pub(crate) alpha: Vec<u8>,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl CustomImage for RgbaImage {
    fn color_channel(&self) -> &[u8] {
        &self.color
    }

    fn alpha_channel(&self) -> Option<&[u8]> {
        (!self.alpha.is_empty()).then_some(&self.alpha)
    }

    fn bits_per_component(&self) -> BitsPerComponent {
        BitsPerComponent::Eight
    }

    fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn icc_profile(&self) -> Option<&[u8]> {
        None
    }

    fn color_space(&self) -> ImageColorspace {
        ImageColorspace::Rgb
    }
}

/// A conic gradient over a tile of `size`, around `center`, starting at `start_angle` (degrees
/// clockwise from the top), as an image of two pixels per CSS pixel (fewer for large tiles).
/// Each pixel takes the mean color of the angles it spans, so that hard stops and the turn's
/// seam are antialiased as Chrome draws them, not stepped.
pub(crate) fn conic(
    size: RectF,
    center: (f32, f32),
    start_angle: f32,
    stops: &[(f32, Rgba)],
) -> Option<Image> {
    Image::from_custom(conic_pixels(size, center, start_angle, stops, 2.0), false).ok()
}

/// The pixels of a conic gradient (see [`conic`]) at `scale` pixels per CSS pixel, fewer for
/// large tiles.
pub(crate) fn conic_pixels(
    size: RectF,
    center: (f32, f32),
    start_angle: f32,
    stops: &[(f32, Rgba)],
    scale: f32,
) -> RgbaImage {
    let scale = scale
        .min(MAX_PIXELS / size.width.max(1.0))
        .min(MAX_PIXELS / size.height.max(1.0));
    let width = (size.width * scale).ceil().max(1.0) as usize;
    let height = (size.height * scale).ceil().max(1.0) as usize;
    let mut color = Vec::with_capacity(width * height * 3);
    let mut alpha = Vec::with_capacity(width * height);
    let turn = Turn::new(stops);
    for row in 0..height {
        let y = (row as f32 + 0.5) / scale - center.1;
        for column in 0..width {
            let x = (column as f32 + 0.5) / scale - center.0;
            let angle = (x.atan2(-y).to_degrees() - start_angle) / 360.0;
            // The part of a turn a pixel spans at its distance from the center.
            let span = (1.0 / (scale * x.hypot(y) * std::f32::consts::TAU)).min(1.0);
            let value = turn.mean(angle, span);
            color.extend([channel(value.r), channel(value.g), channel(value.b)]);
            alpha.push(channel(value.a));
        }
    }
    RgbaImage {
        color,
        alpha,
        width: width as u32,
        height: height as u32,
    }
}

/// A piece of a turn where the color changes linearly.
#[derive(Clone, Copy)]
struct Piece {
    /// Where it starts and ends, in parts of a turn (0 to 1).
    from: f32,
    to: f32,
    /// The colors there, channel by channel (red, green, blue, alpha).
    start: [f32; 4],
    end: [f32; 4],
    /// The integral of the color from 0 to `from`.
    before: [f32; 4],
}

impl Piece {
    /// The color at `at`, in the piece.
    fn color(&self, at: f32) -> [f32; 4] {
        let fraction = if self.to > self.from {
            ((at - self.from) / (self.to - self.from)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        std::array::from_fn(|i| self.start[i] + (self.end[i] - self.start[i]) * fraction)
    }
}

/// The colors of sorted gradient stops around a turn, as pieces where the color changes
/// linearly, with the integrals of the color up to each piece: the mean color over a span of
/// angles is the difference of two integrals over its length.
struct Turn {
    pieces: Vec<Piece>,
    /// The integral over the whole turn.
    total: [f32; 4],
}

impl Turn {
    fn new(stops: &[(f32, Rgba)]) -> Self {
        let channels = |color: Rgba| [color.r, color.g, color.b, color.a];
        // The stops' pieces, with the first and last colors before and after them.
        let mut linear = Vec::new();
        if let (Some(&(first, first_color)), Some(&(last, last_color))) = (stops.first(), stops.last()) {
            let (first_color, last_color) = (channels(first_color), channels(last_color));
            linear.push((f32::NEG_INFINITY, first, first_color, first_color));
            for pair in stops.windows(2) {
                let [(from, start), (to, end)] = [pair[0], pair[1]];
                if to > from {
                    linear.push((from, to, channels(start), channels(end)));
                }
            }
            linear.push((last, f32::INFINITY, last_color, last_color));
        }
        let mut pieces = Vec::new();
        let mut sum = [0.0; 4];
        for (from, to, start, end) in linear {
            let (cut_from, cut_to) = (from.max(0.0), to.min(1.0));
            if cut_to <= cut_from {
                continue;
            }
            // The piece cut to the turn.
            let whole = Piece {
                from,
                to,
                start,
                end,
                before: sum,
            };
            let (start, end) = if from.is_finite() && to.is_finite() {
                (whole.color(cut_from), whole.color(cut_to))
            } else {
                (start, end)
            };
            pieces.push(Piece {
                from: cut_from,
                to: cut_to,
                start,
                end,
                before: sum,
            });
            for i in 0..4 {
                sum[i] += (cut_to - cut_from) * (start[i] + end[i]) / 2.0;
            }
        }
        Self { pieces, total: sum }
    }

    /// The piece at `at`, a part of a turn.
    fn piece(&self, at: f32) -> Option<&Piece> {
        let index = self
            .pieces
            .partition_point(|piece| piece.from <= at)
            .saturating_sub(1);
        self.pieces.get(index)
    }

    /// The integral of the color from 0 to `at`, for any number of turns.
    fn integral(&self, at: f32) -> [f32; 4] {
        let turns = at.floor();
        let rest = at - turns;
        let mut value: [f32; 4] = std::array::from_fn(|i| self.total[i] * turns);
        if let Some(piece) = self.piece(rest) {
            let length = (rest.min(piece.to) - piece.from).max(0.0);
            let here = piece.color(rest);
            for i in 0..4 {
                value[i] += piece.before[i] + length * (piece.start[i] + here[i]) / 2.0;
            }
        }
        value
    }

    /// The mean color over `span` (a part of a turn) around `at`.
    fn mean(&self, at: f32, span: f32) -> Rgba {
        let at = at.rem_euclid(1.0);
        if span < 1e-4 {
            return self.color(at);
        }
        let (low, high) = (self.integral(at - span / 2.0), self.integral(at + span / 2.0));
        let [r, g, b, a] = std::array::from_fn(|i| ((high[i] - low[i]) / span).clamp(0.0, 1.0));
        Rgba { r, g, b, a }
    }

    /// The color at `at`, a part of a turn.
    fn color(&self, at: f32) -> Rgba {
        let Some(piece) = self.piece(at) else {
            return Rgba::TRANSPARENT;
        };
        let [r, g, b, a] = piece.color(at);
        Rgba { r, g, b, a }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_stops() {
        let red = Rgba {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        let blue = Rgba {
            r: 0.0,
            g: 0.0,
            b: 1.0,
            a: 1.0,
        };
        let stops = [(0.25, red), (0.75, blue)];
        let turn = Turn::new(&stops);
        assert_eq!(turn.color(0.0), red);
        assert_eq!(turn.color(1.0), blue);
        let middle = turn.color(0.5);
        assert!((middle.r - 0.5).abs() < 1e-6 && (middle.b - 0.5).abs() < 1e-6);
        // A hard stop.
        let hard = Turn::new(&[(0.5, red), (0.5, blue)]);
        assert_eq!(hard.color(0.49), red);
        assert_eq!(hard.color(0.51), blue);
    }

    #[test]
    fn antialiases_hard_stops_and_the_seam() {
        let red = Rgba {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        let blue = Rgba {
            r: 0.0,
            g: 0.0,
            b: 1.0,
            a: 1.0,
        };
        let close =
            |color: Rgba, (r, b): (f32, f32)| (color.r - r).abs() < 1e-4 && (color.b - b).abs() < 1e-4;
        let hard = Turn::new(&[(0.5, red), (0.5, blue)]);
        // A pixel across the hard stop is half red, half blue; one away from it all red.
        assert!(close(hard.mean(0.5, 0.1), (0.5, 0.5)));
        assert!(close(hard.mean(0.25, 0.1), (1.0, 0.0)));
        // A quarter of it past the stop.
        assert!(close(hard.mean(0.475, 0.1), (0.75, 0.25)));
        // Across the seam at the top, where the turn starts red after ending blue.
        assert!(close(hard.mean(0.0, 0.1), (0.5, 0.5)));
        assert!(close(hard.mean(0.995, 0.02), (0.25, 0.75)));
        // Around the center a pixel spans the whole turn: the mean of all colors.
        assert!(close(hard.mean(0.3, 1.0), (0.5, 0.5)));
        // A linear stretch keeps its color.
        let linear = Turn::new(&[(0.0, red), (1.0, blue)]);
        assert!(close(linear.mean(0.5, 0.2), (0.5, 0.5)));
    }
}

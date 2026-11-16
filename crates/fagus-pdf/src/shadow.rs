//! Box shadows in PDF. Sharp shadows are vector shapes; PDF has no blur, so blurred shadows are
//! images: the shape's coverage, blurred with a Gaussian, as the alpha of a single color.

use fagus_paint::{BoxShadowPaint, Radii, RectF};
use krilla::geom::{PathBuilder, Size, Transform};
use krilla::image::Image;
use krilla::paint::FillRule;
use krilla::surface::Surface;
use rustc_hash::FxHashMap;
use std::fmt::Write;

use crate::raster::{MAX_PIXELS, RgbaImage};
use crate::{append_rounded_rect, channel, fill, rect_path};

/// Draws a shadow; `interpolate` lets viewers smooth the image of a blurred one (PDF/A does not
/// allow it). The images of blurred shadows are kept in `cache`, by the shadow.
pub(crate) fn draw(
    surface: &mut Surface<'_>,
    shadow: &BoxShadowPaint,
    interpolate: bool,
    cache: &mut FxHashMap<String, Option<Image>>,
) {
    let sigma = shadow.blur / 2.0;
    // Everything the blurred shadow can reach; for an inset one, what casts it around the hole
    // (see [`casting_frame`]) and as far around that as the blur reaches.
    let reach = sigma * 3.0;
    let bounds = if shadow.inset {
        grow(casting_frame(shadow, sigma), reach)
    } else {
        grow(shadow.shape, reach)
    };

    // Outer shadows show only outside the box, inset shadows only inside it.
    let mut clip = PathBuilder::new();
    let area = |clip: &mut PathBuilder| match &shadow.outlines {
        Some(outlines) => crate::append_segments(clip, &outlines.area),
        None => append_shape(clip, shadow.area, shadow.area_radii.as_ref()),
    };
    if shadow.inset {
        area(&mut clip);
    } else {
        append_shape(&mut clip, grow(bounds, 1.0), None);
        area(&mut clip);
    }
    let Some(clip) = clip.finish() else { return };
    surface.push_clip_path(&clip, &FillRule::EvenOdd);

    if sigma < 0.01 {
        // A sharp shadow: the shape, or for an inset shadow everything around the hole.
        let mut path = PathBuilder::new();
        if shadow.inset {
            append_shape(&mut path, grow(shadow.area, 1.0), None);
        }
        append_shape(&mut path, shadow.shape, shadow.shape_radii.as_ref());
        if let Some((hole, radii)) = &shadow.hole {
            append_shape(&mut path, *hole, radii.as_ref());
        }
        if let Some(path) = path.finish() {
            surface.set_fill(Some(krilla::paint::Fill {
                rule: FillRule::EvenOdd,
                ..fill(shadow.color)
            }));
            surface.set_stroke(None);
            surface.draw_path(&path);
        }
    } else if let Some(image) = cache
        .entry(image_key(shadow, bounds))
        .or_insert_with(|| blurred(shadow, bounds, sigma, interpolate))
        .clone()
    {
        surface.push_transform(&Transform::from_translate(bounds.x, bounds.y));
        if let Some(size) = Size::from_wh(bounds.width, bounds.height) {
            surface.draw_image(image, size);
        }
        surface.pop();
    } else if let Some(path) = rect_path(shadow.shape) {
        // Too large to rasterize: the unblurred shape.
        surface.set_fill(Some(fill(shadow.color)));
        surface.draw_path(&path);
    }
    surface.pop();
}

/// What the image of a blurred shadow over `bounds` is made from, written out: its shape and
/// everything else that the image shows, in place relative to `bounds`, to a thousandth of a
/// pixel. The shadow of a box broken across pages, drawn whole on each, has one image for all of
/// them, as one on every page (of a fixed box) has: it is blurred once.
fn image_key(shadow: &BoxShadowPaint, bounds: RectF) -> String {
    let mut key = String::new();
    let mut put = |values: &[f32]| {
        for value in values {
            let _ = write!(key, " {}", (f64::from(*value) * 1024.0).round());
        }
    };
    let relative = |rect: RectF| [rect.x - bounds.x, rect.y - bounds.y, rect.width, rect.height];
    let radii = |radii: Option<&Radii>| {
        radii.map_or([-1.0; 8], |[a, b, c, d]| [a.0, a.1, b.0, b.1, c.0, c.1, d.0, d.1])
    };
    let color = shadow.color;
    put(&[
        color.r,
        color.g,
        color.b,
        color.a,
        shadow.blur,
        f32::from(u8::from(shadow.inset)),
    ]);
    put(&[bounds.width, bounds.height]);
    put(&relative(shadow.shape));
    put(&radii(shadow.shape_radii.as_ref()));
    if let Some((hole, hole_radii)) = &shadow.hole {
        put(&relative(*hole));
        put(&radii(hole_radii.as_ref()));
    }
    if let Some(outlines) = &shadow.outlines {
        for segment in &outlines.shape {
            match *segment {
                fagus_layout::PathSegment::Move(x, y) => put(&[-1.0, x - bounds.x, y - bounds.y]),
                fagus_layout::PathSegment::Line(x, y) => put(&[-2.0, x - bounds.x, y - bounds.y]),
                fagus_layout::PathSegment::Cubic([x1, y1, x2, y2, x, y]) => put(&[
                    -3.0,
                    x1 - bounds.x,
                    y1 - bounds.y,
                    x2 - bounds.x,
                    y2 - bounds.y,
                    x - bounds.x,
                    y - bounds.y,
                ]),
                fagus_layout::PathSegment::Close => put(&[-4.0]),
            }
        }
    }
    key
}

/// The rectangle that casts an inset shadow around its hole (its shape): the area grown by the
/// blur's standard deviation (and by a negative spread), and that moved against the shadow's
/// offset, as Chrome prints it (measured: Blink's `AreaCastingShadowInHole` grows it by the
/// blur radius on screen).
fn casting_frame(shadow: &BoxShadowPaint, sigma: f32) -> RectF {
    let area = shadow.area;
    let spread = (area.width - shadow.shape.width) / 2.0;
    let offset = (
        shadow.shape.x + shadow.shape.width / 2.0 - (area.x + area.width / 2.0),
        shadow.shape.y + shadow.shape.height / 2.0 - (area.y + area.height / 2.0),
    );
    let frame = grow(area, sigma + (-spread).max(0.0));
    let moved = RectF {
        x: frame.x - offset.0,
        y: frame.y - offset.1,
        ..frame
    };
    let (left, top) = (frame.x.min(moved.x), frame.y.min(moved.y));
    let right = (frame.x + frame.width).max(moved.x + moved.width);
    let bottom = (frame.y + frame.height).max(moved.y + moved.height);
    RectF {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    }
}

fn grow(rect: RectF, distance: f32) -> RectF {
    RectF {
        x: rect.x - distance,
        y: rect.y - distance,
        width: rect.width + 2.0 * distance,
        height: rect.height + 2.0 * distance,
    }
}

fn append_shape(builder: &mut PathBuilder, rect: RectF, radii: Option<&Radii>) {
    match radii {
        Some(radii) => append_rounded_rect(builder, rect, radii),
        None => {
            if let Some(rect) =
                krilla::geom::Rect::from_xywh(rect.x, rect.y, rect.width.max(0.0), rect.height.max(0.0))
            {
                builder.push_rect(rect);
            }
        }
    }
}

/// Draws a blurred text shadow, made by [`text_shadow`] or taken from `cache`, where it is kept
/// by `key` (the shadow written out): a shadow on every page, as of a fixed box, is blurred once.
pub(crate) fn draw_text(
    surface: &mut Surface<'_>,
    cache: &mut FxHashMap<String, Option<(Image, RectF)>>,
    key: String,
    shadow: impl FnOnce() -> Option<(Image, RectF)>,
) {
    let Some((image, bounds)) = cache.entry(key).or_insert_with(shadow).clone() else {
        return;
    };
    surface.push_transform(&Transform::from_translate(bounds.x, bounds.y));
    if let Some(size) = Size::from_wh(bounds.width, bounds.height) {
        surface.draw_image(image, size);
    }
    surface.pop();
}

/// A blurred shadow of glyphs set from `origin` (the start of their baseline) and of their
/// decoration lines: their coverage, blurred, as the alpha of an image in one color, and where
/// the image goes.
#[allow(clippy::too_many_arguments, reason = "the fields of a text shadow")]
pub(crate) fn text_shadow(
    (face, variations): (&fagus_text::Face, &fagus_text::Variations),
    origin: (f32, f32),
    size: f32,
    (glyphs, lines): (&[fagus_layout::Glyph], &[RectF]),
    color: fagus_style::values::Rgba,
    blur: f32,
    interpolate: bool,
) -> Option<(Image, RectF)> {
    use skrifa::outline::OutlinePen;

    /// Collects an outline in CSS pixels, flipped to y down, from where its glyph is set.
    struct Pen<'a> {
        path: &'a mut tiny_skia::PathBuilder,
        x: f32,
        y: f32,
    }
    impl OutlinePen for Pen<'_> {
        fn move_to(&mut self, x: f32, y: f32) {
            self.path.move_to(self.x + x, self.y - y);
        }
        fn line_to(&mut self, x: f32, y: f32) {
            self.path.line_to(self.x + x, self.y - y);
        }
        fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
            self.path
                .quad_to(self.x + cx, self.y - cy, self.x + x, self.y - y);
        }
        fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
            self.path.cubic_to(
                self.x + cx0,
                self.y - cy0,
                self.x + cx1,
                self.y - cy1,
                self.x + x,
                self.y - y,
            );
        }
        fn close(&mut self) {
            self.path.close();
        }
    }

    let mut path = tiny_skia::PathBuilder::new();
    let mut pen_x = origin.0;
    for glyph in glyphs {
        let mut pen = Pen {
            path: &mut path,
            x: pen_x + glyph.x_offset,
            y: origin.1 - glyph.y_offset,
        };
        face.draw_glyph(glyph.id, size, variations, &mut pen);
        pen_x += glyph.advance;
    }
    for line in lines {
        if let Some(rect) = tiny_skia::Rect::from_xywh(line.x, line.y, line.width, line.height) {
            path.push_rect(rect);
        }
    }
    let path = path.finish()?;
    let sigma = blur / 2.0;
    let glyph_bounds = path.bounds();
    let bounds = grow(
        RectF {
            x: glyph_bounds.x(),
            y: glyph_bounds.y(),
            width: glyph_bounds.width(),
            height: glyph_bounds.height(),
        },
        sigma * 3.0,
    );
    let mut scale = if interpolate {
        (2.0 / sigma).clamp(0.25, 2.0)
    } else {
        2.0
    };
    scale = scale
        .min(MAX_PIXELS / bounds.width.max(1.0))
        .min(MAX_PIXELS / bounds.height.max(1.0));
    let width = (bounds.width * scale).ceil().max(1.0) as u32;
    let height = (bounds.height * scale).ceil().max(1.0) as u32;
    let mut pixmap = tiny_skia::Pixmap::new(width, height)?;
    let paint = tiny_skia::Paint {
        shader: tiny_skia::Shader::SolidColor(tiny_skia::Color::BLACK),
        anti_alias: true,
        ..tiny_skia::Paint::default()
    };
    pixmap.fill_path(
        &path,
        &paint,
        tiny_skia::FillRule::Winding,
        tiny_skia::Transform::from_row(scale, 0.0, 0.0, scale, -bounds.x * scale, -bounds.y * scale),
        None,
    );
    let (width, height) = (width as usize, height as usize);
    let mut mask: Vec<f32> = pixmap
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| f32::from(pixel[3]) / 255.0)
        .collect();
    gaussian(&mut mask, width, height, sigma * scale);
    let alpha: Vec<u8> = mask
        .iter()
        .map(|value| (value.clamp(0.0, 1.0) * color.a * 255.0).round() as u8)
        .collect();
    let rgb = [channel(color.r), channel(color.g), channel(color.b)];
    let Ok(image) = Image::from_custom(
        RgbaImage {
            color: rgb.repeat(width * height),
            alpha,
            width: width as u32,
            height: height as u32,
        },
        interpolate,
    ) else {
        return None;
    };
    Some((image, bounds))
}

/// The shadow over `bounds` as an image.
fn blurred(shadow: &BoxShadowPaint, bounds: RectF, sigma: f32, interpolate: bool) -> Option<Image> {
    if let Some(outlines) = &shadow.outlines {
        return blurred_outline(shadow, &outlines.shape, bounds, sigma, interpolate);
    }
    // What casts an inset shadow.
    let frame = shadow.inset.then(|| casting_frame(shadow, sigma));
    // Pixels per CSS pixel: a soft shadow needs few, at least two per standard deviation; more
    // where viewers may not smooth the image.
    let mut scale = if interpolate {
        (2.0 / sigma).clamp(0.25, 2.0)
    } else {
        2.0
    };
    scale = scale
        .min(MAX_PIXELS / bounds.width.max(1.0))
        .min(MAX_PIXELS / bounds.height.max(1.0));
    let width = (bounds.width * scale).ceil().max(1.0) as usize;
    let height = (bounds.height * scale).ceil().max(1.0) as usize;
    let mut mask = vec![0.0_f32; width * height];
    for (row, line) in mask.chunks_mut(width).enumerate() {
        let y = bounds.y + (row as f32 + 0.5) / scale;
        for (column, value) in line.iter_mut().enumerate() {
            let x = bounds.x + (column as f32 + 0.5) / scale;
            let mut inside = coverage(x, y, shadow.shape, shadow.shape_radii.as_ref(), scale);
            if let Some((hole, radii)) = &shadow.hole {
                inside = (inside - coverage(x, y, *hole, radii.as_ref(), scale)).max(0.0);
            }
            *value = match frame {
                Some(frame) => coverage(x, y, frame, None, scale) * (1.0 - inside),
                None => inside,
            };
        }
    }
    gaussian(&mut mask, width, height, sigma * scale);
    let alpha: Vec<u8> = mask
        .iter()
        .map(|value| (value.clamp(0.0, 1.0) * shadow.color.a * 255.0).round() as u8)
        .collect();
    let color = [
        channel(shadow.color.r),
        channel(shadow.color.g),
        channel(shadow.color.b),
    ];
    Image::from_custom(
        RgbaImage {
            color: color.repeat(width * height),
            alpha,
            width: width as u32,
            height: height as u32,
        },
        interpolate,
    )
    .ok()
}

/// The shadow over `bounds` as an image, of the outline `shape` (of a box with shaped corners).
fn blurred_outline(
    shadow: &BoxShadowPaint,
    shape: &[fagus_layout::PathSegment],
    bounds: RectF,
    sigma: f32,
    interpolate: bool,
) -> Option<Image> {
    let mut scale = if interpolate {
        (2.0 / sigma).clamp(0.25, 2.0)
    } else {
        2.0
    };
    scale = scale
        .min(MAX_PIXELS / bounds.width.max(1.0))
        .min(MAX_PIXELS / bounds.height.max(1.0));
    let width = (bounds.width * scale).ceil().max(1.0) as u32;
    let height = (bounds.height * scale).ceil().max(1.0) as u32;
    let mut pixmap = tiny_skia::Pixmap::new(width, height)?;
    let mut builder = tiny_skia::PathBuilder::new();
    for segment in shape {
        match *segment {
            fagus_layout::PathSegment::Move(x, y) => builder.move_to(x, y),
            fagus_layout::PathSegment::Line(x, y) => builder.line_to(x, y),
            fagus_layout::PathSegment::Cubic([x1, y1, x2, y2, x, y]) => {
                builder.cubic_to(x1, y1, x2, y2, x, y);
            }
            fagus_layout::PathSegment::Close => builder.close(),
        }
    }
    if let Some(path) = builder.finish() {
        let paint = tiny_skia::Paint {
            shader: tiny_skia::Shader::SolidColor(tiny_skia::Color::BLACK),
            anti_alias: true,
            ..tiny_skia::Paint::default()
        };
        pixmap.fill_path(
            &path,
            &paint,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::from_row(scale, 0.0, 0.0, scale, -bounds.x * scale, -bounds.y * scale),
            None,
        );
    }
    let (width, height) = (width as usize, height as usize);
    // What casts an inset shadow.
    let frame = shadow.inset.then(|| casting_frame(shadow, sigma));
    let mut mask: Vec<f32> = pixmap
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .map(|(index, pixel)| {
            let inside = f32::from(pixel[3]) / 255.0;
            match frame {
                Some(frame) => {
                    let x = bounds.x + ((index % width) as f32 + 0.5) / scale;
                    let y = bounds.y + ((index / width) as f32 + 0.5) / scale;
                    coverage(x, y, frame, None, scale) * (1.0 - inside)
                }
                None => inside,
            }
        })
        .collect();
    gaussian(&mut mask, width, height, sigma * scale);
    let alpha: Vec<u8> = mask
        .iter()
        .map(|value| (value.clamp(0.0, 1.0) * shadow.color.a * 255.0).round() as u8)
        .collect();
    let color = [
        channel(shadow.color.r),
        channel(shadow.color.g),
        channel(shadow.color.b),
    ];
    Image::from_custom(
        RgbaImage {
            color: color.repeat(width * height),
            alpha,
            width: width as u32,
            height: height as u32,
        },
        interpolate,
    )
    .ok()
}

/// How much of the pixel around `(x, y)` lies inside the rounded rectangle, from the signed
/// distance to its edge.
pub(crate) fn coverage(x: f32, y: f32, rect: RectF, radii: Option<&Radii>, scale: f32) -> f32 {
    let (left, top, right, bottom) = (rect.x, rect.y, rect.x + rect.width, rect.y + rect.height);
    let mut distance = (left - x).max(x - right).max(top - y).max(y - bottom);
    if let Some(radii) = radii {
        let corners = [
            (
                left + radii[0].0,
                top + radii[0].1,
                radii[0],
                x < left + radii[0].0 && y < top + radii[0].1,
            ),
            (
                right - radii[1].0,
                top + radii[1].1,
                radii[1],
                x > right - radii[1].0 && y < top + radii[1].1,
            ),
            (
                right - radii[2].0,
                bottom - radii[2].1,
                radii[2],
                x > right - radii[2].0 && y > bottom - radii[2].1,
            ),
            (
                left + radii[3].0,
                bottom - radii[3].1,
                radii[3],
                x < left + radii[3].0 && y > bottom - radii[3].1,
            ),
        ];
        for (cx, cy, (rx, ry), within) in corners {
            if within && rx > 0.0 && ry > 0.0 {
                let normalized = ((x - cx) / rx).hypot((y - cy) / ry);
                distance = (normalized - 1.0) * rx.min(ry);
            }
        }
    }
    (0.5 - distance * scale).clamp(0.0, 1.0)
}

/// A Gaussian blur with standard deviation `sigma` (in pixels); the edges repeat. From two
/// pixels on, three box blurs approximate it as in SVG's `feGaussianBlur`, in time that does not
/// grow with the blur.
pub(crate) fn gaussian(values: &mut [f32], width: usize, height: usize, sigma: f32) {
    if sigma < 0.1 {
        return;
    }
    if sigma >= 2.0 {
        let size = (sigma * 3.0 * (2.0 * std::f32::consts::PI).sqrt() / 4.0 + 0.5).floor() as usize;
        let mut buffer = vec![0.0_f32; values.len()];
        for pass in 0..3 {
            // A box of an even size is centered between two pixels: once on the left of each
            // pixel, once on the right, and the third box is one wider (Filter Effects 1, §9.8).
            let (size, before) = match (size % 2, pass) {
                (1, _) => (size, size / 2),
                (_, 0) => (size, size / 2),
                (_, 1) => (size, size / 2 - 1),
                _ => (size + 1, size / 2),
            };
            box_blur(values, &mut buffer, width, height, size, before, true);
            box_blur(&buffer, values, width, height, size, before, false);
        }
        return;
    }
    let radius = (sigma * 3.0).ceil() as isize;
    let kernel: Vec<f32> = (-radius..=radius)
        .map(|offset| (-(offset * offset) as f32 / (2.0 * sigma * sigma)).exp())
        .collect();
    let total: f32 = kernel.iter().sum();
    let kernel: Vec<f32> = kernel.iter().map(|weight| weight / total).collect();
    let mut buffer = vec![0.0_f32; values.len()];
    // Rows.
    for row in 0..height {
        let line = &values[row * width..(row + 1) * width];
        for column in 0..width {
            let mut sum = 0.0;
            for (index, weight) in kernel.iter().enumerate() {
                let source = (column as isize + index as isize - radius).clamp(0, width as isize - 1);
                sum += line[source as usize] * weight;
            }
            buffer[row * width + column] = sum;
        }
    }
    // Columns.
    for column in 0..width {
        for row in 0..height {
            let mut sum = 0.0;
            for (index, weight) in kernel.iter().enumerate() {
                let source = (row as isize + index as isize - radius).clamp(0, height as isize - 1);
                sum += buffer[source as usize * width + column] * weight;
            }
            values[row * width + column] = sum;
        }
    }
}

/// One box blur along rows (`horizontal`) or columns, from `source` into `target`: each pixel
/// becomes the mean of `size` pixels, `before` of them before it. Edges repeat.
fn box_blur(
    source: &[f32],
    target: &mut [f32],
    width: usize,
    height: usize,
    size: usize,
    before: usize,
    horizontal: bool,
) {
    if width == 0 || height == 0 {
        return;
    }
    let before = before as isize;
    let after = size as isize - 1 - before;
    let weight = 1.0 / size as f32;
    if horizontal {
        let last = width as isize - 1;
        for (line, out) in source.chunks_exact(width).zip(target.chunks_exact_mut(width)) {
            let at = |position: isize| line[position.clamp(0, last) as usize];
            let mut sum: f32 = (-before..=after).map(at).sum();
            for (position, value) in out.iter_mut().enumerate() {
                *value = sum * weight;
                let position = position as isize;
                sum += at(position + after + 1) - at(position - before);
            }
        }
        return;
    }
    // Columns are blurred side by side, a row at a time, which reads the image in its order:
    // each column adds and subtracts the same pixels in the same order as on its own.
    let last = height as isize - 1;
    let row = |position: isize| {
        let start = position.clamp(0, last) as usize * width;
        &source[start..start + width]
    };
    let mut sums: Vec<f32> = (0..width)
        .map(|column| (-before..=after).map(|position| row(position)[column]).sum())
        .collect();
    for (position, out) in target.chunks_exact_mut(width).enumerate().take(height) {
        for (value, sum) in out.iter_mut().zip(&sums) {
            *value = sum * weight;
        }
        let position = position as isize;
        let (added, removed) = (row(position + after + 1), row(position - before));
        for ((sum, add), remove) in sums.iter_mut().zip(added).zip(removed) {
            *sum += add - remove;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_the_image_of_a_shadow_by_its_shape_not_its_place() {
        // The shadow of a box broken across pages is drawn whole on each, lower on each page:
        // one image serves them all.
        let shadow = |y: f32, height: f32| BoxShadowPaint {
            area: RectF {
                x: 20.0,
                y,
                width: 200.0,
                height,
            },
            area_radii: None,
            shape: RectF {
                x: 20.0,
                y: y + 4.0,
                width: 200.0,
                height,
            },
            shape_radii: Some([(8.0, 8.0); 4]),
            hole: None,
            color: fagus_style::values::Rgba {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.5,
            },
            blur: 12.0,
            inset: false,
            outlines: None,
        };
        let key = |shadow: &BoxShadowPaint| image_key(shadow, grow(shadow.shape, 18.0));
        let first = shadow(100.0, 1500.0);
        let next = shadow(100.0 - 1123.3, 1500.0);
        assert_eq!(key(&first), key(&next));
        assert_ne!(key(&first), key(&shadow(100.0, 1400.0)));
        let lighter = BoxShadowPaint {
            color: fagus_style::values::Rgba {
                a: 0.4,
                ..first.color
            },
            ..first.clone()
        };
        assert_ne!(key(&first), key(&lighter));
    }

    #[test]
    fn casts_inset_shadows_from_a_frame_around_the_area() {
        let area = RectF {
            x: 10.0,
            y: 10.0,
            width: 100.0,
            height: 100.0,
        };
        let shadow = |shape: RectF| BoxShadowPaint {
            area,
            area_radii: None,
            shape,
            shape_radii: None,
            hole: None,
            color: fagus_style::values::Rgba {
                r: 1.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
            blur: 20.0,
            inset: true,
            outlines: None,
        };
        // Neither moved nor spread: a frame as wide as the standard deviation around the area.
        assert_eq!(
            casting_frame(&shadow(area), 10.0),
            RectF {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 120.0
            }
        );
        // Moved 6px right, the hole is; the frame grows to the left as well.
        let moved = RectF { x: 16.0, ..area };
        assert_eq!(casting_frame(&shadow(moved), 10.0).x, -6.0);
        // The shadow blurs into the area from its edges, and fades to nothing within it.
        let bounds = grow(casting_frame(&shadow(area), 10.0), 30.0);
        let mut mask = Vec::new();
        let scale = 1.0;
        let (width, height) = (bounds.width as usize, bounds.height as usize);
        for row in 0..height {
            for column in 0..width {
                let (x, y) = (bounds.x + column as f32 + 0.5, bounds.y + row as f32 + 0.5);
                let frame = casting_frame(&shadow(area), 10.0);
                mask.push(coverage(x, y, frame, None, scale) * (1.0 - coverage(x, y, area, None, scale)));
            }
        }
        gaussian(&mut mask, width, height, 10.0);
        let at = |x: f32| mask[(60.0 - bounds.y) as usize * width + (x - bounds.x) as usize];
        // Chrome prints about 0.3 at the edge and 0.12 10px in.
        assert!((at(10.0) - 0.32).abs() < 0.05, "{}", at(10.0));
        assert!((at(20.0) - 0.13).abs() < 0.04, "{}", at(20.0));
        assert!(at(60.0) < 0.01, "{}", at(60.0));
    }

    #[test]
    fn blurs_columns_as_one_column_at_a_time() {
        // Box blurs as they were done before, a line after the other.
        fn line_by_line(source: &[f32], width: usize, height: usize, size: usize, before: usize) -> Vec<f32> {
            let mut target = vec![0.0; source.len()];
            let (before, after) = (before as isize, size as isize - 1 - before as isize);
            let last = height as isize - 1;
            for column in 0..width {
                let at = |row: isize| source[row.clamp(0, last) as usize * width + column];
                let mut sum: f32 = (-before..=after).map(at).sum();
                for row in 0..height {
                    target[row * width + column] = sum * (1.0 / size as f32);
                    sum += at(row as isize + after + 1) - at(row as isize - before);
                }
            }
            target
        }
        let (width, height) = (37, 23);
        let source: Vec<f32> = (0..width * height)
            .map(|index| ((index * 7919) % 101) as f32 / 101.0)
            .collect();
        for (size, before) in [(1, 0), (4, 2), (4, 1), (5, 2), (30, 15)] {
            let mut target = vec![0.0; source.len()];
            box_blur(&source, &mut target, width, height, size, before, false);
            let expected = line_by_line(&source, width, height, size, before);
            assert!(
                target
                    .iter()
                    .zip(&expected)
                    .all(|(a, b)| a.to_bits() == b.to_bits()),
                "size {size}"
            );
        }
    }

    #[test]
    fn blurs_without_losing_coverage() {
        let mut values = vec![0.0; 21 * 21];
        values[10 * 21 + 10] = 1.0;
        gaussian(&mut values, 21, 21, 2.0);
        let total: f32 = values.iter().sum();
        assert!((total - 1.0).abs() < 1e-3, "{total}");
        assert!(values[10 * 21 + 10] > values[10 * 21 + 12]);
    }

    #[test]
    fn approximates_wide_blurs_with_boxes() {
        let size = 201;
        let mut values = vec![0.0; size * size];
        values[100 * size + 100] = 1.0;
        let mut exact = values.clone();
        gaussian(&mut values, size, size, 12.0);
        // The exact kernel, for comparison.
        let sigma = 12.0_f32;
        for row in 0..size {
            for column in 0..size {
                let (dx, dy) = (column as f32 - 100.0, row as f32 - 100.0);
                exact[row * size + column] = (-(dx * dx + dy * dy) / (2.0 * sigma * sigma)).exp()
                    / (2.0 * std::f32::consts::PI * sigma * sigma);
            }
        }
        let total: f32 = values.iter().sum();
        assert!((total - 1.0).abs() < 1e-3, "{total}");
        let peak = exact[100 * size + 100];
        for (approximated, exact) in values.iter().zip(&exact) {
            assert!(
                (approximated - exact).abs() < peak * 0.1,
                "{approximated} {exact}"
            );
        }
    }

    #[test]
    fn measures_coverage_of_rounded_corners() {
        let rect = RectF {
            x: 0.0,
            y: 0.0,
            width: 20.0,
            height: 20.0,
        };
        let radii = [(10.0, 10.0); 4];
        assert_eq!(coverage(10.0, 10.0, rect, Some(&radii), 1.0), 1.0);
        assert_eq!(
            coverage(0.5, 0.5, rect, Some(&radii), 1.0),
            0.0,
            "cut off by the corner"
        );
        assert_eq!(coverage(0.5, 0.5, rect, None, 1.0), 1.0);
    }
}

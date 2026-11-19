//! A rasterizer for display lists: draws the items of a page, or of a part of one, onto a
//! tiny-skia pixmap as the PDF writer draws them into a PDF, for what only pixels can show
//! (SVG filters, see [`crate::filtered`]). Boxes, borders, backgrounds and gradients, images,
//! the outlines of glyphs, shadows, clips, transforms, opacity, blend modes and masks are drawn
//! antialiased, as Skia draws them in Chrome.

use std::sync::Arc;

use fagus_paint::{BoxShadowPaint, ClipGeometry, GradientPaint, Item, Radii, RectF};
use fagus_style::values::{BorderStyle, Rgba};
use fagus_text::FontDb;
use tiny_skia::{
    BlendMode, Color, FillRule, FilterQuality, GradientStop, IntSize, LineCap, LinearGradient, Mask,
    MaskType, Paint, Path, PathBuilder, Pixmap, PixmapPaint, Point, RadialGradient, Shader, SpreadMode,
    Stroke, StrokeDash, Transform,
};

use crate::{
    append_centerline, append_rounded_rect, append_segments, append_shape, dash_pattern, fake_bold_width,
    segments_length,
};

/// The most tiles of a background layer drawn one by one; a layer of more draws its first ones.
const MAX_TILES: usize = 4096;

/// What paths are built with: krilla's path builder for the PDF, tiny-skia's for pixels.
pub(crate) trait Outline {
    fn move_to(&mut self, x: f32, y: f32);
    fn line_to(&mut self, x: f32, y: f32);
    fn cubic_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32);
    fn close(&mut self);
}

impl Outline for krilla::geom::PathBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        Self::move_to(self, x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        Self::line_to(self, x, y);
    }
    fn cubic_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        Self::cubic_to(self, x1, y1, x2, y2, x, y);
    }
    fn close(&mut self) {
        Self::close(self);
    }
}

impl Outline for PathBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        Self::move_to(self, x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        Self::line_to(self, x, y);
    }
    fn cubic_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        Self::cubic_to(self, x1, y1, x2, y2, x, y);
    }
    fn close(&mut self) {
        Self::close(self);
    }
}

/// Draws `items` onto a transparent pixmap of `width` × `height` pixels, `transform` mapping
/// their CSS pixels to its pixels; `None` for a pixmap of no size or one too large to allocate.
pub(crate) fn rasterize(
    items: &[Item],
    fonts: &FontDb,
    width: u32,
    height: u32,
    transform: Transform,
) -> Option<Pixmap> {
    let (canvas, held) = crate::raster::pixmap(width, height)?;
    let mut rasterizer = Rasterizer {
        fonts,
        pixmaps: vec![canvas],
        held: vec![held],
        transform,
        clip: None,
        frames: Vec::new(),
    };
    for item in items {
        rasterizer.draw(item);
    }
    while !rasterizer.frames.is_empty() {
        rasterizer.pop();
    }
    rasterizer.pixmaps.into_iter().next()
}

/// Draws a display list onto pixmaps.
struct Rasterizer<'a> {
    fonts: &'a FontDb,
    /// The pixmap of the canvas, then one for every group open above it, drawn into the last.
    pixmaps: Vec<Pixmap>,
    /// The pixmaps, charged to the budget of the render while they live.
    held: Vec<fagus_dom::budget::Held>,
    /// From CSS pixels to the pixels of the pixmaps.
    transform: Transform,
    /// Where drawing shows, if it is clipped.
    clip: Option<Arc<Mask>>,
    /// What every push started, for its pop.
    frames: Vec<Frame>,
}

/// What a push started: the transform and the clip in effect before it, and for a group how
/// its pixmap goes onto the one below.
struct Frame {
    transform: Transform,
    clip: Option<Arc<Mask>>,
    group: Option<Group>,
}

/// How the pixmap of a group goes onto the one below it.
struct Group {
    opacity: f32,
    blend: BlendMode,
    mask: Option<Mask>,
}

impl Rasterizer<'_> {
    fn size(&self) -> (u32, u32) {
        let canvas = &self.pixmaps[0];
        (canvas.width(), canvas.height())
    }

    /// The pixmap drawn into.
    fn target(&mut self) -> &mut Pixmap {
        self.pixmaps.last_mut().expect("the canvas is always there")
    }

    /// How many pixels a CSS pixel spans, on average across directions.
    fn scale(&self) -> f32 {
        let t = self.transform;
        (t.sx * t.sy - t.kx * t.ky).abs().sqrt()
    }

    fn draw(&mut self, item: &Item) {
        match item {
            Item::Rect { rect, color, radii } => {
                if let Some(path) = shape(*rect, radii.as_ref()) {
                    self.fill(&path, &solid(*color), FillRule::Winding);
                }
            }
            Item::BorderSide {
                points,
                color,
                style,
                width,
            } => self.border_side(points, *color, *style, *width),
            Item::RoundedBorder {
                outer,
                outer_radii,
                inner,
                inner_radii,
                color,
                stroke,
            } => {
                let mut builder = PathBuilder::new();
                if let Some(width) = stroke {
                    // Stroked along the middle between the two edges.
                    let middle = RectF {
                        x: f32::midpoint(outer.x, inner.x),
                        y: f32::midpoint(outer.y, inner.y),
                        width: f32::midpoint(outer.width, inner.width),
                        height: f32::midpoint(outer.height, inner.height),
                    };
                    let radii: Radii = std::array::from_fn(|corner| {
                        (
                            f32::midpoint(outer_radii[corner].0, inner_radii[corner].0),
                            f32::midpoint(outer_radii[corner].1, inner_radii[corner].1),
                        )
                    });
                    append_rounded_rect(&mut builder, middle, &radii);
                    if let Some(path) = builder.finish() {
                        // A hairline is a pixel of the PDF's resolution wide.
                        let width = if *width > 0.0 { *width } else { 1.0 / self.scale() };
                        self.stroke(&path, &solid(*color), width, None, LineCap::Butt);
                    }
                } else {
                    append_rounded_rect(&mut builder, *outer, outer_radii);
                    append_rounded_rect(&mut builder, *inner, inner_radii);
                    if let Some(path) = builder.finish() {
                        self.fill(&path, &solid(*color), FillRule::EvenOdd);
                    }
                }
            }
            Item::DashedBorder {
                outer,
                outer_radii,
                inner,
                inner_radii,
                center,
                center_radii,
                color,
                style,
                width,
                stroke,
                centerline,
            } => {
                let mut ring = PathBuilder::new();
                append_rounded_rect(&mut ring, *outer, outer_radii);
                if inner.width > 0.0 && inner.height > 0.0 {
                    append_rounded_rect(&mut ring, *inner, inner_radii);
                }
                let mut middle = PathBuilder::new();
                let length = match centerline {
                    Some(segments) => {
                        append_segments(&mut middle, segments);
                        segments_length(segments)
                    }
                    None => append_centerline(&mut middle, *center, center_radii),
                };
                let (Some(ring), Some(path)) = (ring.finish(), middle.finish()) else {
                    return;
                };
                // A solid one (`border-shape`) is one stroke.
                let pattern = if *style == BorderStyle::Solid {
                    None
                } else {
                    dash_pattern(*style, *width, length.trunc(), true)
                };
                let round = matches!(pattern, Some((_, true)));
                let stroke_width = if round { *width } else { stroke * 2.2 };
                let outer_clip = self.clip.clone();
                self.intersect_clip(&ring, FillRule::EvenOdd);
                self.stroke(
                    &path,
                    &solid(*color),
                    stroke_width,
                    pattern.map(|(array, _)| array),
                    if round { LineCap::Round } else { LineCap::Butt },
                );
                self.clip = outer_clip;
            }
            Item::Tiles {
                area,
                origin,
                step,
                tile,
            } => self.tiles(*area, *origin, *step, tile),
            Item::Text {
                x,
                y,
                face,
                size,
                variations,
                color,
                glyphs,
                synthetic,
                vertical,
                ..
            } => {
                let font = self.fonts.face(*face);
                let adjusted = size * font.size_adjust;
                if glyphs.is_empty() || adjusted <= 0.0 {
                    return;
                }
                let paint = solid(*color);
                // Glyphs lean from their baseline: that of the run, or for glyphs one below the
                // other each its own.
                let slant = |baseline: f32| {
                    if synthetic.italic {
                        Transform::from_row(1.0, 0.0, -crate::SLANT, 1.0, crate::SLANT * baseline, 0.0)
                    } else {
                        Transform::identity()
                    }
                };
                let mut runs: Vec<(PathBuilder, f32)> = vec![(PathBuilder::new(), *y)];
                let mut pen = if *vertical { *y } else { *x };
                for glyph in glyphs {
                    let origin = if *vertical {
                        (x + glyph.x_offset, pen - glyph.y_offset)
                    } else {
                        (pen + glyph.x_offset, y - glyph.y_offset)
                    };
                    if *vertical && synthetic.italic {
                        runs.push((PathBuilder::new(), origin.1));
                    }
                    let Some((builder, _)) = runs.last_mut() else {
                        break;
                    };
                    let mut glyph_pen = GlyphPen {
                        path: builder,
                        x: origin.0,
                        y: origin.1,
                    };
                    font.draw_glyph(glyph.id, *size, variations, &mut glyph_pen);
                    pen += glyph.advance;
                }
                let outer = self.transform;
                for (builder, baseline) in runs {
                    let Some(path) = builder.finish() else { continue };
                    self.transform = outer.pre_concat(slant(baseline));
                    self.fill(&path, &paint, FillRule::Winding);
                    // Synthetic bold: an outline in the text color thickens every stroke.
                    if synthetic.bold {
                        self.stroke(&path, &paint, fake_bold_width(adjusted), None, LineCap::Butt);
                    }
                }
                self.transform = outer;
            }
            Item::Wave {
                start,
                y,
                control,
                wavelength,
                thickness,
                clip,
                color,
            } => {
                if *wavelength <= 0.0 {
                    return;
                }
                let first = start - ((start - clip.x) / wavelength).ceil().max(0.0) * wavelength;
                let count = ((clip.x + clip.width - first) / wavelength)
                    .ceil()
                    .clamp(1.0, 100_000.0) as usize;
                let mut builder = PathBuilder::new();
                builder.move_to(first, *y);
                for index in 0..count {
                    let from = first + index as f32 * wavelength;
                    let middle = from + wavelength / 2.0;
                    builder.cubic_to(middle, y + control, middle, y - control, from + wavelength, *y);
                }
                if let (Some(path), Some(clip)) = (builder.finish(), shape(*clip, None)) {
                    let outer_clip = self.clip.clone();
                    self.intersect_clip(&clip, FillRule::Winding);
                    self.stroke(&path, &solid(*color), *thickness, None, LineCap::Butt);
                    self.clip = outer_clip;
                }
            }
            Item::DashedLine {
                from,
                to,
                thickness,
                dash,
                dotted,
                color,
            } => {
                let length = libm::hypotf(to.0 - from.0, to.1 - from.1);
                let style = if *dotted {
                    BorderStyle::Dotted
                } else {
                    BorderStyle::Dashed
                };
                let pattern = dash_pattern(
                    style,
                    *dash,
                    if *dotted && *dash > 3.0 {
                        length + dash
                    } else {
                        length
                    },
                    false,
                );
                let mut builder = PathBuilder::new();
                builder.move_to(from.0, from.1);
                builder.line_to(to.0, to.1);
                if let Some(path) = builder.finish() {
                    let cap = if matches!(pattern, Some((_, true))) {
                        LineCap::Round
                    } else {
                        LineCap::Butt
                    };
                    self.stroke(
                        &path,
                        &solid(*color),
                        *thickness,
                        pattern.map(|(array, _)| array),
                        cap,
                    );
                }
            }
            Item::Image { rect, image } => self.image(*rect, image),
            Item::Gradient { rect, gradient } => self.gradient(*rect, gradient),
            Item::BoxShadow(shadow) => self.box_shadow(shadow),
            Item::TextShadow {
                x,
                y,
                face,
                size,
                variations,
                glyphs,
                lines,
                color,
                blur,
            } => {
                let font = self.fonts.face(*face);
                let mut builder = PathBuilder::new();
                let mut pen_x = *x;
                for glyph in glyphs {
                    let mut pen = GlyphPen {
                        path: &mut builder,
                        x: pen_x + glyph.x_offset,
                        y: y - glyph.y_offset,
                    };
                    font.draw_glyph(glyph.id, *size, variations, &mut pen);
                    pen_x += glyph.advance;
                }
                for line in lines {
                    if let Some(rect) = tiny_skia::Rect::from_xywh(line.x, line.y, line.width, line.height) {
                        builder.push_rect(rect);
                    }
                }
                if let Some(path) = builder.finish()
                    && let Some(mut coverage) = self.coverage(&path, FillRule::Winding)
                {
                    self.blur(&mut coverage, blur / 2.0);
                    self.fill_through(&coverage, *color);
                }
            }
            Item::Filtered(filtered) => {
                // Nothing outside the pixmap shows.
                let (width, height) = self.size();
                let visible = RectF {
                    x: 0.0,
                    y: 0.0,
                    width: width as f32,
                    height: height as f32,
                };
                if let Some((pixels, rect)) =
                    crate::filtered::pixels(filtered, self.fonts, self.transform, Some(visible))
                {
                    self.pixmap(&pixels, rect, None);
                }
            }
            Item::PushOpacity(opacity) => {
                self.push_group(opacity.clamp(0.0, 1.0), BlendMode::SourceOver, None);
            }
            Item::PushBlend(mode) => self.push_group(1.0, blend_mode(*mode), None),
            Item::PushIsolated | Item::PushLayer(_) => self.push_group(1.0, BlendMode::SourceOver, None),
            Item::PushMask(mask) => {
                let (width, height) = self.size();
                let drawn = rasterize(&mask.items, self.fonts, width, height, self.transform);
                let mask = drawn.map(|drawn| {
                    let kind = if mask.luminance {
                        MaskType::Luminance
                    } else {
                        MaskType::Alpha
                    };
                    Mask::from_pixmap(drawn.as_ref(), kind)
                });
                // A mask that cannot be drawn hides everything.
                let mask = mask.or_else(|| Mask::new(width, height));
                self.push_group(1.0, BlendMode::SourceOver, mask);
            }
            Item::PushTransform([sx, ky, kx, sy, tx, ty]) => {
                self.push_frame(None);
                self.transform = self
                    .transform
                    .pre_concat(Transform::from_row(*sx, *ky, *kx, *sy, *tx, *ty));
            }
            Item::PushClip(rect, radii) => {
                self.push_frame(None);
                match shape(*rect, radii.as_ref()) {
                    Some(path) => self.intersect_clip(&path, FillRule::Winding),
                    None => self.clip_everything(),
                }
            }
            Item::PushClipShape(geometry) => {
                self.push_frame(None);
                let rule = match geometry {
                    ClipGeometry::Polygon { even_odd: true, .. }
                    | ClipGeometry::Path { even_odd: true, .. } => FillRule::EvenOdd,
                    _ => FillRule::Winding,
                };
                let mut builder = PathBuilder::new();
                match append_shape(&mut builder, geometry).and_then(|()| builder.finish()) {
                    Some(path) => self.intersect_clip(&path, rule),
                    None => self.clip_everything(),
                }
            }
            Item::Pop => self.pop(),
            Item::Tag(_) => {}
        }
    }

    /// Starts a push: keeps the transform and the clip in effect for its pop.
    fn push_frame(&mut self, group: Option<Group>) {
        self.frames.push(Frame {
            transform: self.transform,
            clip: self.clip.clone(),
            group,
        });
    }

    /// Starts a group drawn into a pixmap of its own, which goes onto the one below with
    /// `opacity`, `blend` and `mask` at its pop.
    fn push_group(&mut self, opacity: f32, blend: BlendMode, mask: Option<Mask>) {
        let (width, height) = self.size();
        match crate::raster::pixmap(width, height) {
            Some((pixmap, held)) => {
                self.push_frame(Some(Group { opacity, blend, mask }));
                self.pixmaps.push(pixmap);
                self.held.push(held);
            }
            None => self.push_frame(None),
        }
    }

    /// Ends the last push.
    fn pop(&mut self) {
        let Some(frame) = self.frames.pop() else { return };
        self.transform = frame.transform;
        self.clip = frame.clip;
        if let Some(group) = frame.group
            && let Some(mut pixmap) = self.pixmaps.pop()
        {
            self.held.pop();
            if let Some(mask) = &group.mask {
                pixmap.apply_mask(mask);
            }
            let paint = PixmapPaint {
                opacity: group.opacity,
                blend_mode: group.blend,
                quality: FilterQuality::Nearest,
            };
            self.target()
                .draw_pixmap(0, 0, pixmap.as_ref(), &paint, Transform::identity(), None);
        }
    }

    /// Clips what follows to `path` as well.
    fn intersect_clip(&mut self, path: &Path, rule: FillRule) {
        let (width, height) = self.size();
        let mask = match self.clip.take() {
            Some(clip) => {
                let mut mask = Arc::unwrap_or_clone(clip);
                mask.intersect_path(path, rule, true, self.transform);
                Some(mask)
            }
            None => Mask::new(width, height).map(|mut mask| {
                mask.fill_path(path, rule, true, self.transform);
                mask
            }),
        };
        self.clip = mask.map(Arc::new);
    }

    /// Clips everything that follows away.
    fn clip_everything(&mut self) {
        let (width, height) = self.size();
        self.clip = Mask::new(width, height).map(Arc::new);
    }

    fn fill(&mut self, path: &Path, paint: &Paint<'_>, rule: FillRule) {
        let (transform, clip) = (self.transform, self.clip.clone());
        self.target()
            .fill_path(path, paint, rule, transform, clip.as_deref());
    }

    fn stroke(&mut self, path: &Path, paint: &Paint<'_>, width: f32, dashes: Option<[f32; 2]>, cap: LineCap) {
        let stroke = Stroke {
            width,
            line_cap: cap,
            dash: dashes.and_then(|array| StrokeDash::new(array.to_vec(), 0.0)),
            ..Stroke::default()
        };
        let (transform, clip) = (self.transform, self.clip.clone());
        self.target()
            .stroke_path(path, paint, &stroke, transform, clip.as_deref());
    }

    /// A side of a border: a filled quadrilateral, or a dashed or dotted line along its middle.
    fn border_side(&mut self, points: &[(f32, f32); 4], color: Rgba, style: BorderStyle, width: f32) {
        let paint = solid(color);
        if matches!(style, BorderStyle::Dashed | BorderStyle::Dotted) {
            let across = |a: (f32, f32), b: (f32, f32)| (f32::midpoint(a.0, b.0), f32::midpoint(a.1, b.1));
            let (outer_start, outer_end) = (points[0], points[1]);
            let horizontal = (outer_end.0 - outer_start.0).abs() >= (outer_end.1 - outer_start.1).abs();
            let (middle_start, middle_end) = (across(points[0], points[3]), across(points[1], points[2]));
            let (mut start, mut end) = if horizontal {
                ((outer_start.0, middle_start.1), (outer_end.0, middle_end.1))
            } else {
                ((middle_start.0, outer_start.1), (middle_end.0, outer_end.1))
            };
            if (horizontal && start.0 > end.0) || (!horizontal && start.1 > end.1) {
                (start, end) = (end, start);
            }
            let length = libm::hypotf(end.0 - start.0, end.1 - start.1);
            if length <= 0.0 {
                return;
            }
            let pattern = dash_pattern(style, width, length, false);
            let along = ((end.0 - start.0) / length, (end.1 - start.1) / length);
            let round = matches!(pattern, Some((_, true)));
            if round {
                start = (start.0 + along.0 * width / 2.0, start.1 + along.1 * width / 2.0);
                end = (end.0 - along.0 * width / 2.0, end.1 - along.1 * width / 2.0);
            }
            let mut builder = PathBuilder::new();
            builder.move_to(start.0, start.1);
            builder.line_to(end.0, end.1);
            if let Some(path) = builder.finish() {
                let cap = if round { LineCap::Round } else { LineCap::Butt };
                self.stroke(&path, &paint, width, pattern.map(|(array, _)| array), cap);
            }
        } else {
            let mut builder = PathBuilder::new();
            builder.move_to(points[0].0, points[0].1);
            for point in &points[1..] {
                builder.line_to(point.0, point.1);
            }
            builder.close();
            if let Some(path) = builder.finish() {
                self.fill(&path, &paint, FillRule::Winding);
            }
        }
    }

    /// A tile repeated over `area`, from `origin` every `step`.
    fn tiles(&mut self, area: RectF, origin: (f32, f32), step: (f32, f32), tile: &Item) {
        if !(step.0 > 0.0 && step.1 > 0.0) {
            return;
        }
        let Some(clip) = shape(area, None) else { return };
        let outer = (self.transform, self.clip.clone());
        self.intersect_clip(&clip, FillRule::Winding);
        let first = (
            origin.0 + ((area.x - origin.0) / step.0).floor() * step.0,
            origin.1 + ((area.y - origin.1) / step.1).floor() * step.1,
        );
        let columns = ((area.x + area.width - first.0) / step.0).ceil().max(0.0) as usize;
        let rows = ((area.y + area.height - first.1) / step.1).ceil().max(0.0) as usize;
        let mut drawn = 0;
        'rows: for row in 0..rows {
            for column in 0..columns {
                if drawn == MAX_TILES {
                    break 'rows;
                }
                drawn += 1;
                let (dx, dy) = (
                    first.0 + column as f32 * step.0 - origin.0,
                    first.1 + row as f32 * step.1 - origin.1,
                );
                self.transform = outer.0.pre_translate(origin.0 + dx, origin.1 + dy);
                self.draw(tile);
            }
        }
        (self.transform, self.clip) = outer;
    }

    /// An image scaled into `rect`.
    fn image(&mut self, rect: RectF, image: &fagus_layout::Image) {
        if !(rect.width > 0.0 && rect.height > 0.0) {
            return;
        }
        if image.format == fagus_layout::ImageFormat::Svg && image.filters.is_empty() && image.pad == 0 {
            self.svg(rect, image);
            return;
        }
        let drawn = if image.format == fagus_layout::ImageFormat::Svg {
            crate::svg::parse_to_draw(&image.data, self.fonts).and_then(|tree| {
                crate::svg::pixels(
                    &tree,
                    crate::svg::fit(&image.data, image.inline),
                    image.width,
                    image.height,
                )
            })
        } else {
            None
        };
        if image.format == fagus_layout::ImageFormat::Svg && drawn.is_none() {
            return;
        }
        let Some((width, height, mut pixels)) = crate::decode::filtered_pixels(image, drawn) else {
            return;
        };
        premultiply(&mut pixels);
        let Some(pixmap) =
            IntSize::from_wh(width as u32, height as u32).and_then(|size| Pixmap::from_vec(pixels, size))
        else {
            return;
        };
        // The stored pixels, turned and flipped into the box as their EXIF orientation says.
        let (wide, high) = (rect.width, rect.height);
        let turn = match image.orientation {
            2 => [-1.0, 0.0, 0.0, 1.0, wide, 0.0],
            3 => [-1.0, 0.0, 0.0, -1.0, wide, high],
            4 => [1.0, 0.0, 0.0, -1.0, 0.0, high],
            5 => [0.0, 1.0, 1.0, 0.0, 0.0, 0.0],
            6 => [0.0, 1.0, -1.0, 0.0, wide, 0.0],
            7 => [0.0, -1.0, -1.0, 0.0, wide, high],
            8 => [0.0, -1.0, 1.0, 0.0, 0.0, high],
            _ => [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        };
        let stored = if image.orientation >= 5 {
            (high, wide)
        } else {
            (wide, high)
        };
        let [sx, ky, kx, sy, tx, ty] = turn;
        let placed = Transform::from_row(sx, ky, kx, sy, rect.x + tx, rect.y + ty);
        let scaled = placed.pre_scale(stored.0 / width as f32, stored.1 / height as f32);
        self.pixmap_with(&pixmap, scaled);
    }

    /// Draws `pixels` with `transform` from their pixels to CSS pixels, smoothed.
    fn pixmap_with(&mut self, pixels: &Pixmap, transform: Transform) {
        let transform = self.transform.pre_concat(transform);
        let paint = PixmapPaint {
            quality: FilterQuality::Nearest,
            ..PixmapPaint::default()
        };
        let clip = self.clip.clone();
        self.target()
            .draw_pixmap(0, 0, pixels.as_ref(), &paint, transform, clip.as_deref());
    }

    /// Draws `pixels` stretched over `rect`, smoothed unless they fall on whole pixels.
    fn pixmap(&mut self, pixels: &Pixmap, rect: RectF, quality: Option<FilterQuality>) {
        let transform = self
            .transform
            .pre_concat(Transform::from_translate(rect.x, rect.y).pre_scale(
                rect.width / pixels.width() as f32,
                rect.height / pixels.height() as f32,
            ));
        let t = transform;
        let aligned = (t.sx - 1.0).abs() < 1e-3
            && (t.sy - 1.0).abs() < 1e-3
            && t.kx == 0.0
            && t.ky == 0.0
            && (t.tx - t.tx.round()).abs() < 1e-3
            && (t.ty - t.ty.round()).abs() < 1e-3;
        let paint = PixmapPaint {
            quality: quality.unwrap_or(if aligned {
                FilterQuality::Nearest
            } else {
                FilterQuality::Bicubic
            }),
            ..PixmapPaint::default()
        };
        let transform = if aligned {
            Transform::from_translate(t.tx.round(), t.ty.round())
        } else {
            transform
        };
        let clip = self.clip.clone();
        self.target()
            .draw_pixmap(0, 0, pixels.as_ref(), &paint, transform, clip.as_deref());
    }

    /// An SVG image placed in `rect` as the PDF places it.
    fn svg(&mut self, rect: RectF, image: &fagus_layout::Image) {
        let Some(tree) = crate::svg::parse_to_draw(&image.data, self.fonts) else {
            return;
        };
        let natural = (tree.size().width(), tree.size().height());
        let fit = crate::svg::fit(&image.data, image.inline);
        let (x, y, width, height) = fit.place(natural, rect.width, rect.height);
        let (columns, rows) = self.size();
        let Some((mut layer, _held)) = crate::raster::pixmap(columns, rows) else {
            return;
        };
        let transform = self
            .transform
            .pre_translate(rect.x + x, rect.y + y)
            .pre_scale(width / natural.0, height / natural.1);
        if crate::svg::render(&tree, transform, &mut layer.as_mut()).is_none() {
            return;
        }
        // What a `slice` or an unscaled SVG puts outside the box is clipped.
        let outer = self.clip.clone();
        if (x < 0.0 || y < 0.0 || x + width > rect.width + 0.01 || y + height > rect.height + 0.01)
            && let Some(path) = shape(rect, None)
        {
            self.intersect_clip(&path, FillRule::Winding);
        }
        let clip = self.clip.clone();
        self.target().draw_pixmap(
            0,
            0,
            layer.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            clip.as_deref(),
        );
        self.clip = outer;
    }

    /// A rectangle filled with a gradient whose geometry is relative to its top left corner.
    fn gradient(&mut self, rect: RectF, gradient: &GradientPaint) {
        let Some(path) = shape(rect, None) else { return };
        let stops = |stops: &[(f32, Rgba)]| -> Vec<GradientStop> {
            stops
                .iter()
                .map(|(offset, color)| GradientStop::new(*offset, color_of(*color)))
                .collect()
        };
        let shader = match gradient {
            GradientPaint::Solid(color) => Some(Shader::SolidColor(color_of(*color))),
            GradientPaint::Linear {
                start,
                end,
                stops: list,
                repeat,
            } => LinearGradient::new(
                Point::from_xy(rect.x + start.0, rect.y + start.1),
                Point::from_xy(rect.x + end.0, rect.y + end.1),
                stops(list),
                if *repeat {
                    SpreadMode::Repeat
                } else {
                    SpreadMode::Pad
                },
                Transform::identity(),
            )
            .or_else(|| list.last().map(|(_, color)| Shader::SolidColor(color_of(*color)))),
            GradientPaint::Radial {
                center,
                radius,
                scale_y,
                stops: list,
            } => RadialGradient::new(
                Point::from_xy(0.0, 0.0),
                0.0,
                Point::from_xy(0.0, 0.0),
                *radius,
                stops(list),
                SpreadMode::Pad,
                Transform::from_row(1.0, 0.0, 0.0, *scale_y, rect.x + center.0, rect.y + center.1),
            )
            .or_else(|| list.last().map(|(_, color)| Shader::SolidColor(color_of(*color)))),
            GradientPaint::Conic {
                center,
                start_angle,
                stops,
            } => {
                let scale = self.scale().clamp(0.25, 4.0);
                let image = crate::raster::conic_pixels(
                    RectF {
                        x: 0.0,
                        y: 0.0,
                        ..rect
                    },
                    *center,
                    *start_angle,
                    stops,
                    scale,
                );
                if let Some(pixmap) = rgba_pixmap(&image) {
                    self.pixmap(&pixmap, rect, Some(FilterQuality::Bilinear));
                }
                return;
            }
        };
        if let Some(shader) = shader {
            let paint = Paint {
                shader,
                anti_alias: true,
                ..Paint::default()
            };
            self.fill(&path, &paint, FillRule::Winding);
        }
    }

    /// A box shadow: its shape's coverage blurred, outside the box (or inside it for an inset
    /// shadow), in its color.
    fn box_shadow(&mut self, shadow: &BoxShadowPaint) {
        let mut shape_path = PathBuilder::new();
        match &shadow.outlines {
            Some(outlines) => append_segments(&mut shape_path, &outlines.shape),
            None => append_box(&mut shape_path, shadow.shape, shadow.shape_radii.as_ref()),
        }
        let Some(path) = shape_path.finish() else { return };
        let Some(mut coverage) = self.coverage(&path, FillRule::Winding) else {
            return;
        };
        if let Some((hole, radii)) = &shadow.hole
            && let Some(hole) = shape(*hole, radii.as_ref())
            && let Some(hole) = self.coverage(&hole, FillRule::Winding)
        {
            for (value, cut) in coverage.iter_mut().zip(hole) {
                *value = (*value - cut).max(0.0);
            }
        }
        if shadow.inset {
            for value in &mut coverage {
                *value = 1.0 - *value;
            }
        }
        self.blur(&mut coverage, shadow.blur / 2.0);
        let mut area = PathBuilder::new();
        match &shadow.outlines {
            Some(outlines) => append_segments(&mut area, &outlines.area),
            None => append_box(&mut area, shadow.area, shadow.area_radii.as_ref()),
        }
        if let Some(area) = area.finish()
            && let Some(inside) = self.coverage(&area, FillRule::Winding)
        {
            for (value, inside) in coverage.iter_mut().zip(inside) {
                *value *= if shadow.inset { inside } else { 1.0 - inside };
            }
        }
        self.fill_through(&coverage, shadow.color);
    }

    /// How much of each pixel `path` covers, from 0 to 1.
    fn coverage(&self, path: &Path, rule: FillRule) -> Option<Vec<f32>> {
        let (width, height) = self.size();
        let mut mask = Mask::new(width, height)?;
        mask.fill_path(path, rule, true, self.transform);
        Some(
            mask.data()
                .iter()
                .map(|value| f32::from(*value) / 255.0)
                .collect(),
        )
    }

    /// Blurs coverage with a Gaussian of the standard deviation `sigma` in CSS pixels.
    fn blur(&self, coverage: &mut [f32], sigma: f32) {
        let (width, height) = self.size();
        crate::shadow::gaussian(coverage, width as usize, height as usize, sigma * self.scale());
    }

    /// Fills the canvas in `color` as far as `coverage` reaches, within the clip.
    fn fill_through(&mut self, coverage: &[f32], color: Rgba) {
        let (width, height) = self.size();
        let data = coverage
            .iter()
            .map(|value| (value.clamp(0.0, 1.0) * 255.0).round() as u8)
            .collect();
        let Some(mut mask) = IntSize::from_wh(width, height).and_then(|size| Mask::from_vec(data, size))
        else {
            return;
        };
        if let Some(clip) = &self.clip {
            for (value, clip) in mask.data_mut().iter_mut().zip(clip.data()) {
                *value = ((u16::from(*value) * u16::from(*clip) + 127) / 255) as u8;
            }
        }
        if let Some(rect) = tiny_skia::Rect::from_xywh(0.0, 0.0, width as f32, height as f32) {
            self.target()
                .fill_rect(rect, &solid(color), Transform::identity(), Some(&mask));
        }
    }
}

/// Collects the outline of a glyph in CSS pixels, flipped to y down, from where it is set.
struct GlyphPen<'a> {
    path: &'a mut PathBuilder,
    x: f32,
    y: f32,
}

impl skrifa::outline::OutlinePen for GlyphPen<'_> {
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

/// A color of the display list for tiny-skia.
fn color_of(color: Rgba) -> Color {
    Color::from_rgba(
        color.r.clamp(0.0, 1.0),
        color.g.clamp(0.0, 1.0),
        color.b.clamp(0.0, 1.0),
        color.a.clamp(0.0, 1.0),
    )
    .unwrap_or(Color::TRANSPARENT)
}

/// An antialiased paint of one color.
fn solid(color: Rgba) -> Paint<'static> {
    Paint {
        shader: Shader::SolidColor(color_of(color)),
        anti_alias: true,
        ..Paint::default()
    }
}

/// A rectangle with optional rounded corners as a path.
fn shape(rect: RectF, radii: Option<&Radii>) -> Option<Path> {
    let mut builder = PathBuilder::new();
    append_box(&mut builder, rect, radii);
    builder.finish()
}

/// Appends a rectangle with optional rounded corners.
fn append_box(builder: &mut PathBuilder, rect: RectF, radii: Option<&Radii>) {
    match radii {
        Some(radii) => append_rounded_rect(builder, rect, radii),
        None => {
            if let Some(rect) =
                tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.width.max(0.0), rect.height.max(0.0))
            {
                builder.push_rect(rect);
            }
        }
    }
}

/// The tiny-skia blend mode of a CSS one.
fn blend_mode(mode: fagus_style::values::BlendMode) -> BlendMode {
    use fagus_style::values::BlendMode as Css;
    match mode {
        Css::Normal => BlendMode::SourceOver,
        Css::Multiply => BlendMode::Multiply,
        Css::Screen => BlendMode::Screen,
        Css::Overlay => BlendMode::Overlay,
        Css::Darken => BlendMode::Darken,
        Css::Lighten => BlendMode::Lighten,
        Css::ColorDodge => BlendMode::ColorDodge,
        Css::ColorBurn => BlendMode::ColorBurn,
        Css::HardLight => BlendMode::HardLight,
        Css::SoftLight => BlendMode::SoftLight,
        Css::Difference => BlendMode::Difference,
        Css::Exclusion => BlendMode::Exclusion,
        Css::Hue => BlendMode::Hue,
        Css::Saturation => BlendMode::Saturation,
        Css::Color => BlendMode::Color,
        Css::Luminosity => BlendMode::Luminosity,
    }
}

/// Premultiplies unpremultiplied RGBA pixels by their alpha, as tiny-skia stores them.
pub(crate) fn premultiply(pixels: &mut [u8]) {
    for pixel in pixels.as_chunks_mut::<4>().0 {
        let alpha = u16::from(pixel[3]);
        if alpha < 255 {
            for channel in &mut pixel[..3] {
                *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
            }
        }
    }
}

/// An image of [`crate::raster`] as a pixmap.
fn rgba_pixmap(image: &crate::raster::RgbaImage) -> Option<Pixmap> {
    let count = image.width as usize * image.height as usize;
    let mut pixels = Vec::with_capacity(count * 4);
    for index in 0..count {
        let color = image.color.get(index * 3..index * 3 + 3)?;
        pixels.extend_from_slice(color);
        pixels.push(image.alpha.get(index).copied().unwrap_or(255));
    }
    premultiply(&mut pixels);
    Pixmap::from_vec(pixels, IntSize::from_wh(image.width, image.height)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(pixmap: &Pixmap, x: u32, y: u32) -> [u8; 4] {
        let color = pixmap.pixel(x, y).expect("a pixel inside");
        [color.red(), color.green(), color.blue(), color.alpha()]
    }

    #[test]
    fn rasterizes_boxes_clips_groups_and_transforms() {
        let fonts = FontDb::new();
        let red = Rgba::from_u8(255, 0, 0, 1.0);
        let blue = Rgba::from_u8(0, 0, 255, 1.0);
        let items = vec![
            Item::Rect {
                rect: RectF {
                    x: 0.0,
                    y: 0.0,
                    width: 10.0,
                    height: 10.0,
                },
                color: red,
                radii: None,
            },
            Item::PushTransform([1.0, 0.0, 0.0, 1.0, 10.0, 0.0]),
            Item::PushClip(
                RectF {
                    x: 0.0,
                    y: 0.0,
                    width: 5.0,
                    height: 20.0,
                },
                None,
            ),
            Item::PushOpacity(0.5),
            Item::Rect {
                rect: RectF {
                    x: 0.0,
                    y: 0.0,
                    width: 10.0,
                    height: 10.0,
                },
                color: blue,
                radii: None,
            },
            Item::Pop,
            Item::Pop,
            Item::Pop,
        ];
        // Two pixels per CSS pixel.
        let pixmap = rasterize(&items, &fonts, 40, 20, Transform::from_scale(2.0, 2.0)).expect("a pixmap");
        assert_eq!(pixel(&pixmap, 5, 5), [255, 0, 0, 255]);
        // Half opaque blue from x = 10 to 15, clipped after.
        assert_eq!(pixel(&pixmap, 25, 5), [0, 0, 128, 128]);
        assert_eq!(pixel(&pixmap, 35, 5), [0, 0, 0, 0]);
    }

    #[test]
    fn rasterizes_masks_and_blend_modes() {
        let fonts = FontDb::new();
        let rect = RectF {
            x: 0.0,
            y: 0.0,
            width: 4.0,
            height: 4.0,
        };
        let half = RectF { width: 2.0, ..rect };
        let items = vec![
            Item::Rect {
                rect,
                color: Rgba::from_u8(255, 255, 0, 1.0),
                radii: None,
            },
            Item::PushBlend(fagus_style::values::BlendMode::Multiply),
            Item::PushMask(Arc::new(fagus_paint::MaskPaint {
                items: vec![Item::Rect {
                    rect: half,
                    color: Rgba::BLACK,
                    radii: None,
                }],
                luminance: false,
            })),
            Item::Rect {
                rect,
                color: Rgba::from_u8(0, 255, 255, 1.0),
                radii: None,
            },
            Item::Pop,
            Item::Pop,
        ];
        let pixmap = rasterize(&items, &fonts, 4, 4, Transform::identity()).expect("a pixmap");
        // Yellow multiplied by cyan is green, where the mask shows it.
        assert_eq!(pixel(&pixmap, 1, 1), [0, 255, 0, 255]);
        assert_eq!(pixel(&pixmap, 3, 1), [255, 255, 0, 255]);
    }
}

//! CSS masks on SVG elements (CSS Masking 1, §7: `mask-image` and the other `mask-*`
//! properties), written as an SVG `<mask>` for the SVG renderer, which knows only mask
//! elements: the layers are painted as those of a box are (see [`crate::mask`]), in the boxes of
//! the element, and what they draw is written as SVG.

use std::fmt::Write as _;

use fagus_layout::{ImageFormat, Images};
use fagus_style::ComputedStyle;
use fagus_style::values::Rgba;

pub use crate::mask::SvgBoxes;
use crate::mask::{Frame, paint};
use crate::{GradientPaint, Item, MaskPaint, RectF};

/// How many bytes of images a mask of one element may write, at most; a larger one masks
/// nothing.
const MAX_IMAGE_BYTES: usize = 32 << 20;

/// The definitions of the mask that the `mask-*` properties of `style` make for an SVG element
/// with `boxes` (in its user space), as SVG markup: a `<mask>` with the id `id` and the
/// elements it refers to, whose ids start with `id`. `None` where the element is not masked
/// (no layer shows an image).
pub fn svg_mask(style: &ComputedStyle, boxes: SvgBoxes, images: &Images, id: &str) -> Option<String> {
    let mask = style.mask.as_deref()?;
    if mask.is_none() {
        return None;
    }
    // The layers are painted in pixels of CSS, which the zoom of the element scales: user units
    // times the zoom.
    let zoom = if style.zoom.is_finite() && style.zoom > 0.0 {
        style.zoom
    } else {
        1.0
    };
    let zoomed = |rect: RectF| RectF {
        x: rect.x * zoom,
        y: rect.y * zoom,
        width: rect.width * zoom,
        height: rect.height * zoom,
    };
    let boxes = SvgBoxes {
        fill: zoomed(boxes.fill),
        stroke: zoomed(boxes.stroke),
        view: zoomed(boxes.view),
    };
    // All that the element draws: its stroke box and its viewport, and as much again around
    // them (markers, filters and layers without a clip reach out of its boxes).
    let around = union(boxes.stroke, boxes.view);
    let margin = around.width.max(around.height).max(1.0);
    let region = RectF {
        x: around.x - margin,
        y: around.y - margin,
        width: around.width + 2.0 * margin,
        height: around.height + 2.0 * margin,
    };
    if ![region.x, region.y, region.width, region.height]
        .iter()
        .all(|value| value.is_finite())
    {
        return None;
    }
    let paint = paint(style, mask, Frame::Svg(boxes, region), images)?;
    let mut writer = Writer {
        id,
        defs: String::new(),
        count: 0,
        image_bytes: 0,
    };
    let content = writer.mask(&paint, region)?;
    let scale = if (zoom - 1.0).abs() < f32::EPSILON {
        String::new()
    } else {
        format!(" transform=\"scale({})\"", 1.0 / zoom)
    };
    let mut out = writer.defs;
    let _ = write!(
        out,
        "<mask id=\"{id}\" mask-type=\"{}\" maskUnits=\"userSpaceOnUse\" maskContentUnits=\"userSpaceOnUse\" \
         x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"><g{scale}>{content}</g></mask>",
        if paint.luminance { "luminance" } else { "alpha" },
        region.x / zoom,
        region.y / zoom,
        region.width / zoom,
        region.height / zoom,
    );
    Some(out)
}

/// The smallest rectangle around `a` and `b`.
fn union(a: RectF, b: RectF) -> RectF {
    let (left, top) = (a.x.min(b.x), a.y.min(b.y));
    let right = (a.x + a.width).max(b.x + b.width);
    let bottom = (a.y + a.height).max(b.y + b.height);
    RectF {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    }
}

/// Writes display items as SVG elements, the definitions they refer to apart.
struct Writer<'a> {
    id: &'a str,
    defs: String,
    count: usize,
    image_bytes: usize,
}

impl Writer<'_> {
    /// A new id for a definition.
    fn next_id(&mut self) -> String {
        self.count += 1;
        format!("{}-{}", self.id, self.count)
    }

    /// The elements that draw what the items of `mask` draw; `None` where they show images too
    /// large to write.
    fn mask(&mut self, mask: &MaskPaint, region: RectF) -> Option<String> {
        let mut out = String::new();
        // The elements opened by pushed clips and masks, closed by their pops.
        let mut open = 0_usize;
        for item in &mask.items {
            match item {
                Item::Rect { rect, color, .. } => rectangle(&mut out, *rect, &fill(*color)),
                Item::Image { rect, image } => self.image(&mut out, *rect, image)?,
                Item::Gradient { rect, gradient } => {
                    let paint = self.gradient(*rect, gradient);
                    rectangle(&mut out, *rect, &paint);
                }
                Item::Tiles {
                    area,
                    origin,
                    step,
                    tile,
                } => {
                    let pattern = self.next_id();
                    let mut inner = String::new();
                    match &**tile {
                        Item::Image { rect, image } => self.image(&mut inner, *rect, image)?,
                        Item::Gradient { rect, gradient } => {
                            let paint = self.gradient(*rect, gradient);
                            rectangle(&mut inner, *rect, &paint);
                        }
                        Item::Rect { rect, color, .. } => rectangle(&mut inner, *rect, &fill(*color)),
                        _ => {}
                    }
                    let _ = write!(
                        self.defs,
                        "<pattern id=\"{pattern}\" patternUnits=\"userSpaceOnUse\" x=\"{}\" y=\"{}\" \
                         width=\"{}\" height=\"{}\">{inner}</pattern>",
                        origin.0, origin.1, step.0, step.1
                    );
                    rectangle(&mut out, *area, &format!("fill=\"url(#{pattern})\""));
                }
                Item::PushClip(rect, _) => {
                    let clip = self.next_id();
                    let _ = write!(
                        self.defs,
                        "<clipPath id=\"{clip}\" clipPathUnits=\"userSpaceOnUse\">"
                    );
                    rectangle(&mut self.defs, *rect, "");
                    self.defs.push_str("</clipPath>");
                    let _ = write!(out, "<g clip-path=\"url(#{clip})\">");
                    open += 1;
                }
                Item::PushMask(inner) => {
                    let id = self.next_id();
                    let content = self.mask(inner, region)?;
                    let _ = write!(
                        self.defs,
                        "<mask id=\"{id}\" mask-type=\"{}\" maskUnits=\"userSpaceOnUse\" \
                         maskContentUnits=\"userSpaceOnUse\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\">{content}</mask>",
                        if inner.luminance { "luminance" } else { "alpha" },
                        region.x,
                        region.y,
                        region.width,
                        region.height
                    );
                    let _ = write!(out, "<g mask=\"url(#{id})\">");
                    open += 1;
                }
                Item::PushClipShape(_) => {
                    out.push_str("<g>");
                    open += 1;
                }
                Item::Pop if open > 0 => {
                    out.push_str("</g>");
                    open -= 1;
                }
                _ => {}
            }
        }
        for _ in 0..open {
            out.push_str("</g>");
        }
        Some(out)
    }

    /// Writes an `<image>` that shows `image` stretched over `rect`; `None` once the images of
    /// the mask grow too large.
    fn image(&mut self, out: &mut String, rect: RectF, image: &fagus_layout::Image) -> Option<()> {
        self.image_bytes += image.data.len();
        if self.image_bytes > MAX_IMAGE_BYTES {
            return None;
        }
        // usvg tells raster formats apart by their bytes where the type is `text/plain`.
        let mime = if image.format == ImageFormat::Svg {
            "image/svg+xml"
        } else {
            "text/plain"
        };
        let _ = write!(
            out,
            "<image x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\" href=\"data:{mime};base64,",
            rect.x, rect.y, rect.width, rect.height
        );
        base64_into(&image.data, out);
        out.push_str("\"/>");
        Some(())
    }

    /// The paint (a `fill` attribute) of `gradient` over `rect`, its definition written.
    fn gradient(&mut self, rect: RectF, gradient: &GradientPaint) -> String {
        let id = self.next_id();
        let stops = |stops: &[(f32, Rgba)]| {
            let mut out = String::new();
            for (offset, color) in stops {
                let _ = write!(
                    out,
                    "<stop offset=\"{}\" stop-color=\"{}\" stop-opacity=\"{}\"/>",
                    offset.clamp(0.0, 1.0),
                    rgb(*color),
                    color.a.clamp(0.0, 1.0)
                );
            }
            out
        };
        match gradient {
            GradientPaint::Solid(color) => return fill(*color),
            GradientPaint::Linear {
                start,
                end,
                stops: colors,
                repeat,
            } => {
                let _ = write!(
                    self.defs,
                    "<linearGradient id=\"{id}\" gradientUnits=\"userSpaceOnUse\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"{}>{}</linearGradient>",
                    rect.x + start.0,
                    rect.y + start.1,
                    rect.x + end.0,
                    rect.y + end.1,
                    if *repeat { " spreadMethod=\"repeat\"" } else { "" },
                    stops(colors)
                );
            }
            GradientPaint::Radial {
                center,
                radius,
                scale_y,
                stops: colors,
            } => {
                // Circles around the origin, moved to the center and stretched into ellipses.
                let _ = write!(
                    self.defs,
                    "<radialGradient id=\"{id}\" gradientUnits=\"userSpaceOnUse\" cx=\"0\" cy=\"0\" r=\"{radius}\" \
                     gradientTransform=\"matrix(1 0 0 {scale_y} {} {})\">{}</radialGradient>",
                    rect.x + center.0,
                    rect.y + center.1,
                    stops(colors)
                );
            }
            // SVG has no conic gradients: their average color.
            GradientPaint::Conic { stops: colors, .. } => {
                let count = colors.len().max(1) as f32;
                let sum = colors.iter().fold(Rgba::TRANSPARENT, |sum, (_, color)| Rgba {
                    r: sum.r + color.r * color.a,
                    g: sum.g + color.g * color.a,
                    b: sum.b + color.b * color.a,
                    a: sum.a + color.a,
                });
                let alpha = sum.a / count;
                let color = if sum.a > 0.0 {
                    Rgba {
                        r: sum.r / sum.a,
                        g: sum.g / sum.a,
                        b: sum.b / sum.a,
                        a: alpha,
                    }
                } else {
                    Rgba::TRANSPARENT
                };
                return fill(color);
            }
        }
        format!("fill=\"url(#{id})\"")
    }
}

/// Writes a `<rect>` over `rect` with the attributes `paint`.
fn rectangle(out: &mut String, rect: RectF, paint: &str) {
    let _ = write!(
        out,
        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" {paint}/>",
        rect.x,
        rect.y,
        rect.width.max(0.0),
        rect.height.max(0.0)
    );
}

/// The `fill` and `fill-opacity` attributes of `color`.
fn fill(color: Rgba) -> String {
    format!(
        "fill=\"{}\" fill-opacity=\"{}\"",
        rgb(color),
        color.a.clamp(0.0, 1.0)
    )
}

/// `color` as `rgb()` of 8-bit channels.
fn rgb(color: Rgba) -> String {
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round();
    format!(
        "rgb({},{},{})",
        channel(color.r),
        channel(color.g),
        channel(color.b)
    )
}

/// Appends `data` in Base64 (RFC 4648, §4, with padding) to `out`.
fn base64_into(data: &[u8], out: &mut String) {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    out.reserve(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let bytes = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let value = (u32::from(bytes[0]) << 16) | (u32::from(bytes[1]) << 8) | u32::from(bytes[2]);
        for index in 0..4 {
            if index <= chunk.len() {
                out.push(char::from(ALPHABET[((value >> (18 - 6 * index)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
}

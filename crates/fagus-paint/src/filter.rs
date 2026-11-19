//! `filter` (Filter Effects 1): what a stacking context draws, filtered. Color filters map the
//! colors of every drawing operation and the pixels of raster images; `opacity()` makes a group;
//! `drop-shadow()` draws a copy of everything below it, moved, in the shadow's color and
//! blurred; `blur()` blurs boxes, borders, text and images. Chrome prints filtered content as
//! an image; Fagus keeps it vector where it can. SVG images are drawn unfiltered. SVG filters
//! whose primitives these cannot express make the box an image as in Chrome (see
//! [`Item::Filtered`]).

use std::sync::Arc;

use fagus_layout::{ImageFormat, PixelFilter};
use fagus_style::values::{BorderStyle, Filter, Rgba};

use crate::{Artifact, BoxShadowPaint, FilteredPaint, GradientPaint, Item, PixelFilterStep, RectF, Tag};

/// Applies `filters` in order to the items from `start` on, which a stacking context in the
/// color `current` (what `currentColor` stands for) drew; `tagged` says whether the items have
/// the tags of a tagged PDF.
pub(crate) fn apply(items: &mut Vec<Item>, start: usize, filters: &[Filter], current: Rgba, tagged: bool) {
    for filter in filters {
        match filter {
            Filter::Opacity(amount) => {
                if *amount < 1.0 {
                    items.insert(start, Item::PushOpacity(amount.max(0.0)));
                    items.push(Item::Pop);
                }
            }
            Filter::DropShadow { x, y, blur, color } => {
                let color = color.unwrap_or(current);
                if color.is_transparent() {
                    continue;
                }
                // The tag in effect where the shadow goes in, for the items after it.
                let tag = tagged.then(|| {
                    items[..start]
                        .iter()
                        .rev()
                        .find_map(|item| match item {
                            Item::Tag(tag) => Some(*tag),
                            _ => None,
                        })
                        .unwrap_or(Tag::Artifact(Artifact::Layout))
                });
                let shadow = drop_shadow(&items[start..], tag, (*x, *y), *blur, color);
                items.splice(start..start, shadow);
            }
            Filter::Blur(sigma) => {
                if *sigma > 0.0 {
                    let mut blurred = items.split_off(start);
                    unclip(&mut blurred, *sigma);
                    for item in &mut blurred {
                        blur(item, *sigma);
                    }
                    items.append(&mut blurred);
                }
            }
            color => {
                for item in &mut items[start..] {
                    filter_colors(item, color);
                }
            }
        }
    }
}

/// Turns the items from `start` on, which a box with the border box `bounds` drew, into one
/// item that draws them as pixels through `steps` (SVG filters whose primitives CSS cannot
/// express, and the functions after them). Their tags go: the item is the content of the
/// first element inside it, as a group is in a tagged PDF, and the last tag inside goes on
/// after it. A box that draws nothing (an empty or hidden one) is filtered too: primitives such
/// as `feFlood` and `feTurbulence` draw without it, as in Chrome.
pub(crate) fn as_pixels(items: &mut Vec<Item>, start: usize, bounds: RectF, steps: Vec<PixelFilterStep>) {
    if steps.is_empty() || start > items.len() {
        return;
    }
    let mut drawn = items.split_off(start);
    let content = drawn.iter().find_map(|item| match item {
        Item::Tag(tag @ Tag::Content { .. }) => Some(*tag),
        _ => None,
    });
    let last = drawn.iter().rev().find_map(|item| match item {
        Item::Tag(tag) => Some(*tag),
        _ => None,
    });
    drawn.retain(|item| !matches!(item, Item::Tag(_)));
    if let Some(content) = content {
        items.push(Item::Tag(content));
    }
    items.push(Item::Filtered(Arc::new(FilteredPaint {
        items: drawn,
        bounds,
        filters: steps,
    })));
    if let Some(last) = last
        && Some(last) != content
    {
        items.push(Item::Tag(last));
    }
}

/// Applies a color filter to a drawing operation.
fn filter_colors(item: &mut Item, color: &Filter) {
    match item {
        Item::Image { image, rect } => {
            rasterized(image, *rect);
            filter_pixels(image, PixelFilter::Color(color.clone()));
        }
        // Filters clamp what they map, so the colors between two stops are not those between
        // the filtered stops: more stops, each filtered, follow them.
        Item::Gradient { gradient, .. } => {
            *gradient = Arc::new(subdivided(gradient));
            recolor(item, &|value| color.apply(value));
        }
        Item::Tiles { tile, .. } => filter_colors(Arc::make_mut(tile), color),
        _ => recolor(item, &|value| color.apply(value)),
    }
}

/// Maps the colors of a drawing operation other than an image.
fn recolor(item: &mut Item, map: &dyn Fn(Rgba) -> Rgba) {
    match item {
        Item::Rect { color, .. }
        | Item::BorderSide { color, .. }
        | Item::RoundedBorder { color, .. }
        | Item::DashedBorder { color, .. }
        | Item::Text { color, .. }
        | Item::Wave { color, .. }
        | Item::DashedLine { color, .. }
        | Item::TextShadow { color, .. } => *color = map(*color),
        Item::BoxShadow(shadow) => {
            *shadow = Arc::new(BoxShadowPaint {
                color: map(shadow.color),
                ..(**shadow).clone()
            });
        }
        Item::Gradient { gradient, .. } => {
            let stops = |stops: &[(f32, Rgba)]| -> Vec<(f32, Rgba)> {
                stops
                    .iter()
                    .map(|(offset, color)| (*offset, map(*color)))
                    .collect()
            };
            *gradient = Arc::new(match &**gradient {
                GradientPaint::Solid(color) => GradientPaint::Solid(map(*color)),
                GradientPaint::Linear {
                    start,
                    end,
                    stops: list,
                    repeat,
                } => GradientPaint::Linear {
                    start: *start,
                    end: *end,
                    stops: stops(list),
                    repeat: *repeat,
                },
                GradientPaint::Radial {
                    center,
                    radius,
                    scale_y,
                    stops: list,
                } => GradientPaint::Radial {
                    center: *center,
                    radius: *radius,
                    scale_y: *scale_y,
                    stops: stops(list),
                },
                GradientPaint::Conic {
                    center,
                    start_angle,
                    stops: list,
                } => GradientPaint::Conic {
                    center: *center,
                    start_angle: *start_angle,
                    stops: stops(list),
                },
            });
        }
        _ => {}
    }
}

/// The copy of `items` that a drop shadow is: moved by `offset`, blurred with a standard
/// deviation of `sigma`, in `color`. The copies are opaque in a group with the color's alpha,
/// so that where they overlap, the shadow is as dark as elsewhere. It is an artifact, after
/// which `tag` (in a tagged PDF) goes on.
fn drop_shadow(items: &[Item], tag: Option<Tag>, offset: (f32, f32), sigma: f32, color: Rgba) -> Vec<Item> {
    let opaque = Rgba { a: 1.0, ..color };
    let mut shadow = Vec::new();
    if tag.is_some() {
        shadow.push(Item::Tag(Tag::Artifact(Artifact::Layout)));
    }
    shadow.push(Item::PushTransform([1.0, 0.0, 0.0, 1.0, offset.0, offset.1]));
    let group = color.a < 1.0;
    if group {
        shadow.push(Item::PushOpacity(color.a));
    }
    let mut copies: Vec<Item> = items
        .iter()
        .filter(|item| !matches!(item, Item::Tag(_)))
        .cloned()
        .collect();
    if sigma > 0.0 {
        unclip(&mut copies, sigma);
    }
    for mut copy in copies {
        match &mut copy {
            // A raster image casts the shadow of its shape: its alpha, in the color.
            Item::Image { image, rect } => {
                rasterized(image, *rect);
                filter_pixels(image, PixelFilter::Silhouette(opaque));
                if sigma > 0.0 {
                    blur(&mut copy, sigma);
                }
            }
            Item::Gradient { rect, gradient } if sigma > 0.0 && is_opaque(gradient) => {
                copy = Item::Rect {
                    rect: *rect,
                    color: opaque,
                    radii: None,
                };
                blur(&mut copy, sigma);
            }
            other => {
                recolor(other, &|own| Rgba { a: own.a, ..opaque });
                blur(other, sigma);
            }
        }
        shadow.push(copy);
    }
    if group {
        shadow.push(Item::Pop);
    }
    shadow.push(Item::Pop);
    if let Some(tag) = tag {
        shadow.push(Item::Tag(tag));
    }
    shadow
}

/// Makes room for a blur with a standard deviation of `sigma` where items are clipped, as the
/// blur of what is clipped spreads beyond the clip: an image that its box clips (with
/// `object-fit` or rounded corners) is cropped instead, other clips are widened by as far as
/// the blur reaches.
fn unclip(items: &mut Vec<Item>, sigma: f32) {
    crop_to_clips(items);
    let reach = 3.0 * sigma;
    let mut index = 0;
    while index < items.len() {
        if let Item::PushClip(clip, radii) = items[index] {
            // The clip around an image as painted: the clip, the image's tag, the image.
            let image = (index + 1..items.len()).find(|at| !matches!(items[*at], Item::Tag(_)));
            if let Some(at) = image
                && matches!(items.get(at + 1), Some(Item::Pop))
                && let Item::Image { rect, image } = &mut items[at]
                && rect.width > 0.0
                && rect.height > 0.0
            {
                let bounds = [
                    (clip.x - rect.x) / rect.width,
                    (clip.y - rect.y) / rect.height,
                    (clip.x + clip.width - rect.x) / rect.width,
                    (clip.y + clip.height - rect.y) / rect.height,
                ];
                let radii = radii.map(|radii| radii.map(|(x, y)| (x / rect.width, y / rect.height)));
                rasterized(image, *rect);
                filter_pixels(image, PixelFilter::Crop { bounds, radii });
                items.remove(at + 1);
                items.remove(index);
                continue;
            }
        }
        if let Item::PushClip(clip, radii) = &mut items[index] {
            *clip = RectF {
                x: clip.x - reach,
                y: clip.y - reach,
                width: clip.width + 2.0 * reach,
                height: clip.height + 2.0 * reach,
            };
            if let Some(radii) = radii {
                for radius in radii.iter_mut() {
                    *radius = (radius.0 + reach, radius.1 + reach);
                }
            }
        }
        index += 1;
    }
}

/// Cuts the boxes (rectangles without rounded corners) that rectangular clips around them cut,
/// to what shows of them: a blur then spreads from the part that shows, as Chrome blurs what a
/// box draws clipped (an overflow clip on a child of a blurred box), not from the part that the
/// widened clip would let show.
fn crop_to_clips(items: &mut [Item]) {
    // The clip in effect before each pushed item; `None` where it is not a rectangle in the
    // items' coordinates (a rounded or shaped clip, or a transform, came in between).
    let mut clips: Vec<Option<RectF>> = Vec::new();
    let mut current: Option<RectF> = Some(RectF {
        x: f32::MIN / 4.0,
        y: f32::MIN / 4.0,
        width: f32::MAX / 2.0,
        height: f32::MAX / 2.0,
    });
    for item in items.iter_mut() {
        match item {
            Item::PushClip(clip, None) => {
                clips.push(current);
                current = current.map(|outer| intersection(outer, *clip));
            }
            Item::PushClip(..) | Item::PushClipShape(_) | Item::PushTransform(_) => {
                clips.push(current);
                current = None;
            }
            Item::PushOpacity(_)
            | Item::PushBlend(_)
            | Item::PushIsolated
            | Item::PushMask(_)
            | Item::PushLayer(_) => {
                clips.push(current);
            }
            Item::Pop => current = clips.pop().unwrap_or(current),
            Item::Rect {
                rect, radii: None, ..
            } => {
                if let Some(clip) = current {
                    *rect = intersection(*rect, clip);
                }
            }
            _ => {}
        }
    }
}

/// The part two rectangles share (empty where they do not meet).
fn intersection(a: RectF, b: RectF) -> RectF {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = (a.x + a.width).min(b.x + b.width);
    let bottom = (a.y + a.height).min(b.y + b.height);
    RectF {
        x,
        y,
        width: (right - x).max(0.0),
        height: (bottom - y).max(0.0),
    }
}

/// A gradient with fifteen more stops between each two, at the colors between them.
fn subdivided(gradient: &GradientPaint) -> GradientPaint {
    const STEPS: usize = 16;
    let more = |stops: &[(f32, Rgba)]| -> Vec<(f32, Rgba)> {
        let mut more = Vec::with_capacity(stops.len() * STEPS);
        for pair in stops.windows(2) {
            let ((from, start), (to, end)) = (pair[0], pair[1]);
            for step in 0..STEPS {
                let t = step as f32 / STEPS as f32;
                let mix = |a: f32, b: f32| a + (b - a) * t;
                more.push((
                    mix(from, to),
                    Rgba {
                        r: mix(start.r, end.r),
                        g: mix(start.g, end.g),
                        b: mix(start.b, end.b),
                        a: mix(start.a, end.a),
                    },
                ));
            }
        }
        more.extend(stops.last().copied());
        more
    };
    match gradient {
        GradientPaint::Solid(color) => GradientPaint::Solid(*color),
        GradientPaint::Linear {
            start,
            end,
            stops,
            repeat,
        } => GradientPaint::Linear {
            start: *start,
            end: *end,
            stops: more(stops),
            repeat: *repeat,
        },
        GradientPaint::Radial {
            center,
            radius,
            scale_y,
            stops,
        } => GradientPaint::Radial {
            center: *center,
            radius: *radius,
            scale_y: *scale_y,
            stops: more(stops),
        },
        GradientPaint::Conic {
            center,
            start_angle,
            stops,
        } => GradientPaint::Conic {
            center: *center,
            start_angle: *start_angle,
            stops: more(stops),
        },
    }
}

/// Whether a gradient covers its tile: all its colors are opaque.
fn is_opaque(gradient: &GradientPaint) -> bool {
    match gradient {
        GradientPaint::Solid(color) => color.a >= 1.0,
        GradientPaint::Linear { stops, .. }
        | GradientPaint::Radial { stops, .. }
        | GradientPaint::Conic { stops, .. } => stops.iter().all(|(_, color)| color.a >= 1.0),
    }
}

/// Blurs a drawing operation with a standard deviation of `sigma`: boxes and solid borders
/// become blurred box shadows of themselves, text blurred text shadows, and images go through
/// a blur of their pixels; gradients, dashed borders and wavy lines stay sharp.
fn blur(item: &mut Item, sigma: f32) {
    if sigma <= 0.0 {
        return;
    }
    // Box and text shadows take a blur radius, twice the standard deviation.
    let radius = 2.0 * sigma;
    let shadow = |shape: RectF, shape_radii, hole, color| {
        Item::BoxShadow(Arc::new(BoxShadowPaint {
            // Outer shadows show outside their area only: an empty one hides nothing.
            area: RectF {
                width: 0.0,
                height: 0.0,
                ..shape
            },
            area_radii: None,
            shape,
            shape_radii,
            hole,
            color,
            blur: radius,
            inset: false,
            outlines: None,
        }))
    };
    match item {
        Item::Rect { rect, color, radii } => *item = shadow(*rect, *radii, None, *color),
        // A side of a border covers the strip along its edge, give or take the corners.
        Item::BorderSide {
            points, color, style, ..
        } if *style == BorderStyle::Solid => {
            let (mut left, mut top, mut right, mut bottom) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for (x, y) in *points {
                (left, top, right, bottom) = (left.min(x), top.min(y), right.max(x), bottom.max(y));
            }
            let strip = RectF {
                x: left,
                y: top,
                width: right - left,
                height: bottom - top,
            };
            *item = shadow(strip, None, None, *color);
        }
        Item::RoundedBorder {
            outer,
            outer_radii,
            inner,
            inner_radii,
            color,
            ..
        }
        | Item::DashedBorder {
            outer,
            outer_radii,
            inner,
            inner_radii,
            color,
            ..
        } => {
            *item = shadow(
                *outer,
                Some(*outer_radii),
                Some((*inner, Some(*inner_radii))),
                *color,
            );
        }
        Item::BoxShadow(shadow) if !shadow.inset => {
            *shadow = Arc::new(BoxShadowPaint {
                blur: libm::hypotf(shadow.blur, radius),
                ..(**shadow).clone()
            });
        }
        Item::TextShadow { blur, .. } => *blur = libm::hypotf(*blur, radius),
        Item::Text {
            x,
            y,
            face,
            size,
            variations,
            color,
            glyphs,
            ..
        } => {
            *item = Item::TextShadow {
                x: *x,
                y: *y,
                face: *face,
                size: *size,
                variations: std::mem::take(variations),
                glyphs: std::mem::take(glyphs),
                lines: Vec::new(),
                color: *color,
                blur: radius,
            };
        }
        Item::Image { rect, image } => {
            rasterized(image, *rect);
            // In pixels of the image, which are as many as it has across its box, with room
            // around it for what the blur spreads.
            let (columns, rows) = (
                image.width + 2.0 * image.pad as f32,
                image.height + 2.0 * image.pad as f32,
            );
            if rect.width <= 0.0 || rect.height <= 0.0 || columns <= 0.0 || rows <= 0.0 {
                return;
            }
            let (scale_x, scale_y) = (columns / rect.width, rows / rect.height);
            let pixels = sigma * (scale_x * scale_y).sqrt();
            let room = (3.0 * pixels).ceil();
            // An image that would be padded to more pixels than are blurred is not blurred (see
            // `fagus_pdf`): nor padded, which took gigabytes for a large blur of a small image.
            if (columns + 2.0 * room) * (rows + 2.0 * room) > MAX_BLURRED {
                return;
            }
            *rect = RectF {
                x: rect.x - room / scale_x,
                y: rect.y - room / scale_y,
                width: rect.width + 2.0 * room / scale_x,
                height: rect.height + 2.0 * room / scale_y,
            };
            filter_pixels(image, PixelFilter::Blur(pixels));
            Arc::make_mut(image).pad += room as u32;
        }
        _ => {}
    }
}

/// The most pixels an image is blurred with, its padding included, as `fagus_pdf` blurs them.
const MAX_BLURRED: f32 = 25_000_000.0;

/// How many pixels across a CSS pixel an SVG image that is filtered is drawn in: as Chrome
/// prints filtered content, at 300 dpi.
const SVG_PIXELS: f32 = 300.0 / 96.0;

/// The most pixels across a side of a filtered SVG image.
const MAX_SVG_PIXELS: f32 = 4096.0;

/// Makes an SVG image drawn in `rect` ready for filters of its pixels, before the first: it is
/// drawn as an image of the pixels it covers at [`SVG_PIXELS`], its size their number.
fn rasterized(image: &mut Arc<fagus_layout::Image>, rect: RectF) {
    if image.format != ImageFormat::Svg || !image.filters.is_empty() || image.pad > 0 {
        return;
    }
    let longest = rect.width.max(rect.height);
    if !(longest > 0.0 && longest.is_finite()) {
        return;
    }
    let scale = SVG_PIXELS.min(MAX_SVG_PIXELS / longest);
    let mut pixels = (**image).clone();
    pixels.width = (rect.width * scale).round().max(1.0);
    pixels.height = (rect.height * scale).round().max(1.0);
    *image = Arc::new(pixels);
}

/// Adds `filter` to what the pixels of a raster image go through.
fn filter_pixels(image: &mut Arc<fagus_layout::Image>, filter: PixelFilter) {
    let mut filtered = (**image).clone();
    filtered.filters = image.filters.iter().cloned().chain([filter]).collect();
    *image = Arc::new(filtered);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, width: f32, height: f32) -> RectF {
        RectF { x, y, width, height }
    }

    #[test]
    fn blurs_what_a_clip_shows_of_a_box() {
        // A box half outside the overflow clip of its parent spreads its blur from the half
        // that shows, not from all of it once the clip is widened for the blur.
        let green = Rgba::from_u8(0, 128, 0, 1.0);
        let mut items = vec![
            Item::PushClip(rect(0.0, 0.0, 100.0, 100.0), None),
            Item::Rect {
                rect: rect(50.0, 0.0, 100.0, 100.0),
                color: green,
                radii: None,
            },
            Item::Pop,
        ];
        apply(&mut items, 0, &[Filter::Blur(10.0)], green, false);
        let Some(Item::BoxShadow(shadow)) = items.get(1) else {
            panic!("a blurred box: {items:?}");
        };
        assert_eq!(shadow.shape, rect(50.0, 0.0, 50.0, 100.0));
        // A transform between them leaves it as it is.
        let mut items = vec![
            Item::PushClip(rect(0.0, 0.0, 100.0, 100.0), None),
            Item::PushTransform([2.0, 0.0, 0.0, 2.0, 0.0, 0.0]),
            Item::Rect {
                rect: rect(50.0, 0.0, 100.0, 100.0),
                color: green,
                radii: None,
            },
            Item::Pop,
            Item::Pop,
        ];
        crop_to_clips(&mut items);
        assert!(matches!(items[2], Item::Rect { rect: r, .. } if r == rect(50.0, 0.0, 100.0, 100.0)));
    }

    #[test]
    fn filters_what_a_stacking_context_draws() {
        let red = Rgba::from_u8(255, 0, 0, 1.0);
        let box_item = Item::Rect {
            rect: rect(0.0, 0.0, 10.0, 10.0),
            color: red,
            radii: None,
        };
        // Color filters map colors; `opacity()` makes a group.
        let mut items = vec![Item::PushClip(rect(0.0, 0.0, 50.0, 50.0), None), box_item.clone()];
        apply(
            &mut items,
            1,
            &[Filter::Invert(1.0), Filter::Opacity(0.5)],
            red,
            false,
        );
        assert_eq!(
            items,
            [
                Item::PushClip(rect(0.0, 0.0, 50.0, 50.0), None),
                Item::PushOpacity(0.5),
                Item::Rect {
                    rect: rect(0.0, 0.0, 10.0, 10.0),
                    color: Rgba::from_u8(0, 255, 255, 1.0),
                    radii: None,
                },
                Item::Pop,
            ]
        );

        // A drop shadow is an opaque copy, moved and blurred, in a group with the shadow's
        // alpha, below what casts it; in a tagged PDF an artifact.
        let mut items = vec![Item::Tag(Tag::Artifact(Artifact::Layout)), box_item.clone()];
        let shadow = Filter::DropShadow {
            x: 2.0,
            y: 3.0,
            blur: 4.0,
            color: Some(Rgba::from_u8(0, 0, 255, 0.5)),
        };
        apply(&mut items, 1, &[shadow], red, true);
        let Item::BoxShadow(blurred) = &items[4] else {
            panic!("a blurred box: {items:?}");
        };
        assert_eq!((blurred.shape, blurred.blur), (rect(0.0, 0.0, 10.0, 10.0), 8.0));
        assert_eq!(blurred.color, Rgba::from_u8(0, 0, 255, 1.0));
        assert_eq!(items[2], Item::PushTransform([1.0, 0.0, 0.0, 1.0, 2.0, 3.0]));
        assert_eq!(items[3], Item::PushOpacity(0.5));
        assert_eq!(
            items[5..],
            [
                Item::Pop,
                Item::Pop,
                Item::Tag(Tag::Artifact(Artifact::Layout)),
                box_item
            ]
        );

        // An image that its box clips casts the shadow of what is seen of it.
        let image = Arc::new(fagus_layout::Image {
            data: Arc::new(Vec::new()),
            format: ImageFormat::Png,
            width: 200.0,
            height: 100.0,
            natural: (true, true),
            ratio: Some(2.0),
            inline: false,
            orientation: 1,
            filters: Arc::from([]),
            pad: 0,
        });
        let mut items = vec![
            Item::PushClip(rect(50.0, 0.0, 100.0, 100.0), None),
            Item::Image {
                rect: rect(0.0, 0.0, 200.0, 100.0),
                image,
            },
            Item::Pop,
        ];
        let shadow = Filter::DropShadow {
            x: 0.0,
            y: 0.0,
            blur: 2.0,
            color: None,
        };
        apply(&mut items, 0, &[shadow, Filter::Grayscale(1.0)], red, false);
        let Item::Image { rect: shadow, image } = &items[1] else {
            panic!("the image's shadow: {items:?}");
        };
        // Grown by three standard deviations, its pixels cropped, made the color, and blurred.
        assert_eq!(*shadow, rect(-6.0, -6.0, 212.0, 112.0));
        assert_eq!(image.pad, 6);
        assert_eq!(
            image.filters[..],
            [
                PixelFilter::Crop {
                    bounds: [0.25, 0.0, 0.75, 1.0],
                    radii: None
                },
                PixelFilter::Silhouette(red),
                PixelFilter::Blur(2.0),
                PixelFilter::Color(Filter::Grayscale(1.0)),
            ]
        );
        // The image itself stays clipped.
        assert!(matches!(items[3], Item::PushClip(..)));

        // Blurred text is a blurred shadow of itself.
        let mut fonts = fagus_text::FontDb::new();
        fonts.add_default_fonts();
        let face = fonts
            .query(&["Inter"], 400.0, fagus_text::FontStyle::Normal)
            .expect("Inter");
        let mut text = vec![Item::Text {
            x: 0.0,
            y: 10.0,
            face,
            size: 12.0,
            variations: Vec::new(),
            color: red,
            glyphs: Vec::new(),
            text: "A".into(),
            synthetic: crate::Synthetic::default(),
            vertical: false,
            palette: None,
        }];
        apply(&mut text, 0, &[Filter::Blur(1.5)], red, false);
        assert!(matches!(text[0], Item::TextShadow { blur: 3.0, .. }));
    }

    #[test]
    fn draws_boxes_as_pixels_for_svg_filters() {
        use fagus_dom::NodeId;
        use fagus_layout::{Images, SvgReference};

        let mut images = Images::default();
        images.insert_reference(
            "#noise",
            Arc::new(SvgReference::PixelFilter {
                markup: Arc::from("<filter id=\"noise\"><feTurbulence/></filter>"),
            }),
        );
        let red = Rgba::from_u8(255, 0, 0, 1.0);
        let filters = [
            Filter::Grayscale(1.0),
            Filter::Url(Arc::from("#noise")),
            Filter::DropShadow {
                x: 1.0,
                y: 1.0,
                blur: 0.0,
                color: None,
            },
        ];
        // The SVG filter stays a URL, and the functions after it go with it, `currentColor`
        // resolved.
        let resolved = crate::reference::filters(&filters, &images);
        assert!(matches!(resolved[1], Filter::Url(_)));
        let steps = crate::reference::pixel_steps(&resolved[1..], red, &images, false);
        assert!(matches!(&steps[0], PixelFilterStep::Svg { id, .. } if &**id == "noise"));
        assert!(
            matches!(steps[1], PixelFilterStep::Function(Filter::DropShadow { color: Some(color), .. }) if color == red)
        );
        let content = Tag::Content {
            node: NodeId::DOCUMENT,
            order: 0,
            marker: false,
            call: false,
        };
        let mut items = vec![
            Item::Tag(Tag::Artifact(Artifact::Layout)),
            Item::Rect {
                rect: rect(0.0, 0.0, 10.0, 10.0),
                color: red,
                radii: None,
            },
            Item::Tag(content),
            Item::Rect {
                rect: rect(0.0, 0.0, 5.0, 5.0),
                color: red,
                radii: None,
            },
            Item::Tag(Tag::Artifact(Artifact::Layout)),
        ];
        as_pixels(&mut items, 0, rect(0.0, 0.0, 10.0, 10.0), steps);
        // One item, the content of the element inside it, without tags inside; the last tag
        // goes on after it.
        let [Item::Tag(first), Item::Filtered(filtered), Item::Tag(last)] = &items[..] else {
            panic!("a box drawn as pixels: {items:?}");
        };
        assert_eq!((*first, *last), (content, Tag::Artifact(Artifact::Layout)));
        assert_eq!(filtered.items.len(), 2);
        assert!(filtered.items.iter().all(|item| !matches!(item, Item::Tag(_))));
        // A box that draws nothing is filtered too: its filter may draw on its own.
        let mut empty = Vec::new();
        let steps = crate::reference::pixel_steps(&resolved[1..2], red, &images, false);
        as_pixels(&mut empty, 0, rect(0.0, 0.0, 0.0, 0.0), steps);
        assert!(
            matches!(&empty[..], [Item::Filtered(filtered)] if filtered.items.is_empty()),
            "{empty:?}"
        );
    }

    #[test]
    fn filters_svg_images_as_their_pixels() {
        let svg = Item::Image {
            rect: rect(0.0, 0.0, 48.0, 24.0),
            image: Arc::new(fagus_layout::Image {
                data: Arc::new(b"<svg xmlns='http://www.w3.org/2000/svg'/>".to_vec()),
                format: ImageFormat::Svg,
                width: 16.0,
                height: 8.0,
                natural: (true, true),
                ratio: Some(2.0),
                inline: false,
                orientation: 1,
                filters: Arc::from([]),
                pad: 0,
            }),
        };
        let mut items = vec![svg];
        apply(
            &mut items,
            0,
            &[Filter::Grayscale(1.0), Filter::Blur(2.0)],
            Rgba::BLACK,
            false,
        );
        let [Item::Image { rect: drawn, image }] = &items[..] else {
            panic!("an image: {items:?}");
        };
        // Drawn at 300 dpi, 150 × 75 pixels, then grayed and blurred, with room for the blur.
        assert_eq!((image.width, image.height), (150.0, 75.0));
        assert!(matches!(
            &image.filters[..],
            [PixelFilter::Color(Filter::Grayscale(_)), PixelFilter::Blur(_)]
        ));
        assert!(image.pad > 0 && drawn.width > 48.0);
    }
}

//! Drops what clipping hides entirely, as Chrome's PDF output drops it.
//!
//! Blink does not paint what lies outside the clip of the box it is in (its cull rect), and
//! Skia rejects each glyph whose box lies outside the bounds of the clip (the page, the page
//! area, `overflow: hidden` boxes, `clip-path`), or a glyph without outline (a space) whose
//! origin does: such text is not in the PDF at all, and a reader that extracts the text does
//! not find it. Glyphs and shapes that the clip cuts through stay. Nothing at all shows in an
//! empty clip (the `clip-path: inset(50%)` around the `MathML` of `KaTeX`), nor in a group
//! whose opacity is zero as a byte (see [`invisible_opacity`]), and a clip or group left with
//! nothing in it is not drawn either.

use crate::{ClipGeometry, Item, RectF};
use fagus_text::FontDb;

/// How far synthetic italics lean (as `fagus-pdf` slants them), for the box of a glyph.
const SLANT: f32 = 0.25;

/// A box in page coordinates: left, top, right, bottom.
type Bounds = [f32; 4];

/// An affine transform `[a, b, c, d, e, f]` as [`Item::PushTransform`] has it.
type Matrix = [f32; 6];

const IDENTITY: Matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// Drops the glyphs of the text in `items` that lie outside the clip around them, starting
/// from `page` (the page box, bleed included), the shapes that lie outside it, everything in
/// an empty clip, and then the clips and groups that hold nothing. Text in tiles and masks
/// stays as it is.
pub(crate) fn cull_hidden(items: &mut Vec<Item>, fonts: &FontDb, page: Bounds) {
    let mut stack: Vec<(Matrix, Option<Bounds>)> = Vec::new();
    let mut matrix = IDENTITY;
    let mut clip = Some(page);
    items.retain_mut(|item| {
        match item {
            Item::PushTransform(transform) => {
                stack.push((matrix, clip));
                matrix = multiply(matrix, *transform);
            }
            Item::PushClip(rect, _) => {
                stack.push((matrix, clip));
                let rect = transformed(matrix, rect_bounds(rect));
                clip = clip.and_then(|clip| intersection(clip, rect));
            }
            Item::PushClipShape(shape) => {
                stack.push((matrix, clip));
                if let Some(shape) = shape_bounds(shape) {
                    clip = clip.and_then(|clip| intersection(clip, transformed(matrix, shape)));
                }
            }
            Item::PushOpacity(opacity) => {
                stack.push((matrix, clip));
                if invisible_opacity(*opacity) {
                    clip = None;
                }
            }
            Item::PushBlend(_) | Item::PushIsolated | Item::PushMask(_) | Item::PushLayer(_) => {
                stack.push((matrix, clip));
            }
            Item::Pop => {
                if let Some((outer_matrix, outer_clip)) = stack.pop() {
                    (matrix, clip) = (outer_matrix, outer_clip);
                }
            }
            Item::Text { .. } => return keep_visible_glyphs(item, fonts, matrix, clip),
            Item::Tag(_) => {}
            _ => {
                let Some(clip) = clip else {
                    return false;
                };
                if is_colorless(item) {
                    return false;
                }
                return item_bounds(item).is_none_or(|bounds| {
                    // A pixel more for anti-aliasing and the ends of strokes.
                    let [left, top, right, bottom] = bounds;
                    let bounds = [left - 1.0, top - 1.0, right + 1.0, bottom + 1.0];
                    intersection(clip, transformed(matrix, bounds)).is_some()
                });
            }
        }
        true
    });
    drop_empty_groups(items);
}

/// Whether a shape is drawn in a color whose alpha is zero as a byte, which Skia does not
/// draw at all (a `border: 1px solid transparent` around a rounded box): text stays, as Chrome
/// keeps transparent text in its PDF.
fn is_colorless(item: &Item) -> bool {
    match item {
        Item::Rect { color, .. }
        | Item::BorderSide { color, .. }
        | Item::RoundedBorder { color, .. }
        | Item::DashedBorder { color, .. }
        | Item::Wave { color, .. }
        | Item::DashedLine { color, .. } => invisible_opacity(color.a),
        _ => false,
    }
}

/// The clip in effect after `items`, in page coordinates, starting from `page`: the box around
/// the clips still open at their end (`None` where they leave nothing). Where `skip` is the last
/// group still open, the clip around it instead: the own clip of a box pushed just before what
/// it holds, which does not cut the box itself.
pub(crate) fn open_clip(items: &[Item], page: Bounds, skip: Option<&Item>) -> Option<Bounds> {
    let mut stack: Vec<(Matrix, Option<Bounds>, usize)> = Vec::new();
    let mut matrix = IDENTITY;
    let mut clip = Some(page);
    for (index, item) in items.iter().enumerate() {
        match item {
            Item::PushTransform(transform) => {
                stack.push((matrix, clip, index));
                matrix = multiply(matrix, *transform);
            }
            Item::PushClip(rect, _) => {
                stack.push((matrix, clip, index));
                let rect = transformed(matrix, rect_bounds(rect));
                clip = clip.and_then(|clip| intersection(clip, rect));
            }
            Item::PushClipShape(shape) => {
                stack.push((matrix, clip, index));
                if let Some(shape) = shape_bounds(shape) {
                    clip = clip.and_then(|clip| intersection(clip, transformed(matrix, shape)));
                }
            }
            Item::PushOpacity(_)
            | Item::PushBlend(_)
            | Item::PushIsolated
            | Item::PushMask(_)
            | Item::PushLayer(_) => stack.push((matrix, clip, index)),
            Item::Pop => {
                if let Some((outer_matrix, outer_clip, _)) = stack.pop() {
                    (matrix, clip) = (outer_matrix, outer_clip);
                }
            }
            _ => {}
        }
    }
    match (stack.last(), skip) {
        (Some(&(_, outer, index)), Some(skip)) if items[index] == *skip => outer,
        _ => clip,
    }
}

/// Where two boxes (left, top, right, bottom) overlap, `None` if they do not.
pub(crate) fn overlap(a: Bounds, b: Bounds) -> Option<Bounds> {
    intersection(a, b)
}

/// Whether the box `outer` holds all of `inner`.
pub(crate) fn holds(outer: Bounds, inner: Bounds) -> bool {
    contains(outer, inner)
}

/// Whether a group of this opacity shows nothing at all: Skia keeps the alpha of a layer as a
/// byte, and Chrome draws nothing of a layer whose alpha rounds to 0 (opacity below 0.5 / 255),
/// neither its text nor its links.
pub fn invisible_opacity(opacity: f32) -> bool {
    (opacity.clamp(0.0, 1.0) * 255.0).round() <= 0.0
}

/// Removes the clips, transforms and groups that hold nothing that draws, keeping the last tag
/// in them so that what follows is what it was to a reader.
pub(crate) fn drop_empty_groups(items: &mut Vec<Item>) {
    let mut kept: Vec<Item> = Vec::with_capacity(items.len());
    let mut starts: Vec<usize> = Vec::new();
    for item in items.drain(..) {
        match item {
            Item::PushTransform(_)
            | Item::PushClip(..)
            | Item::PushClipShape(_)
            | Item::PushOpacity(_)
            | Item::PushBlend(_)
            | Item::PushIsolated
            | Item::PushLayer(_)
            | Item::PushMask(_) => {
                starts.push(kept.len());
                kept.push(item);
            }
            Item::Pop => {
                let Some(start) = starts.pop() else {
                    kept.push(item);
                    continue;
                };
                if kept[start + 1..].iter().all(|item| matches!(item, Item::Tag(_))) {
                    let tag = kept.drain(start..).rfind(|item| matches!(item, Item::Tag(_)));
                    kept.extend(tag);
                } else {
                    kept.push(item);
                }
            }
            _ => kept.push(item),
        }
    }
    *items = kept;
}

/// The box that a drawing item other than text paints into, roughly, or `None` where it is
/// not known (filtered boxes, SVG images, which may draw past their boxes).
fn item_bounds(item: &Item) -> Option<Bounds> {
    let points = |points: &[(f32, f32)], reach: f32| {
        let [left, top, right, bottom] = points.iter().fold(
            [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY],
            |[left, top, right, bottom], &(x, y)| [left.min(x), top.min(y), right.max(x), bottom.max(y)],
        );
        [left - reach, top - reach, right + reach, bottom + reach]
    };
    let grown = |rect: &RectF, reach: f32| {
        let [left, top, right, bottom] = rect_bounds(rect);
        [left - reach, top - reach, right + reach, bottom + reach]
    };
    let bounds = match item {
        Item::Rect { rect, .. } | Item::Gradient { rect, .. } => rect_bounds(rect),
        Item::Tiles { area, .. } => rect_bounds(area),
        Item::Wave { clip, .. } => rect_bounds(clip),
        Item::BorderSide {
            points: corners,
            width,
            ..
        } => points(corners, *width),
        Item::RoundedBorder { outer, stroke, .. } => grown(outer, stroke.unwrap_or(0.0)),
        Item::DashedBorder { outer, stroke, .. } => grown(outer, *stroke),
        Item::DashedLine {
            from,
            to,
            thickness,
            dash,
            ..
        } => points(&[*from, *to], thickness + dash),
        Item::Image { rect, image }
            if image.format != fagus_layout::ImageFormat::Svg
                && image.pad == 0
                && image.filters.is_empty() =>
        {
            rect_bounds(rect)
        }
        Item::BoxShadow(shadow) if shadow.inset => rect_bounds(&shadow.area),
        Item::BoxShadow(shadow) => grown(&shadow.shape, 2.0 * shadow.blur),
        _ => return None,
    };
    bounds.iter().all(|value| value.is_finite()).then_some(bounds)
}

/// The box around the shape of a `clip-path`, `None` where it is not known.
fn shape_bounds(shape: &ClipGeometry) -> Option<Bounds> {
    match shape {
        ClipGeometry::Ellipse { center, radii } => Some([
            center.0 - radii.0,
            center.1 - radii.1,
            center.0 + radii.0,
            center.1 + radii.1,
        ]),
        ClipGeometry::Polygon { points, .. } if !points.is_empty() => Some(points.iter().fold(
            [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY],
            |[left, top, right, bottom], &(x, y)| [left.min(x), top.min(y), right.max(x), bottom.max(y)],
        )),
        _ => None,
    }
}

/// Drops the glyphs of a text item that lie outside `clip` (in page coordinates, `None` where
/// nothing shows); whether any glyph is left.
fn keep_visible_glyphs(item: &mut Item, fonts: &FontDb, matrix: Matrix, clip: Option<Bounds>) -> bool {
    let Item::Text {
        x,
        y,
        face,
        size,
        glyphs,
        synthetic,
        vertical,
        ..
    } = item
    else {
        return true;
    };
    let Some(clip) = clip else {
        return false;
    };
    let size = *size * fonts.face(*face).size_adjust;
    // The run as a whole first, with room for glyphs that reach past their advances: most runs
    // lie well inside the clip, and some well outside.
    let length: f32 = glyphs.iter().map(|glyph| glyph.advance).sum();
    let reach = 2.0 * size;
    let run = if *vertical {
        [*x - reach, *y - reach, *x + reach, *y + length + reach]
    } else {
        [*x - reach, *y - reach, *x + length + reach, *y + reach]
    };
    let run = transformed(matrix, run);
    if contains(clip, run) {
        return true;
    }
    if intersection(clip, run).is_none() {
        return false;
    }
    let font = fonts.face(*face);
    let bold = if synthetic.bold { size / 24.0 } else { 0.0 };
    let mut pen = if *vertical { *y } else { *x };
    // Each glyph is drawn where it is, or its advance moves the pen of the glyph before it (or
    // the start of the run) on.
    let mut kept: Vec<fagus_layout::Glyph> = Vec::with_capacity(glyphs.len());
    let mut start = (*x, *y);
    for glyph in glyphs.iter() {
        let origin = if *vertical {
            (*x + glyph.x_offset, pen - glyph.y_offset)
        } else {
            (pen + glyph.x_offset, *y - glyph.y_offset)
        };
        pen += glyph.advance;
        let visible = if let Some([left, top, right, bottom]) = font.glyph_bounds(glyph.id, size) {
            let lean = if synthetic.italic { SLANT } else { 0.0 };
            let bounds = [
                origin.0 + left - lean * bottom.max(0.0) - bold,
                origin.1 + top - bold,
                origin.0 + right - lean * top.min(0.0) + bold,
                origin.1 + bottom + bold,
            ];
            intersection(clip, transformed(matrix, bounds)).is_some()
        } else {
            let [px, py, ..] = transformed(matrix, [origin.0, origin.1, origin.0, origin.1]);
            clip[0] <= px && px <= clip[2] && clip[1] <= py && py <= clip[3]
        };
        if visible {
            kept.push(*glyph);
        } else if let Some(previous) = kept.last_mut() {
            previous.advance += glyph.advance;
        } else if *vertical {
            start.1 += glyph.advance;
        } else {
            start.0 += glyph.advance;
        }
    }
    if kept.len() == glyphs.len() {
        return true;
    }
    (*x, *y) = start;
    *glyphs = kept;
    !glyphs.is_empty()
}

/// `outer` applied after `inner`.
#[allow(clippy::many_single_char_names, reason = "the entries of the matrices")]
fn multiply(outer: Matrix, inner: Matrix) -> Matrix {
    let [a, b, c, d, e, f] = outer;
    let [a2, b2, c2, d2, e2, f2] = inner;
    [
        a * a2 + c * b2,
        b * a2 + d * b2,
        a * c2 + c * d2,
        b * c2 + d * d2,
        a * e2 + c * f2 + e,
        b * e2 + d * f2 + f,
    ]
}

/// The box around a box transformed by `matrix`.
#[allow(clippy::many_single_char_names, reason = "the entries of the matrix")]
fn transformed(matrix: Matrix, [left, top, right, bottom]: Bounds) -> Bounds {
    if matrix == IDENTITY {
        return [left, top, right, bottom];
    }
    let [a, b, c, d, e, f] = matrix;
    let corners = [(left, top), (right, top), (left, bottom), (right, bottom)]
        .map(|(x, y)| (a * x + c * y + e, b * x + d * y + f));
    corners.iter().fold(
        [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY],
        |[left, top, right, bottom], &(x, y)| [left.min(x), top.min(y), right.max(x), bottom.max(y)],
    )
}

/// Where two boxes overlap, `None` if they do not (boxes that only touch do not).
fn intersection(a: Bounds, b: Bounds) -> Option<Bounds> {
    let overlap = [a[0].max(b[0]), a[1].max(b[1]), a[2].min(b[2]), a[3].min(b[3])];
    (overlap[0] < overlap[2] && overlap[1] < overlap[3]).then_some(overlap)
}

/// Whether `outer` holds all of `inner`.
fn contains(outer: Bounds, inner: Bounds) -> bool {
    outer[0] <= inner[0] && outer[1] <= inner[1] && inner[2] <= outer[2] && inner[3] <= outer[3]
}

/// The page box of a display list, with its bleed, for [`cull_hidden`].
pub(crate) fn page_bounds(width: f32, height: f32, bleed: f32) -> Bounds {
    [-bleed, -bleed, width + bleed, height + bleed]
}

/// The box of a rectangle.
fn rect_bounds(rect: &RectF) -> Bounds {
    [rect.x, rect.y, rect.x + rect.width, rect.y + rect.height]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Rgba, Synthetic};
    use fagus_layout::Glyph;

    /// A database with Inter, and the text `HHHH` in it at 10 pixels from `x` on the baseline 20,
    /// each glyph 10 pixels wide.
    fn text(x: f32) -> (FontDb, Item) {
        let mut fonts = FontDb::new();
        let face = fonts
            .add_font_data(
                std::fs::read(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../tests/fonts/Inter-Regular.ttf"
                ))
                .expect("font"),
            )
            .expect("faces")[0];
        let id = fonts.shape(face, "H").glyphs[0].id;
        let glyphs = (0..4u32)
            .map(|index| Glyph {
                id,
                advance: 10.0,
                x_offset: 0.0,
                y_offset: 0.0,
                cluster: (index, index + 1),
            })
            .collect();
        let item = Item::Text {
            x,
            y: 20.0,
            face,
            size: 10.0,
            variations: Vec::new(),
            color: Rgba {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
            glyphs,
            text: "HHHH".to_owned(),
            synthetic: Synthetic::default(),
            vertical: false,
            palette: None,
        };
        (fonts, item)
    }

    fn clip(x: f32, width: f32) -> Item {
        Item::PushClip(
            RectF {
                x,
                y: 0.0,
                width,
                height: 100.0,
            },
            None,
        )
    }

    #[test]
    fn drops_the_glyphs_outside_the_clip_and_keeps_those_it_cuts() {
        // The clip starts 25 pixels into the run: the third glyph's outline reaches into it.
        let (fonts, run) = text(0.0);
        let mut items = vec![clip(25.0, 100.0), run, Item::Pop];
        cull_hidden(&mut items, &fonts, page_bounds(200.0, 100.0, 0.0));
        let Item::Text { x, glyphs, .. } = &items[1] else {
            panic!("the run stays");
        };
        assert_eq!(glyphs.len(), 2);
        assert!((x - 20.0).abs() < 1e-4);
        assert_eq!(glyphs[0].cluster, (2, 3));
        // Glyphs after the clip's end are dropped too, their advances kept.
        let (fonts, run) = text(0.0);
        let mut items = vec![clip(0.0, 15.0), run, Item::Pop];
        cull_hidden(&mut items, &fonts, page_bounds(200.0, 100.0, 0.0));
        let Item::Text { glyphs, .. } = &items[1] else {
            panic!("the run stays");
        };
        assert_eq!(glyphs.len(), 2);
        assert!((glyphs[1].advance - 30.0).abs() < 1e-4);
    }

    #[test]
    fn drops_shapes_outside_the_clip_and_all_in_an_empty_one() {
        let (fonts, run) = text(0.0);
        let rect = |x: f32| Item::Rect {
            rect: RectF {
                x,
                y: 10.0,
                width: 10.0,
                height: 10.0,
            },
            color: Rgba {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
            radii: None,
        };
        let tag = Item::Tag(crate::Tag::Artifact(crate::Artifact::Layout));
        // A rectangle past the clip goes, one that it cuts stays.
        let mut items = vec![clip(0.0, 100.0), rect(150.0), rect(95.0), Item::Pop];
        cull_hidden(&mut items, &fonts, page_bounds(200.0, 100.0, 0.0));
        assert_eq!(items, [clip(0.0, 100.0), rect(95.0), Item::Pop]);
        // Nothing shows in an empty clip (KaTeX's `clip-path: inset(50%)`): the clip goes with
        // what it holds, the tag in it stays for what follows.
        let mut items = vec![
            clip(50.0, 0.0),
            tag.clone(),
            rect(50.0),
            run,
            Item::Pop,
            rect(10.0),
        ];
        cull_hidden(&mut items, &fonts, page_bounds(200.0, 100.0, 0.0));
        assert_eq!(items, [tag, rect(10.0)]);
    }

    #[test]
    fn drops_shapes_of_transparent_colors() {
        let (fonts, _) = text(0.0);
        let rect = |a: f32| Item::Rect {
            rect: RectF {
                x: 10.0,
                y: 10.0,
                width: 10.0,
                height: 10.0,
            },
            color: Rgba {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a,
            },
            radii: None,
        };
        let mut items = vec![rect(0.0), rect(0.001), rect(0.01)];
        cull_hidden(&mut items, &fonts, page_bounds(200.0, 100.0, 0.0));
        assert_eq!(items, [rect(0.01)]);
    }

    #[test]
    fn drops_all_in_a_group_whose_opacity_is_zero_as_a_byte() {
        // Chrome leaves out the `#` of a heading's anchor with `opacity: 0`, and anything below
        // an opacity of 0.5 / 255; a group just above that stays.
        let tag = Item::Tag(crate::Tag::Artifact(crate::Artifact::Layout));
        for opacity in [0.0, 0.0003, 0.0019] {
            let (fonts, run) = text(10.0);
            let mut items = vec![
                Item::PushOpacity(opacity),
                tag.clone(),
                run.clone(),
                Item::Pop,
                run,
            ];
            cull_hidden(&mut items, &fonts, page_bounds(200.0, 100.0, 0.0));
            assert!(
                matches!(&items[..], [Item::Tag(_), Item::Text { .. }]),
                "{opacity}"
            );
        }
        let (fonts, run) = text(10.0);
        let mut items = vec![Item::PushOpacity(0.002), run, Item::Pop];
        cull_hidden(&mut items, &fonts, page_bounds(200.0, 100.0, 0.0));
        assert_eq!(items.len(), 3);
        assert!(!invisible_opacity(0.002) && invisible_opacity(-1.0) && !invisible_opacity(f32::NAN));
    }

    #[test]
    fn drops_runs_outside_the_page_or_moved_out_of_the_clip() {
        let (fonts, run) = text(-300.0);
        let mut items = vec![run];
        cull_hidden(&mut items, &fonts, page_bounds(200.0, 100.0, 0.0));
        assert_eq!(items.len(), 0);
        // Inside the clip, but moved out of it by a transform.
        let (fonts, run) = text(10.0);
        let mut items = vec![
            clip(0.0, 100.0),
            Item::PushTransform([1.0, 0.0, 0.0, 1.0, 150.0, 0.0]),
            run,
            Item::Pop,
            Item::Pop,
        ];
        cull_hidden(&mut items, &fonts, page_bounds(200.0, 100.0, 0.0));
        assert!(!items.iter().any(|item| matches!(item, Item::Text { .. })));
        // After the clip ends, text shows again.
        let (fonts, run) = text(150.0);
        let mut items = vec![clip(0.0, 100.0), Item::Pop, run];
        cull_hidden(&mut items, &fonts, page_bounds(200.0, 100.0, 0.0));
        // The clip, empty, goes.
        assert!(matches!(&items[..], [Item::Text { glyphs, .. }] if glyphs.len() == 4));
    }
}

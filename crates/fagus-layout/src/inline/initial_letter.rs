//! Initial letters (`initial-letter`, CSS Inline 3, §7) as Chrome 157 sizes and places them.
//!
//! The letter is set at the font size whose cap height reaches from the cap height of the first
//! line to the baseline of line `size` (Blink's `ComputeInitialLetterFont`). Its box is as large
//! as the ink of its glyphs and their border and padding (`AdjustInitialLetterInTextPosition`,
//! `CalculateInitialLetterBoxInlineSize`), and stands at the start of the first line, below the
//! text indent, at the block offset that `ComputeInitialLetterBoxBlockOffset` computes: its
//! baseline `size` lines down less the descent of the paragraph's lines, or, sunk deeper than
//! its size, its bottom at the end of line `sink`. The lines beside it, its margin box from the
//! top of the first line down, are shortened like those beside a float.
//!
//! Ported from Chromium's `initial_letter_utils.cc` and `style_resolver.cc` (BSD license, see
//! `CHROMIUM-LICENSE`).

use std::sync::Arc;

use fagus_style::{ComputedStyle, ComputedValues};

use super::{
    InlineItem, Layouter, PieceKind, TextLevels, face_for, face_metrics, font_variations, metrics,
    shape_text, used_font_size,
};
use crate::breaks::own_breaks;
use crate::fragment::{Children, Fragment, FragmentKind, GlyphOrientation, TextRun, VisibleSides};
use crate::tree::BoxId;
use crate::units::{Au, Rect};

/// The most pixels [`letter_font_size`] takes off the size it computes.
const MAX_STEPS: usize = 64;

/// The font size at which an initial letter of style `letter` spans `size` lines of a
/// paragraph whose lines are `line_height` apart, with a cap height of `paragraph_cap` there:
/// the size at which the letter's cap height is `(size - 1)` line heights plus that cap height,
/// made smaller by whole pixels while the cap height at it is still larger (Blink's
/// `ComputeInitialLetterFont`). `letter_cap` is the letter font's cap height per pixel of font
/// size; `(computed, used)` are the letter's own font size and the size its font is set at (see
/// [`used_font_size`](super::used_font_size)). As in Blink, the size scales the computed size
/// by the cap height wanted over the cap height of the font, and the cap height of each size
/// tried is that of the font at it, cut to hundredths of a pixel: so a letter in text of 10pt
/// (13.333 pixels, set at 13.33) comes out a hair too large and one pixel smaller. `None` where
/// the font has no cap height.
pub(super) fn letter_font_size(
    size: f32,
    (line_height, paragraph_cap): (f32, f32),
    letter_cap: f32,
    (computed, used): (f32, f32),
) -> Option<f32> {
    let cap_at = |font_size: f32| letter_cap * (font_size * 100.0).trunc() / 100.0;
    if letter_cap <= 0.0 || !letter_cap.is_finite() || used <= 0.0 || computed <= 0.0 {
        return None;
    }
    let desired = line_height * (size - 1.0) + paragraph_cap;
    let mut font_size = desired * computed / (letter_cap * used);
    // Float division may land a hair off a whole size the metrics give exactly.
    if (font_size - font_size.round()).abs() < 1e-3 {
        font_size = font_size.round();
    }
    let exact = font_size;
    // A few pixels at most; huge sizes (of letters spanning billions of lines) that a pixel
    // does not change stay as they are.
    for _ in 0..MAX_STEPS {
        if font_size <= 1.0 || cap_at(font_size) <= desired + 1e-3 {
            break;
        }
        font_size -= 1.0;
    }
    Some(if font_size > 1.0 { font_size } else { exact })
}

/// Where the border box of an initial letter of `block_size` (border box), with its baseline
/// `baseline` below its top, starts below the top of the first line of a paragraph whose lines
/// are `line_height` apart and whose text has `descent` below the baseline of a line (with the
/// leading below it); `(size, sink)` as `initial-letter` gives them, the sink of a drop letter
/// the size rounded up, as in Chrome 157. Horizontal lines (`centered` false) align the
/// letter's baseline with line `size`; vertical ones center the box in the lines it spans.
/// Margins are not counted (Blink's `ComputeInitialLetterBoxBlockOffset`).
pub(super) fn block_offset(
    (size, sink): (f32, f32),
    (block_size, baseline): (f32, f32),
    (line_height, descent): (f32, f32),
    centered: bool,
) -> f32 {
    let lines = size.ceil();
    if lines < sink {
        // Sunk below its size: its bottom at the end of line `sink`.
        return line_height * sink - block_size;
    }
    if centered {
        return (line_height * lines - block_size) / 2.0;
    }
    Au::from_px(line_height * size).to_px() - baseline - descent
}

/// How many empty lines an initial letter raised above the first line puts before the text:
/// Chrome moves the text down by `ceil(size) - sink` lines.
pub(crate) fn raised_lines((size, sink): (f32, f32)) -> f32 {
    (size.ceil() - sink).max(0.0)
}

/// The box of the initial letter `id` of a paragraph of style `container` and `width`, spanning
/// `(size, sink)` lines (see the module's documentation): its fragment at its place in its
/// margin box, which reaches from the top of the first line to the end of what the lines go
/// around and from the start of the line (with the paragraph's text indent) to its end margin.
pub(super) fn initial_letter(
    layouter: &Layouter<'_>,
    id: BoxId,
    container: &ComputedStyle,
    (size, sink): (f32, f32),
    width: f32,
) -> Option<crate::block::Atomic> {
    let fonts = layouter.fonts;
    let layout_box = layouter.tree.get(id);
    let crate::tree::BoxKind::Inline { items } = &layout_box.kind else {
        return None;
    };
    let (text, letter_style) = items.iter().find_map(|item| match item {
        InlineItem::Text { text, style } => Some((text, style)),
        _ => None,
    })?;
    let line = metrics(fonts, container);
    let line_height = Au::from_px(line.line_height).to_px();
    let paragraph = fonts.face(face_for(fonts, container)?);
    let cap = paragraph.cap_height_at(used_font_size(fonts, container));
    let letter_face = fonts.face(face_for(fonts, letter_style)?);
    let letter_cap = letter_face.metrics.cap_height * letter_face.scale(1.0);
    if line_height <= 0.0 {
        return None;
    }
    let style = Arc::new(ComputedStyle::from(ComputedValues {
        font_size: letter_font_size(
            size,
            (line_height, cap),
            letter_cap,
            (letter_style.font_size, used_font_size(fonts, letter_style)),
        )?,
        ..ComputedValues::clone(letter_style)
    }));
    // Preserved tabs before the letter take their room in its box, to the tab stops of the
    // paragraph, as Blink lays them out as items of their own beside the letter's ink
    // (`CalculateInitialLetterBoxInlineSize`).
    let tabs = text.len() - text.trim_start_matches('\t').len();
    let (stop, least) = super::tab_stops(fonts, container);
    let mut lead = 0.0_f32;
    if stop > 0.0 {
        for _ in 0..tabs {
            let mut next = ((lead / stop).floor() + 1.0) * stop;
            if next - lead < least {
                next += stop;
            }
            lead = next;
        }
    }
    let text = &text[tabs..];
    let mut pieces = Vec::new();
    let levels = crate::bidi::levels(&style, std::iter::once(crate::bidi::Content::Text(text)));
    let levels = levels.as_ref().map_or(TextLevels::LTR, |levels| levels.item(0));
    shape_text(fonts, text, &style, (levels, own_breaks(text)), &mut pieces);
    // The runs of the letter, set from 0, and the box around their ink (Blink's ink bounds of
    // the shaped text, enclosed in layout units).
    let mut runs = Vec::new();
    let mut pen = 0.0_f32;
    let (mut ink_left, mut ink_right) = (f32::MAX, f32::MIN);
    let (mut ink_top, mut ink_bottom) = (f32::MAX, f32::MIN);
    for piece in pieces {
        let PieceKind::Text {
            face, glyphs, text, ..
        } = piece.kind
        else {
            continue;
        };
        let run_size = used_font_size(fonts, &style);
        let variations = font_variations(fonts, face, &style, run_size);
        let mut x = pen;
        for glyph in &glyphs {
            let reach = 10.0 * run_size;
            let font = fonts.face(face);
            if let Some((left, right)) = font.intercept(glyph.id, run_size, &variations, (-reach, reach)) {
                ink_left = ink_left.min(x + glyph.x_offset + left);
                ink_right = ink_right.max(x + glyph.x_offset + right);
            }
            if let Some([_, top, _, bottom]) = font.glyph_bounds(glyph.id, run_size) {
                ink_top = ink_top.min(top + glyph.y_offset);
                ink_bottom = ink_bottom.max(bottom + glyph.y_offset);
            }
            x += glyph.advance;
        }
        runs.push((pen, face, glyphs, text, run_size));
        pen = x;
    }
    if runs.is_empty() || ink_right < ink_left || ink_bottom < ink_top {
        return None;
    }
    let enclose = |low: f32, high: f32| (-Au::from_px_ceil(-low).to_px(), Au::from_px_ceil(high).to_px());
    let (ink_left, ink_right) = enclose(ink_left, ink_right);
    let (ink_top, ink_bottom) = enclose(ink_top, ink_bottom);

    let resolve = |value: &fagus_style::values::LengthPercentage| value.resolve(width);
    let frame = (
        style.border_width.left + resolve(&style.padding.left),
        style.border_width.right + resolve(&style.padding.right),
        style.border_width.top + resolve(&style.padding.top),
        style.border_width.bottom + resolve(&style.padding.bottom),
    );
    let margin = |value: &fagus_style::values::LengthPercentageAuto| value.resolve_or(width, 0.0);
    let (margin_left, margin_right) = (margin(&style.margin.left), margin(&style.margin.right));
    let (margin_top, margin_bottom) = (margin(&style.margin.top), margin(&style.margin.bottom));
    // The letter's box is laid out as a line of its own, which starts with the text indent it
    // inherits.
    let own_indent = style.text_indent.resolve(width);
    let start = if style.is_rtl() { 0.0 } else { own_indent + lead };
    let box_width = frame.0 + own_indent + lead + (ink_right - ink_left) + frame.1;
    let box_height = frame.2 + (ink_bottom - ink_top) + frame.3;
    let baseline = frame.2 - ink_top;
    let descent = line.descent + line.leading_below();
    // Upright in vertical lines, the letter is centered in the lines it spans.
    let centered = super::central_baseline(container)
        && style.text_orientation != fagus_style::values::TextOrientation::Sideways;
    let top = block_offset(
        (size, sink),
        (box_height, baseline),
        (line_height, descent),
        centered,
    ) + margin_top;
    let children = runs
        .into_iter()
        .map(|(x, face, glyphs, text, run_size)| {
            let run = face_metrics(fonts, face, run_size);
            let advance: f32 = glyphs.iter().map(|glyph| glyph.advance).sum();
            Fragment {
                rect: Rect::new(
                    Au::from_px(frame.0 + start - ink_left + x),
                    Au::from_px(baseline - run.ascent),
                    Au::from_px(advance),
                    Au::from_px(run.ascent + run.descent),
                ),
                kind: FragmentKind::Text(TextRun {
                    face,
                    size: run_size,
                    style: Arc::clone(&style),
                    glyphs,
                    text,
                    baseline: Au::from_px(run.ascent),
                    marker: false,
                    symbol: None,
                    orientation: GlyphOrientation::Horizontal,
                    exact: false,
                }),
                children: Children::new(),
            }
        })
        .collect();
    // The box stands after the paragraph's text indent, at the start of the line.
    let indent = container.text_indent.resolve(width);
    let (before, after) = if container.is_rtl() {
        (margin_left, margin_right + indent)
    } else {
        (indent + margin_left, margin_right)
    };
    // The lines go around it from the top of the first line to the end of its bottom margin.
    let height = (top + box_height + margin_bottom).max(0.0);
    Some(crate::block::Atomic {
        fragment: Fragment {
            rect: Rect::new(
                Au::from_px(before),
                Au::from_px(top),
                Au::from_px(box_width),
                Au::from_px(box_height),
            ),
            kind: layout_box.fragment_kind(VisibleSides::ALL),
            children,
        },
        width: Au::from_px((before + box_width + after).max(0.0)),
        height: Au::from_px(height),
        baseline: Au::from_px(top + baseline),
    })
}

#[cfg(test)]
mod tests {
    use super::{block_offset, letter_font_size, raised_lines};

    #[test]
    fn font_size_puts_the_cap_height_on_the_last_baseline() {
        // Ahem (cap height 0.8 em) at 20px with lines 24px apart, three lines: the cap height
        // reaches 2 × 24 + 16 = 64px, at 80px.
        assert_eq!(letter_font_size(3.0, (24.0, 16.0), 0.8, (20.0, 20.0)), Some(80.0));
        // A size whose cap height would overshoot goes down by whole pixels.
        let size = letter_font_size(2.0, (25.0, 15.0), 0.7, (20.0, 20.0)).unwrap_or_default();
        assert!(0.7 * size <= 40.0 + 1e-3);
        assert!(letter_font_size(2.0, (25.0, 15.0), 0.0, (20.0, 20.0)).is_none());
    }

    #[test]
    fn font_size_scales_the_computed_size_by_the_cap_height_at_hundredths() {
        // Inter (cap height 1490 / 2048 em) in text of 10pt, 13.333 pixels set at 13.33, with
        // lines of 20 pixels: the cap height of 29.698 pixels wanted makes 40.83 pixels of the
        // computed 13.333, whose cap height at 40.83 overshoots, so Chrome sets the letter at
        // 39.83 pixels. At 13 pixels, set at 13, the size stays at 40.49.
        let cap = 1490.0 / 2048.0;
        let ten_points = 40.0 / 3.0;
        let size = letter_font_size(2.0, (20.0, cap * 13.33), cap, (ten_points, 13.33)).unwrap_or_default();
        assert!((size - 39.83).abs() < 0.01, "{size}");
        let size = letter_font_size(2.0, (20.0, cap * 13.0), cap, (13.0, 13.0)).unwrap_or_default();
        assert!((size - 40.49).abs() < 0.01, "{size}");
    }

    #[test]
    fn block_offset_aligns_the_baseline_or_the_bottom() {
        // Three lines of 24px with a descent of 6px below the baseline: a box 80px tall with its
        // baseline 64px down starts 2px below the first line's top.
        assert!((block_offset((3.0, 3.0), (80.0, 64.0), (24.0, 6.0), false) - 2.0).abs() < 1e-4);
        // Sunk five lines deep with a size of three: its bottom at the end of line 5.
        assert!((block_offset((3.0, 5.0), (80.0, 64.0), (24.0, 6.0), false) - 40.0).abs() < 1e-4);
        // Vertical lines center the box in the lines it spans.
        assert!((block_offset((3.0, 3.0), (60.0, 30.0), (24.0, 6.0), true) - 6.0).abs() < 1e-4);
    }

    #[test]
    fn raised_letters_push_the_text_down_by_whole_lines() {
        assert!((raised_lines((3.0, 1.0)) - 2.0).abs() < f32::EPSILON);
        assert!((raised_lines((2.5, 1.0)) - 2.0).abs() < f32::EPSILON);
        assert!(raised_lines((3.0, 5.0)).abs() < f32::EPSILON);
    }
}

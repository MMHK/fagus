//! `text-box-trim` and `text-box-edge` (CSS Inline 3, §5): a block's first line loses what lies
//! over the edge `text-box-edge` names (cap height, x-height or ascent of the block's first
//! font), its last line what lies under the other edge (descent or alphabetic baseline).

use fagus_style::ComputedStyle;
use fagus_style::values::TextEdge;
use fagus_text::FontDb;

use crate::fragment::FragmentKind;
use crate::inline::{Line, face_for, metrics, used_font_size};
use crate::units::Au;

/// How far over and under the baseline the edges of `text-box-edge` lie for a block of style
/// `style`, in pixels: from its first available font, as Chrome measures them.
pub(crate) fn edges(fonts: &FontDb, style: &ComputedStyle) -> (f32, f32) {
    let primary = metrics(fonts, style);
    let height = |metric: fn(&fagus_text::FaceMetrics) -> f32| {
        face_for(fonts, style).map_or(primary.ascent, |face| {
            let face = fonts.face(face);
            (metric(&face.metrics) * face.scale(used_font_size(fonts, style)) * 64.0).round() / 64.0
        })
    };
    let (over, under) = style.text_box_edge;
    let over = match over {
        TextEdge::Cap => height(|metrics| metrics.cap_height),
        TextEdge::Ex => height(|metrics| metrics.x_height),
        TextEdge::Text | TextEdge::Alphabetic => primary.ascent,
    };
    let under = match under {
        TextEdge::Alphabetic => 0.0,
        _ => primary.descent,
    };
    (over, under)
}

/// Trims the first line of a block of style `style` over its edge (`start`) and the last line
/// under its edge (`end`): the lines move up, and the last one loses height. The edges of the
/// first line are those of its font as `::first-line` (`first_line`) sets it.
pub(crate) fn trim_lines(
    fonts: &FontDb,
    (style, first_line): (&ComputedStyle, Option<&ComputedStyle>),
    lines: &mut [Line],
    (start, end): (bool, bool),
) {
    if !(start || end) || lines.is_empty() {
        return;
    }
    let first_style = first_line.map(|first_line| crate::inline::restyled(style, style, first_line));
    let first_edges = edges(fonts, first_style.as_ref().unwrap_or(style));
    let over = first_edges.0 * lines[0].text_scale;
    // The last line with content: lines after it that hold nothing (an empty inline box after
    // a `<br>`) take no room, and are no line boxes in Chrome.
    let last = lines
        .iter()
        .rposition(|line| line.height > Au::ZERO)
        .unwrap_or(lines.len() - 1);
    let under = if last == 0 {
        first_edges.1
    } else {
        edges(fonts, style).1
    } * lines[last].text_scale;
    let baseline = |line: &Line| match line.fragment.kind {
        FragmentKind::Line { baseline } => baseline,
        _ => Au::ZERO,
    };
    if start {
        let trim = baseline(&lines[0]) - Au::from_px(over);
        for line in lines.iter_mut() {
            line.top -= trim;
        }
    }
    if end {
        let trim = lines[last].height - baseline(&lines[last]) - Au::from_px(under);
        lines[last].height -= trim;
        for line in &mut lines[last + 1..] {
            line.top -= trim;
        }
    }
}

/// How much trimming `line`, a line of a block of style `style`, under its `text-box-edge` takes
/// off its bottom: what lies under the edge.
pub(crate) fn end_trim(fonts: &FontDb, style: &ComputedStyle, line: &Line) -> Au {
    let FragmentKind::Line { baseline } = line.fragment.kind else {
        return Au::ZERO;
    };
    (line.height - baseline - Au::from_px(edges(fonts, style).1 * line.text_scale)).clamp_non_negative()
}

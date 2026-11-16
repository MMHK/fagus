//! Where the content of a list item goes below an outside marker that is taller above its
//! baseline than the content's first line.
//!
//! Chrome aligns an outside marker with the first baseline of the list item's child that holds
//! the first line, and when the marker reaches higher above its baseline than that child does,
//! it moves the child down by the difference (Blink's `UnpositionedListMarker::AddToBox`): an item
//! whose paragraph is set smaller than the item's own font grows by it.

use fagus_style::values::WritingMode;
use fagus_text::FontDb;

use crate::block::{first_baseline, is_phantom_line};
use crate::fragment::{Fragment, FragmentKind};
use crate::inline::metrics;
use crate::units::Au;

/// How far the in-flow child `child` of a list item moves down below the item's outside marker,
/// if the first line in it holds the marker: the marker's ascent in its own line (its font's
/// ascent and the half-leading of its `line-height`) less the child's first baseline from its
/// border box, or nothing. `None` while the marker is not in the child, so that the next child
/// is asked; `Some(Au::ZERO)` once the marker has been found or a line without it comes first.
pub(crate) fn content_push(fonts: &FontDb, child: &Fragment, mode: WritingMode) -> Option<Au> {
    let line = first_line(child, mode)?;
    let Some(style) = line.children.iter().find_map(|run| match &run.kind {
        FragmentKind::Text(text) if text.marker => Some(&text.style),
        _ => None,
    }) else {
        return Some(Au::ZERO);
    };
    let marker = metrics(fonts, style);
    // A line that `text-box-trim` trimmed over its text (which moved it up out of its box) takes
    // the marker's line with it, as Chrome trims the line that holds the marker.
    let trimmed = (-line.rect.y).max(Au::ZERO);
    let ascent = Au::from_px(marker.ascent + marker.leading_above()) - trimmed;
    let baseline = first_baseline(child, mode)?;
    Some((ascent - baseline).max(Au::ZERO))
}

/// The first line box in a fragment that is there for baselines, as `first_baseline` finds it.
fn first_line(fragment: &Fragment, mode: WritingMode) -> Option<&Fragment> {
    fragment.children.iter().find_map(|child| match &child.kind {
        FragmentKind::Line { .. } => (!is_phantom_line(child)).then_some(child),
        FragmentKind::Box { style, .. }
            if !style.is_out_of_flow() && !style.is_floating() && style.writing_mode.is_parallel(mode) =>
        {
            first_line(child, mode)
        }
        FragmentKind::Anonymous => first_line(child, mode),
        _ => None,
    })
}

//! `-webkit-line-clamp` (and `line-clamp`): the box ends after the last line it shows, as in
//! Chrome 157, and the lines after it are laid out on as overflow, painted below it unless
//! `overflow` clips them.

use std::cell::Cell;

use fagus_style::ComputedStyle;
use fagus_style::values::LengthPercentage;

use crate::fragment::{Fragment, FragmentKind};
use crate::tree::LayoutBox;
use crate::units::Au;

/// Whether a fragment's lines count for the line clamp of a box around it: not those of floats
/// and positioned boxes, which are out of its flow, nor those of boxes that lay out their
/// content in a formatting context of their own (`overflow: hidden`, `display: flow-root`,
/// flex and grid containers, tables), as in Chrome.
fn in_flow(fragment: &Fragment) -> bool {
    match &fragment.kind {
        FragmentKind::Box { style, .. } => counts(style),
        _ => true,
    }
}

/// Whether the lines of a box with `style` count for the line clamp of a box around it.
fn counts(style: &ComputedStyle) -> bool {
    !crate::block::establishes_formatting_context(style)
        && matches!(
            style.display,
            fagus_style::values::Display::Normal {
                inside: fagus_style::values::DisplayInside::Flow,
                ..
            }
        )
}

/// The lines left to a `line-clamp`, put aside while a box whose lines do not count for it is
/// laid out, and given back after.
pub(crate) struct BudgetPause<'c> {
    budget: &'c Cell<Option<usize>>,
    saved: Option<usize>,
}

impl<'c> BudgetPause<'c> {
    /// Puts `budget` aside for a box with `style` whose lines do not count; `None` where they
    /// do, or where no clamp is going on.
    pub(crate) fn of(budget: &'c Cell<Option<usize>>, style: &ComputedStyle) -> Option<Self> {
        let saved = budget.get();
        if saved.is_none() || style.line_clamp.is_some() || counts(style) {
            return None;
        }
        budget.set(None);
        Some(Self { budget, saved })
    }
}

impl Drop for BudgetPause<'_> {
    fn drop(&mut self) {
        self.budget.set(self.saved);
    }
}

/// The lines of a box's content in its flow (not those inside atomic inlines).
pub(crate) fn count_lines(fragment: &Fragment) -> usize {
    fragment
        .children
        .iter()
        .filter(|child| in_flow(child))
        .map(|child| match child.kind {
            FragmentKind::Line { .. } => 1,
            _ => count_lines(child),
        })
        .sum()
}

/// The bottom of the `n`th line (counted from 1) below a fragment, in its coordinates; `Err`
/// with the number of lines still to count where it holds fewer.
fn line_bottom(fragment: &Fragment, n: usize) -> Result<Au, usize> {
    let mut left = n;
    for child in fragment.children.iter().filter(|child| in_flow(child)) {
        match child.kind {
            FragmentKind::Line { .. } => {
                left -= 1;
                if left == 0 {
                    return Ok(child.rect.y + child.rect.height);
                }
            }
            _ => match line_bottom(child, left) {
                Ok(bottom) => return Ok(child.rect.y + bottom),
                Err(rest) => left = rest,
            },
        }
    }
    Err(left)
}

/// Makes a box of an automatic height that shows `lines` lines end after the last of them, with
/// its bottom padding and border; what follows overflows it.
pub(crate) fn end_after_line(
    layout_box: &LayoutBox,
    fragment: &mut Fragment,
    lines: usize,
    containing_width: Au,
) {
    let style = &layout_box.style;
    if lines == 0 {
        return;
    }
    let Ok(bottom) = line_bottom(fragment, lines) else {
        return;
    };
    let padding = match &style.padding.bottom {
        LengthPercentage::Px(px) => *px,
        other => other.resolve(containing_width.to_px()),
    };
    let height = bottom + Au::from_px(padding + style.border_width.bottom);
    if height < fragment.rect.height {
        fragment.rect.height = height;
    }
}

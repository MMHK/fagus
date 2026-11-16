//! Content that Chrome lays out without block fragmentation although it prints: that of the
//! boxes it lays out whole (Blink's `LayoutBox::IsMonolithic`), which get no fragmentation in
//! their constraint space and give none to their descendants.
//!
//! Where it matters: Blink aligns the cells of a table on the baseline of their row
//! (`vertical-align: baseline`) only without block fragmentation (`TableRowLayoutAlgorithm`, as
//! the baseline may lie in another fragmentainer), so in print it puts them at the top of their
//! row everywhere else, the row as tall as with aligned cells (see `Layouter::table`).

use std::cell::Cell;

use fagus_style::ComputedStyle;
use fagus_style::values::Position;

use crate::block::Layouter;

/// Marks the content laid out while it lives as laid out without fragmentation.
pub(crate) struct Unfragmented<'l> {
    depth: Option<&'l Cell<u32>>,
}

impl Drop for Unfragmented<'_> {
    fn drop(&mut self) {
        if let Some(depth) = self.depth {
            depth.set(depth.get().saturating_sub(1));
        }
    }
}

/// Whether Chrome lays out the content of a box with `style` without fragmentation when it
/// prints: a box with size containment or `line-clamp`, or one with `position: fixed`, which
/// is repeated on every page. Atomic inlines are monolithic in Blink as well, but measured in
/// Chrome only an inline table aligns its own cells on their baseline (see `Layouter::table`);
/// the content of an `inline-block` is laid out with fragmentation.
pub(crate) fn lays_out_whole(style: &ComputedStyle) -> bool {
    (style.size_containment().0 && style.size_containment().1)
        || style.line_clamp.is_some()
        || style.position == Position::Fixed
}

impl Layouter<'_> {
    /// Lays out what follows, until the returned guard is dropped, as the content of a box with
    /// `style`: without fragmentation where [`lays_out_whole`] says so.
    pub(crate) fn enter_box(&self, style: &ComputedStyle) -> Unfragmented<'_> {
        if lays_out_whole(style) {
            self.enter_unfragmented()
        } else {
            Unfragmented { depth: None }
        }
    }

    /// Lays out what follows, until the returned guard is dropped, without fragmentation (an
    /// orthogonal flow, which Blink lays out whole as the root of a writing mode).
    pub(crate) fn enter_unfragmented(&self) -> Unfragmented<'_> {
        self.unfragmented.set(self.unfragmented.get() + 1);
        Unfragmented {
            depth: Some(&self.unfragmented),
        }
    }

    /// Whether the content being laid out is laid out without fragmentation in Chrome.
    pub(crate) fn is_unfragmented(&self) -> bool {
        self.unfragmented.get() > 0
    }
}

//! The scroll bars of scroll containers, as Chrome on Linux prints them: its classic scroll
//! bars (Blink's `ScrollbarThemeFluent`, not overlaid) take their room between the border and the
//! padding of a box with `overflow: scroll`, and of one with `overflow: auto` whose content
//! overflows it, and are printed scrolled to their start. Chrome on macOS overlays scroll bars,
//! which print nothing; Fagus follows Linux.
//!
//! Layout adds the room of a scroll bar to the padding of the box's style (taking it from a
//! `content-box` width or height that is given), so that the box and its content lay out as
//! Chrome lays them out beside the bar, and marks the style ([`fagus_style::values::Scrollbars`]); painting draws the
//! bars there. A box with `overflow: auto` gets its bars once a layout shows its content to
//! overflow it, and the document is laid out again with them.

use std::sync::Arc;

use fagus_style::ComputedStyle;
use fagus_style::values::{
    BoxSizing, CalcNode, Display, DisplayInside, DisplayOutside, LengthPercentage, Scrollbar, ScrollbarWidth,
    Size,
};
use rustc_hash::FxHashMap;

use crate::fragment::{Fragment, FragmentKind, Page};
use crate::tree::{BoxId, BoxTree};

/// How many times a document is laid out again for scroll bars that overflowing content asks
/// for at most: a bar takes room, which can make the content overflow the other way.
pub(crate) const MAX_PASSES: usize = 3;

/// How thick the scroll bars of `scrollbar-width` are, in pixels (Chrome 157 on Linux: 15, and
/// two thirds of that rounded for `thin`; none for `none`).
pub fn thickness(width: ScrollbarWidth) -> f32 {
    match width {
        ScrollbarWidth::Auto => 15.0,
        ScrollbarWidth::Thin => 10.0,
        ScrollbarWidth::None => 0.0,
    }
}

/// Whether a box of `style` may have scroll bars: a scroll container with `overflow: auto` or
/// `scroll` along an axis, of a block, flex or grid container in horizontal writing.
fn may_scroll(style: &ComputedStyle) -> bool {
    (style.scrollbar_x != Scrollbar::None || style.scrollbar_y != Scrollbar::None)
        && thickness(style.scrollbar_width) > 0.0
        && !style.is_vertical()
        && matches!(
            style.display,
            Display::Normal { outside, inside, .. }
                if inside != DisplayInside::Table
                    && !(outside == DisplayOutside::Inline && inside == DisplayInside::Flow)
        )
}

/// The resize handle Chrome draws in the corner of a scroll container that may be resized, 9
/// pixels square, mirrored right to left.
pub fn resize_handle(mirrored: bool) -> Arc<crate::Image> {
    crate::tree::forms::resizer(mirrored)
}

/// `length` and `delta` pixels.
fn plus(length: &LengthPercentage, delta: f32) -> LengthPercentage {
    let node = match length {
        LengthPercentage::Px(px) => return LengthPercentage::Px(px + delta),
        LengthPercentage::Percent(fraction) => CalcNode::Percent(*fraction),
        LengthPercentage::Calc(node) => (**node).clone(),
    };
    LengthPercentage::Calc(Arc::new(CalcNode::Sum(
        Box::new(node),
        Box::new(CalcNode::Px(delta)),
    )))
}

/// A size given as a length less `delta` pixels, not below zero; other sizes as they are.
fn less(size: &Size, delta: f32) -> Size {
    match size {
        Size::Length(LengthPercentage::Px(px)) => Size::Length(LengthPercentage::Px((px - delta).max(0.0))),
        Size::Length(length) => Size::Length(LengthPercentage::Calc(Arc::new(CalcNode::Max(vec![
            match plus(length, -delta) {
                LengthPercentage::Calc(node) => (*node).clone(),
                LengthPercentage::Px(px) => CalcNode::Px(px),
                LengthPercentage::Percent(fraction) => CalcNode::Percent(fraction),
            },
            CalcNode::Px(0.0),
        ])))),
        other => other.clone(),
    }
}

/// `style` with the scroll bars it lacks of `vertical` and `horizontal` (see the module).
pub(crate) fn with_scrollbars(style: &ComputedStyle, vertical: bool, horizontal: bool) -> ComputedStyle {
    let mut style = style.clone();
    let thickness = thickness(style.scrollbar_width);
    let content_box = style.box_sizing == BoxSizing::ContentBox;
    if vertical && !style.scrollbars.vertical {
        // Right to left, the vertical bar is on the left (Blink's
        // `IsLeftSideVerticalScrollbar`).
        if style.is_rtl() {
            style.padding.left = plus(&style.padding.left, thickness);
        } else {
            style.padding.right = plus(&style.padding.right, thickness);
        }
        if content_box {
            style.width = less(&style.width, thickness);
        }
        style.scrollbars.vertical = true;
    }
    if horizontal && !style.scrollbars.horizontal {
        style.padding.bottom = plus(&style.padding.bottom, thickness);
        if content_box {
            style.height = less(&style.height, thickness);
        }
        style.scrollbars.horizontal = true;
    }
    style.scrollbars.thickness = thickness;
    style
}

/// Gives the boxes with `overflow: scroll` their scroll bars, which they have whether their
/// content overflows or not.
pub(crate) fn add_scrolling(tree: &mut BoxTree) {
    for id in tree.ids() {
        let style = &tree.get(id).style;
        if !may_scroll(style) {
            continue;
        }
        let vertical = style.scrollbar_y == Scrollbar::Scroll;
        let horizontal = style.scrollbar_x == Scrollbar::Scroll;
        if vertical || horizontal {
            let style = Arc::new(with_scrollbars(style, vertical, horizontal));
            tree.box_mut(id).style = style;
        }
    }
}

/// Gives the boxes with `overflow: auto` whose content overflows them on `pages` the scroll bars
/// for that. Returns whether it gave any, after which the document is laid out again.
pub(crate) fn add_overflowing(tree: &mut BoxTree, pages: &[Page]) -> bool {
    // The boxes that may still get a bar, by the address of their style, which their fragments
    // share.
    let mut candidates: FxHashMap<usize, BoxId> = FxHashMap::default();
    for id in tree.ids() {
        let style = &tree.get(id).style;
        if may_scroll(style)
            && (style.scrollbar_x == Scrollbar::Auto && !style.scrollbars.horizontal
                || style.scrollbar_y == Scrollbar::Auto && !style.scrollbars.vertical)
        {
            candidates.insert(Arc::as_ptr(style) as usize, id);
        }
    }
    if candidates.is_empty() {
        return false;
    }
    let mut found: FxHashMap<BoxId, (bool, bool)> = FxHashMap::default();
    for page in pages {
        for fragment in &page.fragments {
            overflowing(fragment, &candidates, &mut found);
        }
    }
    let mut added = false;
    for (id, (vertical, horizontal)) in found {
        let style = &tree.get(id).style;
        let vertical = vertical && style.scrollbar_y == Scrollbar::Auto && !style.scrollbars.vertical;
        let horizontal = horizontal && style.scrollbar_x == Scrollbar::Auto && !style.scrollbars.horizontal;
        if vertical || horizontal {
            let style = Arc::new(with_scrollbars(style, vertical, horizontal));
            tree.box_mut(id).style = style;
            added = true;
        }
    }
    added
}

/// Finds the fragments of `candidates` in `fragment` whose content overflows them, along each
/// axis.
fn overflowing(
    fragment: &Fragment,
    candidates: &FxHashMap<usize, BoxId>,
    found: &mut FxHashMap<BoxId, (bool, bool)>,
) {
    if let FragmentKind::Box { style, sides, .. } = &fragment.kind
        && let Some(id) = candidates.get(&(Arc::as_ptr(style) as usize))
    {
        let (x, y) = overflow(style, fragment);
        let entry = found.entry(*id).or_default();
        entry.0 |= y && sides.top && sides.bottom;
        entry.1 |= x;
    }
    for child in &fragment.children {
        overflowing(child, candidates, found);
    }
}

/// Whether the content of the fragment of a scroll container of `style` overflows its client
/// area horizontally and vertically (see [`scroll_size`]), in whole pixels as Chrome compares
/// its scroll width and height with its client width and height.
pub fn overflow(style: &ComputedStyle, fragment: &Fragment) -> (bool, bool) {
    let size = scroll_size(style, fragment);
    (
        size.total.0.round() > size.client.0.round(),
        size.total.1.round() > size.client.1.round(),
    )
}

/// The sizes of a scroll container's client area (its padding box less its scroll bars) and
/// of its scrollable overflow (CSS Overflow 3, §2.2), in pixels: how much of its content shows
/// and how large it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollSize {
    pub client: (f32, f32),
    pub total: (f32, f32),
}

/// The [`ScrollSize`] of a fragment of a scroll container of `style`: its content reaches to its
/// end padding, and past it where it overflows, to the right (to the left right to left) and
/// down. Percentages of the padding refer to the containing block's width, which is not at hand
/// here: they count as nothing.
pub fn scroll_size(style: &ComputedStyle, fragment: &Fragment) -> ScrollSize {
    let bars = style.scrollbars;
    let thickness = if bars.any() { bars.thickness } else { 0.0 };
    let rtl = style.is_rtl();
    let vertical = if bars.vertical { thickness } else { 0.0 };
    let horizontal = if bars.horizontal { thickness } else { 0.0 };
    let border = &style.border_width;
    let fixed = |length: &LengthPercentage| length.fixed().unwrap_or(0.0);
    // The client area, from the border box, and the padding the box has of its own.
    let left = border.left + if rtl { vertical } else { 0.0 };
    let right = fragment.rect.width.to_px() - border.right - if rtl { 0.0 } else { vertical };
    let top = border.top;
    let bottom = fragment.rect.height.to_px() - border.bottom - horizontal;
    let padding_left = (fixed(&style.padding.left) - if rtl { vertical } else { 0.0 }).max(0.0);
    let padding_right = (fixed(&style.padding.right) - if rtl { 0.0 } else { vertical }).max(0.0);
    let padding_bottom = (fixed(&style.padding.bottom) - horizontal).max(0.0);
    // Chrome's scrollable overflow: the border boxes of what the box holds, and its in-flow
    // content (the margin boxes of its in-flow children and its lines) with its end padding
    // (Blink's `ScrollableOverflowCalculator`).
    let mut all = Extent::EMPTY;
    all.add(fragment, (0.0, 0.0));
    let mut inflow = Extent::EMPTY;
    inflow.add_in_flow(fragment, (0.0, 0.0));
    let client = ((right - left).max(0.0), (bottom - top).max(0.0));
    let width = if rtl {
        right - all.left.min(inflow.left - padding_left)
    } else {
        all.right.max(inflow.right + padding_right) - left
    };
    let height = all.bottom.max(inflow.bottom + padding_bottom) - top;
    ScrollSize {
        client,
        total: (width.max(client.0), height.max(client.1)),
    }
}

/// How far left, right and down the content of a box reaches, from its border box.
struct Extent {
    left: f32,
    right: f32,
    bottom: f32,
}

impl Extent {
    /// Nothing: an extent that any content widens.
    const EMPTY: Self = Self {
        left: f32::MAX,
        right: f32::MIN,
        bottom: f32::MIN,
    };

    fn reach(&mut self, x: f32, y: f32, width: f32, height: f32) {
        self.left = self.left.min(x);
        self.right = self.right.max(x + width);
        self.bottom = self.bottom.max(y + height);
    }

    /// Extends to the in-flow content of `fragment` (at `origin`): the margin boxes of its
    /// in-flow children and what its lines hold, through anonymous boxes and the parts of form
    /// controls.
    fn add_in_flow(&mut self, fragment: &Fragment, origin: (f32, f32)) {
        let fixed = |margin: &fagus_style::values::LengthPercentageAuto| match margin {
            fagus_style::values::LengthPercentageAuto::Length(length) => length.fixed().unwrap_or(0.0),
            fagus_style::values::LengthPercentageAuto::Auto => 0.0,
        };
        for child in &fragment.children {
            let x = origin.0 + child.rect.x.to_px();
            let y = origin.1 + child.rect.y.to_px();
            let (width, height) = (child.rect.width.to_px(), child.rect.height.to_px());
            match &child.kind {
                FragmentKind::Placeholder(_) => {}
                // An empty line (after a last line break) holds nothing that overflows.
                FragmentKind::Line { .. } if height <= 0.0 => {}
                FragmentKind::Line { .. } => self.add(child, (x, y)),
                FragmentKind::Anonymous => self.add_in_flow(child, (x, y)),
                FragmentKind::Box { style, .. } if style.is_out_of_flow() || style.is_floating() => {}
                FragmentKind::Box { style, .. } if style.control_part => self.add_in_flow(child, (x, y)),
                FragmentKind::Box { style, .. } => {
                    let left = fixed(&style.margin.left);
                    let right = fixed(&style.margin.right);
                    let bottom = fixed(&style.margin.bottom);
                    self.reach(x - left, y, width + left + right, height + bottom);
                }
                FragmentKind::Text(_) | FragmentKind::Image(_) => self.reach(x, y, width, height),
            }
        }
    }

    /// Extends to the boxes, text and images inside `fragment` (at `origin` in the box being
    /// measured), into boxes that do not clip their own overflow.
    fn add(&mut self, fragment: &Fragment, origin: (f32, f32)) {
        use fagus_style::values::Overflow;
        for child in &fragment.children {
            let x = origin.0 + child.rect.x.to_px();
            let y = origin.1 + child.rect.y.to_px();
            let (width, height) = (child.rect.width.to_px(), child.rect.height.to_px());
            let mut reach = || self.reach(x, y, width, height);
            match &child.kind {
                FragmentKind::Placeholder(_) => continue,
                FragmentKind::Line { .. } if height <= 0.0 => continue,
                // A line box is as wide as its container; what it holds may reach farther. So
                // may what the parts of a form control hold, which are no boxes in Chrome.
                FragmentKind::Line { .. } => {}
                // The resize handle of a text area is drawn in its corner, as no box.
                FragmentKind::Box { style, .. } if style.control_part && style.is_out_of_flow() => continue,
                FragmentKind::Box { style, .. } if style.control_part => {}
                FragmentKind::Box { style, .. }
                    if style.overflow_x != Overflow::Visible || style.overflow_y != Overflow::Visible =>
                {
                    reach();
                    continue;
                }
                _ => reach(),
            }
            self.add(child, (x, y));
        }
    }
}

#[cfg(test)]
mod tests {
    use fagus_style::ComputedValues;

    use super::*;

    #[test]
    fn takes_the_room_of_scroll_bars_from_the_padding_box() {
        let mut style = ComputedStyle::from(ComputedValues {
            box_sizing: BoxSizing::ContentBox,
            width: Size::Length(LengthPercentage::Px(300.0)),
            height: Size::Length(LengthPercentage::Px(10.0)),
            ..ComputedValues::default()
        });
        let both = with_scrollbars(&style, true, true);
        assert_eq!(both.padding.right, LengthPercentage::Px(15.0));
        assert_eq!(both.padding.bottom, LengthPercentage::Px(15.0));
        assert_eq!(both.width, Size::Length(LengthPercentage::Px(285.0)));
        assert_eq!(both.height, Size::Length(LengthPercentage::Px(0.0)));
        assert!(both.scrollbars.vertical && both.scrollbars.horizontal);
        // Once only.
        assert_eq!(with_scrollbars(&both, true, true), both);
        // A border box keeps its size; `thin` bars are 10px thick.
        style.box_sizing = BoxSizing::BorderBox;
        style.scrollbar_width = ScrollbarWidth::Thin;
        let thin = with_scrollbars(&style, true, false);
        assert_eq!(thin.padding.right, LengthPercentage::Px(10.0));
        assert_eq!(thin.width, style.width);
        assert!(!thin.scrollbars.horizontal);
    }
}

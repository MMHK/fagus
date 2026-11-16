//! The boxes of scroll markers and scroll buttons (CSS Overflow 5, §3 and §4) as Chrome 157
//! prints them: nothing scrolls on paper, but the boxes are there. A scroll container with a
//! `scroll-marker-group` has a `::scroll-marker-group` box before or after it, which holds the
//! `::scroll-marker` of every element it scrolls; an element with `::scroll-button()` styles
//! that give `content` has the buttons after it, in the order of
//! [`ScrollDirection::ALL`], before a group after it.

use std::sync::Arc;

use fagus_css::matching::ScrollDirection;
use fagus_dom::NodeId;
use fagus_style::values::{Display, DisplayInside, Overflow, Position, Rgba, ScrollMarkerGroup};
use fagus_style::{ComputedStyle, PseudoKey};

use super::{BoxId, BoxKind, BoxTree, Builder, Collected, InlineItem, LayoutBox, process_white_space_noting};

impl Builder<'_> {
    /// The scroll marker group that goes before `element` (with `style`), into `out`; `item`
    /// tells whether it is a flex or grid item.
    pub(super) fn scroll_boxes_before(
        &self,
        tree: &mut BoxTree,
        element: NodeId,
        style: &ComputedStyle,
        item: bool,
        out: &mut Vec<Collected>,
    ) {
        if style.scroll_marker_group == ScrollMarkerGroup::Before {
            self.scroll_marker_group(tree, element, style, item, out);
        }
    }

    /// The scroll buttons and the scroll marker group that go after `element`, into `out`.
    pub(super) fn scroll_boxes_after(
        &self,
        tree: &mut BoxTree,
        element: NodeId,
        item: bool,
        out: &mut Vec<Collected>,
    ) {
        let Some(style) = self.styled.style(element) else {
            return;
        };
        let scrolls = is_scroll_container(style);
        for direction in ScrollDirection::ALL {
            let key = PseudoKey::ScrollButton(direction);
            let Some(button) = self.styled.pseudo(element, key) else {
                continue;
            };
            // Nothing is scrolled on paper: the buttons back to the start are disabled, and so
            // are all of an element that does not scroll. (Chrome also disables those of a
            // scroll container whose content does not overflow, which only layout tells.)
            let disabled = !scrolls
                || matches!(
                    direction,
                    ScrollDirection::BlockStart | ScrollDirection::InlineStart
                );
            let mut button = scroll_button_style(button, disabled);
            if item {
                stack_item(Arc::make_mut(&mut button));
            }
            self.pseudo_box(tree, element, key, &button, item, out);
        }
        if style.scroll_marker_group == ScrollMarkerGroup::After {
            self.scroll_marker_group(tree, element, style, item, out);
        }
    }

    /// The scroll marker group of scroll container `element`, with the scroll markers of the
    /// elements it scrolls, into `out`. Blink gives it layout containment, and size
    /// containment where it is in flow (`StyleAdjuster`).
    fn scroll_marker_group(
        &self,
        tree: &mut BoxTree,
        element: NodeId,
        style: &ComputedStyle,
        item: bool,
        out: &mut Vec<Collected>,
    ) {
        if !is_scroll_container(style) {
            return;
        }
        let Some(group) = self.styled.pseudo(element, PseudoKey::ScrollMarkerGroup) else {
            return;
        };
        if group.display == Display::None {
            return;
        }
        let mut own = (**group).clone();
        own.display = match own.display {
            Display::Contents => Display::BLOCK,
            display => display.blockify(),
        };
        if item {
            own.float = fagus_style::values::Float::None;
            stack_item(&mut own);
        }
        own.contain_any = true;
        own.contain_layout = true;
        own.layout_containment = true;
        if !own.is_out_of_flow() {
            own.contain_size = true;
        }
        let container = matches!(
            own.display,
            Display::Normal {
                inside: DisplayInside::Flex | DisplayInside::Grid,
                ..
            }
        );
        let mut collected = Vec::new();
        let fixed = tree.fixed.len();
        for marker in self.scrolled_markers(element) {
            if let Some(marker_style) = self.styled.pseudo(marker, PseudoKey::ScrollMarker) {
                self.pseudo_box(
                    tree,
                    marker,
                    PseudoKey::ScrollMarker,
                    marker_style,
                    container,
                    &mut collected,
                );
            }
        }
        // The group's layout containment makes it the containing block of fixed markers, which
        // are absolutely positioned in it (see `crate::contained_fixed`).
        for id in tree.fixed.split_off(fixed) {
            let layout_box = tree.box_mut(id);
            let mut style = ComputedStyle::clone(&layout_box.style);
            style.position = Position::Absolute;
            layout_box.style = Arc::new(style);
            tree.contained_fixed.insert(id);
        }
        let own = Arc::new(own);
        let kind = if container {
            let children: Vec<BoxId> = collected
                .into_iter()
                .filter_map(|collected| match collected {
                    Collected::Block(id)
                    | Collected::Inline(InlineItem::OutOfFlow(id) | InlineItem::Float(id)) => Some(id),
                    _ => None,
                })
                .collect();
            if matches!(
                own.display,
                Display::Normal {
                    inside: DisplayInside::Flex,
                    ..
                }
            ) {
                BoxKind::Flex { children }
            } else {
                BoxKind::Grid { children }
            }
        } else if collected.iter().any(|item| matches!(item, Collected::Block(_))) {
            BoxKind::Block {
                children: self.block_children(tree, &own, collected),
            }
        } else {
            let items: Vec<InlineItem> = collected
                .into_iter()
                .filter_map(|item| match item {
                    Collected::Inline(item) => Some(item),
                    Collected::Block(_) | Collected::Part(_) => None,
                })
                .collect();
            BoxKind::Inline {
                items: process_white_space_noting(items, &mut tree.after_collapsed),
            }
        };
        let id = tree.push(LayoutBox {
            style: Arc::clone(&own),
            node: None,
            generated: true,
            kind,
        });
        if own.is_out_of_flow() {
            if own.position == Position::Fixed {
                tree.fixed.push(id);
            }
            out.push(Collected::Inline(InlineItem::OutOfFlow(id)));
        } else if own.is_floating() {
            out.push(Collected::Inline(InlineItem::Float(id)));
        } else {
            out.push(Collected::Block(id));
        }
    }

    /// The elements with a `::scroll-marker` that scroll container `scroller` scrolls, in tree
    /// order.
    fn scrolled_markers(&self, scroller: NodeId) -> Vec<NodeId> {
        self.document
            .descendants(scroller)
            .filter(|&node| {
                self.styled.pseudo(node, PseudoKey::ScrollMarker).is_some()
                    && self.scroller_of(node) == Some(scroller)
            })
            .collect()
    }

    /// The scroll container that scrolls `element`: the nearest one up its chain of containing
    /// blocks, which absolutely positioned and fixed boxes leave for the boxes that contain them.
    /// `None` where it or an element around it is not displayed.
    fn scroller_of(&self, element: NodeId) -> Option<NodeId> {
        let style = self.styled.style(element)?;
        if style.display == Display::None {
            return None;
        }
        let mut escaping = escaping(style);
        let mut current = self.document.parent(element);
        while let Some(parent) = current {
            let style = self.styled.style(parent)?;
            if style.display == Display::None || style.content_hidden {
                return None;
            }
            let contains = match escaping {
                None => true,
                Some(Position::Fixed) => style.has_transform() || style.contains_out_of_flow(),
                Some(_) => style.contains_absolute(),
            };
            if contains {
                if is_scroll_container(style) {
                    return Some(parent);
                }
                escaping = self::escaping(style);
            }
            current = self.document.parent(parent);
        }
        None
    }
}

/// The position of a box that leaves the boxes around it for its containing block: absolute or
/// fixed.
fn escaping(style: &ComputedStyle) -> Option<Position> {
    matches!(style.position, Position::Absolute | Position::Fixed).then_some(style.position)
}

/// Makes a flex or grid item with a `z-index` and no position a stacking context, as the
/// cascade does for elements (CSS Flexbox 1, §4.3; CSS Grid 2, §9.2).
fn stack_item(style: &mut ComputedStyle) {
    if style.z_index.is_some() && !style.is_positioned() {
        style.isolation = fagus_style::values::Isolation::Isolate;
    }
}

/// Whether a box is a scroll container: `overflow` other than `visible` and `clip`.
fn is_scroll_container(style: &ComputedStyle) -> bool {
    style.overflow_x == Overflow::Hidden || style.overflow_y == Overflow::Hidden
}

/// The style of a scroll button: that of a button (`<button>`), with the text of a disabled one
/// where the button cannot scroll (Chrome's `scroll_button.css`).
fn scroll_button_style(style: &Arc<ComputedStyle>, disabled: bool) -> Arc<ComputedStyle> {
    let mut style = super::forms::button_style(style, disabled);
    if disabled && style.color == Rgba::BLACK {
        Arc::make_mut(&mut style).color = Rgba {
            r: 16.0 / 255.0,
            g: 16.0 / 255.0,
            b: 16.0 / 255.0,
            a: 0.3,
        };
    }
    style
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_scroll_containers_and_boxes_that_leave_them() {
        let hidden = ComputedStyle {
            overflow_y: Overflow::Hidden,
            ..ComputedStyle::default()
        };
        assert!(is_scroll_container(&hidden));
        let clipped = ComputedStyle {
            overflow_x: Overflow::Clip,
            overflow_y: Overflow::Clip,
            ..ComputedStyle::default()
        };
        assert!(!is_scroll_container(&clipped));
        let fixed = ComputedStyle {
            position: Position::Fixed,
            ..ComputedStyle::default()
        };
        assert_eq!(escaping(&fixed), Some(Position::Fixed));
        assert_eq!(escaping(&ComputedStyle::default()), None);
    }
}

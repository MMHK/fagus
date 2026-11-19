//! Monolithic content that reaches past the end of a page: which part of a page's content it is
//! in, for the way Chrome moves the content after it on the pages that follow.
//!
//! Chrome (Blink's `BlockBreakToken::MonolithicOverflow`) records, in each box that holds an
//! image, a line or another unbreakable piece taller than the rest of the page, how far it reaches
//! into the next page, and the box around it moves its content after it below that point. A box
//! that ends with the piece (no padding or border below it, no content after it) is in a
//! parallel flow from then on, so its parent records nothing, and on the page where the overflow
//! ends that parent moves the empty rest of the box down by the overflow once more, from where
//! the parent starts on that page. When that lies below the end of the page, the parent's content
//! on the page after starts the difference lower, and a page is added for it if the document
//! ends before.

use fagus_dom::NodeId;
use fagus_style::values::{Display, DisplayInside, FlexDirection, FlexWrap};

use crate::fragment::{Fragment, FragmentKind};
use crate::units::Au;

/// How a fragment takes part in monolithic overflow.
pub(crate) enum Reach {
    /// It is laid out whole (a line, an image, a box with size containment or `line-clamp`).
    Monolithic,
    /// It is not in the flow that overflow pushes (text in a line, floats, positioned boxes, a
    /// flex, grid or table container that holds its content beside the overflow on the next page).
    Outside,
    /// Its children are.
    Container,
}

/// How `fragment` takes part in monolithic overflow.
pub(crate) fn reach(fragment: &Fragment) -> Reach {
    match &fragment.kind {
        FragmentKind::Line { .. } | FragmentKind::Image(_) => Reach::Monolithic,
        FragmentKind::Text(_) | FragmentKind::Placeholder(_) => Reach::Outside,
        FragmentKind::Box { style, .. } if style.is_floating() || style.is_out_of_flow() => Reach::Outside,
        FragmentKind::Box { style, .. }
            if (style.size_containment().0 && style.size_containment().1) || style.line_clamp.is_some() =>
        {
            Reach::Monolithic
        }
        // A flex, grid or table container that goes on to the next page holds the content beside
        // the overflow there, in parallel to it, which it does not push (a table moves its rows
        // below it itself, see `BreakToken::Table`). A single-line column flex container stacks
        // its items as a block does: those after the overflow go on below it, as in Chrome.
        FragmentKind::Box { style, sides, .. }
            if !sides.bottom
                && !stacks_items(style)
                && !items_end(fragment)
                && matches!(
                    style.display,
                    Display::Normal {
                        inside: DisplayInside::Flex | DisplayInside::Grid | DisplayInside::Table,
                        ..
                    }
                ) =>
        {
            Reach::Outside
        }
        FragmentKind::Box { .. } | FragmentKind::Anonymous => Reach::Container,
    }
}

/// Whether `fragment` is a grid whose items all end on its page: it holds nothing beside
/// overflow of theirs on the next page, only the rest of its rows, which goes on below it.
fn items_end(fragment: &Fragment) -> bool {
    matches!(&fragment.kind, FragmentKind::Box { style, .. }
        if matches!(style.display, Display::Normal { inside: DisplayInside::Grid, .. }))
        && !fragment.children.is_empty()
        && fragment
            .children
            .iter()
            .all(|child| matches!(&child.kind, FragmentKind::Box { sides, .. } if sides.bottom))
}

/// Whether `fragment` is a grid container in the flow that goes on to the next page.
fn continues_grid(fragment: &Fragment) -> bool {
    matches!(&fragment.kind, FragmentKind::Box { style, sides, .. }
        if !sides.bottom
            && !style.is_floating()
            && !style.is_out_of_flow()
            && matches!(style.display, Display::Normal { inside: DisplayInside::Grid, .. }))
}

/// Whether a box with `style` is a single-line column flex container, whose items follow one
/// another down the page as the boxes of a block container do.
fn stacks_items(style: &fagus_style::ComputedStyle) -> bool {
    style.flex_direction == FlexDirection::Column
        && style.flex_wrap == FlexWrap::NoWrap
        && matches!(
            style.display,
            Display::Normal {
                inside: DisplayInside::Flex,
                ..
            }
        )
}

/// How far down the content in the flow of a fragment lying `origin` below the top of its page
/// reaches: the bottom of its lowest monolithic content (lines, images, boxes laid out whole), as
/// far as the boxes it is in grow to hold it (Chrome lets a box overflow the page with the
/// monolithic content in it), not into floats and positioned boxes.
pub(crate) fn content_bottom(fragment: &Fragment, origin: Au) -> Option<Au> {
    let top = origin + fragment.rect.y;
    let bottom = top + fragment.rect.height;
    match reach(fragment) {
        Reach::Monolithic => Some(bottom),
        // The items of a row flex container that go on to the next page carry the overflow of
        // their monolithic content in their own flow (unlike those that end with it), which
        // moves the container down on the next page, as in Chrome (Blink pushes the outermost
        // box whose break token records monolithic overflow).
        // So does an item that ends with monolithic content reaching past the end of the page,
        // with its bottom margin, where other items go on: the container's part holds it (see
        // `parallel::split_line`), as in Chrome.
        Reach::Outside if lays_out_rows(fragment) => fragment
            .children
            .iter()
            .filter_map(|child| match &child.kind {
                // (An item that clones its decorations shows its bottom border on every page.)
                FragmentKind::Box { sides, style, .. } if !sides.bottom || style.clones_decorations() => {
                    item_content_bottom(child, top)
                }
                FragmentKind::Box { style, .. } if in_flow(child) => content_bottom(child, top)
                    .map(|reach| {
                        reach + Au::from_px(style.margin.bottom.resolve_or(fragment.rect.width.to_px(), 0.0))
                    })
                    .filter(|&reach| reach == bottom - bottom_frame(fragment)),
                _ => None,
            })
            .max(),
        // A grid that goes on to the next page reaches as far as its own box: past the end of
        // the page where the monolithic content of its items does and rows follow (see
        // `grid_item_overflow` in `block`), as in Chrome.
        Reach::Outside if continues_grid(fragment) => Some(bottom),
        Reach::Outside => None,
        Reach::Container => {
            // The columns of a multi-column container are fragmentainers of their own: what
            // overflows them does not reach into the next page, as in Chrome.
            let columns = matches!(&fragment.kind, FragmentKind::Box { style, .. } if style.is_multicol());
            let lowest = fragment
                .children
                .iter()
                .filter(|child| !(columns && matches!(child.kind, FragmentKind::Anonymous)))
                .filter_map(|child| content_bottom(child, top))
                .max();
            match &fragment.kind {
                // A box that repeats its bottom padding and border on every page holds them
                // below the content too, which they then reach past the page with.
                FragmentKind::Box { style, .. } if style.clones_decorations() => lowest.map(|lowest| {
                    let frame = Au::from_px(
                        style.border_width.bottom + style.padding.bottom.resolve(fragment.rect.width.to_px()),
                    );
                    if lowest >= bottom - frame { bottom } else { lowest }
                }),
                // A row flex container that ends with items whose monolithic content reaches
                // past the page holds them with their bottom margins, as in Chrome (Blink's flex
                // line is as tall as the margin boxes of its items laid out whole).
                FragmentKind::Box { .. } if ends_rows(fragment) => {
                    lowest.map(|lowest| lowest.max(bottom - bottom_frame(fragment)).min(bottom))
                }
                FragmentKind::Box { .. } => lowest.map(|lowest| lowest.min(bottom)),
                _ => lowest,
            }
        }
    }
}

/// Whether a block container with `style` ends where monolithic content that ends it reaches
/// past the end of a page, its bottom padding, border and margin taking no room below it, as in
/// Chrome: a formatting context of its own (`display: flow-root`) in the flow that does not clip
/// its content.
pub(crate) fn ends_with_overflow(style: &fagus_style::ComputedStyle) -> bool {
    matches!(
        style.display,
        Display::Normal {
            inside: DisplayInside::FlowRoot,
            ..
        }
    ) && style.overflow_x == fagus_style::values::Overflow::Visible
        && style.overflow_y == fagus_style::values::Overflow::Visible
        && !style.is_floating()
        && !style.is_out_of_flow()
        && !style.is_multicol()
}

/// Whether `fragment` is in the flow of its parent: not a float, a positioned box or its place.
pub(crate) fn in_flow(fragment: &Fragment) -> bool {
    match &fragment.kind {
        FragmentKind::Box { style, .. } => !style.is_floating() && !style.is_out_of_flow(),
        FragmentKind::Placeholder(_) => false,
        _ => true,
    }
}

/// Whether `fragment` is a row flex container of automatic height that ends on its page.
fn ends_rows(fragment: &Fragment) -> bool {
    matches!(
        &fragment.kind,
        FragmentKind::Box { style, sides, .. }
            if sides.bottom
                && matches!(style.height, fagus_style::values::Size::Auto)
                && matches!(style.flex_direction, FlexDirection::Row | FlexDirection::RowReverse)
                && matches!(
                    style.display,
                    Display::Normal {
                        inside: DisplayInside::Flex,
                        ..
                    }
                )
    )
}

/// The bottom padding and border of a box fragment.
fn bottom_frame(fragment: &Fragment) -> Au {
    match &fragment.kind {
        FragmentKind::Box { style, .. } => {
            Au::from_px(style.border_width.bottom + style.padding.bottom.resolve(fragment.rect.width.to_px()))
        }
        _ => Au::ZERO,
    }
}

/// Whether `fragment` is a row flex container that goes on to the next page.
fn lays_out_rows(fragment: &Fragment) -> bool {
    matches!(
        &fragment.kind,
        FragmentKind::Box { style, sides, .. }
            if !sides.bottom
                && !style.is_floating()
                && !style.is_out_of_flow()
                && matches!(style.flex_direction, FlexDirection::Row | FlexDirection::RowReverse)
                && matches!(
                    style.display,
                    Display::Normal {
                        inside: DisplayInside::Flex,
                        ..
                    }
                )
    )
}

/// How far down the content in the flow of a flex item (lying `origin` below the top of its page)
/// reaches, as [`content_bottom`], where the item has the height the line left it on the page:
/// an item of automatic height holds its monolithic content (below which it repeats its bottom
/// padding and border, if it clones them), as a block container does.
pub(crate) fn item_content_bottom(item: &Fragment, origin: Au) -> Option<Au> {
    let FragmentKind::Box { style, .. } = &item.kind else {
        return content_bottom(item, origin);
    };
    let top = origin + item.rect.y;
    let lowest = item
        .children
        .iter()
        .filter_map(|child| content_bottom(child, top))
        .max()?;
    if !matches!(style.height, fagus_style::values::Size::Auto) {
        return Some(lowest.min(top + item.rect.height));
    }
    Some(if style.clones_decorations() {
        lowest
            + Au::from_px(style.border_width.bottom + style.padding.bottom.resolve(item.rect.width.to_px()))
    } else {
        lowest
    })
}

/// What overflow that ends a box in a parallel flow does on the page where it ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Parallel {
    /// The box's parent, of this node, goes on to the next page: its content there moves down.
    Moves(NodeId),
    /// The parent ends too: where the overflow reaches past that page, only a page is added.
    AddsPage,
}

/// The overflow of a page whose content (`content`, its monolithic content reaching down to
/// `bottom` from the top of the page) ends inside a block container that ends with it, if it
/// does (see the module documentation).
pub(crate) fn parallel_overflow(content: &Fragment, bottom: Au) -> Option<(Parallel, Au)> {
    let mut path = Vec::new();
    let found = lowest(content, Au::ZERO, bottom, &mut path)?;
    // The box the monolithic piece is in: the parent of an image's replaced box, else of the
    // line or contained box, or the flex container that holds it with its item's margin.
    let holder = path.len().checked_sub(match found {
        Found::Image => 2,
        Found::Piece | Found::Margin(_) => 1,
    })?;
    // How far the box ends below the piece.
    let below = match found {
        Found::Margin(piece) => bottom - piece,
        _ => Au::ZERO,
    };
    let bottom = bottom - below;
    let (fragment, top) = *path.get(holder)?;
    let FragmentKind::Box { style, sides, .. } = &fragment.kind else {
        // An anonymous block around the line (an inline-level box between blocks) ends with it
        // as a block container does.
        if !matches!(fragment.kind, FragmentKind::Anonymous) || top + fragment.rect.height > bottom {
            return None;
        }
        return Some((moved_by(path.get(holder.checked_sub(1)?)?.0), below));
    };
    // A flex or grid container whose items all end on the page with the piece ends with it as
    // a block container does, as in Chrome.
    let block_container = matches!(
        style.display,
        Display::Normal {
            inside: DisplayInside::Flow | DisplayInside::FlowRoot | DisplayInside::Flex | DisplayInside::Grid,
            ..
        }
    );
    let padding = style.padding.bottom.resolve(fragment.rect.width.to_px());
    // A formatting context of its own ends with the overflow whatever its bottom padding and
    // border (which it repeats below it where it clones them), as in Chrome.
    let frame = Au::from_px(style.border_width.bottom + padding);
    let own_context = ends_with_overflow(style);
    if !block_container
        || !sides.bottom
        || frame > Au::ZERO && !own_context
        || top + fragment.rect.height > bottom + below + if own_context { frame } else { Au::ZERO }
    {
        return None;
    }
    let parent = holder.checked_sub(1).and_then(|index| path.get(index))?;
    if let FragmentKind::Box { style, .. } = &parent.0.kind
        && matches!(
            style.display,
            Display::Normal {
                inside: DisplayInside::Flex | DisplayInside::Grid,
                ..
            }
        )
    {
        // A flex or grid item that ends with the overflow leaves no page to it: the container
        // around it moves nothing, as in Chrome (measured with items of either kind holding an
        // image or a box with size containment).
        return None;
    }
    Some((moved_by(parent.0), below))
}

/// What overflow ending a box in `parent` does on the page where it ends (see
/// [`parallel_overflow`]).
fn moved_by(parent: &Fragment) -> Parallel {
    match &parent.kind {
        FragmentKind::Box {
            node: Some(node),
            sides,
            style,
        } if !sides.bottom
            && matches!(
                style.display,
                Display::Normal {
                    inside: DisplayInside::Flow | DisplayInside::FlowRoot,
                    ..
                }
            ) =>
        {
            Parallel::Moves(*node)
        }
        _ => Parallel::AddsPage,
    }
}

/// Whether the box of `node` has a fragment in `fragment` that goes on to the next page.
pub(crate) fn continues(fragment: &Fragment, node: NodeId) -> bool {
    match &fragment.kind {
        FragmentKind::Box {
            node: Some(own),
            sides,
            ..
        } if *own == node => !sides.bottom,
        FragmentKind::Box { .. } | FragmentKind::Anonymous => {
            fragment.children.iter().any(|child| continues(child, node))
        }
        _ => false,
    }
}

/// The monolithic content [`lowest`] finds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Found {
    /// An image, inside its replaced box.
    Image,
    /// A line or a box laid out whole.
    Piece,
    /// Monolithic content of the items of a row flex container (the last in the path) that ends
    /// below it with their bottom margins: where that content ends.
    Margin(Au),
}

/// Finds the monolithic content of `fragment` (lying `origin` below the top of the page) whose
/// bottom is `bottom`, with the containers around it (and their tops) in `path`, outermost first,
/// or `None` if there is none.
fn lowest<'a>(
    fragment: &'a Fragment,
    origin: Au,
    bottom: Au,
    path: &mut Vec<(&'a Fragment, Au)>,
) -> Option<Found> {
    let top = origin + fragment.rect.y;
    match reach(fragment) {
        Reach::Monolithic => (top + fragment.rect.height == bottom).then_some(
            if matches!(fragment.kind, FragmentKind::Image(_)) {
                Found::Image
            } else {
                Found::Piece
            },
        ),
        Reach::Outside => None,
        Reach::Container => {
            path.push((fragment, top));
            for child in &fragment.children {
                if let Some(found) = lowest(child, top, bottom, path) {
                    return Some(found);
                }
            }
            if ends_rows(fragment)
                && top + fragment.rect.height - bottom_frame(fragment) == bottom
                && let Some(piece) = fragment
                    .children
                    .iter()
                    .filter_map(|child| content_bottom(child, top))
                    .max()
                    .filter(|&piece| piece < bottom)
            {
                return Some(Found::Margin(piece));
            }
            path.pop();
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use fagus_style::ComputedStyle;

    use super::*;
    use crate::fragment::VisibleSides;
    use crate::units::Rect;

    /// Node ids to give the boxes (those of a parsed document; which nodes they are does not
    /// matter).
    fn node(index: usize) -> NodeId {
        let document = fagus_dom::Document::parse_html("<p>a</p><p>b</p>");
        document.descendants(NodeId::DOCUMENT).nth(index).expect("node")
    }

    fn block(node_index: usize, y: f32, height: f32, ends: bool, children: Vec<Fragment>) -> Fragment {
        let style = ComputedStyle::from(fagus_style::ComputedValues {
            display: Display::BLOCK,
            ..fagus_style::ComputedValues::default()
        });
        Fragment {
            rect: Rect::new(Au::ZERO, Au::from_px(y), Au::from_px(100.0), Au::from_px(height)),
            kind: FragmentKind::Box {
                style: Arc::new(style),
                node: Some(node(node_index)),
                sides: VisibleSides {
                    bottom: ends,
                    ..VisibleSides::ALL
                },
            },
            children: children.into(),
        }
    }

    fn line(height: f32) -> Fragment {
        Fragment {
            rect: Rect::new(Au::ZERO, Au::ZERO, Au::from_px(100.0), Au::from_px(height)),
            kind: FragmentKind::Line { baseline: Au::ZERO },
            children: Vec::new().into(),
        }
    }

    #[test]
    fn box_ending_with_the_overflow_moves_its_parent() {
        // body (continues) > div (ends) > a line 400px tall.
        let content = block(
            1,
            0.0,
            400.0,
            false,
            vec![block(2, 0.0, 400.0, true, vec![line(400.0)])],
        );
        let bottom = Au::from_px(400.0);
        assert_eq!(
            parallel_overflow(&content, bottom),
            Some((Parallel::Moves(node(1)), Au::ZERO))
        );
        assert!(continues(&content, node(1)));
        assert!(!continues(&content, node(2)));
    }

    #[test]
    fn content_after_the_overflow_keeps_the_flow() {
        // The line is in the body itself, or the div goes on to the next page.
        let content = block(1, 0.0, 400.0, false, vec![line(400.0)]);
        assert_eq!(parallel_overflow(&content, Au::from_px(400.0)), None);
        let content = block(
            1,
            0.0,
            400.0,
            false,
            vec![block(2, 0.0, 400.0, false, vec![line(400.0)])],
        );
        assert_eq!(parallel_overflow(&content, Au::from_px(400.0)), None);
    }

    #[test]
    fn nested_boxes_ending_with_the_overflow_add_a_page_only() {
        let inner = block(3, 0.0, 400.0, true, vec![line(400.0)]);
        let content = block(
            1,
            0.0,
            400.0,
            false,
            vec![block(2, 0.0, 400.0, true, vec![inner])],
        );
        assert_eq!(
            parallel_overflow(&content, Au::from_px(400.0)),
            Some((Parallel::AddsPage, Au::ZERO))
        );
    }
}

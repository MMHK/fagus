//! `position: sticky` (CSS Positioned Layout 3, §3.4) in a scroll container: a printed page
//! shows the container scrolled to its start, where a sticky box moves to stay inside the
//! container's content box by its insets, within its containing block, as Chrome prints it
//! (Blink's `StickyPositionScrollingConstraints::ComputeStickyOffset`).
//!
//! Outside scroll containers the viewport is the scroll container, scrolled to the top: Chrome
//! keeps the window it loaded the document in ([`WINDOW`]) as the viewport of the content in the
//! flow, at the start of the first page, and the page area as that of the content of fixed
//! boxes, which it repeats on every page.

use fagus_dom::NodeId;
use fagus_style::ComputedStyle;
use fagus_style::values::{LengthPercentage, LengthPercentageAuto, Overflow, Position};

use crate::fragment::{Fragment, FragmentKind};
use crate::units::Au;
use rustc_hash::FxHashMap;

/// The edges of a rectangle in the coordinates of a fragment.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Area {
    left: Au,
    top: Au,
    right: Au,
    bottom: Au,
}

impl Area {
    /// The area in the coordinates of a child at `(x, y)`.
    fn moved(self, x: Au, y: Au) -> Self {
        Self {
            left: self.left - x,
            top: self.top - y,
            right: self.right - x,
            bottom: self.bottom - y,
        }
    }
}

/// The window Chrome prints a document from, in CSS pixels: the viewport of the sticky boxes in
/// the flow of the pages (Chrome 157, measured; the same window as that of fixed backgrounds).
pub(crate) const WINDOW: (f32, f32) = (756.0, 469.0);

/// Where the content of the pages lies in the flow of pages, laid end to end as Chrome lays
/// them out (its stitched coordinates): there the window is the viewport of the sticky boxes,
/// at the start of the first page, and a box broken across pages is their containing block
/// whole.
#[derive(Debug, Default)]
pub(crate) struct Stitched {
    /// Where the content of each page starts in the flow: the heights of the content of the
    /// pages before.
    starts: Vec<Au>,
    /// The tops and bottoms in the flow of the boxes broken across pages, by element.
    boxes: FxHashMap<NodeId, (Au, Au)>,
}

impl Stitched {
    /// The flow of `pages`: the content of each page with the top of its page area in the
    /// coordinates of the content's children.
    pub(crate) fn of<'a>(pages: impl Iterator<Item = (&'a Fragment, Au)>) -> Self {
        fn walk(fragment: &Fragment, y: Au, boxes: &mut FxHashMap<NodeId, (Au, Au)>) {
            for child in &fragment.children {
                let top = y + child.rect.y;
                if let FragmentKind::Box {
                    node: Some(node),
                    sides,
                    ..
                } = &child.kind
                    && !(sides.top && sides.bottom)
                {
                    let bottom = top + child.rect.height;
                    boxes
                        .entry(*node)
                        .and_modify(|extent| extent.1 = bottom)
                        .or_insert((top, bottom));
                }
                walk(child, top, boxes);
            }
        }
        let mut stitched = Self::default();
        let mut start = Au::ZERO;
        for (root, area_top) in pages {
            stitched.starts.push(start);
            // The content of the page from the top of its page area, in the flow.
            walk(root, start - area_top, &mut stitched.boxes);
            start += root.rect.height;
        }
        stitched
    }
}

/// A page in the flow of pages: the content of page `page` of `stitched`, whose children have
/// the top of the page area at `area_top`.
#[derive(Clone, Copy)]
pub(crate) struct OnPage<'a> {
    pub(crate) stitched: &'a Stitched,
    pub(crate) page: usize,
    pub(crate) area_top: Au,
}

impl OnPage<'_> {
    /// How far below the top of the flow the page's content starts.
    fn start(self) -> Au {
        self.stitched.starts.get(self.page).copied().unwrap_or(Au::ZERO)
    }

    /// How far below the top of the flow the content's children have their origin.
    fn origin(self) -> Au {
        self.start() - self.area_top
    }
}

/// Moves the sticky boxes below `root` that lie in scroll containers.
pub(crate) fn apply(root: &mut Fragment) {
    walk(root, None, None, None);
}

/// Moves the sticky boxes below `root`, those outside scroll containers in the viewport
/// `(x, y, width, height)`, given in the coordinates of `root`'s children. With `page`, `root`
/// is the content of a page of a flow of pages, the viewport is in the flow (on that page, the
/// window less where the page starts), and the boxes broken across pages are containing
/// blocks whole; the absolutely positioned boxes are left to [`apply_in_absolute`].
pub(crate) fn apply_in(
    root: &mut Fragment,
    (x, y, width, height): (Au, Au, Au, Au),
    page: Option<OnPage<'_>>,
) {
    let port = Area {
        left: x,
        top: y,
        right: x + width,
        bottom: y + height,
    };
    let port = page.map_or(port, |page| port.moved(Au::ZERO, page.start()));
    walk(
        root,
        Some(port),
        None,
        page.map(|page| (page, -page.origin(), true)),
    );
}

/// Moves the sticky boxes inside the absolutely positioned boxes of `root`, the content of a
/// page of a flow of pages (see [`apply_in`]), placed after the content in the flow; not those
/// inside fixed boxes, which have the page area as their viewport.
pub(crate) fn apply_in_absolute(
    root: &mut Fragment,
    (x, y, width, height): (Au, Au, Au, Au),
    page: OnPage<'_>,
) {
    fn find(fragment: &mut Fragment, port: Area, page: OnPage<'_>, origin: Au) {
        for child in &mut fragment.children {
            let (x, y) = (child.rect.x, child.rect.y);
            match &child.kind {
                FragmentKind::Box { style, .. } if style.position == Position::Fixed => {}
                FragmentKind::Box { style, .. } if style.position == Position::Absolute => {
                    walk(
                        child,
                        Some(port.moved(x, y)),
                        None,
                        Some((page, origin - y, false)),
                    );
                }
                _ => find(child, port.moved(x, y), page, origin - y),
            }
        }
    }
    let port = Area {
        left: x,
        top: y,
        right: x + width,
        bottom: y + height,
    };
    find(root, port.moved(Au::ZERO, page.start()), page, -page.origin());
}

/// Moves the sticky children of `fragment` and the sticky boxes below them: `scrollport` is the
/// content box of the nearest scroll container around, and `containing` the containing block of
/// what lies in a line or an anonymous box, both in the fragment's coordinates. With `flow`,
/// the fragment is on a page of a flow of pages, with its origin where the top of the flow
/// lies that far above it, and absolutely positioned boxes are left alone or not.
fn walk(
    fragment: &mut Fragment,
    scrollport: Option<Area>,
    containing: Option<Area>,
    flow: Option<(OnPage<'_>, Au, bool)>,
) {
    // A scroll container broken across pages grows with what it holds there: only a whole one
    // moves the sticky boxes in it.
    let (own, scrolls) = match &fragment.kind {
        FragmentKind::Box { style, sides, .. } => (
            Some(content_box(style, fragment.rect.width, fragment.rect.height)),
            (style.overflow_x == Overflow::Hidden || style.overflow_y == Overflow::Hidden)
                .then_some(sides.top && sides.bottom),
        ),
        _ => (None, None),
    };
    let scrollport = match scrolls {
        Some(true) => own,
        Some(false) => None,
        None => scrollport,
    };
    // A box broken across pages is the containing block whole, as it lies in the flow.
    let own = match (&fragment.kind, own, flow) {
        (
            FragmentKind::Box {
                node: Some(node),
                style,
                ..
            },
            Some(own),
            Some((page, origin, _)),
        ) => match page.stitched.boxes.get(node) {
            Some(&(top, bottom)) => {
                let whole = content_box(style, fragment.rect.width, bottom - top);
                let shift = top + origin;
                Some(Area {
                    top: whole.top + shift,
                    bottom: whole.bottom + shift,
                    ..own
                })
            }
            None => Some(own),
        },
        (_, own, _) => own,
    };
    let containing = own.or(containing);
    for child in &mut fragment.children {
        if let (Some((_, _, true)), FragmentKind::Box { style, .. }) = (flow, &child.kind)
            && style.position == Position::Absolute
        {
            continue;
        }
        if let (FragmentKind::Box { style, .. }, Some(port), Some(block)) =
            (&child.kind, scrollport, containing)
            && style.position == Position::Sticky
        {
            let (dx, dy) = offset(style, child, port, block);
            child.rect.x += dx;
            child.rect.y += dy;
        }
        let (x, y) = (child.rect.x, child.rect.y);
        walk(
            child,
            scrollport.map(|port| port.moved(x, y)),
            containing.map(|block| block.moved(x, y)),
            flow.map(|(page, origin, skip)| (page, origin - y, skip)),
        );
    }
}

/// The content box of a box with `style`, `width` × `height` from its border box's corner.
/// Percentages of padding resolve against its own width, which painting uses too.
fn content_box(style: &ComputedStyle, width: Au, height: Au) -> Area {
    let padding = |side: &LengthPercentage| Au::from_px(side.resolve(width.to_px()).max(0.0));
    let border = &style.border_width;
    Area {
        left: Au::from_px(border.left) + padding(&style.padding.left),
        top: Au::from_px(border.top) + padding(&style.padding.top),
        right: width - Au::from_px(border.right) - padding(&style.padding.right),
        bottom: height - Au::from_px(border.bottom) - padding(&style.padding.bottom),
    }
}

/// How far the sticky box of `fragment` with `style` moves to keep inside `port` by its insets,
/// without leaving `block`, its containing block less its margins (`auto` ones count as 0, as in
/// Blink): right before left and bottom before top, so that left and top win.
fn offset(style: &ComputedStyle, fragment: &Fragment, port: Area, block: Area) -> (Au, Au) {
    let block_width = (block.right - block.left).to_px();
    let margin = |value: &LengthPercentageAuto| Au::from_px(value.resolve_or(block_width, 0.0));
    let block = Area {
        left: block.left + margin(&style.margin.left),
        top: block.top + margin(&style.margin.top),
        right: block.right - margin(&style.margin.right),
        bottom: block.bottom - margin(&style.margin.bottom),
    };
    let inset = |value: &LengthPercentageAuto, basis: Au| match value {
        LengthPercentageAuto::Auto => None,
        LengthPercentageAuto::Length(length) => Some(Au::from_px(length.resolve(basis.to_px()))),
    };
    let (port_width, port_height) = (port.right - port.left, port.bottom - port.top);
    let mut rect = Area {
        left: fragment.rect.x,
        top: fragment.rect.y,
        right: fragment.rect.x + fragment.rect.width,
        bottom: fragment.rect.y + fragment.rect.height,
    };
    let start = rect;
    if let Some(right) = inset(&style.inset.right, port_width) {
        let delta = (port.right - right - rect.right).max(block.left - rect.left);
        if delta < Au::ZERO {
            rect = rect.moved(-delta, Au::ZERO);
        }
    }
    if let Some(left) = inset(&style.inset.left, port_width) {
        let delta = (port.left + left - rect.left).min(block.right - rect.right);
        if delta > Au::ZERO {
            rect = rect.moved(-delta, Au::ZERO);
        }
    }
    if let Some(bottom) = inset(&style.inset.bottom, port_height) {
        let delta = (port.bottom - bottom - rect.bottom).max(block.top - rect.top);
        if delta < Au::ZERO {
            rect = rect.moved(Au::ZERO, -delta);
        }
    }
    if let Some(top) = inset(&style.inset.top, port_height) {
        let delta = (port.top + top - rect.top).min(block.bottom - rect.bottom);
        if delta > Au::ZERO {
            rect = rect.moved(Au::ZERO, -delta);
        }
    }
    (rect.left - start.left, rect.top - start.top)
}

//! Positioned layout (CSS 2.1 §9.3 and §10): relative offsets and absolutely positioned boxes.
//!
//! Block and inline layout leave a placeholder where an absolutely positioned box would have been
//! in flow (its static position). Once a page is laid out, [`Layouter::position`] lays out each of
//! these boxes in its containing block, the padding box of the nearest positioned ancestor or the
//! page area, and adds it to that ancestor's fragment. The fragment tree then mirrors the
//! containing blocks, so clipping and painting order follow from it.
//!
//! Fixed boxes repeat on every page, at the place in the page area their placeholder has on the
//! page it is on, as in Chrome: one whose vertical position follows its static position (`top`
//! and `bottom` are `auto`) shows from that page on, others on every page.
//!
//! On pages, an absolutely positioned box is placed in its whole containing block, broken across
//! pages as that may be, with the page areas laid end to end, and it goes on the page its top
//! falls on; one that crosses the end of a page breaks there and continues at the top of the
//! next, and pages are added for boxes below the last, as Chrome prints them (see
//! [`PageFlow`]).

use std::sync::Arc;

use fagus_dom::NodeId;
use fagus_style::ComputedStyle;
use fagus_style::values::{Display, DisplayInside, LengthPercentageAuto, Position, Size, WritingMode};
use rustc_hash::FxHashMap;

use crate::block::{BreakToken, Consumed, LEFT, Layouter, RIGHT, Space, TOP};
use crate::fragment::{Children, Fragment, FragmentKind, VisibleSides};
use crate::inline_containing::{InlineBlocks, inline_blocks, is_inline_containing};
use crate::tree::{BoxId, BoxKind};
use crate::units::{Au, Rect};
use crate::writing::Flows;

/// The containing block of absolutely positioned descendants.
#[derive(Clone)]
struct Containing {
    /// Path of child indices from the root to the fragment the boxes are added to.
    path: Vec<usize>,
    /// The page coordinates of that fragment's border box origin.
    origin: (Au, Au),
    /// The containing block (a padding box, or the page area) in page coordinates.
    rect: Rect,
    /// The box whose fragment it is, `None` for the initial containing block.
    key: Option<ChainKey>,
    /// The widths of the box's top and bottom borders.
    borders: (Au, Au),
}

/// A box that holds absolutely positioned boxes, as its fragments on all pages know it: its
/// element and the address of its style.
pub(crate) type ChainKey = (Option<NodeId>, usize);

/// The key of the box of `fragment`, if it is a box's.
fn chain_key(fragment: &Fragment) -> Option<ChainKey> {
    match &fragment.kind {
        FragmentKind::Box { style, node, .. } => Some((*node, Arc::as_ptr(style) as usize)),
        _ => None,
    }
}

/// The fragment of a containing block on one page.
#[derive(Clone)]
pub(crate) struct ChainPart {
    pub(crate) page: usize,
    /// The path of child indices from the page's content fragment to it.
    pub(crate) path: Vec<usize>,
    /// Its border box origin in the page area.
    pub(crate) origin: (Au, Au),
    pub(crate) height: Au,
    /// Whether it holds nothing: the rest of the box's height below its content.
    pub(crate) empty: bool,
    /// How much of its height Chrome does not count in the height of the box put together (see
    /// [`slack`]).
    pub(crate) slack: Au,
}

/// The pages of a document laid end to end, which absolutely positioned boxes are placed in as
/// in Chrome, where the pages are fragments of one flow: a box is placed in its whole containing
/// block (the fragments of a box broken across pages put together, the initial containing block
/// at the start of the document), and goes where that place falls in the flow.
pub(crate) struct PageFlow {
    /// The page whose boxes are being positioned.
    pub(crate) page: usize,
    /// Where each page area starts in the flow.
    pub(crate) starts: Vec<Au>,
    /// How tall each page area is.
    pub(crate) heights: Vec<Au>,
    /// The fragments of the boxes on the pages that hold absolutely positioned boxes, by box,
    /// one per page, in order.
    pub(crate) chains: FxHashMap<ChainKey, Vec<ChainPart>>,
    /// The containing blocks broken across pages that hold an absolutely positioned box before
    /// their last page: Chrome sizes them put together without the [`slack`] of their parts.
    early: rustc_hash::FxHashSet<ChainKey>,
    /// The containing blocks of inline boxes broken across lines, in the flow.
    inline: InlineBlocks,
    /// The boxes that go on another page than the one their placeholder is on, or cross the end
    /// of their page.
    pub(crate) out: Vec<Flowing>,
    /// The anchors on each page, in its coordinates.
    pub(crate) anchors: Vec<Vec<crate::anchor::Found>>,
    /// Whether a box whose content overflows it is broken where the overflow crosses the end of
    /// a fragmentainer, and the first part of a broken box holds content, as Blink breaks them
    /// in columns (see [`block_extent`] and [`Flowing::requires_content`]).
    pub(crate) overflow_breaks: bool,
    /// Whether [`Layouter::place_absolute`] places only the boxes it finds first, not those
    /// inside them, which a later pass places in their fragments in the flow.
    pub(crate) single_round: bool,
}

impl PageFlow {
    /// Finds the containing blocks of absolutely positioned boxes on the pages whose content
    /// fragments are `contents`, which lie in their page areas, `heights` tall.
    pub(crate) fn new<'f>(contents: impl Iterator<Item = &'f Fragment>, heights: Vec<Au>) -> Self {
        let contents: Vec<&Fragment> = contents.collect();
        let mut parts = Vec::new();
        let mut anchors = Vec::new();
        let mut holders: FxHashMap<ChainKey, usize> = FxHashMap::default();
        for (page, content) in contents.iter().enumerate() {
            anchors.push(crate::anchor::anchors_on(content));
            placeholder_pages(content, page, None, &mut holders);
            chain_parts(
                content,
                page,
                &mut Vec::new(),
                (content.rect.x, content.rect.y),
                &mut parts,
            );
        }
        let mut chains: FxHashMap<ChainKey, Vec<ChainPart>> = FxHashMap::default();
        for (key, part) in parts {
            chains.entry(key).or_default().push(part);
        }
        // A box broken more than once on a page (into columns) lies in another flow.
        chains.retain(|_, parts| parts.windows(2).all(|pair| pair[0].page < pair[1].page));
        let early = chains
            .iter()
            .filter(|(key, parts)| {
                parts.len() > 1
                    && holders
                        .get(*key)
                        .is_some_and(|&page| parts.last().is_some_and(|last| page < last.page))
            })
            .map(|(key, _)| *key)
            .collect();
        let starts: Vec<Au> = heights
            .iter()
            .scan(Au::ZERO, |start, &height| {
                let this = *start;
                *start += height;
                Some(this)
            })
            .collect();
        let inline = inline_blocks(
            contents
                .iter()
                .zip(&starts)
                .map(|(content, &start)| (*content, (content.rect.x, start + content.rect.y))),
        );
        Self {
            page: 0,
            starts,
            heights,
            chains,
            early,
            inline,
            out: Vec::new(),
            anchors,
            overflow_breaks: false,
            single_round: false,
        }
    }

    /// The path to the last fragment on page `page` of inline box `key`, if it is broken across
    /// lines, and the fragment's border box origin in the page area.
    pub(crate) fn inline_last(&self, key: ChainKey, page: usize) -> Option<(&[usize], (Au, Au))> {
        let block = self.inline.get(&key)?;
        let (_, path, origin) = block.lasts.iter().find(|(root, ..)| *root == page)?;
        Some((path, (origin.0, origin.1 - self.starts.get(page).copied()?)))
    }

    /// The page whose area holds `y` in the flow (the last one past their end).
    pub(crate) fn page_at(&self, y: Au) -> usize {
        self.starts.partition_point(|&start| start <= y).saturating_sub(1)
    }
}

/// Collects the fragments below `fragment` (itself included) of boxes that hold absolutely
/// positioned boxes, on page `page`, where `fragment` is reached by `path` from the page's
/// content fragment and has its border box origin at `origin`.
pub(crate) fn chain_parts(
    fragment: &Fragment,
    page: usize,
    path: &mut Vec<usize>,
    origin: (Au, Au),
    out: &mut Vec<(ChainKey, ChainPart)>,
) {
    if let FragmentKind::Box { style, .. } = &fragment.kind
        && (style.contains_absolute() || crate::contained_fixed::contains_fixed(style))
        && let Some(key) = chain_key(fragment)
    {
        out.push((
            key,
            ChainPart {
                page,
                path: path.clone(),
                origin,
                height: fragment.rect.height,
                empty: fragment.children.is_empty(),
                slack: slack(fragment),
            },
        ));
    }
    for (index, child) in fragment.children.iter().enumerate() {
        path.push(index);
        chain_parts(
            child,
            page,
            path,
            (origin.0 + child.rect.x, origin.1 + child.rect.y),
            out,
        );
        path.pop();
    }
}

/// The room below the content of a box's fragment that goes on on the next page, where its
/// content ends with a box that also goes on there, below whose end its fragment reaches: a box
/// whose block-end padding or border did not fit below its content. Chrome counts such a box as
/// ending there on the page (Blink's consumed block size, which it does not stretch to the end of
/// the page), so that a box broken across pages that holds it is that much shorter put together,
/// though its fragments on the pages are where they are (Chrome 157, measured: `bottom: 0` lands
/// that much higher on the last page, where a box inside it is placed from an earlier page).
fn slack(fragment: &Fragment) -> Au {
    let continues =
        |fragment: &Fragment| matches!(&fragment.kind, FragmentKind::Box { sides, .. } if !sides.bottom);
    if !continues(fragment) {
        return Au::ZERO;
    }
    let mut inner = fragment;
    let mut end = fragment.rect.height;
    let mut offset = Au::ZERO;
    // Down through the last children that go on (and anonymous blocks), to the deepest.
    while let Some(last) = inner
        .children
        .iter()
        .rev()
        .find(|child| !matches!(child.kind, FragmentKind::Placeholder(_)) && !is_out_of_flow(child))
    {
        offset += last.rect.y;
        if continues(last) {
            end = end.min(offset + last.rect.height);
        } else if !matches!(last.kind, FragmentKind::Anonymous) {
            break;
        }
        inner = last;
    }
    (fragment.rect.height - end).clamp_non_negative()
}

/// Notes for each containing block (by its key) the first page on which a placeholder of a box
/// inside it lies, below `fragment` on page `page`, inside the containing block `key`.
fn placeholder_pages(
    fragment: &Fragment,
    page: usize,
    key: Option<ChainKey>,
    out: &mut FxHashMap<ChainKey, usize>,
) {
    let key = match &fragment.kind {
        FragmentKind::Placeholder(_) => {
            if let Some(key) = key {
                out.entry(key).or_insert(page);
            }
            return;
        }
        FragmentKind::Box { style, .. }
            if style.contains_absolute() || crate::contained_fixed::contains_fixed(style) =>
        {
            chain_key(fragment).or(key)
        }
        _ => key,
    };
    for child in &fragment.children {
        placeholder_pages(child, page, key, out);
    }
}

/// Whether a fragment is of an absolutely positioned or floating box.
fn is_out_of_flow(fragment: &Fragment) -> bool {
    matches!(&fragment.kind, FragmentKind::Box { style, .. } if style.is_out_of_flow() || style.is_floating())
}

/// Where a box placed in the flow of pages goes.
enum InFlow {
    /// On the page of its placeholder, positioned in its containing block's fragment there.
    Here(Fragment),
    /// On another page, or broken across pages (see [`PageFlow::out`]).
    Moved,
}

/// An absolutely positioned box laid out whole, for another page than its placeholder's or to be
/// broken across pages.
pub(crate) struct Flowing {
    pub(crate) id: BoxId,
    /// The box, `rect.x` in the page area and `rect.y` in the flow.
    pub(crate) fragment: Fragment,
    /// The width and height of its containing block.
    pub(crate) containing: (Au, Au),
    /// Its containing block, `None` for the initial one.
    pub(crate) chain: Option<ChainKey>,
    /// Whether its first part holds content even where that does not fit, as in Blink's
    /// columns (`requires_content_before_breaking`), rather than starting empty.
    pub(crate) requires_content: bool,
    /// Whether monolithic content of a part that reaches past the end of its page pushes the
    /// content after it on the next pages (see [`Layouter::break_absolute`]): on pages, not in
    /// columns, whose overflow does not reach into the next column, as in Chrome.
    pub(crate) pushes_overflow: bool,
}

/// A placeholder waiting for its box.
struct Pending {
    id: BoxId,
    /// The static position in page coordinates.
    position: (Au, Au),
    placeholder: Vec<usize>,
    containing: Containing,
    /// The innermost stacking context between the containing block and the placeholder, which
    /// paints the box (see [`crate::stacking_owner`]).
    owner: Option<ChainKey>,
}

/// The static positions of fixed boxes: the page their placeholder is on (counted from 0) and
/// its place in the page area there.
pub(crate) type FixedStatics = FxHashMap<BoxId, (usize, (Au, Au))>;

impl Layouter<'_> {
    /// Applies the relative and sticky offsets of the boxes of page `index`, whose page area is
    /// `area` in the coordinates `root` is positioned in, and records in `statics` where the
    /// fixed boxes whose placeholders are on the page have their static positions, if no page
    /// before had them. With `stitched`, the page is one of a flow of pages, where the sticky
    /// boxes have their viewport (see `crate::sticky`).
    pub(crate) fn prepare_page(
        &self,
        root: &mut Fragment,
        area: Rect,
        (index, stitched): (usize, Option<&crate::sticky::Stitched>),
        statics: &mut FixedStatics,
    ) {
        let height = definite_height(root).then_some(area.height);
        let container = Container {
            style: None,
            in_line: false,
        };
        offset_relative(root, area.width, height, self.frame(), container);
        // The window is the viewport of the sticky boxes in the flow, at the start of the
        // first page (see `crate::sticky`); on the pages after it, as far above as the pages
        // before reach.
        if (index == 0 || stitched.is_some()) && !self.frame().mode.is_vertical() {
            let (width, height) = crate::sticky::WINDOW;
            crate::sticky::apply_in(
                root,
                (
                    area.x - root.rect.x,
                    area.y - root.rect.y,
                    Au::from_px(width),
                    Au::from_px(height),
                ),
                stitched.map(|stitched| crate::sticky::OnPage {
                    stitched,
                    page: index,
                    area_top: area.y - root.rect.y,
                }),
            );
        } else {
            crate::sticky::apply(root);
        }
        let mut found = Vec::new();
        fixed_placeholders(self, root, (root.rect.x, root.rect.y), &mut found);
        for (id, (x, y)) in found {
            statics.entry(id).or_insert((index, (x - area.x, y - area.y)));
        }
    }

    /// Positions the boxes of one page, whose relative offsets [`Layouter::prepare_page`]
    /// applied: lays out the absolutely positioned boxes and, on page `fixed.0` of a document
    /// whose fixed boxes have the static positions `fixed.1`, repeats them. `area` is the page
    /// area (the initial containing block) in the coordinates `root` is positioned in. With
    /// `flow`, the boxes are placed in the flow of pages (see [`Layouter::place_absolute`]).
    pub(crate) fn position(
        &self,
        root: &mut Fragment,
        area: Rect,
        fixed: Option<(usize, &FixedStatics)>,
        flow: Option<&mut PageFlow>,
    ) {
        // Fixed boxes are repeated on every page, relative to the page area (CSS 2.1 §9.6.1).
        for &id in fixed.map_or(&[][..], |_| &self.tree.fixed) {
            let style = &self.tree.get(id).style;
            let known = fixed.and_then(|(_, statics)| statics.get(&id));
            let vertical_static = style.inset.top.is_auto() && style.inset.bottom.is_auto();
            let static_position = match known {
                Some(&(page, position))
                    if !vertical_static || fixed.is_some_and(|(index, _)| index >= page) =>
                {
                    position
                }
                // A box whose placeholder no page holds, being inside an absolutely positioned
                // box, has its static position at the corner of the page area, on every page,
                // as in Chrome (WPT `static-fixed-inside-abspos`).
                None => (Au::ZERO, Au::ZERO),
                _ => continue,
            };
            let mut fragment = self.fixed_box((id, area.width, area.height, static_position), || {
                let mut fragment = self.absolute(id, area.width, area.height, static_position);
                // The sticky boxes in it have the page area as their viewport.
                offset_relative_children_of(&mut fragment, false, self.frame(), false);
                let corner = (-fragment.rect.x, -fragment.rect.y);
                crate::sticky::apply_in(&mut fragment, (corner.0, corner.1, area.width, area.height), None);
                fragment
            });
            fragment.rect.x += area.x - root.rect.x;
            fragment.rect.y += area.y - root.rect.y;
            // Chrome places the box on every page in the flow of pages, so that what of it lies
            // above or below the page area shows on the page before or after; the page area cuts
            // off the rest.
            if let (Some(flow), Some((page, _))) = (flow.as_deref(), fixed) {
                let first = known.filter(|_| vertical_static).map_or(0, |&(page, _)| page);
                let (top, bottom) = (
                    fragment.rect.y + root.rect.y,
                    fragment.rect.y + root.rect.y + fragment.rect.height,
                );
                let start = flow.starts[page];
                let height = flow.heights[page];
                let from = flow.page_at(start - bottom).max(first);
                let to = flow.page_at(start + height - top);
                for other in (from..=to).filter(|&other| other != page).take(8) {
                    let shift = flow.starts[other] - start;
                    if top + shift < height && bottom + shift > Au::ZERO {
                        let mut copy = fragment.clone();
                        copy.rect.y += shift;
                        root.children.push(copy);
                    }
                }
            }
            root.children.push(fragment);
        }
        self.place_absolute(root, area, true, flow);
    }

    /// Applies the relative offsets inside the absolutely positioned box of `fragment`, unless
    /// the page applies them later (see [`Layouter::relative_later`]).
    fn offset_relative_inside(&self, fragment: &mut Fragment) {
        if !self.relative_later.get() {
            offset_relative_children(fragment, self.frame());
        }
    }

    /// The coordinates the boxes being laid out are in, for their relative offsets.
    fn frame(&self) -> Frame<'_> {
        Frame {
            mode: self.writing_mode(),
            flows: &self.tree.flows,
        }
    }

    /// Lays out the absolutely positioned boxes below `root`, in their containing blocks: those
    /// whose containing block is not inside `root` in `area` with `initial`, else not at all.
    /// With `flow`, `root` is the content of page `flow.page`, which lies in its page area, and a
    /// box is placed in its whole containing block in the flow of pages: one that goes on
    /// another page, or crosses the end of its page, is left to `flow.out`.
    pub(crate) fn place_absolute(
        &self,
        root: &mut Fragment,
        area: Rect,
        initial: bool,
        mut flow: Option<&mut PageFlow>,
    ) {
        // Each round places the boxes found so far; boxes inside them are found in the next.
        let rounds = if flow.as_ref().is_some_and(|flow| flow.single_round) {
            1
        } else {
            64
        };
        for _ in 0..rounds {
            let mut pending = Vec::new();
            let inline = inline_blocks(std::iter::once((&*root, (root.rect.x, root.rect.y))));
            let initial_block = Containing {
                path: Vec::new(),
                origin: (root.rect.x, root.rect.y),
                rect: area,
                key: None,
                borders: (Au::ZERO, Au::ZERO),
            };
            let origin = (root.rect.x, root.rect.y);
            collect(
                root,
                &mut Vec::new(),
                (origin, None),
                (&initial_block, &initial_block),
                (&self.tree.contained_fixed, &inline),
                (
                    self.relative_later.get().then(|| self.frame()),
                    (Au::ZERO, Au::ZERO),
                ),
                &mut pending,
            );
            if !initial {
                pending.retain(|item| !item.containing.path.is_empty());
            } else if flow.as_ref().is_some_and(|flow| flow.single_round) {
                // In columns (see `Layouter::columns_hold_absolute`), the boxes whose
                // containing block is the multi-column container and which a flex or grid
                // container aligns at their static position; the others (placed against the
                // container's box, as in Chrome) and fixed boxes go on the page.
                pending.retain(|item| !item.containing.path.is_empty() || self.aligned_in_columns(item.id));
            }
            if pending.is_empty() {
                return;
            }
            for item in pending {
                resolve(root, &item.placeholder);
                if let Some(owner) = item.owner {
                    let layout_box = self.tree.get(item.id);
                    let key = (layout_box.node, Arc::as_ptr(&layout_box.style) as usize);
                    self.stacking_owners.borrow_mut().insert(key, owner);
                }
                // Fixed boxes are placed above; running elements go to the page margins.
                let style = &self.tree.get(item.id).style;
                if style.position == Position::Fixed || style.running.is_some() {
                    continue;
                }
                let containing = &item.containing;
                // A box inside a box that clips it and lies on one page stays on that page.
                if let Some(flow) = flow.as_deref_mut()
                    && !(clips_on_path(root, &containing.path)
                        && containing
                            .key
                            .and_then(|key| flow.chains.get(&key))
                            .is_none_or(|parts| parts.len() <= 1))
                    && let Some(placed) = self.place_in_flow(&item, flow, root)
                {
                    if let InFlow::Here(fragment) = placed {
                        // The box holds the boxes positioned inside it on this page.
                        let parent = fragment_at(root, &containing.path);
                        let origin = (
                            containing.origin.0 + fragment.rect.x,
                            containing.origin.1 + fragment.rect.y,
                        );
                        let mut path = containing.path.clone();
                        path.push(parent.children.len());
                        let mut parts = Vec::new();
                        chain_parts(&fragment, flow.page, &mut path, origin, &mut parts);
                        for (key, part) in parts {
                            flow.chains.entry(key).or_default().push(part);
                        }
                        parent.children.push(fragment);
                    }
                    continue;
                }
                let static_position = (
                    item.position.0 - containing.rect.x,
                    item.position.1 - containing.rect.y,
                );
                let mut fragment = self
                    .anchored(
                        item.id,
                        root,
                        &item.placeholder,
                        (containing.rect.x, containing.rect.y),
                        (containing.rect.width, containing.rect.height),
                        static_position,
                        &[],
                    )
                    .or_else(|| {
                        self.absolute_in_grid_area(
                            item.id,
                            containing.key.and_then(|key| key.0),
                            static_position,
                            (containing.rect.width, containing.rect.height),
                        )
                    })
                    .unwrap_or_else(|| {
                        self.absolute(
                            item.id,
                            containing.rect.width,
                            containing.rect.height,
                            static_position,
                        )
                    });
                self.offset_relative_inside(&mut fragment);
                fragment.rect.x += containing.rect.x - containing.origin.0;
                fragment.rect.y += containing.rect.y - containing.origin.1;
                let parent = fragment_at(root, &containing.path);
                parent.children.push(fragment);
            }
        }
    }

    /// Lays out the box of `item` in its whole containing block in the flow of pages, and
    /// leaves it to `flow.out` unless it lies on the page of its placeholder. `None` where its
    /// containing block is not one of `flow`'s.
    fn place_in_flow(&self, item: &Pending, flow: &mut PageFlow, root: &Fragment) -> Option<InFlow> {
        let containing = &item.containing;
        let page = flow.page;
        let (width, height, static_position, top, left) = match containing.key {
            // The initial containing block is the first page's, at the start of the flow, but
            // a box placed by its static position stays on that position's page.
            None => {
                let style = &self.tree.get(item.id).style;
                // A box placed against an anchor vertically is not placed by its static position.
                let anchored = style.anchoring.as_deref().is_some_and(|anchoring| {
                    anchoring.area.is_some()
                        || anchoring.insets[0].is_some()
                        || anchoring.insets[2].is_some()
                        || anchoring.center[1]
                });
                let start = if style.inset.top.is_auto() && style.inset.bottom.is_auto() && !anchored {
                    flow.starts[page]
                } else {
                    Au::ZERO
                };
                (
                    containing.rect.width,
                    containing.rect.height,
                    (
                        item.position.0 - containing.rect.x,
                        item.position.1 - containing.rect.y,
                    ),
                    start + containing.rect.y,
                    containing.rect.x,
                )
            }
            // An inline box broken across lines, in the flow from its first line to its last.
            Some(key) if let Some(rect) = flow.inline.get(&key).map(|block| block.rect) => (
                rect.width,
                rect.height,
                (
                    item.position.0 - rect.x,
                    flow.starts[page] + item.position.1 - rect.y,
                ),
                rect.y,
                rect.x,
            ),
            // A containing block broken across pages is its fragments put together. Chrome then
            // starts it at the top of its top border, not below it, and ends it above its
            // bottom border; but where its last part is only the rest of its height below its
            // content, it starts below its top border and ends at the bottom of its bottom
            // border.
            Some(key) => {
                let parts = flow.chains.get(&key)?;
                let index = parts.iter().position(|part| part.page == page)?;
                let first = &parts[0];
                let before: Au = parts[..index]
                    .iter()
                    .map(|part| part.height)
                    .fold(Au::ZERO, |a, b| a + b);
                let slack = |part: &ChainPart| {
                    if flow.early.contains(&key) {
                        part.slack
                    } else {
                        Au::ZERO
                    }
                };
                let total: Au = parts
                    .iter()
                    .map(|part| part.height - slack(part))
                    .fold(Au::ZERO, |a, b| a + b);
                let (border_top, border_bottom) = match containing.borders {
                    borders if parts.len() == 1 => borders,
                    (top, _) if parts.last().is_some_and(|part| part.empty) => (top, Au::ZERO),
                    (_, bottom) => (Au::ZERO, bottom),
                };
                // A part with no height after the first is an empty fragment that holds boxes in
                // a column where the block has none (see `crate::column_positioned`): the boxes
                // there lie that much further along the flow, past the rest of the column before.
                let skipped: Au = (1..=index)
                    .filter(|&at| parts[at].height == Au::ZERO)
                    .map(|at| {
                        let previous = &parts[at - 1];
                        (flow.starts[parts[at].page]
                            - (flow.starts[previous.page] + previous.origin.1 + previous.height))
                            .clamp_non_negative()
                    })
                    .fold(Au::ZERO, |a, b| a + b);
                let along = before + skipped + item.position.1 - parts[index].origin.1;
                (
                    containing.rect.width,
                    (total - border_top - border_bottom).clamp_non_negative(),
                    (item.position.0 - containing.rect.x, along - border_top),
                    flow.starts[first.page] + first.origin.1 + border_top,
                    containing.rect.x,
                )
            }
        };
        // The containing block's origin in the coordinates of this page, for anchors on it.
        let origin = (left, top - flow.starts[page]);
        // Anchors on earlier pages, moved by the flow of pages to where they lie from this one.
        let elsewhere: Vec<crate::anchor::Found> = flow.anchors[..page.min(flow.anchors.len())]
            .iter()
            .enumerate()
            .flat_map(|(other, anchors)| {
                let by = flow.starts[other] - flow.starts[page];
                anchors.iter().map(move |anchor| anchor.moved(by))
            })
            .collect();
        let mut fragment = self
            .anchored(
                item.id,
                root,
                &item.placeholder,
                origin,
                (width, height),
                static_position,
                &elsewhere,
            )
            .or_else(|| {
                self.absolute_in_grid_area(
                    item.id,
                    containing.key.and_then(|key| key.0),
                    static_position,
                    (width, height),
                )
            })
            .unwrap_or_else(|| self.absolute(item.id, width, height, static_position));
        self.offset_relative_inside(&mut fragment);
        let y = top + fragment.rect.y;
        let x = left + fragment.rect.x;
        let start = flow.starts[page];
        let end = start + flow.heights[page];
        let reach = if flow.overflow_breaks {
            block_extent(&fragment)
        } else {
            fragment.rect.height
        };
        if (y >= start || page == 0) && y + reach <= end && !self.breaks_inside(item.id) {
            fragment.rect.x = x - containing.origin.0;
            fragment.rect.y = y - start - containing.origin.1;
            return Some(InFlow::Here(fragment));
        }
        fragment.rect.x = x;
        fragment.rect.y = y;
        flow.out.push(Flowing {
            id: item.id,
            fragment,
            containing: (width, height),
            chain: containing.key,
            requires_content: flow.overflow_breaks,
            pushes_overflow: false,
        });
        Some(InFlow::Moved)
    }

    /// Whether the content of box `id` breaks onto another page wherever the box is: at a forced
    /// break or a change of page name between block-level boxes in it (CSS Paged Media 3, §9.2),
    /// which break an absolutely positioned box across pages in Chrome.
    pub(crate) fn breaks_inside(&self, id: BoxId) -> bool {
        if self.measuring.get() {
            return false;
        }
        let (page, column) = self.breaks_below(id);
        page || (column && self.columns.get() > 0)
    }

    /// Whether between the block-level boxes in box `id` (or in those) lies a forced page break
    /// or a change of page name, and whether a forced column break: found once per box, as
    /// each page asks again.
    fn breaks_below(&self, id: BoxId) -> (bool, bool) {
        let BoxKind::Block { children } = &self.tree.get(id).kind else {
            return (false, false);
        };
        if let Some(&found) = self.breaks_below.borrow().get(&id) {
            return found;
        }
        let in_flow: Vec<BoxId> = children
            .iter()
            .copied()
            .filter(|&child| !self.tree.get(child).style.is_out_of_flow())
            .collect();
        let mut found = (false, false);
        for pair in in_flow.windows(2) {
            for value in [self.tree.break_after(pair[0]), self.tree.break_before(pair[1])] {
                found.0 |= value.is_forced_page_break();
                found.1 |= value == fagus_style::values::BreakBetween::Column;
            }
            found.0 |= self.tree.names_pages() && self.end_page(pair[0]) != self.start_page(pair[1]);
        }
        for &child in &in_flow {
            let below = self.breaks_below(child);
            found = (found.0 || below.0, found.1 || below.1);
        }
        self.breaks_below.borrow_mut().insert(id, found);
        found
    }

    /// The parts of the box of `flowing` on the pages from the one its top falls on: the box
    /// broken where it crosses the end of a page, as Chrome breaks it, its content that does
    /// not fit above the end moved on to the next page. `room(n)` is how far the `n`th page
    /// from that one reaches below the top of the box, then how tall its page area is. `None`
    /// for boxes that do not break (other than block containers).
    pub(crate) fn break_absolute(
        &self,
        flowing: &Flowing,
        room: impl Fn(usize) -> Au,
    ) -> Option<Vec<Fragment>> {
        let layout_box = self.tree.get(flowing.id);
        let style = &layout_box.style;
        // (Nor does a box with size containment, which is monolithic.)
        if !matches!(layout_box.kind, BoxKind::Block { .. } | BoxKind::Inline { .. })
            || self.tree.flows.content_of(flowing.id).is_some()
            || style.size_containment().0 && style.size_containment().1
        {
            return None;
        }
        let whole = &flowing.fragment;
        let (cb_width, cb_height) = flowing.containing;
        let unfragmented = Space {
            containing_height: Some(cb_height),
            ..Space::unfragmented(cb_width)
        };
        let model = self.flow_model(style, &unfragmented, None);
        let frame_x = model.border[LEFT] + model.border[RIGHT] + model.padding[LEFT] + model.padding[RIGHT];
        let content_width = (whole.rect.width - frame_x).clamp_non_negative();
        let model = self.flow_model(style, &unfragmented, Some(content_width));
        // A box stretched between its insets keeps its height, what its parts before took
        // taken off (block layout does so for a box of a fixed height).
        let stretched = !(matches!(style.height, Size::Length(_))
            || style.inset.top.is_auto()
            || style.inset.bottom.is_auto());
        let mut parts: Vec<Fragment> = Vec::new();
        let mut token: Option<BreakToken> = None;
        // How far monolithic content of the part before reaches into this page: the content
        // after it goes on below it, as in Chrome (Blink's `MonolithicOverflow` in the box's
        // break token). The part before grows to hold it, and shows on the pages it reaches
        // onto as other content overflowing its page does; a page it covers holds an empty
        // part, and the part on the page where it ends starts below it (`rect.y` tells how far
        // below the box's top: the callers place the parts after the first from there).
        let mut pushed = Au::ZERO;
        while parts.len() < 1000 {
            let first = parts.is_empty();
            let used = parts
                .iter()
                .map(|part| part.rect.height)
                .fold(Au::ZERO, |a, b| a + b);
            if !first && pushed >= room(parts.len()) {
                let height = room(parts.len()).clamp_non_negative();
                pushed -= height;
                if height == Au::ZERO {
                    break;
                }
                parts.push(Fragment {
                    rect: Rect::new(whole.rect.x, whole.rect.y, whole.rect.width, Au::ZERO),
                    kind: layout_box.fragment_kind(VisibleSides {
                        top: false,
                        bottom: false,
                        ..VisibleSides::ALL
                    }),
                    children: Children::new(),
                });
                continue;
            }
            let height_override = stretched.then(|| (whole.rect.height - used - pushed).clamp_non_negative());
            // The layout of the first part takes the box's top margin off the room, which the
            // box's place includes.
            let remaining = room(parts.len()) - pushed + if first { model.margin[TOP] } else { Au::ZERO };
            let space = Space {
                remaining: Some(remaining),
                // Content below monolithic overflow may move on to the next page.
                at_top: (!first && pushed == Au::ZERO) || flowing.requires_content,
                ..unfragmented
            };
            let outcome = self.block_container_with(
                flowing.id,
                space,
                token.as_ref(),
                Some(content_width),
                height_override,
                true,
            );
            let (mut fragment, next) = match outcome {
                Some(outcome) => (outcome.fragment, outcome.token),
                // Not even the first line fits: the box starts here, empty, and its content goes
                // on the next page.
                None if first => {
                    let room = room(0);
                    let mut start = match layout_box.kind {
                        BoxKind::Inline { .. } => BreakToken::Lines {
                            piece: 0,
                            offset: 0,
                            consumed: Consumed::default(),
                            floats: Vec::new(),
                        },
                        _ => BreakToken::at(0, None),
                    };
                    start.set_consumed((room - model.border[TOP] - model.padding[TOP]).clamp_non_negative());
                    let empty = Fragment {
                        rect: Rect::new(Au::ZERO, Au::ZERO, whole.rect.width, room.clamp_non_negative()),
                        kind: layout_box.fragment_kind(VisibleSides {
                            bottom: false,
                            ..VisibleSides::ALL
                        }),
                        children: Children::new(),
                    };
                    (empty, Some(start))
                }
                None => return None,
            };
            self.offset_relative_inside(&mut fragment);
            let below = pushed;
            let room_here = room(parts.len()) - below;
            pushed = if flowing.pushes_overflow {
                fragment
                    .children
                    .iter()
                    .filter_map(|child| crate::monolithic::content_bottom(child, Au::ZERO))
                    .max()
                    .map_or(Au::ZERO, |bottom| (bottom - room_here).clamp_non_negative())
            } else {
                Au::ZERO
            };
            if pushed > Au::ZERO {
                fragment.rect.height = fragment.rect.height.max(room_here + pushed);
            }
            fragment.rect.x = whole.rect.x;
            fragment.rect.y = whole.rect.y + below;
            parts.push(fragment);
            match next {
                Some(next) if token.as_ref() != Some(&next) || first => token = Some(next),
                _ => break,
            }
        }
        Some(parts)
    }
}

/// How far `fragment` and the content overflowing it reach down from its top: not past the end
/// of a box that clips its overflow.
pub(crate) fn block_extent(fragment: &Fragment) -> Au {
    if let FragmentKind::Box { style, .. } = &fragment.kind
        && (style.overflow_x != fagus_style::values::Overflow::Visible
            || style.overflow_y != fagus_style::values::Overflow::Visible)
    {
        return fragment.rect.height;
    }
    fragment
        .children
        .iter()
        .filter(|child| !matches!(child.kind, FragmentKind::Placeholder(_)))
        .map(|child| child.rect.y + block_extent(child))
        .fold(fragment.rect.height, Au::max)
}

/// Collects the placeholders of fixed boxes below `fragment`, whose border box origin is at
/// `origin` (page coordinates), with their positions.
fn fixed_placeholders(
    layouter: &Layouter<'_>,
    fragment: &Fragment,
    origin: (Au, Au),
    out: &mut Vec<(BoxId, (Au, Au))>,
) {
    for child in &fragment.children {
        let child_origin = (origin.0 + child.rect.x, origin.1 + child.rect.y);
        if let FragmentKind::Placeholder(id) = child.kind {
            if layouter.tree.get(id).style.position == Position::Fixed {
                out.push((id, child_origin));
            }
        } else {
            fixed_placeholders(layouter, child, child_origin, out);
        }
    }
}

/// Collects the unresolved placeholders below `fragment`, whose border box origin is at `origin`
/// (page coordinates) and which is reached from the root by `path`. `containing` is the
/// containing block of absolutely positioned boxes there, `fixed` that of the fixed boxes in
/// `contained` (see [`crate::contained_fixed`]); `inline` are the containing blocks of inline
/// boxes broken across lines.
fn collect(
    fragment: &Fragment,
    path: &mut Vec<usize>,
    (origin, owner): ((Au, Au), Option<ChainKey>),
    (containing, fixed): (&Containing, &Containing),
    (contained, inline): (&rustc_hash::FxHashSet<BoxId>, &InlineBlocks),
    (later, shift): (Option<Frame<'_>>, (Au, Au)),
    pending: &mut Vec<Pending>,
) {
    let own;
    let (containing, fixed) = match &fragment.kind {
        FragmentKind::Box { style, .. }
            if style.contains_absolute() || crate::contained_fixed::contains_fixed(style) =>
        {
            let key = chain_key(fragment);
            // An inline box broken across lines contains them from its first line to its last;
            // its last fragment holds them.
            let inline_block = key
                .filter(|_| is_inline_containing(style))
                .and_then(|key| inline.get(&key));
            let (path, origin) = match inline_block.and_then(|block| block.lasts.last()) {
                Some((_, last, origin)) => (last.clone(), *origin),
                None => (path.clone(), origin),
            };
            own = Containing {
                path,
                origin,
                rect: inline_block.map_or_else(|| padding_box(fragment, style, origin), |block| block.rect),
                key,
                borders: (
                    Au::from_px(style.border_width.top),
                    Au::from_px(style.border_width.bottom),
                ),
            };
            (
                if style.contains_absolute() {
                    &own
                } else {
                    containing
                },
                if crate::contained_fixed::contains_fixed(style) {
                    &own
                } else {
                    fixed
                },
            )
        }
        _ => (containing, fixed),
    };
    // A box that contains the absolutely positioned boxes inside it paints them; else the
    // innermost stacking context around them inside their containing block does. (Not one with
    // a `filter`, whose shadows the boxes inside would cast without their clips.)
    let owner = match &fragment.kind {
        FragmentKind::Box { style, .. } if style.contains_absolute() => None,
        FragmentKind::Box { style, .. } if style.is_stacking_context() && style.filter.is_empty() => {
            chain_key(fragment)
        }
        _ => owner,
    };
    // Where relative offsets are applied later (see `Layouter::relative_later`), those of the
    // boxes between a fixed box and the box that contains it, which do not contain it, move its
    // static position all the same: they are added up from that box down.
    let shift = match &fragment.kind {
        FragmentKind::Box { style, .. } if crate::contained_fixed::contains_fixed(style) => {
            (Au::ZERO, Au::ZERO)
        }
        _ => shift,
    };
    for (index, child) in fragment.children.iter().enumerate() {
        let child_origin = (origin.0 + child.rect.x, origin.1 + child.rect.y);
        let child_shift = match (&child.kind, later) {
            (FragmentKind::Box { style, .. }, Some(frame)) if style.position == Position::Relative => {
                let (width, container) = match &fragment.kind {
                    FragmentKind::Box { style, .. } => (
                        (fragment.rect.width
                            - Au::from_px(style.border_width.left + style.border_width.right))
                        .clamp_non_negative(),
                        Some(frame.flows.physical_of(style)),
                    ),
                    _ => (fragment.rect.width, None),
                };
                let (dx, dy) = relative_offset(style, (width, None), frame, (container, false));
                (shift.0 + dx, shift.1 + dy)
            }
            _ => shift,
        };
        path.push(index);
        if let FragmentKind::Placeholder(id) = child.kind {
            let fixed_box = contained.contains(&id);
            pending.push(Pending {
                id,
                position: if fixed_box {
                    (child_origin.0 + shift.0, child_origin.1 + shift.1)
                } else {
                    child_origin
                },
                placeholder: path.clone(),
                containing: if fixed_box { fixed } else { containing }.clone(),
                owner: owner.filter(|_| !fixed_box),
            });
        } else {
            collect(
                child,
                path,
                (child_origin, owner),
                (containing, fixed),
                (contained, inline),
                (later, child_shift),
                pending,
            );
        }
        path.pop();
    }
}

/// The padding box of a fragment in page coordinates.
fn padding_box(fragment: &Fragment, style: &ComputedStyle, origin: (Au, Au)) -> Rect {
    let border = |width: f32| Au::from_px(width);
    let left = border(style.border_width.left);
    let top = border(style.border_width.top);
    Rect::new(
        origin.0 + left,
        origin.1 + top,
        (fragment.rect.width - left - border(style.border_width.right)).clamp_non_negative(),
        (fragment.rect.height - top - border(style.border_width.bottom)).clamp_non_negative(),
    )
}

/// Whether a box on `path` from `root` (the end of it included) clips its overflow, and with it
/// the absolutely positioned boxes inside the box at its end, which then add no pages, as in
/// Chrome.
fn clips_on_path(root: &Fragment, path: &[usize]) -> bool {
    let mut fragment = root;
    for &index in path {
        let Some(child) = fragment.children.get(index) else {
            return false;
        };
        fragment = child;
        if let FragmentKind::Box { style, .. } = &fragment.kind
            && crate::column_positioned::clips_block_overflow(style)
        {
            return true;
        }
    }
    false
}

fn fragment_at<'f>(root: &'f mut Fragment, path: &[usize]) -> &'f mut Fragment {
    path.iter()
        .fold(root, |fragment, &index| &mut fragment.children[index])
}

/// Turns a handled placeholder into an empty fragment.
fn resolve(root: &mut Fragment, path: &[usize]) {
    fragment_at(root, path).kind = FragmentKind::Anonymous;
}

/// The coordinates fragments are in where relative offsets apply to them: those of writing mode
/// `mode` (see [`crate::writing`]), whose fragments may carry styles turned for it.
#[derive(Clone, Copy)]
pub(crate) struct Frame<'f> {
    pub(crate) mode: WritingMode,
    pub(crate) flows: &'f Flows,
}

/// The containing block of the boxes in a fragment, for their relative offsets: its style as
/// written (the initial containing block: `None`), and whether the fragment is content of a line.
#[derive(Clone, Copy)]
struct Container<'s> {
    style: Option<&'s ComputedStyle>,
    in_line: bool,
}

/// Applies `position: relative` to the descendants of `fragment`, whose content box is
/// `width` × `height` (CSS 2.1 §9.4.3), its height `None` where it is not definite: percentages
/// of it are then `auto`, as in Blink. The offsets do not change the layout of anything else.
fn offset_relative(
    fragment: &mut Fragment,
    width: Au,
    height: Option<Au>,
    frame: Frame<'_>,
    container: Container<'_>,
) {
    // The items of a flex or grid container have the heights it gives them.
    let items = matches!(
        &fragment.kind,
        FragmentKind::Box { style, .. } if matches!(
            style.display,
            Display::Normal { inside: DisplayInside::Flex | DisplayInside::Grid, .. }
        )
    );
    let in_line = crate::writing::holds_line_content(fragment, container.in_line);
    let style = match &fragment.kind {
        FragmentKind::Box { style, .. } => Some(frame.flows.physical_of(style)),
        _ => container.style,
    };
    for child in &mut fragment.children {
        if let FragmentKind::Box {
            style: child_style, ..
        } = &child.kind
            && child_style.position == Position::Relative
        {
            let (dx, dy) = relative_offset(child_style, (width, height), frame, (style, in_line));
            child.rect.x += dx;
            child.rect.y += dy;
        }
        // Lines, the inline boxes on them and anonymous blocks leave the containing block of
        // what they hold to the block container, as in Blink.
        let inline = match &child.kind {
            FragmentKind::Line { .. } | FragmentKind::Anonymous => true,
            FragmentKind::Box { style, node, .. } => {
                style.display == Display::INLINE
                    || (node.is_none()
                        && style.display == Display::BLOCK
                        && matches!(style.height, Size::Auto))
            }
            _ => false,
        };
        if inline {
            offset_relative(child, width, height, frame, Container { style, in_line });
        } else {
            offset_relative_children_of(child, items, frame, in_line);
        }
    }
}

/// The offsets of a relatively positioned box with `style` (as laid out in `frame`) in a
/// containing block `width` × `height` of the frame (its height `None` where it is not
/// definite), in the coordinates of the frame. The offsets are those of the style as written:
/// where both of a pair are given, the one on the start side of the containing block wins, in
/// its writing mode and direction (`container`, with whether the box is content of a line).
fn relative_offset(
    style: &ComputedStyle,
    (width, height): (Au, Option<Au>),
    frame: Frame<'_>,
    (container, in_line): (Option<&ComputedStyle>, bool),
) -> (Au, Au) {
    let physical = frame.flows.physical_of(style);
    let mode = frame.mode;
    // The size of the containing block on the page.
    let (page_width, page_height) = if mode.is_vertical() {
        (height, Some(width))
    } else {
        (Some(width), height)
    };
    let percent = |value: &LengthPercentageAuto| matches!(value, LengthPercentageAuto::Length(length) if length.fixed().is_none());
    let resolve = |value: &LengthPercentageAuto, basis: Option<Au>| match basis {
        Some(basis) => resolve_inset(value, basis),
        None if percent(value) => None,
        None => resolve_inset(value, Au::ZERO),
    };
    let (container_mode, rtl) = container.map_or((WritingMode::HorizontalTb, false), |style| {
        (style.writing_mode, style.is_rtl())
    });
    // Whether the right inset wins over the left one, and the bottom one over the top one.
    let right_first = match container_mode {
        WritingMode::HorizontalTb => rtl,
        WritingMode::VerticalRl | WritingMode::SidewaysRl => true,
        WritingMode::VerticalLr | WritingMode::SidewaysLr => false,
    };
    let bottom_first = match container_mode {
        WritingMode::HorizontalTb => false,
        WritingMode::SidewaysLr => !rtl,
        _ => rtl,
    };
    let inset = &physical.inset;
    let pair =
        |start: &LengthPercentageAuto, end: &LengthPercentageAuto, basis: Option<Au>, end_first: bool| {
            let start = || resolve(start, basis);
            let end = || resolve(end, basis).map(|end| -end);
            if end_first {
                end().or_else(start)
            } else {
                start().or_else(end)
            }
            .unwrap_or(Au::ZERO)
        };
    let dx = pair(&inset.left, &inset.right, page_width, right_first);
    let dy = pair(&inset.top, &inset.bottom, page_height, bottom_first);
    // The offsets on the page in the frame's coordinates (see `crate::writing::physical_rect`).
    match mode {
        WritingMode::HorizontalTb => (dx, dy),
        WritingMode::VerticalRl | WritingMode::SidewaysRl => (dy, -dx),
        WritingMode::VerticalLr if in_line => (dy, -dx),
        WritingMode::VerticalLr => (dy, dx),
        WritingMode::SidewaysLr => (-dy, dx),
    }
}

/// [`offset_relative`] for the children of `fragment`, with its content box as the basis; then
/// moves the sticky boxes in it (see `crate::sticky`).
fn offset_relative_children(fragment: &mut Fragment, frame: Frame<'_>) {
    offset_relative_children_of(fragment, false, frame, false);
    crate::sticky::apply(fragment);
}

/// [`offset_relative_children`] for a fragment that is a flex or grid item (`item`) or not, and
/// content of a line (`in_line`) or not.
fn offset_relative_children_of(fragment: &mut Fragment, item: bool, frame: Frame<'_>, in_line: bool) {
    let (width, height) = match &fragment.kind {
        FragmentKind::Box { style, .. } => {
            let frame = |a: f32, b: f32| Au::from_px(a + b);
            (
                (fragment.rect.width - frame(style.border_width.left, style.border_width.right))
                    .clamp_non_negative(),
                (fragment.rect.height - frame(style.border_width.top, style.border_width.bottom))
                    .clamp_non_negative(),
            )
        }
        _ => (fragment.rect.width, fragment.rect.height),
    };
    let container = Container { style: None, in_line };
    // In a table, a row group or a row, the percentages of `top` and `bottom` of the parts
    // inside resolve against its own `height` where that is a length, and are `auto` without
    // one, whatever height the table gives it (as in Blink).
    if let FragmentKind::Box { style, .. } = &fragment.kind
        && is_table_box(style)
    {
        let basis = match &style.height {
            Size::Length(length) => length.fixed().map(Au::from_px),
            _ => None,
        };
        offset_relative(fragment, width, basis, frame, container);
        return;
    }
    let definite = item || definite_height(fragment);
    offset_relative(fragment, width, definite.then_some(height), frame, container);
}

/// Whether a box is a table, a row group or a row.
fn is_table_box(style: &ComputedStyle) -> bool {
    use fagus_style::values::InternalDisplay;
    matches!(
        style.display,
        Display::Normal {
            inside: DisplayInside::Table,
            ..
        } | Display::Internal(
            InternalDisplay::TableRowGroup
                | InternalDisplay::TableHeaderGroup
                | InternalDisplay::TableFooterGroup
                | InternalDisplay::TableRow
        )
    )
}

/// Whether the height of the box of `fragment` is definite for the percentages of its
/// children: not where its `height` is `auto` and its content gives it, as for a block
/// container in flow or one positioned without both `top` and `bottom` (Blink then resolves
/// the percentages of `top` and `bottom` to `auto`).
fn definite_height(fragment: &Fragment) -> bool {
    let FragmentKind::Box { style, .. } = &fragment.kind else {
        return true;
    };
    let block_container = matches!(
        style.display,
        Display::Normal {
            inside: DisplayInside::Flow | DisplayInside::FlowRoot,
            ..
        }
    );
    let stretched = style.is_out_of_flow() && !style.inset.top.is_auto() && !style.inset.bottom.is_auto();
    !matches!(style.height, Size::Auto) || !block_container || stretched
}

/// A length of `inset` or `margin`, resolved against `basis`; `None` for `auto`.
fn resolve_inset(value: &LengthPercentageAuto, basis: Au) -> Option<Au> {
    match value {
        LengthPercentageAuto::Auto => None,
        LengthPercentageAuto::Length(length) => Some(Au::from_px(length.resolve(basis.to_px()))),
    }
}

/// How far the box of the root element moves from where the flow of the pages puts it, at
/// `rect` in a page area (the initial containing block) `width` × `height` wide: by its offsets
/// where it is relatively positioned, to them where it is absolutely positioned or fixed, its
/// containing block being the initial one (CSS 2.1 §9.3.1 and §10.1), as Chrome places it. Such
/// a root moves down on the first page only (`first`), where its content starts that far below
/// the top of the page area.
pub(crate) fn root_offset(
    style: &ComputedStyle,
    rect: Rect,
    (width, height): (Au, Au),
    first: bool,
) -> (Au, Au) {
    let inset = &style.inset;
    match style.position {
        Position::Relative => {
            let dx = resolve_inset(&inset.left, width)
                .or_else(|| resolve_inset(&inset.right, width).map(|right| -right))
                .unwrap_or(Au::ZERO);
            let dy = resolve_inset(&inset.top, height)
                .or_else(|| resolve_inset(&inset.bottom, height).map(|bottom| -bottom))
                .unwrap_or(Au::ZERO);
            (dx, dy)
        }
        Position::Absolute | Position::Fixed => {
            // The flow puts the margin box at the corner of the page area, the static position.
            let margin = |value: &LengthPercentageAuto| resolve_inset(value, width).unwrap_or(Au::ZERO);
            let dx = resolve_inset(&inset.left, width)
                .or_else(|| {
                    resolve_inset(&inset.right, width)
                        .map(|right| width - right - margin(&style.margin.right) - rect.x - rect.width)
                })
                .unwrap_or(Au::ZERO);
            let dy = resolve_inset(&inset.top, height)
                .or_else(|| {
                    resolve_inset(&inset.bottom, height)
                        .map(|bottom| height - bottom - margin(&style.margin.bottom) - rect.y - rect.height)
                })
                .filter(|_| first)
                .unwrap_or(Au::ZERO);
            (dx, dy)
        }
        _ => (Au::ZERO, Au::ZERO),
    }
}

/// The `top` of an absolutely positioned or fixed root element, resolved against the height of
/// the initial containing block, where it is positive (see [`root_offset`]).
pub(crate) fn root_top(style: &ComputedStyle, height: Au) -> Option<Au> {
    matches!(style.position, Position::Absolute | Position::Fixed)
        .then(|| resolve_inset(&style.inset.top, height))
        .flatten()
        .filter(|&top| top > Au::ZERO)
}

/// Whether the root element with `style` is absolutely positioned or fixed with an automatic
/// height and both a `top` and a `bottom`, in a horizontal writing mode: its height is what they
/// leave of the initial containing block (CSS 2.1 §10.6.4), as in Chrome.
pub(crate) fn root_fills_height(style: &ComputedStyle) -> bool {
    matches!(style.position, Position::Absolute | Position::Fixed)
        && matches!(style.height, Size::Auto)
        && !style.inset.top.is_auto()
        && !style.inset.bottom.is_auto()
        && !style.writing_mode.is_vertical()
}

/// The height of the containing block that the root element with `style`, which
/// [`root_fills_height`] gave `height: stretch`, fills on a page area `height` high: the area
/// less its `top` and `bottom`.
pub(crate) fn root_block_room(style: &ComputedStyle, height: Au) -> Option<Au> {
    if !matches!(style.position, Position::Absolute | Position::Fixed)
        || !matches!(style.height, Size::Stretch)
    {
        return None;
    }
    let top = resolve_inset(&style.inset.top, height)?;
    let bottom = resolve_inset(&style.inset.bottom, height)?;
    Some((height - top - bottom).clamp_non_negative())
}

/// The room across the page area of `inline` that the root element with `style` takes: less
/// its `left` and `right` where it is absolutely positioned or fixed with both and an automatic
/// width, which fills what they leave (CSS 2.1 §10.3.7), in horizontal writing modes.
pub(crate) fn root_inline_room(style: &ComputedStyle, inline: Au, vertical: bool) -> Au {
    if vertical
        || !matches!(style.position, Position::Absolute | Position::Fixed)
        || !matches!(style.width, Size::Auto)
    {
        return inline;
    }
    match (
        resolve_inset(&style.inset.left, inline),
        resolve_inset(&style.inset.right, inline),
    ) {
        (Some(left), Some(right)) => (inline - left - right).clamp_non_negative(),
        _ => inline,
    }
}

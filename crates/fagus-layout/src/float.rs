//! Floats (CSS 2.1 §9.5): the space they take from the lines and boxes of their block formatting
//! context.
//!
//! Each block formatting context has an exclusion space: the margin boxes of the floats placed in
//! it so far, in coordinates whose origin is the border box corner of the box that established
//! the context. The layouter keeps one per context it is inside of; layout that is thrown away (a
//! box that moves to the next page, lines past the end of a page) removes its floats again.
//!
//! Floats break across pages and columns like other boxes (CSS Fragmentation 3, §2): a float
//! that reaches past the end of the page is laid out up to it, and one that does not fit at all
//! moves to the next page, while the content beside and after it goes on. Their break tokens
//! are parallel flows, kept by the root of the formatting context, which places them first on
//! the next page, at the top of its content.

use std::sync::Arc;

use fagus_style::values::{Clear, Float};

use crate::block::{Atomic, BreakToken, Layouter, Space};
use crate::fragment::{Fragment, FragmentKind};
use crate::shape::{FloatBox, FloatShape};
use crate::tree::{BoxId, BoxKind};
use crate::units::{Au, Rect};

/// A float in an exclusion space.
#[derive(Clone, Debug)]
struct Placed {
    side: Float,
    /// The margin box.
    rect: Rect,
    /// Where the float continues on the next page, if it broke or moved there.
    token: Option<FloatToken>,
    /// The float area that lines wrap around, if it is not the margin box (`shape-outside`).
    shape: Option<Arc<FloatShape>>,
    /// Whether this is the box of an initial letter, which `clear` does not move boxes below
    /// but floats and blocks after it go below (Blink's `ExclusionArea::IsForInitialLetterBox`).
    letter: bool,
}

impl Placed {
    /// Whether the float takes room on this page: not where it only stands for a float that
    /// moved on to the next.
    fn takes_room(&self) -> bool {
        self.rect.height > Au::ZERO || self.token.is_none()
    }
}

/// A float that continues on the next page or column.
#[derive(Clone, Debug, PartialEq)]
pub struct FloatToken {
    pub(crate) id: BoxId,
    pub(crate) side: Float,
    /// Where it continues inside; `None` for a float that did not fit and starts again.
    pub(crate) inner: Option<Box<BreakToken>>,
    /// The edges of its containing block's content box in the formatting context.
    pub(crate) edges: (Au, Au),
    /// The height its percentage heights refer to (see [`Layouter::float_part`]).
    pub(crate) containing_height: Option<Au>,
    /// How far monolithic content of its part on the page before reaches into the next page,
    /// below which its content goes on, as in Chrome (Blink's `MonolithicOverflow`), with where
    /// that part's box lies in its margin box: its left and width, and the margin box's width.
    /// A page that the overflow covers holds an empty part, which takes room beside the
    /// content there but shows nothing (the part before, grown to hold the overflow, shows).
    pub(crate) pushed: (Au, (Au, Au, Au)),
}

/// Where the content laid out in a formatting context has to end on the page: `limit` in its
/// coordinates, and `top`, where content starts at the very top of the page, if it does (see
/// [`crate::block::Space::flush`]).
#[derive(Clone, Copy, Debug)]
pub(crate) struct FlowLimit {
    pub(crate) limit: Au,
    pub(crate) top: Option<Au>,
}

/// The floats of one block formatting context.
#[derive(Debug, Default)]
pub(crate) struct Exclusions {
    floats: Vec<Placed>,
    /// The end of the page or column for the content being laid out; `None` without
    /// fragmentation.
    limit: Option<FlowLimit>,
}

impl Exclusions {
    /// The free part of `[left, right)` beside the floats that overlap the band from `top` to
    /// `top + height`. A float of no height takes room from the bands it lies inside (not those
    /// that start or end where it lies), as in Blink's exclusion space; one that moved on to the
    /// next page (which stands for it here) takes none.
    fn free(&self, left: Au, right: Au, top: Au, height: Au) -> (Au, Au) {
        let (start, end) = self.free_signed(left, right, top, height);
        (start, end.max(start))
    }

    /// [`Exclusions::free`] where floats on both sides may leave less than nothing: the end
    /// before the start.
    fn free_signed(&self, left: Au, right: Au, top: Au, height: Au) -> (Au, Au) {
        let bottom = top + height.max(Au::EPSILON);
        let (mut start, mut end) = (left, right);
        for float in self.floats.iter().filter(|float| float.takes_room()) {
            if float.rect.y < bottom && float.rect.y + float.rect.height > top {
                match float.side {
                    Float::Left => start = start.max(float.rect.x + float.rect.width),
                    Float::Right => end = end.min(float.rect.x),
                    Float::None => {}
                }
            }
        }
        (start, end)
    }

    /// The free part of `[left, right)` for a line box from `top` to `top + height`: beside the
    /// float areas of the floats whose margin boxes it overlaps, the extent of their shapes
    /// over its height where they have one (CSS Shapes 1, §6.1).
    fn line_free(&self, left: Au, right: Au, top: Au, height: Au) -> (Au, Au) {
        self.line_free_among(self.floats.len(), left, right, top, height)
    }

    /// [`Self::line_free`] beside the first `count` floats alone.
    fn line_free_among(&self, count: usize, left: Au, right: Au, top: Au, height: Au) -> (Au, Au) {
        let bottom = top + height.max(Au::EPSILON);
        let (mut start, mut end) = (left, right);
        for float in self.floats.iter().take(count) {
            if !float.takes_room() || float.rect.y >= bottom || float.rect.y + float.rect.height <= top {
                continue;
            }
            let (from, to) = match &float.shape {
                Some(shape) => {
                    let Some((from, to)) = shape.excluded((top - float.rect.y).to_px(), height.to_px())
                    else {
                        continue;
                    };
                    (float.rect.x + Au::from_px(from), float.rect.x + Au::from_px(to))
                }
                None => (float.rect.x, float.rect.x + float.rect.width),
            };
            match float.side {
                Float::Left => start = start.max(to),
                Float::Right => end = end.min(from),
                Float::None => {}
            }
        }
        (start, end.max(start))
    }

    /// The bottom of the initial letters on `side` (both sides for `None`), if there are any.
    fn letter_clearance(&self, side: Option<Float>) -> Option<Au> {
        self.floats
            .iter()
            .filter(|float| float.letter && side.is_none_or(|side| float.side == side))
            .map(|float| float.rect.y + float.rect.height)
            .max()
    }

    /// The floats `clear` refers to (initial letters are not among them).
    fn cleared(&self, clear: Clear) -> impl Iterator<Item = &Placed> {
        self.floats
            .iter()
            .filter(|float| !float.letter)
            .filter(move |float| match clear {
                Clear::Left => float.side == Float::Left,
                Clear::Right => float.side == Float::Right,
                Clear::Both => true,
                Clear::None => false,
            })
    }

    /// The lowest bottom edge of the floats `clear` refers to.
    fn clearance(&self, clear: Clear) -> Option<Au> {
        self.cleared(clear)
            .map(|float| float.rect.y + float.rect.height)
            .max()
    }

    /// The nearest bottom edge of a float below `top`.
    fn next_edge(&self, top: Au) -> Option<Au> {
        self.floats
            .iter()
            .map(|float| float.rect.y + float.rect.height)
            .filter(|&bottom| bottom > top)
            .min()
    }

    /// Whether a float with a shape reaches below `top`.
    fn shapes_below(&self, top: Au) -> bool {
        self.floats.iter().any(|float| {
            float.shape.is_some() && float.takes_room() && float.rect.y + float.rect.height > top
        })
    }

    /// Where a line that does not fit at `top` tries next: 1px lower while the float area of a
    /// float with a shape reaches below `top`, which the line may fit beside further down, as
    /// in Blink (`InlineLayoutAlgorithm::Layout` steps the block delta of an opportunity with
    /// shapes by 1px), and not past the next bottom edge of a float; else at that edge.
    fn next_line_top(&self, top: Au) -> Option<Au> {
        let edge = self.next_edge(top);
        if self.shapes_below(top) {
            let step = top + Au::from_px(1.0);
            return Some(edge.map_or(step, |edge| edge.min(step)));
        }
        edge
    }

    /// Places a float with a margin box of `width` × `height` as high as possible from `top`,
    /// as far left or right as possible in `[left, right)` (CSS 2.1 §9.5.1).
    fn place(&mut self, side: Float, width: Au, height: Au, top: Au, left: Au, right: Au) -> Rect {
        // Not above an earlier float, and below the initial letters on its side.
        let mut top = self
            .floats
            .iter()
            .rfind(|float| !float.letter)
            .map_or(top, |float| top.max(float.rect.y));
        if let Some(bottom) = self.letter_clearance(Some(side)) {
            top = top.max(bottom);
        }
        loop {
            let (start, end) = self.free(left, right, top, height);
            // Too narrow here: below the next float that ends, unless none does, or none takes
            // room from the containing block here (a float wider than it stays, as in Chrome).
            if end - start < width
                && (start > left || end < right)
                && let Some(edge) = self.next_edge(top)
            {
                top = edge;
                continue;
            }
            let x = if side == Float::Right { end - width } else { start };
            let rect = Rect::new(x, top, width, height);
            self.floats.push(Placed {
                side,
                rect,
                token: None,
                shape: None,
                letter: false,
            });
            return rect;
        }
    }
}

impl Layouter<'_> {
    /// Starts a block formatting context.
    pub(crate) fn push_context(&self) {
        self.contexts.borrow_mut().push(Exclusions::default());
    }

    pub(crate) fn pop_context(&self) {
        self.contexts.borrow_mut().pop();
    }

    /// How many floats the current context holds; with [`Layouter::truncate_floats`], layout that
    /// is thrown away takes its floats back.
    pub(crate) fn float_count(&self) -> usize {
        self.contexts
            .borrow()
            .last()
            .map_or(0, |context| context.floats.len())
    }

    pub(crate) fn truncate_floats(&self, count: usize) {
        if let Some(context) = self.contexts.borrow_mut().last_mut() {
            context.floats.truncate(count);
        }
    }

    /// The free part of `[left, right)` in the band from `top` to `top + height` (context
    /// coordinates).
    pub(crate) fn free_range(&self, left: Au, right: Au, top: Au, height: Au) -> (Au, Au) {
        match self.contexts.borrow().last() {
            Some(context) if !context.floats.is_empty() => context.free(left, right, top, height),
            _ => (left, right),
        }
    }

    /// Whether floats on both sides overlap each other's room in the band from `top` to
    /// `top + height` (context coordinates), so that not even a box of no width fits beside
    /// them, as Blink drops layout opportunities of negative size.
    pub(crate) fn floats_close_band(&self, left: Au, right: Au, top: Au, height: Au) -> bool {
        match self.contexts.borrow().last() {
            Some(context) if !context.floats.is_empty() => {
                let (start, end) = context.free_signed(left, right, top, height);
                end < start
            }
            _ => false,
        }
    }

    /// The free part of `[left, right)` for a line box from `top` to `top + height` (context
    /// coordinates), beside the float areas of the floats.
    pub(crate) fn line_range(&self, left: Au, right: Au, top: Au, height: Au) -> (Au, Au) {
        match self.contexts.borrow().last() {
            Some(context) if !context.floats.is_empty() => context.line_free(left, right, top, height),
            _ => (left, right),
        }
    }

    /// The free part of `[left, right)` for a line box beside the first `count` floats of the
    /// current context, those placed before it (see [`Self::float_count`]).
    pub(crate) fn line_range_among(
        &self,
        count: usize,
        left: Au,
        right: Au,
        top: Au,
        height: Au,
    ) -> (Au, Au) {
        match self.contexts.borrow().last() {
            Some(context) if count > 0 => context.line_free_among(count, left, right, top, height),
            _ => (left, right),
        }
    }

    /// Places a float in the current context; returns its margin box.
    pub(crate) fn place_float(&self, side: Float, size: (Au, Au), top: Au, left: Au, right: Au) -> Rect {
        let mut contexts = self.contexts.borrow_mut();
        match contexts.last_mut() {
            Some(context) => context.place(side, size.0, size.1, top, left, right),
            None => Rect::new(left, top, size.0, size.1),
        }
    }

    /// Adds the margin box `rect` of an initial letter on `side` to the current context: it
    /// stands where its line starts, beside the floats there, not where a float would go (Blink
    /// adds an exclusion for it after laying out the line).
    pub(crate) fn place_initial_letter(&self, side: Float, rect: Rect) {
        if let Some(context) = self.contexts.borrow_mut().last_mut() {
            context.floats.push(Placed {
                side,
                rect,
                token: None,
                shape: None,
                letter: true,
            });
        }
    }

    /// Moves the initial letter added last to the current context down by `by`, with the line
    /// that ruby annotations move down (Blink's `ComputeInitialLetterBoxBlockStartAdjustment`).
    pub(crate) fn move_initial_letter(&self, by: Au) {
        if let Some(context) = self.contexts.borrow_mut().last_mut()
            && let Some(letter) = context.floats.iter_mut().rev().find(|placed| placed.letter)
        {
            letter.rect.y += by;
        }
    }

    /// The bottom of the initial letters of the current context on the sides `clear` names:
    /// new formatting contexts after them go below them all and a formatting context contains
    /// them, as in Blink (`ClearanceOffsetIncludingInitialLetter`); a paragraph that starts
    /// with an initial letter goes below them all, others below those their `clear` names.
    pub(crate) fn letter_clearance(&self, clear: Clear) -> Option<Au> {
        let side = match clear {
            Clear::None => return None,
            Clear::Left => Some(Float::Left),
            Clear::Right => Some(Float::Right),
            Clear::Both => None,
        };
        self.contexts
            .borrow()
            .last()
            .and_then(|context| context.letter_clearance(side))
    }

    /// The bottom of the floats that `clear` moves a box below, if there are any.
    pub(crate) fn clearance(&self, clear: Clear) -> Option<Au> {
        self.contexts
            .borrow()
            .last()
            .and_then(|context| context.clearance(clear))
    }

    /// Whether a float that `clear` moves a box below goes on to the next page (broken, or
    /// moved there whole): the box then starts on the next page too, below the rest of the
    /// float (Blink's `ExclusionSpace::NeedsClearancePastFragmentainer`).
    pub(crate) fn clears_past_page(&self, clear: Clear) -> bool {
        self.contexts
            .borrow()
            .last()
            .is_some_and(|context| context.cleared(clear).any(|float| float.token.is_some()))
    }

    /// The nearest bottom edge of a float below `top`.
    pub(crate) fn next_float_edge(&self, top: Au) -> Option<Au> {
        self.contexts
            .borrow()
            .last()
            .and_then(|context| context.next_edge(top))
    }

    /// Where a line that does not fit at `top` (context coordinates) tries next (see
    /// [`Exclusions::next_line_top`]).
    pub(crate) fn next_line_top(&self, top: Au) -> Option<Au> {
        self.contexts
            .borrow()
            .last()
            .and_then(|context| context.next_line_top(top))
    }

    /// Whether a float with a shape in the current context reaches below `top`.
    pub(crate) fn shapes_below(&self, top: Au) -> bool {
        self.contexts
            .borrow()
            .last()
            .is_some_and(|context| context.shapes_below(top))
    }

    /// Whether the current context holds floats.
    pub(crate) fn has_floats(&self) -> bool {
        self.contexts
            .borrow()
            .last()
            .is_some_and(|context| !context.floats.is_empty())
    }

    /// Where content of the current context ends on the page, if it does.
    pub(crate) fn flow_limit(&self) -> Option<Au> {
        self.contexts
            .borrow()
            .last()
            .and_then(|context| context.limit)
            .map(|limit| limit.limit)
    }

    /// Sets where content of the current context ends on the page; returns what was set
    /// before.
    pub(crate) fn set_flow_limit(&self, limit: Option<FlowLimit>) -> Option<FlowLimit> {
        self.contexts
            .borrow_mut()
            .last_mut()
            .and_then(|context| std::mem::replace(&mut context.limit, limit))
    }

    /// Whether a float of the current context reaches past `limit`, or is broken there and goes
    /// on: content beside such floats stays on the page beside them, while below floats that
    /// all end there it goes on to the next page.
    pub(crate) fn floats_reach_past(&self, limit: Au) -> bool {
        self.contexts.borrow().last().is_some_and(|context| {
            context.floats.iter().any(|float| {
                float.rect.y + float.rect.height > limit
                    || float.token.as_ref().is_some_and(|token| token.inner.is_some())
            })
        })
    }

    /// Whether floats of the current context continue on the next page.
    pub(crate) fn floats_continue(&self) -> bool {
        self.contexts
            .borrow()
            .last()
            .is_some_and(|context| context.floats.iter().any(|float| float.token.is_some()))
    }

    /// The break tokens of the floats of the current context that continue on the next page,
    /// taken out of it.
    pub(crate) fn take_float_tokens(&self) -> Vec<FloatToken> {
        self.contexts
            .borrow_mut()
            .last_mut()
            .map_or_else(Vec::new, |context| {
                context
                    .floats
                    .iter_mut()
                    .filter_map(|float| float.token.take())
                    .collect()
            })
    }

    /// Places float `id`, laid out unbroken as `fragment` with a margin box of `size`, as high
    /// as possible from `top` between the `edges` of its containing block. Where it reaches
    /// past the end of the page, it is laid out again up to there; where not even its start
    /// fits, it moves to the next page and `None` is returned. Otherwise returns its margin box
    /// and the fragment to show (at its margins, like that of [`Layouter::atomic`]).
    pub(crate) fn place_float_box(
        &self,
        id: BoxId,
        (fragment, size): (Fragment, (Au, Au)),
        side: Float,
        top: Au,
        edges: (Au, Au),
    ) -> Option<(Rect, Fragment)> {
        let limit = self.contexts.borrow().last().and_then(|context| context.limit);
        // A float that clears a float going on to the next page, or comes after one that moved
        // there whole, moves there too: floats are not placed above earlier ones (Blink's
        // `ExclusionSpace::NeedsClearancePastFragmentainer`).
        let past = limit.is_some()
            && self.contexts.borrow().last().is_some_and(|context| {
                context
                    .cleared(self.tree.get(id).style.clear)
                    .any(|float| float.token.is_some())
                    || context
                        .floats
                        .iter()
                        .any(|float| float.token.as_ref().is_some_and(|token| token.inner.is_none()))
            });
        if past {
            let y = limit.map_or(top, |limit| top.max(limit.limit));
            self.push_moved(
                FloatToken {
                    id,
                    side,
                    inner: None,
                    edges,
                    containing_height: self.inline_containing_height.get(),
                    pushed: (Au::ZERO, (Au::ZERO, Au::ZERO, Au::ZERO)),
                },
                y,
            );
            return None;
        }
        let rect = self.place_float(side, size, top, edges.0, edges.1);
        // An initial letter is sized for its lines and goes with them. A float whose content
        // overflows it past the end of the page breaks too: the lines that do not fit go on at
        // the top of the next page, as in Chrome.
        let Some(limit) = limit.filter(|limit| {
            (rect.y + rect.height > limit.limit
                || rect.y + fragment.rect.y + overflow_reach(&fragment) > limit.limit)
                && self.tree.get(id).style.initial_letter.is_none()
        }) else {
            self.shape_float(id, &fragment, rect, edges.1 - edges.0);
            return Some((rect, fragment));
        };
        let at_top = limit.top.is_some_and(|top| rect.y <= top);
        let part = self.float_part(id, edges.1 - edges.0, Some(limit.limit - rect.y), None, at_top);
        let mut contexts = self.contexts.borrow_mut();
        let Some(placed) = contexts.last_mut().and_then(|context| context.floats.last_mut()) else {
            return Some((rect, fragment));
        };
        if let Some((mut atomic, token)) = part {
            placed.rect.height = atomic.height;
            let pushed = (
                self.monolithic_overflow(&atomic, limit.limit - rect.y),
                part_box(&atomic),
            );
            grow_to_overflow(&mut atomic, pushed.0);
            placed.token = token.map(|token| FloatToken {
                id,
                side,
                inner: Some(Box::new(token)),
                edges,
                containing_height: self.inline_containing_height.get(),
                pushed,
            });
            let rect = placed.rect;
            drop(contexts);
            // Each part of a broken float has the float area of its own box, as in Chrome.
            self.shape_float(id, &atomic.fragment, rect, edges.1 - edges.0);
            return Some((rect, atomic.fragment));
        }
        // An empty float at the end of the page: later floats go below it, and so on to the
        // next page too, as do boxes that clear it.
        placed.rect = Rect::new(rect.x, rect.y.max(limit.limit), Au::ZERO, Au::ZERO);
        placed.token = Some(FloatToken {
            id,
            side,
            inner: None,
            edges,
            containing_height: self.inline_containing_height.get(),
            pushed: (Au::ZERO, (Au::ZERO, Au::ZERO, Au::ZERO)),
        });
        None
    }

    /// Gives the float placed last, `id` with `fragment` in margin box `rect`, its float area:
    /// for a part of a broken float, that of the part's box.
    fn shape_float(&self, id: BoxId, fragment: &Fragment, rect: Rect, containing_width: Au) {
        let style = &self.tree.get(id).style;
        if style.shape_outside.is_none() {
            return;
        }
        let basis = containing_width.to_px();
        let border = |side: f32| side;
        let float = FloatBox {
            size: (rect.width.to_px(), rect.height.to_px()),
            margin: [
                fragment.rect.y.to_px(),
                (rect.width - fragment.rect.x - fragment.rect.width).to_px(),
                (rect.height - fragment.rect.y - fragment.rect.height).to_px(),
                fragment.rect.x.to_px(),
            ],
            border: [
                border(style.border_width.top),
                border(style.border_width.right),
                border(style.border_width.bottom),
                border(style.border_width.left),
            ],
            padding: [
                style.padding.top.resolve(basis),
                style.padding.right.resolve(basis),
                style.padding.bottom.resolve(basis),
                style.padding.left.resolve(basis),
            ],
            replaced: matches!(self.tree.get(id).kind, BoxKind::Replaced { .. }),
        };
        let shape = FloatShape::new(
            style,
            float,
            basis,
            self.tree.shape_image(id),
            self.writing_mode(),
        );
        if let Some(placed) = self
            .contexts
            .borrow_mut()
            .last_mut()
            .and_then(|context| context.floats.last_mut())
        {
            placed.shape = shape.map(Arc::new);
        }
    }

    /// Places the floats that continue from the page before at `top` of the content of the
    /// formatting context being resumed; returns their fragments in its coordinates.
    pub(crate) fn resume_floats(&self, floats: &[FloatToken], top: Au, at_top: bool) -> Vec<Fragment> {
        let limit = self.contexts.borrow().last().and_then(|context| context.limit);
        let mut fragments = Vec::new();
        for float in floats {
            let available = float.edges.1 - float.edges.0;
            let inner = float.inner.as_deref();
            // A float that starts again clears the floats placed before it.
            let top = match inner {
                None => self
                    .clearance(self.tree.get(float.id).style.clear)
                    .map_or(top, |clear| clear.max(top)),
                Some(_) => top,
            };
            let remaining = |top: Au| limit.map(|limit| limit.limit - top);
            // Below monolithic overflow of its part before: a page it covers holds an empty
            // part, and the content goes on below it.
            let (pushed, (part_left, part_width, outer_width)) = float.pushed;
            let pushed = if inner.is_some() { pushed } else { Au::ZERO };
            if pushed > Au::ZERO
                && let Some(room) = remaining(top).filter(|&room| pushed >= room && room > Au::ZERO)
            {
                let rect =
                    self.place_float(float.side, (outer_width, room), top, float.edges.0, float.edges.1);
                if let Some(placed) = self
                    .contexts
                    .borrow_mut()
                    .last_mut()
                    .and_then(|context| context.floats.last_mut())
                {
                    placed.token = Some(FloatToken {
                        pushed: (pushed - room, float.pushed.1),
                        ..float.clone()
                    });
                }
                fragments.push(Fragment {
                    rect: Rect::new(rect.x + part_left, rect.y, part_width, Au::ZERO),
                    kind: self
                        .tree
                        .get(float.id)
                        .fragment_kind(crate::fragment::VisibleSides {
                            top: false,
                            bottom: false,
                            ..crate::fragment::VisibleSides::ALL
                        }),
                    children: crate::fragment::Children::new(),
                });
                continue;
            }
            let outer_height = self.inline_containing_height.replace(float.containing_height);
            let part = self.float_part(
                float.id,
                available,
                remaining(top).map(|room| room - pushed),
                inner,
                at_top && pushed == Au::ZERO,
            );
            self.inline_containing_height.set(outer_height);
            let part = part.map(|(mut atomic, token)| {
                if pushed > Au::ZERO {
                    atomic.fragment.rect.y += pushed;
                    atomic.height += pushed;
                    atomic.baseline += pushed;
                }
                (atomic, token)
            });
            let Some((mut atomic, mut token)) = part else {
                self.push_moved(float.clone(), limit.map_or(top, |limit| limit.limit));
                continue;
            };
            let mut rect = self.place_float(
                float.side,
                (atomic.width, atomic.height),
                top,
                float.edges.0,
                float.edges.1,
            );
            if rect.y > top {
                // Below another float: laid out again for the room left there.
                self.truncate_floats(self.float_count() - 1);
                let outer_height = self.inline_containing_height.replace(float.containing_height);
                let part = self.float_part(float.id, available, remaining(rect.y), inner, false);
                self.inline_containing_height.set(outer_height);
                let Some(part) = part else {
                    self.push_moved(float.clone(), limit.map_or(rect.y, |limit| limit.limit));
                    continue;
                };
                (atomic, token) = part;
                rect = self.place_float(
                    float.side,
                    (atomic.width, atomic.height),
                    rect.y,
                    float.edges.0,
                    float.edges.1,
                );
            }
            if let Some(token) = token
                && let Some(placed) = self
                    .contexts
                    .borrow_mut()
                    .last_mut()
                    .and_then(|context| context.floats.last_mut())
            {
                placed.token = Some(FloatToken {
                    inner: Some(Box::new(token)),
                    pushed: (
                        remaining(rect.y).map_or(Au::ZERO, |room| self.monolithic_overflow(&atomic, room)),
                        part_box(&atomic),
                    ),
                    ..float.clone()
                });
            }
            // The float area of the continuation, from its own box.
            self.shape_float(float.id, &atomic.fragment, rect, available);
            if let Some(room) = remaining(rect.y) {
                let overflow = self.monolithic_overflow(&atomic, room);
                grow_to_overflow(&mut atomic, overflow);
            }
            let mut fragment = atomic.fragment;
            fragment.rect.x += rect.x;
            fragment.rect.y += rect.y;
            fragments.push(fragment);
        }
        fragments
    }

    /// How far the monolithic content of a float's part (laid out as `atomic`, its margin box)
    /// reaches past `room` from the top of its margin box, on pages (see
    /// [`FloatToken::pushed`]); in columns, whose overflow does not reach into the next column,
    /// nothing, as in Chrome.
    fn monolithic_overflow(&self, atomic: &Atomic, room: Au) -> Au {
        if self.columns.get() > 0 {
            return Au::ZERO;
        }
        let fragment = &atomic.fragment;
        fragment
            .children
            .iter()
            .filter_map(|child| crate::monolithic::content_bottom(child, fragment.rect.y))
            .max()
            .map_or(Au::ZERO, |bottom| (bottom - room).clamp_non_negative())
    }

    /// Keeps a float that does not fit on this page for the next, as an empty float at `y`.
    fn push_moved(&self, token: FloatToken, y: Au) {
        let rect = self.place_float(token.side, (Au::ZERO, Au::ZERO), y, token.edges.0, token.edges.1);
        if let Some(placed) = self
            .contexts
            .borrow_mut()
            .last_mut()
            .and_then(|context| context.floats.last_mut())
        {
            placed.rect = Rect::new(rect.x, rect.y, Au::ZERO, Au::ZERO);
            placed.token = Some(token);
        }
    }

    /// Lays out the part of float `id` (in a containing block of width `available`) that fits
    /// in `remaining` from the top of its margin box, from `token` on. Block containers, tables,
    /// flex, grid and multi-column containers break as they do in the flow, as in Chrome; boxes
    /// that cannot break (replaced elements, boxes with `break-inside: avoid`) fit as a whole or
    /// not at all, unless they are `at_top` of the page. `None` means that nothing fits.
    fn float_part(
        &self,
        id: BoxId,
        available: Au,
        remaining: Option<Au>,
        token: Option<&BreakToken>,
        at_top: bool,
    ) -> Option<(Atomic, Option<BreakToken>)> {
        let layout_box = self.tree.get(id);
        let style = &layout_box.style;
        // Boxes with size containment or `line-clamp` are monolithic (see `block_level`).
        let monolithic =
            (style.size_containment().0 && style.size_containment().1) || style.line_clamp.is_some();
        let breakable = matches!(
            layout_box.kind,
            BoxKind::Block { .. }
                | BoxKind::Inline { .. }
                | BoxKind::Table { .. }
                | BoxKind::Flex { .. }
                | BoxKind::Grid { .. }
        ) && !monolithic;
        let avoids = self.avoids_breaks_inside(style.break_inside);
        // A float broken on a page before goes on breaking.
        let Some(remaining) = remaining.filter(|_| breakable && (!avoids || token.is_some())) else {
            let atomic = self.atomic(id, available);
            let too_tall = remaining.is_some_and(|remaining| atomic.height > remaining);
            if too_tall && !at_top {
                return None;
            }
            // A float that avoids breaks inside but does not fit even at the top of a page
            // breaks there after all, as in Chrome.
            match remaining {
                Some(remaining) if breakable && too_tall => {
                    return self.float_part_broken(id, available, remaining, token, at_top);
                }
                _ => return Some((atomic, None)),
            }
        };
        self.float_part_broken(id, available, remaining, token, at_top)
    }

    /// [`Layouter::float_part`] for a float that breaks where it does not fit in `remaining`.
    fn float_part_broken(
        &self,
        id: BoxId,
        available: Au,
        remaining: Au,
        token: Option<&BreakToken>,
        at_top: bool,
    ) -> Option<(Atomic, Option<BreakToken>)> {
        let layout_box = self.tree.get(id);
        let style = &layout_box.style;
        let margin = |value: &fagus_style::values::LengthPercentageAuto| {
            Au::from_px(value.resolve_or(available.to_px(), 0.0))
        };
        let (margin_left, margin_right) = (margin(&style.margin.left), margin(&style.margin.right));
        let margin_top = if token.is_some() {
            Au::ZERO
        } else {
            margin(&style.margin.top)
        };
        let content_width = self.shrunk_width(id, available, available, self.inline_containing_height.get());
        let space = Space {
            containing_width: available,
            // Percentage heights refer to the height of the block the float is in, where that
            // is definite (see `Layouter::atomic`).
            containing_height: self.inline_containing_height.get(),
            remaining: Some(remaining),
            at_top,
            flush: true,
            bfc_left: Au::ZERO,
            bfc_top: Au::ZERO,
            // The float's own direction, which it inherits from its containing block.
            rtl: style.is_rtl(),
            stretch_skips_margins: (false, false),
        };
        let outcome = match &layout_box.kind {
            BoxKind::Table { .. } => self.table(id, space, token),
            BoxKind::Flex { .. } | BoxKind::Grid { .. } => self.flex_or_grid(id, space, token, content_width),
            _ if style.is_multicol() => {
                self.in_multicol(id, || self.multicol_with(id, space, token, content_width))
            }
            _ => self.block_container_with(id, space, token, content_width, None, true),
        }?;
        let mut fragment = outcome.fragment;
        fragment.rect.x = margin_left;
        fragment.rect.y = margin_top;
        // Content that overflows a float whose box ended on a page before goes on without the
        // box, in a parallel flow that takes no room from the content beside it, as in Chrome.
        let overflow_only = matches!(token, Some(BreakToken::Overflow { .. }));
        let margin_bottom = if outcome.token.is_some() || overflow_only {
            Au::ZERO
        } else {
            margin(&style.margin.bottom)
        };
        let height = margin_top + fragment.rect.height + margin_bottom;
        // A float that ends on the page needs room for its bottom margin too: where that does
        // not fit, the whole float moves to the next page, as in Chrome (which does not break
        // it earlier instead).
        if outcome.token.is_none() && !at_top && margin_bottom > Au::ZERO && height > remaining {
            return None;
        }
        Some((
            Atomic {
                width: margin_left + fragment.rect.width + margin_right,
                height,
                baseline: height,
                fragment,
            },
            outcome.token,
        ))
    }
}

/// Lets the box of a float's part (laid out as `atomic`) grow to hold its monolithic content
/// that reaches `overflow` past the end of the page, as Chrome draws it: it shows on the pages
/// after, as other content that overflows its page does (see [`FloatToken::pushed`]).
fn grow_to_overflow(atomic: &mut Atomic, overflow: Au) {
    if overflow > Au::ZERO {
        let fragment = &mut atomic.fragment;
        let bottom = fragment
            .children
            .iter()
            .filter_map(|child| crate::monolithic::content_bottom(child, Au::ZERO))
            .max()
            .unwrap_or(Au::ZERO);
        fragment.rect.height = fragment.rect.height.max(bottom);
    }
}

/// Where the box of a float's part lies in its margin box (laid out as `atomic`): its left
/// and width, and the margin box's width.
fn part_box(atomic: &Atomic) -> (Au, Au, Au) {
    (atomic.fragment.rect.x, atomic.fragment.rect.width, atomic.width)
}

/// How far below its top the content of `fragment` reaches, as far as boxes do not clip it:
/// its height, or more where its content overflows it.
fn overflow_reach(fragment: &Fragment) -> Au {
    let clips = matches!(&fragment.kind, FragmentKind::Box { style, .. }
        if style.overflow_y != fagus_style::values::Overflow::Visible);
    if clips || matches!(fragment.kind, FragmentKind::Text(_)) {
        return fragment.rect.height;
    }
    fragment
        .children
        .iter()
        .map(|child| child.rect.y + overflow_reach(child))
        .fold(fragment.rect.height, Au::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(value: f32) -> Au {
        Au::from_px(value)
    }

    #[test]
    fn places_floats_side_by_side_and_below() {
        let mut space = Exclusions::default();
        let a = space.place(Float::Left, px(100.0), px(50.0), Au::ZERO, Au::ZERO, px(300.0));
        let b = space.place(Float::Left, px(100.0), px(80.0), Au::ZERO, Au::ZERO, px(300.0));
        let c = space.place(Float::Right, px(50.0), px(30.0), Au::ZERO, Au::ZERO, px(300.0));
        assert_eq!((a.x, a.y), (Au::ZERO, Au::ZERO));
        assert_eq!((b.x, b.y), (px(100.0), Au::ZERO), "beside the first");
        assert_eq!((c.x, c.y), (px(250.0), Au::ZERO), "at the right edge");
        // No room beside them: down past float edges until it fits.
        let d = space.place(Float::Left, px(120.0), px(10.0), Au::ZERO, Au::ZERO, px(300.0));
        assert_eq!((d.x, d.y), (Au::ZERO, px(80.0)));
        assert_eq!(
            space.free(Au::ZERO, px(300.0), px(10.0), px(5.0)),
            (px(200.0), px(250.0))
        );
        assert_eq!(space.clearance(Clear::Left), Some(px(90.0)));
        assert_eq!(space.clearance(Clear::Right), Some(px(30.0)));
    }

    #[test]
    fn puts_floats_below_initial_letters_but_does_not_clear_them() {
        let mut space = Exclusions::default();
        space.floats.push(Placed {
            side: Float::Left,
            rect: Rect::new(Au::ZERO, px(20.0), px(60.0), px(70.0)),
            token: None,
            shape: None,
            letter: true,
        });
        // Lines go around it like around a float.
        assert_eq!(
            space.line_free(Au::ZERO, px(300.0), px(30.0), px(10.0)).0,
            px(60.0)
        );
        // `clear` does not move boxes below it, but a float on its side goes there, and one
        // on the other side may stand above its top.
        assert_eq!(space.clearance(Clear::Both), None);
        assert_eq!(space.letter_clearance(None), Some(px(90.0)));
        let left = space.place(Float::Left, px(10.0), px(10.0), Au::ZERO, Au::ZERO, px(300.0));
        assert_eq!(left.y, px(90.0));
        let right = space.place(Float::Right, px(10.0), px(10.0), Au::ZERO, Au::ZERO, px(300.0));
        assert_eq!(right.y, px(90.0), "not above an earlier float");
    }

    #[test]
    fn narrows_bands_across_floats_of_no_height() {
        let mut space = Exclusions::default();
        space.place(Float::Left, px(10.0), px(30.0), Au::ZERO, Au::ZERO, px(500.0));
        // An empty float that clears the first one, at its bottom.
        let empty = space.place(Float::Left, px(100.0), Au::ZERO, px(30.0), Au::ZERO, px(500.0));
        assert_eq!((empty.x, empty.y), (Au::ZERO, px(30.0)));
        // A line across it goes beside it; one that starts or ends where it lies does not.
        assert_eq!(
            space.line_free(Au::ZERO, px(500.0), px(20.0), px(20.0)).0,
            px(100.0)
        );
        assert_eq!(
            space.line_free(Au::ZERO, px(500.0), px(30.0), px(20.0)).0,
            Au::ZERO
        );
        assert_eq!(
            space.line_free(Au::ZERO, px(500.0), px(10.0), px(20.0)).0,
            px(10.0)
        );
        assert_eq!(space.free(Au::ZERO, px(500.0), px(25.0), px(10.0)).0, px(100.0));
    }
}

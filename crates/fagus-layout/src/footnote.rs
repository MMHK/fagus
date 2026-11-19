//! Footnotes (CSS Generated Content for Paged Media 3, §2).
//!
//! An element with `float: footnote` leaves its call (`::footnote-call`) in the text, with the
//! placeholder of its box inside (see `tree::Builder::footnote`), and its box goes into the
//! footnote area at the bottom of the page the call is on, which `@page { @footnote { … } }`
//! styles. The area takes its height from the page area: the more footnotes a page holds, the
//! less room is left for its content.
//!
//! A page is laid out, the calls on it found, and laid out again with room for their
//! footnotes, which may move calls to the next page and so leave room for footnotes again (see
//! [`Footnotes::fit`]). The search takes a handful of layouts at most and depends on nothing
//! but the content: it gives the same pages every time.
//!
//! A footnote that does not fit on the page of its call goes on the next page
//! (`footnote-policy: auto`, as in `WeasyPrint`), or takes the line (`line`) or the block (`block`)
//! of its call with it. A footnote taller than a page breaks across pages.

use std::sync::Arc;

use fagus_style::ComputedStyle;
use fagus_style::values::{Display, FootnoteDisplay, FootnotePolicy, MaxSize};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::block::{BreakToken, Layouter, Margin, Outcome, Space};
use crate::fragment::{Children, Fragment, FragmentKind, VisibleSides};
use crate::tree::{BoxId, BoxKind, BoxTree, InlineItem};
use crate::units::{Au, Rect};

/// How many layouts of a page the search for its footnotes takes at most, besides the first
/// and the one that moves a call's line: enough for 2⁶ footnotes on a page.
const MAX_TRIES: usize = 6;

/// A footnote to place in a footnote area: its box, where it goes on (a footnote broken across
/// pages), and the numbering of the page it is called on, which its marker shows.
#[derive(Clone, Debug)]
struct Note {
    id: BoxId,
    token: Option<BreakToken>,
    numbering: Option<(u32, i32)>,
}

/// A call of a footnote in the content of a page.
#[derive(Clone, Copy, Debug)]
struct Call {
    id: BoxId,
    ordinal: u32,
    /// Where the line of the call starts, and the block that holds the line where it starts on
    /// the page, from the start of the room the content is laid out in.
    line: Au,
    block: Option<Au>,
    policy: FootnotePolicy,
}

/// A laid-out footnote area: its fragment, placed in its margins at the top of the room for it,
/// and how much of the page it takes, margins included.
pub(crate) struct Area {
    pub fragment: Fragment,
    pub extent: Au,
}

/// The footnotes of a document while its pages are laid out.
pub(crate) struct Footnotes {
    /// The footnotes called on earlier pages that did not fit there, in order.
    deferred: Vec<Note>,
    /// The footnotes called so far: a call repeated on a later page (in a table header) calls
    /// its footnote only once.
    seen: FxHashSet<BoxId>,
    /// The number of the first footnote not called yet.
    next: u32,
    /// How the page being laid out numbers its footnotes (see `Layouter::footnote_numbering`).
    numbering: Option<(u32, i32)>,
}

impl Footnotes {
    /// The footnotes of `tree`; `None` if it has none.
    pub(crate) fn of(tree: &BoxTree) -> Option<Self> {
        (!tree.footnotes.is_empty()).then(|| Self {
            deferred: Vec::new(),
            seen: FxHashSet::default(),
            next: 0,
            numbering: None,
        })
    }

    /// Whether footnotes moved from earlier pages wait for a page.
    pub(crate) fn waiting(&self) -> bool {
        !self.deferred.is_empty()
    }

    /// Starts a page whose page box resets the `footnote` counter to `reset`, if it does: the
    /// footnotes called on it count from there, those of the pages after on from them.
    pub(crate) fn start_page(&mut self, layouter: &Layouter<'_>, reset: Option<i32>) {
        if let Some(value) = reset {
            self.numbering = Some((self.next, value));
        }
        layouter.footnote_numbering.set(self.numbering);
    }

    /// Lays out the content of a page with its footnotes: `attempt` lays out the content in the
    /// room it is given, `room` is the room the page leaves for it without footnotes. Returns
    /// the layout of the content (`None` where the footnotes moved here take the whole page)
    /// and the footnote area, if the page has one.
    ///
    /// The footnotes moved from earlier pages come first, as many as fit, the first of them in
    /// any case (broken where it is taller than the page). Then the largest number of the
    /// footnotes called on the page is found whose calls stay on it when the content has the
    /// room they leave: each layout with more room for footnotes holds fewer calls, so a binary
    /// search over that number finds it. Where the footnote after them has `footnote-policy:
    /// line` or `block`, the content is laid out once more to end before the line or block of
    /// its call; else the footnotes of the calls left go on the next page.
    pub(crate) fn fit(
        &mut self,
        layouter: &Layouter<'_>,
        style: &ComputedStyle,
        inline: Au,
        room: Au,
        attempt: &dyn Fn(Au) -> Option<Outcome>,
    ) -> (Option<Outcome>, Option<Area>) {
        let mut cache = FxHashMap::default();
        // The room of the last layout, which left the state of the layouter.
        let last = std::cell::Cell::new(None);
        let attempt = |room: Au| {
            last.set(Some(room));
            attempt(room)
        };
        let deferred = std::mem::take(&mut self.deferred);
        let (laid, placed, rest) = lay_out(layouter, style, inline, &deferred, room, &mut cache);
        let notes = deferred[..placed].to_vec();
        let base = if notes.is_empty() { Au::ZERO } else { laid.extent };
        let moved = (!notes.is_empty()).then_some(laid);
        let outcome = (base < room).then(|| attempt(room - base)).flatten();
        // The footnotes moved here leave no room for what comes first: it waits for the next
        // page.
        if base > Au::ZERO
            && outcome
                .as_ref()
                .is_some_and(|outcome| !fits(outcome, room - base))
        {
            self.deferred = rest;
            return (None, moved);
        }
        let numbering = self.numbering;
        let forced = layouter.forced_break.get();
        let calls = self.calls(layouter.tree, outcome.as_ref());
        let note = |call: &Call| Note {
            id: call.id,
            token: None,
            numbering,
        };
        // Once footnotes wait for a page, those called after them wait too.
        if calls.is_empty() || !rest.is_empty() {
            self.deferred = rest;
            self.deferred.extend(calls.iter().map(note));
            self.called(&calls);
            return (outcome, moved);
        }
        // The room the footnotes moved here and those of the first `count` calls take, if
        // they fit whole.
        let extent = |count: usize, cache: &mut FxHashMap<BoxId, Option<Outcome>>| {
            let mut all = notes.clone();
            all.extend(calls[..count].iter().map(note));
            let (laid, placed, rest) = lay_out(layouter, style, inline, &all, room, cache);
            (placed == all.len() && rest.is_empty() && laid.extent < room).then_some(laid.extent)
        };
        // The layout that holds the calls of the first `count` footnotes in the room they
        // leave, and that room.
        let mut best = (0, outcome, calls.clone(), forced, room - base);
        let (mut low, mut high) = (0, calls.len());
        let mut count = high;
        let mut tries = 0;
        while low < high && tries < MAX_TRIES {
            tries += 1;
            let held = extent(count, &mut cache).and_then(|needed| {
                let outcome = attempt(room - needed)?;
                let held = self.calls(layouter.tree, Some(&outcome));
                (fits(&outcome, room - needed)
                    && held.len() >= count
                    && same(&held[..count], &calls[..count]))
                .then(|| {
                    (
                        count,
                        Some(outcome),
                        held,
                        layouter.forced_break.get(),
                        room - needed,
                    )
                })
            });
            match held {
                Some(found) => {
                    best = found;
                    low = count;
                }
                None => high = count - 1,
            }
            count = (low + high).div_ceil(2);
        }
        let (mut count, mut outcome, mut held, mut forced, mut chosen) = best;
        // The footnote after those that fit takes the line or the block of its call with it,
        // where something comes before that on the page.
        if let Some(first) = held.get(count).copied()
            && first.policy != FootnotePolicy::Auto
            && let Some(needed) = extent(count, &mut cache)
        {
            let cut = match first.policy {
                FootnotePolicy::Block => first.block.unwrap_or(first.line),
                _ => first.line,
            }
            .min(room - needed);
            if cut > Au::ZERO
                && let Some(moved) = attempt(cut)
            {
                let left = self.calls(layouter.tree, Some(&moved));
                if fits(&moved, cut) && left.len() <= count && same(&left, &held[..left.len()]) {
                    (count, outcome, held) = (left.len(), Some(moved), left);
                    (forced, chosen) = (layouter.forced_break.get(), cut);
                }
            }
        }
        // The layouter keeps what the last layout found (the static positions of boxes, which
        // the boxes are placed by later): the chosen one is laid out again where it was not
        // the last.
        if last.get() == Some(chosen) {
            layouter.forced_break.set(forced);
        } else {
            outcome = attempt(chosen);
        }
        let mut all = notes;
        all.extend(held[..count].iter().map(note));
        let (laid, placed, rest) = lay_out(layouter, style, inline, &all, room, &mut cache);
        self.deferred = rest;
        self.deferred.extend(held[count..].iter().map(note));
        self.called(&held);
        (outcome, (placed > 0).then_some(laid))
    }

    /// The footnote area of a page that holds only footnotes moved from earlier pages, the
    /// content having ended: as many as fit in `room`, the first in any case.
    pub(crate) fn only_notes(
        &mut self,
        layouter: &Layouter<'_>,
        style: &ComputedStyle,
        inline: Au,
        room: Au,
    ) -> Option<Area> {
        let (_, area) = self.fit(layouter, style, inline, room, &|_| None);
        area
    }

    /// Marks the footnotes of `calls` as called.
    fn called(&mut self, calls: &[Call]) {
        for call in calls {
            self.seen.insert(call.id);
            self.next = self.next.max(call.ordinal.saturating_add(1));
        }
    }

    /// The calls of footnotes not called before in the content of a page, in document order.
    fn calls(&self, tree: &BoxTree, outcome: Option<&Outcome>) -> Vec<Call> {
        let mut calls = Vec::new();
        if let Some(outcome) = outcome {
            find_calls(tree, &outcome.fragment, Au::ZERO, None, None, &mut calls);
        }
        calls.retain(|call| !self.seen.contains(&call.id));
        calls.sort_by_key(|call| call.ordinal);
        calls.dedup_by_key(|call| call.id);
        calls
    }
}

/// Whether the content of a layout stays in its `room`: a layout at the top of a page places
/// a line or an image taller than the room all the same, which would run into the footnotes.
fn fits(outcome: &Outcome, room: Au) -> bool {
    let fragment = &outcome.fragment;
    let bottom = crate::monolithic::content_bottom(fragment, Au::ZERO)
        .unwrap_or(Au::ZERO)
        .max(fragment.rect.y + fragment.rect.height);
    bottom <= room
}

/// Whether two lists of calls call the same footnotes.
fn same(one: &[Call], other: &[Call]) -> bool {
    one.len() == other.len() && one.iter().zip(other).all(|(one, other)| one.id == other.id)
}

/// Collects the calls in `fragment`, whose parent's border box lies `origin` from the start of
/// the room, inside the line starting at `line` and the block starting at `block`.
fn find_calls(
    tree: &BoxTree,
    fragment: &Fragment,
    origin: Au,
    line: Option<Au>,
    block: Option<Au>,
    calls: &mut Vec<Call>,
) {
    let y = origin + fragment.rect.y;
    let (line, block) = match &fragment.kind {
        FragmentKind::Placeholder(id) => {
            if let Some(&ordinal) = tree.footnotes.get(id) {
                calls.push(Call {
                    id: *id,
                    ordinal,
                    line: line.unwrap_or(y),
                    block,
                    policy: tree.get(*id).style.footnote_policy,
                });
            }
            return;
        }
        FragmentKind::Line { .. } => (Some(y), block),
        // The block around the lines, where it starts on this page.
        FragmentKind::Box { sides, .. } if line.is_none() => (line, sides.top.then_some(y)),
        _ => (line, block),
    };
    for child in &fragment.children {
        find_calls(tree, child, y, line, block, calls);
    }
}

/// Lays out the footnote area of `notes` with `style`, `inline` wide, in `room`: as many notes
/// as fit, the first in any case, broken where it does not fit. Returns the area, how many
/// notes it holds (a broken one included) and the notes left for the next page (the rest of a
/// broken one first).
fn lay_out(
    layouter: &Layouter<'_>,
    style: &ComputedStyle,
    inline: Au,
    notes: &[Note],
    room: Au,
    cache: &mut FxHashMap<BoxId, Option<Outcome>>,
) -> (Area, usize, Vec<Note>) {
    let px = Au::from_px;
    let basis = inline.to_px();
    let margin = [
        px(style.margin.top.resolve_or(basis, 0.0)),
        px(style.margin.right.resolve_or(basis, 0.0)),
        px(style.margin.bottom.resolve_or(basis, 0.0)),
        px(style.margin.left.resolve_or(basis, 0.0)),
    ];
    let inset = [
        px(style.border_width.top) + px(style.padding.top.resolve(basis)),
        px(style.border_width.right) + px(style.padding.right.resolve(basis)),
        px(style.border_width.bottom) + px(style.padding.bottom.resolve(basis)),
        px(style.border_width.left) + px(style.padding.left.resolve(basis)),
    ];
    let width = (inline - margin[1] - margin[3]).clamp_non_negative();
    let content_width = (width - inset[1] - inset[3]).clamp_non_negative();
    // `max-height` of the area, a percentage of the page area, limits the room for footnotes.
    let room = match &style.max_height {
        MaxSize::Length(length) => {
            let frame = if style.box_sizing == fagus_style::values::BoxSizing::ContentBox {
                inset[0] + inset[2]
            } else {
                Au::ZERO
            };
            room.min(px(length.resolve(room.to_px())) + frame + margin[0] + margin[2])
        }
        _ => room,
    };
    let room_for_notes = (room - margin[0] - margin[2] - inset[0] - inset[2]).clamp_non_negative();
    let outer = layouter.footnote_numbering.get();
    let mut children = Vec::new();
    let mut y = Au::ZERO;
    let mut below: Option<Margin> = None;
    let mut placed = 0;
    let mut continued = None;
    while placed < notes.len() {
        let first = placed == 0;
        // Footnotes laid out inline share lines with those beside them that are too.
        let run = inline_run(layouter, &notes[placed..], content_width);
        if !run.is_empty() {
            layouter.footnote_numbering.set(notes[placed].numbering);
            let lines = crate::inline::layout_lines(layouter, style, &run.items, content_width);
            let height: Au = lines.iter().map(|line| line.height).sum();
            let gap = below.map_or(Au::ZERO, Margin::resolve);
            if !first && y + gap + height > room_for_notes {
                break;
            }
            y += gap;
            for line in lines {
                let mut fragment = line.fragment;
                fragment.rect.x += inset[3];
                fragment.rect.y = inset[0] + y;
                y += line.height;
                children.push(fragment);
            }
            below = None;
            placed += run.notes;
            continue;
        }
        let note = &notes[placed];
        layouter.footnote_numbering.set(note.numbering);
        // The first footnote breaks where it does not fit; the others fit whole or wait.
        let outcome = if first {
            let space = Space {
                remaining: Some(room_for_notes),
                at_top: true,
                ..Space::unfragmented(content_width)
            };
            layouter.block_level(note.id, space, note.token.as_ref())
        } else {
            cache
                .entry(note.id)
                .or_insert_with(|| layouter.block_level(note.id, Space::unfragmented(content_width), None))
                .clone()
        };
        let Some(outcome) = outcome else {
            placed += 1;
            continue;
        };
        let gap = match below {
            Some(below) => below.adjoin(outcome.collapsed_top).resolve(),
            None => outcome.collapsed_top.resolve(),
        };
        let height = outcome.fragment.rect.height;
        if !first && y + gap + height > room_for_notes {
            break;
        }
        let mut fragment = outcome.fragment;
        fragment.rect.x += inset[3];
        fragment.rect.y = inset[0] + y + gap;
        y += gap + height;
        below = Some(outcome.collapsed_bottom);
        children.push(fragment);
        placed += 1;
        if first && let Some(token) = outcome.token {
            continued = Some(token);
            break;
        }
    }
    layouter.footnote_numbering.set(outer);
    y += below.map_or(Au::ZERO, Margin::resolve);
    let height = inset[0] + y + inset[2];
    let mut rest = Vec::new();
    if let Some(token) = continued
        && let Some(note) = notes.first()
    {
        rest.push(Note {
            token: Some(token),
            ..note.clone()
        });
    }
    rest.extend(notes[placed..].iter().cloned());
    let fragment = Fragment {
        rect: Rect::new(margin[3], margin[0], width, height),
        kind: FragmentKind::Box {
            style: Arc::new(style.clone()),
            node: None,
            sides: VisibleSides::ALL,
        },
        children: Children::from(children),
    };
    let area = Area {
        fragment,
        extent: margin[0] + height + margin[2],
    };
    (area, placed, rest)
}

/// Footnotes laid out on shared lines: their inline content, each in an inline box of the
/// footnote's element, and how many footnotes it holds.
#[derive(Default)]
struct InlineRun {
    items: Vec<InlineItem>,
    notes: usize,
}

impl InlineRun {
    fn is_empty(&self) -> bool {
        self.notes == 0
    }
}

/// The footnotes at the start of `notes` that go on shared lines, `width` long: those with
/// `footnote-display: inline`, and those with `compact` that fit on a line (CSS Generated
/// Content for Paged Media 3, §2.3), whose content is inline, called on the same page and not
/// broken.
fn inline_run(layouter: &Layouter<'_>, notes: &[Note], width: Au) -> InlineRun {
    let mut run = InlineRun::default();
    for note in notes {
        let layout_box = layouter.tree.get(note.id);
        let BoxKind::Inline { items } = &layout_box.kind else {
            break;
        };
        if note.token.is_some() || notes[0].numbering != note.numbering {
            break;
        }
        let style = &layout_box.style;
        let mut own = Vec::with_capacity(items.len() + 2);
        own.push(InlineItem::Open {
            style: Arc::new(ComputedStyle {
                display: Display::INLINE,
                ..(**style).clone()
            }),
            node: layout_box.node,
        });
        own.extend(items.iter().cloned());
        own.push(InlineItem::Close);
        let shared = match style.footnote_display {
            FootnoteDisplay::Block => false,
            FootnoteDisplay::Inline => true,
            FootnoteDisplay::Compact => {
                let outer = layouter.footnote_numbering.replace(note.numbering);
                let (_, max_content) = crate::inline::intrinsic_widths(layouter, &own);
                layouter.footnote_numbering.set(outer);
                Au::from_px_ceil(max_content) <= width
            }
        };
        if !shared {
            break;
        }
        run.items.append(&mut own);
        run.notes += 1;
    }
    run
}

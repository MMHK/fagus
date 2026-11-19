//! Multi-column layout (CSS Multi-column Layout 1). The content of a multi-column container is
//! an anonymous flow box (see the box tree), broken into columns of equal width side by side the
//! way pages break block flow, and continued on the next page when the columns of this one are
//! full. Columns are balanced to the same height, except with `column-fill: auto` on pages where
//! the content does not end.
//!
//! A spanner (`column-span: all`) ends the row of columns above it: the container places it
//! below them, across all of them, and the content after it starts a new row.

use std::sync::Arc;

use fagus_style::values::{BorderStyle, BreakBetween, ColumnFill, Sides};
use fagus_style::{ComputedStyle, ComputedValues};

use crate::block::{BOTTOM, BreakToken, LEFT, Layouter, Margin, Outcome, RIGHT, Space, TOP};
use crate::column_positioned::has_placeholder;
use crate::fragment::{Children, Fragment, FragmentKind, VisibleSides};
use crate::tree::{BoxId, BoxKind};
use crate::units::{Au, Rect};

/// A height no column reaches, for the height a container may take at most.
const UNBOUNDED: Au = Au(i32::MAX / 4);

/// How many times balanced columns grow by the room the content lacks before the height at
/// which they hold it is found by bisection instead.
const BALANCING_STEPS: usize = 32;

/// How many multi-column containers inside one another lay out columns of their own; those
/// inside them are laid out as one column.
const NESTED_COLUMNS: u32 = 3;

/// Columns laid out at one height: their fragments, and where the content continues.
type Filled = (Vec<Fragment>, Option<BreakToken>);

/// The columns of a multi-column container.
struct Columns {
    /// The anonymous box of the content.
    flow: BoxId,
    count: usize,
    width: Au,
    fill: ColumnFill,
    /// The container's definite content height, which percentage heights in the columns and
    /// in spanners refer to (the content box is anonymous).
    percent_basis: Option<Au>,
    /// The container's `text-box-trim` at the start and end, which trims lines at the start
    /// and end of each column too, as in Blink.
    trim: (bool, bool),
}

/// A row of columns: their fragments and height, and where the content continues after them
/// (`None` if it ends in them).
struct Row {
    columns: Vec<Fragment>,
    height: Au,
    rest: Option<BreakToken>,
}

/// Where the content of a column ends: the bottom of its last line or box, not of a box broken
/// at the end of the column, which reaches down to it.
fn column_content_end(column: &Fragment) -> Au {
    fn end(fragment: &Fragment) -> Au {
        match &fragment.kind {
            FragmentKind::Box { sides, .. } if !sides.bottom && !fragment.children.is_empty() => {
                fragment.rect.y + fragment.children.iter().map(end).max().unwrap_or(Au::ZERO)
            }
            _ => fragment.rect.y + fragment.rect.height,
        }
    }
    column.children.iter().map(end).max().unwrap_or(Au::ZERO)
}

/// The number and width of columns in `available` content width, and the gap between them
/// (CSS Multi-column Layout 1, §3.4).
pub(crate) fn columns(style: &ComputedStyle, available: Au) -> (usize, Au, Au) {
    let gap = Au::from_px(
        style
            .column_gap
            .as_ref()
            .map_or(style.font_size, |gap| gap.resolve(available.to_px()))
            .max(0.0),
    );
    let fitting = |width: f32| (((available + gap).to_px() / (width + gap.to_px())).floor() as usize).max(1);
    let count = match (style.column_count, style.column_width) {
        (Some(count), None) => count as usize,
        (None, Some(width)) => fitting(width),
        (Some(count), Some(width)) => (count as usize).min(fitting(width)),
        (None, None) => 1,
    }
    // As many as `column-count` asks for even where they have no width, as in Blink.
    .max(1);
    let width = ((available - gap * (count as i32 - 1)) / count as i32).clamp_non_negative();
    (count, width, gap)
}

impl Layouter<'_> {
    /// Lays out a multi-column container, optionally with a fixed content width (of a float, a
    /// flex or grid item, or from a sizing keyword). `None` means that not even its first line
    /// fits in the remaining space, so it moves to the next page. The columns take the page
    /// down to its end where they continue on the next page, as in Chrome; where they end, they
    /// leave room for the container's bottom border and padding.
    pub(crate) fn multicol_with(
        &self,
        id: BoxId,
        space: Space,
        token: Option<&BreakToken>,
        width_override: Option<Au>,
    ) -> Option<Outcome> {
        self.multicol_sized(id, space, token, (width_override, None))
    }

    /// Lays out a multi-column container as [`Layouter::multicol_with`] does, optionally with
    /// a fixed border-box height too (of a stretched or sized flex or grid item), which the
    /// columns then fill as that of a container of a fixed height.
    pub(crate) fn multicol_sized(
        &self,
        id: BoxId,
        space: Space,
        token: Option<&BreakToken>,
        (width_override, height_override): (Option<Au>, Option<Au>),
    ) -> Option<Outcome> {
        // The lines of a list item in columns are in formatting contexts of their own: its
        // outside marker is placed beside it afterwards.
        self.list_item_in_contexts(&self.tree.get(id).style, || {
            // Laid out as one column (see `multicol_in`), it leaves room for its bottom border
            // and padding itself: laying it out again would do the same again, at each level of
            // nested columns, which takes time growing exponentially with the nesting.
            if self.flow(id).is_none() {
                return self.block_container_with(id, space, token, width_override, height_override, true);
            }
            let floats = self.float_count();
            let sizes = (width_override, height_override);
            let outcome = self.multicol_in(id, space, token, sizes, false)?;
            // Where the content of a box around has to end early, before its bottom border and
            // padding (see `Layouter::ending_early`), columns that start on the page do not break
            // inside but move to the next page, as in Chrome (Blink finds no early break inside
            // columns, which are fragmentainers of their own).
            if outcome.token.is_some() && token.is_none() && self.ending_early.get() && !space.at_top {
                self.truncate_floats(floats);
                return None;
            }
            // Columns measured unbroken (see `multicol_in`) leave no room for their bottom
            // border and padding on the page: laid out again for that, they would be the same,
            // at each level of nested columns.
            let fits = outcome.token.is_some()
                || self.measuring.get()
                || space.remaining.is_none_or(|remaining| {
                    outcome.fragment.rect.height + outcome.collapsed_top.resolve() <= remaining
                });
            if fits {
                return Some(outcome);
            }
            self.truncate_floats(floats);
            self.multicol_in(id, space, token, sizes, true)
        })
    }

    /// The box that flows through the columns of multi-column container `id`, if it is laid out
    /// in columns. Columns nested deeper than [`NESTED_COLUMNS`] are laid out as one: balancing
    /// them inside the balancing of the outer columns takes time growing exponentially with the
    /// depth.
    fn flow(&self, id: BoxId) -> Option<BoxId> {
        match &self.tree.get(id).kind {
            BoxKind::Block { children } if children.len() == 1 && self.columns.get() < NESTED_COLUMNS => {
                Some(children[0])
            }
            _ => None,
        }
    }

    /// Lays out a multi-column container, leaving room for its bottom border and padding on the
    /// page if `ending`.
    fn multicol_in(
        &self,
        id: BoxId,
        space: Space,
        token: Option<&BreakToken>,
        (width_override, height_override): (Option<Au>, Option<Au>),
        ending: bool,
    ) -> Option<Outcome> {
        let layout_box = self.tree.get(id);
        let style = &layout_box.style;
        let Some(flow) = self.flow(id) else {
            return self.block_container_with(id, space, token, width_override, height_override, true);
        };
        let model = self.flow_model(style, &space, width_override);
        let continuing = token.is_some();
        // A container that clones its decorations has its border and padding on every page it
        // spans, and its columns end above them (`box-decoration-break: clone`).
        let clones = style.clones_decorations();
        let top = if continuing && !clones {
            Au::ZERO
        } else {
            model.border[TOP] + model.padding[TOP]
        };
        let bottom_frame = model.padding[BOTTOM] + model.border[BOTTOM];
        // The frame around the content of a part that does not end the container.
        let broken_frame = if clones { bottom_frame } else { Au::ZERO };
        let left = model.border[LEFT] + model.padding[LEFT];
        let width = model.content_width
            + model.border[LEFT]
            + model.border[RIGHT]
            + model.padding[LEFT]
            + model.padding[RIGHT];
        // The rest of a container of a fixed height below its content goes on as empty space.
        if let Some(BreakToken::Rest { rest }) = token {
            let frames = if clones { top + bottom_frame } else { Au::ZERO };
            let placed = space.remaining.map_or(*rest, |remaining| {
                (*rest).min((remaining - frames).clamp_non_negative())
            });
            let broken = placed < *rest;
            return Some(Outcome::new(
                Fragment {
                    rect: Rect::new(model.margin[LEFT], Au::ZERO, width, placed + frames),
                    kind: layout_box.fragment_kind(VisibleSides {
                        top: clones,
                        bottom: !broken || clones,
                        ..VisibleSides::ALL
                    }),
                    children: Children::new(),
                },
                broken.then(|| BreakToken::Rest { rest: *rest - placed }),
                Au::ZERO,
                if broken { Au::ZERO } else { model.margin[BOTTOM] },
            ));
        }
        let (count, column_width, gap) = columns(style, model.content_width);
        // The container's definite content height: its `height` (a length, or a percentage of a
        // definite containing block height), within `min-height` and `max-height`.
        let definite = Self::definite_height(
            style,
            &space.stretch_model(&model),
            space.containing_height,
            height_override,
        );
        let columns = Columns {
            flow,
            count,
            width: column_width,
            fill: style.column_fill,
            percent_basis: definite,
            trim: (style.text_box_trim.start, style.text_box_trim.end),
        };

        // The height the content may take on this page: what is left of it, and no more than
        // a definite height of the container.
        let reserved = if ending || clones { bottom_frame } else { Au::ZERO };
        // Columns inside the single column in which the columns around them measure their
        // content are laid out as if nothing fragmented them (balanced, whatever their
        // `column-fill`), as Blink's column balancer lays out the content unbroken.
        let remaining = space.remaining.filter(|_| !self.measuring.get());
        let limit = remaining.map(|remaining| {
            (remaining - if continuing { Au::ZERO } else { model.margin[TOP] } - top - reserved)
                .clamp_non_negative()
        });
        // A container broken on the pages before has the rest of its height left.
        let earlier = token.map_or(Au::ZERO, BreakToken::consumed);
        let fixed = definite.map(|fixed| (fixed - earlier).clamp_non_negative());
        // Without a definite height, `max-height` (or a larger `min-height`) still bounds
        // balanced columns, as in Blink, which never makes them taller than the container may
        // be; columns filled in turn take all the content they can.
        let bound = fixed.or_else(|| {
            if style.column_fill == ColumnFill::Auto {
                return None;
            }
            let most = Self::used_height(
                style,
                &space.stretch_model(&model),
                UNBOUNDED,
                space.containing_height,
            );
            (!style.size_containment().1 && most < UNBOUNDED).then(|| (most - earlier).clamp_non_negative())
        });
        let available = match (limit, bound) {
            (Some(limit), Some(bound)) => Some(limit.min(bound)),
            (limit, bound) => limit.or(bound),
        };

        let rule_style = self.rule_style(style);
        let mut children = Vec::new();
        // Where the columns are among the children, and how tall each is for the absolutely
        // positioned boxes placed in them; the top, height and number of columns of the last row.
        let mut column_slots: Vec<(usize, Au)> = Vec::new();
        let mut last_row: Option<(Au, Au, usize)> = None;
        // Where the content continues: where the previous page stopped, then after each spanner.
        let mut position = match token {
            Some(BreakToken::Block { inner, .. }) => inner.as_deref().cloned(),
            _ => None,
        };
        let mut y = top;
        // Whether anything is placed on this page, and whether the content continues right below
        // a spanner placed on it.
        let mut placed = false;
        // A page that starts right below a spanner which ended the page before starts as below
        // one: the margins at its top stay, as Chrome keeps them after a break that a spanner
        // caused.
        let mut below_spanner = position
            .as_ref()
            .is_some_and(|position| self.follows_spanner(flow, flow, position));
        // The bottom margin of the spanner placed last; it collapses with the top margin of a
        // spanner right below it, and stays apart from columns.
        let mut margin = Margin::default();
        // In right-to-left text the columns go from the right.
        let mirror = |x: Au, width: Au| {
            if style.is_rtl() {
                left + model.content_width - (x - left) - width
            } else {
                x
            }
        };
        // The rules before the columns `columns` of a row at `top`, `height` tall (none before
        // the first).
        let rules = |children: &mut Vec<Fragment>, columns: std::ops::Range<usize>, top: Au, height: Au| {
            let Some(rule_style) = &rule_style else {
                return;
            };
            let rule = Au::from_px(style.column_rule_width);
            for index in columns.start.max(1)..columns.end {
                let x = mirror(
                    left + (column_width + gap) * index as i32 - gap / 2 - rule / 2,
                    rule,
                );
                children.push(Fragment {
                    rect: Rect::new(x, top, rule, height),
                    kind: FragmentKind::Box {
                        style: Arc::clone(rule_style),
                        node: None,
                        sides: VisibleSides::ALL,
                    },
                    children: Children::new(),
                });
            }
        };
        // With `column-wrap: wrap`, the content goes on in rows of columns below one another,
        // as tall as `column-height` or the container's height, the rows reaching out of the
        // container (CSS Multi-column Layout 2, §3.4).
        // Fragmentainers are at least a pixel tall (CSS Fragmentation 3, §4.4).
        let column_height = style.column_height.map(|height| Au::from_px(height.max(1.0)));
        let wraps = style.column_wrap.unwrap_or(column_height.is_some());
        let row_cap = column_height.or(bound.filter(|_| wraps));
        let row_gap = style.row_gap.as_ref().map_or(Au::ZERO, |gap| {
            Au::from_px(gap.resolve(fixed.unwrap_or(Au::ZERO).to_px()).max(0.0))
        });
        // The rows lie on a grid of rows `row_cap` tall with gaps between them, from the top of
        // the content, as in Blink: spanners take room from the rows they lie in, and the rows
        // of columns below them take the rest of it.
        let grid = RowGrid {
            height: row_cap.unwrap_or(Au::ZERO),
            gap: row_gap,
        };
        // Whether the row placed last wrapped, so the next one starts in the next row.
        let mut wrapped = false;
        // The rules of the row placed last, unless a spanner follows it.
        let mut last_rules: Option<std::ops::Range<usize>> = None;
        let rest = loop {
            let mut row_top = y + margin.resolve();
            if let Some(cap) = row_cap
                && (wrapped || (wraps && cap > Au::ZERO && grid.remaining(row_top - top) <= Au::ZERO))
            {
                row_top += grid.to_next_row(row_top - top);
            }
            wrapped = false;
            let page_room = limit.map(|limit| (limit - (row_top - top)).clamp_non_negative());
            let room = match row_cap {
                Some(cap) => {
                    // What is left of the row, or all of it where a spanner reached past it in
                    // columns that do not wrap.
                    let left = grid.remaining(row_top - top);
                    let cap = if left > Au::ZERO { left } else { cap };
                    Some(page_room.map_or(cap, |room| room.min(cap)))
                }
                None => available.map(|available| {
                    let room = (available - (row_top - top)).clamp_non_negative();
                    // Columns that the container's own height ends are at least a pixel tall,
                    // as fragmentainers are (CSS Fragmentation 3, §4.4), where they have content.
                    if bound.is_some_and(|bound| limit.is_none_or(|limit| bound <= limit))
                        && self.content_left(flow, position.as_ref())
                    {
                        room.max(Au::from_px(1.0))
                    } else {
                        room
                    }
                }),
            };
            // Below a spanner, the first column may take any break (one that `break-after:
            // avoid` avoids) where the container has no earlier break on the page: it continues
            // from the page before, or starts this one. Elsewhere Chrome breaks before the
            // container instead, an early break that moves all of it to the next page.
            let last_resort = below_spanner && (continuing || space.at_top);
            // Columns that the container's own height or `column-height` ends before the page
            // does are fragmentainers of their own, whose content would not fit better on the
            // next page: what they start with stays in them, overflowing them, as in Chrome.
            let own_height = room.is_some_and(|room| {
                page_room.is_none_or(|page_room| room < page_room)
                    || bound.is_some_and(|bound| limit.is_none_or(|limit| bound <= limit))
            });
            let Some(row) = self.row(
                &columns,
                room,
                (space.at_top && !placed) || own_height,
                position.as_ref(),
                (below_spanner, last_resort),
            ) else {
                if !placed {
                    return None;
                }
                if below_spanner
                    && !last_resort
                    && self
                        .row(&columns, room, false, position.as_ref(), (true, true))
                        .is_some()
                {
                    return None;
                }
                // A row below a spanner that moves to the next page leaves its columns empty
                // on this one, to its end, with the rules between them, as in Chrome.
                if below_spanner && let Some(room) = room.filter(|&room| room > Au::ZERO) {
                    rules(&mut children, 0..columns.count, row_top, room);
                }
                break position;
            };
            if row.height > Au::ZERO {
                y = row_top + row.height;
                margin = Margin::default();
                placed = true;
            } else if row.columns.iter().any(starts_content) {
                // A row without height that holds the start of a box (around a spanner below
                // it) keeps the margins of the spanners around it apart, as in Chrome; one that
                // only goes on with boxes started above does not (Blink's empty spanner parent).
                y = row_top;
                margin = Margin::default();
            }
            // The columns of the row, and the rules in the middle of the gaps between them.
            let row_height = row.height;
            // Columns that fill a container of a fixed height, or the rest of the page or column
            // around them, are as tall as that for the boxes placed in them, whatever their
            // content, as in Blink, whose unbalanced columns are fragmentainers as tall as the
            // space they may take; but a row that a spanner ends is balanced (`column-fill`
            // applies to the last row only), its columns as tall as its content.
            let before_spanner = row
                .rest
                .as_ref()
                .is_some_and(|rest| self.spanner_at(flow, rest).is_some());
            let fragmentainer = match room {
                Some(room) if columns.fill == ColumnFill::Auto && !before_spanner => room.max(row_height),
                _ => row_height,
            };
            let mut place_row = |children: &mut Vec<Fragment>, fragments: Vec<Fragment>| {
                let count = fragments.len();
                // An empty row (after a spanner that ends the content) is no fragmentainer for
                // the absolutely positioned boxes: those that reach past the row before go on in
                // columns added to it.
                let empty = fragmentainer <= Au::ZERO
                    && last_row.is_some()
                    && fragments.iter().all(|fragment| fragment.children.is_empty());
                if !empty {
                    last_row = Some((row_top, fragmentainer, count));
                }
                // The rules go below the content of the columns, which paints over them where it
                // overflows its column, as in Chrome (Blink paints them with the container's
                // background).
                let first_rule = children.len();
                rules(children, 0..count, row_top, row_height);
                last_rules = Some(first_rule..children.len());
                for (index, mut fragment) in fragments.into_iter().enumerate() {
                    fragment.rect.x = mirror(left + (column_width + gap) * index as i32, column_width);
                    fragment.rect.y = row_top;
                    if !empty {
                        column_slots.push((children.len(), fragmentainer));
                    }
                    children.push(fragment);
                }
            };
            let mut row_columns = row.columns;
            let Some(rest) = row.rest else {
                place_row(&mut children, row_columns);
                break None;
            };
            let rest = if self.spanner_at(flow, &rest).is_some() {
                rest
            } else {
                // Wrapping rows go on below, where the page has room for more.
                if wraps
                    && let Some(cap) = row_cap
                    && row_height > Au::ZERO
                    && page_room.is_none_or(|room| {
                        room - row_height - grid.to_next_row(row_top + row_height - top) >= cap
                    })
                {
                    place_row(&mut children, row_columns);
                    y = row_top + row_height;
                    wrapped = true;
                    position = Some(rest);
                    below_spanner = false;
                    continue;
                }
                // Where the container's own height ends the columns before the page does, the
                // content goes on in columns beyond them in the row, overflowing the container
                // to the side, as in Chrome; but wrapping rows go on on the next page instead
                // (empty rows hold nothing anywhere).
                if !(wraps && row_height > Au::ZERO)
                    && (bound.is_some_and(|bound| limit.is_none_or(|limit| bound <= limit))
                        || column_height.is_some_and(|cap| page_room.is_none_or(|room| cap <= room)))
                    && let Some(room) = room
                {
                    let overflow = Columns {
                        fill: ColumnFill::Auto,
                        ..columns
                    };
                    let mut rest = Some(rest);
                    while let Some(at) = rest.take() {
                        let more = (self.spanner_at(flow, &at).is_none())
                            // Each of them a fragmentainer of its own, which holds what it
                            // starts with.
                            .then(|| self.row(&overflow, Some(room), true, Some(&at), (false, false)))
                            .flatten()
                            .filter(|more| !more.columns.is_empty());
                        let Some(more) = more else {
                            rest = Some(at);
                            break;
                        };
                        row_columns.extend(more.columns);
                        rest = more.rest;
                        if row_columns.len() > 10_000 {
                            break;
                        }
                    }
                    // A spanner that the content of the overflowing columns stops at follows
                    // them below the container's height, as in Chrome.
                    match rest {
                        Some(at) if self.spanner_at(flow, &at).is_some() => at,
                        rest => {
                            place_row(&mut children, row_columns);
                            break rest;
                        }
                    }
                } else {
                    // The page is full.
                    place_row(&mut children, row_columns);
                    break Some(rest);
                }
            };
            let Some((spanner, inside)) = self.spanner_at(flow, &rest) else {
                place_row(&mut children, row_columns);
                break Some(rest);
            };
            place_row(&mut children, row_columns);

            // The spanner, across all columns. Margins at the top of a page after a break are
            // truncated, and those at the start of the container by `margin-trim: block-start`
            // (CSS Box 4, §3.3).
            let truncated =
                inside.is_some() || (continuing && !placed) || (style.margin_trim_block.0 && !placed);
            let margin_top = if truncated {
                Au::ZERO
            } else {
                self.vertical_margin(&self.tree.get(spanner).style, model.content_width, true)
            };
            let estimate = margin.adjoin(Margin::of(margin_top)).resolve();
            let lay_out_spanner = |y: Au| {
                self.block_level(
                    spanner,
                    Space {
                        containing_width: model.content_width,
                        containing_height: columns.percent_basis,
                        // The container's own height does not end a spanner, which overflows
                        // it, as in Chrome: only the page does.
                        remaining: limit.map(|limit| limit - (y - top) - estimate + margin_top),
                        at_top: space.at_top && !placed,
                        flush: true,
                        bfc_left: Au::ZERO,
                        bfc_top: Au::ZERO,
                        rtl: style.is_rtl(),
                        stretch_skips_margins: (false, false),
                    },
                    inside,
                )
            };
            let mut outcome = lay_out_spanner(y);
            // A spanner that does not fit in the rest of a wrapping row it does not start goes
            // in the next row, as in Blink, unless that row does not fit on the page.
            if wraps
                && let Some(cap) = row_cap
                && let Some(laid_out) = &outcome
                && grid.offset_in_row(y + estimate - top) > Au::ZERO
                && grid.remaining(y - top) < estimate + laid_out.fragment.rect.height
            {
                y += grid.remaining(y - top) + row_gap;
                if limit.is_some_and(|limit| cap > limit - (y - top)) {
                    outcome = None;
                } else {
                    outcome = lay_out_spanner(y);
                }
            }
            let Some(outcome) = outcome else {
                // It moves to the next page, along with the container if nothing stays here.
                if !placed {
                    return None;
                }
                break Some(rest);
            };
            let spanner_top = if truncated {
                Margin::default()
            } else {
                outcome.collapsed_top
            };
            let mut fragment = outcome.fragment;
            fragment.rect.x += left;
            fragment.rect.y = y + margin.adjoin(spanner_top).resolve();
            y = fragment.rect.y + fragment.rect.height;
            margin = outcome.collapsed_bottom;
            children.push(fragment);
            last_rules = None;
            placed = true;
            if let Some(inner) = outcome.token {
                // The spanner continues on the next page.
                break self.past_spanner(flow, flow, &rest, Some(inner));
            }
            position = self.past_spanner(flow, flow, &rest, None);
            below_spanner = true;
        };

        // The absolutely positioned boxes in the columns, and the columns they add after the
        // last row.
        if let Some((row_top, row_height, row_count)) = last_row {
            let heights: Vec<Au> = column_slots.iter().map(|&(_, height)| height).collect();
            let mut fragments: Vec<Fragment> = column_slots
                .iter()
                .map(|&(index, _)| {
                    std::mem::replace(
                        &mut children[index],
                        Fragment {
                            rect: Rect::new(Au::ZERO, Au::ZERO, Au::ZERO, Au::ZERO),
                            kind: FragmentKind::Anonymous,
                            children: Children::new(),
                        },
                    )
                })
                .collect();
            self.position_in_columns(&mut fragments, &heights, column_width);
            let mut fragments = fragments.into_iter();
            for (&(index, _), fragment) in column_slots.iter().zip(fragments.by_ref()) {
                children[index] = fragment;
            }
            for (n, mut fragment) in fragments.enumerate() {
                let index = row_count + n;
                fragment.rect.x = mirror(left + (column_width + gap) * index as i32, column_width);
                fragment.rect.y = row_top;
                fragment.rect.height = row_height;
                children.push(fragment);
            }
        }

        let mut broken = rest.is_some();
        // `margin-trim: block-end` trims the bottom margin of a spanner at the end.
        if !broken && !style.margin_trim_block.1 {
            y += margin.resolve();
        }
        // The last row takes all of `column-height` too, but no more than the page has left, as
        // in Blink.
        if column_height.is_some() {
            let left = grid.remaining(y - top);
            let left = limit
                .map_or(left, |limit| left.min(limit - (y - top)))
                .clamp_non_negative();
            if left < grid.height {
                y += left;
            }
        }
        let mut content_height = match (broken, fixed) {
            (false, Some(fixed)) => fixed,
            // Size containment leaves the container as tall as if empty, within `min-height` and
            // `max-height` (CSS Contain 2, §3.1), its columns overflowing it, as in Chrome.
            (false, None) if style.size_containment().1 => (Self::used_height(
                style,
                &space.stretch_model(&model),
                Au::ZERO,
                space.containing_height,
            ) - earlier)
                .clamp_non_negative(),
            _ => y - top,
        };
        let mut next = rest.map(|rest| {
            let mut token = BreakToken::at(0, Some(Box::new(rest)));
            token.set_consumed(earlier + content_height);
            token
        });
        // A container of a fixed height whose content ends on the page, but not the box, breaks
        // at the end of the page; the rest of it goes on on the next page, as in Chrome.
        if !broken
            && fixed.is_some()
            && let Some(limit) = limit
            && content_height > limit
            && y - top <= limit
        {
            next = Some(BreakToken::Rest {
                rest: content_height - limit + bottom_frame - broken_frame,
            });
            content_height = limit;
            broken = true;
        }
        // The rules of the last row reach down to the end of the content box, as in Chrome, where
        // the container's height (or the page) ends it below the columns.
        if !wraps && column_height.is_none() {
            for rule in last_rules.map_or(&mut [][..], |range| &mut children[range]) {
                rule.rect.height = rule.rect.height.max(top + content_height - rule.rect.y);
            }
        }
        let height = top + content_height + if broken { broken_frame } else { bottom_frame };
        let kind = layout_box.fragment_kind(VisibleSides {
            top: !continuing || clones,
            bottom: !broken || clones,
            ..VisibleSides::ALL
        });
        Some(Outcome::new(
            Fragment {
                rect: Rect::new(model.margin[LEFT], Au::ZERO, width, height),
                kind,
                children: children.into(),
            },
            next,
            if continuing { Au::ZERO } else { model.margin[TOP] },
            if broken { Au::ZERO } else { model.margin[BOTTOM] },
        ))
    }

    /// Lays out the content from `start` in a row of columns, up to a spanner, the end of the
    /// content, or the end of the `room` there is. `None` means that not even its first line
    /// fits.
    fn row(
        &self,
        columns: &Columns,
        room: Option<Au>,
        at_top: bool,
        start: Option<&BreakToken>,
        (below_spanner, last_resort): (bool, bool),
    ) -> Option<Row> {
        // One column: a formatting context and a fragmentainer of its own. `after` tells
        // whether the column before it ended with a forced break (`None` for the first column,
        // which starts like the page); the column tells whether it ends with one.
        let page_after_forced_break = self.after_forced_break.get();
        let column = |height: Option<Au>,
                      at_top: bool,
                      token: Option<&BreakToken>,
                      after: Option<bool>,
                      measuring: bool| {
            let scope = Scope {
                below_spanner: after.is_none() && below_spanner,
                measuring,
                after_forced_break: after.unwrap_or(page_after_forced_break),
                height,
                trim: columns.trim,
            };
            let _columns = ColumnScope::open(self, columns.flow, scope);
            // The container's own first and last lines are trimmed as the content's.
            if columns.trim != (false, false) {
                self.trim_pending
                    .set((columns.trim.0 && token.is_none(), columns.trim.1));
            }
            let outcome = self.block_container_with(
                columns.flow,
                Space {
                    containing_width: columns.width,
                    containing_height: columns.percent_basis,
                    remaining: height,
                    at_top,
                    flush: true,
                    bfc_left: Au::ZERO,
                    bfc_top: Au::ZERO,
                    rtl: false,
                    stretch_skips_margins: (false, false),
                },
                token,
                None,
                None,
                true,
            )?;
            Some((outcome, self.forced_break.get()))
        };
        // The columns at one height, as many as there are and the content needs. A column places
        // what it starts with even where it does not fit (or leaves fewer lines than `orphans`
        // and `widows` ask for), unless `strict`, as when balancing the columns: they then grow
        // instead, as in Chrome.
        let fill = |height: Option<Au>, strict: bool| -> Option<Filled> {
            let mut fragments = Vec::new();
            let mut current = start.cloned();
            let mut after = None;
            // Where the last column started, and whether the first took a poor break.
            let mut last = (start.cloned(), after);
            let mut poor = false;
            for index in 0..columns.count {
                // A column is at the top of a page where its row is: below content on the page,
                // what it starts with goes on in the first column of the next page where it does
                // not fit, as in Chrome, which leaves the column empty.
                let column_top = !strict && at_top;
                last = (current.clone(), after);
                let laid_out = column(height, column_top, current.as_ref(), after, false);
                if laid_out.is_none() && index > 0 && !strict && !at_top {
                    break;
                }
                // The first column takes a poor break where it has no other, if `last_resort`,
                // rather than moving the row to the next page, as in Chrome; balanced columns
                // grow instead.
                let laid_out = if laid_out.is_none() && index == 0 && last_resort && !strict && !column_top {
                    self.last_resort_column.set(true);
                    let mut laid_out = column(height, column_top, current.as_ref(), after, false);
                    // A break between the lines of the block before a break that `break-after`
                    // avoids, even one that leaves fewer lines than `orphans` and `widows` ask
                    // for, is a better one in Chrome (`kBreakAppealViolatingOrphansAndWidows`
                    // ranks above `kBreakAppealViolatingBreakAvoid`): the column ends above the
                    // block's last line instead.
                    if let Some((outcome, _)) = &laid_out
                        && let Some(cut) = last_line_before_avoided_break(&outcome.fragment)
                        && height.is_none_or(|height| cut < height)
                        && let Some((mut inside, forced)) =
                            column(Some(cut), column_top, current.as_ref(), after, false)
                        && inside.token != outcome.token
                    {
                        inside.fragment.rect.height =
                            inside.fragment.rect.height.max(outcome.fragment.rect.height);
                        laid_out = Some((inside, forced));
                    }
                    self.last_resort_column.set(false);
                    poor = laid_out.is_some();
                    laid_out
                } else {
                    laid_out
                };
                let (outcome, forced) = laid_out?;
                fragments.push(outcome.fragment);
                current = outcome.token;
                after = Some(forced);
                // Floats and overflowing content broken at the end of the column (flows parallel
                // to the content) go on in the next one even where the content stops at a
                // spanner, which then follows them, as in Chrome.
                if !self.continues(columns.flow, current.as_ref())
                    && current.as_ref().is_none_or(|token| !parallel_flows(token))
                {
                    break;
                }
            }
            // Chrome keeps such a row on the page only where its last column ends with a break
            // that leaves all it asks for (`MovePastBreakpoint` looks at the last column): the
            // last column laid out as one not at the top of a page breaks at the same place, and
            // not inside a paragraph that it holds fewer lines of than `orphans` asks for (which
            // Blink counts in every column the paragraph spans; see `breaks_poorly`).
            if poor && self.continues(columns.flow, current.as_ref()) {
                let (outcome, _) = column(height, false, last.0.as_ref(), last.1, false)?;
                if outcome.token != current || fragments.last().is_some_and(breaks_poorly) {
                    return None;
                }
            }
            Some((fragments, current))
        };

        // The content in a single column, where it ends or stops at a spanner, measured no
        // further than the columns can hold on this page: on long content, every page would
        // measure the rest of it otherwise.
        let measure =
            |height: Option<Au>| column(height, true, start, None, true).map(|(outcome, _)| outcome);
        let count = i32::try_from(columns.count).unwrap_or(i32::MAX);
        let capacity = room.map(|room| room * count + Au::EPSILON);
        // Columns laid out unbroken inside the single column in which the columns around them
        // measure their content are measured no further than that column holds either: where
        // their content is taller than as many columns as tall as it, they are taller than it,
        // and that is all the measuring around needs (see `Layouter::too_tall_columns`).
        if let Some(cap) = self.too_tall_columns(room) {
            let capacity = cap * count + Au::EPSILON;
            let capped = measure(Some(capacity))?;
            let content = capped
                .fragment
                .rect
                .height
                .max(crate::parallel::flow_extent(&capped.fragment));
            if self.continues(columns.flow, capped.token.as_ref()) || content > capacity {
                return Some(Row {
                    columns: Vec::new(),
                    height: cap + Au::EPSILON,
                    rest: None,
                });
            }
        }
        let mut natural = measure(capacity)?;
        if let Some(room) = room
            && self.continues(columns.flow, natural.token.as_ref())
        {
            // The columns fill the room, unless margins truncated at their breaks leave room
            // for all of the content after all.
            let (fragments, rest) = fill(Some(room), false)?;
            if self.continues(columns.flow, rest.as_ref()) {
                return Some(Row {
                    columns: fragments,
                    height: room,
                    rest,
                });
            }
            natural = measure(None)?;
        }
        // Absolutely positioned boxes placed in the columns need room in them as their content
        // does: Blink's column balancer lays them out in the columns too.
        let positioned = has_placeholder(&natural.fragment);
        // Content that overflows its boxes is balanced with them, as in Chrome.
        let content = match natural
            .fragment
            .rect
            .height
            .max(crate::parallel::flow_extent(&natural.fragment))
        {
            content if positioned && content > Au::ZERO => {
                let mut single = vec![natural.fragment.clone()];
                content.max(self.position_in_columns(&mut single, &[content], columns.width))
            }
            // Where they are all there is, the columns hold them as they would content as tall:
            // they drive the balancing (and that of columns around), as in Blink.
            Au::ZERO if positioned => {
                let mut single = vec![natural.fragment.clone()];
                self.position_in_columns(&mut single, &[UNBOUNDED], columns.width);
                single
                    .first()
                    .map_or(Au::ZERO, crate::parallel::flow_extent)
                    .min(UNBOUNDED)
            }
            content => content,
        };
        if columns.fill == ColumnFill::Auto
            && natural.token.is_none()
            && let Some(room) = room
        {
            let (fragments, rest) = fill(Some(room), false)?;
            // Where the content ends in them, the columns are as tall as it, with what overflows
            // its boxes, as in Blink (`BlockSizeForFragmentation`).
            let height = if rest.is_some() {
                room
            } else {
                fragments
                    .iter()
                    .map(|fragment| fragment.rect.height.max(crate::parallel::flow_extent(fragment)))
                    .max()
                    .unwrap_or(Au::ZERO)
            };
            return Some(Row {
                columns: fragments,
                height,
                rest,
            });
        }
        // Where the content of a column ends: not at the bottom margin it ends with, which is
        // truncated at the end of the column, as in Chrome.
        let content_bottom = |fragment: &Fragment| {
            fragment
                .children
                .iter()
                .map(|child| child.rect.y + child.rect.height)
                .max()
                .unwrap_or(fragment.rect.height)
        };
        // Balanced: the lowest height from an even share of the content on at which the columns
        // hold it, found by bisection, or columns filling the room if they cannot.
        let upper = room.map_or(content, |room| room.min(content));
        let mut best = fill(Some(upper), false)?;
        let mut height = upper;
        // Content that goes on after the columns at the height of all of it (forced breaks
        // leave it more columns than there are) goes on on the next page: the columns take the
        // room there is on this one, as in Chrome.
        if let Some(room) = room
            && upper < room
            && self.continues(columns.flow, best.1.as_ref())
        {
            best = fill(Some(room), false)?;
            height = room;
        }
        if !self.continues(columns.flow, best.1.as_ref()) {
            // An even share of the content, rounded up: the columns are that tall if they hold
            // the content at it, as in browsers.
            let count = i32::try_from(columns.count).unwrap_or(i32::MAX);
            let share = Au(content.clamp_non_negative().0.saturating_add(count - 1) / count).min(upper);
            // Floats that go on past the last column do not fit either, where the content stops
            // at a spanner, which would otherwise follow them.
            let fits = |height: Au| {
                fill(Some(height), true)
                    .filter(|filled| !self.continues(columns.flow, filled.1.as_ref()))
                    .filter(|filled| filled.1.as_ref().is_none_or(|rest| !parallel_flows(rest)))
                    .filter(|filled| {
                        !positioned || self.positioned_fit(&filled.0, height, columns.count, columns.width)
                    })
            };
            // As Blink's column balancer: from the even share, the columns grow by the least room
            // that content which did not fit at the end of one of them needed (the minimal space
            // shortage), until they hold all of the content. Where no such room is known (or it
            // takes too many steps), the lowest height at which they do is found by bisection.
            let mut low = share;
            let mut stepped = None;
            for _ in 0..BALANCING_STEPS {
                self.space_shortage.set(None);
                self.shortage_taken.set(self.shortage_taken.get() + 1);
                let filled = fits(low);
                self.shortage_taken.set(self.shortage_taken.get() - 1);
                let shortage = self.space_shortage.take();
                if let Some(filled) = filled {
                    stepped = Some(filled);
                    break;
                }
                match shortage {
                    Some(shortage) if low + shortage < height => low += shortage,
                    _ => break,
                }
            }
            if let Some(filled) = stepped {
                best = filled;
                height = low;
            } else {
                // Twelve halvings come within a quarter pixel for columns up to a thousand
                // pixels tall; the result always fits.
                for _ in 0..12 {
                    if height - low <= Au::from_px(0.25) {
                        break;
                    }
                    let middle = (low + height) / 2;
                    if let Some(filled) = fits(middle) {
                        best = filled;
                        height = middle;
                    } else {
                        low = middle;
                    }
                }
                // Chrome stretches the columns from the even share by just as much as the content
                // that did not fit needs: they end at the bottom of the content of the tallest
                // column, not somewhere in the quarter pixel the bisection leaves.
                let tallest_content = best.0.iter().map(column_content_end).max().unwrap_or(Au::ZERO);
                if tallest_content < height
                    && let Some(filled) = fits(tallest_content)
                {
                    best = filled;
                    height = tallest_content;
                }
            }
        }
        // Content taller than the columns makes them taller.
        let tallest_content = best.0.iter().map(content_bottom).max().unwrap_or(Au::ZERO);
        Some(Row {
            height: height.max(tallest_content),
            columns: best.0,
            rest: best.1,
        })
    }

    /// The height of the single column in which the columns around measure their content, where
    /// columns laid out in it unbroken (with no `room`) need only be measured as far as it holds
    /// (see `Layouter::row`). Balanced columns are at least as tall as an even share of their
    /// content; where that is taller than the column, they do not fit in it: it breaks before
    /// them, or they overflow it at its top, and the measuring around (see the callers of
    /// `measure`) needs no more than that its content is taller than it. Not in documents with
    /// spanners, at which the content may stop, or with absolutely positioned boxes, whose places
    /// in the columns the measuring looks for further on, nor while the space shortage of the
    /// content being laid out is taken, which the content further on adds to.
    fn too_tall_columns(&self, room: Option<Au>) -> Option<Au> {
        if room.is_some()
            || !self.measuring.get()
            || self.shortage_taken.get() > 0
            || self.tree.has_spanners()
            || *self
                .has_out_of_flow
                .get_or_init(|| self.tree.any_box(|layout_box| layout_box.style.is_out_of_flow()))
        {
            return None;
        }
        self.column_height.get()
    }

    /// Whether content of the columns continues after `token`, in the next column: it does not
    /// end there, nor stop at a spanner.
    fn continues(&self, flow: BoxId, token: Option<&BreakToken>) -> bool {
        token.is_some_and(|token| !self.tree.has_spanners() || self.spanner_at(flow, token).is_none())
    }

    /// Whether layout of box `id` stopped at `token` for a spanner of the columns being laid
    /// out.
    pub(crate) fn stops_at_spanner(&self, id: BoxId, token: &BreakToken) -> bool {
        self.tree.has_spanners()
            && self
                .spanning
                .get()
                .is_some_and(|flow| self.spanner_in(flow, id, token).is_some())
    }

    /// Whether content of box `parent` is left to lay out at `token` (all of it without one).
    fn content_left(&self, parent: BoxId, token: Option<&BreakToken>) -> bool {
        let BoxKind::Block { children } = &self.tree.get(parent).kind else {
            return true;
        };
        match token {
            None => !children.is_empty(),
            Some(BreakToken::Block {
                index,
                inner,
                overflow,
                ..
            }) => {
                let inside = inner.as_deref().is_some_and(|inner| {
                    children
                        .get(*index)
                        .is_none_or(|&child| self.content_left(child, Some(inner)))
                });
                let from = if inner.is_some() { index + 1 } else { *index };
                inside || !overflow.is_empty() || children.len() > from
            }
            Some(_) => true,
        }
    }

    /// Whether the content of box `id` holds a spanner of the columns being laid out after
    /// `token`, where it goes on.
    pub(crate) fn spanner_ahead(&self, id: BoxId, token: &BreakToken) -> bool {
        let Some(flow) = self.spanning.get() else {
            return false;
        };
        let spans =
            |child: BoxId| self.tree.spanned_flow(child) == Some(flow) || self.tree.holds_spanner(child);
        let mut parent = id;
        let mut token = token;
        loop {
            if let BreakToken::Overflow { inner } = token {
                token = inner;
                continue;
            }
            let (BreakToken::Block { index, inner, .. }, BoxKind::Block { children }) =
                (token, &self.tree.get(parent).kind)
            else {
                return false;
            };
            if children.iter().skip(index + 1).any(|&child| spans(child)) {
                return true;
            }
            let Some(&child) = children.get(*index) else {
                return false;
            };
            match inner.as_deref() {
                Some(inner) if self.tree.holds_spanner(child) => {
                    parent = child;
                    token = inner;
                }
                Some(_) => return false,
                None => return spans(child),
            }
        }
    }

    /// The spanner of the columns of `flow` at which their content stops at `token`, with where
    /// it continues inside the spanner.
    fn spanner_at<'t>(&self, flow: BoxId, token: &'t BreakToken) -> Option<(BoxId, Option<&'t BreakToken>)> {
        self.spanner_in(flow, flow, token)
    }

    /// The spanner of the columns of `flow` at which content of `parent` stops at `token`.
    fn spanner_in<'t>(
        &self,
        flow: BoxId,
        parent: BoxId,
        token: &'t BreakToken,
    ) -> Option<(BoxId, Option<&'t BreakToken>)> {
        let mut parent = parent;
        let mut token = token;
        loop {
            // The content of a box that ended above but overflows it goes on in its children.
            if let BreakToken::Overflow { inner } = token {
                token = inner;
                continue;
            }
            let (BreakToken::Block { index, inner, .. }, BoxKind::Block { children }) =
                (token, &self.tree.get(parent).kind)
            else {
                return None;
            };
            let child = *children.get(*index)?;
            if self.tree.spanned_flow(child) == Some(flow) {
                return Some((child, inner.as_deref()));
            }
            parent = child;
            token = inner.as_deref()?;
        }
    }

    /// Whether content of `parent` continues at `token` right after a spanner of the columns of
    /// `flow`, of which nothing is laid out yet.
    fn follows_spanner(&self, flow: BoxId, parent: BoxId, token: &BreakToken) -> bool {
        let mut parent = parent;
        let mut token = token;
        loop {
            if let BreakToken::Overflow { inner } = token {
                token = inner;
                continue;
            }
            let (BreakToken::Block { index, inner, .. }, BoxKind::Block { children }) =
                (token, &self.tree.get(parent).kind)
            else {
                return false;
            };
            match inner.as_deref() {
                Some(inner) => {
                    let Some(&child) = children.get(*index) else {
                        return false;
                    };
                    parent = child;
                    token = inner;
                }
                None => {
                    return index
                        .checked_sub(1)
                        .and_then(|before| children.get(before))
                        .is_some_and(|&before| self.tree.spanned_flow(before) == Some(flow));
                }
            }
        }
    }

    /// A token inside `parent` that stops at a spanner of the columns of `flow`, moved on:
    /// past the spanner, or into it to `inside`.
    fn past_spanner(
        &self,
        flow: BoxId,
        parent: BoxId,
        token: &BreakToken,
        inside: Option<BreakToken>,
    ) -> Option<BreakToken> {
        if let BreakToken::Overflow { inner } = token {
            return Some(BreakToken::Overflow {
                inner: Box::new(self.past_spanner(flow, parent, inner, inside)?),
            });
        }
        let (
            BreakToken::Block {
                index,
                inner,
                consumed,
                floats,
                overflow,
            },
            BoxKind::Block { children },
        ) = (token, &self.tree.get(parent).kind)
        else {
            return None;
        };
        let child = *children.get(*index)?;
        let consumed = *consumed;
        Some(if self.tree.spanned_flow(child) == Some(flow) {
            match inside {
                Some(inside) => BreakToken::Block {
                    index: *index,
                    inner: Some(Box::new(inside)),
                    consumed,
                    floats: floats.clone(),
                    overflow: overflow.clone(),
                },
                None => BreakToken::Block {
                    index: index + 1,
                    inner: None,
                    consumed,
                    floats: floats.clone(),
                    overflow: overflow.clone(),
                },
            }
        } else {
            BreakToken::Block {
                index: *index,
                inner: Some(Box::new(self.past_spanner(
                    flow,
                    child,
                    inner.as_deref()?,
                    inside,
                )?)),
                consumed,
                floats: floats.clone(),
                overflow: overflow.clone(),
            }
        })
    }

    /// The style of the rules between columns, if they have any.
    fn rule_style(&self, style: &ComputedStyle) -> Option<Arc<ComputedStyle>> {
        if matches!(style.column_rule_style, BorderStyle::None | BorderStyle::Hidden)
            || Au::from_px(style.column_rule_width) <= Au::ZERO
        {
            return None;
        }
        Some(Arc::new(ComputedStyle::from(ComputedValues {
            color: style.color,
            border_width: Sides {
                left: style.column_rule_width,
                ..Sides::default()
            },
            border_style: Sides {
                left: style.column_rule_style,
                ..Sides::default()
            },
            border_color: Sides {
                left: style.column_rule_color,
                ..Sides::default()
            },
            ..ComputedValues::default()
        })))
    }
}

/// Whether floats or overflowing content of the content box go on after `token`.
fn parallel_flows(token: &BreakToken) -> bool {
    !token.floats().is_empty() || matches!(token, BreakToken::Block { overflow, .. } if !overflow.is_empty())
}

/// Rows of columns of a fixed height (`column-height`, or the height of a container whose
/// columns wrap) with gaps between them, laid out from the top of the content.
#[derive(Clone, Copy)]
struct RowGrid {
    height: Au,
    gap: Au,
}

impl RowGrid {
    /// How far `offset` from the top of the content lies into its row (or the gap after it).
    fn offset_in_row(self, offset: Au) -> Au {
        let stride = self.height + self.gap;
        if stride <= Au::ZERO {
            return Au::ZERO;
        }
        Au(offset.clamp_non_negative().0 % stride.0)
    }

    /// How much of its row is left below `offset` (negative in the gap after it).
    fn remaining(self, offset: Au) -> Au {
        self.height - self.offset_in_row(offset)
    }

    /// How far below `offset` the next row starts: past the rest of this row and the gap.
    fn to_next_row(self, offset: Au) -> Au {
        let within = self.offset_in_row(offset);
        let to_end = if within > Au::ZERO {
            self.height - within
        } else {
            Au::ZERO
        };
        to_end + self.gap
    }
}

/// Whether `fragment` holds content that starts in it: anything but boxes that go on from above
/// without such content.
fn starts_content(fragment: &Fragment) -> bool {
    fragment.children.iter().any(|child| match &child.kind {
        FragmentKind::Box { sides, .. } if !sides.top => starts_content(child),
        _ => true,
    })
}

/// Whether `column` ends with a break inside a paragraph that leaves fewer of its lines in it
/// than `orphans` asks for, or than `widows` asks for where the paragraph goes on from the
/// column before (which a break leaves on both sides): a break that Blink deems a violation
/// (`kBreakAppealViolatingOrphansAndWidows`, see `BlockLayoutAlgorithm::BreakBeforeChildIfNeeded`)
/// even where it allows it, at the top of a column.
fn breaks_poorly(column: &Fragment) -> bool {
    let mut fragment = column;
    loop {
        let Some(last) = fragment.children.iter().rev().find(|child| {
            matches!(&child.kind, FragmentKind::Box { style, .. }
                if !style.is_floating() && !style.is_out_of_flow())
        }) else {
            return false;
        };
        let FragmentKind::Box { style, sides, .. } = &last.kind else {
            return false;
        };
        if sides.bottom {
            return false;
        }
        let lines = last
            .children
            .iter()
            .filter(|child| matches!(child.kind, FragmentKind::Line { .. }))
            .count();
        if lines > 0 {
            let asked = if sides.top {
                style.orphans
            } else {
                style.orphans.max(style.widows)
            };
            return lines < asked as usize;
        }
        fragment = last;
    }
}

/// Where the last line of the last block in `column` starts, if the column breaks right after
/// that block, which has more than one line and `break-after: avoid`.
fn last_line_before_avoided_break(column: &Fragment) -> Option<Au> {
    let block = column
        .children
        .iter()
        .rev()
        .find(|child| matches!(child.kind, FragmentKind::Box { .. }))?;
    let FragmentKind::Box { style, sides, .. } = &block.kind else {
        return None;
    };
    if !sides.bottom || !matches!(style.break_after, BreakBetween::Avoid | BreakBetween::AvoidColumn) {
        return None;
    }
    let mut lines = block
        .children
        .iter()
        .filter(|child| matches!(child.kind, FragmentKind::Line { .. }));
    let last = lines.next_back()?;
    lines.next_back()?;
    Some(block.rect.y + last.rect.y)
}

/// How a column starts: right below a spanner, measured in one column, or after a forced break.
#[derive(Clone, Copy)]
#[allow(clippy::struct_excessive_bools, reason = "independent states of the column")]
struct Scope {
    below_spanner: bool,
    measuring: bool,
    after_forced_break: bool,
    /// The height of the column, if it is known.
    height: Option<Au>,
    /// Whether lines at the start and end of the column are trimmed (see
    /// [`Layouter::fragmentainer_trim`]).
    trim: (bool, bool),
}

/// Marks layout inside a column of the content box `flow`, where `break-before: column` and
/// `break-after: column` break and spanners end the row of columns, with the state of the
/// layout outside it kept to restore.
struct ColumnScope<'l, 'a> {
    layouter: &'l Layouter<'a>,
    spanning: Option<BoxId>,
    outside: Scope,
    forced_break: bool,
}

impl<'l, 'a> ColumnScope<'l, 'a> {
    fn open(layouter: &'l Layouter<'a>, flow: BoxId, scope: Scope) -> Self {
        layouter.push_context();
        layouter.columns.set(layouter.columns.get() + 1);
        Self {
            layouter,
            spanning: layouter.spanning.replace(Some(flow)),
            outside: Scope {
                below_spanner: layouter.below_spanner.replace(scope.below_spanner),
                measuring: layouter.measuring.replace(scope.measuring),
                after_forced_break: layouter.after_forced_break.replace(scope.after_forced_break),
                height: layouter.column_height.replace(scope.height),
                trim: layouter.fragmentainer_trim.replace({
                    let outer = layouter.fragmentainer_trim.get();
                    (outer.0 || scope.trim.0, outer.1 || scope.trim.1)
                }),
            },
            forced_break: layouter.forced_break.replace(false),
        }
    }
}

impl Drop for ColumnScope<'_, '_> {
    fn drop(&mut self) {
        let layouter = self.layouter;
        layouter.forced_break.set(self.forced_break);
        layouter.after_forced_break.set(self.outside.after_forced_break);
        layouter.measuring.set(self.outside.measuring);
        layouter.column_height.set(self.outside.height);
        layouter.fragmentainer_trim.set(self.outside.trim);
        layouter.below_spanner.set(self.outside.below_spanner);
        layouter.spanning.set(self.spanning);
        layouter.columns.set(layouter.columns.get() - 1);
        layouter.pop_context();
    }
}

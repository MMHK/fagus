//! Absolutely positioned boxes in columns. A box whose containing block lies in the columns of
//! a multi-column container is placed in the columns laid end to end (those of all its rows on
//! the page, in order), as Blink places it in the fragmentainers of the container: in its whole
//! containing block (the fragments of a block broken across columns put together), in the column
//! its top falls in, broken where it crosses the end of a column and continued at the top of the
//! next. Where it reaches past the last column, columns are added after it in its row, beside
//! the container, as in Chrome; but a box whose containing block clips its overflow or lies in
//! a box that does adds none, and the rest of it stays in the last column.
//!
//! Boxes whose containing block is the container or a box around it are placed later, on the
//! page (see [`crate::positioned`]).

use std::sync::Arc;

use fagus_style::values::Overflow;
use rustc_hash::FxHashSet;

use crate::block::Layouter;
use crate::fragment::{Children, Fragment, FragmentKind, VisibleSides};
use crate::positioned::{ChainKey, PageFlow, block_extent};
use crate::units::{Au, Rect};

/// How many columns absolutely positioned boxes may add after the last column.
const MAX_ADDED_COLUMNS: usize = 1000;

/// How many passes place boxes inside boxes placed in the passes before.
const MAX_PASSES: usize = 16;

/// How many fragments of absolutely positioned boxes lie below `fragment`.
fn absolutes(fragment: &Fragment) -> usize {
    fragment
        .children
        .iter()
        .map(|child| {
            let absolute = matches!(&child.kind, FragmentKind::Box { style, .. } if style.is_out_of_flow());
            usize::from(absolute) + absolutes(child)
        })
        .sum()
}

/// Whether a placeholder of an absolutely positioned box lies below `fragment`.
pub(crate) fn has_placeholder(fragment: &Fragment) -> bool {
    fragment
        .children
        .iter()
        .any(|child| matches!(child.kind, FragmentKind::Placeholder(_)) || has_placeholder(child))
}

/// Whether a box with `style` clips its overflow in the block direction: one that clips it only
/// in the inline direction (`overflow-x: clip`) lets the boxes inside break across columns and
/// pages, as in Blink (WPT css-break out-of-flow-in-multicolumn-109). (In vertical writing
/// modes, either direction counts.)
pub(crate) fn clips_block_overflow(style: &fagus_style::ComputedStyle) -> bool {
    style.overflow_y != Overflow::Visible || (style.is_vertical() && style.overflow_x != Overflow::Visible)
}

/// Whether a box on `path` from `column` down to the fragment at its end, that one included,
/// clips its overflow in the block direction.
fn inside_clip(column: &Fragment, path: &[usize]) -> bool {
    let clips = |fragment: &Fragment| matches!(&fragment.kind, FragmentKind::Box { style, .. } if clips_block_overflow(style));
    let mut fragment = column;
    for &index in path {
        match fragment.children.get(index) {
            Some(child) => fragment = child,
            None => return false,
        }
        if clips(fragment) {
            return true;
        }
    }
    false
}

/// Empty fragments (no height, no top or bottom border) of the boxes on `path` from `column`
/// that hold `part` in a column where its containing block has no fragment, as in Blink: the
/// box at the end of `path`, its containing block, and those around it that contain fixed boxes
/// (see [`crate::contained_fixed`]) if `fixed`, outermost first, each with its `x` in the
/// column.
fn empty_chain(column: &Fragment, path: &[usize], fixed: bool) -> Vec<(Au, Fragment)> {
    let mut out = Vec::new();
    let mut fragment = column;
    let mut x = Au::ZERO;
    for (depth, &index) in path.iter().enumerate() {
        let Some(child) = fragment.children.get(index) else {
            return Vec::new();
        };
        fragment = child;
        x += fragment.rect.x;
        let FragmentKind::Box { style, node, .. } = &fragment.kind else {
            continue;
        };
        if depth + 1 < path.len() && !(fixed && crate::contained_fixed::contains_fixed(style)) {
            continue;
        }
        out.push((
            x,
            Fragment {
                rect: Rect::new(x, Au::ZERO, fragment.rect.width, Au::ZERO),
                kind: FragmentKind::Box {
                    style: Arc::clone(style),
                    node: *node,
                    sides: VisibleSides {
                        top: false,
                        bottom: false,
                        ..VisibleSides::ALL
                    },
                },
                children: Children::new(),
            },
        ));
    }
    if matches!(fragment.kind, FragmentKind::Box { .. }) {
        out
    } else {
        Vec::new()
    }
}

/// Whether a placeholder of a fixed box in `contained` (one whose containing block is a box
/// that contains fixed boxes) lies below `fragment`.
fn holds_fixed(fragment: &Fragment, contained: &FxHashSet<crate::tree::BoxId>) -> bool {
    fragment.children.iter().any(|child| match child.kind {
        FragmentKind::Placeholder(id) => contained.contains(&id),
        _ => holds_fixed(child, contained),
    })
}

/// Puts `part` (positioned in the column) in the innermost of `empties` (see [`empty_chain`])
/// and each of them in the one around it, their `x` made relative to it: the outermost, or
/// `part` itself where there are none.
fn nest(empties: Vec<(Au, Fragment)>, part: Fragment) -> Fragment {
    let mut current_x = part.rect.x;
    let mut current = part;
    for (x, mut empty) in empties.into_iter().rev() {
        current.rect.x = current_x - x;
        empty.children.push(current);
        current = empty;
        current_x = x;
    }
    current.rect.x = current_x;
    current
}

impl Layouter<'_> {
    /// Runs `layout`, the layout of multi-column container `id`, with it as the multi-column
    /// container being laid out: absolutely positioned boxes whose containing block it is are
    /// placed in its columns (see [`Layouter::position_in_columns`]).
    pub(crate) fn in_multicol<T>(&self, id: crate::tree::BoxId, layout: impl FnOnce() -> T) -> T {
        let outer = self.multicol_container.get();
        self.multicol_container.set(Some((id, outer.is_some())));
        let outcome = layout();
        self.multicol_container.set(outer);
        outcome
    }

    /// Whether the multi-column container being laid out is the containing block of absolutely
    /// positioned boxes, which are then placed in its columns laid end to end where they are
    /// placed by their static position, as in Chrome: in the column where that lies (as far
    /// down the columns as the alignment of a grid or flex container moves it), unbroken. (In
    /// the columns of another container, those columns break them, as in Chrome.)
    pub(crate) fn columns_hold_absolute(&self) -> bool {
        self.multicol_container
            .get()
            .is_some_and(|(id, nested)| !nested && self.tree.get(id).style.contains_absolute())
    }

    /// Whether absolutely positioned box `id` is placed by its static position in all
    /// directions, where a flex or grid container (its parent) aligns it (see
    /// [`Layouter::columns_hold_absolute`]).
    pub(crate) fn aligned_in_columns(&self, id: crate::tree::BoxId) -> bool {
        let style = &self.tree.get(id).style;
        style.position == fagus_style::values::Position::Absolute
            && [
                &style.inset.top,
                &style.inset.right,
                &style.inset.bottom,
                &style.inset.left,
            ]
            .iter()
            .all(|inset| inset.is_auto())
            && (self.flex_statics.borrow().contains_key(&id)
                || self
                    .grid_tracks
                    .borrow()
                    .values()
                    .any(|tracks| tracks.absolute.contains(&id)))
    }

    /// Places the absolutely positioned boxes whose containing blocks lie in `columns`, the
    /// columns of a multi-column container in order, each as tall as `heights` says and `width`
    /// wide (see the module documentation), and adds the columns the boxes reach into after the
    /// last, at the origin. Returns how far down the columns laid end to end the boxes placed
    /// in other columns than their placeholders' reach, unless clipped (zero if none are).
    pub(crate) fn position_in_columns(&self, columns: &mut Vec<Fragment>, heights: &[Au], width: Au) -> Au {
        self.position_in_columns_with(columns, heights, width).0
    }

    /// [`Layouter::position_in_columns`], with how far the part of a box in a column reaches
    /// past the end of that column at most (a box or content in it that cannot break, which
    /// Blink lets overflow the column, and which its column balancer makes room for).
    fn position_in_columns_with(&self, columns: &mut Vec<Fragment>, heights: &[Au], width: Au) -> (Au, Au) {
        let count = columns.len();
        if count == 0
            || heights.len() != count
            || heights.last().is_none_or(|&height| height <= Au::ZERO)
            || !columns.iter().any(has_placeholder)
        {
            return (Au::ZERO, Au::ZERO);
        }
        // The columns are placed at the origin while their boxes are placed.
        let origins: Vec<(Au, Au)> = columns
            .iter_mut()
            .map(|column| {
                let origin = (column.rect.x, column.rect.y);
                column.rect.x = Au::ZERO;
                column.rect.y = Au::ZERO;
                origin
            })
            .collect();
        let outer = self.relative_later.replace(true);
        // Each pass places the boxes found so far; boxes inside them are found in the next, in
        // the fragments of the boxes around them.
        let (mut reach, mut overflow) = (Au::ZERO, Au::ZERO);
        let mut placed = columns.iter().map(absolutes).sum::<usize>();
        for _ in 0..MAX_PASSES {
            let (pass_reach, pass_overflow) = self.columns_pass(columns, heights, width);
            reach = reach.max(pass_reach);
            overflow = overflow.max(pass_overflow);
            let now = columns.iter().map(absolutes).sum::<usize>();
            if now == placed || !columns.iter().any(has_placeholder) {
                break;
            }
            placed = now;
        }
        self.relative_later.set(outer);
        for (column, origin) in columns.iter_mut().zip(origins) {
            column.rect.x = origin.0;
            column.rect.y = origin.1;
        }
        (reach, overflow)
    }

    /// One pass of [`Layouter::position_in_columns`] over `columns` at the origin, the first
    /// `heights.len()` of them as tall as `heights` says, those added after them as the last.
    fn columns_pass(&self, columns: &mut Vec<Fragment>, heights: &[Au], width: Au) -> (Au, Au) {
        let last_height = heights[heights.len() - 1];
        let height_at = |column: usize| heights.get(column).copied().unwrap_or(last_height);
        let heights: Vec<Au> = (0..columns.len()).map(height_at).collect();
        let count = columns.len();
        let mut flow = PageFlow::new(columns.iter(), heights.clone());
        flow.overflow_breaks = true;
        flow.single_round = true;
        // The containing blocks inside boxes that clip their overflow.
        let clipped: FxHashSet<ChainKey> = flow
            .chains
            .iter()
            .filter(|(_, parts)| {
                parts
                    .iter()
                    .any(|part| inside_clip(&columns[part.page], &part.path))
            })
            .map(|(key, _)| *key)
            .collect();
        let area = Rect::new(Au::ZERO, Au::ZERO, width, last_height);
        for (index, column) in columns.iter_mut().enumerate() {
            flow.page = index;
            self.place_absolute(column, area, self.columns_hold_absolute(), Some(&mut flow));
        }
        let flowing = std::mem::take(&mut flow.out);
        // The boxes that go in other columns than their placeholders', broken where they cross
        // the end of a column; columns after the last are as tall as it.
        let limit = count + MAX_ADDED_COLUMNS;
        let end_of_columns = flow.starts[count - 1] + last_height;
        let start_at = |column: usize| {
            flow.starts.get(column).copied().unwrap_or_else(|| {
                end_of_columns + last_height * i32::try_from(column - count).unwrap_or(i32::MAX)
            })
        };
        let column_at = |y: Au| {
            if y < end_of_columns {
                flow.page_at(y)
            } else {
                count + usize::try_from((y - end_of_columns).0 / last_height.0).unwrap_or(0)
            }
        };
        let mut placed = Vec::new();
        let mut reach_all = Au::ZERO;
        let mut overflow = Au::ZERO;
        for item in flowing {
            let (top, reach) = (item.fragment.rect.y, block_extent(&item.fragment));
            let clips = item.chain.is_some_and(|key| clipped.contains(&key));
            // Content of the box that avoids breaks inside needs columns as tall as itself, as
            // Blink's column balancer starts from the tallest such block.
            if !clips {
                overflow = overflow.max(self.tallest_unbreakable(&item.fragment) - last_height);
            }
            if !clips {
                reach_all = reach_all.max(top + reach);
            }
            // A box in clipped overflow stays in the last column, unbroken.
            let last = if clips { count - 1 } else { limit };
            let target = column_at(top.max(Au::ZERO)).min(last);
            let end = start_at(target) + height_at(target);
            let room = |n: usize| {
                if target + n >= last {
                    Au(i32::MAX / 4)
                } else if n == 0 {
                    end - top
                } else {
                    height_at(target + n)
                }
            };
            // A box whose containing block is the multi-column container overflows its column,
            // as in Chrome.
            let own = item.chain.is_none();
            let parts = (top + reach > end && target < last && !own)
                .then(|| self.break_absolute(&item, room))
                .flatten();
            if let Some(parts) = parts {
                for (n, mut part) in parts.into_iter().enumerate().take(last + 1 - target) {
                    if target + n < last && !clips {
                        overflow = overflow.max(block_extent(&part) - room(n));
                    }
                    if n > 0 {
                        // (A part below monolithic overflow starts below it, see `break_absolute`.)
                        part.rect.y = start_at(target + n) + (part.rect.y - item.fragment.rect.y);
                    }
                    placed.push((target + n, part, item.chain));
                }
            } else {
                // A box that cannot break overflows its column.
                if target < last && !clips && !own {
                    overflow = overflow.max(top + reach - end);
                }
                placed.push((target, item.fragment, item.chain));
            }
        }
        let last = placed.iter().map(|(column, ..)| *column).max().unwrap_or(0);
        while columns.len() <= last {
            columns.push(Fragment {
                rect: Rect::new(Au::ZERO, Au::ZERO, width, last_height),
                kind: FragmentKind::Anonymous,
                children: Children::new(),
            });
        }
        // Each part goes in the fragment of its containing block in its column. Where the block
        // has none, the part goes in an empty one at the top of the column, as in Blink, whose
        // transform and clip it then has.
        for (column, mut part, chain) in placed {
            // The last fragment in the column of an inline box broken across lines holds it.
            let inline_last = chain
                .and_then(|key| flow.inline_last(key, column))
                .map(|(path, origin)| (path.to_vec(), origin));
            if let Some((path, origin)) = inline_last {
                part.rect.x -= origin.0;
                part.rect.y -= start_at(column) + origin.1;
                let parent = path.iter().try_fold(&mut columns[column], |fragment, &index| {
                    fragment.children.get_mut(index)
                });
                if let Some(parent) = parent {
                    parent.children.push(part);
                }
                continue;
            }
            let parts = chain.and_then(|key| flow.chains.get(&key));
            if let Some(found) = parts.and_then(|parts| parts.iter().find(|part| part.page == column)) {
                part.rect.x -= found.origin.0;
                part.rect.y -= start_at(column) + found.origin.1;
                let parent = found
                    .path
                    .iter()
                    .try_fold(&mut columns[column], |fragment, &index| {
                        fragment.children.get_mut(index)
                    });
                if let Some(parent) = parent {
                    parent.children.push(part);
                }
                continue;
            }
            part.rect.y -= start_at(column);
            // The block, and where fixed boxes inside the part have their containing blocks
            // around it, the boxes around it that contain fixed boxes (which the next pass
            // places them in), with their places in the column of the block's last fragment.
            let fixed_inside = holds_fixed(&part, &self.tree.contained_fixed);
            let empties = parts
                .and_then(|parts| parts.last())
                .map(|last| empty_chain(&columns[last.page], &last.path, fixed_inside))
                .unwrap_or_default();
            columns[column].children.push(nest(empties, part));
        }
        (reach_all, overflow)
    }

    /// The height of the tallest box in `fragment` (that one included) that avoids breaks
    /// inside it or cannot break (size containment).
    fn tallest_unbreakable(&self, fragment: &Fragment) -> Au {
        if let FragmentKind::Box { style, .. } = &fragment.kind
            && (self.avoids_breaks_inside(style.break_inside)
                || style.size_containment().0 && style.size_containment().1)
        {
            return fragment.rect.height;
        }
        fragment
            .children
            .iter()
            .map(|child| self.tallest_unbreakable(child))
            .fold(Au::ZERO, Au::max)
    }

    /// Whether the absolutely positioned boxes in `columns`, `height` tall, fit in `count`
    /// columns; if not, notes how much taller the columns would have to be for them as a space
    /// shortage, as Blink's column balancer does.
    pub(crate) fn positioned_fit(&self, columns: &[Fragment], height: Au, count: usize, width: Au) -> bool {
        let mut trial = columns.to_vec();
        let heights = vec![height; trial.len()];
        let (reach, overflow) = self.position_in_columns_with(&mut trial, &heights, width);
        // A box (or content in it) that cannot break and reaches past the end of its column
        // needs the columns that much taller.
        if overflow > Au::ZERO {
            self.note_shortage(overflow);
            return false;
        }
        let count = i32::try_from(count.max(1)).unwrap_or(i32::MAX);
        let capacity = height * count;
        if reach <= capacity {
            return true;
        }
        self.note_shortage(Au((reach - capacity).0.saturating_add(count - 1) / count));
        false
    }
}

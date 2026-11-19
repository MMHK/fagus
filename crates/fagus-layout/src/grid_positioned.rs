//! Absolutely positioned boxes whose containing block is a grid container (CSS Grid 2, §10):
//! the box's grid lines make a grid area of the container its containing block, where a line
//! that is `auto`, or that the grid does not have, is the container's padding edge. A child of
//! the container starts at the area's start, and `justify-self` and `align-self` align it in
//! the area along an axis in which both its insets are `auto`, as in Chrome.

use fagus_dom::NodeId;
use fagus_style::values::{GridLine, ItemAlignment, LengthPercentageAuto, Position};

use crate::block::Layouter;
use crate::fragment::Fragment;
use crate::grid::{LineNames, resolve_lines_after};
use crate::tree::{BoxId, BoxKind};
use crate::units::{Au, Rect};

/// The tracks of a grid along one axis: their sizes and offsets from the start of the content
/// box, the number of explicit ones and the names of the lines.
#[derive(Clone, Debug)]
pub(crate) struct AxisTracks {
    pub(crate) sizes: Vec<Au>,
    pub(crate) offsets: Vec<Au>,
    /// The explicit tracks, with the implicit ones before them.
    pub(crate) explicit: usize,
    /// The implicit tracks before the explicit ones.
    pub(crate) lead: usize,
    pub(crate) names: LineNames,
}

/// The tracks of a grid container that is the containing block of absolutely positioned boxes,
/// as its last layout found them.
#[derive(Clone, Debug)]
pub(crate) struct GridTracks {
    pub(crate) container: BoxId,
    pub(crate) columns: AxisTracks,
    pub(crate) rows: AxisTracks,
    /// The width of the content box, whose columns go from the right where `rtl`, and its
    /// height.
    pub(crate) width: Au,
    pub(crate) height: Au,
    pub(crate) rtl: bool,
    /// The left and top padding: where the content box starts in the padding box.
    pub(crate) padding: (Au, Au),
    /// The absolutely positioned children of the container.
    pub(crate) absolute: Vec<BoxId>,
}

/// The lines (indexes from the first, of `lines` lines) that the edges of an absolutely
/// positioned box lie on along an axis, from its `start` and `end` lines and where they
/// resolve to (`resolved`, see [`resolve_lines_after`]): `None` for an edge that is the padding edge,
/// where the line is `auto`, a span against another that is not definite, or a line the grid
/// does not have (CSS Grid 2, §10.1).
pub(crate) fn edge_lines(
    (start, end): (&GridLine, &GridLine),
    resolved: (Option<usize>, usize),
    lines: usize,
) -> (Option<usize>, Option<usize>) {
    let definite =
        |line: &GridLine| !matches!(line, GridLine::Auto | GridLine::Span(_) | GridLine::SpanNamed(..));
    let spans = |line: &GridLine| matches!(line, GridLine::Span(_) | GridLine::SpanNamed(..));
    let Some(first) = resolved.0 else {
        return (None, None);
    };
    let last = first + resolved.1;
    let (from, to) = match (definite(start), definite(end)) {
        (true, true) => (Some(first), Some(last)),
        (true, false) => (Some(first), spans(end).then_some(last)),
        (false, true) => (spans(start).then_some(first), Some(last)),
        (false, false) => (None, None),
    };
    let exists = |line: &usize| *line < lines;
    (from.filter(exists), to.filter(exists))
}

/// Where line `index` lies along `tracks`: the start of the track after it as a start edge
/// (`start`), the end of the track before it as an end edge, as gaps lie outside of areas.
fn line_position(index: usize, tracks: &AxisTracks, start: bool) -> Option<Au> {
    let end_of = |track: usize| Some(*tracks.offsets.get(track)? + *tracks.sizes.get(track)?);
    if start && index < tracks.sizes.len() {
        tracks.offsets.get(index).copied()
    } else if index == 0 {
        tracks.offsets.first().copied()
    } else {
        end_of(index - 1)
    }
}

/// The edges of the area between `lines` (see [`edge_lines`]) of `tracks` from the start of
/// the content box; with `reversed`, as the tracks go from the end of a content box that long.
fn area_edges(
    lines: (Option<usize>, Option<usize>),
    tracks: &AxisTracks,
    reversed: Option<Au>,
) -> (Option<Au>, Option<Au>) {
    let start = lines.0.and_then(|line| line_position(line, tracks, true));
    let end = lines.1.and_then(|line| line_position(line, tracks, false));
    match reversed {
        Some(extent) => (end.map(|end| extent - end), start.map(|start| extent - start)),
        None => (start, end),
    }
}

impl GridTracks {
    /// The grid area of the absolutely positioned box of `style` in the container's padding
    /// box, `size` big.
    fn area(&self, style: &fagus_style::ComputedStyle, size: (Au, Au)) -> Rect {
        let along = |tracks: &AxisTracks, (start, end): (&GridLine, &GridLine)| {
            let resolved = resolve_lines_after(start, end, (tracks.lead, tracks.explicit), &tracks.names);
            edge_lines((start, end), resolved, tracks.sizes.len() + 1)
        };
        let column_lines = along(&self.columns, (&style.grid_column_start, &style.grid_column_end));
        let row_lines = along(&self.rows, (&style.grid_row_start, &style.grid_row_end));
        let x = area_edges(column_lines, &self.columns, self.rtl.then_some(self.width));
        let y = area_edges(row_lines, &self.rows, None);
        let left = x.0.map_or(Au::ZERO, |x| self.padding.0 + x);
        let right = x.1.map_or(size.0, |x| self.padding.0 + x);
        let top = y.0.map_or(Au::ZERO, |y| self.padding.1 + y);
        let bottom = y.1.map_or(size.1, |y| self.padding.1 + y);
        Rect::new(
            left,
            top,
            (right - left).clamp_non_negative(),
            (bottom - top).clamp_non_negative(),
        )
    }
}

impl Layouter<'_> {
    /// Notes the `columns` and `rows` of grid container `id`, whose content box is `size`, where
    /// it is the containing block of absolutely positioned boxes or has such children.
    pub(crate) fn note_grid_tracks(
        &self,
        id: BoxId,
        (columns, rows): (AxisTracks, AxisTracks),
        size: (Au, Au),
    ) {
        let layout_box = self.tree.get(id);
        let Some(node) = layout_box.node else {
            return;
        };
        let absolute = match &layout_box.kind {
            BoxKind::Grid { children } => children
                .iter()
                .copied()
                .filter(|&child| self.tree.get(child).style.position == Position::Absolute)
                .collect(),
            _ => Vec::new(),
        };
        let padding = self
            .grid_tracks
            .borrow()
            .get(&node)
            .map_or((Au::ZERO, Au::ZERO), |tracks| tracks.padding);
        self.grid_tracks.borrow_mut().insert(
            node,
            GridTracks {
                container: id,
                columns,
                rows,
                width: size.0,
                height: size.1,
                rtl: layout_box.style.is_rtl(),
                padding,
                absolute,
            },
        );
    }

    /// Whether the tracks of grid container `id` are worth noting: it is the containing block
    /// of absolutely positioned boxes or has such children, and is laid out for real.
    pub(crate) fn notes_grid_tracks(&self, id: BoxId) -> bool {
        let layout_box = self.tree.get(id);
        let has_absolute = || match &layout_box.kind {
            BoxKind::Grid { children } => children
                .iter()
                .any(|&child| self.tree.get(child).style.position == Position::Absolute),
            _ => false,
        };
        layout_box.node.is_some()
            && !self.measuring.get()
            && (layout_box.style.contains_absolute() || has_absolute())
    }

    /// Notes the left and top `padding` of grid container `id`, laid out last.
    pub(crate) fn note_grid_padding(&self, id: BoxId, padding: (Au, Au)) {
        let Some(node) = self.tree.get(id).node else {
            return;
        };
        if let Some(tracks) = self.grid_tracks.borrow_mut().get_mut(&node)
            && tracks.container == id
        {
            tracks.padding = padding;
        }
    }

    /// Lays out absolutely positioned box `id` in its grid area, where its containing block is
    /// the grid container of element `container`, whose padding box is `size`; the box's
    /// static position is `static_position`, relative to that padding box, as the fragment is
    /// positioned. A child of a grid container that is not its containing block is aligned in
    /// the container's content box instead, where its static position is the start of that box
    /// (CSS Grid 2, §10.2). `None` where neither applies.
    pub(crate) fn absolute_in_grid_area(
        &self,
        id: BoxId,
        container: Option<NodeId>,
        static_position: (Au, Au),
        size: (Au, Au),
    ) -> Option<Fragment> {
        let style = &self.tree.get(id).style;
        if style.position != Position::Absolute {
            return None;
        }
        // The area in the containing block, or the content box of the grid around the box, as
        // the notes have them (no borrow outlives this: layout below notes tracks again).
        let found = {
            let all = self.grid_tracks.borrow();
            if let Some(tracks) = container.and_then(|node| all.get(&node)) {
                Ok((
                    tracks.area(style, size),
                    tracks.absolute.contains(&id),
                    tracks.rtl,
                    tracks.container,
                ))
            } else {
                let parent = all.values().find(|tracks| tracks.absolute.contains(&id))?;
                Err((parent.rtl, parent.container, (parent.width, parent.height)))
            }
        };
        let (rect, child, rtl, grid) = match found {
            Ok(found) => found,
            Err((true, ..)) => return None,
            Err((false, grid, content)) => {
                self.grid_aligned.set(true);
                let mut fragment = self.absolute(id, size.0, size.1, static_position);
                self.grid_aligned.set(false);
                let shift = self.alignment_shift(id, grid, &fragment, content);
                fragment.rect.x += shift.0;
                fragment.rect.y += shift.1;
                self.keep_safe_in(id, grid, &mut fragment, size.0);
                return Some(fragment);
            }
        };
        // A child of the container starts at the area's start (its end where the columns go
        // from the right); another box at its own static position.
        let start = if !child {
            (static_position.0 - rect.x, static_position.1 - rect.y)
        } else if rtl {
            (rect.width, Au::ZERO)
        } else {
            (Au::ZERO, Au::ZERO)
        };
        self.grid_aligned.set(child);
        let mut fragment = self.absolute(id, rect.width, rect.height, start);
        self.grid_aligned.set(false);
        if child {
            let shift = self.alignment_shift(id, grid, &fragment, (rect.width, rect.height));
            fragment.rect.x += if rtl { -shift.0 } else { shift.0 };
            fragment.rect.y += shift.1;
        }
        fragment.rect.x += rect.x;
        fragment.rect.y += rect.y;
        Some(fragment)
    }

    /// Keeps the fragment of absolutely positioned box `id`, a child of grid container `grid`
    /// that is not its containing block (whose width is `cb_width`, which percentages of margins
    /// refer to), from starting before the containing block along an axis in which it has
    /// `safe` center or end alignment, as Chrome does (WPT `grid-abspos-staticpos-align-self-
    /// safe-outer-cb-001`; in a grid area it ignores `safe`).
    fn keep_safe_in(&self, id: BoxId, grid: BoxId, fragment: &mut Fragment, cb_width: Au) {
        let style = &self.tree.get(id).style;
        let container = &self.tree.get(grid).style;
        let margin = |value: &LengthPercentageAuto| match value {
            LengthPercentageAuto::Auto => Au::ZERO,
            LengthPercentageAuto::Length(length) => Au::from_px(length.resolve(cb_width.to_px())),
        };
        let moves = |alignment: ItemAlignment, safe: bool| {
            safe && matches!(alignment, ItemAlignment::Center | ItemAlignment::End)
        };
        let justify_safe = match style.justify_self {
            ItemAlignment::Auto => container.justify_items_block.safe,
            _ => style.justify_self_block.safe,
        } == Some(true);
        if style.inset.left.is_auto()
            && style.inset.right.is_auto()
            && moves(crate::grid::justify(container, style), justify_safe)
        {
            fragment.rect.x = fragment.rect.x.max(margin(&style.margin.left));
        }
        if style.inset.top.is_auto()
            && style.inset.bottom.is_auto()
            && moves(
                crate::block::item_alignment(container, style),
                crate::block::item_flags(container, style).safe,
            )
        {
            fragment.rect.y = fragment.rect.y.max(margin(&style.margin.top));
        }
    }

    /// How far the fragment of absolutely positioned box `id`, a child of grid container
    /// `grid` placed at the start of an area of `size`, moves along each axis in which both its
    /// insets are `auto` to be aligned in the area by its `justify-self` and `align-self` (the
    /// container's `justify-items` and `align-items` for `auto`), as in Chrome; away from the
    /// start, which is the right where the columns go from the right.
    fn alignment_shift(&self, id: BoxId, grid: BoxId, fragment: &Fragment, size: (Au, Au)) -> (Au, Au) {
        let style = &self.tree.get(id).style;
        let container = &self.tree.get(grid).style;
        let margin = |value: &LengthPercentageAuto| match value {
            LengthPercentageAuto::Auto => Au::ZERO,
            LengthPercentageAuto::Length(length) => Au::from_px(length.resolve(size.0.to_px())),
        };
        let shift = |alignment: ItemAlignment, free: Au| match alignment {
            ItemAlignment::Center => free / 2,
            ItemAlignment::End => free,
            _ => Au::ZERO,
        };
        let x = if style.inset.left.is_auto() && style.inset.right.is_auto() {
            let free =
                size.0 - fragment.rect.width - margin(&style.margin.left) - margin(&style.margin.right);
            shift(crate::grid::justify(container, style), free)
        } else {
            Au::ZERO
        };
        let y = if style.inset.top.is_auto() && style.inset.bottom.is_auto() {
            let free =
                size.1 - fragment.rect.height - margin(&style.margin.top) - margin(&style.margin.bottom);
            shift(crate::block::item_alignment(container, style), free)
        } else {
            Au::ZERO
        };
        (x, y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(value: f32) -> Au {
        Au::from_px(value)
    }

    /// Two tracks of 30px at 0 and 40 (a gap of 10 between them).
    fn tracks() -> AxisTracks {
        AxisTracks {
            sizes: vec![px(30.0), px(30.0)],
            offsets: vec![px(0.0), px(40.0)],
            explicit: 2,
            lead: 0,
            names: LineNames(Vec::new()),
        }
    }

    #[test]
    fn auto_lines_and_missing_lines_are_padding_edges() {
        let none = LineNames(Vec::new());
        let lines = |start: GridLine, end: GridLine| {
            let resolved = resolve_lines_after(&start, &end, (0, 2), &none);
            edge_lines((&start, &end), resolved, 3)
        };
        assert_eq!(lines(GridLine::Line(2), GridLine::Auto), (Some(1), None));
        assert_eq!(lines(GridLine::Auto, GridLine::Line(2)), (None, Some(1)));
        assert_eq!(lines(GridLine::Line(1), GridLine::Line(3)), (Some(0), Some(2)));
        assert_eq!(lines(GridLine::Line(2), GridLine::Span(1)), (Some(1), Some(2)));
        // A span past the grid and a line it does not have are padding edges too.
        assert_eq!(lines(GridLine::Line(1), GridLine::Span(5)), (Some(0), None));
        assert_eq!(lines(GridLine::Line(4), GridLine::Line(5)), (None, None));
        assert_eq!(lines(GridLine::Auto, GridLine::Auto), (None, None));
        assert_eq!(lines(GridLine::Span(2), GridLine::Auto), (None, None));
    }

    #[test]
    fn areas_lie_between_track_edges_without_gaps() {
        let tracks = tracks();
        assert_eq!(
            area_edges((Some(1), Some(2)), &tracks, None),
            (Some(px(40.0)), Some(px(70.0)))
        );
        // The last line as a start edge is the end of the last track.
        assert_eq!(area_edges((Some(2), None), &tracks, None), (Some(px(70.0)), None));
        // Columns from the right of a content box 100px wide.
        assert_eq!(
            area_edges((Some(0), Some(1)), &tracks, Some(px(100.0))),
            (Some(px(70.0)), Some(px(100.0)))
        );
    }

    #[test]
    fn area_rects_take_padding_edges_for_auto_lines() {
        let grid = GridTracks {
            container: BoxId(0),
            columns: tracks(),
            rows: tracks(),
            width: px(100.0),
            height: px(100.0),
            rtl: false,
            padding: (px(8.0), px(8.0)),
            absolute: Vec::new(),
        };
        let style = fagus_style::ComputedStyle::from(fagus_style::ComputedValues {
            grid_column_start: GridLine::Line(2),
            grid_row_end: GridLine::Line(2),
            ..fagus_style::ComputedValues::default()
        });
        // The padding box is 116px square; the content box starts 8px into it.
        let rect = grid.area(&style, (px(116.0), px(116.0)));
        assert_eq!(rect, Rect::new(px(48.0), px(0.0), px(68.0), px(38.0)));
    }
}

//! Subgrids (CSS Grid 2, §9): a grid item with `grid-template-columns: subgrid` or
//! `grid-template-rows: subgrid` takes the tracks it spans in its parent grid as its own, with
//! their line names and the parent's gaps (unless it has gaps of its own). Its items take part
//! in sizing the parent's tracks, with the subgrid's margin, border and padding added at its
//! edges.
//!
//! The parent passes the sized tracks down before it lays out the subgrid. The rows are sized
//! from the heights of the items, so a subgrid in rows is laid out twice: first on its own,
//! when it reports what its items need of the rows, then in the parent's rows.

use std::cell::RefCell;
use std::sync::Arc;

use rustc_hash::FxHashMap;

use fagus_style::ComputedStyle;
use fagus_style::values::{GridLine, TrackSize};

use crate::block::{BOTTOM, LEFT, Layouter, RIGHT, TOP};
use crate::grid::{
    Contribution, LineNames, Placed, Room, Span, expand, expand_in, explicit_grid, spans_flexible,
};
use crate::tree::{BoxId, BoxKind};
use crate::units::Au;

/// The tracks a grid passes down to a subgrid along one axis, in the coordinates of the
/// subgrid's content box.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Inherited {
    pub(crate) sizes: Vec<Au>,
    pub(crate) offsets: Vec<Au>,
    /// The parent's gap, which the subgrid's `normal` gap is.
    pub(crate) gap: Au,
    /// The names of the lines, the parent's and the subgrid's own.
    pub(crate) names: LineNames,
}

/// The axes of a grid.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Axis {
    Columns,
    Rows,
}

/// What grids pass to their subgrids and back while they are laid out.
#[derive(Debug, Default)]
pub(crate) struct Subgrids {
    /// The columns and rows passed down to subgrids.
    tracks: FxHashMap<BoxId, [Option<Arc<Inherited>>; 2]>,
    /// What the items of subgrids laid out on their own need of their rows.
    reported: FxHashMap<BoxId, Vec<Contribution>>,
    /// How many rows subgrids span in their parents, and the names of their lines, while they
    /// are laid out on their own.
    spans: FxHashMap<BoxId, (usize, LineNames)>,
}

/// A grid's track count and line names along one axis where it is a subgrid.
pub(crate) type Given = Option<(usize, LineNames)>;

/// What a grid's parent passed down to it as a subgrid: the columns and rows, and the rows it
/// spans while laid out on its own (see [`Layouter::subgrid_input`]).
pub(crate) type SubgridInput = ([Option<Arc<Inherited>>; 2], Given);

impl Layouter<'_> {
    fn subgrids(&self) -> &RefCell<Subgrids> {
        &self.subgrids
    }

    /// Whether box `id` is a grid whose tracks along `axis` are its parent's. A grid that is
    /// absolutely positioned or contains its layout (`contain: layout` or `paint`, or a size
    /// query container) is an independent formatting context: its `subgrid` is `none` (CSS
    /// Grid 2, §9).
    pub(crate) fn is_subgrid(&self, id: BoxId, axis: Axis) -> bool {
        let layout_box = self.tree.get(id);
        let style = &layout_box.style;
        let independent = style.is_out_of_flow()
            || style.contain_layout
            || style.container_type != fagus_style::values::ContainerType::Normal;
        matches!(layout_box.kind, BoxKind::Grid { .. })
            && !independent
            && match axis {
                Axis::Columns => layout_box.style.grid_template_columns.subgrid,
                Axis::Rows => layout_box.style.grid_template_rows.subgrid,
            }
    }

    /// The tracks passed down to subgrid `id`: columns and rows.
    pub(crate) fn inherited(&self, id: BoxId) -> [Option<Arc<Inherited>>; 2] {
        self.subgrids()
            .borrow()
            .tracks
            .get(&id)
            .cloned()
            .unwrap_or_default()
    }

    /// Passes tracks along `axis` down to subgrid `id`, or takes them back.
    pub(crate) fn pass_down(&self, id: BoxId, axis: Axis, tracks: Option<Inherited>) {
        let mut subgrids = self.subgrids().borrow_mut();
        let entry = subgrids.tracks.entry(id).or_default();
        entry[axis as usize] = tracks.map(Arc::new);
    }

    /// Keeps what the items of subgrid `id` need of its rows, in its own row numbers.
    pub(crate) fn report_rows(&self, id: BoxId, contributions: Vec<Contribution>) {
        self.subgrids().borrow_mut().reported.insert(id, contributions);
    }

    /// Keeps how many rows subgrid `id` spans in its parent, with the names of their lines.
    pub(crate) fn note_row_span(&self, id: BoxId, span: (usize, LineNames)) {
        self.subgrids().borrow_mut().spans.insert(id, span);
    }

    /// How many rows subgrid `id` spans in its parent, and the names of their lines, as its
    /// parent noted it.
    pub(crate) fn row_span(&self, id: BoxId) -> Option<(usize, LineNames)> {
        self.subgrids().borrow().spans.get(&id).cloned()
    }

    /// What the items of subgrid `id` need of its rows, as it reported it.
    pub(crate) fn reported_rows(&self, id: BoxId) -> Option<Vec<Contribution>> {
        self.subgrids().borrow_mut().reported.remove(&id)
    }

    /// What the parent of box `id` passed down to it, which its layout depends on where it is a
    /// subgrid (see `crate::memo`).
    pub(crate) fn subgrid_input(&self, id: BoxId) -> SubgridInput {
        let subgrids = self.subgrids().borrow();
        (
            subgrids.tracks.get(&id).cloned().unwrap_or_default(),
            subgrids.spans.get(&id).cloned(),
        )
    }

    /// What subgrid `id` reported of its rows and its parent has not taken yet.
    pub(crate) fn unread_report(&self, id: BoxId) -> Option<Vec<Contribution>> {
        self.subgrids().borrow().reported.get(&id).cloned()
    }

    /// Leaves what subgrid `id` reported of its rows as `report` (none for `None`).
    pub(crate) fn restore_report(&self, id: BoxId, report: Option<Vec<Contribution>>) {
        let mut subgrids = self.subgrids().borrow_mut();
        match report {
            Some(report) => subgrids.reported.insert(id, report),
            None => subgrids.reported.remove(&id),
        };
    }

    /// The explicit grid and placed items of a grid container: its column and row templates
    /// for `width` and `height`, unless it has `given` tracks along an axis as a subgrid, where
    /// items are kept within them. The templates and the counts of explicit tracks start with
    /// the implicit tracks that items need before the explicit grid, whose counts come with
    /// them.
    #[allow(clippy::type_complexity, reason = "the parts of one result")]
    pub(crate) fn grid_items(
        &self,
        style: &ComputedStyle,
        children: &[BoxId],
        (width, height): (Option<Au>, Option<Au>),
        (gaps, given): ((Au, Au), [&Given; 2]),
    ) -> (
        (Vec<TrackSize>, Option<std::ops::Range<usize>>),
        Vec<TrackSize>,
        ((usize, usize), (usize, usize)),
        (LineNames, LineNames),
        Vec<Placed>,
    ) {
        let auto = |count: usize| vec![TrackSize::Auto; count];
        let (column_template, fit) = match given[0] {
            Some((count, _)) => (auto(*count), None),
            // Without an available width, as many as fill the container's minimum width.
            None => expand_in(
                &style.grid_template_columns,
                width
                    .map(Room::Available)
                    .or_else(|| self.repeat_minimum(style).map(Room::Minimum)),
                gaps.0,
            ),
        };
        let row_template = match given[1] {
            Some((count, _)) => auto(*count),
            None => expand(&style.grid_template_rows, height, gaps.1).0,
        };
        let (explicit, mut names) = explicit_grid(style, &column_template, &row_template);
        // Along a subgridded axis, the explicit grid is the tracks the subgrid spans, even where
        // its own `grid-template-areas` name more (CSS Grid 2, §9.1).
        let explicit = (
            given[0].as_ref().map_or(explicit.0, |(count, _)| *count),
            given[1].as_ref().map_or(explicit.1, |(count, _)| *count),
        );
        if let Some((_, inherited)) = given[0] {
            names.0.0.extend(inherited.0.iter().cloned());
        }
        if let Some((_, inherited)) = given[1] {
            names.1.0.extend(inherited.0.iter().cloned());
        }
        let mut order: Vec<BoxId> = children.to_vec();
        order.sort_by_key(|&child| self.tree.get(child).style.order);
        // Implicit tracks before the explicit grid, where items reach before its first line,
        // except along a subgridded axis (CSS Grid 2, §7.5).
        let lead = |given: &Given, start: fn(&ComputedStyle) -> (&GridLine, &GridLine), explicit: usize| {
            if given.is_some() {
                return 0;
            }
            order
                .iter()
                .map(|&child| {
                    let (from, to) = start(&self.tree.get(child).style);
                    crate::grid::leading(from, to, explicit)
                })
                .max()
                .unwrap_or(0)
        };
        let leads = (
            lead(
                given[0],
                |style| (&style.grid_column_start, &style.grid_column_end),
                explicit.0,
            ),
            lead(
                given[1],
                |style| (&style.grid_row_start, &style.grid_row_end),
                explicit.1,
            ),
        );
        let (mut column_template, mut fit, mut row_template) = (column_template, fit, row_template);
        if leads.0 > 0 {
            column_template.splice(0..0, before(&style.grid_auto_columns, leads.0));
            fit = fit.map(|range| range.start + leads.0..range.end + leads.0);
            names.0 = names.0.shifted(leads.0);
        }
        if leads.1 > 0 {
            row_template.splice(0..0, before(&style.grid_auto_rows, leads.1));
            names.1 = names.1.shifted(leads.1);
        }
        let explicit = (explicit.0 + leads.0, explicit.1 + leads.1);
        let mut placed = self.place(style, &order, explicit, (&names.0, &names.1), leads);
        // A subgrid has no implicit tracks along its subgridded axes: the lines of an item past
        // its last line are that line, and an item left without tracks so takes the last one,
        // as in Chrome (CSS Grid 2, §9).
        let clamp = |span: &mut Span, count: usize| {
            if count == 0 {
                return;
            }
            let start = span.0.min(count);
            let end = span.1.min(count);
            *span = if end > start {
                (start, end)
            } else {
                (count - 1, count)
            };
        };
        for item in &mut placed {
            if let Some((count, _)) = given[0] {
                clamp(&mut item.columns, *count);
            }
            if let Some((count, _)) = given[1] {
                clamp(&mut item.rows, *count);
            }
        }
        (
            (column_template, fit),
            row_template,
            (explicit, leads),
            names,
            placed,
        )
    }

    /// What the items need of the columns of a grid with `gap` between them: their min-content
    /// and max-content widths, and for subgrids, those of their items, with the subgrid's
    /// margins, borders and padding at its edges. `basis` resolves percentages.
    pub(crate) fn grid_column_contributions(
        &self,
        placed: &[Placed],
        names: (&LineNames, &LineNames),
        (basis, gap): (Au, Au),
        flexible: &[bool],
    ) -> Vec<Contribution> {
        let mut contributions = Vec::with_capacity(placed.len());
        for item in placed {
            if !self.is_subgrid(item.id, Axis::Columns) {
                let sizes = self.intrinsic(item.id);
                let spans_flexible = spans_flexible(flexible, item.columns);
                contributions.push((
                    item.columns,
                    sizes,
                    self.minimum_contribution(item.id, basis, sizes.0, spans_flexible),
                ));
                continue;
            }
            let style = &self.tree.get(item.id).style;
            let span = item.columns.1 - item.columns.0;
            let columns = Some((span, LineNames(names_in(names.0, item.columns))));
            let rows = self
                .is_subgrid(item.id, Axis::Rows)
                .then(|| (item.rows.1 - item.rows.0, LineNames(names_in(names.1, item.rows))));
            let children = grid_children(self, item.id);
            let (_, _, _, own_names, inner) = self.grid_items(
                style,
                &children,
                (None, None),
                ((Au::ZERO, Au::ZERO), [&columns, &rows]),
            );
            let model = self.box_model(style, basis, None);
            let frame = (
                model.margin[LEFT] + model.border[LEFT] + model.padding[LEFT],
                model.margin[RIGHT] + model.border[RIGHT] + model.padding[RIGHT],
            );
            if inner.is_empty() {
                let edges = frame.0 + frame.1;
                contributions.push((item.columns, (edges, edges), edges));
            }
            let own_gap = style
                .column_gap
                .as_ref()
                .map(|value| Au::from_px(value.resolve(basis.to_px())));
            let inner_contributions = self.grid_column_contributions(
                &inner,
                (&own_names.0, &own_names.1),
                (basis, own_gap.unwrap_or(gap)),
                flexible.get(item.columns.0..item.columns.1).unwrap_or(&[]),
            );
            contributions.extend(edged(
                inner_contributions,
                (item.columns.0, span),
                frame,
                (gap, own_gap),
            ));
        }
        contributions
    }

    /// The frame of subgrid `id` along `axis` at its start and end: margin, border and padding.
    pub(crate) fn subgrid_frame(&self, id: BoxId, axis: Axis, basis: Au) -> (Au, Au) {
        let style = &self.tree.get(id).style;
        let model = self.box_model(style, basis, None);
        match axis {
            Axis::Columns => (
                model.margin[LEFT] + model.border[LEFT] + model.padding[LEFT],
                model.margin[RIGHT] + model.border[RIGHT] + model.padding[RIGHT],
            ),
            Axis::Rows => (
                self.vertical_margin(style, basis, true) + model.border[TOP] + model.padding[TOP],
                self.vertical_margin(style, basis, false) + model.border[BOTTOM] + model.padding[BOTTOM],
            ),
        }
    }
}

/// What the items of a subgrid at `start` of its parent's tracks, `span` of them, need of
/// them: with the subgrid's `frame` at its edges, and half the difference between its own gap
/// and the parent's `gap` at the gaps, as extra margin (CSS Grid 2, §9). Items that the subgrid
/// placed past its tracks (before it knew how many it spans) are kept within them, as a
/// subgrid's grid ends with its span.
pub(crate) fn edged(
    contributions: Vec<Contribution>,
    (offset, span): (usize, usize),
    frame: (Au, Au),
    (gap, own_gap): (Au, Option<Au>),
) -> impl Iterator<Item = Contribution> {
    let half = own_gap.map_or(Au::ZERO, |own| (own - gap) / 2);
    contributions
        .into_iter()
        .filter(move |_| span > 0)
        .map(move |((start, end), sizes, minimum)| {
            let end = end.min(span);
            ((start.min(end - 1), end), sizes, minimum)
        })
        .map(move |((start, end), (min, max), minimum)| {
            let extra = if start == 0 { frame.0 } else { half } + if end == span { frame.1 } else { half };
            (
                (offset + start, offset + end),
                (
                    (min + extra).clamp_non_negative(),
                    (max + extra).clamp_non_negative(),
                ),
                (minimum + extra).clamp_non_negative(),
            )
        })
}

/// The in-flow children of a grid container.
/// The `count` implicit tracks before the explicit grid, sized by `grid-auto-columns` or
/// `grid-auto-rows` (`auto` sizes), whose last size the track next to the explicit grid takes,
/// the one before it the size before that, and so on (CSS Grid 2, §7.6).
fn before(auto: &[TrackSize], count: usize) -> Vec<TrackSize> {
    (0..count)
        .map(|index| {
            if auto.is_empty() {
                return TrackSize::Auto;
            }
            // From the explicit grid backwards: the track `count - index` before it.
            let back = (count - index - 1) % auto.len();
            auto[auto.len() - 1 - back].clone()
        })
        .collect()
}

fn grid_children(layouter: &Layouter<'_>, id: BoxId) -> Vec<BoxId> {
    match &layouter.tree.get(id).kind {
        BoxKind::Grid { children } => children
            .iter()
            .copied()
            .filter(|&child| !layouter.tree.get(child).style.is_out_of_flow())
            .collect(),
        _ => Vec::new(),
    }
}

/// The names of the lines of `span`, numbered from its start.
pub(crate) fn names_in(names: &LineNames, (start, end): Span) -> Vec<(Arc<str>, usize)> {
    names
        .0
        .iter()
        .filter(|(_, line)| (start..=end).contains(line))
        .map(|(name, line)| (Arc::clone(name), line - start))
        .collect()
}

/// The tracks of `span` of a grid, with `sizes` and `offsets` and `gap` between them, as a
/// subgrid that spans them takes them: from the start of its content box, inside its `frame`
/// (margin, border and padding at its start and end), with half the difference of its own gap
/// on either side of the gaps.
pub(crate) fn tracks_for(
    (sizes, offsets): (&[Au], &[Au]),
    (start, end): Span,
    frame: (Au, Au),
    (gap, own_gap): (Au, Option<Au>),
    names: LineNames,
) -> Inherited {
    let origin = offsets[start] + frame.0;
    let content =
        (offsets[end - 1] + sizes[end - 1] - offsets[start] - frame.0 - frame.1).clamp_non_negative();
    let half = own_gap.map_or(Au::ZERO, |own| (own - gap) / 2);
    let mut inherited_sizes = Vec::with_capacity(end - start);
    let mut inherited_offsets = Vec::with_capacity(end - start);
    for index in start..end {
        let mut from = offsets[index] - origin;
        let mut to = from + sizes[index];
        if index > start {
            from += half;
        }
        if index + 1 < end {
            to -= half;
        }
        let from = from.max(Au::ZERO).min(content);
        let to = to.min(content).max(from);
        inherited_offsets.push(from);
        inherited_sizes.push(to - from);
    }
    Inherited {
        sizes: inherited_sizes,
        offsets: inherited_offsets,
        gap,
        names,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(value: f32) -> Au {
        Au::from_px(value)
    }

    #[test]
    fn passes_tracks_down_inside_the_frame() {
        let sizes = [px(100.0), px(50.0), px(80.0)];
        let offsets = [px(0.0), px(110.0), px(170.0)];
        let inherited = tracks_for(
            (&sizes, &offsets),
            (0, 3),
            (px(5.0), px(5.0)),
            (px(10.0), None),
            LineNames::default(),
        );
        assert_eq!(inherited.offsets, [px(0.0), px(105.0), px(165.0)]);
        assert_eq!(inherited.sizes, [px(95.0), px(50.0), px(75.0)]);
        // An own gap of 20px takes 5px off each side of the gaps.
        let inherited = tracks_for(
            (&sizes, &offsets),
            (1, 3),
            (Au::ZERO, Au::ZERO),
            (px(10.0), Some(px(20.0))),
            LineNames::default(),
        );
        assert_eq!(inherited.offsets, [px(0.0), px(65.0)]);
        assert_eq!(inherited.sizes, [px(45.0), px(75.0)]);
    }

    #[test]
    fn renumbers_line_names_of_a_span() {
        let names = LineNames(vec![
            (Arc::from("a"), 0),
            (Arc::from("b"), 2),
            (Arc::from("c"), 4),
        ]);
        let inside = names_in(&names, (2, 4));
        assert_eq!(inside, [(Arc::from("b"), 0), (Arc::from("c"), 2)]);
    }
}

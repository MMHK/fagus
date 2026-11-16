//! The sizes of the columns and rows of an `<mtable>`, which Chrome lays out as a CSS table
//! (Blink's `table_layout_utils.cc`, BSD licensed: see `CHROMIUM-LICENSE`): cells in the slots of
//! a grid, column widths from the widths of the cells with the excess of cells that span columns
//! shared out, row heights from the cells aligned on their baselines with the excess of cells
//! that span rows shared out. Math has no percentages, fixed sizes or border spacing, and its
//! content does not wrap: a cell's narrowest and widest sizes are the same.

/// Where a cell lies in the grid of a table: its first row and column and how many it spans.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Slot {
    pub(crate) row: usize,
    pub(crate) column: usize,
    pub(crate) rows: usize,
    pub(crate) columns: usize,
}

/// The slots of the cells of a table's rows, given by how many columns and rows each spans (0
/// rows spans to the last row, as in HTML): each cell takes the first column of its row that no
/// cell above spans into, as HTML's table model places them. The cells come in the order given.
pub(crate) fn slots(rows: &[Vec<(usize, usize)>]) -> Vec<Slot> {
    let count = rows.len();
    // Which columns of each row cells take.
    let mut taken: Vec<Vec<bool>> = vec![Vec::new(); count];
    let mut slots = Vec::new();
    for (row, cells) in rows.iter().enumerate() {
        let mut column = 0;
        for &(columns, spanned) in cells {
            while taken[row].get(column).copied().unwrap_or(false) {
                column += 1;
            }
            let columns = columns.max(1);
            let spanned = if spanned == 0 {
                count - row
            } else {
                spanned.min(count - row)
            };
            for taken in &mut taken[row..row + spanned] {
                if taken.len() < column + columns {
                    taken.resize(column + columns, false);
                }
                taken[column..column + columns].fill(true);
            }
            slots.push(Slot {
                row,
                column,
                rows: spanned,
                columns,
            });
            column += columns;
        }
    }
    slots
}

/// A length in layout units (1/64 px), as Blink's `LayoutUnit` keeps it.
fn units(px: f32) -> i64 {
    // Lengths of a formula are far from the range of i64.
    #[allow(clippy::cast_possible_truncation, reason = "lengths in layout units")]
    let units = (px * 64.0).round() as i64;
    units
}

/// Pixels of a length in layout units.
#[allow(clippy::cast_precision_loss, reason = "lengths in layout units")]
fn pixels(units: i64) -> f32 {
    units as f32 / 64.0
}

/// `a * b / c` in layout units, as `LayoutUnit::MulDiv` computes it.
fn mul_div(a: i64, b: i64, c: i64) -> i64 {
    if c == 0 { 0 } else { a * b / c }
}

/// The widths of the columns of a table from its cells' slots and widths (Blink's
/// `ApplyCellConstraintsToColumnConstraints` for a table as wide as its content): each column
/// as wide as its widest cell that spans no other; a cell that spans columns and is wider than
/// they are together widens them in proportion to their widths (equally where they have
/// none), the narrowest spans first. Columns no cell of their own starts in take part only
/// where a spanning cell has no other.
pub(crate) fn column_widths(cells: &[(Slot, f32)]) -> Vec<f32> {
    let count = cells
        .iter()
        .map(|(slot, _)| slot.column + slot.columns)
        .max()
        .unwrap_or(0);
    let mut widths = vec![0_i64; count];
    // Mergeable columns (Blink's `is_mergeable`): no cell that spans only them starts there.
    let mut mergeable = vec![true; count];
    for (slot, width) in cells {
        if slot.columns == 1 {
            widths[slot.column] = widths[slot.column].max(units(*width));
            mergeable[slot.column] = false;
        }
    }
    let mut spanning: Vec<&(Slot, f32)> = cells.iter().filter(|(slot, _)| slot.columns > 1).collect();
    for (slot, _) in &spanning {
        let range = slot.column..slot.column + slot.columns;
        if mergeable[range.clone()].iter().all(|merge| *merge) {
            mergeable[slot.column] = false;
        }
    }
    spanning.sort_by_key(|(slot, _)| (slot.columns, slot.column));
    for (slot, width) in spanning {
        let range = slot.column..slot.column + slot.columns;
        let columns: Vec<usize> = range.filter(|&column| !mergeable[column]).collect();
        let total: i64 = columns.iter().map(|&column| widths[column]).sum();
        let excess = units(*width) - total;
        let Some(&last) = columns.last() else {
            continue;
        };
        if excess <= 0 {
            continue;
        }
        let mut left = excess;
        let count = i64::try_from(columns.len()).unwrap_or(1);
        for &column in &columns {
            let delta = if total > 0 {
                mul_div(excess, widths[column], total)
            } else {
                excess / count
            };
            widths[column] += delta;
            left -= delta;
        }
        widths[last] += left;
    }
    widths.into_iter().map(pixels).collect()
}

/// A cell as the rows of a table take it: its slot, its height, and its baseline from its top
/// (`None` for a cell without content, whose baseline is the bottom of its content box, which
/// lies `bottom` above its bottom edge).
#[derive(Clone, Copy, Debug)]
pub(crate) struct CellBlock {
    pub(crate) slot: Slot,
    pub(crate) height: f32,
    pub(crate) baseline: Option<f32>,
    pub(crate) bottom: f32,
}

/// The heights and baselines (from their tops) of the rows of a table (Blink's
/// `ComputeMinimumRowBlockSize` and `DistributeRowspanCellToRows`).
pub(crate) fn rows(count: usize, cells: &[CellBlock]) -> Vec<(f32, f32)> {
    let mut heights = vec![0_i64; count];
    let mut baselines = vec![0_i64; count];
    let mut spans_from = vec![false; count];
    for (row, (height, baseline)) in heights.iter_mut().zip(&mut baselines).enumerate() {
        let mut tallest = 0_i64;
        let (mut ascent, mut descent): (Option<i64>, Option<i64>) = (None, None);
        let mut fallback: Option<i64> = None;
        for cell in cells.iter().filter(|cell| cell.slot.row == row) {
            let cell_height = units(cell.height);
            let spans = cell.slot.rows > 1;
            if let Some(cell_baseline) = cell.baseline.map(units) {
                ascent = Some(ascent.map_or(cell_baseline, |ascent| ascent.max(cell_baseline)));
                descent = Some(if spans {
                    descent.unwrap_or(0)
                } else {
                    descent.map_or(cell_height - cell_baseline, |descent| {
                        descent.max(cell_height - cell_baseline)
                    })
                });
            }
            if ascent.is_none() {
                let bottom = units(cell.bottom);
                fallback = Some(fallback.map_or(bottom, |fallback| fallback.min(bottom)));
            }
            if spans {
                spans_from[row] = true;
            } else {
                tallest = tallest.max(cell_height);
            }
        }
        *height = match (ascent, descent) {
            (Some(ascent), Some(descent)) => tallest.max(ascent + descent),
            _ => tallest,
        };
        *baseline = match (ascent, fallback) {
            (Some(ascent), _) => ascent,
            (None, Some(bottom)) => (*height - bottom).max(0),
            (None, None) => 0,
        };
    }
    // Cells that span rows share out what they need more than the rows give them.
    let mut spanning: Vec<&CellBlock> = cells.iter().filter(|cell| cell.slot.rows > 1).collect();
    spanning.sort_by_key(|cell| (cell.slot.row, cell.slot.rows));
    for cell in spanning {
        let range = cell.slot.row..cell.slot.row + cell.slot.rows;
        let total: i64 = heights[range.clone()].iter().sum();
        let excess = units(cell.height) - total;
        if excess <= 0 {
            continue;
        }
        // Rows where other spanning cells start take it equally; else the rows with content in
        // proportion to their heights; else the last row.
        let starting: Vec<usize> = range.clone().skip(1).filter(|&row| spans_from[row]).collect();
        let filled: Vec<usize> = range.clone().filter(|&row| heights[row] > 0).collect();
        if let Some(&last) = starting.last() {
            let share = excess / i64::try_from(starting.len()).unwrap_or(1);
            for &row in &starting {
                heights[row] += share;
            }
            heights[last] += excess - share * i64::try_from(starting.len()).unwrap_or(1);
        } else if let Some(&last) = filled.last() {
            let filled_total: i64 = filled.iter().map(|&row| heights[row]).sum();
            let mut left = excess;
            for &row in &filled {
                let delta = mul_div(excess, heights[row], filled_total);
                heights[row] += delta;
                left -= delta;
            }
            heights[last] += left;
        } else if let Some(last) = range.last() {
            heights[last] += excess;
        }
    }
    heights
        .into_iter()
        .zip(baselines)
        .map(|(height, baseline)| (pixels(height), pixels(baseline)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(row: usize, column: usize, rows: usize, columns: usize) -> Slot {
        Slot {
            row,
            column,
            rows,
            columns,
        }
    }

    #[test]
    fn places_cells_past_those_that_span_rows() {
        // A cell over two rows, a cell over two columns, and a row span of 0 to the end.
        let rows = vec![vec![(1, 3), (2, 1)], vec![(1, 1), (1, 0)], vec![(1, 1)]];
        assert_eq!(
            slots(&rows),
            [
                slot(0, 0, 3, 1),
                slot(0, 1, 1, 2),
                slot(1, 1, 1, 1),
                slot(1, 2, 2, 1),
                slot(2, 1, 1, 1),
            ]
        );
    }

    #[test]
    fn widens_spanned_columns_in_proportion_to_their_widths() {
        let cells = [
            (slot(0, 0, 1, 2), 100.0),
            (slot(1, 0, 1, 1), 10.0),
            (slot(1, 1, 1, 1), 30.0),
        ];
        assert_eq!(column_widths(&cells), [25.0, 75.0]);
        // Columns without cells of their own share equally, the last the rest.
        let cells = [
            (slot(0, 0, 1, 3), 10.0),
            (slot(1, 0, 1, 1), 0.0),
            (slot(1, 1, 1, 1), 0.0),
        ];
        assert_eq!(column_widths(&cells), [5.0, 5.0, 0.0]);
    }

    #[test]
    fn aligns_rows_on_baselines_and_shares_out_spanning_cells() {
        let cell = |row, rows, height, baseline| CellBlock {
            slot: slot(row, 0, rows, 1),
            height,
            baseline,
            bottom: 0.0,
        };
        // Two cells, one deeper, aligned on their baselines.
        let cells = [
            cell(0, 1, 20.0, Some(15.0)),
            cell(0, 1, 10.0, Some(5.0)),
            cell(1, 1, 10.0, Some(8.0)),
        ];
        assert_eq!(rows(2, &cells), [(20.0, 15.0), (10.0, 8.0)]);
        // A cell over both rows, 60 tall, grows them in proportion.
        let cells = [
            cell(0, 2, 60.0, Some(10.0)),
            cell(0, 1, 20.0, Some(15.0)),
            cell(1, 1, 10.0, Some(8.0)),
        ];
        assert_eq!(rows(2, &cells), [(40.0, 15.0), (20.0, 8.0)]);
    }
}

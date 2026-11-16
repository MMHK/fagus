//! How cells that span several columns of an automatic table layout widen those columns, as
//! Chrome does it: Blink's `DistributeColspanCellToColumnsAuto` and
//! `DistributeInlineSizeToComputedInlineSizeAuto` (`table_layout_utils.cc`), ported under
//! their BSD license (`CHROMIUM-LICENSE`).
//!
//! A spanning cell's minimum and maximum widths are shared among its columns by the guesses of
//! the width distribution algorithm (CSS Tables 3, §3.9.3): every column at its minimum, then
//! percentage columns at their share, then columns of a fixed width at it, then auto columns at
//! their maximum, then more for the auto columns.

use crate::table::shares;
use crate::units::Au;

/// What a column of an automatic table layout asks for, as far as spanning cells have widened it
/// yet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SpanColumn {
    pub(crate) min: Au,
    pub(crate) max: Au,
    /// Its percentage width, as a fraction.
    pub(crate) percent: Option<f32>,
    /// Whether a cell or column gives it a fixed width.
    pub(crate) constrained: bool,
}

impl SpanColumn {
    /// The width its percentage asks for of `target`, never less than its minimum.
    fn percent_width(&self, target: Au) -> Au {
        self.min
            .max(Au::from_px(target.to_px() * self.percent.unwrap_or(0.0)))
    }
}

/// A cell spanning `columns`, with its minimum and maximum widths, its percentage width as a
/// fraction, whether it has a fixed width, and the border spacing between its columns: widens
/// its columns where they do not give it as much.
pub(crate) fn widen(
    columns: &mut [SpanColumn],
    (min, max): (Au, Au),
    percent: Option<f32>,
    constrained: bool,
    spacing: Au,
) {
    if columns.is_empty() {
        return;
    }
    let inner_spacing = spacing * (columns.len() as i32 - 1);
    let min = (min - inner_spacing).clamp_non_negative();
    let max = (max - inner_spacing).clamp_non_negative();
    // What the cell's percentage asks for beyond its columns' goes to those without one, in
    // proportion to their maximum widths (or the same to each).
    if let Some(percent) = percent {
        let columns_percent: f32 = columns.iter().filter_map(|column| column.percent).sum();
        let others = columns.iter().filter(|column| column.percent.is_none()).count();
        let surplus = percent - columns_percent;
        if surplus > 0.0 && others > 0 {
            let others_max: Au = columns
                .iter()
                .filter(|column| column.percent.is_none())
                .map(|column| column.max)
                .sum();
            for column in columns.iter_mut().filter(|column| column.percent.is_none()) {
                column.percent = Some(if others_max > Au::ZERO {
                    surplus * column.max.to_px() / others_max.to_px()
                } else {
                    surplus / others as f32
                });
            }
        }
    }
    let widths = distribute(min, columns, true);
    for (column, width) in columns.iter_mut().zip(widths) {
        column.min = column.min.max(width);
    }
    let widths = distribute(max, columns, constrained);
    for (column, width) in columns.iter_mut().zip(widths) {
        column.max = column.min.max(column.max).max(width);
    }
}

/// Shares `target` among `columns` by the guesses of the width distribution algorithm (Blink's
/// `DistributeInlineSizeToComputedInlineSizeAuto`). Beyond all columns' maximum widths, the auto
/// columns grow, or else the fixed ones where the width shared is `constrained`, or else the
/// percentage columns.
pub(crate) fn distribute(target: Au, columns: &[SpanColumn], constrained: bool) -> Vec<Au> {
    let percent = |column: &SpanColumn| column.percent.is_some();
    let fixed = |column: &SpanColumn| column.percent.is_none() && column.constrained;
    let auto = |column: &SpanColumn| column.percent.is_none() && !column.constrained;
    let min_guess: Au = columns.iter().map(|column| column.min).sum();
    let target = target.max(min_guess);
    let percentage_guess: Au = columns
        .iter()
        .map(|column| {
            if percent(column) {
                column.percent_width(target)
            } else {
                column.min
            }
        })
        .sum();
    let specified_guess: Au = columns
        .iter()
        .map(|column| {
            if percent(column) {
                column.percent_width(target)
            } else if fixed(column) {
                column.max
            } else {
                column.min
            }
        })
        .sum();
    let max_guess: Au = columns
        .iter()
        .map(|column| {
            if percent(column) {
                column.percent_width(target)
            } else {
                column.max
            }
        })
        .sum();
    // Shares `amount` among the columns `grow` picks, in proportion to `weight` (or the same to
    // each where those are all zero), each added to `base`; the others get `rest`.
    let grow = |amount: Au,
                picks: &dyn Fn(&SpanColumn) -> bool,
                weight: &dyn Fn(&SpanColumn) -> Au,
                base: &dyn Fn(&SpanColumn) -> Au,
                rest: &dyn Fn(&SpanColumn) -> Au| {
        let picked: Vec<usize> = (0..columns.len())
            .filter(|&index| picks(&columns[index]))
            .collect();
        let weights: Vec<Au> = picked.iter().map(|&index| weight(&columns[index])).collect();
        let total: Au = weights.iter().copied().sum();
        let weights = if total > Au::ZERO {
            weights
        } else {
            vec![Au::EPSILON; picked.len()]
        };
        let total = weights.iter().copied().sum();
        let mut sizes: Vec<Au> = columns.iter().map(rest).collect();
        for (&index, share) in picked.iter().zip(shares(amount, &weights, total)) {
            sizes[index] = base(&columns[index]) + share;
        }
        sizes
    };
    if min_guess >= target {
        return columns.iter().map(|column| column.min).collect();
    }
    if percentage_guess >= target {
        return grow(
            target - min_guess,
            &percent,
            &|column| column.percent_width(target) - column.min,
            &|column| column.min,
            &|column| column.min,
        );
    }
    // A percentage column's share of `target`, or `other` for the other columns.
    let percent_or = |column: &SpanColumn, other: Au| {
        if percent(column) {
            column.percent_width(target)
        } else {
            other
        }
    };
    if specified_guess >= target {
        return grow(
            target - percentage_guess,
            &fixed,
            &|column| column.max - column.min,
            &|column| column.min,
            &|column| percent_or(column, column.min),
        );
    }
    if max_guess >= target {
        // Where the widths match exactly (an auto table as wide as its content), the columns
        // take their maximum widths, without rounding.
        if max_guess == target {
            return columns
                .iter()
                .map(|column| percent_or(column, column.max))
                .collect();
        }
        return grow(
            target - specified_guess,
            &auto,
            &|column| column.max - column.min,
            &|column| column.min,
            &|column| percent_or(column, column.max),
        );
    }
    let above = target - max_guess;
    if columns.iter().any(auto) {
        grow(
            above,
            &auto,
            &|column| column.max,
            &|column| column.max,
            &|column| percent_or(column, column.max),
        )
    } else if constrained && columns.iter().any(fixed) {
        grow(
            above,
            &fixed,
            &|column| column.max,
            &|column| column.max,
            &|column| percent_or(column, column.max),
        )
    } else if columns.iter().any(percent) {
        let weight = |column: &SpanColumn| Au::from_px(column.percent.unwrap_or(0.0) * 1000.0);
        grow(
            above,
            &percent,
            &weight,
            &|column| column.percent_width(target),
            &|column| column.max,
        )
    } else {
        columns.iter().map(|column| column.max).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(value: f32) -> Au {
        Au::from_px(value)
    }

    fn column(min: f32, max: f32, constrained: bool) -> SpanColumn {
        SpanColumn {
            min: px(min),
            max: px(max),
            percent: None,
            constrained,
        }
    }

    #[test]
    fn widens_auto_columns_before_fixed_ones() {
        // A cell 100px wide over a fixed 5px column and an empty auto one: the auto column
        // takes the rest (`colspan-004`).
        let mut columns = [column(0.0, 5.0, true), column(0.0, 0.0, false)];
        widen(&mut columns, (Au::ZERO, px(100.0)), None, true, Au::ZERO);
        assert_eq!(columns[0].max, px(5.0));
        assert_eq!(columns[1].max, px(95.0));
        // Its minimum goes to both in proportion to what they ask beyond theirs.
        let mut columns = [column(10.0, 30.0, false), column(10.0, 10.0, false)];
        widen(&mut columns, (px(40.0), px(40.0)), None, false, Au::ZERO);
        assert_eq!((columns[0].min, columns[1].min), (px(30.0), px(10.0)));
    }

    #[test]
    fn leaves_out_the_spacing_between_the_columns() {
        let mut columns = [column(0.0, 0.0, false), column(0.0, 0.0, false)];
        widen(&mut columns, (px(50.0), px(50.0)), None, false, px(10.0));
        assert_eq!((columns[0].min, columns[1].min), (px(20.0), px(20.0)));
    }

    #[test]
    fn gives_a_spanning_percentage_to_columns_without_one() {
        let mut columns = [column(0.0, 30.0, false), column(0.0, 10.0, false)];
        widen(&mut columns, (Au::ZERO, px(40.0)), Some(0.4), false, Au::ZERO);
        let percents: Vec<f32> = columns
            .iter()
            .map(|column| column.percent.unwrap_or(0.0))
            .collect();
        assert!((percents[0] - 0.3).abs() < 1e-6 && (percents[1] - 0.1).abs() < 1e-6);
    }
}

//! `flex-wrap: balance` (CSS Flexible Box Layout 2): the items of a flex container go into
//! lines whose free space is as even as possible, and into at least `flex-line-count` lines,
//! as Blink breaks them (`BalanceBreakFlexItemsIntoLines` in Chromium's
//! `flex_line_breaker.cc`, BSD-licensed; see `CHROMIUM-LICENSE`).
//!
//! Each line costs the square of the room it leaves free (none if a single item overflows it),
//! and the lines are the ones of the lowest total cost (Knuth and Plass for flex items), found
//! here from the first item to the last. Where greedy filling makes fewer lines than the
//! minimum count, the break length is bisected down to the shortest that still makes as few,
//! and items that end a line exactly are then made a hair longer until the count is reached.

use std::ops::Range;

use crate::units::Au;

/// The lines (ranges of the items) of items whose outer hypothetical main sizes are `sizes`,
/// balanced in lines `limit` long with `gap` between items, at least `min_lines` of them where
/// there are as many items.
pub(crate) fn balanced_lines(sizes: &[Au], limit: Au, gap: Au, min_lines: usize) -> Vec<Range<usize>> {
    if sizes.is_empty() {
        return Vec::new();
    }
    let min_lines = min_lines.clamp(1, sizes.len());
    let raw = |size: Au| u64::try_from(size.0).unwrap_or(0);
    let gap = raw(gap);
    // Prefix sums of the items with a gap after each: strictly increasing, negative sizes
    // counting as nothing.
    let mut sums: Vec<u64> = Vec::with_capacity(sizes.len());
    let mut total = 0u64;
    for &size in sizes {
        total = total.saturating_add(raw(size)).saturating_add(gap);
        sums.push(total);
    }
    let mut context = Context {
        gap,
        limit: raw(limit),
        sums,
    };
    if context.greedy_count(context.limit) < min_lines {
        context.apply_min_count(min_lines);
    }
    let counts = context.best_counts();
    let mut lines = Vec::with_capacity(counts.len());
    let mut start = 0;
    for count in counts {
        lines.push(start..start + count);
        start += count;
    }
    lines
}

/// What the balancing works on: the gap between items, the length lines break at and the
/// prefix sums of the items (each with a gap after it), in raw units.
struct Context {
    gap: u64,
    limit: u64,
    sums: Vec<u64>,
}

impl Context {
    /// The length of the items `start..=end` on a line.
    fn length(&self, start: usize, end: usize) -> u64 {
        let before = if start == 0 { 0 } else { self.sums[start - 1] };
        self.sums[end].saturating_sub(before).saturating_sub(self.gap)
    }

    /// How many lines filling them greedily up to `limit` makes.
    fn greedy_count(&self, limit: u64) -> usize {
        let mut index = 0;
        let mut previous = 0u64;
        let mut count = 1;
        loop {
            // The first item past the break.
            let bound = previous.saturating_add(limit).saturating_add(self.gap);
            let next = index + self.sums[index..].partition_point(|&sum| sum <= bound);
            if next == self.sums.len() {
                break;
            }
            // A single item longer than the limit stays on its line; otherwise the line ends
            // before the item past the break.
            let single = usize::from(count == 1) + (next - index) <= 1;
            index = if single { next } else { next - 1 };
            previous = self.sums[index];
            if index + 1 != self.sums.len() {
                count += 1;
            }
        }
        count
    }

    /// Lowers the break length, or else lengthens items that end a line exactly, until greedy
    /// filling makes `min_lines` lines; returns the count it makes.
    fn apply_min_count(&mut self, min_lines: usize) -> usize {
        let mut low = 0u64;
        let mut high = self
            .limit
            .min(self.sums.last().copied().unwrap_or(0).saturating_sub(self.gap));
        while low < high {
            let middle = low + (high - low) / 2;
            if self.greedy_count(middle) > min_lines {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        self.limit = high;
        let mut count = self.greedy_count(self.limit);
        if count >= min_lines {
            return count;
        }
        // Items that fit their line exactly, which a hair more moves to the next line.
        let mut exact = Vec::new();
        let mut previous = 0u64;
        let mut has_content = false;
        for index in 0..self.sums.len() {
            let size = self.sums[index].saturating_sub(previous).saturating_sub(self.gap);
            if size == self.limit {
                exact.push(index);
            }
            if size > self.limit {
                previous = if has_content && index > 0 {
                    self.sums[index - 1]
                } else {
                    self.sums[index]
                };
                has_content = false;
                continue;
            }
            has_content = true;
        }
        for &index in exact.iter().rev() {
            for sum in &mut self.sums[index..] {
                *sum = sum.saturating_add(1);
            }
            count = self.greedy_count(self.limit);
            if count >= min_lines {
                break;
            }
        }
        count
    }

    /// The item counts of the lines of the lowest total cost, first line first.
    fn best_counts(&self) -> Vec<usize> {
        let count = self.sums.len();
        // The earliest item a line ending with each item may start with.
        let mut earliest = vec![0; count];
        let mut start = 0;
        for (end, earliest) in earliest.iter_mut().enumerate() {
            while start < end && self.length(start, end) > self.limit {
                start += 1;
            }
            *earliest = start;
            if start == end && self.length(start, end) > self.limit {
                start += 1;
            }
        }
        // The lowest cost of the lines up to each item (with a break after it), and where the
        // line before the last of them ends.
        let mut best = vec![u64::MAX; count];
        let mut breaks: Vec<Option<usize>> = vec![None; count];
        for end in 0..count {
            let mut best_score = u64::MAX;
            let mut best_break = None;
            for start in earliest[end]..=end {
                let length = self.length(start, end);
                let line = if length > self.limit {
                    0
                } else {
                    (self.limit - length).saturating_mul(self.limit - length)
                };
                // Shorter lines only cost more.
                if line > best_score {
                    break;
                }
                let score = line.saturating_add(if start == 0 { 0 } else { best[start - 1] });
                if score <= best_score {
                    best_score = score;
                    best_break = start.checked_sub(1);
                }
            }
            best[end] = best_score;
            breaks[end] = best_break;
        }
        let mut counts = Vec::new();
        let mut last = count - 1;
        let mut index = breaks[last];
        while let Some(at) = index {
            counts.push(last - at);
            last = at;
            index = breaks[at];
        }
        counts.push(last + 1);
        counts.reverse();
        counts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(values: &[f32]) -> Vec<Au> {
        values.iter().map(|&value| Au::from_px(value)).collect()
    }

    #[test]
    fn four_items_balance_two_and_two() {
        // Greedy filling would make lines of three and one.
        let lines = balanced_lines(&px(&[25.0; 4]), Au::from_px(100.0), Au::from_px(10.0), 1);
        assert_eq!(lines, vec![0..2, 2..4]);
    }

    #[test]
    fn minimum_line_count_splits_a_line_that_fits() {
        let lines = balanced_lines(&px(&[40.0; 3]), Au::MAX, Au::from_px(20.0), 2);
        assert_eq!(lines.len(), 2);
        let lines = balanced_lines(&px(&[25.0; 4]), Au::MAX, Au::ZERO, 3);
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn an_item_longer_than_the_line_gets_a_line_of_its_own() {
        let lines = balanced_lines(&px(&[10.0, 200.0, 10.0]), Au::from_px(100.0), Au::ZERO, 1);
        assert_eq!(lines, vec![0..1, 1..2, 2..3]);
    }

    #[test]
    fn a_line_count_above_the_items_is_clamped() {
        let lines = balanced_lines(&px(&[10.0, 10.0]), Au::from_px(100.0), Au::ZERO, 10_000);
        assert_eq!(lines, vec![0..1, 1..2]);
    }
}

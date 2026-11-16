//! The main sizes of the items of a flex row: their flex base sizes, their minimum and maximum
//! (with the automatic minimum size) and the resolution of flexible lengths (CSS Flexbox 1,
//! §9.2 step 3, §4.5 and §9.7).

use fagus_style::values::{BoxSizing, LengthPercentage, MaxSize, Size};

use crate::block::{BOTTOM, Layouter, TOP};
use crate::tree::{BoxId, BoxKind};
use crate::units::Au;

/// The sizes of one flex item in the main axis, all of its margin box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FlexLength {
    /// The flex base size.
    pub base: Au,
    /// The item's border, padding and margins.
    pub frame: Au,
    pub min: Au,
    pub max: Au,
    pub grow: f32,
    pub shrink: f32,
}

impl FlexLength {
    /// The hypothetical main size: the base size within the minimum and maximum.
    pub(crate) fn hypothetical(&self) -> Au {
        self.base.min(self.max).max(self.min)
    }

    fn clamp(&self, size: Au) -> Au {
        size.min(self.max).max(self.min)
    }
}

/// The main sizes of the items of a line `available` long (gaps taken off), from their flex
/// factors, minimums and maximums (CSS Flexbox 1, §9.7: items that would leave their bounds are
/// frozen at them and the rest of the free space goes to the others).
pub(crate) fn resolve(items: &[FlexLength], available: Au) -> Vec<Au> {
    let hypothetical: Au = items.iter().map(FlexLength::hypothetical).sum();
    let grow = hypothetical < available;
    let factor = |item: &FlexLength| if grow { item.grow } else { item.shrink };
    let mut target: Vec<Au> = items.iter().map(FlexLength::hypothetical).collect();
    let mut frozen: Vec<bool> = items
        .iter()
        .map(|item| {
            factor(item) == 0.0
                || (grow && item.base > item.hypothetical())
                || (!grow && item.base < item.hypothetical())
        })
        .collect();
    let free = |target: &[Au], frozen: &[bool]| {
        available
            - items
                .iter()
                .zip(target)
                .zip(frozen)
                .map(|((item, &target), &frozen)| if frozen { target } else { item.base })
                .sum::<Au>()
    };
    let initial = free(&target, &frozen);
    while frozen.iter().any(|frozen| !frozen) {
        let mut remaining = free(&target, &frozen).to_px();
        let factors: f32 = items
            .iter()
            .zip(&frozen)
            .filter(|(_, frozen)| !**frozen)
            .map(|(item, _)| factor(item))
            .sum();
        if factors < 1.0 {
            let scaled = initial.to_px() * factors;
            if scaled.abs() < remaining.abs() {
                remaining = scaled;
            }
        }
        // Items shrink in proportion to their flex shrink factor times their inner base size.
        let scaled: f32 = items
            .iter()
            .zip(&frozen)
            .filter(|(_, frozen)| !**frozen)
            .map(|(item, _)| item.shrink * inner(item).to_px())
            .sum();
        let mut violation = Au::ZERO;
        let mut violations = vec![Au::ZERO; items.len()];
        for (index, item) in items.iter().enumerate() {
            if frozen[index] {
                continue;
            }
            let wanted = if grow {
                if factors > 0.0 {
                    item.base + Au::from_px(remaining * (item.grow / factors))
                } else {
                    item.base
                }
            } else if scaled > 0.0 {
                item.base + Au::from_px(remaining * item.shrink * inner(item).to_px() / scaled)
            } else {
                item.base
            };
            let clamped = item.clamp(wanted);
            violations[index] = clamped - wanted;
            violation += clamped - wanted;
            target[index] = clamped;
        }
        for (index, item_violation) in violations.into_iter().enumerate() {
            if frozen[index] {
                continue;
            }
            frozen[index] = violation == Au::ZERO
                || (violation > Au::ZERO && item_violation > Au::ZERO)
                || (violation < Au::ZERO && item_violation < Au::ZERO);
        }
    }
    target
}

/// An item's base size without its border, padding and margins.
fn inner(item: &FlexLength) -> Au {
    (item.base - item.frame).clamp_non_negative()
}

impl Layouter<'_> {
    /// The main sizes of an item of a flex row `width` wide: its base size from `flex-basis` or
    /// `width`, and its minimum and maximum from `min-width` and `max-width`, where `min-width:
    /// auto` is the automatic minimum size, the content's min-content width (no larger than a
    /// fixed width), or zero for a scroll container. `frame` is the item's border, padding and
    /// margins and `(min_content, max_content)` its contributions.
    pub(crate) fn flex_length(
        &self,
        child: BoxId,
        width: Au,
        frame: Au,
        (min_content, max_content): (Au, Au),
        cross: Option<Au>,
    ) -> FlexLength {
        let style = &self.tree.get(child).style;
        let replaced = matches!(self.tree.get(child).kind, BoxKind::Replaced { .. });
        // A ratio turns a definite cross size into a width (CSS Flexbox 1, §9.2 step 3B).
        let ratio = match &self.tree.get(child).kind {
            BoxKind::Replaced { image, .. } => style
                .aspect_ratio
                .or_else(|| image.as_ref().and_then(|image| image.ratio)),
            _ => style.aspect_ratio,
        }
        .filter(|ratio| *ratio > 0.0);
        let transferred = ratio.zip(cross).map(|(ratio, cross)| {
            let model = self.box_model(style, width, Some(Au::ZERO));
            let frame_y =
                model.border[TOP] + model.border[BOTTOM] + model.padding[TOP] + model.padding[BOTTOM];
            if style.box_sizing == BoxSizing::BorderBox {
                let (left, right) = self.flex_margins(style, width);
                let frame_x = frame - left - right;
                (Au::from_px((cross + frame_y).to_px() * ratio)).max(frame_x) - frame_x + frame
            } else {
                Au::from_px(cross.to_px() * ratio) + frame
            }
        });
        let margins = {
            let (left, right) = self.flex_margins(style, width);
            left + right
        };
        // A length or percentage as the margin box's width.
        let outer = |length: &LengthPercentage| {
            let value = Au::from_px(length.resolve(width.to_px()));
            if style.box_sizing == BoxSizing::BorderBox {
                value.max(frame - margins) + margins
            } else {
                value + frame
            }
        };
        let content = std::cell::OnceCell::new();
        let content = || {
            *content.get_or_init(|| {
                let (min, max) = self.content_box_widths(child);
                (min + frame, max + frame)
            })
        };
        // A sizing keyword's width, of the margin box.
        let keyword = |size: &Size| -> Option<Au> {
            match size {
                Size::MinContent => Some(content().0),
                Size::MaxContent => Some(content().1),
                Size::FitContent(_) => {
                    let (min, max) = content();
                    Some(max.min(width.max(min)))
                }
                Size::Stretch => Some(width.max(frame)),
                _ => None,
            }
        };
        let base = match (&style.flex_basis, &style.width) {
            (Size::Length(length), _) | (Size::Auto, Size::Length(length)) => outer(length),
            // So does `flex-basis: content` (here `max-content`), whatever the width.
            (Size::Auto, Size::Auto) | (Size::MaxContent, _) if let Some(transferred) = transferred => {
                transferred
            }
            // A replaced item is as wide as in a block as wide as the container, as in
            // Chrome: an SVG with a ratio only fills it.
            _ if replaced => self.replaced_size(child, width, None, None, false).0 + frame,
            (Size::Auto, size) => keyword(size).unwrap_or(max_content),
            (basis, _) => keyword(basis).unwrap_or(max_content),
        };
        // A `calc-size()` width of the size its basis gives.
        let base = if style.flex_basis == Size::Auto && crate::sizing::calc_size(style, 0).is_some() {
            let content_box = (base - frame).clamp_non_negative();
            crate::sizing::calculated(style, 0, content_box, frame - margins, width) + frame
        } else {
            base
        };
        let max = match &style.max_width {
            MaxSize::Length(length) => outer(length),
            MaxSize::MinContent => content().0,
            MaxSize::MaxContent => content().1,
            MaxSize::FitContent(_) => keyword(&Size::FitContent(None)).unwrap_or(Au::MAX),
            MaxSize::Stretch => width.max(frame),
            MaxSize::None => Au::MAX,
        };
        let scrolls = crate::sizing::is_scroll_container(style);
        let min = match &style.min_width {
            Size::Length(length) => outer(length),
            Size::Auto if scrolls => frame,
            // The content size suggestion, no larger than a fixed width (the specified size
            // suggestion) or the maximum (CSS Flexbox 1, §4.5).
            Size::Auto => {
                let specified = match &style.width {
                    Size::Length(length) => Some(outer(length)),
                    _ => None,
                };
                // The content's own min-content width, whatever the item's `width`.
                // A replaced item with a ratio and a definite height has the width that gives it.
                let control = self.tree.control_widths.get(&child);
                let suggestion = match transferred {
                    // A checkbox or text field: its content with `width: auto`.
                    _ if let Some(&control) = control => Au::from_px(control) + frame,
                    Some(transferred) if replaced => transferred,
                    // A box whose ratio turns its definite height into a width: its min-content
                    // width is that (see `crate::ratio`).
                    Some(_) if matches!(style.width, Size::Auto) => min_content,
                    // A replaced item's min-content width within the widths its minimum and
                    // maximum heights give through its ratio.
                    None if replaced && let Some(ratio) = ratio => {
                        let model = self.box_model(style, width, Some(Au::ZERO));
                        let frame_y = model.border[TOP]
                            + model.border[BOTTOM]
                            + model.padding[TOP]
                            + model.padding[BOTTOM];
                        let bound = |length: &LengthPercentage| {
                            let height = Au::from_px(length.fixed()?);
                            let height = if style.box_sizing == BoxSizing::BorderBox {
                                (height - frame_y).clamp_non_negative()
                            } else {
                                height
                            };
                            Some(Au::from_px(height.to_px() * ratio) + frame)
                        };
                        let low = match &style.min_height {
                            Size::Length(length) => bound(length),
                            _ => None,
                        };
                        let high = match &style.max_height {
                            MaxSize::Length(length) => bound(length),
                            _ => None,
                        };
                        // Bounded so, its min-content width is its natural width, whatever its
                        // `width`, as in Chrome.
                        let natural = match &self.tree.get(child).kind {
                            BoxKind::Replaced {
                                image: Some(image), ..
                            } if image.natural.0 && (low.is_some() || high.is_some()) => {
                                Au::from_px(image.width) + frame
                            }
                            _ => content().0,
                        };
                        natural.min(high.unwrap_or(Au::MAX)).max(low.unwrap_or(Au::ZERO))
                    }
                    _ if self.is_orthogonal(child) => min_content,
                    _ => content().0,
                };
                suggestion.min(specified.unwrap_or(Au::MAX)).min(max)
            }
            other => keyword(other).unwrap_or(frame),
        };
        // A table is never narrower than its min-content width.
        let min = if matches!(self.tree.get(child).kind, BoxKind::Table { .. }) {
            min.max(self.table_intrinsic(child).0 + margins)
        } else {
            min
        };
        FlexLength {
            base,
            frame,
            min,
            max,
            grow: style.flex_grow,
            shrink: style.flex_shrink,
        }
    }
}

impl Layouter<'_> {
    /// The main sizes of an item of a flex column `width` wide (heights), whose margin box is
    /// `laid_out` tall at its own height and `content` tall at its content's; `height` is the
    /// container's height if it is definite, which percentages refer to. A table's definite
    /// flex basis is that of its box, below and above which its `captions` come, as in Blink.
    pub(crate) fn column_flex_length(
        &self,
        child: BoxId,
        width: Au,
        height: Option<Au>,
        (laid_out, content, captions): (Au, Au, Au),
    ) -> FlexLength {
        let style = &self.tree.get(child).style;
        let model = self.box_model(style, width, Some(Au::ZERO));
        let margins = self.vertical_margin(style, width, true) + self.vertical_margin(style, width, false);
        let frame =
            model.border[TOP] + model.border[BOTTOM] + model.padding[TOP] + model.padding[BOTTOM] + margins;
        let outer = |length: &LengthPercentage| {
            let px = match length.fixed() {
                Some(px) => px,
                None => length.resolve(height?.to_px()),
            };
            let value = Au::from_px(px);
            Some(if style.box_sizing == BoxSizing::BorderBox {
                value.max(frame - margins) + margins
            } else {
                value + frame
            })
        };
        let specified = match &style.height {
            Size::Length(length) => outer(length),
            _ => None,
        };
        // An automatic height is the content's, whatever the minimum and maximum heights; so is
        // a content-based flex basis (`flex-basis: content`), whatever the height.
        let auto_height = !matches!(style.height, Size::Length(_))
            || matches!(
                style.flex_basis,
                Size::MinContent | Size::MaxContent | Size::FitContent(_)
            );
        let base = match &style.flex_basis {
            Size::Length(length) => outer(length),
            Size::Auto => specified,
            _ => None,
        }
        .map_or(if auto_height { content } else { laid_out }, |base| {
            base + captions
        });
        // Content-based keywords are the content's height in the block axis (CSS Box Sizing 3,
        // §3.1).
        let max = match &style.max_height {
            MaxSize::Length(length) => outer(length),
            MaxSize::MinContent | MaxSize::MaxContent | MaxSize::FitContent(_) => Some(content),
            _ => None,
        }
        .unwrap_or(Au::MAX);
        let min = match &style.min_height {
            Size::Length(length) => outer(length).unwrap_or(frame),
            Size::Auto if crate::sizing::is_scroll_container(style) => frame,
            // The content's height, no taller than a definite height or the maximum.
            Size::Auto => content.min(specified.unwrap_or(Au::MAX)).min(max),
            _ => content,
        };
        FlexLength {
            base,
            frame,
            min,
            max,
            grow: style.flex_grow,
            shrink: style.flex_shrink,
        }
    }

    /// The min-content and max-content contributions of item `child` of a flex row to the
    /// row's own (margin boxes; `wraps` tells whether the row wraps, which leaves the min-content
    /// contribution as it is): those of the item, but its hypothetical width where the item has
    /// a definite flex basis and cannot grow or shrink from it to them, as in Blink; `cross` is
    /// the item's definite content height, if stretched to one (see
    /// [`Layouter::stretched_intrinsic`])
    /// (`FlexLayoutAlgorithm::ComputeMinMaxSizeOfRowContainer`).
    pub(crate) fn row_item_contributions(&self, child: BoxId, wraps: bool, cross: Option<Au>) -> (Au, Au) {
        let (min_content, max_content) = self.stretched_intrinsic(child, cross);
        let style = &self.tree.get(child).style;
        let fixed = |size: &Size| matches!(size, Size::Length(length) if length.fixed().is_some());
        let definite = match &style.flex_basis {
            Size::Auto => fixed(&style.width),
            basis => fixed(basis),
        };
        if !definite || self.is_orthogonal(child) {
            return (min_content, max_content);
        }
        let model = self.box_model(style, Au::ZERO, Some(Au::ZERO));
        let (left, right) = self.flex_margins(style, Au::ZERO);
        let frame = model.border[crate::block::LEFT]
            + model.border[crate::block::RIGHT]
            + model.padding[crate::block::LEFT]
            + model.padding[crate::block::RIGHT]
            + left
            + right;
        let length = self.flex_length(child, Au::ZERO, frame, (min_content, max_content), None);
        let hypothetical = length.hypothetical();
        let pick = |contribution: Au| {
            let stuck = (contribution > length.base && length.grow == 0.0)
                || (contribution < length.base && length.shrink == 0.0);
            if stuck { hypothetical } else { contribution }
        };
        (
            if wraps { min_content } else { pick(min_content) },
            pick(max_content),
        )
    }

    /// Whether item `child` of a column flex container whose height is its content's has a
    /// definite flex basis (a fixed length, or `auto` with a fixed height), which makes its
    /// height after flexing definite: percentage heights of its children refer to it, as in
    /// Chrome (CSS Flexbox 1, §9.8).
    pub(crate) fn definite_column_basis(&self, child: BoxId) -> bool {
        let style = &self.tree.get(child).style;
        let fixed = |size: &Size| matches!(size, Size::Length(length) if length.fixed().is_some());
        match &style.flex_basis {
            Size::Auto => fixed(&style.height),
            basis => fixed(basis),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(base: f32, min: f32, max: f32, grow: f32, shrink: f32) -> FlexLength {
        FlexLength {
            base: Au::from_px(base),
            frame: Au::ZERO,
            min: Au::from_px(min),
            max: Au::from_px(max),
            grow,
            shrink,
        }
    }

    fn px(sizes: Vec<Au>) -> Vec<f32> {
        sizes.into_iter().map(Au::to_px).collect()
    }

    #[test]
    fn grows_by_factors() {
        let items = [item(0.0, 0.0, 1e6, 1.0, 1.0), item(0.0, 0.0, 1e6, 3.0, 1.0)];
        assert_eq!(px(resolve(&items, Au::from_px(400.0))), [100.0, 300.0]);
    }

    #[test]
    fn a_maximum_gives_the_rest_to_others() {
        let items = [item(0.0, 0.0, 50.0, 1.0, 1.0), item(0.0, 0.0, 1e6, 1.0, 1.0)];
        assert_eq!(px(resolve(&items, Au::from_px(400.0))), [50.0, 350.0]);
    }

    #[test]
    fn shrinks_by_base_size_and_stops_at_the_minimum() {
        let items = [item(300.0, 250.0, 1e6, 0.0, 1.0), item(300.0, 0.0, 1e6, 0.0, 1.0)];
        assert_eq!(px(resolve(&items, Au::from_px(400.0))), [250.0, 150.0]);
    }

    #[test]
    fn small_grow_factors_take_part_of_the_space() {
        let items = [item(100.0, 0.0, 1e6, 0.5, 1.0)];
        assert_eq!(px(resolve(&items, Au::from_px(300.0))), [200.0]);
    }

    #[test]
    fn inflexible_items_keep_their_hypothetical_size() {
        let items = [item(0.0, 68.0, 1e6, 0.0, 0.0), item(10.0, 0.0, 1e6, 0.0, 0.0)];
        assert_eq!(px(resolve(&items, Au::from_px(400.0))), [68.0, 10.0]);
    }
}

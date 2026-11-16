//! The contributions of grid items whose widths depend on the height of their grid area (CSS
//! Grid 2, §12.1, steps 1 and 3): an item with a percentage height or an aspect ratio, a replaced
//! element stretched along the column axis, and a box stretched or of a percentage height whose
//! replaced children have percentage heights. Where the rows an item spans are of a fixed size,
//! the columns are sized from what the item needs in them; where they are not, the columns are
//! sized once more with the rows the first pass gave, as in Chrome (`grid-item-inline-contribution-*`
//! and `aspect-ratio-*` in WPT `css-grid/grid-items`).

use fagus_style::ComputedStyle;
use fagus_style::values::{BoxSizing, ItemAlignment, LengthPercentage, MaxSize, Size};

use crate::block::{BOTTOM, LEFT, Layouter, RIGHT, TOP, item_alignment};
use crate::subgrid::Axis;
use crate::tree::{BoxId, BoxKind};
use crate::units::Au;

/// Whether a style's height, minimum or maximum height is a percentage, which refers to the
/// height of the grid area.
pub(crate) fn percent_height(style: &ComputedStyle) -> bool {
    let percentage = |length: &LengthPercentage| length.fixed().is_none();
    matches!(&style.height, Size::Length(length) if percentage(length))
        || matches!(&style.min_height, Size::Length(length) if percentage(length))
        || matches!(&style.max_height, MaxSize::Length(length) if percentage(length))
}

/// Whether a replaced element has a preferred aspect ratio: its own or its image's.
pub(crate) fn has_ratio(kind: &BoxKind, style: &ComputedStyle) -> bool {
    style.aspect_ratio.is_some_and(|ratio| ratio > 0.0)
        || matches!(kind, BoxKind::Replaced { image: Some(image), .. } if image.ratio.is_some())
}

impl Layouter<'_> {
    /// Whether grid item `id` of a container of style `container` stretches to the height of its
    /// area for its contributions: `align-self: stretch` (or `normal`, but not for a replaced
    /// element), an automatic height and no automatic margins along the column (CSS Grid 2,
    /// §11.2). A box with an aspect ratio counts as stretched here, though `normal` leaves its
    /// height to the ratio in the end (see [`Layouter::ratio_item`]): the columns are sized again
    /// with the rows it spans, as in Chrome, whose rows of such items come out a little taller
    /// than the items.
    pub(crate) fn stretches_in_area(&self, container: &ComputedStyle, id: BoxId) -> bool {
        let style = &self.tree.get(id).style;
        let replaced = self.tree.is_replaced(id);
        let alignment = item_alignment(container, style);
        (alignment == ItemAlignment::Stretch || (alignment == ItemAlignment::Normal && !replaced))
            && matches!(style.height, Size::Auto)
            && !style.margin.top.is_auto()
            && !style.margin.bottom.is_auto()
    }

    /// Whether grid item `id` is a box with a preferred aspect ratio that is not a replaced
    /// element, in a horizontal writing mode: `normal` alignment does not stretch its height,
    /// which follows its width through the ratio, nor its width where its height is not
    /// automatic or is stretched explicitly (as Blink's `ComputeBlockSizeForFragment` and
    /// `ComputeInlineSizeForFragment` prefer the ratio to an implicit stretch).
    pub(crate) fn ratio_item(&self, id: BoxId) -> bool {
        let style = &self.tree.get(id).style;
        !self.tree.is_replaced(id) && !style.is_vertical() && crate::ratio::ratio(style).is_some()
    }

    /// Whether the min-content and max-content contributions of grid item `id` depend on the
    /// height of its grid area (see the module).
    pub(crate) fn area_dependent(&self, container: &ComputedStyle, id: BoxId) -> bool {
        let layout_box = self.tree.get(id);
        let style = &layout_box.style;
        if self.is_subgrid(id, Axis::Columns) || !matches!(style.width, Size::Auto) {
            return false;
        }
        let percent = percent_height(style);
        let stretches = self.stretches_in_area(container, id);
        if matches!(layout_box.kind, BoxKind::Replaced { .. }) {
            return percent || (stretches && has_ratio(&layout_box.kind, style));
        }
        if style.aspect_ratio.is_some_and(|ratio| ratio > 0.0) {
            return percent || stretches;
        }
        // The percentage heights of replaced elements inside, also in anonymous boxes, refer to
        // the item's height.
        let holds_percentages = |id: BoxId| {
            self.tree.children(id).into_iter().any(|child| {
                self.percentage_replaced(child)
                    || (self.tree.get(child).node.is_none()
                        && self
                            .tree
                            .children(child)
                            .into_iter()
                            .any(|inner| self.percentage_replaced(inner)))
            })
        };
        (percent || stretches) && holds_percentages(id)
    }

    /// The content width of replaced grid item `id`, or one with an aspect ratio (see
    /// [`Layouter::ratio_item`]), that stretches to the height of its area (`area`, with its
    /// vertical `margins`) through its ratio, where it does not stretch along the row as well:
    /// `None` where it does or has no ratio.
    pub(crate) fn stretched_replaced_width(
        &self,
        container: &ComputedStyle,
        id: BoxId,
        area: (Au, Au),
        margins: (Au, Au),
    ) -> Option<Au> {
        let layout_box = self.tree.get(id);
        let style = &layout_box.style;
        if !has_ratio(&layout_box.kind, style) {
            return None;
        }
        let justified = crate::grid::justify(container, style) == ItemAlignment::Stretch
            && !style.margin.left.is_auto()
            && !style.margin.right.is_auto();
        if justified || !matches!(style.width, Size::Auto) {
            return None;
        }
        let model = self.box_model(style, area.0, None);
        let frame_y = model.border[TOP] + model.border[BOTTOM] + model.padding[TOP] + model.padding[BOTTOM];
        if self.ratio_item(id) {
            let ratio = crate::ratio::ratio(style)?;
            let frame_x =
                model.border[LEFT] + model.border[RIGHT] + model.padding[LEFT] + model.padding[RIGHT];
            let height = (area.1 - margins.0 - margins.1).clamp_non_negative();
            let width = crate::ratio::width_from_height(style, ratio, (frame_x, frame_y), height);
            return Some(self.clamped_width(id, (width - frame_x).clamp_non_negative(), area.0, area.0));
        }
        let (width, height) = self.replaced_size(id, area.0, None, None, false);
        if height <= Au::ZERO || width <= Au::ZERO {
            return None;
        }
        let content_height = (area.1 - margins.0 - margins.1 - frame_y).clamp_non_negative();
        Some(Au::from_px(
            content_height.to_px() * width.to_px() / height.to_px(),
        ))
    }

    /// The minimum content width of a grid container of style `style` without an available
    /// width: its fixed `min-width`, or the width its fixed `min-height` gives through its aspect
    /// ratio. Tracks repeated automatically fill it (see `crate::grid::Room`).
    pub(crate) fn repeat_minimum(&self, style: &ComputedStyle) -> Option<Au> {
        let model = self.box_model(style, Au::ZERO, None);
        let frame_x = model.border[LEFT] + model.border[RIGHT] + model.padding[LEFT] + model.padding[RIGHT];
        let border_box = style.box_sizing == BoxSizing::BorderBox;
        if let Size::Length(length) = &style.min_width
            && let Some(px) = length.fixed()
            && px > 0.0
        {
            let width = Au::from_px(px);
            return Some(if border_box {
                (width - frame_x).clamp_non_negative()
            } else {
                width
            });
        }
        let ratio = style.aspect_ratio.filter(|ratio| *ratio > 0.0)?;
        let Size::Length(length) = &style.min_height else {
            return None;
        };
        let height = Au::from_px(length.fixed()?);
        Some(if border_box {
            (Au::from_px(height.to_px() * ratio) - frame_x).clamp_non_negative()
        } else {
            Au::from_px(height.to_px() * ratio)
        })
    }

    /// The width a fixed `min-height` gives grid item `id` of an automatic width through its
    /// aspect ratio (CSS Box Sizing 4, §5.1), as a margin box: its contributions are at least
    /// that, also where its rows are not sized yet.
    pub(crate) fn ratio_minimum(&self, id: BoxId) -> Option<Au> {
        let layout_box = self.tree.get(id);
        let style = &layout_box.style;
        let ratio = style.aspect_ratio.filter(|ratio| *ratio > 0.0)?;
        if matches!(layout_box.kind, BoxKind::Replaced { .. }) || !matches!(style.width, Size::Auto) {
            return None;
        }
        let Size::Length(length) = &style.min_height else {
            return None;
        };
        let min_height = Au::from_px(length.fixed()?);
        let model = self.box_model(style, Au::ZERO, None);
        let frame_x = model.border[LEFT] + model.border[RIGHT] + model.padding[LEFT] + model.padding[RIGHT];
        let width = if style.box_sizing == BoxSizing::BorderBox {
            Au::from_px(min_height.to_px() * ratio).max(frame_x)
        } else {
            Au::from_px(min_height.to_px() * ratio) + frame_x
        };
        Some(width + model.margin[LEFT] + model.margin[RIGHT])
    }

    /// The minimum contribution of grid item `id` to the rows it spans where its `min-height` is
    /// a length and its `height` is not a fixed length or intrinsic keyword (CSS Grid 2, §6.6,
    /// as Blink's `CalculateIntrinsicMinimumContribution`): its border box at that minimum,
    /// percentages of which refer to nothing yet, so an item of `height: 100%; min-height: 100%`
    /// lets a grid of a fixed height give its `auto` row the room left. `None` for other items,
    /// whose minimum contribution is their height. `basis` is the width of the item's area.
    pub(crate) fn row_minimum(&self, id: BoxId, basis: Au) -> Option<Au> {
        let style = &self.tree.get(id).style;
        let Size::Length(min_height) = &style.min_height else {
            return None;
        };
        let preferred_auto = match &style.height {
            Size::Auto | Size::Stretch | Size::FitContent(_) => true,
            Size::Length(length) => length.fixed().is_none(),
            Size::MinContent | Size::MaxContent => false,
        };
        if !preferred_auto {
            return None;
        }
        let model = self.box_model(style, basis, None);
        let frame_y = model.border[TOP] + model.border[BOTTOM] + model.padding[TOP] + model.padding[BOTTOM];
        let min_height = Au::from_px(min_height.resolve(0.0)).clamp_non_negative();
        Some(match style.box_sizing {
            BoxSizing::BorderBox => min_height.max(frame_y),
            BoxSizing::ContentBox => min_height + frame_y,
        })
    }

    /// The min-content and max-content contributions of grid item `id` (see
    /// [`Layouter::area_dependent`]) in a grid area `area` tall, in a grid `basis` wide.
    pub(crate) fn contributions_in_area(
        &self,
        container: &ComputedStyle,
        id: BoxId,
        basis: Au,
        area: Au,
    ) -> (Au, Au) {
        let layout_box = self.tree.get(id);
        let style = &layout_box.style;
        let model = self.box_model(style, basis, None);
        let margins_y = model.margin[TOP] + model.margin[BOTTOM];
        let margins_x = model.margin[LEFT] + model.margin[RIGHT];
        let frame_x = model.border[LEFT] + model.border[RIGHT] + model.padding[LEFT] + model.padding[RIGHT];
        let frame_y = model.border[TOP] + model.border[BOTTOM] + model.padding[TOP] + model.padding[BOTTOM];
        let stretches = self.stretches_in_area(container, id);
        let content_height = (area - margins_y - frame_y).clamp_non_negative();
        if matches!(layout_box.kind, BoxKind::Replaced { .. }) {
            if stretches {
                // As tall as the area, as wide as its ratio makes it.
                let (width, height) = self.replaced_size(id, Au::ZERO, None, None, true);
                if height > Au::ZERO {
                    let width = Au::from_px(content_height.to_px() * width.to_px() / height.to_px());
                    let outer = width + frame_x + margins_x;
                    return (outer, outer);
                }
            }
            return self.intrinsic_at_height(id, Some(area));
        }
        if let Some(ratio) = style.aspect_ratio.filter(|ratio| *ratio > 0.0) {
            // The height the area gives, through the ratio.
            let height = if stretches {
                Some((area - margins_y).clamp_non_negative())
            } else {
                match &style.height {
                    Size::Length(length) => Some(
                        Au::from_px(length.resolve(area.to_px()))
                            + match style.box_sizing {
                                BoxSizing::BorderBox => Au::ZERO,
                                BoxSizing::ContentBox => frame_y,
                            },
                    ),
                    _ => None,
                }
            };
            if let Some(height) = height {
                let width = if style.box_sizing == BoxSizing::BorderBox {
                    Au::from_px(height.to_px() * ratio).max(frame_x)
                } else {
                    Au::from_px((height - frame_y).clamp_non_negative().to_px() * ratio) + frame_x
                };
                let outer = width + margins_x;
                return (outer, outer);
            }
        }
        let height = if stretches {
            Some(content_height)
        } else {
            Self::definite_height(style, &model, Some(area), None)
        };
        self.intrinsic_at_height(id, height)
    }
}

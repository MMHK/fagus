//! Preferred aspect ratios of boxes that are not replaced elements (CSS Box Sizing 4, §5), as
//! Blink applies them (`InlineSizeFromAspectRatio`, `ComputeMinMaxInlineSizesFromAspectRatio`
//! and `BlockNode::ComputeMinMaxSizes` in Chromium, BSD-licensed; see `CHROMIUM-LICENSE`).
//!
//! A box whose height is definite takes its width from it through the ratio, also where its
//! width is content-based: its min-content and max-content widths are that width (no less
//! than its content's min-content width while its `min-width` is `auto`). A box whose height
//! is not definite keeps its content's widths, within the widths its `min-height` and
//! `max-height` give through the ratio.

use fagus_style::ComputedStyle;
use fagus_style::values::{BoxSizing, LengthPercentage, MaxSize, Size};

use crate::block::{BOTTOM, LEFT, Layouter, RIGHT, TOP};
use crate::tree::{BoxId, BoxKind};
use crate::units::Au;

/// The box an aspect ratio sizes: the one `box-sizing` names, but the content box for
/// `aspect-ratio: auto && <ratio>` (CSS Box Sizing 4, §5.1).
pub(crate) fn ratio_sizing(style: &ComputedStyle) -> BoxSizing {
    if style.aspect_ratio_auto {
        BoxSizing::ContentBox
    } else {
        style.box_sizing
    }
}

/// The preferred aspect ratio (width over height) of a box, if it has one.
pub(crate) fn ratio(style: &ComputedStyle) -> Option<f32> {
    style
        .aspect_ratio
        .filter(|ratio| ratio.is_finite() && *ratio > 0.0)
}

/// A border-box width from a border-box height `height` through `ratio`, for a box with
/// horizontal and vertical borders and paddings `frame` (Blink's `InlineSizeFromAspectRatio`).
pub(crate) fn width_from_height(
    style: &ComputedStyle,
    ratio: f32,
    (frame_x, frame_y): (Au, Au),
    height: Au,
) -> Au {
    if ratio_sizing(style) == BoxSizing::BorderBox {
        frame_x.max(Au::from_px(height.to_px() * ratio))
    } else {
        Au::from_px((height - frame_y).clamp_non_negative().to_px() * ratio) + frame_x
    }
}

/// A border-box height from a length of `height`, `min-height` or `max-height`, whose
/// percentages refer to `containing_height` (`None` where they cannot be resolved).
fn border_box_height(
    style: &ComputedStyle,
    length: &LengthPercentage,
    frame_y: Au,
    containing_height: Option<Au>,
) -> Option<Au> {
    let px = length
        .fixed()
        .or_else(|| containing_height.map(|height| length.resolve(height.to_px())))?;
    let height = Au::from_px(px);
    Some(if style.box_sizing == BoxSizing::BorderBox {
        height.max(frame_y)
    } else {
        height + frame_y
    })
}

/// A border-box height within a box's `min-height` and `max-height` (those that resolve
/// without its content).
pub(crate) fn bounded_height(
    style: &ComputedStyle,
    frame_y: Au,
    containing_height: Option<Au>,
    height: Au,
) -> Au {
    let (min, max) = height_bounds(style, frame_y, containing_height);
    height.min(max.unwrap_or(Au::MAX)).max(min)
}

/// The border-box `min-height` and `max-height` of a box that resolve without its content: the
/// minimum zero and the maximum `None` where they do not.
fn height_bounds(style: &ComputedStyle, frame_y: Au, containing_height: Option<Au>) -> (Au, Option<Au>) {
    let min = match &style.min_height {
        Size::Length(length) => border_box_height(style, length, frame_y, containing_height),
        _ => None,
    }
    .unwrap_or(Au::ZERO);
    let max = match &style.max_height {
        MaxSize::Length(length) => border_box_height(style, length, frame_y, containing_height),
        _ => None,
    };
    (min, max.map(|max| max.max(min)))
}

/// The border-box height of a box that does not depend on its content (a length, or a
/// percentage of a definite `containing_height`), within its minimum and maximum.
pub(crate) fn definite_height(
    style: &ComputedStyle,
    frame_y: Au,
    containing_height: Option<Au>,
) -> Option<Au> {
    let Size::Length(length) = &style.height else {
        return None;
    };
    let height = border_box_height(style, length, frame_y, containing_height)?;
    Some(bounded_height(style, frame_y, containing_height, height))
}

/// The border-box widths that a box's `min-height` and `max-height` give through its aspect
/// ratio: the minimum zero and the maximum `Au::MAX` where they give none (Blink's
/// `ComputeTransferredMinMaxInlineSizes`).
pub(crate) fn transferred_widths(
    style: &ComputedStyle,
    ratio: f32,
    frame: (Au, Au),
    containing_height: Option<Au>,
) -> (Au, Au) {
    let (min, max) = height_bounds(style, frame.1, containing_height);
    let low = if min > Au::ZERO {
        width_from_height(style, ratio, frame, min)
    } else {
        Au::ZERO
    };
    let high = max.map_or(Au::MAX, |max| width_from_height(style, ratio, frame, max));
    (low, high.max(low))
}

/// The size of a replaced element whose width and height are both automatic, `width` ×
/// `height` from its natural sizes and ratio, within its minimum and maximum widths and heights
/// (`(min, max)` each, content boxes), keeping the ratio where it can (the table of CSS 2.1,
/// §10.4; the minimums win over the maximums).
pub(crate) fn constrain_both(
    (width, height): (f32, f32),
    (min_width, max_width): (f32, f32),
    (min_height, max_height): (f32, f32),
) -> (f32, f32) {
    let max_width = max_width.max(min_width);
    let max_height = max_height.max(min_height);
    if width <= 0.0 || height <= 0.0 {
        return (
            width.clamp(min_width, max_width),
            height.clamp(min_height, max_height),
        );
    }
    let (wide, narrow) = (width > max_width, width < min_width);
    let (tall, short) = (height > max_height, height < min_height);
    match (wide, narrow, tall, short) {
        (true, _, true, _) if max_width / width <= max_height / height => {
            (max_width, min_height.max(max_width * height / width))
        }
        (true, _, true, _) => (min_width.max(max_height * width / height), max_height),
        (_, true, _, true) if min_width / width <= min_height / height => {
            (max_width.min(min_height * width / height), min_height)
        }
        (_, true, _, true) => (min_width, max_height.min(min_width * height / width)),
        (_, true, true, _) => (min_width, max_height),
        (true, _, _, true) => (max_width, min_height),
        (true, ..) => (max_width, min_height.max(max_width * height / width)),
        (_, true, ..) => (min_width, max_height.min(min_width * height / width)),
        (_, _, true, _) => (min_width.max(max_height * width / height), max_height),
        (_, _, _, true) => (max_width.min(min_height * width / height), min_height),
        _ => (width, height),
    }
}

impl Layouter<'_> {
    /// The min-content and max-content widths (border box plus horizontal margins) of box `id`
    /// with an aspect ratio, from `content`, those of its content, where percentage heights
    /// refer to `containing_height` and `given` is the content height the box is given whatever
    /// its own (see [`Layouter::intrinsic_at_height`]); `content` itself for boxes without one,
    /// replaced elements and tables.
    pub(crate) fn ratio_widths(
        &self,
        id: BoxId,
        (containing_height, given): (Option<Au>, Option<Au>),
        content: (Au, Au),
    ) -> (Au, Au) {
        let layout_box = self.tree.get(id);
        let style = &layout_box.style;
        let Some(ratio) = ratio(style) else {
            return content;
        };
        if !matches!(
            layout_box.kind,
            BoxKind::Block { .. } | BoxKind::Inline { .. } | BoxKind::Flex { .. } | BoxKind::Grid { .. }
        ) {
            return content;
        }
        let model = self.box_model(style, Au::ZERO, Some(Au::ZERO));
        let frame = (
            model.border[LEFT] + model.border[RIGHT] + model.padding[LEFT] + model.padding[RIGHT],
            model.border[TOP] + model.border[BOTTOM] + model.padding[TOP] + model.padding[BOTTOM],
        );
        let margins = model.margin[LEFT] + model.margin[RIGHT];
        let given = given.map(|height| bounded_height(style, frame.1, containing_height, height + frame.1));
        if let Some(height) = given.or_else(|| definite_height(style, frame.1, containing_height)) {
            let width = width_from_height(style, ratio, frame, height) + margins;
            // The automatic minimum size: no narrower than the content's min-content width.
            let auto_minimum =
                matches!(style.min_width, Size::Auto) && !crate::sizing::is_scroll_container(style);
            let width = if auto_minimum { width.max(content.0) } else { width };
            return (width, width);
        }
        let (low, high) = transferred_widths(style, ratio, frame, containing_height);
        if low == Au::ZERO && high == Au::MAX {
            return content;
        }
        let clamp = |width: Au| (width - margins).min(high).max(low) + margins;
        (clamp(content.0), clamp(content.1))
    }

    /// The content-box width of a block-level box `id` with an aspect ratio and `width: auto`
    /// in block flow, in a containing block `containing_width` wide whose height is
    /// `containing_height` if definite: through the ratio from its height where that is
    /// definite (no narrower than its content's min-content width while its `min-width` is
    /// `auto`, the automatic minimum size), else the width it fills within the widths its
    /// minimum and maximum heights give (CSS Box Sizing 4, §5.1 and §5.2). `None` where the
    /// ratio does not decide the width.
    pub(crate) fn block_ratio_width(
        &self,
        id: BoxId,
        containing_width: Au,
        containing_height: Option<Au>,
    ) -> Option<Au> {
        let layout_box = self.tree.get(id);
        let style = &layout_box.style;
        let ratio = ratio(style)?;
        if !matches!(style.width, Size::Auto)
            || !matches!(
                layout_box.kind,
                BoxKind::Block { .. } | BoxKind::Inline { .. } | BoxKind::Flex { .. } | BoxKind::Grid { .. }
            )
        {
            return None;
        }
        let model = self.box_model(style, containing_width, Some(Au::ZERO));
        let frame = (
            model.border[LEFT] + model.border[RIGHT] + model.padding[LEFT] + model.padding[RIGHT],
            model.border[TOP] + model.border[BOTTOM] + model.padding[TOP] + model.padding[BOTTOM],
        );
        let margins = |value: &fagus_style::values::LengthPercentageAuto| {
            Au::from_px(value.resolve_or(containing_width.to_px(), 0.0))
        };
        let fill = (containing_width - margins(&style.margin.left) - margins(&style.margin.right) - frame.0)
            .clamp_non_negative();
        if let Some(height) = definite_height(style, frame.1, containing_height) {
            let width = (width_from_height(style, ratio, frame, height) - frame.0).clamp_non_negative();
            let mut width = self.clamped_width(id, width, containing_width, containing_width);
            if matches!(style.min_width, Size::Auto) && !crate::sizing::is_scroll_container(style) {
                let (min_content, _) = self.content_box_widths(id);
                // The automatic minimum stays within the maximum.
                let max = self.clamped_width(id, Au::MAX, containing_width, containing_width);
                width = width.max(min_content.min(max));
            }
            return Some(width);
        }
        let (low, high) = transferred_widths(style, ratio, frame, containing_height);
        if low == Au::ZERO && high == Au::MAX {
            return None;
        }
        let content = |width: Au| {
            if width == Au::MAX {
                width
            } else {
                (width - frame.0).clamp_non_negative()
            }
        };
        let width = fill.min(content(high)).max(content(low));
        Some(self.clamped_width(id, width, containing_width, containing_width))
    }

    /// Whether the widths of box `id` depend on the height that percentage heights inside the
    /// box refer to: a box with an aspect ratio and a percentage height, minimum or maximum.
    pub(crate) fn ratio_percentage(&self, id: BoxId) -> bool {
        let style = &self.tree.get(id).style;
        let percentage = |length: &LengthPercentage| length.fixed().is_none();
        ratio(style).is_some()
            && (matches!(&style.height, Size::Length(length) if percentage(length))
                || matches!(&style.min_height, Size::Length(length) if percentage(length))
                || matches!(&style.max_height, MaxSize::Length(length) if percentage(length)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(css: &str) -> ComputedStyle {
        let mut style = ComputedStyle::default();
        for declaration in css.split(';').map(str::trim).filter(|d| !d.is_empty()) {
            let (name, value) = declaration.split_once(':').unwrap_or((declaration, ""));
            let px = |value: &str| value.trim().trim_end_matches("px").parse::<f32>().unwrap_or(0.0);
            match name.trim() {
                "height" => style.height = Size::Length(LengthPercentage::Px(px(value))),
                "min-height" => style.min_height = Size::Length(LengthPercentage::Px(px(value))),
                "max-height" => style.max_height = MaxSize::Length(LengthPercentage::Px(px(value))),
                "border-box" => style.box_sizing = BoxSizing::BorderBox,
                "ratio" => {
                    style.aspect_ratio = Some(px(value));
                    style.aspect_ratio_auto = false;
                }
                "auto-ratio" => {
                    style.aspect_ratio = Some(px(value));
                    style.aspect_ratio_auto = true;
                }
                _ => {}
            }
        }
        style
    }

    #[test]
    fn widths_follow_definite_heights_within_their_bounds() {
        let px = Au::from_px;
        let no_frame = (Au::ZERO, Au::ZERO);
        let square = style("height: 100px; ratio: 1");
        let height = definite_height(&square, Au::ZERO, None);
        assert_eq!(height, Some(px(100.0)));
        assert_eq!(width_from_height(&square, 1.0, no_frame, px(100.0)), px(100.0));
        // The minimum wins over the height, the maximum limits it.
        assert_eq!(
            definite_height(&style("height: 10px; min-height: 25px"), Au::ZERO, None),
            Some(px(25.0))
        );
        assert_eq!(
            definite_height(&style("height: 100px; max-height: 25px"), Au::ZERO, None),
            Some(px(25.0))
        );
        // A border-box ratio takes the padding in; `auto && <ratio>` sizes the content box.
        let padded = (Au::ZERO, px(15.0));
        let border_box = style("height: 25px; ratio: 4; border-box");
        assert_eq!(width_from_height(&border_box, 4.0, padded, px(25.0)), px(100.0));
        let content_box = style("height: 40px; auto-ratio: 4; border-box");
        assert_eq!(width_from_height(&content_box, 4.0, padded, px(40.0)), px(100.0));
        // Bounds of an indefinite height bound the width.
        let bounded = style("min-height: 10px; max-height: 20px; ratio: 2");
        assert_eq!(
            transferred_widths(&bounded, 2.0, no_frame, None),
            (px(20.0), px(40.0))
        );
        assert_eq!(
            transferred_widths(&style("ratio: 2"), 2.0, no_frame, None),
            (Au::ZERO, Au::MAX)
        );
    }

    #[test]
    fn replaced_sizes_keep_their_ratio_within_both_bounds() {
        let none = (0.0, f32::INFINITY);
        // Too wide: the maximum width, the height through the ratio.
        assert_eq!(constrain_both((200.0, 100.0), (0.0, 100.0), none), (100.0, 50.0));
        // Too wide and too tall: the bound that shrinks it most.
        assert_eq!(
            constrain_both((200.0, 100.0), (0.0, 150.0), (0.0, 50.0)),
            (100.0, 50.0)
        );
        // Too narrow and too tall: both bounds, the ratio given up.
        assert_eq!(
            constrain_both((50.0, 200.0), (75.0, f32::INFINITY), (0.0, 75.0)),
            (75.0, 75.0)
        );
        // A minimum height that the ratio would carry past the maximum width.
        assert_eq!(
            constrain_both((50.0, 50.0), (0.0, 85.0), (100.0, f32::INFINITY)),
            (85.0, 100.0)
        );
        assert_eq!(constrain_both((60.0, 30.0), none, none), (60.0, 30.0));
    }
}

//! The boxes of the `<foreignObject>` elements of an inline `<svg>` (see [`crate::foreign`]).

use std::sync::Arc;

use fagus_dom::NodeId;
use fagus_style::values::{
    BoxSizing, Display, Float, LengthPercentage, LengthPercentageAuto, Overflow, Position, Sides, Size,
};
use fagus_style::{ComputedStyle, ComputedValues};

use super::{BoxId, BoxTree, Builder};
use crate::foreign::ForeignBoxes;

impl Builder<'_> {
    /// Builds the boxes of the `<foreignObject>` elements of the inline `<svg>` element `svg`,
    /// whose box is `id`, if it has any: each a block of the element's size whose content is
    /// clipped to it, as Chrome's style sheet for SVG says (`overflow: hidden`). Returns
    /// whether it has any.
    pub(super) fn foreign_objects(&self, tree: &mut BoxTree, svg: NodeId, id: BoxId) -> bool {
        let Some(objects) = self.images.foreign(svg) else {
            return false;
        };
        let mut boxes = Vec::with_capacity(objects.objects.len());
        for (index, object) in objects.objects.iter().enumerate() {
            let Some(style) = self.styled.style(object.node) else {
                continue;
            };
            if style.display == Display::None {
                continue;
            }
            let [_, _, width, height] = object.rect;
            let style = Arc::new(ComputedStyle::from(ComputedValues {
                display: style.display.blockify(),
                position: Position::Absolute,
                inset: Sides {
                    top: LengthPercentageAuto::ZERO,
                    right: LengthPercentageAuto::Auto,
                    bottom: LengthPercentageAuto::Auto,
                    left: LengthPercentageAuto::ZERO,
                },
                float: Float::None,
                box_sizing: BoxSizing::BorderBox,
                width: Size::Length(LengthPercentage::Px(width)),
                height: Size::Length(LengthPercentage::Px(height)),
                margin: Sides {
                    top: LengthPercentageAuto::ZERO,
                    right: LengthPercentageAuto::ZERO,
                    bottom: LengthPercentageAuto::ZERO,
                    left: LengthPercentageAuto::ZERO,
                },
                overflow_x: Overflow::Hidden,
                overflow_y: Overflow::Hidden,
                transform_origin: (LengthPercentage::ZERO, LengthPercentage::ZERO),
                ..ComputedValues::clone(style)
            }));
            self.enter(object.node, &style);
            boxes.push((self.block_level_box(tree, object.node, style), index));
        }
        let any = !boxes.is_empty();
        tree.foreign.insert(
            id,
            ForeignBoxes {
                objects: Arc::clone(objects),
                boxes,
            },
        );
        any
    }
}

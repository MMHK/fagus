//! The flexible box of 2009 that `display: -webkit-box` makes (templates written for
//! `wkhtmltopdf` and older engines use it). Chrome lays it out as a flex container whose direction,
//! alignment and item sizes come from the `-webkit-box-*` properties instead of the flex ones
//! (Blink's `ComputedStyle::ResolvedFlexGrow` and `FlexLayoutAlgorithm` with `is_webkit_box_`);
//! here those properties become the flex values that the layout reads.

use crate::style::ComputedValues;
use crate::values::{
    BoxOrient, ContentAlignment, Display, DisplayInside, FlexDirection, FlexWrap, ItemAlignment, Size,
};

/// `-webkit-box-pack`: where the items go along the main axis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BoxPack {
    #[default]
    Start,
    End,
    Center,
    Justify,
}

/// `-webkit-box-align`: where the items go across.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BoxAlign {
    Start,
    End,
    Center,
    Baseline,
    #[default]
    Stretch,
}

/// The `-webkit-box-*` values of a box, and whether `display` made it a `-webkit-box`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LegacyBox {
    /// `display: -webkit-box` or `-webkit-inline-box`.
    pub is_box: bool,
    /// `-webkit-box-flex`: how much an item grows and shrinks.
    pub flex: f32,
    pub pack: BoxPack,
    pub align: BoxAlign,
    /// `-webkit-box-direction: reverse`.
    pub reverse: bool,
    /// `-webkit-box-ordinal-group`: the order of an item, in place of `order`.
    pub ordinal_group: i32,
}

impl Default for LegacyBox {
    fn default() -> Self {
        LegacyBox {
            is_box: false,
            flex: 0.0,
            pack: BoxPack::Start,
            align: BoxAlign::Stretch,
            reverse: false,
            ordinal_group: 1,
        }
    }
}

/// Whether a style is that of a `-webkit-box` laid out as a flex container.
fn is_flex_box(style: &ComputedValues) -> bool {
    style.legacy_box.is_box
        && matches!(
            style.display,
            Display::Normal {
                inside: DisplayInside::Flex,
                ..
            }
        )
}

/// Gives a `-webkit-box` the flex direction, wrapping and alignment its `-webkit-box-*`
/// properties ask for: it never wraps, and `flex-*`, `justify-content` and `align-items` do
/// not apply to it.
pub(crate) fn finish_container(style: &mut ComputedValues) {
    if !is_flex_box(style) {
        return;
    }
    let legacy = style.legacy_box;
    style.flex_direction = match (style.box_orient, legacy.reverse) {
        (BoxOrient::Horizontal, false) => FlexDirection::Row,
        (BoxOrient::Horizontal, true) => FlexDirection::RowReverse,
        (BoxOrient::Vertical, false) => FlexDirection::Column,
        (BoxOrient::Vertical, true) => FlexDirection::ColumnReverse,
    };
    style.flex_wrap = FlexWrap::NoWrap;
    style.justify_content = match legacy.pack {
        BoxPack::Start => ContentAlignment::FlexStart,
        BoxPack::End => ContentAlignment::FlexEnd,
        BoxPack::Center => ContentAlignment::Center,
        BoxPack::Justify => ContentAlignment::SpaceBetween,
    };
    style.align_items = match legacy.align {
        BoxAlign::Start => ItemAlignment::Start,
        BoxAlign::End => ItemAlignment::End,
        BoxAlign::Center => ItemAlignment::Center,
        BoxAlign::Baseline => ItemAlignment::Baseline,
        BoxAlign::Stretch => ItemAlignment::Stretch,
    };
}

/// Sizes an item of a `-webkit-box` (`parent`) as Chrome does: it grows and shrinks by its
/// `-webkit-box-flex`, goes in the order of its `-webkit-box-ordinal-group` (Blink's
/// `FlexChildIterator`), has no automatic minimum size, takes the container's alignment, and an
/// auto size along the main axis is its `fit-content` size (not its `max-content` size) where
/// the box is horizontal or does not stretch its items.
pub fn finish_item(style: &mut ComputedValues, parent: &ComputedValues) {
    if !is_flex_box(parent) {
        return;
    }
    let flex = style.legacy_box.flex.max(0.0);
    style.flex_grow = flex;
    style.flex_shrink = flex;
    style.order = style.legacy_box.ordinal_group;
    style.align_self = ItemAlignment::Auto;
    let horizontal = parent.box_orient == BoxOrient::Horizontal;
    let (main_size, min_size) = if horizontal {
        (&style.width, &mut style.min_width)
    } else {
        (&style.height, &mut style.min_height)
    };
    let auto_main = *main_size == Size::Auto;
    if *min_size == Size::Auto {
        *min_size = Size::Length(crate::values::LengthPercentage::Px(0.0));
    }
    if style.flex_basis == Size::Auto
        && auto_main
        && (horizontal || parent.legacy_box.align != BoxAlign::Stretch)
    {
        style.flex_basis = Size::FitContent(None);
    }
}

#[cfg(test)]
mod tests {
    use crate::values::{FlexDirection, ItemAlignment, Size};
    use crate::{ComputedStyle, USER_AGENT_CSS, compute_styles};
    use fagus_css::{MediaEnvironment, Origin, RuleSet, Stylesheet};
    use fagus_dom::{Document, NodeId};

    /// The styles of the `<div>`s of `html` under `css`, in document order.
    fn styles(css: &str, html: &str) -> Vec<std::sync::Arc<ComputedStyle>> {
        let document = Document::parse_html(html);
        let ua = Stylesheet::parse(USER_AGENT_CSS, Origin::UserAgent, "ua.css");
        let author = Stylesheet::parse(css, Origin::Author, "test.css");
        let environment = MediaEnvironment::default();
        let rules = RuleSet::new(&[&ua, &author], &environment);
        let styled = compute_styles(&document, &rules, (environment.width, environment.height));
        document
            .descendants(NodeId::DOCUMENT)
            .filter(|&node| document.element(node).is_some_and(|data| data.is("div")))
            .filter_map(|node| styled.style(node).cloned())
            .collect()
    }

    #[test]
    fn a_webkit_box_maps_its_box_properties_to_flex_values() {
        let found = styles(
            ".b { display: -webkit-box; -webkit-box-pack: justify; -webkit-box-align: center;
                  -webkit-box-direction: reverse; justify-content: center; flex-wrap: wrap }
             .i { -webkit-box-flex: 2; flex-grow: 5; align-self: end; order: 7; -webkit-box-ordinal-group: 3 }
             .f { display: flex; -webkit-box-pack: end } .j { -webkit-box-flex: 3 }",
            "<div class=b><div class=i></div></div><div class=f><div class=j></div></div>",
        );
        let (container, item, flex, other) = (&found[0], &found[1], &found[2], &found[3]);
        assert_eq!(container.flex_direction, FlexDirection::RowReverse);
        assert_eq!(container.flex_wrap, crate::values::FlexWrap::NoWrap);
        assert_eq!(
            container.justify_content,
            crate::values::ContentAlignment::SpaceBetween
        );
        assert_eq!(container.align_items, ItemAlignment::Center);
        assert_eq!((item.flex_grow, item.flex_shrink), (2.0, 2.0));
        assert_eq!(item.align_self, ItemAlignment::Auto);
        assert_eq!(item.order, 3);
        assert_eq!(item.flex_basis, Size::FitContent(None));
        assert_eq!(
            item.min_width,
            Size::Length(crate::values::LengthPercentage::Px(0.0))
        );
        // A modern flex container ignores the `-webkit-box-*` properties.
        assert_eq!(flex.justify_content, crate::values::ContentAlignment::Normal);
        assert_eq!((other.flex_grow, other.flex_shrink), (0.0, 1.0));
    }
}

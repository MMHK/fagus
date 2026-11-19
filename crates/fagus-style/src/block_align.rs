//! `justify-self` and `justify-items` as block layout uses them ([`BlockJustify`]), and the
//! `flow-start` and `flow-end` positions of `align-content` and `justify-content`, which Chrome
//! knows and lightningcss does not.

use lightningcss::properties::align::{
    JustifyItems, JustifySelf, LegacyJustify, OverflowPosition, SelfPosition,
};
use lightningcss::properties::custom::UnparsedProperty;

use crate::style::ComputedValues;
use crate::values::{BlockJustify, ContentAlignment, JustifyPosition};

/// `safe` as `Some(true)`, `unsafe` as `Some(false)`.
fn safety(overflow: Option<&OverflowPosition>) -> Option<bool> {
    overflow.map(|overflow| *overflow == OverflowPosition::Safe)
}

fn self_position(value: SelfPosition) -> JustifyPosition {
    match value {
        SelfPosition::Center => JustifyPosition::Center,
        SelfPosition::Start | SelfPosition::FlexStart => JustifyPosition::Start,
        SelfPosition::End | SelfPosition::FlexEnd => JustifyPosition::End,
        SelfPosition::SelfStart => JustifyPosition::SelfStart,
        SelfPosition::SelfEnd => JustifyPosition::SelfEnd,
    }
}

/// A computed `justify-self`.
pub(crate) fn justify_self(value: &JustifySelf) -> BlockJustify {
    let (position, overflow) = match value {
        JustifySelf::Auto => (JustifyPosition::Auto, None),
        JustifySelf::Normal => (JustifyPosition::Normal, None),
        JustifySelf::Stretch => (JustifyPosition::Stretch, None),
        JustifySelf::BaselinePosition(_) => (JustifyPosition::Start, None),
        JustifySelf::SelfPosition { value, overflow } => (self_position(*value), overflow.as_ref()),
        JustifySelf::Left { overflow } => (JustifyPosition::Left, overflow.as_ref()),
        JustifySelf::Right { overflow } => (JustifyPosition::Right, overflow.as_ref()),
    };
    BlockJustify {
        safe: safety(overflow),
        ..BlockJustify::of(position)
    }
}

/// A computed `justify-items`.
pub(crate) fn justify_items(value: &JustifyItems) -> BlockJustify {
    let (position, overflow) = match value {
        JustifyItems::Normal => (JustifyPosition::Normal, None),
        JustifyItems::Stretch => (JustifyPosition::Stretch, None),
        JustifyItems::BaselinePosition(_) => (JustifyPosition::Start, None),
        JustifyItems::SelfPosition { value, overflow } => (self_position(*value), overflow.as_ref()),
        JustifyItems::Left { overflow } => (JustifyPosition::Left, overflow.as_ref()),
        JustifyItems::Right { overflow } => (JustifyPosition::Right, overflow.as_ref()),
        JustifyItems::Legacy(legacy) => {
            let position = match legacy {
                LegacyJustify::Left => JustifyPosition::Left,
                LegacyJustify::Right => JustifyPosition::Right,
                LegacyJustify::Center => JustifyPosition::Center,
            };
            return BlockJustify {
                legacy: true,
                ..BlockJustify::of(position)
            };
        }
    };
    BlockJustify {
        safe: safety(overflow),
        ..BlockJustify::of(position)
    }
}

/// Applies `align-content` or `justify-content` of `flow-start` or `flow-end` (with `safe` or
/// `unsafe` before it), which outside grid lanes are `flex-start` and `flex-end` in Chrome.
/// Returns whether the declaration was one of them.
pub(crate) fn apply_flow_content(unparsed: &UnparsedProperty<'_>, style: &mut ComputedValues) -> bool {
    let Some(position) = fagus_css::flow_position(unparsed) else {
        return false;
    };
    // As `flex-start` and `flex-end`, which reversed flex directions turn around; elsewhere they
    // are `start` and `end`.
    let value = if position.end {
        ContentAlignment::FlexEnd
    } else {
        ContentAlignment::FlexStart
    };
    if position.align {
        style.align_content = value;
        style.align_content_unsafe = position.safe == Some(false);
        style.align_content_safe = position.safe == Some(true);
    } else {
        style.justify_content = value;
        style.justify_content_safe = position.safe == Some(true);
    }
    true
}

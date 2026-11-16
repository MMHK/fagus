//! `justify-self` and `justify-items` on block-level boxes in block layout (CSS Box Alignment 3,
//! §6.1), as Chrome does them: a box whose `justify-self` is neither `normal` nor `stretch`
//! shrinks to fit (Blink's `AutoSizeBehavior::kFitContent`) and moves into the room beside it
//! (Blink's `WebkitTextAlignAndJustifySelfOffset`), unless an auto margin takes that room.

use fagus_style::ComputedStyle;
use fagus_style::values::{BlockJustify, JustifyPosition};

use crate::fragment::Fragment;
use crate::units::Au;

/// The `justify-self` of a box with style `child` in a block container with style `container`:
/// its own, or the container's `justify-items` for `auto`.
pub(crate) fn resolved(container: &ComputedStyle, child: &ComputedStyle) -> BlockJustify {
    if child.justify_self_block.position == JustifyPosition::Auto {
        container.justify_items_block
    } else {
        child.justify_self_block
    }
}

/// Whether a block-level box with style `child` in a block container with style `container`
/// shrinks to fit rather than filling the container.
pub(crate) fn shrinks(container: &ComputedStyle, child: &ComputedStyle) -> bool {
    !matches!(
        resolved(container, child).position,
        JustifyPosition::Auto | JustifyPosition::Normal | JustifyPosition::Stretch
    )
}

/// Whether a block-level box with style `child` in a block container with style `container` is
/// stretched explicitly (`justify-self: stretch`), which also stretches a form control.
pub(crate) fn stretches(container: &ComputedStyle, child: &ComputedStyle) -> bool {
    resolved(container, child).position == JustifyPosition::Stretch
}

/// How far `justify-self` moves a block-level child with style `child`, laid out as `fragment`,
/// across a containing block with style `container` that is `width` wide, from where its start
/// margin puts it (left to right: to the right; right to left: to the left, where its end is).
/// `None` where `justify-self` does not align the box, which `text-align` may then do. Without
/// `safe`, a box wider than the room overflows its start, as in Chrome.
pub(crate) fn offset(
    container: &ComputedStyle,
    child: &ComputedStyle,
    width: Au,
    fragment: &Fragment,
) -> Option<Au> {
    if child.margin.left.is_auto() || child.margin.right.is_auto() {
        return None;
    }
    let justify = resolved(container, child);
    let rtl = container.is_rtl();
    let basis = width.to_px();
    let margins =
        Au::from_px(child.margin.left.resolve_or(basis, 0.0) + child.margin.right.resolve_or(basis, 0.0));
    let mut free = width - fragment.rect.width - margins;
    if justify.safe == Some(true) {
        free = free.clamp_non_negative();
    }
    // Along the inline axis, from the start edge.
    let same_direction = child.is_rtl() == rtl;
    let along = match justify.position {
        JustifyPosition::Auto | JustifyPosition::Normal | JustifyPosition::Stretch => return None,
        JustifyPosition::Start => Au::ZERO,
        JustifyPosition::End => free,
        JustifyPosition::Center => free / 2,
        JustifyPosition::Left => {
            if rtl {
                free
            } else {
                Au::ZERO
            }
        }
        JustifyPosition::Right => {
            if rtl {
                Au::ZERO
            } else {
                free
            }
        }
        JustifyPosition::SelfStart => {
            if same_direction {
                Au::ZERO
            } else {
                free
            }
        }
        JustifyPosition::SelfEnd => {
            if same_direction {
                free
            } else {
                Au::ZERO
            }
        }
    };
    Some(if rtl { -along } else { along })
}

/// Where `justify-self` puts an absolutely positioned box with style `child` that was
/// block-level in a block container with style `container`, at its static position: how much
/// of the room left beside it in the container goes before it, from the inline start (Blink's
/// `InlineStaticPositionEdge`, whose alignment container is a line across the content box).
pub(crate) fn static_fraction(container: &ComputedStyle, child: &ComputedStyle) -> f32 {
    let rtl = container.is_rtl();
    let same_direction = child.is_rtl() == rtl;
    match resolved(container, child).position {
        JustifyPosition::Center => 0.5,
        JustifyPosition::End => 1.0,
        JustifyPosition::Left if rtl => 1.0,
        JustifyPosition::Right if !rtl => 1.0,
        JustifyPosition::SelfStart if !same_direction => 1.0,
        JustifyPosition::SelfEnd if same_direction => 1.0,
        _ => 0.0,
    }
}

//! Empty pieces of a containing block broken across pages, on pages where it has none of its own
//! but an absolutely positioned box inside it goes.
//!
//! Chrome places an absolutely positioned box in its whole containing block with the page areas
//! laid end to end, and it may land on a page before the first or after the last fragment of that
//! block (a negative `top`, or a `top` past its end). Blink then gives the containing block a
//! fragment there, without height, at the top of the page area, which holds the box: what the
//! block does to its content applies to it there too. A transform turns the box around that
//! fragment, whose origin is on that page (WPT `css-break/transform-024-print`, Chrome 157:
//! `rotate(180deg)` puts a box placed above its block's first page at the top of the first page,
//! one past the end of the block on the page after it).

use crate::fragment::{Fragment, FragmentKind, VisibleSides};
use crate::positioned::ChainPart;
use crate::units::{Au, Rect};

/// The fragment of a containing block for a page where it has none: a copy of `part` (its
/// fragment on another page, whose border box starts `origin.0` from the left of its page area),
/// without children, height or block-start and block-end borders, at the top of the page area,
/// whose content fragment lies at `content` in it. Its border box origin in the page area comes
/// with it. `None` where `part` is not a box's.
pub(crate) fn empty_piece(
    part: &Fragment,
    origin: (Au, Au),
    content: (Au, Au),
) -> Option<(Fragment, (Au, Au))> {
    let FragmentKind::Box { style, node, sides } = &part.kind else {
        return None;
    };
    let piece = Fragment {
        rect: Rect::new(origin.0 - content.0, -content.1, part.rect.width, Au::ZERO),
        kind: FragmentKind::Box {
            style: style.clone(),
            node: *node,
            sides: VisibleSides {
                top: false,
                bottom: false,
                ..*sides
            },
        },
        children: crate::fragment::Children::new(),
    };
    Some((piece, (origin.0, Au::ZERO)))
}

/// The part of `parts` (a containing block's fragments, by page) nearest to page `page`: the
/// last before it, else the first after it.
pub(crate) fn nearest(parts: &[ChainPart], page: usize) -> Option<&ChainPart> {
    parts
        .iter()
        .rev()
        .find(|part| part.page < page)
        .or_else(|| parts.iter().find(|part| part.page > page))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fragment::Children;

    fn px(value: f32) -> Au {
        Au::from_px(value)
    }

    #[test]
    fn puts_an_empty_piece_at_the_top_of_the_page_area() {
        let part = Fragment {
            rect: Rect::new(px(10.0), px(30.0), px(200.0), px(100.0)),
            kind: FragmentKind::Box {
                style: std::sync::Arc::new(fagus_style::ComputedStyle::default()),
                node: None,
                sides: VisibleSides::ALL,
            },
            children: Children::new(),
        };
        let (piece, origin) = empty_piece(&part, (px(10.0), px(30.0)), (px(0.0), px(5.0))).expect("piece");
        assert_eq!(piece.rect, Rect::new(px(10.0), px(-5.0), px(200.0), Au::ZERO));
        assert_eq!(origin, (px(10.0), Au::ZERO));
        let FragmentKind::Box { sides, .. } = piece.kind else {
            panic!("box")
        };
        assert!(!sides.top && !sides.bottom && sides.left && sides.right);
    }
}

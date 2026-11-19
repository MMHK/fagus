//! Absolutely positioned boxes painted by the stacking context around them.
//!
//! An absolutely positioned box goes in the fragment of its containing block (see
//! [`crate::positioned`]), and is painted with it. But a box that is a stacking context without
//! being a containing block (one with `opacity`, `mix-blend-mode`, `isolation`, `clip-path` or
//! a mask; not yet one with `filter`, see `positioned::collect`) paints the absolutely positioned boxes inside it whose containing block lies further out: in
//! Chrome, a box belongs to the stacking context around it in the document (Blink's paint
//! layers follow the layout tree), and so is painted in that box's group, at its opacity.
//!
//! Once a page is laid out, [`move_to_owners`] moves each such box into the fragment of that
//! stacking context, keeping its place on the page. A part of a box broken across columns goes
//! in the stacking context's fragment nearest to it among those inside its containing block's
//! fragment. Boxes stay where they are where a box in between clips its overflow, which clips
//! the stacking context but not them.

use std::sync::Arc;

use fagus_style::values::Overflow;
use rustc_hash::FxHashMap;

use crate::fragment::{Fragment, FragmentKind};
use crate::positioned::ChainKey;
use crate::units::Au;

/// The key of the box of `fragment`, if it is a box's.
fn key(fragment: &Fragment) -> Option<ChainKey> {
    match &fragment.kind {
        FragmentKind::Box { style, node, .. } => Some((*node, Arc::as_ptr(style) as usize)),
        _ => None,
    }
}

/// Whether `fragment` clips its overflow.
fn clips(fragment: &Fragment) -> bool {
    matches!(&fragment.kind, FragmentKind::Box { style, .. }
        if style.overflow_x != Overflow::Visible || style.overflow_y != Overflow::Visible)
}

/// Moves the absolutely positioned boxes below `fragment` that `owners` gives a stacking context
/// to paint them into that stacking context's fragment (see the module documentation).
pub(crate) fn move_to_owners(fragment: &mut Fragment, owners: &FxHashMap<ChainKey, ChainKey>) {
    if owners.is_empty() {
        return;
    }
    let mut index = 0;
    while index < fragment.children.len() {
        let child = &fragment.children[index];
        let target = key(child)
            .and_then(|own| owners.get(&own))
            .and_then(|&owner| nearest_owner(fragment, index, owner));
        match target {
            Some((path, (x, y))) => {
                let mut moved = fragment.children.remove(index);
                moved.rect.x -= x;
                moved.rect.y -= y;
                let owner = path
                    .iter()
                    .fold(&mut *fragment, |parent, &step| &mut parent.children[step]);
                owner.children.push(moved);
            }
            None => index += 1,
        }
    }
    for child in &mut fragment.children {
        move_to_owners(child, owners);
    }
}

/// The path (after removing child `skip`) and the origin, relative to `parent`, of the fragment
/// of `owner` below `parent` nearest to child `skip`, unless a box clips on the way.
fn nearest_owner(parent: &Fragment, skip: usize, owner: ChainKey) -> Option<(Vec<usize>, (Au, Au))> {
    let rect = parent.children[skip].rect;
    let mut found: Vec<(Vec<usize>, (Au, Au))> = Vec::new();
    for (index, child) in parent
        .children
        .iter()
        .enumerate()
        .filter(|&(index, _)| index != skip)
    {
        let step = if index > skip { index - 1 } else { index };
        find(
            child,
            &mut vec![step],
            (child.rect.x, child.rect.y),
            owner,
            &mut found,
        );
    }
    let distance = |&(x, y): &(Au, Au)| {
        let dx = (x - rect.x).0.unsigned_abs();
        let dy = (y - rect.y).0.unsigned_abs();
        u64::from(dx) + u64::from(dy)
    };
    found.into_iter().min_by_key(|(_, origin)| distance(origin))
}

/// Collects the fragments of `owner` at or below `fragment` (reached by `path`, at `origin`),
/// not inside or behind a box that clips its overflow.
fn find(
    fragment: &Fragment,
    path: &mut Vec<usize>,
    origin: (Au, Au),
    owner: ChainKey,
    found: &mut Vec<(Vec<usize>, (Au, Au))>,
) {
    if clips(fragment) {
        return;
    }
    if key(fragment) == Some(owner) {
        found.push((path.clone(), origin));
        return;
    }
    for (index, child) in fragment.children.iter().enumerate() {
        path.push(index);
        find(
            child,
            path,
            (origin.0 + child.rect.x, origin.1 + child.rect.y),
            owner,
            found,
        );
        path.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fragment::VisibleSides;
    use crate::units::Rect;
    use fagus_style::ComputedStyle;

    fn boxed(style: &Arc<ComputedStyle>, rect: Rect, children: Vec<Fragment>) -> Fragment {
        Fragment {
            rect,
            kind: FragmentKind::Box {
                style: Arc::clone(style),
                node: None,
                sides: VisibleSides::ALL,
            },
            children: children.into(),
        }
    }

    fn px(value: i32) -> Au {
        Au::from_px(value as f32)
    }

    #[test]
    fn moves_absolute_boxes_into_the_stacking_context_around_them() {
        let plain = Arc::new(ComputedStyle::default());
        let faded = Arc::new(ComputedStyle::from(fagus_style::ComputedValues {
            opacity: 0.5,
            ..fagus_style::ComputedValues::default()
        }));
        let absolute = Arc::new(ComputedStyle::default());
        let owner = boxed(&faded, Rect::new(px(10), px(20), px(100), px(0)), Vec::new());
        let wrapper = boxed(&plain, Rect::new(px(5), px(5), px(100), px(0)), vec![owner]);
        let positioned = boxed(&absolute, Rect::new(px(15), px(25), px(50), px(50)), Vec::new());
        let mut root = boxed(
            &plain,
            Rect::new(px(0), px(0), px(200), px(200)),
            vec![wrapper, positioned],
        );
        let mut owners = FxHashMap::default();
        owners.insert(
            key(&root.children[1]).unwrap(),
            key(&root.children[0].children[0]).unwrap(),
        );
        move_to_owners(&mut root, &owners);
        assert_eq!(root.children.len(), 1);
        let owner = &root.children[0].children[0];
        assert_eq!(owner.children.len(), 1);
        // It keeps its place on the page: (15, 25) less the owner's origin (15, 25).
        assert_eq!(
            (owner.children[0].rect.x, owner.children[0].rect.y),
            (px(0), px(0))
        );
    }

    #[test]
    fn leaves_absolute_boxes_where_a_box_in_between_clips() {
        let plain = Arc::new(ComputedStyle::default());
        let clipping = Arc::new(ComputedStyle::from(fagus_style::ComputedValues {
            overflow_x: Overflow::Hidden,
            overflow_y: Overflow::Hidden,
            ..fagus_style::ComputedValues::default()
        }));
        let faded = Arc::new(ComputedStyle::from(fagus_style::ComputedValues {
            opacity: 0.5,
            ..fagus_style::ComputedValues::default()
        }));
        let absolute = Arc::new(ComputedStyle::default());
        let owner = boxed(&faded, Rect::new(px(0), px(0), px(10), px(10)), Vec::new());
        let wrapper = boxed(&clipping, Rect::new(px(0), px(0), px(10), px(10)), vec![owner]);
        let positioned = boxed(&absolute, Rect::new(px(0), px(0), px(50), px(50)), Vec::new());
        let mut root = boxed(
            &plain,
            Rect::new(px(0), px(0), px(200), px(200)),
            vec![wrapper, positioned],
        );
        let mut owners = FxHashMap::default();
        owners.insert(
            key(&root.children[1]).unwrap(),
            key(&root.children[0].children[0]).unwrap(),
        );
        move_to_owners(&mut root, &owners);
        assert_eq!(root.children.len(), 2);
    }
}

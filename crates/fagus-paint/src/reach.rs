//! How far the boxes of a page reach up and down with all they hold, to find what of a page's
//! content reaches onto the pages after it without walking all of it for each of those pages
//! (see `carried::Carrier`).

use fagus_layout::Fragment;
use fagus_layout::units::Au;
use rustc_hash::FxHashMap;

/// The extents of the boxes of a page's content: for each fragment with children, the top and
/// bottom of all of it and its descendants, relative to the top of its parent (a fragment
/// without children reaches as far as its own rectangle).
#[derive(Debug, Default)]
pub(crate) struct Extents(FxHashMap<*const Fragment, (Au, Au)>);

impl Extents {
    /// The extents of `fragment` and all it holds.
    pub(crate) fn of(fragment: &Fragment) -> Self {
        let mut extents = Self::default();
        extents.measure(fragment);
        extents
    }

    fn measure(&mut self, fragment: &Fragment) -> (Au, Au) {
        let own = (fragment.rect.y, fragment.rect.y + fragment.rect.height);
        if fragment.children.is_empty() {
            return own;
        }
        let extent = fragment.children.iter().fold(own, |(top, bottom), child| {
            let (child_top, child_bottom) = self.measure(child);
            (
                top.min(fragment.rect.y + child_top),
                bottom.max(fragment.rect.y + child_bottom),
            )
        });
        self.0.insert(std::ptr::from_ref(fragment), extent);
        extent
    }

    /// Whether `fragment` (lying `origin` below the top of the page) or something in it may lie
    /// between `from` and `to`: `false` only where nothing does.
    pub(crate) fn may_reach(&self, fragment: &Fragment, origin: Au, (from, to): (Au, Au)) -> bool {
        let (top, bottom) = self
            .0
            .get(&std::ptr::from_ref(fragment))
            .copied()
            .unwrap_or((fragment.rect.y, fragment.rect.y + fragment.rect.height));
        origin + top < to && origin + bottom > from
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fagus_layout::FragmentKind;
    use fagus_layout::units::Rect;

    fn fragment(y: f32, height: f32, children: Vec<Fragment>) -> Fragment {
        Fragment {
            rect: Rect::new(Au::ZERO, Au::from_px(y), Au::from_px(10.0), Au::from_px(height)),
            kind: FragmentKind::Anonymous,
            children: children.into(),
        }
    }

    #[test]
    fn extents_reach_as_far_as_the_deepest_descendant() {
        // A short box at 100px whose grandchild reaches from 150px to 550px of the page.
        let grandchild = fragment(40.0, 400.0, Vec::new());
        let child = fragment(10.0, 5.0, vec![grandchild]);
        let root = fragment(100.0, 20.0, vec![child]);
        let extents = Extents::of(&root);
        let px = Au::from_px;
        assert!(extents.may_reach(&root, Au::ZERO, (px(500.0), px(600.0))));
        assert!(!extents.may_reach(&root, Au::ZERO, (px(550.0), px(600.0))));
        assert!(!extents.may_reach(&root, Au::ZERO, (px(0.0), px(100.0))));
        let child = &root.children[0];
        assert!(extents.may_reach(child, px(100.0), (px(540.0), px(541.0))));
        assert!(!extents.may_reach(child, px(200.0), (px(0.0), px(210.0))));
    }
}

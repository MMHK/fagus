//! Fixed boxes inside a box that contains them. A transformed box, or one with a perspective or
//! a filter, layout or paint containment, or a `will-change` of one of these, is the containing
//! block of its fixed descendants (CSS Transforms 1 §3, Filter Effects 1 §5, CSS Containment 2
//! §3, CSS Will Change 1 §2.1): they are absolutely positioned in it, not repeated on every
//! page, and break across pages and columns like the absolutely positioned boxes there, as in
//! Chrome.

use std::sync::Arc;

use fagus_dom::Document;
use fagus_style::values::Position;
use fagus_style::{ComputedStyle, StyledDocument};

use crate::tree::BoxTree;

/// Whether a box with `style` is the containing block of its fixed descendants.
pub(crate) fn contains_fixed(style: &ComputedStyle) -> bool {
    style.has_transform()
        || style.perspective.is_some()
        || !style.filter.is_empty()
        || style.contains_out_of_flow()
}

impl BoxTree {
    /// Turns the fixed boxes inside a box that contains them into absolutely positioned boxes
    /// in it (see the module documentation); their containing block is found by
    /// [`contains_fixed`] rather than by position.
    pub(crate) fn contain_fixed(&mut self, document: &Document, styled: &StyledDocument) {
        let mut repeated = Vec::new();
        for id in std::mem::take(&mut self.fixed) {
            let contained = self.get(id).node.is_some_and(|node| {
                std::iter::successors(document.parent_element(node), |&parent| {
                    document.parent_element(parent)
                })
                .any(|parent| styled.style(parent).is_some_and(|style| contains_fixed(style)))
            });
            if !contained {
                repeated.push(id);
                continue;
            }
            let layout_box = self.box_mut(id);
            let mut style = ComputedStyle::clone(&layout_box.style);
            style.position = Position::Absolute;
            layout_box.style = Arc::new(style);
            self.contained_fixed.insert(id);
        }
        self.fixed = repeated;
    }
}

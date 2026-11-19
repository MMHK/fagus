//! `<details>` as Chrome shows it: its first `<summary>` child, with a disclosure triangle
//! (`details > summary:first-of-type` in the user agent style), and the rest of its content only
//! while it is `open`. Without a summary, Chrome shows one of its own, "Details" in the language
//! of its user interface; Fagus writes it in the language of the element, as form controls.

use std::sync::Arc;

use fagus_dom::NodeId;
use fagus_style::values::WhiteSpace;
use fagus_style::{ComputedStyle, ComputedValues};

use super::{BoxId, BoxKind, BoxTree, Builder, InlineItem, LayoutBox, anonymous_style, process_white_space};
use crate::fragment::SymbolShape;

/// What a `<details>` element shows.
#[derive(Clone, Copy, Debug)]
pub(super) struct Details {
    /// Whether it has `open`: whether its content shows.
    pub open: bool,
    /// Its first `<summary>` child, which always shows.
    pub summary: Option<NodeId>,
}

impl Details {
    /// Whether the child `child` of the element shows.
    pub fn shows(self, child: NodeId) -> bool {
        self.open || self.summary == Some(child)
    }
}

impl Builder<'_> {
    /// The [`Details`] of `element`, if it is a `<details>`.
    pub(super) fn details(&self, element: NodeId) -> Option<Details> {
        let data = self.document.element(element).filter(|data| data.is("details"))?;
        let summary = self.document.child_elements(element).find(|&child| {
            self.document
                .element(child)
                .is_some_and(|child| child.is("summary"))
        });
        Some(Details {
            open: data.attribute("open").is_some(),
            summary,
        })
    }

    /// The summary of a `<details>` with `style` that has none: a block with a disclosure
    /// triangle inside and the label in the element's language.
    pub(super) fn default_summary(&self, tree: &mut BoxTree, style: &ComputedStyle, open: bool) -> BoxId {
        let own = anonymous_style(style);
        let label = self.ui_locale(&own).details;
        let text = Arc::new(own.inherit());
        let marker = Arc::new(ComputedStyle::from(ComputedValues {
            white_space: WhiteSpace::PRE,
            ..ComputedValues::from(own.inherit())
        }));
        let symbol = if open { "\u{25BE}" } else { "\u{25B8}" };
        let items = vec![
            InlineItem::Marker {
                text: symbol.to_owned(),
                style: marker,
                symbol: Some(SymbolShape::Disclosure { open }),
                inside: true,
                image: None,
            },
            InlineItem::Text {
                text: label.to_owned(),
                style: text,
            },
        ];
        tree.push(LayoutBox {
            style: Arc::new(own),
            node: None,
            generated: true,
            kind: BoxKind::Inline {
                items: process_white_space(items),
            },
        })
    }
}

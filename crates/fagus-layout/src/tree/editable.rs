//! The line of an empty editing host: Chrome gives a block container that is the root of
//! `contenteditable` content a line even when it has none (Blink's `HasLineIfEmpty`).

use fagus_dom::{Document, NodeId};

use super::{Collected, InlineItem};

/// Whether `element` has `contenteditable` on, as an attribute value says (`None` for
/// `inherit`, an invalid value or none).
fn editable(document: &Document, element: NodeId) -> Option<bool> {
    let value = document.element(element)?.attribute("contenteditable")?;
    match value.to_ascii_lowercase().as_str() {
        "" | "true" | "plaintext-only" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// Whether `element` is an editing host whose parent is not editable.
pub(super) fn is_editing_root(document: &Document, element: NodeId) -> bool {
    if editable(document, element) != Some(true) {
        return false;
    }
    let mut ancestor = document.parent_element(element);
    while let Some(id) = ancestor {
        if let Some(on) = editable(document, id) {
            return !on;
        }
        ancestor = document.parent_element(id);
    }
    true
}

/// Whether collected content makes no line: only collapsible white space, floats and
/// absolutely positioned boxes.
pub(super) fn makes_no_line(collected: &[Collected]) -> bool {
    collected.iter().all(|item| match item {
        Collected::Inline(InlineItem::Text { text, .. }) => text
            .chars()
            .all(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}')),
        Collected::Inline(InlineItem::OutOfFlow(_) | InlineItem::Float(_)) => true,
        _ => false,
    })
}

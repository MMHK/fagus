//! The bounding boxes of the elements of an SVG document, which the transforms of elements
//! with `transform-box: fill-box` or `stroke-box` refer to (CSS Transforms 1, §6.1).

use rustc_hash::FxHashMap;

use fagus_text::FontDb;

/// The object bounding box and the stroke bounding box of an element, each `[x, y, width,
/// height]` in the user space of the element (without its own transform).
pub type Boxes = ([f32; 4], [f32; 4]);

/// The bounding boxes of the elements of the SVG document `markup` whose ids start with
/// `prefix`, by id; empty if the document cannot be read. An element whose own transform was
/// left out of `markup` is measured in its user space; the translation of a `<use>` by its `x`
/// and `y` counts.
pub fn bounding_boxes(markup: &[u8], fonts: &FontDb, prefix: &str) -> FxHashMap<String, Boxes> {
    let mut boxes = FxHashMap::default();
    if let Some(tree) = crate::svg::parse(markup, fonts) {
        collect(tree.root(), prefix, &mut boxes);
    }
    boxes
}

fn collect(group: &usvg::Group, prefix: &str, boxes: &mut FxHashMap<String, Boxes>) {
    for node in group.children() {
        if node.id().starts_with(prefix) && !boxes.contains_key(node.id()) {
            // What is left of a group's transform is the element's own (a `<use>` at `x`, `y`).
            let transform = match node {
                usvg::Node::Group(group) => group.transform(),
                _ => usvg::Transform::default(),
            };
            let rect = |rect: usvg::Rect| {
                let rect = rect.transform(transform).unwrap_or(rect);
                [rect.x(), rect.y(), rect.width(), rect.height()]
            };
            boxes.insert(
                node.id().to_owned(),
                (rect(node.bounding_box()), rect(node.stroke_bounding_box())),
            );
        }
        if let usvg::Node::Group(child) = node {
            collect(child, prefix, boxes);
        }
        node.subroots(|root| collect(root, prefix, boxes));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_marked_elements() {
        let fonts = FontDb::new();
        let markup = br##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200">
            <rect id="fagus-box-1" x="10" y="20" width="30" height="40" stroke="black" stroke-width="4"/>
            <g id="fagus-box-2"><circle cx="50" cy="50" r="10"/><rect x="100" y="0" width="5" height="5"/></g>
            <defs><rect id="r" width="10" height="10"/></defs>
            <use id="fagus-box-3" href="#r" x="7" y="9"/>
            <rect id="other" width="1" height="1"/>
        </svg>"##;
        let boxes = bounding_boxes(markup, &fonts, "fagus-box-");
        assert_eq!(boxes.len(), 3);
        assert_eq!(
            boxes["fagus-box-1"],
            ([10.0, 20.0, 30.0, 40.0], [8.0, 18.0, 34.0, 44.0])
        );
        assert_eq!(boxes["fagus-box-2"].0, [40.0, 0.0, 65.0, 60.0]);
        assert_eq!(boxes["fagus-box-3"].0, [7.0, 9.0, 10.0, 10.0]);
    }
}

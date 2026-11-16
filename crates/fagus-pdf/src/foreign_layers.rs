//! The layers of an inline `<svg>` with `<foreignObject>` elements: Chrome draws the HTML of
//! each in document order with the SVG's own shapes, so that a shape after a label covers it.
//! The SVG is drawn in slices, one before the first `<foreignObject>` and one after each, each
//! with the shapes outside it hidden, and the HTML goes between them.

/// The containers whose content is not drawn where it stands.
const RESOURCES: [&str; 11] = [
    "defs",
    "clipPath",
    "mask",
    "pattern",
    "marker",
    "symbol",
    "filter",
    "linearGradient",
    "radialGradient",
    "style",
    "title",
];

/// The elements that draw.
const GRAPHICS: [&str; 10] = [
    "path", "rect", "circle", "ellipse", "line", "polyline", "polygon", "text", "image", "use",
];

/// The SVG documents of the layers of the inline SVG `data` whose `<foreignObject>` groups (see
/// [`fagus_dom::FOREIGN_OBJECT`]) have the indices `order`, in the order the SVG draws them:
/// the first with the shapes before the first group, then one with those after each group, up
/// to the next. `None` where no shape follows a group (the HTML then goes over the SVG whole).
pub fn layers(data: &[u8], order: &[usize]) -> Option<Vec<Vec<u8>>> {
    let text = std::str::from_utf8(data).ok()?;
    let starts: Vec<usize> = order
        .iter()
        .map(|index| text.find(&format!("<g id=\"{}{index}\"", fagus_dom::FOREIGN_OBJECT)))
        .collect::<Option<_>>()?;
    if starts.is_empty() || starts.windows(2).any(|pair| pair[0] >= pair[1]) {
        return None;
    }
    // The shapes outside resources: where their start tag's name ends, and their layer.
    let mut shapes: Vec<(usize, usize)> = Vec::new();
    let mut resources = 0_usize;
    let mut rest = 0;
    while let Some(open) = text[rest..].find('<').map(|at| rest + at) {
        let after = &text[open + 1..];
        if after.starts_with("!--") {
            rest = text[open..].find("-->").map_or(text.len(), |end| open + end + 3);
            continue;
        }
        if after.starts_with("![CDATA[") {
            rest = text[open..].find("]]>").map_or(text.len(), |end| open + end + 3);
            continue;
        }
        let close = text[open..].find('>').map_or(text.len(), |end| open + end);
        let tag = &text[open + 1..close];
        rest = close;
        // The place of a `<foreignObject>`, which draws nothing.
        if tag.starts_with(&format!("g id=\"{}", fagus_dom::FOREIGN_OBJECT)) {
            rest = text[close..].find("</g>").map_or(text.len(), |end| close + end);
            continue;
        }
        if let Some(name) = tag.strip_prefix('/') {
            if RESOURCES.contains(&name.trim()) {
                resources = resources.saturating_sub(1);
            }
            continue;
        }
        let name_end = tag
            .find(|character: char| character.is_whitespace() || character == '/')
            .unwrap_or(tag.len());
        let name = &tag[..name_end];
        let self_closing = tag.ends_with('/');
        if RESOURCES.contains(&name) {
            if !self_closing {
                resources += 1;
            }
            continue;
        }
        if resources == 0 && GRAPHICS.contains(&name) {
            let layer = starts.partition_point(|&start| start < open);
            shapes.push((open + 1 + name_end, layer));
        }
    }
    if !shapes.iter().any(|(_, layer)| *layer > 0) {
        return None;
    }
    Some(
        (0..=starts.len())
            .map(|layer| {
                let mut out = String::with_capacity(text.len() + shapes.len() * 30);
                let mut done = 0;
                for &(at, own) in &shapes {
                    if own == layer {
                        continue;
                    }
                    out.push_str(&text[done..at]);
                    hide(&text[at..], &mut out, &mut done, at);
                }
                out.push_str(&text[done..]);
                out.into_bytes()
            })
            .collect(),
    )
}

/// Writes what makes the element whose start tag goes on with `tag` (after its name, at `at`
/// in the document) hidden: `visibility: hidden` at the end of its `style` attribute, where it
/// wins over what the attribute says before; `done` moves past what is written.
fn hide(tag: &str, out: &mut String, done: &mut usize, at: usize) {
    let end = tag.find('>').unwrap_or(tag.len());
    if let Some(style) = tag[..end].find(" style=\"") {
        let value = style + " style=\"".len();
        if let Some(quote) = tag[value..end].find('"') {
            out.push_str(&tag[..value + quote]);
            out.push_str(";visibility:hidden");
            *done = at + value + quote;
            return;
        }
    }
    out.push_str(" style=\"visibility:hidden\"");
    *done = at;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slices_the_shapes_around_foreign_objects() {
        let svg = "<svg><defs><rect id=\"r\"/></defs><rect fill=\"red\"/>\
                   <g id=\"fagus-foreign-object-7\"><rect fill=\"none\"></rect></g>\
                   <circle style=\"fill:blue\" r=\"5\"/></svg>";
        let layers = layers(svg.as_bytes(), &[7]).expect("a shape after the foreignObject");
        let [first, second] = layers.as_slice() else {
            panic!("two layers: {layers:?}");
        };
        let first = String::from_utf8_lossy(first);
        let second = String::from_utf8_lossy(second);
        // The first layer hides the circle, the second the rectangle before the group; the
        // definitions stay as they are.
        assert!(
            first.contains("<circle style=\"fill:blue;visibility:hidden\""),
            "{first}"
        );
        assert!(first.contains("<rect fill=\"red\"/>"), "{first}");
        assert!(
            second.contains("<rect style=\"visibility:hidden\" fill=\"red\"/>"),
            "{second}"
        );
        assert!(second.contains("<defs><rect id=\"r\"/></defs>"), "{second}");
        // Nothing after the group: no layers.
        assert!(
            layers_of(
                "<svg><rect/><g id=\"fagus-foreign-object-1\"><rect></rect></g></svg>",
                1
            )
            .is_none()
        );
    }

    fn layers_of(svg: &str, index: usize) -> Option<Vec<Vec<u8>>> {
        layers(svg.as_bytes(), &[index])
    }
}

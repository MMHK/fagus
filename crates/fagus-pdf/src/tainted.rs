//! Tainted filter primitives (Filter Effects 1, §16, "Privacy and Security Considerations"): a
//! color that refers to `currentcolor` (`flood-color`, `lighting-color`) taints the result of
//! its primitive and of the primitives that read it, and an `feDisplacementMap` whose
//! displacement map is tainted passes its input through unchanged, as in Chrome. usvg displaces
//! by any map, so the document is rewritten with such maps scaled by zero.

use usvg::roxmltree;

/// The inputs that a filter itself provides, which are not tainted.
const STANDARD_INPUTS: [&str; 6] = [
    "SourceGraphic",
    "SourceAlpha",
    "BackgroundImage",
    "BackgroundAlpha",
    "FillPaint",
    "StrokePaint",
];

/// The SVG document `text` with its `feDisplacementMap` primitives whose displacement maps are
/// tainted written with `scale="0"`; `None` if it has none.
pub(crate) fn rewrite(text: &str) -> Option<String> {
    if !text.contains("feDisplacementMap") || !text.to_ascii_lowercase().contains("currentcolor") {
        return None;
    }
    let options = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..roxmltree::ParsingOptions::default()
    };
    let document = roxmltree::Document::parse_with_options(text, options).ok()?;
    let mut starts = Vec::new();
    for filter in document
        .descendants()
        .filter(|node| node.tag_name().name() == "filter")
    {
        tainted_maps(filter, &mut starts);
    }
    if starts.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(text.len() + 16 * starts.len());
    let mut rest = 0;
    for start in starts {
        let Some(tag_end) = text[start..].find('>').map(|end| start + end) else {
            continue;
        };
        let mut tag = text[start..tag_end].to_owned();
        if let Some((span, _)) = crate::svg::attribute_at(&tag, "scale") {
            tag.replace_range(span, "");
        }
        let self_closing = tag.ends_with('/');
        if self_closing {
            tag.pop();
        }
        out.push_str(&text[rest..start]);
        out.push_str(&tag);
        out.push_str(" scale=\"0\"");
        if self_closing {
            out.push('/');
        }
        rest = tag_end;
    }
    out.push_str(&text[rest..]);
    Some(out)
}

/// Adds where the `feDisplacementMap` primitives of `filter` with tainted displacement maps
/// start to `starts`.
fn tainted_maps(filter: roxmltree::Node<'_, '_>, starts: &mut Vec<usize>) {
    let mut results: Vec<(&str, bool)> = Vec::new();
    let mut previous: Option<bool> = None;
    for primitive in filter.children().filter(roxmltree::Node::is_element) {
        let name = primitive.tag_name().name();
        // An input's taint: a result named before, the previous result, or the filter's own.
        let input = |attribute: &str| -> bool {
            match primitive.attribute(attribute).map(str::trim) {
                Some(standard) if STANDARD_INPUTS.contains(&standard) => false,
                Some(named) if !named.is_empty() => results
                    .iter()
                    .rev()
                    .find(|(result, _)| *result == named)
                    .map_or(previous.unwrap_or(false), |(_, tainted)| *tainted),
                _ => previous.unwrap_or(false),
            }
        };
        let tainted = match name {
            "feDisplacementMap" => {
                if input("in2") {
                    starts.push(primitive.range().start);
                }
                input("in")
            }
            "feMerge" => primitive
                .children()
                .filter(|node| node.tag_name().name() == "feMergeNode")
                .any(|node| match node.attribute("in").map(str::trim) {
                    Some(standard) if STANDARD_INPUTS.contains(&standard) => false,
                    Some(named) if !named.is_empty() => results
                        .iter()
                        .rev()
                        .find(|(result, _)| *result == named)
                        .is_some_and(|(_, tainted)| *tainted),
                    _ => previous.unwrap_or(false),
                }),
            "feFlood" => color_tainted(primitive, "flood-color"),
            "feTurbulence" | "feImage" => false,
            "feDropShadow" => color_tainted(primitive, "flood-color") || input("in"),
            "feDiffuseLighting" | "feSpecularLighting" => {
                color_tainted(primitive, "lighting-color") || input("in")
            }
            "feBlend" | "feComposite" => input("in") || input("in2"),
            _ => input("in"),
        };
        if let Some(result) = primitive.attribute("result").map(str::trim) {
            results.push((result, tainted));
        }
        previous = Some(tainted);
    }
}

/// Whether the color property `property` of `element`, as an attribute or in its `style`
/// attribute, refers to `currentcolor`.
fn color_tainted(element: roxmltree::Node<'_, '_>, property: &str) -> bool {
    let current = |value: &str| value.to_ascii_lowercase().contains("currentcolor");
    element.attribute(property).is_some_and(current)
        || element.attribute("style").is_some_and(|style| {
            style.split(';').any(|declaration| {
                declaration
                    .split_once(':')
                    .is_some_and(|(name, value)| name.trim().eq_ignore_ascii_case(property) && current(value))
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_inputs_through_tainted_displacement_maps() {
        let tainted = r#"<svg xmlns="http://www.w3.org/2000/svg"><filter id="f">
            <feFlood flood-color="currentcolor" style="color: lime"/>
            <feBlend/>
            <feDisplacementMap in="SourceGraphic" scale="100"/></filter></svg>"#;
        let rewritten = rewrite(tainted).expect("rewritten");
        assert!(rewritten.contains(r#"<feDisplacementMap in="SourceGraphic" scale="0"/>"#));
        // A result named and read later, through `style`.
        let named = r#"<svg xmlns="http://www.w3.org/2000/svg"><filter id="f">
            <feFlood style="flood-color: color-mix(in srgb, currentColor 50%, red)" result="a"/>
            <feFlood flood-color="lime"/>
            <feDisplacementMap in="SourceGraphic" in2="a" scale="9"></feDisplacementMap></filter></svg>"#;
        assert!(
            rewrite(named)
                .expect("rewritten")
                .contains(r#"in2="a" scale="0">"#)
        );
        // A map of colors that do not refer to `currentcolor` displaces.
        let untainted = r#"<svg xmlns="http://www.w3.org/2000/svg"><filter id="f">
            <feFlood flood-color="currentcolor" result="a"/>
            <feFlood flood-color="lime"/>
            <feDisplacementMap in="a" scale="100"/></filter></svg>"#;
        assert_eq!(rewrite(untainted), None);
    }
}

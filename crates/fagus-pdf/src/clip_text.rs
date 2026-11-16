//! Text in a `<clipPath>`: Chrome clips with such a clip path through a soft mask in which the
//! text is drawn as text, so that it stays extractable, in synthetic bold where its face lacks
//! the weight. usvg and krilla turn it into an outline that clips, without the bold. A clip path
//! with text is written as an alpha `<mask>` of its content instead (covering everything, its
//! content in its units), and the `clip-path` references to it become `mask` references.

use std::fmt::Write as _;

/// The document with each `<clipPath>` that holds text written as a `<mask>`, and the
/// references to it changed; `None` if it has none.
pub(crate) fn rewrite(data: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(data).ok()?;
    if !text.contains("<clipPath") || !text.contains("<text") {
        return None;
    }
    let mut out = String::with_capacity(text.len() + 256);
    let mut ids = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("<clipPath") {
        let Some(tag_end) = rest[start..].find('>').map(|end| start + end) else {
            break;
        };
        let tag = &rest[start + 1..tag_end];
        let close = rest[tag_end..].find("</clipPath>").map(|end| tag_end + end);
        let id = crate::svg::attribute(tag, "id");
        match (close, id) {
            (Some(close), Some(id))
                if !tag.ends_with('/')
                    && rest[tag_end..close].contains("<text")
                    && crate::svg::attribute(tag, "clip-path").is_none() =>
            {
                out.push_str(&rest[..start]);
                let units = crate::svg::attribute(tag, "clipPathUnits").unwrap_or("userSpaceOnUse");
                out.push_str("<mask");
                write_attribute(&mut out, "id", id);
                write_attribute(&mut out, "mask-type", "alpha");
                out.push_str(
                    " maskUnits=\"userSpaceOnUse\" x=\"-100000\" y=\"-100000\" width=\"200000\" height=\"200000\"",
                );
                write_attribute(&mut out, "maskContentUnits", units.trim());
                // Its other attributes (what its content inherits) go to a group around its
                // content, which is filled and not stroked.
                out.push_str("><g fill=\"#000\" stroke=\"none\"");
                let mut attributes = &tag["clipPath".len()..];
                while let Some((name, value, rest)) = crate::non_scaling::next_attribute(attributes) {
                    if !["id", "clipPathUnits", "fill", "stroke"].contains(&name) {
                        let _ = write!(out, " {name}={value}");
                    }
                    attributes = rest;
                }
                out.push('>');
                out.push_str(&geometry_only(&rest[tag_end + 1..close]));
                out.push_str("</g></mask>");
                ids.push(id.to_owned());
                rest = &rest[close + "</clipPath>".len()..];
            }
            _ => {
                out.push_str(&rest[..=tag_end]);
                rest = &rest[tag_end + 1..];
            }
        }
    }
    out.push_str(rest);
    if ids.is_empty() {
        return None;
    }
    Some(references(&out, &ids))
}

/// The painting properties that a clip path ignores: it clips with the geometry of its content,
/// however that is filled or stroked.
const PAINT: [&str; 6] = [
    "fill",
    "stroke",
    "fill-opacity",
    "stroke-opacity",
    "stroke-width",
    "opacity",
];

/// The content of a clip path without the painting properties of its elements (attributes and
/// declarations of `style` attributes), so that the group around it fills it.
fn geometry_only(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let Some(end) = rest.find('>') else {
            break;
        };
        let mut tag = rest[..end].to_owned();
        if !tag.starts_with("</") && !tag.starts_with("<!") && !tag.starts_with("<?") {
            for name in PAINT {
                while let Some((span, _)) = crate::svg::attribute_at(&tag, name) {
                    tag.replace_range(span, "");
                }
            }
            if let Some((_, value)) = crate::svg::attribute_at(&tag, "style") {
                let kept: Vec<&str> = tag[value.clone()]
                    .split(';')
                    .filter(|declaration| {
                        declaration.split_once(':').is_none_or(|(name, _)| {
                            !PAINT.contains(&name.trim().to_ascii_lowercase().as_str())
                        })
                    })
                    .collect();
                let kept = kept.join(";");
                tag.replace_range(value, &kept);
            }
        }
        out.push_str(&tag);
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// Writes an attribute with a value that came from a start tag (escaped already).
fn write_attribute(out: &mut String, name: &str, value: &str) {
    let quote = if value.contains('"') { '\'' } else { '"' };
    let _ = write!(out, " {name}={quote}{value}{quote}");
}

/// `text` with its `clip-path` references (attributes, declarations) to the elements `ids`
/// changed to `mask` references.
fn references(text: &str, ids: &[String]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("clip-path") {
        out.push_str(&rest[..start]);
        let after = &rest[start + "clip-path".len()..];
        let value = after
            .trim_start()
            .strip_prefix(['=', ':'])
            .map(|value| value.trim_start().trim_start_matches(['"', '\'']).trim_start());
        let names = value
            .and_then(|value| value.strip_prefix("url("))
            .map(|url| url.trim_start().trim_start_matches(['"', '\'']))
            .and_then(|url| url.strip_prefix('#'))
            .is_some_and(|url| {
                ids.iter().any(|id| {
                    url.strip_prefix(id.as_str())
                        .is_some_and(|end| end.trim_start().starts_with([')', '"', '\'']))
                })
            });
        out.push_str(if names { "mask" } else { "clip-path" });
        rest = after;
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_clip_paths_with_text_as_masks() {
        let svg = "<svg xmlns='http://www.w3.org/2000/svg'><style>.t { clip-path: url(\"#c\") }</style>\
                   <clipPath id='c' transform='scale(2)' font-family='Inter' fill='red'><text y='10'>Fagus</text></clipPath>\
                   <clipPath id='r'><rect width='5' height='5'/></clipPath>\
                   <rect clip-path='url(#c)' width='10' height='10'/><rect clip-path=\"url(#r)\" width='10' height='10'/></svg>";
        let rewritten = rewrite(svg.as_bytes()).expect("rewritten");
        assert_eq!(
            rewritten,
            "<svg xmlns='http://www.w3.org/2000/svg'><style>.t { mask: url(\"#c\") }</style>\
             <mask id=\"c\" mask-type=\"alpha\" maskUnits=\"userSpaceOnUse\" x=\"-100000\" y=\"-100000\" \
             width=\"200000\" height=\"200000\" maskContentUnits=\"userSpaceOnUse\">\
             <g fill=\"#000\" stroke=\"none\" transform='scale(2)' font-family='Inter'><text y='10'>Fagus</text></g></mask>\
             <clipPath id='r'><rect width='5' height='5'/></clipPath>\
             <rect mask='url(#c)' width='10' height='10'/><rect clip-path=\"url(#r)\" width='10' height='10'/></svg>"
        );
        // The content clips with its geometry, however it is painted.
        assert_eq!(
            geometry_only(
                "<text fill=\"none\" x=\"1\" style=\"stroke: red; font-size: 9px;stroke-width:10px\">A</text>"
            ),
            "<text x=\"1\" style=\" font-size: 9px\">A</text>"
        );
        // The text is drawn as text in the mask.
        let tree = usvg::Tree::from_data(rewritten.as_bytes(), &usvg::Options::default()).expect("svg");
        assert!(tree.root().has_children());
        assert_eq!(
            rewrite(b"<svg><clipPath id='r'><rect/></clipPath><text/></svg>"),
            None
        );
    }
}

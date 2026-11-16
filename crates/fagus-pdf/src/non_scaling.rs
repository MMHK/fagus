//! `vector-effect: non-scaling-stroke`, which usvg ignores: Chrome strokes such a shape in the
//! coordinates of the SVG's box, as wide as `stroke-width` says in CSS pixels however the SVG
//! and its groups scale it, and prints its outline transformed into those coordinates. Here the
//! document is parsed once to learn where each such shape lands, and the shape is written again
//! as a path in those coordinates, under a transform that undoes its groups', so that its
//! stroke is not scaled.

use std::fmt::Write as _;

use usvg::Tree;

/// The shapes that `vector-effect` applies to.
const SHAPES: [&str; 7] = ["rect", "circle", "ellipse", "line", "polyline", "polygon", "path"];

/// The start of the ids given to shapes without one, before their number.
const ID: &str = "fagus-non-scaling-";

/// The document `data` with its shapes that have a non-scaling stroke written as paths in the
/// coordinates of the SVG as `parse` draws it, `natural` (its natural width) CSS pixels wide;
/// `None` if it has none that can be written so. A shape keeps its stroke where it is filled or stroked with
/// anything but a color (which would move with its coordinates), or where `<use>` draws it more
/// than once.
pub(crate) fn rewrite(
    data: &[u8],
    natural: Option<f32>,
    parse: impl Fn(&[u8]) -> Option<Tree>,
) -> Option<String> {
    let text = std::str::from_utf8(data).ok()?;
    if !text.contains("non-scaling-stroke") {
        return None;
    }
    let (marked, shapes) = mark(text);
    if shapes.is_empty() {
        return None;
    }
    let tree = parse(marked.as_bytes())?;
    let scale = natural
        .filter(|width| *width > 0.0)
        .map_or(1.0, |width| tree.size().width() / width);
    let mut found: Vec<(&str, Option<&usvg::Path>)> =
        shapes.iter().map(|shape| (shape.id.as_str(), None)).collect();
    let mut twice = Vec::new();
    find_paths(tree.root(), &mut found, &mut twice);
    let mut out = String::with_capacity(marked.len());
    let mut rest = 0;
    let mut changed = false;
    for (shape, (id, path)) in shapes.iter().zip(&found) {
        let Some(path) = path.filter(|_| !twice.contains(id)) else {
            continue;
        };
        let solid = |paint: &usvg::Paint| matches!(paint, usvg::Paint::Color(_));
        if path.stroke().is_none_or(|stroke| !solid(stroke.paint()))
            || path.fill().is_some_and(|fill| !solid(fill.paint()))
        {
            continue;
        }
        let transform = path.abs_transform().post_scale(1.0 / scale, 1.0 / scale);
        let Some(inverse) = transform.invert() else {
            continue;
        };
        let Some(outline) = path.data().clone().transform(transform) else {
            continue;
        };
        out.push_str(&marked[rest..shape.start]);
        write_path(&marked[shape.start..shape.end], &outline, inverse, &mut out);
        rest = shape.end;
        changed = true;
    }
    out.push_str(&marked[rest..]);
    changed.then_some(out)
}

/// A shape with a non-scaling stroke: where it stands in the marked document, from its start
/// tag to its end, and its id.
struct Shape {
    start: usize,
    end: usize,
    id: String,
}

/// The document with an id on each shape with a non-scaling stroke that has none, and those
/// shapes.
fn mark(text: &str) -> (String, Vec<Shape>) {
    let mut out = String::with_capacity(text.len() + 64);
    let mut shapes = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        rest = &rest[open..];
        // Comments, character data and declarations stay as they are.
        let skip = [("<!--", "-->"), ("<![CDATA[", "]]>"), ("<?", "?>"), ("<!", ">")]
            .iter()
            .find(|(start, _)| rest.starts_with(start))
            .map(|(_, end)| rest.find(end).map_or(rest.len(), |at| at + end.len()));
        if let Some(skip) = skip {
            out.push_str(&rest[..skip]);
            rest = &rest[skip..];
            continue;
        }
        let Some(tag_end) = tag_end(rest) else {
            break;
        };
        let tag = &rest[..=tag_end];
        let name_end = tag[1..]
            .find(|c: char| c.is_whitespace() || c == '/' || c == '>')
            .map_or(tag.len(), |at| at + 1);
        let name = &tag[1..name_end];
        if !SHAPES.contains(&name) || !non_scaling(tag) {
            out.push_str(tag);
            rest = &rest[tag.len()..];
            continue;
        }
        // The element ends with its start tag, or at its end tag (shapes hold no shapes).
        let element_end = if tag.ends_with("/>") {
            tag.len()
        } else {
            let close = format!("</{name}");
            match rest.find(&close) {
                Some(at) => rest[at..].find('>').map_or(rest.len(), |end| at + end + 1),
                None => tag.len(),
            }
        };
        let start = out.len();
        let id = if let Some(id) = crate::svg::attribute(&tag[1..], "id") {
            out.push_str(&rest[..element_end]);
            id.to_owned()
        } else {
            let id = format!("{ID}{}", shapes.len());
            out.push_str(&tag[..name_end]);
            let _ = write!(out, " id=\"{id}\"");
            out.push_str(&rest[name_end..element_end]);
            id
        };
        shapes.push(Shape {
            start,
            end: out.len(),
            id,
        });
        rest = &rest[element_end..];
    }
    out.push_str(rest);
    (out, shapes)
}

/// Where the start tag at the start of `text` ends (its `>`), outside quoted values.
fn tag_end(text: &str) -> Option<usize> {
    let mut quote = None;
    for (index, character) in text.char_indices() {
        match (quote, character) {
            (None, '"' | '\'') => quote = Some(character),
            (Some(open), _) if character == open => quote = None,
            (None, '>') => return Some(index),
            _ => {}
        }
    }
    None
}

/// Whether a start tag gives its element a non-scaling stroke, by attribute or `style`.
fn non_scaling(tag: &str) -> bool {
    let tag = &tag[1..];
    if crate::svg::attribute(tag, "vector-effect").is_some_and(|value| value.trim() == "non-scaling-stroke") {
        return true;
    }
    crate::svg::attribute(tag, "style").is_some_and(|style| {
        style.split(';').any(|declaration| {
            declaration.split_once(':').is_some_and(|(name, value)| {
                name.trim() == "vector-effect" && value.trim() == "non-scaling-stroke"
            })
        })
    })
}

/// Finds the paths with the ids of `found` in a group and those in it; ids met more than once
/// go into `twice`.
fn find_paths<'t>(
    group: &'t usvg::Group,
    found: &mut [(&str, Option<&'t usvg::Path>)],
    twice: &mut Vec<&'t str>,
) {
    for node in group.children() {
        match node {
            usvg::Node::Group(group) => find_paths(group, found, twice),
            usvg::Node::Path(path) if !path.id().is_empty() => {
                if let Some((_, slot)) = found.iter_mut().find(|(id, _)| *id == path.id()) {
                    if slot.is_some() {
                        twice.push(path.id());
                    }
                    *slot = Some(path);
                }
            }
            _ => {}
        }
    }
}

/// Writes the shape `element` as a `<path>` with the outline `outline`, under `transform`, with
/// the shape's other attributes.
fn write_path(
    element: &str,
    outline: &usvg::tiny_skia_path::Path,
    transform: usvg::Transform,
    out: &mut String,
) {
    const GEOMETRY: [&str; 15] = [
        "x", "y", "width", "height", "rx", "ry", "cx", "cy", "r", "x1", "y1", "x2", "y2", "points", "d",
    ];
    let Some(tag_end) = tag_end(element) else {
        out.push_str(element);
        return;
    };
    let tag = element[..tag_end].trim_end_matches('/');
    let name_end = tag[1..].find(char::is_whitespace).map_or(tag.len(), |at| at + 1);
    let mut attributes = &tag[name_end..];
    out.push_str("<path d=\"");
    for segment in outline.segments() {
        use usvg::tiny_skia_path::PathSegment;
        let _ = match segment {
            PathSegment::MoveTo(p) => write!(out, "M{} {}", p.x, p.y),
            PathSegment::LineTo(p) => write!(out, "L{} {}", p.x, p.y),
            PathSegment::QuadTo(a, p) => write!(out, "Q{} {} {} {}", a.x, a.y, p.x, p.y),
            PathSegment::CubicTo(a, b, p) => {
                write!(out, "C{} {} {} {} {} {}", a.x, a.y, b.x, b.y, p.x, p.y)
            }
            PathSegment::Close => write!(out, "Z"),
        };
    }
    out.push('"');
    let own = crate::svg::attribute(tag, "transform").unwrap_or_default();
    let _ = write!(
        out,
        " transform=\"{own} matrix({} {} {} {} {} {})\"",
        transform.sx, transform.ky, transform.kx, transform.sy, transform.tx, transform.ty
    );
    // The other attributes as they are.
    while let Some((name, value, rest)) = next_attribute(attributes) {
        if !GEOMETRY.contains(&name) && name != "transform" && name != "pathLength" {
            let _ = write!(out, " {name}={value}");
        }
        attributes = rest;
    }
    out.push_str("/>");
}

/// The first attribute of the attributes of a start tag: its name, its quoted value and what
/// follows it.
pub(crate) fn next_attribute(attributes: &str) -> Option<(&str, &str, &str)> {
    let attributes = attributes.trim_start();
    let equals = attributes.find('=')?;
    let name = attributes[..equals].trim();
    let value = attributes[equals + 1..].trim_start();
    let quote = value.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let end = value[1..].find(quote)? + 2;
    Some((name, &value[..end], &value[end..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strokes_non_scaling_shapes_in_the_svg_box() {
        fn walk<'t>(group: &'t usvg::Group, out: &mut Vec<&'t usvg::Path>) {
            for node in group.children() {
                match node {
                    usvg::Node::Group(group) => walk(group, out),
                    usvg::Node::Path(path) => out.push(path),
                    _ => {}
                }
            }
        }
        let svg = "<svg xmlns='http://www.w3.org/2000/svg' width='200' height='100' viewBox='0 0 100 50'>\
                   <g transform='scale(3 1)'><rect x='10' y='5' width='10' height='20' fill='none' stroke='red' \
                   stroke-width='2' vector-effect='non-scaling-stroke'/></g>\
                   <circle id='c' cx='50' cy='25' r='5' stroke='blue' style='vector-effect: non-scaling-stroke'></circle>\
                   <rect width='5' height='5' stroke='red'/></svg>";
        let parse = |data: &[u8]| Tree::from_data(data, &usvg::Options::default()).ok();
        let rewritten = rewrite(svg.as_bytes(), Some(200.0), parse).expect("rewritten");
        let tree = parse(rewritten.as_bytes()).expect("svg");
        let mut paths = Vec::new();
        walk(tree.root(), &mut paths);
        assert_eq!(paths.len(), 3);
        // The rectangle lies where it did, scaled 6 × 2 into the box, under no transform.
        let rect = paths[0];
        assert!(rect.abs_transform().is_identity(), "{:?}", rect.abs_transform());
        let bounds = rect.data().bounds();
        assert_eq!(
            (bounds.x(), bounds.y(), bounds.width(), bounds.height()),
            (60.0, 10.0, 60.0, 40.0)
        );
        assert_eq!(rect.stroke().map(|stroke| stroke.width().get()), Some(2.0));
        assert_eq!(paths[1].id(), "c");
        assert!(paths[1].abs_transform().is_identity());
        // A shape without the effect keeps its scale.
        assert_eq!(paths[2].abs_transform().sx, 2.0);
        assert_eq!(rewrite(b"<svg/>", None, parse), None);
    }

    #[test]
    fn strokes_at_the_size_an_image_is_drawn_at() {
        fn find(group: &usvg::Group, stroke: &mut Option<(f32, f32)>) {
            for node in group.children() {
                match node {
                    usvg::Node::Group(group) => find(group, stroke),
                    usvg::Node::Path(path) => {
                        *stroke = path
                            .stroke()
                            .map(|paint| (paint.width().get(), path.abs_transform().sx));
                    }
                    _ => {}
                }
            }
        }
        // An image 100px wide drawn 300px wide: the stroke is still 2px wide.
        let svg = "<svg xmlns='http://www.w3.org/2000/svg' width='100' height='50' viewBox='0 0 50 25'>\
                   <rect x='5' y='5' width='40' height='15' fill='none' stroke='red' stroke-width='2' \
                   vector-effect='non-scaling-stroke'/></svg>";
        let fonts = fagus_text::FontDb::new();
        let tree = crate::svg::parse_drawn(svg.as_bytes(), &fonts, 300.0).expect("svg");
        let mut stroke = None;
        find(tree.root(), &mut stroke);
        let (width, scale) = stroke.expect("a stroke");
        // Drawn 300px wide, the tree's 100px are 3px each: the stroke is two thirds of one.
        assert!((width * scale * 3.0 - 2.0).abs() < 1e-3, "{width} × {scale}");
        assert!(crate::svg::has_non_scaling_stroke(svg.as_bytes()));
    }
}

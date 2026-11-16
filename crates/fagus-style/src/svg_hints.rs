//! The presentation attributes of SVG elements that Fagus' own cascade reads (SVG 2, §6.6):
//! `transform`, `transform-origin` and `clip-path` with a basic shape, which CSS overrides (a
//! `transform` in a style sheet wins over the attribute), written out as CSS like the
//! presentational hints of HTML elements (see [`crate::hints`]). The other presentation
//! attributes are left to the SVG renderer.
//!
//! Values are read as Chrome reads them: the `transform` attribute with the grammar of SVG
//! transform lists (an error anywhere drops all of it), `transform-origin` as the CSS property
//! with numbers taken as pixels. Only values that were read are written.

use std::fmt::Write as _;

use fagus_dom::{Document, ElementData, NodeId};

/// The SVG elements that the `transform` presentation attribute applies to.
pub const TRANSFORMABLE: [&str; 16] = [
    "a",
    "circle",
    "clipPath",
    "ellipse",
    "foreignObject",
    "g",
    "image",
    "line",
    "path",
    "polygon",
    "polyline",
    "rect",
    "svg",
    "switch",
    "text",
    "use",
];

/// Writes the declarations of the presentation attributes of the SVG element `element`
/// (`data`) to `out`. The SVG elements in another one have the origin of their transform at
/// `0 0` (Chrome's user agent style sheet, `svg.css`); an outer `<svg>` is a CSS box.
pub(crate) fn hints(document: &Document, element: NodeId, data: &ElementData, out: &mut String) {
    let outer = data.is_svg("svg")
        && document
            .parent_element(element)
            .and_then(|parent| document.element(parent))
            .is_none_or(|parent| parent.name.ns != data.name.ns || parent.is_svg("foreignObject"));
    if outer {
        return;
    }
    out.push_str("transform-origin:0 0;");
    if let Some(origin) = data.attribute("transform-origin").and_then(transform_origin) {
        let _ = write!(out, "transform-origin:{origin};");
    }
    if !TRANSFORMABLE.contains(&&*data.name.local) {
        return;
    }
    if let Some(transform) = data.attribute("transform") {
        let _ = write!(
            out,
            "transform:{};",
            transform_list(transform).unwrap_or_default()
        );
    }
    // Filter functions and filters of other documents, which CSS resolves (the SVG renderer
    // misreads the colors of `drop-shadow()` and finds no other documents); it takes the
    // elements of its own (`url(#f)`).
    if let Some(filter) = data.attribute("filter").map(str::trim).filter(|filter| {
        let outside = outside_urls(filter);
        (!outside.trim().is_empty() || (filter.contains("url(") && !only_fragments(filter)))
            && !outside.contains([';', '{', '}', '!', '\\'])
    }) {
        let _ = write!(out, "filter:{filter};");
    }
    // A basic shape (`clip-path="circle(50%) fill-box"`); the SVG renderer takes `url()`s.
    if let Some(clip) = data.attribute("clip-path").map(str::trim).filter(|clip| {
        !clip.is_empty()
            && !clip
                .get(..4)
                .is_some_and(|start| start.eq_ignore_ascii_case("url("))
            && !clip.contains([';', '{', '}', '!', '\\', '"', '\''])
    }) {
        let _ = write!(out, "clip-path:{clip};");
    }
}

/// Whether all `url(…)`s of `value` name elements of the document (`url(#f)`).
fn only_fragments(value: &str) -> bool {
    value.match_indices("url(").all(|(at, _)| {
        value[at + 4..]
            .trim_start()
            .trim_start_matches(['"', '\''])
            .starts_with('#')
    })
}

/// `value` without the insides of its `url(…)`s (which hold `data:` URLs with `;`).
fn outside_urls(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find("url(") {
        out.push_str(&rest[..start]);
        match rest[start..].find(')') {
            Some(end) => rest = &rest[start + end + 1..],
            None => return out + &rest[start..],
        }
    }
    out.push_str(rest);
    out
}

/// The CSS of a `transform` attribute (an SVG transform list); `None` if it is not one, which
/// Chrome takes as no transform. An empty list is `none`.
pub fn transform_list(value: &str) -> Option<String> {
    let mut css = String::new();
    let mut rest = value.trim_start_matches(is_space);
    while !rest.is_empty() {
        let name_end = rest.find(|c: char| !c.is_ascii_alphabetic())?;
        let name = &rest[..name_end];
        rest = rest[name_end..].trim_start_matches(is_space).strip_prefix('(')?;
        let close = rest.find(')')?;
        let numbers = numbers(&rest[..close])?;
        // Transforms are separated by spaces, a comma or neither; a trailing comma is an error.
        rest = rest[close + 1..].trim_start_matches(is_space);
        if let Some(after) = rest.strip_prefix(',') {
            rest = after.trim_start_matches(is_space);
            if rest.is_empty() {
                return None;
            }
        }
        let n = |at: usize| numbers[at];
        if !css.is_empty() {
            css.push(' ');
        }
        match (name, numbers.len()) {
            ("matrix", 6) => {
                let _ = write!(
                    css,
                    "matrix({},{},{},{},{},{})",
                    n(0),
                    n(1),
                    n(2),
                    n(3),
                    n(4),
                    n(5)
                );
            }
            ("translate", 1) => {
                let _ = write!(css, "translate({}px)", n(0));
            }
            ("translate", 2) => {
                let _ = write!(css, "translate({}px,{}px)", n(0), n(1));
            }
            ("scale", 1) => {
                let _ = write!(css, "scale({})", n(0));
            }
            ("scale", 2) => {
                let _ = write!(css, "scale({},{})", n(0), n(1));
            }
            ("rotate", 1) => {
                let _ = write!(css, "rotate({}deg)", n(0));
            }
            ("rotate", 3) => {
                let _ = write!(
                    css,
                    "translate({}px,{}px) rotate({}deg) translate({}px,{}px)",
                    n(1),
                    n(2),
                    n(0),
                    -n(1),
                    -n(2)
                );
            }
            ("skewX", 1) => {
                let _ = write!(css, "skewX({}deg)", n(0));
            }
            ("skewY", 1) => {
                let _ = write!(css, "skewY({}deg)", n(0));
            }
            _ => return None,
        }
    }
    Some(if css.is_empty() { "none".to_owned() } else { css })
}

/// The numbers of the arguments of a transform, separated by spaces, a comma or both; `None`
/// if they are not numbers.
fn numbers(arguments: &str) -> Option<Vec<f64>> {
    let mut numbers = Vec::new();
    let mut rest = arguments.trim_matches(is_space);
    while !rest.is_empty() {
        let (number, after) = number(rest)?;
        numbers.push(number);
        let trimmed = after.trim_start_matches(is_space);
        rest = match trimmed.strip_prefix(',') {
            Some(after_comma) => {
                let after_comma = after_comma.trim_start_matches(is_space);
                if after_comma.is_empty() {
                    return None;
                }
                after_comma
            }
            None => trimmed,
        };
    }
    Some(numbers)
}

/// The number at the start of `text` (SVG's number grammar: a sign, digits with an optional
/// fraction and exponent) and what follows it.
fn number(text: &str) -> Option<(f64, &str)> {
    let bytes = text.as_bytes();
    let mut end = 0;
    if matches!(bytes.first(), Some(b'+' | b'-')) {
        end += 1;
    }
    let digits = |from: usize| {
        bytes[from..]
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count()
    };
    let integer = digits(end);
    end += integer;
    let mut fraction = 0;
    if bytes.get(end) == Some(&b'.') {
        fraction = digits(end + 1);
        if fraction > 0 {
            end += 1 + fraction;
        }
    }
    if integer == 0 && fraction == 0 {
        return None;
    }
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let mut exponent = end + 1;
        if matches!(bytes.get(exponent), Some(b'+' | b'-')) {
            exponent += 1;
        }
        let count = digits(exponent);
        if count > 0 {
            end = exponent + count;
        }
    }
    let number: f64 = text[..end].parse().ok()?;
    number.is_finite().then_some((number, &text[end..]))
}

fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c')
}

/// The CSS of a `transform-origin` attribute: its keywords, lengths and percentages, with
/// numbers in pixels; `None` if it has anything else (a third value is a length too).
fn transform_origin(value: &str) -> Option<String> {
    let mut css = Vec::new();
    for part in value.split(is_space).filter(|part| !part.is_empty()) {
        let lower = part.to_ascii_lowercase();
        if matches!(lower.as_str(), "left" | "right" | "top" | "bottom" | "center") {
            css.push(lower);
            continue;
        }
        let (number, unit) = number(part)?;
        let unit = unit.to_ascii_lowercase();
        let unit = match unit.as_str() {
            "" => "px",
            "px" | "em" | "rem" | "ex" | "ch" | "pt" | "pc" | "in" | "cm" | "mm" | "q" | "vw" | "vh"
            | "%" => &unit,
            _ => return None,
        };
        css.push(format!("{number}{unit}"));
    }
    (!css.is_empty() && css.len() <= 3).then(|| css.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_transform_lists_as_chrome_does() {
        assert_eq!(
            transform_list("rotate(90) translate(-75,-75)").as_deref(),
            Some("rotate(90deg) translate(-75px,-75px)")
        );
        assert_eq!(
            transform_list(" matrix(1 0 0 1 10 20),scale(2)  skewX(30)").as_deref(),
            Some("matrix(1,0,0,1,10,20) scale(2) skewX(30deg)")
        );
        assert_eq!(
            transform_list("rotate(45 10 20)").as_deref(),
            Some("translate(10px,20px) rotate(45deg) translate(-10px,-20px)")
        );
        assert_eq!(
            transform_list("translate(1e1 .5)").as_deref(),
            Some("translate(10px,0.5px)")
        );
        assert_eq!(transform_list("  ").as_deref(), Some("none"));
        // An error anywhere drops the whole list.
        assert_eq!(transform_list("rotate(90,)"), None);
        assert_eq!(transform_list("rotate(90deg)"), None);
        assert_eq!(transform_list("scale(1) foo(2)"), None);
        assert_eq!(transform_list("rotate(1, 2)"), None);
        assert_eq!(transform_list("scale(2),"), None);
    }

    #[test]
    fn reads_transform_origins_with_numbers_in_pixels() {
        assert_eq!(transform_origin("75").as_deref(), Some("75px"));
        assert_eq!(transform_origin("50% 10.5").as_deref(), Some("50% 10.5px"));
        assert_eq!(transform_origin("Left 2em").as_deref(), Some("left 2em"));
        assert_eq!(transform_origin("1;color:red"), None);
        assert_eq!(transform_origin("10 foo"), None);
        assert_eq!(transform_origin(""), None);
    }
}

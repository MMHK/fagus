//! The geometry properties of SVG elements (SVG 2, §9.1 "Geometry properties" and §13.2 for
//! `d`): `x`, `y`, `width`, `height`, `cx`, `cy`, `r`, `rx`, `ry` and `d`, which CSS may set
//! over the element's attributes (`rect { width: 100px; y: 10px }`). The SVG renderer reads them
//! as attributes only, so Fagus writes them as attributes.

use fagus_dom::{Document, NodeId};
use fagus_style::StyledDocument;
use fagus_style::values::{LengthPercentage, Size};

/// The elements that each geometry property applies to.
fn applies(property: &str, element: &str) -> bool {
    match property {
        "x" | "y" | "width" | "height" => matches!(element, "rect" | "image" | "use" | "svg"),
        "cx" | "cy" => matches!(element, "circle" | "ellipse"),
        "r" => element == "circle",
        "rx" | "ry" => matches!(element, "rect" | "ellipse"),
        "d" => element == "path",
        _ => false,
    }
}

/// The attributes that CSS sets for the SVG element `element`: its geometry properties, as SVG
/// attribute values. An outer `<svg>`, a CSS box, takes none.
pub(crate) fn attributes(
    document: &Document,
    styled: &StyledDocument,
    element: NodeId,
) -> Vec<(&'static str, String)> {
    let Some(data) = document
        .element(element)
        .filter(|data| data.namespace() == &fagus_dom::ns!(svg))
    else {
        return Vec::new();
    };
    let name = &*data.name.local;
    if !matches!(name, "rect" | "image" | "use" | "circle" | "ellipse" | "path") {
        return Vec::new();
    }
    let Some(style) = styled.style(element) else {
        return Vec::new();
    };
    let mut attributes = Vec::new();
    for (property, value) in style.svg_paint.iter() {
        if !fagus_style::SVG_GEOMETRY.contains(property) || !applies(property, name) {
            continue;
        }
        if let Some(value) = attribute_value(property, value) {
            attributes.push((*property, value));
        }
    }
    // `width` and `height` are sizes of CSS, as of boxes.
    for (property, size) in [("width", &style.width), ("height", &style.height)] {
        if applies(property, name)
            && let Size::Length(length) = size
            && let Some(value) = length_value(length)
        {
            attributes.push((property, value));
        }
    }
    attributes
}

/// The attribute value of the geometry property `property` declared as `value`: a length or
/// percentage (`auto` for `rx` and `ry`), or the path data of `path("…")` for `d`. `None` for a
/// value that the attribute does not take, such as a calculation.
fn attribute_value(property: &str, value: &str) -> Option<String> {
    let value = value.trim();
    if property == "d" {
        let inner = value.strip_prefix("path(")?.strip_suffix(')')?.trim();
        let data = inner
            .strip_prefix('"')
            .and_then(|inner| inner.strip_suffix('"'))
            .or_else(|| {
                inner
                    .strip_prefix('\'')
                    .and_then(|inner| inner.strip_suffix('\''))
            })?;
        return Some(data.to_owned());
    }
    if value.eq_ignore_ascii_case("auto") && matches!(property, "rx" | "ry") {
        return Some("auto".to_owned());
    }
    let number_end = value
        .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')))
        .unwrap_or(value.len());
    // `e` may start a unit (`em`, `ex`) rather than an exponent.
    let number_end = value[..number_end]
        .rfind(['e', 'E'])
        .filter(|&at| !value[at + 1..].starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '+'))
        .unwrap_or(number_end);
    value[..number_end].parse::<f32>().ok()?;
    let unit = &value[number_end..];
    let known = ["", "px", "%", "em", "ex", "in", "cm", "mm", "pt", "pc"];
    known
        .iter()
        .any(|known| unit.eq_ignore_ascii_case(known))
        .then(|| value.to_owned())
}

/// A length or percentage as an SVG attribute value; `None` for a calculation.
fn length_value(length: &LengthPercentage) -> Option<String> {
    match length {
        LengthPercentage::Px(pixels) if pixels.is_finite() => Some(pixels.to_string()),
        LengthPercentage::Percent(fraction) if fraction.is_finite() => Some(format!("{}%", fraction * 100.0)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_the_geometry_that_css_sets_as_attributes() {
        use fagus_css::{MediaEnvironment, Origin, RuleSet, Stylesheet};
        let css = r#"
            rect { x: 5px; y: 10%; width: 100px; height: 50%; rx: auto }
            circle { cx: 2em; r: 3px; x: 9px }
            path { d: path("M 0 0 L 10 10") }
            .calc { cx: calc(1px + 2px) }"#;
        let html =
            r#"<svg><rect id="r"/><circle id="c"/><path id="p"/><ellipse id="k" class="calc" rx="7"/></svg>"#;
        let document = Document::parse_html(html);
        let ua = Stylesheet::parse(fagus_style::USER_AGENT_CSS, Origin::UserAgent, "ua.css");
        let author = Stylesheet::parse(css, Origin::Author, "test.css");
        let rules = RuleSet::new(&[&ua, &author], &MediaEnvironment::default());
        let styled = fagus_style::compute_styles(&document, &rules, (500.0, 300.0));
        let find = |id: &str| document.svg_element(id).expect("an element");
        let mut rect = attributes(&document, &styled, find("r"));
        rect.sort();
        assert_eq!(
            rect,
            [
                ("height", "50%".to_owned()),
                ("rx", "auto".to_owned()),
                ("width", "100".to_owned()),
                ("x", "5px".to_owned()),
                ("y", "10%".to_owned()),
            ]
        );
        // `x` does not apply to circles.
        let mut circle = attributes(&document, &styled, find("c"));
        circle.sort();
        assert_eq!(circle, [("cx", "2em".to_owned()), ("r", "3px".to_owned())]);
        assert_eq!(
            attributes(&document, &styled, find("p")),
            [("d", "M 0 0 L 10 10".to_owned())]
        );
        // A calculation stays out; the element's own `rx` stays its own.
        assert_eq!(attributes(&document, &styled, find("k")), []);
        assert_eq!(attribute_value("x", "1e2px"), Some("1e2px".to_owned()));
        assert_eq!(attribute_value("x", "2em"), Some("2em".to_owned()));
    }
}

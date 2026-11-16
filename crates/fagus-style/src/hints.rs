//! Presentational hints (HTML, §15.2): attributes that style elements, such as `width`,
//! `align` and `bgcolor` on tables, `cellpadding` and `border` for the cells of a table, or
//! `color`, `face` and `size` on `font`. They count as author declarations of specificity zero
//! that come before all others, so any style sheet overrides them.
//!
//! Values are read the way Chrome reads them (lengths with the rules for dimension values, colors
//! with the rules for legacy color values) and written out as CSS that the cascade parses like a
//! `style` attribute. Only values that were read are written, so an attribute cannot add
//! declarations of its own.

use std::fmt::Write as _;

use fagus_dom::{Document, ElementData, NodeId};
use lightningcss::traits::Parse;
use lightningcss::values::color::CssColor;

/// The presentational hints of an element as CSS declarations; empty if it has none.
pub(crate) fn hints(document: &Document, element: NodeId) -> String {
    let mut css = String::new();
    let Some(data) = document.element(element) else {
        return css;
    };
    if &*data.name.ns == SVG {
        crate::svg_hints::hints(document, element, data, &mut css);
    }
    if data.is_svg("svg") {
        // The `width` and `height` of an `<svg>` are its CSS sizes (SVG 2, §8.2).
        for property in ["width", "height"] {
            if let Some(value) = data.attribute(property).and_then(svg_length) {
                declare(&mut css, property, &value);
            }
        }
        // `overflow` is a presentation attribute (SVG 2, §13.1), whose `auto` shows the
        // overflow as `visible` does.
        let overflow = data
            .attribute("overflow")
            .map(|value| value.trim().to_ascii_lowercase());
        match overflow.as_deref() {
            Some("visible" | "auto") => declare(&mut css, "overflow", "visible"),
            Some(value @ ("hidden" | "scroll" | "clip")) => declare(&mut css, "overflow", value),
            _ => {}
        }
        return css;
    }
    if &*data.name.ns == MATHML {
        mathml(data, &mut css);
        return css;
    }
    if !data.is_html() {
        return css;
    }
    let out = &mut css;
    match &*data.name.local {
        "table" => table(data, out),
        "td" | "th" => {
            if let Some(table) = parent_table(document, element) {
                cells_of(table, out);
            }
            cell(data, out);
        }
        "tr" | "thead" | "tbody" | "tfoot" => {
            table_part(data, out);
            if data.is("tr") {
                dimension(data, "height", "height", Units::Any, out);
            } else if parent_table(document, element).and_then(rules) == Some("groups") {
                // `rules=groups` rules off the row groups (and column groups).
                out.push_str("border-top:1px solid;border-bottom:1px solid;");
            }
        }
        "col" | "colgroup" => {
            dimension(data, "width", "width", Units::Any, out);
            if data.is("colgroup") && parent_table(document, element).and_then(rules) == Some("groups") {
                out.push_str("border-left:1px solid;border-right:1px solid;");
            }
        }
        "caption" => {
            if let Some(side) = keyword(data, "align", &["top", "bottom"]) {
                declare(out, "caption-side", side);
            }
        }
        "div" => text_align(data, out, true),
        "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => text_align(data, out, false),
        "img" => {
            image(data, out);
            image_size(data, out);
        }
        // An `<object>` or `<embed>` has the sizes and alignment of an image, but no ratio from
        // them.
        "object" | "embed" => {
            image(data, out);
            dimension(data, "width", "width", Units::Any, out);
            dimension(data, "height", "height", Units::Any, out);
        }
        // A `<video>` has the sizes of its attributes (`HTMLVideoElement`), no ratio from them.
        "video" => {
            dimension(data, "width", "width", Units::Any, out);
            dimension(data, "height", "height", Units::Any, out);
        }
        // An `<iframe>` has the sizes of its attributes, and no border where `frameborder` is
        // not a number other than 0 (`HTMLIFrameElement::CollectStyleForPresentationAttribute`).
        "iframe" => {
            dimension(data, "width", "width", Units::Any, out);
            dimension(data, "height", "height", Units::Any, out);
            if data
                .attribute("frameborder")
                .is_some_and(|value| value.trim().parse::<i64>().unwrap_or(0) == 0)
            {
                out.push_str("border-width:0;");
            }
        }
        "input"
            if data
                .attribute("type")
                .is_some_and(|kind| kind.trim().eq_ignore_ascii_case("image")) =>
        {
            image_size(data, out);
        }
        "hr" => rule(data, out),
        "font" => font(data, out),
        "html" if document.document_element() == Some(element) => {
            if let Some(schemes) = meta_color_scheme(document) {
                declare(out, "color-scheme", &schemes);
            }
        }
        "body" => {
            color(data, "bgcolor", "background-color", out);
            color(data, "text", "color", out);
            // Each of these sets the margins on both sides, as in browsers.
            for (attribute, sides) in [
                ("marginwidth", ["left", "right"]),
                ("leftmargin", ["left", "right"]),
                ("marginheight", ["top", "bottom"]),
                ("topmargin", ["top", "bottom"]),
            ] {
                for side in sides {
                    dimension(data, attribute, &format!("margin-{side}"), Units::Any, out);
                }
            }
        }
        "ul" => {
            if let Some(style) = keyword(data, "type", &["disc", "circle", "square", "none"]) {
                declare(out, "list-style-type", style);
            }
        }
        "ol" | "li" => {
            let numbered = data.attribute("type").and_then(|kind| match kind.trim() {
                "1" => Some("decimal"),
                "a" => Some("lower-alpha"),
                "A" => Some("upper-alpha"),
                "i" => Some("lower-roman"),
                "I" => Some("upper-roman"),
                _ => None,
            });
            let symbol =
                || keyword(data, "type", &["disc", "circle", "square", "none"]).filter(|_| data.is("li"));
            if let Some(style) = numbered.or_else(symbol) {
                declare(out, "list-style-type", style);
            }
        }
        "br" => {
            let clear = data.attribute("clear").map(str::trim).and_then(|value| {
                ["left", "right", "all", "both"]
                    .into_iter()
                    .find(|keyword| value.eq_ignore_ascii_case(keyword))
            });
            if let Some(clear) = clear {
                declare(out, "clear", if clear == "all" { "both" } else { clear });
            }
        }
        _ => {}
    }
    css
}

/// The namespace of `MathML`.
const MATHML: &str = "http://www.w3.org/1998/Math/MathML";
const SVG: &str = "http://www.w3.org/2000/svg";

/// The attributes of `MathML` elements that style them, as Chrome maps them
/// (`MathMLElement::CollectStyleForPresentationAttribute`): `dir`, `mathcolor`,
/// `mathbackground`, `mathsize`, `scriptlevel` and `displaystyle`.
fn mathml(data: &ElementData, out: &mut String) {
    if let Some(dir) = keyword(data, "dir", &["ltr", "rtl"]) {
        declare(out, "direction", dir);
    }
    // `mathsize` takes the lengths of CSS, but none of the keywords of `font-size`.
    if let Some(size) = data.attribute("mathsize").map(str::trim).filter(|size| {
        let lower = size.to_ascii_lowercase();
        !(lower == "medium"
            || lower.ends_with("large")
            || lower.ends_with("small")
            || lower == "smaller"
            || lower == "larger"
            || lower == "math")
            && size.starts_with(|c: char| c.is_ascii_digit() || c == '.' || c == '+')
            && size
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '%' | '+'))
    }) {
        declare(out, "font-size", size);
    }
    for (attribute, property) in [("mathbackground", "background-color"), ("mathcolor", "color")] {
        if let Some(value) = data.attribute(attribute).and_then(css_color) {
            declare(out, property, &value);
        }
    }
    // `scriptlevel`: a depth, or a change of it with a sign (`+1`, `-1`).
    if let Some(level) = data.attribute("scriptlevel") {
        let level = level.trim();
        let (sign, digits) = match level.as_bytes().first() {
            Some(b'+') => (Some(1), &level[1..]),
            Some(b'-') => (Some(-1), &level[1..]),
            _ => (None, level),
        };
        if !digits.is_empty()
            && digits.bytes().all(|byte| byte.is_ascii_digit())
            && let Ok(value) = digits.parse::<i32>()
        {
            match sign {
                Some(sign) => declare(out, "math-depth", &format!("add({})", sign * value)),
                None => declare(out, "math-depth", &value.to_string()),
            }
        }
    }
    if let Some(style) = data.attribute("displaystyle").map(str::trim) {
        if style.eq_ignore_ascii_case("true") {
            declare(out, "math-style", "normal");
        } else if style.eq_ignore_ascii_case("false") {
            declare(out, "math-style", "compact");
        }
    }
}

/// A CSS color of an attribute, written out as CSS; `None` if it is none.
fn css_color(value: &str) -> Option<String> {
    use lightningcss::traits::ToCss;
    let color = CssColor::parse_string(value.trim()).ok()?;
    color
        .to_css_string(lightningcss::printer::PrinterOptions::default())
        .ok()
}

/// The color schemes of the document's first `<meta name=color-scheme>` whose content is a
/// list of them (HTML, §4.2.5.4), which the root element takes as its `color-scheme` where no
/// style sheet sets one, as in Chrome: its words, lowercased, each of letters and hyphens.
fn meta_color_scheme(document: &Document) -> Option<String> {
    let content = document
        .descendants(NodeId::DOCUMENT)
        .filter_map(|node| document.element(node))
        .filter(|data| data.is("meta"))
        .filter(|data| {
            data.attribute("name")
                .is_some_and(|name| name.trim().eq_ignore_ascii_case("color-scheme"))
        })
        .find_map(|data| {
            let words: Vec<String> = data
                .attribute("content")?
                .split_ascii_whitespace()
                .map(str::to_ascii_lowercase)
                .collect();
            let valid = !words.is_empty()
                && words.iter().all(|word| {
                    word.chars()
                        .all(|character| character.is_ascii_lowercase() || character == '-')
                        && !word.starts_with('-')
                });
            valid.then(|| words.join(" "))
        })?;
    Some(content)
}

fn declare(out: &mut String, property: &str, value: &str) {
    let _ = write!(out, "{property}:{value};");
}

/// A length of an SVG attribute as CSS: a number with a unit, in pixels without one, or a
/// percentage; `None` for anything else.
fn svg_length(value: &str) -> Option<String> {
    let value = value.trim();
    let split = value
        .find(|c: char| c.is_ascii_alphabetic() || c == '%')
        .unwrap_or(value.len());
    let (number, unit) = value.split_at(split);
    let number: f32 = number
        .parse()
        .ok()
        .filter(|number: &f32| number.is_finite() && *number >= 0.0)?;
    let unit = unit.to_ascii_lowercase();
    let unit = match unit.as_str() {
        "" => "px",
        "px" | "em" | "rem" | "ex" | "ch" | "pt" | "pc" | "in" | "cm" | "mm" | "q" | "vw" | "vh" | "%" => {
            &unit
        }
        _ => return None,
    };
    Some(format!("{number}{unit}"))
}

/// `table`: its size, spacing, alignment, background and border.
fn table(data: &ElementData, out: &mut String) {
    dimension(data, "width", "width", Units::Any, out);
    dimension(data, "height", "height", Units::Any, out);
    dimension(data, "cellspacing", "border-spacing", Units::Pixels, out);
    color(data, "bgcolor", "background-color", out);
    match data.attribute("align").map(str::trim) {
        Some(align) if align.eq_ignore_ascii_case("center") => {
            declare(out, "margin-inline-start", "auto");
            declare(out, "margin-inline-end", "auto");
        }
        Some(align) if align.eq_ignore_ascii_case("left") => declare(out, "float", "left"),
        Some(align) if align.eq_ignore_ascii_case("right") => declare(out, "float", "right"),
        _ => {}
    }
    let border = border_width(data);
    let frame = frame(data);
    // `border` and `frame` both set the width of the border: the later attribute wins, as the
    // attributes are read in order in Chrome.
    let position = |name: &str| {
        data.attributes
            .iter()
            .position(|attribute| attribute.name.ns == fagus_dom::ns!() && &*attribute.name.local == name)
    };
    match (border, frame) {
        (Some(width), Some(_)) if position("border") > position("frame") => {
            let _ = write!(out, "border-width:{width}px;");
        }
        (_, Some(_)) => declare(out, "border-width", "1px"),
        (Some(width), None) => {
            let _ = write!(out, "border-width:{width}px;");
        }
        (None, None) => {}
    }
    let color_set = color(data, "bordercolor", "border-color", out);
    if let Some(sides) = frame {
        // `frame` draws the sides it names, solid, and hides the others.
        for (side, shown) in ["top", "right", "bottom", "left"].into_iter().zip(sides) {
            let _ = write!(
                out,
                "border-{side}-style:{};",
                if shown { "solid" } else { "hidden" }
            );
        }
    } else if border.is_some_and(|width| width > 0) {
        // `bordercolor` alone draws no border (Chrome 157, measured): the initial width would.
        declare(out, "border-style", if color_set { "solid" } else { "outset" });
    } else if rules(data).is_some() {
        declare(out, "border-style", "hidden");
    }
    // A valid `rules` collapses the borders, as in Chrome.
    if rules(data).is_some() {
        declare(out, "border-collapse", "collapse");
    }
}

/// The sides of a table that its `frame` draws (top, right, bottom, left), if it has a known
/// value.
fn frame(data: &ElementData) -> Option<[bool; 4]> {
    let value = data.attribute("frame")?.trim().to_ascii_lowercase();
    Some(match value.as_str() {
        "void" => [false; 4],
        "above" => [true, false, false, false],
        "below" => [false, false, true, false],
        "hsides" => [true, false, true, false],
        "lhs" => [false, false, false, true],
        "rhs" => [false, true, false, false],
        "vsides" => [false, true, false, true],
        "box" | "border" => [true; 4],
        _ => return None,
    })
}

/// The `border` of a table: a width in pixels, 1 for an empty or invalid value.
fn border_width(data: &ElementData) -> Option<u32> {
    data.attribute("border")
        .map(|value| non_negative_integer(value).unwrap_or(1))
}

/// The `rules` of a table, in lower case, if it has a known value.
fn rules(data: &ElementData) -> Option<&'static str> {
    let value = data.attribute("rules")?.trim();
    ["none", "groups", "rows", "cols", "all"]
        .into_iter()
        .find(|rules| value.eq_ignore_ascii_case(rules))
}

/// What a table gives its cells: their padding (`cellpadding`) and borders (`border`,
/// `bordercolor` and `rules`).
fn cells_of(table: &ElementData, out: &mut String) {
    if let Some(padding) = table.attribute("cellpadding") {
        // As in Chrome: an empty value is the default of one pixel, an invalid one zero.
        let padding = if padding.trim().is_empty() {
            1
        } else {
            leading_integer(padding).unwrap_or(0)
        };
        let _ = write!(out, "padding:{padding}px;");
    }
    let bordered = border_width(table).is_some_and(|width| width > 0);
    let colored = table
        .attribute("bordercolor")
        .is_some_and(|value| !value.trim().is_empty());
    let (sides, style): (&[&str], &str) = match rules(table) {
        Some("all") => (&["top", "right", "bottom", "left"], "solid"),
        Some("cols") => (&["left", "right"], "solid"),
        Some("rows") => (&["top", "bottom"], "solid"),
        Some(_) => (&[], ""),
        None if !bordered => (&[], ""),
        None if colored => (&["top", "right", "bottom", "left"], "solid"),
        None => (&["top", "right", "bottom", "left"], "inset"),
    };
    for side in sides {
        let _ = write!(out, "border-{side}-width:1px;border-{side}-style:{style};");
    }
    if !sides.is_empty() {
        declare(out, "border-color", "inherit");
    }
}

/// `td` and `th`: their size, wrapping, alignment and background.
fn cell(data: &ElementData, out: &mut String) {
    // Zero widths and heights are ignored, as in browsers.
    for (attribute, property) in [("width", "width"), ("height", "height")] {
        if data
            .attribute(attribute)
            .and_then(dimension_value)
            .is_some_and(|value| value.number > 0.0)
        {
            dimension(data, attribute, property, Units::Any, out);
        }
    }
    if data.attribute("nowrap").is_some() {
        declare(out, "white-space", "nowrap");
    }
    table_part(data, out);
}

/// The attributes of all parts of a table: background, alignment of the text and vertical
/// alignment.
fn table_part(data: &ElementData, out: &mut String) {
    color(data, "bgcolor", "background-color", out);
    text_align(data, out, true);
    if let Some(align) = keyword(data, "valign", &["top", "middle", "bottom", "baseline"]) {
        declare(out, "vertical-align", align);
    }
}

/// `align` as `text-align` (`middle` is `center`). For `div` and the parts of tables (`blocks`),
/// `left`, `center` and `right` align block-level children too, as Chrome's `-webkit-left`,
/// `-webkit-center` and `-webkit-right` do (HTML, §15.3.3).
fn text_align(data: &ElementData, out: &mut String, blocks: bool) {
    let Some(align) = keyword(data, "align", &["left", "right", "center", "middle", "justify"]) else {
        return;
    };
    let align = if align == "middle" { "center" } else { align };
    if blocks && align != "justify" {
        let _ = write!(out, "text-align:-webkit-{align};");
    } else {
        declare(out, "text-align", align);
    }
}

/// `img`: a float or a vertical alignment, a border and space around.
fn image(data: &ElementData, out: &mut String) {
    let align = data
        .attribute("align")
        .map(|value| value.trim().to_ascii_lowercase());
    let (float, vertical) = match align.as_deref() {
        Some("left") => (Some("left"), Some("top")),
        Some("right") => (Some("right"), Some("top")),
        Some("top") => (None, Some("top")),
        // As in Chrome: `middle` sets the middle of the image on the baseline.
        Some("middle") => (None, Some("-webkit-baseline-middle")),
        Some("absmiddle" | "abscenter" | "center") => (None, Some("middle")),
        Some("bottom" | "baseline") => (None, Some("baseline")),
        Some("absbottom") => (None, Some("bottom")),
        Some("texttop") => (None, Some("text-top")),
        _ => (None, None),
    };
    if let Some(float) = float {
        declare(out, "float", float);
    }
    if let Some(vertical) = vertical {
        declare(out, "vertical-align", vertical);
    }
    if let Some(width) = border_width(data) {
        let _ = write!(out, "border-width:{width}px;border-style:solid;");
    }
    dimension(data, "hspace", "margin-left", Units::Any, out);
    dimension(data, "hspace", "margin-right", Units::Any, out);
    dimension(data, "vspace", "margin-top", Units::Any, out);
    dimension(data, "vspace", "margin-bottom", Units::Any, out);
}

/// `width` and `height` of an `img` or `<input type=image>`: its size, and where both are
/// pixels, its ratio as `aspect-ratio: auto <width> / <height>`, as Chrome maps them
/// (`HTMLElement::ApplyAspectRatioToStyle`). A style that sets `height: auto` keeps the width and
/// the ratio: the image is as tall as its own ratio (or, broken, that of the attributes) says.
fn image_size(data: &ElementData, out: &mut String) {
    dimension(data, "width", "width", Units::Any, out);
    dimension(data, "height", "height", Units::Any, out);
    let pixels = |name: &str| {
        data.attribute(name)
            .and_then(dimension_value)
            .filter(|value| !value.percent && value.number > 0.0)
            .map(|value| value.number)
    };
    if let (Some(width), Some(height)) = (pixels("width"), pixels("height")) {
        let _ = write!(out, "aspect-ratio:auto {width} / {height};");
    }
}

/// `hr`: its alignment, width, thickness and color.
fn rule(data: &ElementData, out: &mut String) {
    match keyword(data, "align", &["left", "right", "center"]) {
        Some("left") => out.push_str("margin-left:0;margin-right:auto;"),
        Some("right") => out.push_str("margin-left:auto;margin-right:0;"),
        Some(_) => out.push_str("margin-left:auto;margin-right:auto;"),
        None => {}
    }
    dimension(data, "width", "width", Units::Any, out);
    if data.attribute("color").is_some() {
        if color(data, "color", "border-color", out) {
            color(data, "color", "background-color", out);
            declare(out, "border-style", "solid");
        }
    } else if data.attribute("noshade").is_some() {
        out.push_str("border-style:solid;border-color:#808080;background-color:#808080;");
    }
    if let Some(size) = data.attribute("size") {
        let size = leading_integer(size).unwrap_or(0);
        if size <= 1 {
            declare(out, "border-bottom-width", "0");
        } else {
            let _ = write!(out, "height:{}px;", size - 2);
        }
    }
}

/// `font`: color, family and one of the seven legacy sizes.
fn font(data: &ElementData, out: &mut String) {
    color(data, "color", "color", out);
    if let Some(face) = data.attribute("face") {
        let families: Vec<String> = face
            .split(',')
            .map(str::trim)
            .filter(|family| !family.is_empty())
            .map(|family| {
                // Generic families stay keywords, as in Chrome (`face=monospace` is `monospace`).
                let generic = [
                    "serif",
                    "sans-serif",
                    "monospace",
                    "cursive",
                    "fantasy",
                    "system-ui",
                ];
                if generic.iter().any(|generic| generic.eq_ignore_ascii_case(family)) {
                    return family.to_ascii_lowercase();
                }
                let escaped = family.replace('\\', "\\\\").replace('"', "\\\"");
                format!("\"{escaped}\"")
            })
            .collect();
        if !families.is_empty() {
            let _ = write!(out, "font-family:{};", families.join(","));
        }
    }
    if let Some(size) = data.attribute("size").and_then(legacy_font_size) {
        declare(out, "font-size", size);
    }
}

/// The CSS size of a `font` element's `size` by the rules for parsing a legacy font size (HTML): 1
/// to 7, or relative to 3 with a sign.
fn legacy_font_size(value: &str) -> Option<&'static str> {
    let value = value.trim_start();
    let (sign, digits) = match value.as_bytes().first()? {
        b'+' => (1, &value[1..]),
        b'-' => (-1, &value[1..]),
        _ => (0, value),
    };
    let number = i64::from(leading_integer(digits)?.min(1000));
    let size = match sign {
        0 => number,
        sign => 3 + sign * number,
    };
    Some(match size.clamp(1, 7) {
        1 => "x-small",
        2 => "small",
        3 => "medium",
        4 => "large",
        5 => "x-large",
        6 => "xx-large",
        _ => "xxx-large",
    })
}

/// The table a cell belongs to: its nearest `table` ancestor.
fn parent_table(document: &Document, element: NodeId) -> Option<&ElementData> {
    let mut node = document.parent_element(element);
    while let Some(parent) = node {
        let data = document.element(parent)?;
        if data.is("table") {
            return Some(data);
        }
        node = document.parent_element(parent);
    }
    None
}

/// The value of an attribute if it is one of `keywords` (ASCII case-insensitive), as written
/// in `keywords`.
fn keyword(data: &ElementData, attribute: &str, keywords: &[&'static str]) -> Option<&'static str> {
    let value = data.attribute(attribute)?.trim();
    keywords
        .iter()
        .find(|keyword| value.eq_ignore_ascii_case(keyword))
        .copied()
}

/// Which lengths an attribute takes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Units {
    Any,
    Pixels,
}

/// A number read by the rules for parsing dimension values, with whether it ends in `%`.
#[derive(Clone, Copy)]
struct Dimension {
    number: f32,
    percent: bool,
}

/// Declares `property` from a dimension attribute: pixels, or a percentage unless `units` says
/// pixels only.
fn dimension(data: &ElementData, attribute: &str, property: &str, units: Units, out: &mut String) {
    let Some(value) = data.attribute(attribute).and_then(dimension_value) else {
        return;
    };
    if value.percent {
        if units == Units::Any {
            let _ = write!(out, "{property}:{}%;", value.number);
        }
    } else {
        let _ = write!(out, "{property}:{}px;", value.number);
    }
}

/// The rules for parsing dimension values (HTML): digits, an optional fraction, and
/// a `%` for a percentage; whatever follows is ignored.
fn dimension_value(value: &str) -> Option<Dimension> {
    let value = value.trim_start_matches(|c: char| c.is_ascii_whitespace());
    let digits = value.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let mut end = digits;
    let rest = &value.as_bytes()[digits..];
    if rest.first() == Some(&b'.') {
        let fraction = rest[1..].iter().take_while(|byte| byte.is_ascii_digit()).count();
        if fraction > 0 {
            end += 1 + fraction;
        }
    }
    let number: f32 = value[..end].parse().ok()?;
    let number = number.min(1e6);
    Some(Dimension {
        number,
        percent: value.as_bytes().get(end) == Some(&b'%'),
    })
}

/// The rules for parsing non-negative integers (HTML): whitespace, an optional `+`,
/// digits; `None` for anything else.
fn non_negative_integer(value: &str) -> Option<u32> {
    let value = value.trim_start_matches(|c: char| c.is_ascii_whitespace());
    let value = value.strip_prefix('+').unwrap_or(value);
    leading_integer(value)
}

/// The digits a value starts with, as a number (capped), ignoring what follows.
fn leading_integer(value: &str) -> Option<u32> {
    let value = value.trim_start_matches(|c: char| c.is_ascii_whitespace());
    let digits = value.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    Some(value[..digits.min(9)].parse::<u32>().ok()?.min(100_000))
}

/// Declares `property` from a color attribute; whether the color was read.
fn color(data: &ElementData, attribute: &str, property: &str, out: &mut String) -> bool {
    match data.attribute(attribute).and_then(legacy_color) {
        Some([red, green, blue]) => {
            let _ = write!(out, "{property}:#{red:02x}{green:02x}{blue:02x};");
            true
        }
        None => false,
    }
}

/// The rules for parsing a legacy color value (HTML): a named color, `#rgb`, or hex
/// digits read leniently, so that `eeeeee` and even words that are no color give one.
fn legacy_color(value: &str) -> Option<[u8; 3]> {
    let value = value.trim_matches(|c: char| c.is_ascii_whitespace());
    if value.is_empty() || value.eq_ignore_ascii_case("transparent") {
        return None;
    }
    if value.bytes().all(|byte| byte.is_ascii_alphabetic())
        && let Ok(CssColor::RGBA(rgba)) = CssColor::parse_string(value)
    {
        return Some([rgba.red, rgba.green, rgba.blue]);
    }
    let bytes = value.as_bytes();
    if bytes.len() == 4 && bytes[0] == b'#' && bytes[1..].iter().all(u8::is_ascii_hexdigit) {
        let digit = |index: usize| {
            let digit = (bytes[index] as char).to_digit(16).unwrap_or(0) as u8;
            digit * 17
        };
        return Some([digit(1), digit(2), digit(3)]);
    }
    let mut digits: Vec<u8> = value
        .chars()
        .flat_map(|c| {
            // Characters outside the Basic Multilingual Plane count as two.
            let count = if u32::from(c) > 0xFFFF { 2 } else { 1 };
            std::iter::repeat_n(if c.is_ascii_hexdigit() { c as u8 } else { b'0' }, count)
        })
        .take(128)
        .collect();
    if value.starts_with('#') {
        digits.remove(0);
    }
    while digits.is_empty() || !digits.len().is_multiple_of(3) {
        digits.push(b'0');
    }
    let mut length = digits.len() / 3;
    let mut components: Vec<&[u8]> = digits.chunks(length).collect();
    if length > 8 {
        for component in &mut components {
            *component = &component[length - 8..];
        }
        length = 8;
    }
    while length > 2 && components.iter().all(|component| component[0] == b'0') {
        for component in &mut components {
            *component = &component[1..];
        }
        length -= 1;
    }
    let mut channels = [0; 3];
    for (channel, component) in channels.iter_mut().zip(&components) {
        let component = &component[..length.min(2)];
        *channel = component.iter().fold(0, |value, &digit| {
            value * 16 + (digit as char).to_digit(16).unwrap_or(0) as u8
        });
    }
    Some(channels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_legacy_colors() {
        assert_eq!(legacy_color("#eee"), Some([0xee, 0xee, 0xee]));
        assert_eq!(legacy_color("eeeeee"), Some([0xee, 0xee, 0xee]));
        assert_eq!(legacy_color(" #FF0000 "), Some([0xff, 0, 0]));
        assert_eq!(legacy_color("gray"), Some([128, 128, 128]));
        assert_eq!(legacy_color("transparent"), None);
        assert_eq!(legacy_color(""), None);
        // The famous ones: words that are no color still give one.
        assert_eq!(legacy_color("chucknorris"), Some([0xc0, 0, 0]));
        assert_eq!(legacy_color("crap"), Some([0xc0, 0xa0, 0]));
    }

    #[test]
    fn reads_dimensions_and_integers() {
        let read = |value: &str| dimension_value(value).map(|value| (value.number, value.percent));
        assert_eq!(read("100%"), Some((100.0, true)));
        assert_eq!(read(" 160px"), Some((160.0, false)));
        assert_eq!(read("50.5"), Some((50.5, false)));
        assert_eq!(read("12."), Some((12.0, false)));
        assert_eq!(read("*"), None);
        assert_eq!(read("-5"), None);
        assert_eq!(non_negative_integer("+3"), Some(3));
        assert_eq!(non_negative_integer("x"), None);
        assert_eq!(legacy_font_size("+2"), Some("x-large"));
        assert_eq!(legacy_font_size("9"), Some("xxx-large"));
        assert_eq!(legacy_font_size("-5"), Some("x-small"));
    }

    #[test]
    fn maps_the_size_attributes_of_images_to_sizes_and_a_ratio() {
        let css = |html: &str| {
            let document = Document::parse_html(html);
            let image = document
                .descendants(NodeId::DOCUMENT)
                .find(|&node| {
                    document
                        .element(node)
                        .is_some_and(|data| data.is("img") || data.is("input"))
                })
                .expect("image");
            hints(&document, image)
        };
        assert_eq!(
            css("<img width=100 height=50>"),
            "width:100px;height:50px;aspect-ratio:auto 100 / 50;"
        );
        // A percentage gives no ratio.
        assert_eq!(css("<img width=50% height=50>"), "width:50%;height:50px;");
        assert_eq!(
            css("<input type=image width=20 height=10>"),
            "width:20px;height:10px;aspect-ratio:auto 20 / 10;"
        );
        assert_eq!(css("<input width=20 height=10>"), "");
    }

    #[test]
    fn maps_the_size_and_frameborder_attributes_of_frames() {
        let css = |html: &str| {
            let document = Document::parse_html(html);
            let frame = document
                .descendants(NodeId::DOCUMENT)
                .find(|&node| document.element(node).is_some_and(|data| data.is("iframe")))
                .expect("frame");
            hints(&document, frame)
        };
        assert_eq!(
            css("<iframe width=100 height=50%></iframe>"),
            "width:100px;height:50%;"
        );
        // `frameborder` takes the border off where it is not a number other than 0, as Blink's
        // `ToInt` reads it.
        assert_eq!(css("<iframe frameborder=0></iframe>"), "border-width:0;");
        assert_eq!(css("<iframe frameborder=no></iframe>"), "border-width:0;");
        assert_eq!(css("<iframe frameborder=1></iframe>"), "");
    }

    #[test]
    fn gives_the_root_the_color_scheme_of_the_meta_element() {
        let root = |html: &str| {
            let document = Document::parse_html(html);
            hints(&document, document.document_element().expect("root"))
        };
        assert_eq!(
            root("<meta name=Color-Scheme content=' Dark  light'>"),
            "color-scheme:dark light;"
        );
        // The first one whose content is a list of schemes counts.
        assert_eq!(
            root("<meta name=color-scheme content='dark;x:y'><meta name=color-scheme content=only-x>"),
            "color-scheme:only-x;"
        );
        assert_eq!(root("<meta name=color-scheme content=''>"), "");
        assert_eq!(root("<p>"), "");
    }
}

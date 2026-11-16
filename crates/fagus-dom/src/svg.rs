//! Inline SVG: an `<svg>` element in an HTML document, written out as an SVG document of its own,
//! so that it can be drawn like an SVG image.

use std::fmt::Write as _;

use markup5ever::ns;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{Document, NodeId, NodeKind};

/// The most elements written for one inline SVG, the definitions it refers to included.
const MAX_ELEMENTS: usize = 100_000;

/// The presentation attributes that the content of an SVG element inherits from it.
const INHERITED: [&str; 24] = [
    "color",
    "fill",
    "fill-opacity",
    "fill-rule",
    "clip-rule",
    "stroke",
    "stroke-width",
    "stroke-opacity",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "stroke-dasharray",
    "stroke-dashoffset",
    "font-family",
    "font-size",
    "font-weight",
    "font-style",
    "font-variant",
    "font-stretch",
    "letter-spacing",
    "word-spacing",
    "text-anchor",
    "dominant-baseline",
    "visibility",
];

/// The start of the id of the group a `<foreignObject>` is written as, before the index of the
/// element.
pub const FOREIGN_OBJECT: &str = "fagus-foreign-object-";

/// The start of the id of the `<clipPath>` written for the basic shape that clips an element
/// (see [`Declared::clip`]), before the index of the element.
pub const CLIP: &str = "fagus-clip-";

/// The start of the id of the `<mask>` written for the CSS mask of an element (see
/// [`Declared::mask`]), before the index of the element.
pub const MASK: &str = "fagus-mask-";

/// What CSS makes of an SVG element, written into its markup (see [`Document::svg_document`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Declared {
    /// Declarations for its `style` attribute, after its own.
    pub style: String,
    /// Its transform as an SVG transform list (empty for none), which takes the place of its
    /// own `transform` and `transform-origin` (and of the transform declarations of its
    /// `style` attribute); `None` keeps them.
    pub transform: Option<String>,
    /// An id that takes the place of its own.
    pub id: Option<String>,
    /// The outline of a basic shape that clips it (`clip-path`, in its user space) as SVG path
    /// data, and whether it is filled by the even-odd rule: written as a `<clipPath>` before
    /// the element that takes the place of its own `clip-path`.
    pub clip: Option<(String, bool)>,
    /// The definitions of a `<mask>` (with the id [`MASK`] and the element's index) that CSS
    /// masks it with (`mask-image` and the other `mask-*` properties): written before the
    /// element, whose `mask` it takes the place of.
    pub mask: Option<String>,
    /// Attributes that take the place of its own of the same name: the geometry properties
    /// (`x`, `width`, `r`, `d`, …) that CSS sets.
    pub attributes: Vec<(&'static str, String)>,
}

impl From<String> for Declared {
    fn from(style: String) -> Self {
        Self {
            style,
            ..Self::default()
        }
    }
}

/// The properties of a `style` attribute that the declarations of the computed font replace
/// (the `font` shorthand) and, after it, those that [`Declared::transform`] replaces.
const REPLACED: [&str; 7] = [
    "font",
    "transform",
    "transform-origin",
    "transform-box",
    "translate",
    "rotate",
    "scale",
];

impl Document {
    /// The SVG document of the inline `<svg>` element `svg`: the element with its content, in the
    /// SVG namespace, and the elements elsewhere in the document that it refers to by id (the
    /// symbols of a sprite used with `<use href="#icon">`, a gradient in `fill="url(#g)"`) in a
    /// `<defs>` element. `inherited` are attributes for the root, such as the color for
    /// `currentColor`; the element's own attributes of the same name win. `styles` gives the
    /// declarations that CSS makes for an element, which go into its `style` attribute, after
    /// its own. Elements of other namespaces, scripts and comments are left out; a
    /// `<foreignObject>` becomes an empty group that marks its place (see
    /// [`FOREIGN_OBJECT`]).
    pub fn svg_document(
        &self,
        svg: NodeId,
        inherited: &[(&str, String)],
        styles: &dyn Fn(NodeId) -> Declared,
    ) -> String {
        let mut writer = Writer {
            document: self,
            out: String::new(),
            elements: 0,
            references: Vec::new(),
            styles,
        };
        let own: Vec<&str> = self
            .element(svg)
            .map(|data| {
                data.attributes
                    .iter()
                    .map(|attribute| &*attribute.name.local)
                    .collect()
            })
            .unwrap_or_default();
        writer.out.push_str(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"",
        );
        for (name, value) in inherited {
            if !own.contains(name) {
                writer.attribute(name, value);
            }
        }
        writer.attributes(svg);
        writer.out.push('>');
        writer.children(svg);
        // The definitions it refers to that lie outside it, and those they refer to in turn.
        let pending = std::mem::take(&mut writer.references);
        let defs = self.definitions(svg, pending, writer.elements, styles);
        if !defs.is_empty() {
            writer.out.push_str("<defs>");
            writer.out.push_str(&defs);
            writer.out.push_str("</defs>");
        }
        writer.out.push_str("</svg>");
        writer.out
    }

    /// The SVG element with the id `id`, such as a `<clipPath>`, `<mask>` or `<filter>` that
    /// CSS names with `url(#id)`, as markup for the `<defs>` of an SVG document of its own,
    /// followed by the elements elsewhere that it refers to; `None` if no SVG element has the
    /// id. The element keeps what it inherits as attributes of its own: the inherited
    /// presentation attributes of the SVG elements around it, the nearest first, then
    /// `inherited`, which gives what the element inherits from the HTML around them (as for
    /// [`Document::svg_document`]). `styles` gives declarations as for
    /// [`Document::svg_document`].
    pub fn svg_definition(
        &self,
        id: &str,
        inherited: &dyn Fn(NodeId) -> Vec<(&'static str, String)>,
        styles: &dyn Fn(NodeId) -> Declared,
    ) -> Option<String> {
        let target = *self.svg_ids().get(id)?;
        let mut writer = Writer {
            document: self,
            out: String::new(),
            elements: 0,
            references: Vec::new(),
            styles,
        };
        writer.element(target);
        // What it inherits goes into its start tag, after its name.
        let own: Vec<&str> = self
            .element(target)
            .map(|data| {
                data.attributes
                    .iter()
                    .map(|attribute| &*attribute.name.local)
                    .collect()
            })
            .unwrap_or_default();
        let mut extra: Vec<(&str, String)> = Vec::new();
        for ancestor in std::iter::successors(self.parent_element(target), |&node| self.parent_element(node))
        {
            let Some(data) = self.element(ancestor).filter(|data| data.name.ns == ns!(svg)) else {
                break;
            };
            for attribute in &data.attributes {
                let name = &*attribute.name.local;
                if attribute.name.ns == ns!()
                    && INHERITED.contains(&name)
                    && !own.contains(&name)
                    && !extra.iter().any(|(known, _)| *known == name)
                {
                    extra.push((name, String::from(&*attribute.value)));
                }
            }
        }
        for (name, value) in inherited(target) {
            if !own.contains(&name) && !extra.iter().any(|(known, _)| *known == name) {
                extra.push((name, value));
            }
        }
        if !extra.is_empty()
            && let Some(name_end) = writer.out.find(|c: char| c.is_whitespace() || c == '>')
        {
            let tail = writer.out.split_off(name_end);
            for (name, value) in extra {
                writer.attribute(name, &value);
            }
            writer.out.push_str(&tail);
        }
        let pending = std::mem::take(&mut writer.references);
        let defs = self.definitions(target, pending, writer.elements, styles);
        writer.out.push_str(&defs);
        Some(writer.out)
    }

    /// The markup of the elements that the ids `pending` name and that lie outside `within`,
    /// and of those they refer to in turn, each once; `elements` have been written before.
    fn definitions(
        &self,
        within: NodeId,
        mut pending: Vec<String>,
        elements: usize,
        styles: &dyn Fn(NodeId) -> Declared,
    ) -> String {
        let inside: FxHashSet<NodeId> = self.descendants(within).collect();
        let mut by_id: Option<FxHashMap<&str, NodeId>> = None;
        let mut written: FxHashSet<NodeId> = FxHashSet::default();
        let mut defs = Writer {
            document: self,
            out: String::new(),
            elements,
            references: Vec::new(),
            styles,
        };
        while let Some(id) = pending.pop() {
            let by_id = by_id.get_or_insert_with(|| self.svg_ids());
            let Some(&target) = by_id.get(id.as_str()) else {
                continue;
            };
            if inside.contains(&target)
                || written
                    .iter()
                    .any(|&done| done == target || self.descendants(done).any(|node| node == target))
            {
                continue;
            }
            written.insert(target);
            defs.element(target);
            pending.append(&mut defs.references);
        }
        defs.out
    }

    /// The `<foreignObject>` elements of the inline `<svg>` element `svg` by the index in their
    /// group's id (see [`FOREIGN_OBJECT`]).
    pub fn foreign_objects(&self, svg: NodeId) -> FxHashMap<usize, NodeId> {
        self.descendants(svg)
            .filter(|&node| {
                self.element(node)
                    .is_some_and(|data| data.is_svg("foreignObject"))
            })
            .map(|node| (node.index(), node))
            .collect()
    }

    /// The SVG element with the id `id`, the first if several have it.
    pub fn svg_element(&self, id: &str) -> Option<NodeId> {
        self.svg_ids().get(id).copied()
    }

    /// The SVG elements of the document by their id.
    fn svg_ids(&self) -> FxHashMap<&str, NodeId> {
        let mut ids = FxHashMap::default();
        for node in self.descendants(NodeId::DOCUMENT) {
            if let Some(data) = self.element(node)
                && data.name.ns == ns!(svg)
                && let Some(id) = data.id.as_deref()
            {
                ids.entry(id).or_insert(node);
            }
        }
        ids
    }
}

struct Writer<'a> {
    document: &'a Document,
    out: String,
    elements: usize,
    /// The ids that `href` attributes and `url(#…)` references point to.
    references: Vec<String>,
    styles: &'a dyn Fn(NodeId) -> Declared,
}

impl Writer<'_> {
    fn element(&mut self, node: NodeId) {
        let Some(data) = self.document.element(node) else {
            return;
        };
        if data.name.ns != ns!(svg) || &*data.name.local == "script" || self.elements >= MAX_ELEMENTS {
            return;
        }
        self.elements += 1;
        let name = &*data.name.local;
        if name == "foreignObject" {
            self.foreign_object(node);
            return;
        }
        let mut declared = (self.styles)(node);
        // The transform of a nested `<svg>` goes on a group around it: usvg applies one of its
        // own to its viewport's clip and again to its content, so that it took effect twice.
        let nested = name == "svg"
            && self
                .document
                .parent_element(node)
                .and_then(|parent| self.document.element(parent))
                .is_some_and(|parent| parent.name.ns == ns!(svg));
        let wrapper = nested
            .then(|| match &declared.transform {
                Some(transform) => Some(transform.clone()),
                None => data.attribute("transform").map(str::to_owned),
            })
            .flatten()
            .filter(|transform| !transform.trim().is_empty());
        if let Some(transform) = &wrapper {
            self.out.push_str("<g");
            // Its own transform goes with its own origin.
            if declared.transform.is_none()
                && let Some(origin) = data.attribute("transform-origin")
            {
                self.attribute("transform-origin", origin);
            }
            self.attribute("transform", transform);
            self.out.push('>');
            declared.transform = Some(String::new());
        }
        if let Some((d, even_odd)) = &declared.clip {
            let _ = write!(
                self.out,
                "<clipPath id=\"{CLIP}{}\" clipPathUnits=\"userSpaceOnUse\"><path clip-rule=\"{}\" d=\"",
                node.index(),
                if *even_odd { "evenodd" } else { "nonzero" }
            );
            escape(&mut self.out, d, true);
            self.out.push_str("\"></path></clipPath>");
        }
        if let Some(mask) = &declared.mask {
            self.out.push_str(mask);
        }
        // A shape whose geometry disables its rendering is not filtered either (the SVG
        // renderer would draw what its filter makes of nothing), as in Chrome.
        let filtered = data.attribute("filter").is_some()
            || data
                .attribute("style")
                .is_some_and(|style| style.contains("filter"))
            || declared.style.contains("filter");
        if filtered && disabled(name, data, &declared.attributes) {
            if !declared.style.is_empty() {
                declared.style.push_str("; ");
            }
            declared.style.push_str("filter: none");
        }
        let _ = write!(self.out, "<{name}");
        self.declared_attributes(node, &declared);
        self.out.push('>');
        self.children(node);
        let _ = write!(self.out, "</{name}>");
        if wrapper.is_some() {
            self.out.push_str("</g>");
        }
    }

    /// Writes a `<foreignObject>` as a group with the id [`FOREIGN_OBJECT`] and the element's
    /// index, which holds an invisible rectangle of its place and size: the HTML in it is laid
    /// out by the document and drawn where the SVG puts the group (see
    /// [`Document::foreign_objects`]). Its transform and the conditions of a `<switch>` stay
    /// with the group.
    fn foreign_object(&mut self, node: NodeId) {
        let Some(data) = self.document.element(node) else {
            return;
        };
        let _ = write!(self.out, "<g id=\"{FOREIGN_OBJECT}{}\"", node.index());
        let transform = (self.styles)(node).transform;
        if let Some(transform) = transform.as_deref().filter(|transform| !transform.is_empty()) {
            self.attribute("transform", transform);
        }
        let mut rect = String::new();
        for attribute in &data.attributes {
            let name = &*attribute.name.local;
            if attribute.name.ns != ns!() {
                continue;
            }
            match name {
                "transform" if transform.is_some() => {}
                // Its filter draws where the group is (what it makes of nothing, such as a
                // flood), as in Chrome.
                "transform" | "systemLanguage" | "requiredExtensions" | "display" | "visibility"
                | "filter" => {
                    self.attribute(name, &attribute.value);
                }
                "x" | "y" | "width" | "height" => {
                    let _ = write!(rect, " {name}=\"");
                    escape(&mut rect, &attribute.value, true);
                    rect.push('"');
                }
                _ => {}
            }
        }
        // Its style attribute wins over the SVG's style sheets, which may fill or stroke all
        // rectangles (as Mermaid's do).
        let _ = write!(
            self.out,
            "><rect{rect} style=\"fill: none; stroke: none; filter: none\"></rect></g>"
        );
    }

    fn children(&mut self, node: NodeId) {
        for child in self.document.children(node) {
            match &self.document.node(child).kind {
                NodeKind::Element(_) => self.element(child),
                NodeKind::Text(text) => escape(&mut self.out, text, false),
                _ => {}
            }
        }
    }

    fn attributes(&mut self, node: NodeId) {
        let declared = (self.styles)(node);
        self.declared_attributes(node, &declared);
    }

    /// Writes the attributes of `node` with what CSS declares for it (see [`Declared`]).
    fn declared_attributes(&mut self, node: NodeId, declared: &Declared) {
        let Some(data) = self.document.element(node) else {
            return;
        };
        let mut style = String::new();
        for attribute in &data.attributes {
            let name = &attribute.name;
            if name.ns == ns!() && &*name.local == "style" {
                let mut left_out = if declared.transform.is_some() {
                    REPLACED.to_vec()
                } else {
                    vec![REPLACED[0]]
                };
                if declared.clip.is_some() {
                    left_out.push("clip-path");
                }
                if declared.mask.is_some() {
                    left_out.push("mask");
                }
                style.push_str(&without_properties(&attribute.value, &left_out));
                continue;
            }
            if name.ns == ns!()
                && (declared.attributes.iter().any(|(set, _)| *set == &*name.local)
                    || (declared.transform.is_some()
                        && matches!(&*name.local, "transform" | "transform-origin"))
                    || (declared.id.is_some() && &*name.local == "id")
                    || (declared.clip.is_some() && &*name.local == "clip-path")
                    || (declared.mask.is_some() && &*name.local == "mask"))
            {
                continue;
            }
            let qualified = if name.ns == ns!() {
                (*name.local).to_owned()
            } else if name.ns == ns!(xlink) {
                format!("xlink:{}", &*name.local)
            } else if name.ns == ns!(xml) {
                format!("xml:{}", &*name.local)
            } else {
                // Namespace declarations are written on the root.
                continue;
            };
            // Chrome ignores `requiredFeatures` (a `<switch>` takes a `<foreignObject>`
            // behind `#Extensibility`, as draw.io writes it).
            if name.ns == ns!() && &*name.local == "requiredFeatures" {
                continue;
            }
            let value = &*attribute.value;
            if &*name.local == "href"
                && let Some(id) = value.trim().strip_prefix('#')
            {
                self.references.push(id.to_owned());
            }
            let mut rest = value;
            while let Some(start) = rest.find("url(") {
                rest = &rest[start + 4..];
                let inner = rest.trim_start().trim_start_matches(['"', '\'']);
                if let Some(id) = inner.strip_prefix('#') {
                    let end = id.find(|c: char| c == ')' || c == '"' || c == '\'' || c.is_whitespace());
                    self.references.push(id[..end.unwrap_or(id.len())].to_owned());
                }
            }
            self.attribute(&qualified, value);
        }
        for (name, value) in &declared.attributes {
            self.attribute(name, value);
        }
        if let Some(id) = &declared.id {
            self.attribute("id", id);
        }
        if declared.clip.is_some() {
            self.attribute("clip-path", &format!("url(#{CLIP}{})", node.index()));
        }
        if declared.mask.is_some() {
            self.attribute("mask", &format!("url(#{MASK}{})", node.index()));
        }
        if let Some(transform) = declared
            .transform
            .as_deref()
            .filter(|transform| !transform.is_empty())
        {
            self.attribute("transform", transform);
        }
        if !declared.style.is_empty() {
            if !style.trim().is_empty() {
                style.push(';');
            }
            style.push_str(&declared.style);
        }
        if !style.is_empty() {
            self.attribute("style", &style);
        }
    }

    fn attribute(&mut self, name: &str, value: &str) {
        let _ = write!(self.out, " {name}=\"");
        escape(&mut self.out, value, true);
        self.out.push('"');
    }
}

/// Escapes text for XML content or a quoted attribute value.
fn escape(out: &mut String, text: &str, attribute: bool) {
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attribute => out.push_str("&quot;"),
            _ => out.push(character),
        }
    }
}

/// The declarations of a `style` attribute without those of the properties `left_out`: the `font`
/// shorthand, which the declarations of the computed font replace (see
/// [`Document::svg_document`]; the SVG renderer drops the fonts of shorthands with a line
/// height, `font: 20px/1 Ahem`, which CSS takes), and the transform properties that
/// [`Declared::transform`] replaces.
/// Whether the geometry of the SVG shape `name` (`data`, with the attributes `set` that CSS
/// sets over its own) disables its rendering (SVG 2, §10): a rectangle, an ellipse or an image
/// of no width or height, a circle of no radius, a path without data, or a polygon or a
/// polyline without points.
fn disabled(name: &str, data: &crate::ElementData, set: &[(&'static str, String)]) -> bool {
    let value = |attribute: &str| {
        set.iter()
            .find(|(name, _)| *name == attribute)
            .map(|(_, value)| value.as_str())
            .or_else(|| data.attribute(attribute))
    };
    // The number a length starts with; `None` for none, `auto` or another keyword.
    let number = |attribute: &str| -> Option<f32> {
        let text = value(attribute)?.trim();
        let end = text
            .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')))
            .unwrap_or(text.len());
        text[..end].parse().ok()
    };
    let empty = |attribute: &str| number(attribute).is_some_and(|length| length <= 0.0);
    let missing = |attribute: &str| value(attribute).is_none_or(|text| text.trim().is_empty());
    match name {
        "rect" => missing("width") || missing("height") || empty("width") || empty("height"),
        "image" => empty("width") || empty("height"),
        "circle" => missing("r") || empty("r"),
        // A radius left out (or `auto`) is the other one.
        "ellipse" => {
            let rx = number("rx").or_else(|| number("ry"));
            let ry = number("ry").or_else(|| number("rx"));
            rx.is_none_or(|rx| rx <= 0.0) || ry.is_none_or(|ry| ry <= 0.0)
        }
        "path" => value("d").is_none_or(|d| {
            let d = d.trim();
            d.is_empty() || d.eq_ignore_ascii_case("none")
        }),
        "polygon" | "polyline" => value("points").is_none_or(|points| points.trim().is_empty()),
        _ => false,
    }
}

fn without_properties(style: &str, left_out: &[&str]) -> String {
    let mut declarations = Vec::new();
    let (mut start, mut depth, mut quote) = (0, 0_u32, None);
    for (at, character) in style.char_indices() {
        match (quote, character) {
            (Some(open), _) if character == open => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(character),
            (None, '(') => depth += 1,
            (None, ')') => depth = depth.saturating_sub(1),
            (None, ';') if depth == 0 => {
                declarations.push(&style[start..at]);
                start = at + 1;
            }
            _ => {}
        }
    }
    declarations.push(&style[start..]);
    declarations
        .into_iter()
        .filter(|declaration| {
            declaration.split_once(':').is_none_or(|(name, _)| {
                !left_out
                    .iter()
                    .any(|property| name.trim().eq_ignore_ascii_case(property))
            })
        })
        .collect::<Vec<_>>()
        .join(";")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaves_the_font_shorthand_out_of_style_attributes() {
        assert_eq!(
            without_properties("font: 20px/1 'Ahem'; fill: red", &REPLACED[..1]),
            " fill: red"
        );
        assert_eq!(
            without_properties("FONT:bold 2em serif;font-size:3px", &REPLACED[..1]),
            "font-size:3px"
        );
        // Semicolons in strings and functions do not end a declaration.
        assert_eq!(
            without_properties("font-family: 'a;b'; font: x; fill: url(\"#a;b\")", &REPLACED[..1]),
            "font-family: 'a;b'; fill: url(\"#a;b\")"
        );
    }

    #[test]
    fn writes_inline_svg_as_a_document() {
        let document = Document::parse_html(
            "<p>Logo <svg viewBox='0 0 24 24' width=24 height=24><title>A &amp; B</title>\
             <linearGradient id=g><stop offset=0 stop-color=red /></linearGradient>\
             <path d='M0 0h24' stroke=currentColor fill='url(#g)' /><use xlink:href='#icon' />\
             <foreignObject x=5 width=10 height='2' class=c><div>html</div></foreignObject><script>alert(1)</script></svg></p>\
             <svg style='display:none'><symbol id=icon viewBox='0 0 10 10'><use href='#dot' /></symbol>\
             <circle id=dot r=1 /></svg>",
        );
        let svg = document
            .descendants(NodeId::DOCUMENT)
            .find(|&node| document.element(node).is_some_and(|data| data.is_svg("svg")))
            .expect("svg");
        let markup = document.svg_document(
            svg,
            &[("color", "#123456".to_owned()), ("width", "1".to_owned())],
            &|node| {
                if document.element(node).is_some_and(|data| data.is_svg("path")) {
                    "fill: rgba(39, 65, 217, 1)".to_owned().into()
                } else {
                    Declared::default()
                }
            },
        );
        // The `<foreignObject>` is a group that marks its place.
        let foreign = document.foreign_objects(svg);
        assert_eq!(foreign.len(), 1);
        let (index, _) = foreign.iter().next().expect("foreignObject");
        assert_eq!(
            markup,
            format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" color=\"#123456\" \
             viewBox=\"0 0 24 24\" width=\"24\" height=\"24\"><title>A &amp; B</title>\
             <linearGradient id=\"g\"><stop offset=\"0\" stop-color=\"red\"></stop></linearGradient>\
             <path d=\"M0 0h24\" stroke=\"currentColor\" fill=\"url(#g)\" style=\"fill: rgba(39, 65, 217, 1)\"></path><use xlink:href=\"#icon\"></use>\
             <g id=\"fagus-foreign-object-{index}\"><rect x=\"5\" width=\"10\" height=\"2\" style=\"fill: none; stroke: none; filter: none\"></rect></g>\
             <defs><symbol id=\"icon\" viewBox=\"0 0 10 10\"><use href=\"#dot\"></use></symbol>\
             <circle id=\"dot\" r=\"1\"></circle></defs></svg>"
            )
        );
    }

    #[test]
    fn writes_the_transforms_and_clips_that_css_makes() {
        let document = Document::parse_html(
            "<svg><rect id=r x=1 transform='rotate(9)' transform-origin='1 2' clip-path='circle()' \
             mask='url(#m)' style='transform: scale(2); fill: red; mask: url(#m)'/><circle r=1 transform='scale(3)'/></svg>",
        );
        let svg = document
            .descendants(NodeId::DOCUMENT)
            .find(|&node| document.element(node).is_some_and(|data| data.is_svg("svg")))
            .expect("svg");
        let rect = document
            .descendants(svg)
            .find(|&node| document.element(node).is_some_and(|data| data.is_svg("rect")))
            .expect("rect");
        let markup = document.svg_document(svg, &[], &|node| {
            if node == rect {
                Declared {
                    style: String::new(),
                    transform: Some("matrix(2 0 0 2 0 0)".to_owned()),
                    id: Some("mark".to_owned()),
                    clip: Some(("M0 0H1V1Z".to_owned(), true)),
                    mask: Some("<mask id=\"css\"></mask>".to_owned()),
                    attributes: vec![("x", "5px".to_owned())],
                }
            } else {
                Declared::default()
            }
        });
        // The transform, clip and geometry that CSS makes take the place of the element's own;
        // the other elements keep theirs.
        assert_eq!(
            markup,
            format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\">\
                 <clipPath id=\"{CLIP}{}\" clipPathUnits=\"userSpaceOnUse\"><path clip-rule=\"evenodd\" d=\"M0 0H1V1Z\"></path></clipPath>\
                 <mask id=\"css\"></mask>\
                 <rect x=\"5px\" id=\"mark\" clip-path=\"url(#{CLIP}{})\" mask=\"url(#{MASK}{})\" transform=\"matrix(2 0 0 2 0 0)\" style=\" fill: red\"></rect>\
                 <circle r=\"1\" transform=\"scale(3)\"></circle></svg>",
                rect.index(),
                rect.index(),
                rect.index()
            )
        );
    }

    #[test]
    fn filters_no_shapes_whose_geometry_disables_them() {
        let document = Document::parse_html(
            "<svg><rect width=0 height=5 filter='url(#f)'/><rect width=5 height=5 filter='url(#f)'/>\
             <circle filter='url(#f)'/><ellipse rx=0 ry=2 filter='url(#f)'/><ellipse ry=2 filter='url(#f)'/>\
             <path d='' filter='url(#f)'/><polygon filter='url(#f)'/><image width=5 height=0 filter='url(#f)'/>\
             <foreignObject width=0 height=0 filter='url(#f)'></foreignObject></svg>",
        );
        let svg = document
            .descendants(NodeId::DOCUMENT)
            .find(|&node| document.element(node).is_some_and(|data| data.is_svg("svg")))
            .expect("svg");
        let markup = document.svg_document(svg, &[], &|_| Declared::default());
        let filtered: Vec<bool> = markup
            .split('<')
            .filter(|tag| tag.contains("filter=\"url(#f)\""))
            .map(|tag| !tag.contains("filter: none"))
            .collect();
        // Only the rectangle of a size, the ellipse whose radii are both 2 and the group of the
        // `<foreignObject>` keep their filters.
        assert_eq!(
            filtered,
            [false, true, false, false, true, false, false, false, true],
            "{markup}"
        );
    }

    #[test]
    fn writes_the_transforms_of_nested_svgs_on_groups_around_them() {
        // usvg applies the transform of a nested `<svg>` to the clip of its viewport and again
        // to its content: Chrome applies it once.
        let document = Document::parse_html(
            "<svg><svg id=a width=5 height=5 transform='scale(2)' transform-origin='1 1'><rect/></svg>\
             <svg id=b width=5 height=5><rect/></svg></svg>",
        );
        let svgs: Vec<NodeId> = document
            .descendants(NodeId::DOCUMENT)
            .filter(|&node| document.element(node).is_some_and(|data| data.is_svg("svg")))
            .collect();
        let markup = document.svg_document(svgs[0], &[], &|node| {
            if node == svgs[2] {
                Declared {
                    transform: Some("matrix(3 0 0 3 0 0)".to_owned()),
                    ..Declared::default()
                }
            } else {
                Declared::default()
            }
        });
        assert_eq!(
            markup,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\">\
             <g transform-origin=\"1 1\" transform=\"scale(2)\"><svg id=\"a\" width=\"5\" height=\"5\"><rect></rect></svg></g>\
             <g transform=\"matrix(3 0 0 3 0 0)\"><svg id=\"b\" width=\"5\" height=\"5\"><rect></rect></svg></g></svg>"
        );
    }

    #[test]
    fn writes_the_svg_elements_that_css_names() {
        let document = Document::parse_html(
            "<svg width=0 height=0><mask id=m><rect width=10 height=10 fill='url(#g)' /></mask>\
             <linearGradient id=g><stop offset=0 stop-color=white /></linearGradient></svg>",
        );
        assert_eq!(
            document
                .svg_definition("m", &|_| Vec::new(), &|_| Declared::default())
                .as_deref(),
            Some(
                "<mask id=\"m\"><rect width=\"10\" height=\"10\" fill=\"url(#g)\"></rect></mask>\
                 <linearGradient id=\"g\"><stop offset=\"0\" stop-color=\"white\"></stop></linearGradient>"
            )
        );
        assert_eq!(
            document.svg_definition("missing", &|_| Vec::new(), &|_| Declared::default()),
            None
        );
        // What a clip path inherits from the SVG around it and from the HTML stays with it.
        let document = Document::parse_html(
            "<svg font-family=Inter fill=red><g font-size=20 font-family=Geist><clipPath id=c fill=blue>\
             <text>Fagus</text></clipPath></g></svg>",
        );
        assert_eq!(
            document
                .svg_definition(
                    "c",
                    &|_| vec![("font-size", "16".to_owned()), ("color", "#123456".to_owned())],
                    &|_| Declared::default()
                )
                .as_deref(),
            Some(
                "<clipPath font-size=\"20\" font-family=\"Geist\" color=\"#123456\" id=\"c\" fill=\"blue\">\
                 <text>Fagus</text></clipPath>"
            )
        );
    }
}

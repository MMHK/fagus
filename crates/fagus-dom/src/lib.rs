//! Arena DOM for Fagus.
//!
//! Nodes live in one vector and refer to each other by [`NodeId`]. HTML is parsed with html5ever,
//! which implements the HTML5 parsing algorithm, so the tree matches what browsers build.

#![forbid(unsafe_code)]

pub mod budget;
mod sink;
mod svg;

pub use sink::MAX_DEPTH;
pub use svg::{Declared as SvgDeclared, FOREIGN_OBJECT, MASK as SVG_MASK};

pub use markup5ever::{LocalName, Namespace, QualName, local_name, ns};

/// Index of a node in its [`Document`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(u32);

impl NodeId {
    /// The document node, the root of every tree.
    pub const DOCUMENT: Self = Self(0);

    pub fn index(self) -> usize {
        self.0 as usize
    }

    fn from_index(index: usize) -> Self {
        Self(u32::try_from(index).expect("more than u32::MAX nodes"))
    }
}

/// An attribute of an element.
#[derive(Clone, Debug)]
pub struct Attribute {
    pub name: QualName,
    pub value: String,
}

/// Data of an element node.
#[derive(Clone, Debug)]
pub struct ElementData {
    pub name: QualName,
    pub attributes: Vec<Attribute>,
    /// The value of the `id` attribute.
    pub id: Option<String>,
    /// The values of the `class` attribute, split at ASCII whitespace.
    pub classes: Vec<String>,
    /// The contents of a `<template>` element, kept out of the rendered tree.
    pub template_contents: Option<NodeId>,
    /// The source line the element started on (1-based), for diagnostics.
    pub line: u32,
}

impl ElementData {
    pub fn local_name(&self) -> &LocalName {
        &self.name.local
    }

    pub fn namespace(&self) -> &Namespace {
        &self.name.ns
    }

    pub fn is_html(&self) -> bool {
        self.name.ns == ns!(html)
    }

    /// Whether this is an HTML element with the given local name.
    pub fn is(&self, local_name: &str) -> bool {
        self.is_html() && &*self.name.local == local_name
    }

    /// Whether this is an SVG element with the given local name.
    pub fn is_svg(&self, local_name: &str) -> bool {
        self.name.ns == ns!(svg) && &*self.name.local == local_name
    }

    /// The value of an attribute without namespace.
    pub fn attribute(&self, local_name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|attribute| attribute.name.ns == ns!() && &*attribute.name.local == local_name)
            .map(|attribute| attribute.value.as_str())
    }

    /// The language of the element as its attributes give it: `xml:lang`, which wins in XML
    /// documents, or `lang`.
    pub fn language(&self) -> Option<&str> {
        self.attributes
            .iter()
            .find(|attribute| attribute.name.ns == ns!(xml) && &*attribute.name.local == "lang")
            .map(|attribute| attribute.value.as_str())
            .or_else(|| self.attribute("lang"))
    }

    fn cache_id_and_classes(&mut self) {
        self.id = self.attribute("id").map(str::to_owned);
        self.classes = self
            .attribute("class")
            .map(|classes| classes.split_ascii_whitespace().map(str::to_owned).collect())
            .unwrap_or_default();
    }
}

/// What a node is.
#[derive(Clone, Debug)]
pub enum NodeKind {
    Document,
    Doctype,
    Element(ElementData),
    Text(String),
    Comment,
    ProcessingInstruction,
}

/// A node with its links to parent and siblings.
#[derive(Clone, Debug)]
pub struct Node {
    pub parent: Option<NodeId>,
    pub first_child: Option<NodeId>,
    pub last_child: Option<NodeId>,
    pub previous_sibling: Option<NodeId>,
    pub next_sibling: Option<NodeId>,
    pub kind: NodeKind,
}

impl Node {
    fn new(kind: NodeKind) -> Self {
        Self {
            parent: None,
            first_child: None,
            last_child: None,
            previous_sibling: None,
            next_sibling: None,
            kind,
        }
    }
}

/// A parsed document.
#[derive(Clone, Debug)]
pub struct Document {
    nodes: Vec<Node>,
    quirks_mode: bool,
    limited_quirks_mode: bool,
    truncated: bool,
}

impl Document {
    /// Parses an HTML document. Parsing never fails: like browsers, html5ever repairs any input.
    pub fn parse_html(html: &str) -> Self {
        sink::parse_html(html).charged(html.len())
    }

    /// Parses an XHTML document as XML, as browsers do for `.xhtml` files: CDATA sections and
    /// empty-element tags work, nothing is implied, names are case-sensitive. Like
    /// [`Document::parse_html`], it never fails: malformed XML is repaired.
    pub fn parse_xhtml(xml: &str) -> Self {
        sink::parse_xml(xml).charged(xml.len())
    }

    /// Charges the document, parsed from `markup` bytes, to the budget of the render (see
    /// [`budget`]): its nodes, and its text and attributes as about as large as the markup.
    fn charged(self, markup: usize) -> Self {
        budget::check_time_now();
        budget::charge(
            budget::Kind::Document,
            (self.nodes.len() * size_of::<Node>() + markup) as u64,
        );
        self
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.index()]
    }

    /// Number of nodes, including the document node and nodes outside the tree.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.len() <= 1
    }

    /// Whether the parser stopped before the end of the input, because elements were nested
    /// absurdly deep.
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// Whether the document is rendered in quirks mode (no or an old doctype).
    pub fn quirks_mode(&self) -> bool {
        self.quirks_mode
    }

    /// Whether the document is rendered in limited-quirks mode (a transitional or frameset
    /// doctype with a system identifier, such as XHTML 1.0 Transitional).
    pub fn limited_quirks_mode(&self) -> bool {
        self.limited_quirks_mode
    }

    pub fn element(&self, id: NodeId) -> Option<&ElementData> {
        match &self.node(id).kind {
            NodeKind::Element(element) => Some(element),
            _ => None,
        }
    }

    pub fn text(&self, id: NodeId) -> Option<&str> {
        match &self.node(id).kind {
            NodeKind::Text(text) => Some(text),
            _ => None,
        }
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).parent
    }

    pub fn parent_element(&self, id: NodeId) -> Option<NodeId> {
        self.node(id)
            .parent
            .filter(|&parent| self.element(parent).is_some())
    }

    pub fn children(&self, id: NodeId) -> Children<'_> {
        Children {
            document: self,
            next: self.node(id).first_child,
        }
    }

    /// The element children of a node.
    pub fn child_elements(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.children(id).filter(|&child| self.element(child).is_some())
    }

    pub fn previous_sibling_element(&self, id: NodeId) -> Option<NodeId> {
        let mut current = self.node(id).previous_sibling;
        while let Some(sibling) = current {
            if self.element(sibling).is_some() {
                return Some(sibling);
            }
            current = self.node(sibling).previous_sibling;
        }
        None
    }

    pub fn next_sibling_element(&self, id: NodeId) -> Option<NodeId> {
        let mut current = self.node(id).next_sibling;
        while let Some(sibling) = current {
            if self.element(sibling).is_some() {
                return Some(sibling);
            }
            current = self.node(sibling).next_sibling;
        }
        None
    }

    /// The `<html>` element.
    pub fn document_element(&self) -> Option<NodeId> {
        self.child_elements(NodeId::DOCUMENT).next()
    }

    /// All nodes below `id` in tree order, without `id` itself.
    pub fn descendants(&self, id: NodeId) -> Descendants<'_> {
        Descendants {
            document: self,
            root: id,
            next: self.node(id).first_child,
        }
    }

    /// The first HTML element with this local name, in tree order.
    pub fn find(&self, local_name: &str) -> Option<NodeId> {
        self.descendants(NodeId::DOCUMENT)
            .find(|&id| self.element(id).is_some_and(|element| element.is(local_name)))
    }

    /// The text content of a node: the concatenated text of all text descendants.
    pub fn text_content(&self, id: NodeId) -> String {
        if let Some(text) = self.text(id) {
            return text.to_owned();
        }
        self.descendants(id).filter_map(|node| self.text(node)).collect()
    }
}

/// Iterator over the children of a node.
#[derive(Debug)]
pub struct Children<'a> {
    document: &'a Document,
    next: Option<NodeId>,
}

impl Iterator for Children<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        let current = self.next?;
        self.next = self.document.node(current).next_sibling;
        Some(current)
    }
}

/// Iterator over the descendants of a node in tree order.
#[derive(Debug)]
pub struct Descendants<'a> {
    document: &'a Document,
    root: NodeId,
    next: Option<NodeId>,
}

impl Iterator for Descendants<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        let current = self.next?;
        let node = self.document.node(current);
        self.next = if let Some(child) = node.first_child {
            Some(child)
        } else {
            let mut ancestor = current;
            loop {
                if ancestor == self.root {
                    break None;
                }
                let ancestor_node = self.document.node(ancestor);
                if let Some(sibling) = ancestor_node.next_sibling {
                    break Some(sibling);
                }
                match ancestor_node.parent {
                    Some(parent) if parent != self.root => ancestor = parent,
                    _ => break None,
                }
            }
        };
        Some(current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_document_like_a_browser() {
        let document =
            Document::parse_html("<!doctype html><title>T</title><p class='a  b' id=x>Hello <b>World</b>");
        let html = document.document_element().expect("html element");
        assert!(document.element(html).unwrap().is("html"));

        let paragraph = document.find("p").expect("p element");
        let element = document.element(paragraph).unwrap();
        assert_eq!(element.id.as_deref(), Some("x"));
        assert_eq!(element.classes, ["a", "b"]);
        assert_eq!(document.text_content(paragraph), "Hello World");
        assert!(!document.quirks_mode());

        let body = document.find("body").expect("body is created by the parser");
        assert_eq!(document.parent_element(paragraph), Some(body));
    }

    #[test]
    fn merges_adjacent_text() {
        let document = Document::parse_html("<p>a&amp;b</p>");
        let paragraph = document.find("p").unwrap();
        assert_eq!(document.children(paragraph).count(), 1);
        assert_eq!(document.text_content(paragraph), "a&b");
    }

    #[test]
    fn limits_the_depth_of_the_tree() {
        let depth = MAX_DEPTH + 100;
        let html = format!(
            "{}tief{}<p>danach</p>",
            "<div>".repeat(depth),
            "</div>".repeat(depth)
        );
        let document = Document::parse_html(&html);
        let depth_of = |node: NodeId| {
            let mut depth = 0;
            let mut current = Some(node);
            while let Some(id) = current {
                current = document.parent(id);
                depth += 1;
            }
            depth - 1
        };
        let deepest = document
            .descendants(NodeId::DOCUMENT)
            .map(depth_of)
            .max()
            .unwrap_or_default();
        assert_eq!(deepest, MAX_DEPTH);
        // Nothing is lost, and the order stays.
        let body = document.find("body").expect("body");
        assert_eq!(document.text_content(body), "tiefdanach");
        let divs = document
            .descendants(NodeId::DOCUMENT)
            .filter(|node| document.element(*node).is_some_and(|element| element.is("div")))
            .count();
        assert_eq!(divs, depth);
        assert!(!document.truncated());
    }

    #[test]
    fn stops_reading_absurdly_deep_documents() {
        // Each level makes the parser search all open elements: deep nesting is slow.
        let html = format!("<p>davor</p>{}<p>danach</p>", "<div>".repeat(100_000));
        let start = std::time::Instant::now();
        let document = Document::parse_html(&html);
        assert!(document.truncated());
        assert!(start.elapsed().as_secs_f32() < 1.0, "{:?}", start.elapsed());
        let body = document.find("body").expect("body");
        assert_eq!(document.text_content(body), "davor");
    }

    #[test]
    fn records_source_lines() {
        let document = Document::parse_html("<!doctype html>\n<html>\n<body>\n<p>x</p>");
        let paragraph = document.find("p").unwrap();
        assert_eq!(document.element(paragraph).unwrap().line, 4);
    }

    #[test]
    fn keeps_template_contents_out_of_the_tree() {
        let document = Document::parse_html("<body><template><p>hidden</p></template>");
        let template = document.find("template").unwrap();
        assert_eq!(document.children(template).count(), 0);
        assert!(document.element(template).unwrap().template_contents.is_some());
        assert!(document.find("p").is_none());
    }

    #[test]
    fn detects_quirks_mode() {
        assert!(Document::parse_html("<p>no doctype").quirks_mode());
        let limited = Document::parse_html(
            "<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Transitional//EN\" \
             \"http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd\"><p>x",
        );
        assert!(!limited.quirks_mode() && limited.limited_quirks_mode());
        let standards = Document::parse_html("<!DOCTYPE html><p>x");
        assert!(!standards.quirks_mode() && !standards.limited_quirks_mode());
    }

    #[test]
    fn parses_xhtml_as_xml() {
        let document = Document::parse_xhtml(
            "<?xml version=\"1.0\"?>\n<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Transitional//EN\" \
             \"http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd\">\n\
             <html xmlns=\"http://www.w3.org/1999/xhtml\"><head>\
             <style type=\"text/css\"><![CDATA[ p > b { color: red } ]]></style></head>\
             <body><div/><p>a&nbsp;<b>b</b></p></body></html>",
        );
        // An XHTML doctype does not make XHTML quirky.
        assert!(!document.quirks_mode() && !document.limited_quirks_mode());
        let style = document.find("style").expect("style");
        assert!(document.element(style).unwrap().is_html());
        // The CDATA section is the style sheet's text, without its markers.
        assert_eq!(document.text_content(style), " p > b { color: red } ");
        // `<div/>` is empty: the paragraph is its sibling, not its child.
        let div = document.find("div").expect("div");
        assert_eq!(document.children(div).count(), 0);
        let paragraph = document.find("p").expect("p");
        assert_eq!(document.parent_element(paragraph), document.parent_element(div));
        // HTML's named character references are known, as the XHTML doctype declares them.
        assert_eq!(document.text_content(paragraph), "a\u{a0}b");
    }

    #[test]
    fn implies_no_elements_in_xhtml() {
        let document = Document::parse_xhtml("<html xmlns=\"http://www.w3.org/1999/xhtml\"><p>x</p></html>");
        assert!(document.find("body").is_none());
        let html = document.document_element().expect("html");
        assert_eq!(document.parent_element(document.find("p").unwrap()), Some(html));
    }

    #[test]
    fn takes_the_language_from_xml_lang_or_lang() {
        let document = Document::parse_xhtml(
            "<html xmlns=\"http://www.w3.org/1999/xhtml\" xml:lang=\"ja\" lang=\"en\"><p lang=\"de\">x</p></html>",
        );
        let html = document.document_element().expect("html");
        assert_eq!(document.element(html).and_then(ElementData::language), Some("ja"));
        let paragraph = document.find("p").expect("p");
        assert_eq!(
            document.element(paragraph).and_then(ElementData::language),
            Some("de")
        );
    }
}

//! The html5ever tree sink that builds the arena.

use std::borrow::Cow;
use std::cell::{Cell, Ref, RefCell};
use std::rc::Rc;

use html5ever::interface::tree_builder::{ElementFlags, NodeOrText, QuirksMode, TreeSink};
use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::{ParseOpts, parse_document};
use markup5ever::QualName;

use crate::{Attribute, Document, ElementData, Node, NodeId, NodeKind};

/// How deep elements nest at most. Deeper nodes become siblings at this depth, as in browsers
/// (Blink stops at 512), so that the steps after parsing, which walk the tree recursively, need a
/// bounded stack.
pub const MAX_DEPTH: usize = 256;

/// How deep the parser nests elements before it stops reading. Every block-level start tag makes
/// the parser search its stack of open elements, so deep nesting costs time with the square of
/// the depth; real documents stay far below this.
const PARSE_DEPTH: u32 = 512;

/// How much input the parser reads before it checks the depth.
pub(crate) const CHUNK: usize = 16 * 1024;

pub(crate) fn parse_html(html: &str) -> Document {
    let too_deep = Rc::new(Cell::new(false));
    let parser = parse_document(Sink::new(Rc::clone(&too_deep)), ParseOpts::default());
    feed(parser, html, &too_deep)
}

/// Parses an XHTML document as XML (xml5ever), as browsers parse `application/xhtml+xml`:
/// `<![CDATA[`, empty-element tags (`<div/>`) and case-sensitive names, no quirks mode and no
/// implied `html`, `head` or `body` elements. Malformed input is repaired rather than refused.
pub(crate) fn parse_xml(xml: &str) -> Document {
    let too_deep = Rc::new(Cell::new(false));
    let parser = xml5ever::driver::parse_document(
        Sink::new(Rc::clone(&too_deep)),
        xml5ever::driver::XmlParseOpts::default(),
    );
    feed(parser, xml, &too_deep)
}

/// Feeds `input` to a parser in chunks, stopping once elements nest too deep.
fn feed(
    mut parser: impl TendrilSink<html5ever::tendril::fmt::UTF8, Output = Document>,
    input: &str,
    too_deep: &Cell<bool>,
) -> Document {
    let mut rest = input;
    while !rest.is_empty() && !too_deep.get() {
        let mut end = rest.floor_char_boundary(CHUNK.min(rest.len()));
        if end == 0 {
            end = rest.ceil_char_boundary(1);
        }
        let (chunk, after) = rest.split_at(end);
        parser.process(StrTendril::from(chunk));
        rest = after;
    }
    let mut document = parser.finish();
    document.truncated = !rest.is_empty();
    document
}

struct Sink {
    nodes: RefCell<Vec<Node>>,
    /// The depth of each node when it was inserted.
    depths: RefCell<Vec<u32>>,
    /// Set when the parser nests deeper than [`PARSE_DEPTH`].
    too_deep: Rc<Cell<bool>>,
    line: Cell<u32>,
    quirks_mode: Cell<QuirksMode>,
}

impl Sink {
    fn new(too_deep: Rc<Cell<bool>>) -> Self {
        Self {
            nodes: RefCell::new(vec![Node::new(NodeKind::Document)]),
            depths: RefCell::new(vec![0]),
            too_deep,
            line: Cell::new(1),
            quirks_mode: Cell::new(QuirksMode::NoQuirks),
        }
    }

    fn push(&self, kind: NodeKind) -> NodeId {
        let mut nodes = self.nodes.borrow_mut();
        nodes.push(Node::new(kind));
        self.depths.borrow_mut().push(0);
        NodeId::from_index(nodes.len() - 1)
    }

    /// Records the depth of a node inserted at `depth`.
    fn place(&self, id: NodeId, depth: u32) {
        self.depths.borrow_mut()[id.index()] = depth;
        if depth > PARSE_DEPTH {
            self.too_deep.set(true);
        }
    }

    fn detach(&self, id: NodeId) {
        detach(&mut self.nodes.borrow_mut(), id);
    }

    fn append_node(&self, parent: NodeId, child: NodeId) {
        let depth = self.depths.borrow()[parent.index()] + 1;
        self.place(child, depth);
        self.detach(child);
        let mut nodes = self.nodes.borrow_mut();
        let last = nodes[parent.index()].last_child;
        nodes[child.index()].parent = Some(parent);
        nodes[child.index()].previous_sibling = last;
        match last {
            Some(last) => nodes[last.index()].next_sibling = Some(child),
            None => nodes[parent.index()].first_child = Some(child),
        }
        nodes[parent.index()].last_child = Some(child);
    }

    fn insert_before(&self, sibling: NodeId, child: NodeId) {
        let depth = self.depths.borrow()[sibling.index()];
        self.place(child, depth);
        self.detach(child);
        let mut nodes = self.nodes.borrow_mut();
        let parent = nodes[sibling.index()].parent;
        let previous = nodes[sibling.index()].previous_sibling;
        nodes[child.index()].parent = parent;
        nodes[child.index()].next_sibling = Some(sibling);
        nodes[child.index()].previous_sibling = previous;
        nodes[sibling.index()].previous_sibling = Some(child);
        match previous {
            Some(previous) => nodes[previous.index()].next_sibling = Some(child),
            None => {
                if let Some(parent) = parent {
                    nodes[parent.index()].first_child = Some(child);
                }
            }
        }
    }

    /// Appends text to `previous` if it is a text node, so adjacent text ends up in one node.
    fn merge_text(&self, previous: Option<NodeId>, text: &str) -> bool {
        let Some(previous) = previous else { return false };
        let mut nodes = self.nodes.borrow_mut();
        if let NodeKind::Text(existing) = &mut nodes[previous.index()].kind {
            existing.push_str(text);
            true
        } else {
            false
        }
    }
}

impl TreeSink for Sink {
    type Handle = NodeId;
    type Output = Document;
    type ElemName<'a> = Ref<'a, QualName>;

    fn finish(self) -> Document {
        let mut nodes = self.nodes.into_inner();
        limit_depth(&mut nodes);
        Document {
            nodes,
            quirks_mode: self.quirks_mode.get() == QuirksMode::Quirks,
            limited_quirks_mode: self.quirks_mode.get() == QuirksMode::LimitedQuirks,
            truncated: false,
        }
    }

    fn parse_error(&self, _message: Cow<'static, str>) {}

    fn get_document(&self) -> NodeId {
        NodeId::DOCUMENT
    }

    fn elem_name<'a>(&'a self, target: &'a NodeId) -> Ref<'a, QualName> {
        Ref::map(self.nodes.borrow(), |nodes| match &nodes[target.index()].kind {
            NodeKind::Element(element) => &element.name,
            _ => panic!("elem_name called on a node that is not an element"),
        })
    }

    fn create_element(
        &self,
        name: QualName,
        attributes: Vec<html5ever::Attribute>,
        flags: ElementFlags,
    ) -> NodeId {
        let template_contents = flags.template.then(|| self.push(NodeKind::Document));
        let mut element = ElementData {
            name,
            attributes: attributes
                .into_iter()
                .map(|attribute| Attribute {
                    name: attribute.name,
                    value: attribute.value.to_string(),
                })
                .collect(),
            id: None,
            classes: Vec::new(),
            template_contents,
            line: self.line.get(),
        };
        element.cache_id_and_classes();
        self.push(NodeKind::Element(element))
    }

    fn create_comment(&self, _text: StrTendril) -> NodeId {
        self.push(NodeKind::Comment)
    }

    fn create_pi(&self, _target: StrTendril, _data: StrTendril) -> NodeId {
        self.push(NodeKind::ProcessingInstruction)
    }

    fn append(&self, parent: &NodeId, child: NodeOrText<NodeId>) {
        match child {
            NodeOrText::AppendNode(node) => self.append_node(*parent, node),
            NodeOrText::AppendText(text) => {
                let last = self.nodes.borrow()[parent.index()].last_child;
                if !self.merge_text(last, &text) {
                    let node = self.push(NodeKind::Text(text.to_string()));
                    self.append_node(*parent, node);
                }
            }
        }
    }

    fn append_based_on_parent_node(&self, element: &NodeId, previous: &NodeId, child: NodeOrText<NodeId>) {
        if self.nodes.borrow()[element.index()].parent.is_some() {
            self.append_before_sibling(element, child);
        } else {
            self.append(previous, child);
        }
    }

    fn append_doctype_to_document(&self, _name: StrTendril, _public_id: StrTendril, _system_id: StrTendril) {
        let doctype = self.push(NodeKind::Doctype);
        self.append_node(NodeId::DOCUMENT, doctype);
    }

    fn get_template_contents(&self, target: &NodeId) -> NodeId {
        match &self.nodes.borrow()[target.index()].kind {
            NodeKind::Element(ElementData {
                template_contents: Some(contents),
                ..
            }) => *contents,
            _ => panic!("get_template_contents called on a node that is not a template"),
        }
    }

    fn same_node(&self, x: &NodeId, y: &NodeId) -> bool {
        x == y
    }

    fn set_quirks_mode(&self, mode: QuirksMode) {
        self.quirks_mode.set(mode);
    }

    fn append_before_sibling(&self, sibling: &NodeId, child: NodeOrText<NodeId>) {
        match child {
            NodeOrText::AppendNode(node) => self.insert_before(*sibling, node),
            NodeOrText::AppendText(text) => {
                let previous = self.nodes.borrow()[sibling.index()].previous_sibling;
                if !self.merge_text(previous, &text) {
                    let node = self.push(NodeKind::Text(text.to_string()));
                    self.insert_before(*sibling, node);
                }
            }
        }
    }

    fn add_attrs_if_missing(&self, target: &NodeId, attributes: Vec<html5ever::Attribute>) {
        let mut nodes = self.nodes.borrow_mut();
        if let NodeKind::Element(element) = &mut nodes[target.index()].kind {
            for attribute in attributes {
                if !element
                    .attributes
                    .iter()
                    .any(|existing| existing.name == attribute.name)
                {
                    element.attributes.push(Attribute {
                        name: attribute.name,
                        value: attribute.value.to_string(),
                    });
                }
            }
            element.cache_id_and_classes();
        }
    }

    fn remove_from_parent(&self, target: &NodeId) {
        self.detach(*target);
    }

    fn reparent_children(&self, node: &NodeId, new_parent: &NodeId) {
        let mut child = self.nodes.borrow()[node.index()].first_child;
        while let Some(current) = child {
            child = self.nodes.borrow()[current.index()].next_sibling;
            self.append_node(*new_parent, current);
        }
    }

    fn set_current_line(&self, line_number: u64) {
        self.line.set(u32::try_from(line_number).unwrap_or(u32::MAX));
    }
}

fn detach(nodes: &mut [Node], id: NodeId) {
    let node = &mut nodes[id.index()];
    let parent = node.parent.take();
    let previous = node.previous_sibling.take();
    let next = node.next_sibling.take();
    match next {
        Some(next) => nodes[next.index()].previous_sibling = previous,
        None => {
            if let Some(parent) = parent {
                nodes[parent.index()].last_child = previous;
            }
        }
    }
    match previous {
        Some(previous) => nodes[previous.index()].next_sibling = next,
        None => {
            if let Some(parent) = parent {
                nodes[parent.index()].first_child = next;
            }
        }
    }
}

/// Inserts a detached node after `reference`, as its next sibling.
fn insert_after(nodes: &mut [Node], reference: NodeId, id: NodeId) {
    let parent = nodes[reference.index()].parent;
    let next = nodes[reference.index()].next_sibling;
    nodes[id.index()].parent = parent;
    nodes[id.index()].previous_sibling = Some(reference);
    nodes[id.index()].next_sibling = next;
    nodes[reference.index()].next_sibling = Some(id);
    match next {
        Some(next) => nodes[next.index()].previous_sibling = Some(id),
        None => {
            if let Some(parent) = parent {
                nodes[parent.index()].last_child = Some(id);
            }
        }
    }
}

/// Moves the descendants of nodes at [`MAX_DEPTH`] up beside them, in document order.
fn limit_depth(nodes: &mut [Node]) {
    let mut current = nodes[NodeId::DOCUMENT.index()]
        .first_child
        .map(|first| (first, 1));
    while let Some((id, depth)) = current {
        if depth >= MAX_DEPTH && nodes[id.index()].first_child.is_some() {
            let descendants = descendants(nodes, id);
            let mut previous = id;
            for descendant in descendants {
                detach(nodes, descendant);
                insert_after(nodes, previous, descendant);
                previous = descendant;
            }
        }
        current = match nodes[id.index()].first_child {
            Some(child) => Some((child, depth + 1)),
            None => following(nodes, id, depth),
        };
    }
}

/// The descendants of a node in document order.
fn descendants(nodes: &[Node], root: NodeId) -> Vec<NodeId> {
    let mut found = Vec::new();
    let mut current = nodes[root.index()].first_child;
    while let Some(id) = current {
        found.push(id);
        current = nodes[id.index()].first_child.or_else(|| {
            let mut node = id;
            loop {
                if let Some(next) = nodes[node.index()].next_sibling {
                    return Some(next);
                }
                node = nodes[node.index()].parent.filter(|parent| *parent != root)?;
            }
        });
    }
    found
}

/// The node after a node and its descendants in document order, with its depth.
fn following(nodes: &[Node], mut id: NodeId, mut depth: usize) -> Option<(NodeId, usize)> {
    loop {
        if let Some(next) = nodes[id.index()].next_sibling {
            return Some((next, depth));
        }
        id = nodes[id.index()].parent?;
        depth = depth.checked_sub(1)?;
    }
}

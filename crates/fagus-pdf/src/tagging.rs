//! Tagged PDF: the logical structure of a document, which screen readers and other assistive
//! technology read instead of the drawing operations.
//!
//! The display lists say which element each text and image belongs to ([`Tag`]); a [`Structure`]
//! says what the elements are and how they nest. While a page is drawn, each piece of content
//! becomes a marked-content sequence and everything else an artifact; afterwards the sequences go
//! into their elements, in reading order.

use std::num::{NonZeroU16, NonZeroU32};

use fagus_dom::NodeId;
use fagus_paint::{Artifact, Item, PageList, Tag};

use crate::LinkRole;
use krilla::surface::Surface;
use krilla::tagging::{
    self, ArtifactType, ContentTag, Identifier, ListNumbering, Node, SpanTag, TableHeaderScope, TagGroup,
    TagKind, TagTree,
};
use rustc_hash::FxHashMap;

/// The logical structure of a document.
#[derive(Clone, Debug, Default)]
pub struct Structure {
    /// The elements. The first is the document, which contains the others.
    pub elements: Vec<Element>,
    /// What the content of each node belongs to. The content of nodes that are not listed belongs
    /// to the document.
    pub owners: FxHashMap<NodeId, Owner>,
    /// The links of each footnote, by its element: the `Link` of its call, which leads to it,
    /// and the one of its marker in its label, which leads back. They hold the content of the
    /// call and of the marker with the link annotation; a link without its annotation gives its
    /// content to the element around it.
    pub footnotes: FxHashMap<NodeId, (usize, usize)>,
}

/// What content belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    /// An element, by its index.
    Element(usize),
    /// Nothing: the content is decoration that a reader skips, such as an image with an empty
    /// `alt`.
    Artifact,
}

/// An element of the logical structure.
#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    pub role: Role,
    /// The elements inside, in reading order.
    pub children: Vec<usize>,
    /// The language of the element, where it differs from the one around it.
    pub language: Option<String>,
    pub repeat: Repeat,
}

impl Element {
    pub fn new(role: Role) -> Self {
        Self {
            role,
            children: Vec::new(),
            language: None,
            repeat: Repeat::Never,
        }
    }
}

/// Whether the layout repeats an element on pages, as it does with table headers and footers and
/// with fixed boxes. Only one occurrence is content, the others are artifacts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Repeat {
    #[default]
    Never,
    /// The first occurrence counts (table headers, fixed boxes).
    First,
    /// The last occurrence counts (table footers, which close every page of their table).
    Last,
}

/// What an element is.
#[derive(Clone, Debug, PartialEq)]
pub enum Role {
    Document,
    Article,
    Section,
    /// A block without a more specific role.
    Division,
    BlockQuote,
    Caption,
    Paragraph,
    /// A heading of level 1 to 6, with its text.
    Heading {
        level: u8,
        title: String,
    },
    List {
        numbering: Numbering,
    },
    ListItem,
    /// The marker of a list item.
    Label,
    /// The content of a list item.
    ListBody,
    /// A footnote, after the text that calls it (the call a link to it); its marker is a
    /// label (with a link back to the call). Each gets an ID, as PDF/UA asks.
    Note,
    Table {
        summary: Option<String>,
    },
    TableHead,
    TableBody,
    TableFoot,
    TableRow,
    TableHeader {
        scope: Scope,
        rows: u32,
        columns: u32,
    },
    TableCell {
        rows: u32,
        columns: u32,
    },
    /// An image, with its alternative text.
    Figure {
        alt: Option<String>,
    },
    /// A formula (`MathML` `<math>`), with its alternative text.
    Formula {
        alt: Option<String>,
    },
    Link,
    /// A field of an interactive form, which holds its widget annotation.
    Form,
    /// Inline content without a more specific role, such as text in another language.
    Span,
    /// An abbreviation, with what it stands for.
    Abbreviation {
        expansion: String,
    },
    Code,
    Quote,
    /// Stressed text (`<em>`), as Chrome tags it; PDF 1.7 readers take it for a span.
    Emphasis,
    /// Important text (`<strong>`), as Chrome tags it; PDF 1.7 readers take it for a span.
    Strong,
}

/// How a list numbers its items.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Numbering {
    None,
    Disc,
    Circle,
    Square,
    Decimal,
    LowerRoman,
    UpperRoman,
    LowerAlpha,
    UpperAlpha,
}

/// The cells a table header cell is the header of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Row,
    Column,
    Both,
}

/// A marked-content sequence: the element it belongs to and its position in reading order.
struct Marked {
    element: usize,
    order: u32,
    id: Identifier,
}

/// Marks the content of the pages while they are drawn, then builds the tag tree.
pub(crate) struct Tagger<'a> {
    structure: &'a Structure,
    /// The element around each element; the document is its own parent.
    parents: Vec<usize>,
    /// The elements in tree order, starting with the document.
    order: Vec<usize>,
    /// For each element, the closest element around or at it that the layout repeats.
    repeating: Vec<Option<usize>>,
    /// The first and last page with content of each repeating element.
    pages: FxHashMap<usize, (usize, usize)>,
    marked: Vec<Marked>,
    /// Link annotations and the widget annotations of form fields (`true`), with the element
    /// they belong to.
    annotations: Vec<(Option<usize>, Identifier, bool)>,
}

impl<'a> Tagger<'a> {
    pub(crate) fn new(structure: &'a Structure, pages: &[PageList]) -> Self {
        let count = structure.elements.len().max(1);
        // Walk the tree from the document; an element that is reached twice stays where it was
        // reached first, and one that is never reached does not count.
        let mut parents = vec![usize::MAX; count];
        parents[0] = 0;
        let mut order = Vec::with_capacity(count);
        let mut stack = vec![0];
        while let Some(element) = stack.pop() {
            order.push(element);
            let children = structure
                .elements
                .get(element)
                .map_or(&[][..], |element| &element.children);
            for &child in children.iter().rev() {
                if child < count && parents[child] == usize::MAX {
                    parents[child] = element;
                    stack.push(child);
                }
            }
        }
        let mut repeating = vec![None; count];
        for &element in &order {
            let repeats = structure
                .elements
                .get(element)
                .is_some_and(|element| element.repeat != Repeat::Never);
            repeating[element] = if repeats {
                Some(element)
            } else if element == 0 {
                None
            } else {
                repeating[parents[element]]
            };
        }
        let mut tagger = Self {
            structure,
            parents,
            order,
            repeating,
            pages: FxHashMap::default(),
            marked: Vec::new(),
            annotations: Vec::new(),
        };
        for (index, page) in pages.iter().enumerate() {
            for item in &page.items {
                if let Item::Tag(Tag::Content { node, .. }) = item
                    && let Some(element) = tagger.owner(*node)
                    && let Some(group) = tagger.repeating[element]
                {
                    let range = tagger.pages.entry(group).or_insert((index, index));
                    range.0 = range.0.min(index);
                    range.1 = range.1.max(index);
                }
            }
        }
        tagger
    }

    /// The element a node's content belongs to, or `None` for decoration.
    fn owner(&self, node: NodeId) -> Option<usize> {
        match self.structure.owners.get(&node) {
            Some(Owner::Element(element))
                if self
                    .parents
                    .get(*element)
                    .is_some_and(|parent| *parent != usize::MAX) =>
            {
                Some(*element)
            }
            Some(Owner::Artifact) => None,
            _ => Some(0),
        }
    }

    /// Where content of a node on a page goes: into an element, or an artifact of a kind. The
    /// call of a footnote goes into its link, the marker of a footnote into the link in its
    /// label.
    fn place(&self, node: NodeId, page: usize, marker: bool, call: bool) -> Result<usize, ArtifactType> {
        let footnote = self.footnote_links(node);
        let element = match footnote {
            Some((link, _)) if call => link,
            _ => self.owner(node).ok_or(ArtifactType::Layout)?,
        };
        // A form field holds its widget only (PDF/UA-1, ISO 14289-1 7.18.4): what the control
        // draws on the page beside it is its design.
        if self.structure.elements[element].role == Role::Form {
            return Err(ArtifactType::Layout);
        }
        if let Some(group) = self.repeating[element]
            && let Some(&(first, last)) = self.pages.get(&group)
        {
            let repeated = match self.structure.elements[group].repeat {
                Repeat::Never => false,
                Repeat::First => page > first,
                Repeat::Last => page < last,
            };
            if repeated {
                return Err(ArtifactType::PaginationOther);
            }
        }
        if marker && let Some(label) = self.label(element) {
            return Ok(footnote.map_or(label, |(_, back)| back));
        }
        Ok(element)
    }

    /// The links of the footnote whose element is `node` (see [`Structure::footnotes`]), where
    /// they are in the tree.
    fn footnote_links(&self, node: NodeId) -> Option<(usize, usize)> {
        let &(call, back) = self.structure.footnotes.get(&node)?;
        let reached = |element: usize| {
            self.parents
                .get(element)
                .is_some_and(|parent| *parent != usize::MAX)
        };
        (reached(call) && reached(back)).then_some((call, back))
    }

    /// The label of the list item or the footnote an element is in.
    fn label(&self, mut element: usize) -> Option<usize> {
        let elements = &self.structure.elements;
        loop {
            let current = elements.get(element)?;
            if matches!(current.role, Role::ListItem | Role::Note) {
                return current.children.iter().copied().find(|&child| {
                    self.parents.get(child) == Some(&element)
                        && elements.get(child).is_some_and(|child| child.role == Role::Label)
                });
            }
            if element == 0 {
                return None;
            }
            element = self.parents[element];
        }
    }

    /// Draws the items of a page, each as content or as an artifact. Sequences never span the
    /// start or end of a group, so that they nest with the graphics state; a transparency group
    /// is drawn into a group of its own, so it is marked as a whole, as the content of the first
    /// element inside it.
    pub(crate) fn draw(
        &mut self,
        surface: &mut Surface<'_>,
        page: usize,
        items: &[Item],
        draw: &mut impl FnMut(&mut Surface<'_>, &Item),
    ) {
        let mut tag = Tag::Artifact(Artifact::Layout);
        let mut open: Option<Tag> = None;
        let mut index = 0;
        while let Some(item) = items.get(index) {
            match item {
                Item::Tag(next) => tag = *next,
                Item::PushOpacity(_)
                | Item::PushBlend(_)
                | Item::PushIsolated
                | Item::PushMask(_)
                | Item::PushClip(..)
                | Item::PushClipShape(_)
                    if crate::opens_group(item) =>
                {
                    let end = group_end(items, index);
                    let group = &items[index..end];
                    if open.take().is_some() {
                        surface.end_tagged();
                    }
                    let visible = !matches!(item, Item::PushClip(..) | Item::PushClipShape(_))
                        && !matches!(item, Item::PushOpacity(opacity) if *opacity <= 0.0);
                    let content = group.iter().find_map(|item| match item {
                        Item::Tag(content @ Tag::Content { .. }) => Some(*content),
                        _ => None,
                    });
                    let marked = match content {
                        Some(content) if visible => content,
                        _ => Tag::Artifact(Artifact::Layout),
                    };
                    self.start(surface, page, marked, group.iter().find(|item| is_drawing(item)));
                    for item in group {
                        match item {
                            Item::Tag(next) => tag = *next,
                            _ => draw(surface, item),
                        }
                    }
                    surface.end_tagged();
                    index = end;
                    continue;
                }
                Item::PushOpacity(_)
                | Item::PushBlend(_)
                | Item::PushIsolated
                | Item::PushLayer(_)
                | Item::PushMask(_)
                | Item::PushClip(..)
                | Item::PushClipShape(_)
                | Item::PushTransform(_)
                | Item::Pop => {
                    if open.take().is_some() {
                        surface.end_tagged();
                    }
                    draw(surface, item);
                }
                _ => {
                    if open != Some(tag) {
                        if open.is_some() {
                            surface.end_tagged();
                        }
                        self.start(surface, page, tag, Some(item));
                        open = Some(tag);
                    }
                    draw(surface, item);
                }
            }
            index += 1;
        }
        if open.is_some() {
            surface.end_tagged();
        }
    }

    /// Starts a marked-content sequence for what `first` begins.
    fn start(&mut self, surface: &mut Surface<'_>, page: usize, tag: Tag, first: Option<&Item>) {
        let kind = match tag {
            Tag::Content {
                node,
                order,
                marker,
                call,
            } => match self.place(node, page, marker, call) {
                Ok(element) => {
                    let content = match first {
                        Some(Item::Text { .. }) => ContentTag::Span(SpanTag::empty()),
                        _ => ContentTag::Other,
                    };
                    let id = surface.start_tagged(content);
                    self.marked.push(Marked { element, order, id });
                    return;
                }
                Err(kind) => kind,
            },
            Tag::Artifact(Artifact::Layout) => ArtifactType::Layout,
            Tag::Artifact(Artifact::Header) => ArtifactType::Header,
            Tag::Artifact(Artifact::Footer) => ArtifactType::Footer,
            Tag::Artifact(Artifact::Pagination) => ArtifactType::PaginationOther,
        };
        surface.start_tagged(ContentTag::Artifact(tagging::Artifact::with_kind(kind)));
    }

    /// Adds a link annotation, which belongs to the element of `node`, or to a link of the
    /// footnote `node` is.
    pub(crate) fn annotation(&mut self, node: Option<NodeId>, role: LinkRole, id: Identifier) {
        let links = node.and_then(|node| self.footnote_links(node));
        let element = match (role, links) {
            (LinkRole::FootnoteCall, Some((call, _))) => Some(call),
            (LinkRole::FootnoteMarker, Some((_, back))) => Some(back),
            _ => self.annotated(node),
        };
        self.annotations.push((element, id, false));
    }

    /// Adds the widget annotation of a form field, which belongs to the element of `node`.
    pub(crate) fn widget(&mut self, node: Option<NodeId>, id: Identifier) {
        let element = self.annotated(node);
        self.annotations.push((element, id, true));
    }

    /// The element an annotation of `node` belongs to.
    fn annotated(&self, node: Option<NodeId>) -> Option<usize> {
        node.and_then(|node| match self.structure.owners.get(&node) {
            Some(Owner::Element(_)) => self.owner(node),
            _ => None,
        })
    }

    /// The tag tree: every element with content, the content in reading order.
    pub(crate) fn finish(self) -> TagTree {
        let count = self.parents.len();
        let mut content: Vec<Vec<(u32, Identifier)>> = vec![Vec::new(); count];
        for marked in self.marked {
            content[marked.element].push((marked.order, marked.id));
        }
        for list in &mut content {
            list.sort_by_key(|(order, _)| *order);
        }
        let mut annotations: Vec<Vec<(Identifier, bool)>> = vec![Vec::new(); count];
        let mut loose = Vec::new();
        for (element, id, widget) in self.annotations {
            match element {
                Some(element) => annotations[element].push((id, widget)),
                None => loose.push((id, widget)),
            }
        }
        // The links of footnotes that have no annotation (a call that is not shown, a marker
        // whose call is not) give their content to the element around them.
        let dissolved: Vec<bool> = (0..count)
            .map(|element| {
                annotations[element].is_empty()
                    && self
                        .structure
                        .footnotes
                        .values()
                        .any(|&(call, back)| call == element || back == element)
            })
            .collect();
        // Children come after their parents in tree order, so in reverse they are done first.
        let mut first: Vec<Option<u32>> = vec![None; count];
        let mut groups: Vec<Option<Vec<Node>>> = (0..count).map(|_| None).collect();
        for &element in self.order.iter().rev() {
            let own = &content[element];
            let mut nodes = Vec::new();
            let mut position = 0;
            let mut earliest = own.first().map(|(order, _)| *order);
            for &child in self
                .structure
                .elements
                .get(element)
                .map_or(&[][..], |element| &element.children)
            {
                if self.parents.get(child) != Some(&element) || child == element {
                    continue;
                }
                let Some(children) = groups[child].take() else {
                    continue;
                };
                let at = first[child].unwrap_or(u32::MAX);
                while let Some((order, id)) = own.get(position)
                    && *order < at
                {
                    nodes.push(Node::from(*id));
                    position += 1;
                }
                earliest = match (earliest, first[child]) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, b) => a.or(b),
                };
                if dissolved[child] {
                    nodes.extend(children);
                } else {
                    nodes.push(
                        TagGroup::with_children(kind(&self.structure.elements[child]), children).into(),
                    );
                }
            }
            nodes.extend(own[position..].iter().map(|(_, id)| Node::from(*id)));
            let role = self.structure.elements.get(element).map(|element| &element.role);
            for &(id, widget) in &annotations[element] {
                nodes.push(match (role, widget) {
                    (Some(Role::Link), false) | (Some(Role::Form), true) => Node::from(id),
                    _ => annotation_group(id, widget),
                });
            }
            first[element] = earliest;
            // A table cell stays in its row without content, so that the rows of the table keep
            // the same number of columns (PDF/UA-1, ISO 14289-1 7.2).
            let cell = self.structure.elements.get(element).is_some_and(|element| {
                matches!(element.role, Role::TableCell { .. } | Role::TableHeader { .. })
            });
            if !nodes.is_empty() || cell {
                groups[element] = Some(nodes);
            }
        }
        let mut tree = TagTree::new().with_lang(
            self.structure
                .elements
                .first()
                .and_then(|document| document.language.clone()),
        );
        for node in groups[0].take().unwrap_or_default() {
            tree.push(node);
        }
        for (id, widget) in loose {
            tree.push(annotation_group(id, widget));
        }
        tree
    }
}

/// An annotation in an element of its own: a link, or a form field for a widget.
fn annotation_group(id: Identifier, widget: bool) -> Node {
    let kind: TagKind = if widget {
        tagging::Tag::Form.into()
    } else {
        tagging::Tag::Link.into()
    };
    TagGroup::with_children(kind, vec![Node::from(id)]).into()
}

/// The index after the `Pop` that ends the group `items[start]` begins.
fn group_end(items: &[Item], start: usize) -> usize {
    let mut depth = 0_usize;
    for (index, item) in items.iter().enumerate().skip(start) {
        match item {
            Item::PushOpacity(_)
            | Item::PushBlend(_)
            | Item::PushIsolated
            | Item::PushLayer(_)
            | Item::PushMask(_)
            | Item::PushClip(..)
            | Item::PushClipShape(_)
            | Item::PushTransform(_) => depth += 1,
            Item::Pop => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return index + 1;
                }
            }
            _ => {}
        }
    }
    items.len()
}

/// Whether an item draws something.
fn is_drawing(item: &Item) -> bool {
    !matches!(
        item,
        Item::Tag(_)
            | Item::PushOpacity(_)
            | Item::PushBlend(_)
            | Item::PushIsolated
            | Item::PushLayer(_)
            | Item::PushMask(_)
            | Item::PushClip(..)
            | Item::PushClipShape(_)
            | Item::PushTransform(_)
            | Item::Pop
    )
}

/// The structure tag of an element.
fn kind(element: &Element) -> TagKind {
    use krilla::tagging::Tag;
    let span = |count: u32| NonZeroU32::new(count).filter(|count| count.get() > 1);
    let kind: TagKind = match &element.role {
        Role::Document => Tag::Part.into(),
        Role::Article => Tag::Article.into(),
        Role::Section => Tag::Section.into(),
        Role::Division => Tag::Div.into(),
        Role::BlockQuote => Tag::BlockQuote.into(),
        Role::Caption => Tag::Caption.into(),
        Role::Paragraph => Tag::P.into(),
        Role::Heading { level, title } => Tag::Hn(
            NonZeroU16::new(u16::from((*level).clamp(1, 6))).unwrap_or(NonZeroU16::MIN),
            Some(title.clone()),
        )
        .into(),
        Role::List { numbering } => Tag::L(match numbering {
            Numbering::None => ListNumbering::None,
            Numbering::Disc => ListNumbering::Disc,
            Numbering::Circle => ListNumbering::Circle,
            Numbering::Square => ListNumbering::Square,
            Numbering::Decimal => ListNumbering::Decimal,
            Numbering::LowerRoman => ListNumbering::LowerRoman,
            Numbering::UpperRoman => ListNumbering::UpperRoman,
            Numbering::LowerAlpha => ListNumbering::LowerAlpha,
            Numbering::UpperAlpha => ListNumbering::UpperAlpha,
        })
        .into(),
        Role::ListItem => Tag::LI.into(),
        Role::Label => Tag::Lbl.into(),
        Role::ListBody => Tag::LBody.into(),
        Role::Note => Tag::Note.into(),
        Role::Table { summary } => Tag::Table.with_summary(summary.clone()).into(),
        Role::TableHead => Tag::THead.into(),
        Role::TableBody => Tag::TBody.into(),
        Role::TableFoot => Tag::TFoot.into(),
        Role::TableRow => Tag::TR.into(),
        Role::TableHeader { scope, rows, columns } => Tag::TH(match scope {
            Scope::Row => TableHeaderScope::Row,
            Scope::Column => TableHeaderScope::Column,
            Scope::Both => TableHeaderScope::Both,
        })
        .with_row_span(span(*rows))
        .with_col_span(span(*columns))
        .into(),
        Role::TableCell { rows, columns } => Tag::TD
            .with_row_span(span(*rows))
            .with_col_span(span(*columns))
            .into(),
        Role::Figure { alt } => Tag::Figure(alt.clone()).into(),
        Role::Formula { alt } => Tag::Formula(alt.clone()).into(),
        Role::Link => Tag::Link.into(),
        Role::Form => Tag::Form.into(),
        Role::Span => Tag::Span.into(),
        Role::Abbreviation { expansion } => TagKind::from(Tag::Span).with_expanded(Some(expansion.clone())),
        Role::Code => Tag::Code.into(),
        Role::Quote => Tag::InlineQuote.into(),
        Role::Emphasis => Tag::Em.into(),
        Role::Strong => Tag::Strong.into(),
    };
    kind.with_lang(element.language.clone())
}

#[cfg(test)]
mod tests {
    use fagus_paint::{RectF, Tag};
    use fagus_style::values::Rgba;
    use krilla::tagging::fmt::Output;

    use super::*;

    /// Distinct node ids.
    fn nodes() -> Vec<NodeId> {
        let document = fagus_dom::Document::parse_html("<p>a</p><p>b</p><p>c</p>");
        document.descendants(NodeId::DOCUMENT).collect()
    }

    fn structure(elements: Vec<(Role, Vec<usize>)>, owners: &[(NodeId, Owner)]) -> Structure {
        Structure {
            elements: elements
                .into_iter()
                .map(|(role, children)| Element {
                    children,
                    ..Element::new(role)
                })
                .collect(),
            owners: owners.iter().copied().collect(),
            ..Structure::default()
        }
    }

    fn content(node: NodeId, order: u32) -> Item {
        Item::Tag(Tag::Content {
            node,
            order,
            marker: false,
            call: false,
        })
    }

    fn rect() -> Item {
        Item::Rect {
            rect: RectF {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            color: Rgba::BLACK,
            radii: None,
        }
    }

    /// Draws pages with links (page and node) and returns the tag tree as an outline: `c1.2` is
    /// the content with MCID 2 on page 1, `a0.0` the first annotation on page 0.
    fn tree(structure: &Structure, pages: &[Vec<Item>], links: &[(usize, Option<NodeId>)]) -> String {
        let links: Vec<_> = links
            .iter()
            .map(|&(page, node)| (page, node, LinkRole::Element))
            .collect();
        tree_with(structure, pages, &links)
    }

    /// [`tree`] with links of other roles.
    fn tree_with(
        structure: &Structure,
        pages: &[Vec<Item>],
        links: &[(usize, Option<NodeId>, LinkRole)],
    ) -> String {
        let lists: Vec<PageList> = pages
            .iter()
            .map(|items| PageList {
                width: 100.0,
                height: 100.0,
                items: items.clone(),
                bleed: 0.0,
                marks: fagus_layout::PageMarks::default(),
                orientation: fagus_layout::PageOrientation::Upright,
            })
            .collect();
        let mut tagger = Tagger::new(structure, &lists);
        let mut document = krilla::Document::new();
        for (index, list) in lists.iter().enumerate() {
            let mut page = document.start_page();
            let mut surface = page.surface();
            tagger.draw(&mut surface, index, &list.items, &mut |_, _| {});
            surface.finish();
            for (_, node, role) in links.iter().filter(|(page, ..)| *page == index) {
                let rect = krilla::geom::Rect::from_xywh(0.0, 0.0, 10.0, 10.0).expect("rect");
                let target = krilla::annotation::Target::Action(
                    krilla::action::LinkAction::new("https://quittio.dev".to_owned()).into(),
                );
                let annotation = krilla::annotation::Annotation::new_link(
                    krilla::annotation::LinkAnnotation::new(rect, target),
                    None,
                );
                let id = page.add_tagged_annotation(annotation);
                tagger.annotation(*node, *role, id);
            }
            page.finish();
        }
        let tree = tagger.finish();
        let mut out = String::new();
        for (index, node) in tree.children.iter().enumerate() {
            if index > 0 {
                out.push(' ');
            }
            outline(node, &mut out);
        }
        out
    }

    fn outline(node: &Node, out: &mut String) {
        match node {
            Node::Group(group) => {
                let tag = group.tag.display().to_string();
                out.push_str(
                    tag.lines()
                        .next()
                        .unwrap_or_default()
                        .trim()
                        .trim_start_matches("- Tag: "),
                );
                out.push('[');
                for (index, child) in group.children.iter().enumerate() {
                    if index > 0 {
                        out.push(' ');
                    }
                    outline(child, out);
                }
                out.push(']');
            }
            Node::Leaf(_) => {
                let leaf = node.display().to_string();
                let numbers: Vec<&str> = leaf
                    .split(|character: char| !character.is_ascii_digit())
                    .filter(|part| !part.is_empty())
                    .collect();
                out.push(if leaf.contains("Annotation") { 'a' } else { 'c' });
                out.push_str(&numbers.join("."));
            }
        }
    }

    #[test]
    fn puts_content_in_reading_order() {
        let nodes = nodes();
        let structure = structure(
            vec![
                (Role::Document, vec![1]),
                (Role::Paragraph, vec![2]),
                (Role::Link, vec![]),
            ],
            &[(nodes[1], Owner::Element(1)), (nodes[2], Owner::Element(2))],
        );
        // Drawn in another order than read.
        let page = vec![
            content(nodes[1], 2),
            rect(),
            content(nodes[1], 0),
            rect(),
            content(nodes[2], 1),
            rect(),
            Item::Tag(Tag::Artifact(Artifact::Layout)),
            rect(),
            // Content of a node without an element belongs to the document.
            content(nodes[3], 3),
            rect(),
        ];
        assert_eq!(
            tree(&structure, &[page], &[(0, Some(nodes[2]))]),
            "P[c0.1 Link[c0.2 a0.0] c0.0] c0.3"
        );
    }

    #[test]
    fn counts_repeated_content_once() {
        let nodes = nodes();
        let mut structure = structure(
            vec![
                (Role::Document, vec![1]),
                (Role::Table { summary: None }, vec![2, 4]),
                (Role::TableHead, vec![3]),
                (
                    Role::TableHeader {
                        scope: Scope::Column,
                        rows: 1,
                        columns: 1,
                    },
                    vec![],
                ),
                (Role::TableBody, vec![5]),
                (Role::TableCell { rows: 1, columns: 1 }, vec![]),
            ],
            &[(nodes[1], Owner::Element(3)), (nodes[2], Owner::Element(5))],
        );
        structure.elements[2].repeat = Repeat::First;
        let pages = [
            vec![content(nodes[1], 0), rect(), content(nodes[2], 1), rect()],
            vec![content(nodes[1], 2), rect(), content(nodes[2], 3), rect()],
        ];
        assert_eq!(
            tree(&structure, &pages, &[]),
            "Table[THead[TH[c0.0]] TBody[TD[c0.1 c1.0]]]"
        );
        structure.elements[2].repeat = Repeat::Last;
        assert_eq!(
            tree(&structure, &pages, &[]),
            "Table[THead[TH[c1.0]] TBody[TD[c0.0 c1.1]]]"
        );
    }

    #[test]
    fn keeps_empty_table_cells() {
        let nodes = nodes();
        let structure = structure(
            vec![
                (Role::Document, vec![1, 9]),
                (Role::Table { summary: None }, vec![2]),
                (Role::TableBody, vec![3, 6]),
                (Role::TableRow, vec![4, 5]),
                (
                    Role::TableHeader {
                        scope: Scope::Row,
                        rows: 1,
                        columns: 1,
                    },
                    vec![],
                ),
                (Role::TableCell { rows: 1, columns: 1 }, vec![]),
                (Role::TableRow, vec![7, 8]),
                (Role::TableCell { rows: 1, columns: 1 }, vec![]),
                (Role::TableCell { rows: 1, columns: 1 }, vec![]),
                // An element other than a cell without content is left out.
                (Role::Paragraph, vec![]),
            ],
            &[(nodes[1], Owner::Element(5)), (nodes[2], Owner::Element(7))],
        );
        let page = vec![content(nodes[1], 0), rect(), content(nodes[2], 1), rect()];
        assert_eq!(
            tree(&structure, &[page], &[]),
            "Table[TBody[TR[TH[] TD[c0.0]] TR[TD[c0.1] TD[]]]]"
        );
    }

    #[test]
    fn puts_markers_into_labels() {
        let nodes = nodes();
        let structure = structure(
            vec![
                (
                    Role::List {
                        numbering: Numbering::Decimal,
                    },
                    vec![],
                ),
                (Role::Document, vec![2]),
                (
                    Role::List {
                        numbering: Numbering::Decimal,
                    },
                    vec![3],
                ),
                (Role::ListItem, vec![4, 5]),
                (Role::Label, vec![]),
                (Role::ListBody, vec![]),
            ],
            &[(nodes[1], Owner::Element(5))],
        );
        // The first element is the document, whatever its role; the second is unreachable.
        let page = vec![
            Item::Tag(Tag::Content {
                node: nodes[1],
                order: 0,
                marker: true,
                call: false,
            }),
            rect(),
            content(nodes[1], 1),
            rect(),
        ];
        assert_eq!(tree(&structure, &[page], &[]), "c0.0 c0.1");
        let mut reachable = structure.clone();
        reachable.elements[0].children = vec![2];
        assert_eq!(
            tree(
                &reachable,
                &[vec![
                    Item::Tag(Tag::Content {
                        node: nodes[1],
                        order: 0,
                        marker: true,
                        call: false
                    }),
                    rect(),
                    content(nodes[1], 1),
                    rect(),
                ]],
                &[]
            ),
            "L[LI[Lbl[c0.0] LBody[c0.1]]]"
        );
    }

    #[test]
    fn puts_footnotes_into_notes_with_their_markers_as_labels() {
        let nodes = nodes();
        let structure = structure(
            vec![
                (Role::Document, vec![1]),
                (Role::Paragraph, vec![2]),
                (Role::Note, vec![3]),
                (Role::Label, vec![]),
            ],
            &[(nodes[1], Owner::Element(1)), (nodes[2], Owner::Element(2))],
        );
        let page = vec![
            content(nodes[1], 0),
            rect(),
            Item::Tag(Tag::Content {
                node: nodes[2],
                order: 1,
                marker: true,
                call: false,
            }),
            rect(),
            content(nodes[2], 2),
            rect(),
        ];
        assert_eq!(tree(&structure, &[page], &[]), "P[c0.0 Note[Lbl[c0.1] c0.2]]");
    }

    #[test]
    fn links_footnote_calls_and_markers() {
        let nodes = nodes();
        let mut structure = structure(
            vec![
                (Role::Document, vec![1]),
                (Role::Paragraph, vec![2, 3]),
                (Role::Link, vec![]),
                (Role::Note, vec![4]),
                (Role::Label, vec![5]),
                (Role::Link, vec![]),
            ],
            &[(nodes[1], Owner::Element(1)), (nodes[2], Owner::Element(3))],
        );
        structure.footnotes.insert(nodes[2], (2, 5));
        let part = |order: u32, marker: bool, call: bool| {
            Item::Tag(Tag::Content {
                node: nodes[2],
                order,
                marker,
                call,
            })
        };
        let page = vec![
            content(nodes[1], 0),
            rect(),
            part(1, false, true),
            rect(),
            content(nodes[1], 2),
            rect(),
            part(3, true, false),
            rect(),
            content(nodes[2], 4),
            rect(),
        ];
        // The call is a link to the note, the marker in its label one back.
        let links = [
            (0, Some(nodes[2]), LinkRole::FootnoteCall),
            (0, Some(nodes[2]), LinkRole::FootnoteMarker),
        ];
        assert_eq!(
            tree_with(&structure, std::slice::from_ref(&page), &links),
            "P[c0.0 Link[c0.1 a0.0] c0.2 Note[Lbl[Link[c0.3 a0.1]] c0.4]]"
        );
        // Without their annotations, the call and the marker are text of the elements around.
        assert_eq!(
            tree(&structure, &[page], &[]),
            "P[c0.0 c0.1 c0.2 Note[Lbl[c0.3] c0.4]]"
        );
    }

    #[test]
    fn marks_transparency_groups_as_a_whole() {
        let nodes = nodes();
        let structure = structure(
            vec![
                (Role::Document, vec![1, 2]),
                (Role::Paragraph, vec![]),
                (Role::Paragraph, vec![]),
            ],
            &[(nodes[1], Owner::Element(1)), (nodes[2], Owner::Element(2))],
        );
        let page = vec![
            Item::PushOpacity(0.5),
            content(nodes[1], 0),
            rect(),
            content(nodes[2], 1),
            rect(),
            Item::Pop,
            rect(),
            // Invisible content is no content.
            Item::PushOpacity(0.0),
            content(nodes[2], 3),
            rect(),
            Item::Pop,
        ];
        assert_eq!(tree(&structure, &[page], &[]), "P[c0.0] P[c0.1]");
    }

    #[test]
    fn tags_links_outside_the_structure() {
        let nodes = nodes();
        let structure = structure(
            vec![(Role::Document, vec![1]), (Role::Paragraph, vec![])],
            &[(nodes[1], Owner::Element(1)), (nodes[2], Owner::Artifact)],
        );
        let page = vec![content(nodes[1], 0), rect(), content(nodes[2], 1), rect()];
        assert_eq!(
            tree(
                &structure,
                &[page],
                &[(0, Some(nodes[1])), (0, Some(nodes[2])), (0, None)]
            ),
            "P[c0.0 Link[a0.0]] Link[a0.1] Link[a0.2]"
        );
    }
}

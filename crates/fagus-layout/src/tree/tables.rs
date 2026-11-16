//! The boxes of tables (CSS Tables 3, §3.9; CSS 2.1, §17.2.1): table, row group, row, cell,
//! caption and column elements, and the anonymous boxes Chrome generates where these are out of
//! place. Content in a table or row group goes into an anonymous row and cell, content in a row
//! into an anonymous cell, and table parts outside their proper parents into an anonymous table
//! (an inline table in an inline box), which the parts right after them join too.

use std::sync::Arc;

use fagus_dom::{ElementData, NodeId};
use fagus_style::ComputedStyle;
use fagus_style::values::{
    Appearance, Display, DisplayInside, DisplayOutside, InternalDisplay, LengthPercentage,
    LengthPercentageAuto, WhiteSpaceCollapse,
};

use super::{
    BoxId, BoxKind, BoxTree, Builder, Collected, ColumnBackground, InlineItem, LayoutBox, TableCell,
    process_white_space,
};

/// The style of a table cell, which has no margins (CSS 2.1, §8.3): they would widen its column
/// otherwise.
fn without_margins(style: &Arc<ComputedStyle>) -> Arc<ComputedStyle> {
    let zero = LengthPercentageAuto::Length(LengthPercentage::Px(0.0));
    let margin = &style.margin;
    if [&margin.top, &margin.right, &margin.bottom, &margin.left]
        .iter()
        .all(|side| **side == zero)
    {
        return Arc::clone(style);
    }
    let mut cell = (**style).clone();
    cell.margin.top = zero.clone();
    cell.margin.right = zero.clone();
    cell.margin.bottom = zero.clone();
    cell.margin.left = zero;
    Arc::new(cell)
}

/// The display of a caption's box: a block that establishes a formatting context.
const CAPTION: Display = Display::Normal {
    outside: DisplayOutside::Block,
    inside: DisplayInside::FlowRoot,
    list_item: false,
};

/// A box that belongs in a table, made by an element or pseudo-element with an internal table
/// display.
pub(super) enum TablePart {
    Caption(BoxId),
    /// The styles of the columns a column or column group makes, one per column, and those of
    /// them and their group that have a background, counted from the first.
    Columns(Vec<Arc<ComputedStyle>>, Vec<ColumnBackground>),
    Group(BoxId, GroupKind),
    Row(BoxId),
    Cell(TableCell),
}

/// The kind of a row group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GroupKind {
    Header,
    Body,
    Footer,
}

/// What the children being collected go into.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Parent {
    #[default]
    Block,
    Inline,
    /// A table, row group or row: table parts are collected as they are, for it to place.
    Table,
}

/// The anonymous boxes a table, row group or row holds open while its children come: the row
/// group new rows go into, the row new cells go into, and the content waiting for an anonymous
/// cell of that row.
#[derive(Default)]
pub(super) struct OpenParts {
    group: Option<BoxId>,
    row: Option<BoxId>,
    content: Vec<Collected>,
}

/// Which of the boxes that hold table parts a box is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Level {
    Table,
    Group,
    Row,
}

/// The style of an anonymous box in a table: the properties `parent` passes on, with `display`.
fn anonymous(parent: &ComputedStyle, display: Display) -> Arc<ComputedStyle> {
    Arc::new(ComputedStyle {
        display,
        ..parent.inherit()
    })
}

/// The `display` of a table laid out as a block, or placed on a line as an atomic inline.
const TABLE: Display = Display::Normal {
    outside: DisplayOutside::Block,
    inside: DisplayInside::Table,
    list_item: false,
};

/// White space in the sense of HTML: space, tab, line feed, form feed and carriage return.
const WHITE_SPACE: [char; 5] = [' ', '\t', '\n', '\r', '\u{c}'];

/// Whether a text is only white space, or empty.
fn is_white_space(text: &str) -> bool {
    text.chars().all(|character| WHITE_SPACE.contains(&character))
}

/// Whether a form control with an internal table display is laid out as an inline-block: one
/// with its native look is, as Chrome adjusts the display of widgets.
pub(super) fn widget_inline(style: &ComputedStyle) -> bool {
    style.appearance == Appearance::Auto && matches!(style.display, Display::Internal(_))
}

impl Builder<'_> {
    /// The box of a table element, with its children placed in it.
    pub(super) fn table(&self, tree: &mut BoxTree, element: NodeId, style: Arc<ComputedStyle>) -> BoxId {
        let children = self.collect_table_children(tree, element, &style);
        let id = tree.push(LayoutBox {
            style,
            node: Some(element),
            generated: false,
            kind: empty_table(),
        });
        self.assemble(tree, id, children);
        let end = self.string_log.borrow().len();
        tree.string_positions.insert(id, (end, end));
        id
    }

    /// The box of a row group element.
    /// The box of a row group element. Its named strings are assigned where the first cell of
    /// its first row starts, before the row's, whether that row is an element or anonymous.
    fn row_group(&self, tree: &mut BoxTree, element: NodeId, style: Arc<ComputedStyle>) -> BoxId {
        let children = self.collect_table_children(tree, element, &style);
        let id = tree.push(LayoutBox {
            style,
            node: Some(element),
            generated: false,
            kind: BoxKind::RowGroup { rows: Vec::new() },
        });
        self.assemble(tree, id, children);
        if self.strings.borrow().contains_key(&element)
            && let BoxKind::RowGroup { rows } = &tree.get(id).kind
            && let Some(BoxKind::Row { cells }) = rows.first().map(|&row| &tree.get(row).kind)
            && let Some(node) = cells.first().and_then(|cell| tree.get(cell.id).node)
        {
            tree.leading_strings.entry(node).or_default().insert(0, element);
        }
        id
    }

    /// The box of a table row element. `before` is where the string log stood before the row
    /// was entered: the strings of the row are assigned where its first cell starts.
    fn row(&self, tree: &mut BoxTree, element: NodeId, style: Arc<ComputedStyle>, before: usize) -> BoxId {
        let after = self.string_log.borrow().len();
        let children = self.collect_table_children(tree, element, &style);
        let id = tree.push(LayoutBox {
            style,
            node: Some(element),
            generated: false,
            kind: BoxKind::Row { cells: Vec::new() },
        });
        self.assemble(tree, id, children);
        if after > before
            && let BoxKind::Row { cells } = &tree.get(id).kind
            && let Some(node) = cells.first().and_then(|cell| tree.get(cell.id).node)
        {
            tree.leading_strings.entry(node).or_default().push(element);
        }
        tree.string_positions.insert(id, (before, after));
        id
    }

    /// The children of a table, row group or row element, with the table parts among them
    /// collected as they are.
    fn collect_table_children(
        &self,
        tree: &mut BoxTree,
        element: NodeId,
        style: &Arc<ComputedStyle>,
    ) -> Vec<Collected> {
        let outer = self.parent.replace(Parent::Table);
        let mut out = Vec::new();
        self.collect_children_with(tree, element, style, &mut out, None);
        self.parent.set(outer);
        out
    }

    /// Places the collected children of a table, row group or row in it.
    fn assemble(&self, tree: &mut BoxTree, parent: BoxId, children: Vec<Collected>) {
        let mut open = OpenParts::default();
        for item in children {
            match item {
                Collected::Part(part) => self.add_part(tree, parent, &mut open, part),
                item => self.add_content(tree, parent, &mut open, item),
            }
        }
        self.close_parts(tree, parent, &mut open);
    }

    /// The table part of an element with an internal table display.
    pub(super) fn table_part(
        &self,
        tree: &mut BoxTree,
        element: NodeId,
        style: &Arc<ComputedStyle>,
        strings_before: usize,
    ) -> TablePart {
        let Display::Internal(display) = style.display else {
            return TablePart::Columns(Vec::new(), Vec::new());
        };
        match display {
            InternalDisplay::TableCaption => {
                // A caption is a formatting context of its own: the margins of its content stay
                // inside it, as in Chrome.
                let caption_style = Arc::new(ComputedStyle {
                    display: CAPTION,
                    ..(**style).clone()
                });
                // The outside marker of a list item that starts with the table goes to the
                // table's first row, past the caption, as Chrome aligns it with the table.
                let marker = self.pending_marker.borrow_mut().take();
                let caption = self.block_container(tree, element, caption_style);
                if marker.is_some() {
                    *self.pending_marker.borrow_mut() = marker;
                }
                TablePart::Caption(caption)
            }
            InternalDisplay::TableColumnGroup => {
                let mut columns = Vec::new();
                // The group's background comes first: it is drawn below its columns'.
                let mut backgrounds = vec![ColumnBackground {
                    start: 0,
                    span: 0,
                    style: Arc::clone(style),
                }];
                for column in self.document.child_elements(element) {
                    if let Some(column_style) = self.styled.style(column)
                        && column_style.display == Display::Internal(InternalDisplay::TableColumn)
                    {
                        let span = self.span_attribute(column, "span", 1, 1000);
                        backgrounds.push(ColumnBackground {
                            start: columns.len(),
                            span,
                            style: Arc::clone(column_style),
                        });
                        columns.extend(std::iter::repeat_n(Arc::clone(column_style), span));
                    }
                }
                if columns.is_empty() {
                    let span = self.span_attribute(element, "span", 1, 1000);
                    columns.extend(std::iter::repeat_n(Arc::clone(style), span));
                }
                backgrounds[0].span = columns.len();
                backgrounds.retain(|background| has_background(&background.style));
                TablePart::Columns(columns, backgrounds)
            }
            InternalDisplay::TableColumn => {
                let span = self.span_attribute(element, "span", 1, 1000);
                let backgrounds = has_background(style)
                    .then(|| ColumnBackground {
                        start: 0,
                        span,
                        style: Arc::clone(style),
                    })
                    .into_iter()
                    .collect();
                TablePart::Columns(
                    std::iter::repeat_n(Arc::clone(style), span).collect(),
                    backgrounds,
                )
            }
            InternalDisplay::TableRowGroup
            | InternalDisplay::TableHeaderGroup
            | InternalDisplay::TableFooterGroup => {
                let kind = match display {
                    InternalDisplay::TableHeaderGroup => GroupKind::Header,
                    InternalDisplay::TableFooterGroup => GroupKind::Footer,
                    _ => GroupKind::Body,
                };
                TablePart::Group(self.row_group(tree, element, Arc::clone(style)), kind)
            }
            InternalDisplay::TableRow => {
                TablePart::Row(self.row(tree, element, Arc::clone(style), strings_before))
            }
            InternalDisplay::TableCell => {
                // Only HTML's cells span rows and columns.
                let html_cell = self
                    .document
                    .element(element)
                    .is_some_and(|data| data.is("td") || data.is("th"));
                let (colspan, rowspan) = if html_cell {
                    (
                        self.span_attribute(element, "colspan", 1, 1000).max(1),
                        self.span_attribute(element, "rowspan", 1, 65534),
                    )
                } else {
                    (1, 1)
                };
                TablePart::Cell(TableCell {
                    id: self.block_container(tree, element, without_margins(style)),
                    colspan,
                    rowspan,
                })
            }
        }
    }

    /// The table part of a `::before` or `::after` with an internal table display, holding its
    /// content. Content in a row or row group goes into an anonymous cell.
    pub(super) fn pseudo_part(
        &self,
        tree: &mut BoxTree,
        style: &Arc<ComputedStyle>,
        display: InternalDisplay,
        content: Vec<InlineItem>,
    ) -> TablePart {
        let generated = |tree: &mut BoxTree, style: Arc<ComputedStyle>, kind: BoxKind| {
            tree.push(LayoutBox {
                style,
                node: None,
                generated: true,
                kind,
            })
        };
        let in_cell = |builder: &Self, tree: &mut BoxTree, parent: BoxId, content: Vec<InlineItem>| {
            let mut open = OpenParts::default();
            for item in content {
                builder.add_content(tree, parent, &mut open, Collected::Inline(item));
            }
            builder.close_parts(tree, parent, &mut open);
        };
        match display {
            InternalDisplay::TableCell => TablePart::Cell(TableCell {
                id: generated(
                    tree,
                    without_margins(style),
                    BoxKind::Inline {
                        items: process_white_space(content),
                    },
                ),
                colspan: 1,
                rowspan: 1,
            }),
            InternalDisplay::TableCaption => {
                // A caption is a formatting context of its own: the margins of its content stay
                // inside it, as in Chrome.
                let caption_style = Arc::new(ComputedStyle {
                    display: CAPTION,
                    ..(**style).clone()
                });
                TablePart::Caption(generated(
                    tree,
                    caption_style,
                    BoxKind::Inline {
                        items: process_white_space(content),
                    },
                ))
            }
            InternalDisplay::TableColumn | InternalDisplay::TableColumnGroup => {
                let backgrounds = has_background(style)
                    .then(|| ColumnBackground {
                        start: 0,
                        span: 1,
                        style: Arc::clone(style),
                    })
                    .into_iter()
                    .collect();
                TablePart::Columns(vec![Arc::clone(style)], backgrounds)
            }
            InternalDisplay::TableRow => {
                let row = generated(tree, Arc::clone(style), BoxKind::Row { cells: Vec::new() });
                in_cell(self, tree, row, content);
                TablePart::Row(row)
            }
            InternalDisplay::TableRowGroup
            | InternalDisplay::TableHeaderGroup
            | InternalDisplay::TableFooterGroup => {
                let group = generated(tree, Arc::clone(style), BoxKind::RowGroup { rows: Vec::new() });
                in_cell(self, tree, group, content);
                let kind = match display {
                    InternalDisplay::TableHeaderGroup => GroupKind::Header,
                    InternalDisplay::TableFooterGroup => GroupKind::Footer,
                    _ => GroupKind::Body,
                };
                TablePart::Group(group, kind)
            }
        }
    }

    /// Adds a table part to the children collected in `out`, whose parent has `parent_style`:
    /// as it is if they are a table's, a row group's or a row's, or else to the anonymous table
    /// the part right before it went to, or a new one, an inline table if the parent is an
    /// inline box (CSS Tables 3, §3.9.1).
    pub(super) fn add_to_parent(
        &self,
        tree: &mut BoxTree,
        parent_style: &Arc<ComputedStyle>,
        out: &mut Vec<Collected>,
        part: TablePart,
    ) {
        match self.parent.get() {
            Parent::Table => out.push(Collected::Part(part)),
            parent => {
                let parent_style = self.box_parent_style(parent_style);
                self.add_to_anonymous_table(tree, &parent_style, parent == Parent::Inline, out, part);
            }
        }
    }

    /// The style of the box that the children of an element with `style` go into: its own, or,
    /// for a `display: contents` element whose children are being collected, that of the box
    /// around it.
    pub(super) fn box_parent_style(&self, style: &Arc<ComputedStyle>) -> Arc<ComputedStyle> {
        self.contents_parents
            .borrow()
            .iter()
            .rev()
            .find(|(contents, _)| Arc::ptr_eq(contents, style))
            .map_or_else(|| Arc::clone(style), |(_, parent)| Arc::clone(parent))
    }

    /// Adds a table part to the anonymous table at the end of `out`, or to a new one.
    fn add_to_anonymous_table(
        &self,
        tree: &mut BoxTree,
        parent_style: &ComputedStyle,
        inline: bool,
        out: &mut Vec<Collected>,
        part: TablePart,
    ) {
        let table = match out.last() {
            Some(Collected::Block(id) | Collected::Inline(InlineItem::Atomic(id)))
                if self.anonymous_tables.borrow().contains_key(id) =>
            {
                *id
            }
            _ => {
                let id = tree.push(LayoutBox {
                    style: anonymous(parent_style, TABLE),
                    node: None,
                    generated: false,
                    kind: empty_table(),
                });
                if inline {
                    out.push(Collected::Inline(InlineItem::Atomic(id)));
                } else {
                    if let Some(marker) = self.marker_here(out) {
                        *self.pending_marker.borrow_mut() = Some(marker);
                    }
                    out.push(Collected::Block(id));
                }
                id
            }
        };
        let mut open = self
            .anonymous_tables
            .borrow_mut()
            .remove(&table)
            .unwrap_or_default();
        self.add_part(tree, table, &mut open, part);
        self.anonymous_tables.borrow_mut().insert(table, open);
        let end = self.string_log.borrow().len();
        tree.string_positions.insert(table, (end, end));
    }

    /// Whether a text child is left out, as Chrome leaves it out: white space directly in a
    /// table, row group or row, unless it follows text that does not end in white space, and
    /// collapsible white space right after a table part that went into an anonymous table.
    pub(super) fn drops_white_space(&self, out: &[Collected], text: &str, style: &ComputedStyle) -> bool {
        if !is_white_space(text) {
            return false;
        }
        let preserves_breaks = style.white_space.collapse != WhiteSpaceCollapse::Collapse;
        let previous = out.iter().rev().find(|item| {
            !matches!(
                item,
                Collected::Inline(InlineItem::OutOfFlow(_) | InlineItem::Float(_))
            )
        });
        if self.parent.get() == Parent::Table {
            return !matches!(
                previous,
                Some(Collected::Inline(InlineItem::Text { text, .. }))
                    if preserves_breaks || !text.ends_with(WHITE_SPACE)
            );
        }
        !preserves_breaks
            && matches!(
                previous,
                Some(Collected::Block(id) | Collected::Inline(InlineItem::Atomic(id)))
                    if self.anonymous_tables.borrow().contains_key(id)
            )
    }

    /// Whether `child` is a `<form>` in a table, row group or row, where the HTML parser leaves
    /// forms empty and Chrome gives them no box.
    pub(super) fn is_demoted_form(&self, parent: NodeId, child: &ElementData) -> bool {
        child.is("form")
            && self.document.element(parent).is_some_and(|data| {
                ["table", "thead", "tbody", "tfoot", "tr"]
                    .iter()
                    .any(|name| data.is(name))
            })
    }

    /// Adds a table part to a table, row group or row, in the anonymous boxes it misses there.
    fn add_part(&self, tree: &mut BoxTree, parent: BoxId, open: &mut OpenParts, part: TablePart) {
        match (level(tree, parent), part) {
            (
                Level::Table,
                part @ (TablePart::Caption(_) | TablePart::Columns(..) | TablePart::Group(..)),
            ) => {
                self.close_parts(tree, parent, open);
                let BoxKind::Table {
                    header,
                    bodies,
                    footer,
                    captions,
                    columns,
                    column_backgrounds,
                } = &mut tree.box_mut(parent).kind
                else {
                    return;
                };
                match part {
                    TablePart::Caption(id) => captions.push(id),
                    TablePart::Columns(styles, backgrounds) => {
                        let first = columns.len();
                        column_backgrounds.extend(backgrounds.into_iter().map(|background| {
                            ColumnBackground {
                                start: background.start + first,
                                ..background
                            }
                        }));
                        columns.extend(styles);
                    }
                    TablePart::Group(id, GroupKind::Header) if header.is_none() => *header = Some(id),
                    TablePart::Group(id, GroupKind::Footer) if footer.is_none() => *footer = Some(id),
                    TablePart::Group(id, _) => bodies.push(id),
                    TablePart::Row(_) | TablePart::Cell(_) => {}
                }
            }
            (Level::Table, part) => {
                let group = self.open_group(tree, parent, open);
                self.add_part(tree, group, open, part);
            }
            (Level::Group, TablePart::Row(id)) => {
                self.close_parts(tree, parent, open);
                if let BoxKind::RowGroup { rows } = &mut tree.box_mut(parent).kind {
                    rows.push(id);
                }
            }
            (Level::Group, part) => {
                let row = self.open_row(tree, parent, open);
                self.add_part(tree, row, open, part);
            }
            (Level::Row, TablePart::Cell(cell)) => {
                self.close_parts(tree, parent, open);
                if let BoxKind::Row { cells } = &mut tree.box_mut(parent).kind {
                    cells.push(cell);
                }
            }
            // A part out of place in a row goes into an anonymous table in an anonymous cell.
            (Level::Row, part) => {
                let parent_style = Arc::clone(&tree.get(parent).style);
                self.add_to_anonymous_table(tree, &parent_style, false, &mut open.content, part);
            }
        }
    }

    /// Adds content other than table parts to a table, row group or row: it goes into an
    /// anonymous cell, with the content right before and after it.
    fn add_content(&self, tree: &mut BoxTree, parent: BoxId, open: &mut OpenParts, item: Collected) {
        match level(tree, parent) {
            Level::Table => {
                let group = self.open_group(tree, parent, open);
                self.open_row(tree, group, open);
            }
            Level::Group => {
                self.open_row(tree, parent, open);
            }
            Level::Row => {}
        }
        open.content.push(item);
    }

    /// Closes what a table, row group or row holds open: the content waiting for a cell goes
    /// into one.
    fn close_parts(&self, tree: &mut BoxTree, parent: BoxId, open: &mut OpenParts) {
        let level = level(tree, parent);
        let row = if level == Level::Row {
            Some(parent)
        } else {
            open.row
        };
        if let Some(row) = row
            && !open.content.is_empty()
        {
            let content = std::mem::take(&mut open.content);
            let cell = self.anonymous_cell(tree, row, content);
            if let BoxKind::Row { cells } = &mut tree.box_mut(row).kind {
                cells.push(TableCell {
                    id: cell,
                    colspan: 1,
                    rowspan: 1,
                });
            }
        }
        if level != Level::Row {
            open.row = None;
        }
        if level == Level::Table {
            open.group = None;
        }
    }

    /// The anonymous row group of a table that rows go into, made if there is none.
    fn open_group(&self, tree: &mut BoxTree, table: BoxId, open: &mut OpenParts) -> BoxId {
        if let Some(group) = open.group {
            return group;
        }
        let style = anonymous(
            &tree.get(table).style,
            Display::Internal(InternalDisplay::TableRowGroup),
        );
        let group = tree.push(LayoutBox {
            style,
            node: None,
            generated: false,
            kind: BoxKind::RowGroup { rows: Vec::new() },
        });
        if let BoxKind::Table { bodies, .. } = &mut tree.box_mut(table).kind {
            bodies.push(group);
        }
        open.group = Some(group);
        group
    }

    /// The anonymous row of a row group that cells go into, made if there is none.
    fn open_row(&self, tree: &mut BoxTree, group: BoxId, open: &mut OpenParts) -> BoxId {
        if let Some(row) = open.row {
            return row;
        }
        let style = anonymous(
            &tree.get(group).style,
            Display::Internal(InternalDisplay::TableRow),
        );
        let row = tree.push(LayoutBox {
            style,
            node: None,
            generated: false,
            kind: BoxKind::Row { cells: Vec::new() },
        });
        if let BoxKind::RowGroup { rows } = &mut tree.box_mut(group).kind {
            rows.push(row);
        }
        open.row = Some(row);
        row
    }

    /// An anonymous cell of `row` holding `content`.
    fn anonymous_cell(&self, tree: &mut BoxTree, row: BoxId, content: Vec<Collected>) -> BoxId {
        let style = anonymous(
            &tree.get(row).style,
            Display::Internal(InternalDisplay::TableCell),
        );
        let has_blocks = content.iter().any(|item| matches!(item, Collected::Block(_)));
        let kind = if has_blocks {
            BoxKind::Block {
                children: self.block_children(tree, &style, content),
            }
        } else {
            BoxKind::Inline {
                items: self.inline_content(tree, content),
            }
        };
        let id = tree.push(LayoutBox {
            style,
            node: None,
            generated: false,
            kind,
        });
        if !has_blocks && let Some(first_line) = self.pending_first_line.borrow_mut().take() {
            tree.first_lines.insert(id, first_line);
        }
        id
    }
}

/// A table without parts yet.
fn empty_table() -> BoxKind {
    BoxKind::Table {
        header: None,
        bodies: Vec::new(),
        footer: None,
        captions: Vec::new(),
        columns: Vec::new(),
        column_backgrounds: Vec::new(),
    }
}

/// Whether a box paints a background: a color or an image.
fn has_background(style: &ComputedStyle) -> bool {
    !style.background_color.is_transparent()
        || style
            .background_image
            .iter()
            .any(|image| !matches!(image, fagus_style::values::Image::None))
}

/// Which of the boxes that hold table parts `id` is: a table, a row group, or a row.
fn level(tree: &BoxTree, id: BoxId) -> Level {
    match tree.get(id).kind {
        BoxKind::Table { .. } => Level::Table,
        BoxKind::RowGroup { .. } => Level::Group,
        _ => Level::Row,
    }
}

#[cfg(test)]
mod tests {
    use fagus_css::{MediaEnvironment, Origin, RuleSet, Stylesheet};
    use fagus_dom::Document;

    use super::super::{BoxId, BoxKind, BoxTree, InlineItem};
    use crate::image::Images;

    /// The box tree of the body of an HTML document, written as `kind[children]`, with `~` before
    /// anonymous boxes and the text of inline content in parentheses.
    fn body(html: &str) -> String {
        let tree = build(html);
        let root = tree.root.expect("root box");
        let body = tree.children(root).into_iter().last().expect("body box");
        content(&tree, body)
    }

    /// The box tree of an HTML document.
    fn build(html: &str) -> BoxTree {
        let document = Document::parse_html(html);
        let ua = Stylesheet::parse(fagus_style::USER_AGENT_CSS, Origin::UserAgent, "ua.css");
        let css: String = document
            .descendants(fagus_dom::NodeId::DOCUMENT)
            .filter(|&node| document.element(node).is_some_and(|data| data.is("style")))
            .map(|node| document.text_content(node))
            .collect();
        let author = Stylesheet::parse(&css, Origin::Author, "style");
        let environment = MediaEnvironment::default();
        let rules = RuleSet::new(&[&ua, &author], &environment);
        let styled = fagus_style::compute_styles(&document, &rules, (environment.width, environment.height));
        BoxTree::build(&document, &styled, &Images::default())
    }

    /// The first table in a box tree.
    fn first_table(tree: &BoxTree) -> BoxId {
        let mut stack: Vec<BoxId> = tree.root.into_iter().collect();
        while let Some(id) = stack.pop() {
            if matches!(tree.get(id).kind, BoxKind::Table { .. }) {
                return id;
            }
            stack.extend(tree.children(id).into_iter().rev());
        }
        panic!("no table");
    }

    fn describe(tree: &BoxTree, id: BoxId) -> String {
        let layout_box = tree.get(id);
        let anonymous = if layout_box.node.is_none() && !layout_box.generated {
            "~"
        } else {
            ""
        };
        let list = |ids: &mut dyn Iterator<Item = BoxId>| {
            ids.map(|id| describe(tree, id)).collect::<Vec<_>>().join(" ")
        };
        match &layout_box.kind {
            BoxKind::Table {
                header,
                bodies,
                footer,
                captions,
                ..
            } => {
                let captions = captions
                    .iter()
                    .map(|&id| format!("caption({})", content(tree, id)));
                let groups = header
                    .iter()
                    .chain(bodies)
                    .chain(footer)
                    .map(|&id| describe(tree, id));
                let parts: Vec<String> = captions.chain(groups).collect();
                format!("{anonymous}table[{}]", parts.join(" "))
            }
            BoxKind::RowGroup { rows } => format!("{anonymous}group[{}]", list(&mut rows.iter().copied())),
            BoxKind::Row { cells } => {
                let cells: Vec<String> = cells
                    .iter()
                    .map(|cell| {
                        let cell_box = tree.get(cell.id);
                        let anonymous = if cell_box.node.is_none() && !cell_box.generated {
                            "~"
                        } else {
                            ""
                        };
                        format!("{anonymous}cell({})", content(tree, cell.id))
                    })
                    .collect();
                format!("{anonymous}row[{}]", cells.join(" "))
            }
            BoxKind::Block { .. } | BoxKind::Inline { .. } => {
                format!("{anonymous}block({})", content(tree, id))
            }
            _ => "other".to_owned(),
        }
    }

    /// The content of a block container: its children, or its text and atomic inlines.
    fn content(tree: &BoxTree, id: BoxId) -> String {
        match &tree.get(id).kind {
            BoxKind::Block { children } => children
                .iter()
                .map(|&id| describe(tree, id))
                .collect::<Vec<_>>()
                .join(" "),
            BoxKind::Inline { items } => items
                .iter()
                .map(|item| match item {
                    InlineItem::Text { text, .. } => text.clone(),
                    InlineItem::Atomic(id) => describe(tree, *id),
                    _ => String::new(),
                })
                .collect(),
            _ => describe(tree, id),
        }
    }

    const CELL: &str = "display: table-cell";

    #[test]
    fn keeps_the_backgrounds_of_columns_and_column_groups() {
        let tree = build(
            "<table><colgroup style='background: red'><col><col span=2 style='background: blue'></colgroup>\
             <colgroup span=2 style='background: lime'></colgroup><col><tr><td>A</td></tr></table>",
        );
        let BoxKind::Table {
            columns,
            column_backgrounds,
            ..
        } = &tree.get(first_table(&tree)).kind
        else {
            panic!("a table");
        };
        assert_eq!(columns.len(), 6);
        let parts: Vec<(usize, usize, bool)> = column_backgrounds
            .iter()
            .map(|part| (part.start, part.span, part.style.background_color.g > 0.5))
            .collect();
        // The group comes before its columns; columns without a background are left out.
        assert_eq!(parts, [(0, 3, false), (1, 2, false), (3, 2, true)]);
    }

    #[test]
    fn styles_anonymous_tables_after_the_box_around_display_contents() {
        let tree = build(&format!(
            "<div style='border-spacing: 2px'><div style='display: contents; border-spacing: 12px'>\
             <div style='{CELL}'>A</div></div></div>"
        ));
        let table = tree.get(first_table(&tree));
        assert_eq!(table.node, None);
        assert!((table.style.border_spacing.0 - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn wraps_content_of_a_table_in_an_anonymous_row_and_cell() {
        assert_eq!(
            body("<div style='display: table'>text <b>only</b></div>"),
            "table[~group[~row[~cell(text only)]]]"
        );
        assert_eq!(
            body(&format!(
                "<div style='display: table'><div style='{CELL}'>A</div>\n<div style='display: table-row'><div style='{CELL}'>B</div></div></div>"
            )),
            "table[~group[~row[cell(A)] row[cell(B)]]]"
        );
    }

    #[test]
    fn wraps_cells_outside_tables_in_one_anonymous_table() {
        // The white space between the cells goes; text, or an absolutely positioned box, ends
        // the table.
        assert_eq!(
            body(&format!(
                "<div style='{CELL}'>A</div>\n  <div style='{CELL}'>B</div>text<div style='{CELL}'>C</div>"
            )),
            "~table[~group[~row[cell(A) cell(B)]]] ~block(text) ~table[~group[~row[cell(C)]]]"
        );
        assert_eq!(
            body(&format!(
                "<div style='display: table-row'><div style='{CELL}'>A</div></div><div style='display: table-row-group'></div>"
            )),
            "~table[~group[row[cell(A)]] group[]]"
        );
    }

    #[test]
    fn makes_inline_tables_in_inline_boxes() {
        assert_eq!(
            body(&format!(
                "x <span><span style='{CELL}'>A</span> <span style='{CELL}'>B</span></span> y"
            )),
            "x ~table[~group[~row[cell(A) cell(B)]]] y"
        );
    }

    #[test]
    fn wraps_other_content_of_a_row_in_one_anonymous_cell() {
        assert_eq!(
            body(&format!(
                "<div style='display: table-row'><div style='{CELL}'>A</div>t<b>u</b> <i>v</i><div style='{CELL}'>B</div></div>"
            )),
            "~table[~group[row[cell(A) ~cell(tuv) cell(B)]]]"
        );
        // A caption out of place goes into an anonymous table in that cell.
        assert_eq!(
            body("<div style='display: table-row'><div style='display: table-caption'>C</div></div>"),
            "~table[~group[row[~cell(~table[caption(C)])]]]"
        );
    }

    #[test]
    fn places_pseudo_elements_and_contents_children_in_tables() {
        assert_eq!(
            body(&format!(
                "<style>.t::before {{ content: 'pre'; {CELL} }} .t::after {{ content: 'post' }}</style>
                 <div class=t style='display: table'><div style='display: contents'>
                 <div style='{CELL}'>A</div> </div><div style='{CELL}'>B</div></div>"
            )),
            "table[~group[~row[cell(pre) cell(A) cell(B) ~cell(post)]]]"
        );
    }

    #[test]
    fn leaves_out_forms_and_white_space_in_html_tables() {
        // The HTML parser moves text out of the table, and leaves the form empty in it.
        assert_eq!(
            body("<table>text<form></form> <tr><td>A</td> </tr></table>"),
            "~block(text) table[group[row[cell(A)]]]"
        );
    }

    #[test]
    fn lays_out_form_controls_with_table_displays_as_inline_blocks() {
        assert_eq!(
            body(&format!(
                "<div><input style='{CELL}'><div style='{CELL}'>B</div></div>"
            )),
            // The input is on a line of its own, before the table of the cell.
            "block(~block(other) ~table[~group[~row[cell(B)]]])"
        );
    }
}

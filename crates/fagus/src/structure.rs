//! The logical structure of a document for a tagged PDF: what its elements are to a reader.
//!
//! The structure follows the elements in document order. Elements with meaning of their own
//! (headings, paragraphs, lists, tables, images, links) become structure elements; other blocks
//! become paragraphs when they hold only text and divisions when they hold blocks; inline elements
//! without meaning add their text to the element around them.

use fagus_dom::{Document, ElementData, NodeId};
use fagus_pdf::{Element, Numbering, Owner, Repeat, Role, Scope, Structure};
use fagus_style::values::{Display, DisplayInside, InternalDisplay, ListStyleType, Position};
use fagus_style::{ComputedStyle, StyledDocument};
use rustc_hash::FxHashSet;

/// The structure of a document, with what a check for PDF/UA needs to know about it.
pub(crate) struct Built {
    pub structure: Structure,
    /// The images that have no alternative text.
    pub without_alt: Vec<NodeId>,
    /// The headings that readers meet, in document order, with their levels.
    pub headings: Vec<(NodeId, u8)>,
}

/// The structure of a document whose controls of `fields` are the fields of an interactive
/// form.
pub(crate) fn build(document: &Document, styled: &StyledDocument, fields: &FxHashSet<NodeId>) -> Built {
    let root = document.document_element();
    let language = root
        .and_then(|root| document.element(root))
        .and_then(|root| root.attribute("lang"))
        .map(|lang| lang.trim().to_owned())
        .filter(|lang| !lang.is_empty());
    let mut builder = Builder {
        document,
        styled,
        fields,
        structure: Structure::default(),
        without_alt: Vec::new(),
        headings: Vec::new(),
    };
    let mut element = Element::new(Role::Document);
    element.language.clone_from(&language);
    builder.structure.elements.push(element);
    if let Some(root) = root {
        let context = Context {
            language: language.as_deref(),
            ..Context::default()
        };
        builder.children(root, 0, &context);
    }
    Built {
        structure: builder.structure,
        without_alt: builder.without_alt,
        headings: builder.headings,
    }
}

/// What surrounds an element.
#[derive(Clone, Copy, Default)]
struct Context<'a> {
    /// The language of the text around.
    language: Option<&'a str>,
    /// The parent is a list.
    list: bool,
    /// The element is in a table: one with data, or one for layout only.
    table: Option<Table>,
    /// The element is in the header group of a data table.
    head: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Table {
    Data,
    Layout,
}

struct Builder<'a> {
    document: &'a Document,
    styled: &'a StyledDocument,
    /// The controls that are fields: form fields of the structure, with their widgets.
    fields: &'a FxHashSet<NodeId>,
    structure: Structure,
    without_alt: Vec<NodeId>,
    headings: Vec<(NodeId, u8)>,
}

impl<'a> Builder<'a> {
    fn children(&mut self, node: NodeId, parent: usize, context: &Context<'a>) {
        for child in self.document.children(node) {
            self.element(child, parent, context);
        }
    }

    fn element(&mut self, node: NodeId, parent: usize, context: &Context<'a>) {
        let (Some(data), Some(style)) = (self.document.element(node), self.styled.style(node)) else {
            return;
        };
        if style.display == Display::None {
            return;
        }
        // Running elements show in margin boxes, which a reader skips; hidden elements are not
        // for readers at all.
        if style.running.is_some()
            || data
                .attribute("aria-hidden")
                .is_some_and(|value| value.trim() == "true")
        {
            self.hide(node);
            return;
        }
        let lang = data
            .attribute("lang")
            .map(str::trim)
            .filter(|lang| Some(*lang) != context.language);
        let mut inner = Context {
            language: lang.or(context.language),
            list: false,
            ..*context
        };
        let presentation = data
            .attribute("role")
            .is_some_and(|role| matches!(role.trim(), "presentation" | "none"));
        let aria_heading = crate::interactive::aria_heading(data);
        let role = match data.local_name().as_ref() {
            "html" | "body" => None,
            "img" => {
                let alt = data
                    .attribute("alt")
                    .or_else(|| data.attribute("aria-label"))
                    .map(normalize);
                if presentation || alt.as_deref() == Some("") {
                    self.hide(node);
                    return;
                }
                if alt.is_none() {
                    self.without_alt.push(node);
                }
                Some(Role::Figure { alt })
            }
            // An inline SVG is a figure named by its label or its `<title>`, drawn as one image.
            "svg" if data.is_svg("svg") => {
                let alt = data.attribute("aria-label").map(normalize).or_else(|| {
                    self.document
                        .child_elements(node)
                        .find(|&child| {
                            self.document
                                .element(child)
                                .is_some_and(|child| child.is_svg("title"))
                        })
                        .map(|title| normalize(&self.document.text_content(title)))
                });
                if presentation || alt.as_deref() == Some("") {
                    self.hide(node);
                    return;
                }
                if alt.is_none() {
                    self.without_alt.push(node);
                }
                Some(Role::Figure { alt })
            }
            // A formula is read as a whole: its alternative text is its `alttext` or its TeX.
            "math" if &*data.name.ns == "http://www.w3.org/1998/Math/MathML" => {
                if presentation {
                    self.hide(node);
                    return;
                }
                let alt = fagus_layout::formula_alternative_text(self.document, node);
                if alt.is_none() {
                    self.without_alt.push(node);
                }
                Some(Role::Formula { alt })
            }
            _ if self.fields.contains(&node) => Some(Role::Form),
            _ if presentation => None,
            name @ ("h1" | "h2" | "h3" | "h4" | "h5" | "h6") => {
                let level = name.as_bytes()[1] - b'0';
                self.headings.push((node, level));
                Some(Role::Heading {
                    level,
                    title: normalize(&self.document.text_content(node)),
                })
            }
            // An element with the ARIA role `heading` is a heading too, as in Chrome.
            _ if aria_heading.is_some() => {
                let level = aria_heading.unwrap_or(2);
                self.headings.push((node, level));
                Some(Role::Heading {
                    level,
                    title: normalize(&self.document.text_content(node)),
                })
            }
            "p" => Some(Role::Paragraph),
            "ul" | "ol" | "menu" => {
                inner.list = true;
                Some(Role::List {
                    numbering: numbering(&style.list_style_type),
                })
            }
            "li" if context.list => {
                self.list_item(node, parent, style, &inner, lang);
                return;
            }
            // A footnote is a note where it is called, its marker a label (PDF/UA-1, §7.9). Its
            // call is a link to it, its marker one back to the call.
            _ if style.footnote => {
                let call = self.push(parent, Element::new(Role::Link));
                let mut note = Element::new(Role::Note);
                note.language = lang.map(str::to_owned);
                let note = self.push(parent, note);
                let label = self.push(note, Element::new(Role::Label));
                let back = self.push(label, Element::new(Role::Link));
                self.structure.footnotes.insert(node, (call, back));
                self.own(node, note);
                self.children(node, note, &inner);
                return;
            }
            "table" => {
                if self.is_data_table(node, data) {
                    inner.table = Some(Table::Data);
                    inner.head = false;
                    Some(Role::Table {
                        summary: data
                            .attribute("summary")
                            .map(normalize)
                            .filter(|summary| !summary.is_empty()),
                    })
                } else {
                    inner.table = Some(Table::Layout);
                    Some(Role::Division)
                }
            }
            "thead" | "tbody" | "tfoot" | "tr" if context.table == Some(Table::Layout) => None,
            "thead" if context.table == Some(Table::Data) => {
                inner.head = true;
                Some(Role::TableHead)
            }
            "tbody" if context.table == Some(Table::Data) => Some(Role::TableBody),
            "tfoot" if context.table == Some(Table::Data) => Some(Role::TableFoot),
            "tr" if context.table == Some(Table::Data) => Some(Role::TableRow),
            "th" if context.table == Some(Table::Data) => Some(Role::TableHeader {
                scope: match data.attribute("scope").map(str::trim) {
                    Some("row" | "rowgroup") => Scope::Row,
                    Some("col" | "colgroup") => Scope::Column,
                    _ if context.head => Scope::Column,
                    _ => Scope::Row,
                },
                rows: span(data, "rowspan", 65_534),
                columns: span(data, "colspan", 1000),
            }),
            "td" if context.table == Some(Table::Data) => Some(Role::TableCell {
                rows: span(data, "rowspan", 65_534),
                columns: span(data, "colspan", 1000),
            }),
            "caption" if context.table == Some(Table::Data) => Some(Role::Caption),
            "figcaption" => Some(Role::Caption),
            "blockquote" => Some(Role::BlockQuote),
            "article" => Some(Role::Article),
            "section" => Some(Role::Section),
            "a" if data.attribute("href").is_some() => Some(Role::Link),
            "q" => Some(Role::Quote),
            "code" | "kbd" | "samp" => Some(Role::Code),
            "em" => Some(Role::Emphasis),
            "strong" => Some(Role::Strong),
            "abbr"
                if data
                    .attribute("title")
                    .is_some_and(|title| !title.trim().is_empty()) =>
            {
                Some(Role::Abbreviation {
                    expansion: normalize(data.attribute("title").unwrap_or_default()),
                })
            }
            _ => self.generic(node, style),
        };
        // Fixed boxes repeat on every page and table headers and footers on every page of their
        // table: only one occurrence is content.
        let repeat = if style.position == Position::Fixed {
            Repeat::First
        } else {
            match style.display {
                Display::Internal(InternalDisplay::TableHeaderGroup) if role.is_some() => Repeat::First,
                Display::Internal(InternalDisplay::TableFooterGroup) if role.is_some() => Repeat::Last,
                _ => Repeat::Never,
            }
        };
        let role = match role {
            Some(role) => Some(role),
            None if repeat != Repeat::Never => Some(Role::Division),
            None if lang.is_some() && !matches!(data.local_name().as_ref(), "html" | "body") => {
                Some(Role::Span)
            }
            None => None,
        };
        let Some(role) = role else {
            // The element's content belongs to the element around it.
            self.own(node, parent);
            self.children(node, parent, &inner);
            return;
        };
        let mut element = Element::new(role);
        element.language = lang.map(str::to_owned);
        element.repeat = repeat;
        let index = self.push(parent, element);
        self.own(node, index);
        // The content of an inline SVG is part of its picture, that of a formula part of the
        // formula.
        if matches!(
            self.structure.elements[index].role,
            Role::Formula { .. } | Role::Form
        ) {
            for descendant in self.document.descendants(node) {
                if self.document.element(descendant).is_some() {
                    self.own(descendant, index);
                }
            }
        } else if !data.is_svg("svg") {
            self.children(node, index, &inner);
        }
    }

    /// A list item: its marker is a label, everything else the body.
    fn list_item(
        &mut self,
        node: NodeId,
        parent: usize,
        style: &ComputedStyle,
        context: &Context<'a>,
        lang: Option<&str>,
    ) {
        let mut item = Element::new(Role::ListItem);
        item.language = lang.map(str::to_owned);
        if style.position == Position::Fixed {
            item.repeat = Repeat::First;
        }
        let item = self.push(parent, item);
        self.push(item, Element::new(Role::Label));
        let body = self.push(item, Element::new(Role::ListBody));
        self.own(node, body);
        self.children(node, body, context);
    }

    /// The role of an element without a meaning of its own: a paragraph for a block with only
    /// text, a division for a block with blocks, none for inline content.
    fn generic(&self, node: NodeId, style: &ComputedStyle) -> Option<Role> {
        if style.display == Display::Contents || !self.is_block(node, style) {
            return None;
        }
        let has_blocks = self.document.children(node).any(|child| {
            self.styled.style(child).is_some_and(|child_style| {
                child_style.display != Display::None && self.is_block_in(child_style, style)
            })
        });
        Some(if has_blocks {
            Role::Division
        } else {
            Role::Paragraph
        })
    }

    /// Whether an element's box is a block (or laid out like one).
    fn is_block(&self, node: NodeId, style: &ComputedStyle) -> bool {
        let parent = self
            .document
            .parent_element(node)
            .and_then(|parent| self.styled.style(parent));
        match parent {
            Some(parent) => self.is_block_in(style, parent),
            None => true,
        }
    }

    /// Whether a box with `style` inside a box with `parent` is a block: block-level, floating,
    /// absolutely positioned or a flex or grid item.
    fn is_block_in(&self, style: &ComputedStyle, parent: &ComputedStyle) -> bool {
        style.display.is_block_level()
            || style.is_floating()
            || matches!(style.position, Position::Absolute | Position::Fixed)
            || matches!(
                parent.display,
                Display::Normal {
                    inside: DisplayInside::Flex | DisplayInside::Grid,
                    ..
                }
            )
    }

    /// Whether a table holds data, rather than only arranging content: it has header cells, a
    /// caption, a header or footer group, or says so.
    fn is_data_table(&self, node: NodeId, data: &ElementData) -> bool {
        if data
            .attribute("role")
            .is_some_and(|role| matches!(role.trim(), "table" | "grid"))
            || data.attribute("summary").is_some()
        {
            return true;
        }
        // Rows are in row groups or directly in the table; nested tables do not count.
        let mut stack: Vec<NodeId> = self.document.child_elements(node).collect();
        while let Some(child) = stack.pop() {
            let Some(element) = self.document.element(child) else {
                continue;
            };
            match element.local_name().as_ref() {
                "th" | "caption" | "thead" | "tfoot" => return true,
                "tbody" | "tr" => stack.extend(self.document.child_elements(child)),
                _ => {}
            }
        }
        false
    }

    /// Adds an element to the structure, inside `parent`.
    fn push(&mut self, parent: usize, element: Element) -> usize {
        let index = self.structure.elements.len();
        self.structure.elements.push(element);
        self.structure.elements[parent].children.push(index);
        index
    }

    fn own(&mut self, node: NodeId, element: usize) {
        self.structure.owners.insert(node, Owner::Element(element));
    }

    /// Makes the content of an element and its descendants decoration.
    fn hide(&mut self, node: NodeId) {
        self.structure.owners.insert(node, Owner::Artifact);
        for descendant in self.document.descendants(node) {
            if self.document.element(descendant).is_some() {
                self.structure.owners.insert(descendant, Owner::Artifact);
            }
        }
    }
}

fn numbering(style: &ListStyleType) -> Numbering {
    match style {
        ListStyleType::Disc => Numbering::Disc,
        ListStyleType::Circle => Numbering::Circle,
        ListStyleType::Square => Numbering::Square,
        ListStyleType::Decimal | ListStyleType::DecimalLeadingZero => Numbering::Decimal,
        ListStyleType::LowerAlpha => Numbering::LowerAlpha,
        ListStyleType::UpperAlpha => Numbering::UpperAlpha,
        ListStyleType::LowerRoman => Numbering::LowerRoman,
        ListStyleType::UpperRoman => Numbering::UpperRoman,
        ListStyleType::None | ListStyleType::String(_) | ListStyleType::Named(_) => Numbering::None,
    }
}

/// The number of rows or columns a cell spans, at most `limit` as in HTML.
fn span(data: &ElementData, attribute: &str, limit: u32) -> u32 {
    data.attribute(attribute)
        .and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|span| *span >= 1)
        .map_or(1, |span| span.min(limit))
}

/// Text with white space collapsed.
fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use fagus_css::{MediaEnvironment, Origin, RuleSet, Stylesheet};

    /// The structure of a document as an outline: roles, nested in brackets.
    fn outline(html: &str) -> String {
        let document = Document::parse_html(html);
        let user_agent = Stylesheet::parse(fagus_style::USER_AGENT_CSS, Origin::UserAgent, "ua.css");
        let rules = RuleSet::new(&[&user_agent], &MediaEnvironment::default());
        let styled = fagus_style::compute_styles(&document, &rules, (794.0, 1123.0));
        let structure = build(&document, &styled, &FxHashSet::default()).structure;
        let mut out = String::new();
        write(&structure, 0, &mut out);
        out
    }

    fn write(structure: &Structure, element: usize, out: &mut String) {
        let element = &structure.elements[element];
        let name = match &element.role {
            Role::Heading { level, title } => format!("H{level}({title})"),
            Role::Figure { alt } => format!("Figure({})", alt.as_deref().unwrap_or("-")),
            Role::Formula { alt } => format!("Formula({})", alt.as_deref().unwrap_or("-")),
            Role::TableHeader { scope, .. } => format!("TH({scope:?})"),
            Role::TableCell { columns, .. } if *columns > 1 => format!("TD({columns})"),
            Role::TableCell { .. } => "TD".to_owned(),
            Role::List { numbering } => format!("L({numbering:?})"),
            role => format!("{role:?}")
                .split([' ', '{'])
                .next()
                .unwrap_or_default()
                .to_owned(),
        };
        out.push_str(&name);
        if let Some(language) = &element.language {
            out.push('@');
            out.push_str(language);
        }
        if element.repeat != Repeat::Never {
            out.push('~');
            out.push_str(if element.repeat == Repeat::First {
                "First"
            } else {
                "Last"
            });
        }
        if !element.children.is_empty() {
            out.push('[');
            for (index, &child) in element.children.iter().enumerate() {
                if index > 0 {
                    out.push(' ');
                }
                write(structure, child, out);
            }
            out.push(']');
        }
    }

    #[test]
    fn gives_elements_their_roles() {
        assert_eq!(
            outline(
                "<html lang=de><h1>Rechnung <small>Nr. 1</small></h1><p>Text <b>fett</b> \
                 <a href=\"https://quittio.dev\">Link</a> <span lang=en>invoice</span> \
                 <abbr title=\"Umsatzsteuer\">USt.</abbr></p><div>Nur Text</div>\
                 <div><p>Absatz</p></div><img src=a.png alt=Logo><img src=b.png alt=\"\">\
                 <blockquote>Zitat</blockquote>"
            ),
            "Document@de[H1(Rechnung Nr. 1) Paragraph[Link Span@en Abbreviation] Paragraph \
             Division[Paragraph] Figure(Logo) BlockQuote]"
        );
    }

    #[test]
    fn tags_stress_and_aria_headings_as_chrome() {
        assert_eq!(
            outline(
                "<div role=heading aria-level=3>Positionen</div><div role=heading>Summen</div>\
                 <p><em>sehr</em> <strong>wichtig</strong> <b>fett</b></p>"
            ),
            "Document[H3(Positionen) H2(Summen) Paragraph[Emphasis Strong]]"
        );
    }

    #[test]
    fn tags_formulas_with_their_alternative_text() {
        assert_eq!(
            outline(
                "<p>Mass <math><semantics><mrow><mi>E</mi><mo>=</mo><mi>m</mi></mrow>\
                 <annotation encoding=\"application/x-tex\">E = m</annotation></semantics></math> \
                 <math alttext=\"x squared\" display=block><msup><mi>x</mi><mn>2</mn></msup></math></p>"
            ),
            "Document[Paragraph[Formula(E = m) Formula(x squared)]]"
        );
    }

    #[test]
    fn makes_lists_with_labels() {
        assert_eq!(
            outline("<ol><li>Eins</li><li>Zwei</li></ol><ul style=\"list-style: square\"><li>A</li></ul>"),
            "Document[L(Decimal)[ListItem[Label ListBody] ListItem[Label ListBody]] \
             L(Square)[ListItem[Label ListBody]]]"
        );
    }

    #[test]
    fn makes_notes_of_footnotes_after_their_calls() {
        assert_eq!(
            outline(
                "<p>Text<span style=\"float: footnote\">Note <em>with</em> <a href=\"#x\">a link</a></span>                  goes on.</p><aside style=\"float: footnote\"><p>Block</p></aside>"
            ),
            "Document[Paragraph[Link Note[Label[Link] Emphasis Link]] Link Note[Label[Link] Paragraph]]"
        );
    }

    #[test]
    fn tells_data_tables_from_layout_tables() {
        assert_eq!(
            outline(
                "<table><thead><tr><th>Pos.</th><th>Betrag</th></tr></thead>\
                 <tbody><tr><th>1</th><td>10 €</td></tr><tr><td colspan=2>Summe</td></tr></tbody>\
                 <tfoot><tr><td>Übertrag</td><td>10 €</td></tr></tfoot></table>\
                 <table><tr><td>Rechnungsnummer</td><td>RE-1</td></tr></table>"
            ),
            "Document[Table[TableHead~First[TableRow[TH(Column) TH(Column)]] \
             TableBody[TableRow[TH(Row) TD] TableRow[TD(2)]] \
             TableFoot~Last[TableRow[TD TD]]] Division[Paragraph Paragraph]]"
        );
    }

    #[test]
    fn leaves_out_what_readers_skip() {
        let document = Document::parse_html(
            "<div style=\"position: running(header)\">Kopf</div><p aria-hidden=true>Versteckt</p>\
             <p>Inhalt</p><div style=\"display: none\"><p>Nichts</p></div>",
        );
        let user_agent = Stylesheet::parse(fagus_style::USER_AGENT_CSS, Origin::UserAgent, "ua.css");
        let rules = RuleSet::new(&[&user_agent], &MediaEnvironment::default());
        let styled = fagus_style::compute_styles(&document, &rules, (794.0, 1123.0));
        let structure = build(&document, &styled, &FxHashSet::default()).structure;
        assert_eq!(structure.elements.len(), 2);
        let hidden = structure
            .owners
            .values()
            .filter(|owner| **owner == Owner::Artifact)
            .count();
        assert_eq!(hidden, 2);
    }
}

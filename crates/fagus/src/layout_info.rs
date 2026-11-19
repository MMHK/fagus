//! Where the elements of a rendered document ended up: the pages and boxes of the elements with
//! an `id`, and the rows of their tables ([`Document::report_layout`](crate::Document::report_layout)).

use fagus_dom::{Document, NodeId};
use fagus_layout::{Fragment, FragmentKind, Page};
use rustc_hash::FxHashMap;

/// Points per CSS pixel.
const POINTS: f32 = 72.0 / 96.0;

/// Where the elements of a document ended up on its pages, in PDF points from the top left
/// corner of each page.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct Layout {
    /// The pages, in order.
    pub pages: Vec<LayoutPage>,
    /// The elements with an `id`, in document order, also those that have no box.
    pub elements: Vec<LayoutElement>,
}

impl Layout {
    /// The first element with the `id`.
    pub fn element(&self, id: &str) -> Option<&LayoutElement> {
        self.elements.iter().find(|element| element.id == id)
    }
}

/// The size of a page, in points: that of its page box, without bleed and printer's marks, and
/// before `page-orientation` turns it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct LayoutPage {
    pub width: f32,
    pub height: f32,
}

/// An element with an `id` and the boxes it got.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct LayoutElement {
    /// The value of its `id` attribute.
    pub id: String,
    /// Its tag name, in lowercase for HTML elements (`td`).
    pub tag: String,
    /// Its boxes, one for each page it is on and, inline, for each line: in page order, empty
    /// for an element that is not rendered (`display: none`, `display: contents`). A header or
    /// footer that repeats, a fixed box and a running element in the page margins have a box on
    /// each page they show on.
    pub boxes: Vec<LayoutBox>,
    /// The rows of a table, in document order, with the boxes each one got: where the table
    /// breaks. Empty for elements other than tables.
    pub rows: Vec<LayoutRow>,
}

impl LayoutElement {
    /// The numbers of the pages the element is on (from 1), in order, each once.
    pub fn pages(&self) -> Vec<usize> {
        pages_of(&self.boxes)
    }
}

/// The border box of an element, or of a row, on one page, in points from the top left corner
/// of the page, before `transform`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct LayoutBox {
    /// The number of the page, from 1.
    pub page: usize,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// A row of a table (`<tr>`) and the boxes it got: one on each page it is on, the area of its
/// cells there.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct LayoutRow {
    /// The row group it is in.
    pub section: RowSection,
    /// Its place among the rows of that kind of row group in the table, from 0: the first row
    /// of the body is 0, also where the table has several `<tbody>`.
    pub index: usize,
    /// The value of its `id` attribute, if it has one.
    pub id: Option<String>,
    pub boxes: Vec<LayoutBox>,
}

impl LayoutRow {
    /// The numbers of the pages the row is on (from 1), in order, each once.
    pub fn pages(&self) -> Vec<usize> {
        pages_of(&self.boxes)
    }
}

/// The row group a row of a table is in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RowSection {
    /// `<thead>`, or a row group with `display: table-header-group`.
    Header,
    /// `<tbody>`, or rows outside any row group.
    #[default]
    Body,
    /// `<tfoot>`, or a row group with `display: table-footer-group`.
    Footer,
}

fn pages_of(boxes: &[LayoutBox]) -> Vec<usize> {
    let mut pages: Vec<usize> = boxes.iter().map(|placed| placed.page).collect();
    pages.dedup();
    pages
}

/// The rows of a table: each with its row group and its place among the rows of that kind.
type TableRows = Vec<(NodeId, RowSection, usize)>;

/// What a box of an element counts for.
#[derive(Clone, Copy, Default)]
struct Wanted {
    /// Whether it is an element with an `id`.
    element: bool,
    /// The row it is a cell of, and whether it spans more rows.
    row: Option<(NodeId, bool)>,
}

/// The layout of the elements with an `id` of `document` on `pages`.
pub(crate) fn collect(document: &Document, pages: &[Page]) -> Layout {
    let Some(root) = document.document_element() else {
        return Layout {
            pages: page_sizes(pages),
            elements: Vec::new(),
        };
    };
    let mut wanted: FxHashMap<NodeId, Wanted> = FxHashMap::default();
    // The rows of each table with an `id` and of each row with one, by row.
    let mut rows: Vec<NodeId> = Vec::new();
    let mut tables: Vec<(NodeId, TableRows)> = Vec::new();
    for node in document.descendants(root) {
        let Some(element) = document.element(node) else {
            continue;
        };
        if element.id.as_deref().is_none_or(str::is_empty) {
            continue;
        }
        wanted.entry(node).or_default().element = true;
        if element.is_html() && element.is("table") {
            let table_rows = table_rows(document, node);
            rows.extend(table_rows.iter().map(|&(row, ..)| row));
            tables.push((node, table_rows));
        } else if element.is_html() && element.is("tr") {
            rows.push(node);
        }
    }
    for &row in &rows {
        for cell in document.children(row) {
            let Some(element) = document.element(cell) else {
                continue;
            };
            if element.is_html() && (element.is("td") || element.is("th")) {
                let spans = element
                    .attribute("rowspan")
                    .and_then(|value| value.trim().parse::<u32>().ok())
                    .is_some_and(|span| span != 1);
                wanted.entry(cell).or_default().row = Some((row, spans));
            }
        }
    }

    let mut found = Found {
        wanted: &wanted,
        boxes: FxHashMap::default(),
        cells: FxHashMap::default(),
        shrink: None,
        shift: 0.0,
    };
    for (index, page) in pages.iter().enumerate() {
        for (position, fragment) in page.fragments.iter().enumerate() {
            // Content laid out wider than the page shrinks from its corner, as it is painted
            // (see `interactive`).
            found.shrink = (position == 0 && page.scale > 0.0 && page.scale < 1.0)
                .then(|| (page.scale, fragment.rect.x.to_px(), fragment.rect.y.to_px()));
            found.shift = if position == 0 { page.shift } else { 0.0 };
            found.walk(fragment, index + 1, (0.0, 0.0));
        }
    }
    let Found { mut boxes, cells, .. } = found;
    let row_boxes: FxHashMap<NodeId, Vec<LayoutBox>> = cells
        .into_iter()
        .map(|(row, cells)| (row, row_boxes(&cells)))
        .collect();
    let mut tables: FxHashMap<NodeId, TableRows> = tables.into_iter().collect();
    let mut elements = Vec::new();
    for node in document.descendants(root) {
        let Some(element) = document.element(node) else {
            continue;
        };
        let Some(id) = element.id.as_ref().filter(|id| !id.is_empty()) else {
            continue;
        };
        let own = if element.is_html() && element.is("tr") {
            row_boxes.get(&node).cloned().unwrap_or_default()
        } else {
            boxes.remove(&node).unwrap_or_default()
        };
        let rows = tables
            .remove(&node)
            .unwrap_or_default()
            .into_iter()
            .map(|(row, section, index)| LayoutRow {
                section,
                index,
                id: document
                    .element(row)
                    .and_then(|row| row.id.clone())
                    .filter(|id| !id.is_empty()),
                boxes: row_boxes.get(&row).cloned().unwrap_or_default(),
            })
            .collect();
        elements.push(LayoutElement {
            id: id.clone(),
            tag: element.local_name().to_string(),
            boxes: own,
            rows,
        });
    }
    Layout {
        pages: page_sizes(pages),
        elements,
    }
}

fn page_sizes(pages: &[Page]) -> Vec<LayoutPage> {
    pages
        .iter()
        .map(|page| LayoutPage {
            width: page.width.to_px() * POINTS,
            height: page.height.to_px() * POINTS,
        })
        .collect()
}

/// The rows of `table` in document order, with their row group and their place among the rows
/// of that kind: those of its row groups and those directly in it, not those of tables in its
/// cells.
fn table_rows(document: &Document, table: NodeId) -> TableRows {
    let mut found = Vec::new();
    let mut counts = [0usize; 3];
    let mut add = |row: NodeId, section: RowSection| {
        let slot = match section {
            RowSection::Header => 0,
            RowSection::Body => 1,
            RowSection::Footer => 2,
        };
        found.push((row, section, counts[slot]));
        counts[slot] += 1;
    };
    for child in document.children(table) {
        let Some(element) = document.element(child) else {
            continue;
        };
        if !element.is_html() {
            continue;
        }
        if element.is("tr") {
            add(child, RowSection::Body);
            continue;
        }
        let section = if element.is("thead") {
            RowSection::Header
        } else if element.is("tfoot") {
            RowSection::Footer
        } else if element.is("tbody") {
            RowSection::Body
        } else {
            continue;
        };
        for row in document.children(child) {
            if document
                .element(row)
                .is_some_and(|row| row.is_html() && row.is("tr"))
            {
                add(row, section);
            }
        }
    }
    found
}

/// The boxes of a row from those of its cells, by page: on each page, the area that its cells
/// cover there, those that span more rows only where it has no other.
fn row_boxes(cells: &[(LayoutBox, bool)]) -> Vec<LayoutBox> {
    let mut boxes: Vec<LayoutBox> = Vec::new();
    let mut pages: Vec<usize> = cells.iter().map(|(placed, _)| placed.page).collect();
    pages.dedup();
    for page in pages {
        let on_page: Vec<&(LayoutBox, bool)> =
            cells.iter().filter(|(placed, _)| placed.page == page).collect();
        let single: Vec<&LayoutBox> = on_page
            .iter()
            .filter(|(_, spans)| !spans)
            .map(|(placed, _)| placed)
            .collect();
        let chosen: Vec<&LayoutBox> = if single.is_empty() {
            on_page.iter().map(|(placed, _)| placed).collect()
        } else {
            single
        };
        // A row repeated on a page (none is) or broken would have one area per piece; the
        // pieces of one page are joined.
        let left = chosen.iter().map(|placed| placed.x).fold(f32::INFINITY, f32::min);
        let top = chosen.iter().map(|placed| placed.y).fold(f32::INFINITY, f32::min);
        let right = chosen
            .iter()
            .map(|placed| placed.x + placed.width)
            .fold(f32::NEG_INFINITY, f32::max);
        let bottom = chosen
            .iter()
            .map(|placed| placed.y + placed.height)
            .fold(f32::NEG_INFINITY, f32::max);
        if left.is_finite() && top.is_finite() {
            boxes.push(LayoutBox {
                page,
                x: left,
                y: top,
                width: right - left,
                height: bottom - top,
            });
        }
    }
    boxes
}

struct Found<'a> {
    wanted: &'a FxHashMap<NodeId, Wanted>,
    /// The boxes of the elements with an `id`.
    boxes: FxHashMap<NodeId, Vec<LayoutBox>>,
    /// The boxes of the cells of each row asked for, and whether each spans more rows.
    cells: FxHashMap<NodeId, Vec<(LayoutBox, bool)>>,
    /// How the fragments being walked are shrunk on the page: a scale around a point.
    shrink: Option<(f32, f32, f32)>,
    /// How far right the fragments being walked are moved before they shrink.
    shift: f32,
}

impl Found<'_> {
    /// The box on page `page` (from 1) of a fragment at `(x, y)` of `size` in the coordinates of
    /// the fragments being walked, in points.
    fn placed(&self, page: usize, (x, y): (f32, f32), (width, height): (f32, f32)) -> LayoutBox {
        let x = x + self.shift;
        let (x, y, scale) = match self.shrink {
            Some((scale, left, top)) => (left + (x - left) * scale, top + (y - top) * scale, scale),
            None => (x, y, 1.0),
        };
        LayoutBox {
            page,
            x: x * POINTS,
            y: y * POINTS,
            width: width * scale * POINTS,
            height: height * scale * POINTS,
        }
    }

    fn walk(&mut self, fragment: &Fragment, page: usize, origin: (f32, f32)) {
        let x = origin.0 + fragment.rect.x.to_px();
        let y = origin.1 + fragment.rect.y.to_px();
        if let FragmentKind::Box { node: Some(node), .. } = &fragment.kind
            && let Some(&wanted) = self.wanted.get(node)
        {
            let placed = self.placed(
                page,
                (x, y),
                (fragment.rect.width.to_px(), fragment.rect.height.to_px()),
            );
            if wanted.element {
                self.boxes.entry(*node).or_default().push(placed);
            }
            if let Some((row, spans)) = wanted.row {
                self.cells.entry(row).or_default().push((placed, spans));
            }
        }
        for child in &fragment.children {
            self.walk(child, page, (x, y));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placed(page: usize, (x, y, width, height): (f32, f32, f32, f32)) -> LayoutBox {
        LayoutBox {
            page,
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn joins_the_cells_of_a_row_on_each_page() {
        // Two cells on page 1, the second broken onto page 2; a cell spanning more rows counts
        // only where the row has no other.
        let cells = vec![
            (placed(1, (10.0, 20.0, 30.0, 15.0)), false),
            (placed(1, (40.0, 20.0, 30.0, 40.0)), true),
            (placed(1, (70.0, 20.0, 30.0, 18.0)), false),
            (placed(2, (70.0, 10.0, 30.0, 5.0)), false),
            (placed(3, (40.0, 10.0, 30.0, 8.0)), true),
        ];
        assert_eq!(
            row_boxes(&cells),
            [
                placed(1, (10.0, 20.0, 90.0, 18.0)),
                placed(2, (70.0, 10.0, 30.0, 5.0)),
                placed(3, (40.0, 10.0, 30.0, 8.0)),
            ]
        );
    }

    #[test]
    fn numbers_the_rows_of_each_section_in_document_order() {
        let document = Document::parse_html(
            "<table id=t><tfoot><tr><td>f</td></tr></tfoot><thead><tr><td>h</td></tr></thead>
             <tbody><tr><td>1<table><tr><td>inner</td></tr></table></td></tr></tbody>
             <tbody><tr id=two><td>2</td></tr></tbody></table>",
        );
        let table = document.find("table").expect("table");
        let sections: Vec<(RowSection, usize)> = table_rows(&document, table)
            .into_iter()
            .map(|(_, section, index)| (section, index))
            .collect();
        assert_eq!(
            sections,
            [
                (RowSection::Footer, 0),
                (RowSection::Header, 0),
                (RowSection::Body, 0),
                (RowSection::Body, 1),
            ]
        );
    }
}

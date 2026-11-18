//! Where tables, their headers and footers, and the boxes around them break at the end of a
//! page, as Chrome prints them. Each case puts a spacer of `height` above the content on pages
//! 400px × 300px with margins of 20px; the numbers are Chrome's, measured with the same font.

use super::{find, render_document};
use crate::fragment::Page;

/// Renders `content` below a spacer `height` pixels tall.
fn render(height: u32, content: &str) -> (fagus_dom::Document, Vec<Page>) {
    render_document(&format!(
        "<style>@page {{ size: 400px 300px; margin: 20px }} body {{ margin: 0; font: 14px/20px Inter }}
         table {{ border: 2px solid blue }} td {{ border: 1px solid }} p {{ margin: 0 }}</style>
         <div style='height: {height}px'></div>{content}"
    ))
}

/// The number of the page (from 0) the element with class `class` starts on.
fn page_of(document: &fagus_dom::Document, pages: &[Page], class: &str) -> Option<usize> {
    pages
        .iter()
        .position(|page| find(page, document, class).is_some())
}

#[test]
fn moves_the_last_row_of_a_collapsed_table_whose_bottom_border_does_not_fit() {
    let table =
        "<table style='border-collapse: collapse; border-width: 6px'><tr><td class=a>A</td></tr></table>";
    let (document, pages) = render(226, table);
    assert_eq!(page_of(&document, &pages, "a"), Some(0));
    let (document, pages) = render(229, table);
    assert_eq!(page_of(&document, &pages, "a"), Some(1));
}

#[test]
fn breaks_tables_with_captions_in_column_flex_containers() {
    let (document, pages) = render(
        215,
        "<div style='display: flex; flex-direction: column; border: 1px solid'>
         <table><caption class=caption>C</caption><tr><td class=a>A</td></tr></table></div>",
    );
    // The caption stays, the rows move on, as in block flow.
    assert_eq!(page_of(&document, &pages, "caption"), Some(0));
    assert_eq!(page_of(&document, &pages, "a"), Some(1));
}

#[test]
fn breaks_inline_tables_of_column_flex_containers_inside_their_cells() {
    let (document, pages) = render(
        225,
        "<div style='display: flex; flex-direction: column; border-top: 3px solid'>
         <div class=table style='display: inline-table; border: 2px solid'>
         <div class=cell style='display: table-cell; height: 36px'>A</div></div></div>",
    );
    assert_eq!(page_of(&document, &pages, "table"), Some(0));
    assert_eq!(pages.len(), 2);
}

#[test]
fn breaks_before_a_table_footer_that_does_not_fit_below_the_last_rows() {
    let table = "<table><thead><tr><th class=head>H</th></tr></thead>
        <tbody><tr><td>A</td></tr><tr><td class=b>B</td></tr></tbody>
        <tfoot><tr><td class=foot>S</td></tr></tfoot></table>";
    // The last row fits with room for the footer as it repeats, but not with the closing footer:
    // the footer goes on alone, below the repeated header.
    let (document, pages) = render(155, table);
    assert_eq!(page_of(&document, &pages, "b"), Some(0));
    assert!(find(&pages[0], &document, "foot").is_none());
    assert!(find(&pages[1], &document, "head").is_some());
    assert!(find(&pages[1], &document, "foot").is_some());
    // Two pixels further down, the last row moves on with the footer.
    let (document, pages) = render(157, table);
    assert_eq!(page_of(&document, &pages, "b"), Some(1));
    assert!(find(&pages[0], &document, "foot").is_some());
}

#[test]
fn reserves_the_collapsed_bottom_border_with_a_repeated_footer() {
    let table = "<table style='border-collapse: collapse; border-bottom-width: 6px'><tbody>
        <tr><td>A</td></tr><tr><td class=b>B</td></tr><tr><td>C</td></tr></tbody>
        <tfoot><tr><td>S</td></tr></tfoot></table>";
    // Where the table breaks, the footer needs room for half the table's bottom border below it
    // too (3px), as in Chrome: from 185px down, the second row goes on to the next page.
    let (document, pages) = render(184, table);
    assert_eq!(page_of(&document, &pages, "b"), Some(0));
    let (document, pages) = render(185, table);
    assert_eq!(page_of(&document, &pages, "b"), Some(1));
}

#[test]
fn reserves_the_spacing_of_a_footer_without_rows() {
    // The footer's only row is not displayed: the footer still repeats in Chrome, taking the
    // spacing below it (8px) on every page where the table breaks.
    let table = "<table style='border-spacing: 0 8px'><tbody>
        <tr><td>A</td></tr><tr><td class=b>B</td></tr><tr><td>C</td></tr></tbody>
        <tfoot><tr style='display: none'><td>S</td></tr></tfoot></table>";
    let (document, pages) = render(186, table);
    assert_eq!(page_of(&document, &pages, "b"), Some(0));
    let (document, pages) = render(187, table);
    assert_eq!(page_of(&document, &pages, "b"), Some(1));
}

#[test]
fn moves_a_repeated_footer_up_into_the_spacing_above_it() {
    let (document, pages) = render(
        182,
        "<table><thead><tr><th>H</th></tr></thead><tbody><tr><td>A</td></tr><tr><td>B</td></tr></tbody>
         <tfoot><tr><td class=foot>S</td></tr></tfoot></table>",
    );
    // It ends with the spacing below it at the end of the page.
    let (_, y, foot) = find(&pages[0], &document, "foot").expect("footer");
    assert_eq!(y + foot.rect.height.to_px(), 278.0);
}

#[test]
fn starts_tables_with_their_header_and_footer_below_captions() {
    let table = "<table><caption>Q</caption><thead><tr><th class=head>H</th></tr></thead>
        <tbody><tr><td class=a>A</td></tr></tbody><tfoot><tr><td class=foot>S</td></tr></tfoot></table>";
    let (document, pages) = render(190, table);
    // The table box starts below the caption with the header and the footer, without rows.
    assert!(find(&pages[0], &document, "head").is_some());
    assert!(find(&pages[0], &document, "foot").is_some());
    assert_eq!(page_of(&document, &pages, "a"), Some(1));
    // It continues at the top of the next page, without its top border.
    let (_, y, _) = find(&pages[1], &document, "head").expect("header");
    assert_eq!(y, 20.0);
    // Without room for them, the caption moves along.
    let (document, pages) = render(191, table);
    assert_eq!(page_of(&document, &pages, "head"), Some(1));
}

#[test]
fn lets_the_last_line_go_alone_where_only_the_bottom_border_pushes_it() {
    let (document, pages) = render(
        213,
        "<div style='border: 4px solid'><b class=a>A</b><br><b class=b>B</b></div>",
    );
    // The orphan and the widow are fewer than asked for.
    assert_eq!(page_of(&document, &pages, "a"), Some(0));
    assert_eq!(page_of(&document, &pages, "b"), Some(1));
}

#[test]
fn keeps_the_orphans_before_the_widows() {
    let (document, pages) = render(
        177,
        "<div style='border: 4px solid; widows: 3'>A<br><b class=b>B</b><br><b class=c>C</b><br>D</div>",
    );
    // Three widows would leave one orphan: two lines stay.
    assert_eq!(page_of(&document, &pages, "b"), Some(0));
    assert_eq!(page_of(&document, &pages, "c"), Some(1));
}

#[test]
fn widens_columns_of_a_fixed_width_before_auto_columns() {
    let words = "Beratung Software Lizenz ".repeat(20);
    let (document, pages) = render(
        0,
        &format!(
            "<table style='width: 360px'><tr><td class=narrow style='width: 40px'>L1</td><td>{words}</td></tr></table>"
        ),
    );
    let (_, _, narrow) = find(&pages[0], &document, "narrow").expect("cell");
    // Its width and its border, at the table's spacing.
    assert_eq!(narrow.rect.width.to_px(), 44.0);
}

#[test]
fn decides_whether_headers_repeat_in_columns_where_the_table_starts() {
    fn count(fragment: &crate::Fragment, needle: &str) -> usize {
        let own = usize::from(matches!(&fragment.kind, crate::FragmentKind::Text(run) if run.text == needle));
        own + fragment
            .children
            .iter()
            .map(|child| count(child, needle))
            .sum::<usize>()
    }
    let rows = "<tr><td>Position</td></tr>".repeat(40);
    let (_, pages) = render(
        200,
        &format!("<div style='columns: 2'><table><thead><tr><th>Kopf</th></tr></thead>{rows}</table></div>"),
    );
    // The table starts in columns that are balanced, whose height is not known: its header does
    // not repeat in the columns on the next pages either, as in Chrome.
    let headers: usize = pages
        .iter()
        .flat_map(|page| &page.fragments)
        .map(|fragment| count(fragment, "Kopf"))
        .sum();
    assert!(pages.len() > 1);
    assert_eq!(headers, 1);
}

#[test]
fn grows_the_rows_of_tables_stretched_by_their_grid_or_flex_line() {
    for display in ["grid; grid-template-columns: 1fr 1fr", "flex"] {
        let (document, pages) = render(
            0,
            &format!(
                "<div style='display: {display}'><table style='border: 0; border-spacing: 0'>
                 <tr><td class=a style='border: 0; padding: 0'>A</td></tr>
                 <tr><td class=b style='border: 0; padding: 0'>B</td></tr></table>
                 <div style='height: 100px; width: 50px'></div></div>"
            ),
        );
        // The rows share the height of the line, as in Chrome.
        let (_, _, a) = find(&pages[0], &document, "a").expect("a");
        let (_, y, _) = find(&pages[0], &document, "b").expect("b");
        assert_eq!((a.rect.height.to_px(), y), (50.0, 70.0), "{display}");
    }
}

#[test]
fn grows_the_last_rows_of_stretched_tables_by_the_room_their_breaks_took() {
    let rows = (1..=20)
        .map(|row| {
            [
                "<tr><td class=r",
                &row.to_string(),
                " style='border: 0; padding: 0'>R</td></tr>",
            ]
            .concat()
        })
        .collect::<String>();
    for (display, after) in [("grid", 180.0), ("flex", 184.0)] {
        let (document, pages) = render(
            10,
            &format!(
                "<div style='display: {display}'><table style='border: 0; border-spacing: 0'>{rows}</table></div>
                 <p class=after>after</p>"
            ),
        );
        // 12 rows fit on the first page, 10px above its end: the table goes on 10px further
        // than its area, and its 20 rows share that, as in Chrome. A grid row ends where the
        // table ended before it grew, a flex line below the grown table.
        assert_eq!(pages.len(), 2);
        let (_, y, _) = find(&pages[1], &document, "r13").expect("r13");
        assert_eq!(y, 20.0);
        let (_, y, last) = find(&pages[1], &document, "r20").expect("r20");
        assert!(
            (y - 163.5).abs() < 0.1 && (last.rect.height.to_px() - 20.5).abs() < 0.1,
            "{display}: {y}"
        );
        let (_, y, _) = find(&pages[1], &document, "after").expect("after");
        assert!((y - after).abs() < 0.1, "{display}: {y}");
    }
}

#[test]
fn keeps_the_spacing_above_rows_that_a_cell_spanning_them_carries_on() {
    // The table breaks between rows that a cell spans: that cell goes on at the top of the next
    // page, the next row below the border spacing, as in Chrome.
    let rows = (1..=4)
        .map(|group| {
            format!(
                "<tr><td rowspan=3 class=g{group}>G{group}</td><td>a{group}</td></tr>
                 <tr><td class=b{group}>b{group}</td></tr><tr><td>c{group}</td></tr>"
            )
        })
        .collect::<Vec<_>>()
        .concat();
    let (document, pages) = render(
        120,
        &format!("<table style='border-spacing: 6px; width: 100%'>{rows}</table>"),
    );
    assert_eq!(pages.len(), 2);
    let (_, y, _) = find(&pages[1], &document, "b2").expect("row below the break");
    assert_eq!(y, 26.0);
    let (_, y, _) = find(&pages[1], &document, "g2").expect("rest of the spanning cell");
    assert_eq!(y, 20.0);
}

#[test]
fn ends_cells_spanning_into_the_next_page_with_the_rows_above_a_repeated_footer() {
    // The spanning cell's lines go on, but its box ends with the row left on the page, the
    // repeated footer right below it; the next page starts with the spacing above the next row.
    let rows = (1..=3)
        .map(|group| {
            format!(
                "<tr><td rowspan=3 class=g{group}>G{group}<br>x<br>y<br>z</td><td>a{group}</td></tr>
                 <tr><td class=b{group}>b{group}</td></tr><tr><td>c{group}</td></tr>"
            )
        })
        .collect::<Vec<_>>()
        .concat();
    let (document, pages) = render(
        60,
        &format!(
            "<table style='border-spacing: 4px 10px; width: 100%'><tfoot><tr><td class=foot>F</td></tr></tfoot>{rows}</table>"
        ),
    );
    let foot = find(&pages[0], &document, "foot").expect("repeated footer").1;
    let (_, top, cell) = find(&pages[0], &document, "g2").expect("spanning cell");
    assert_eq!(top + cell.rect.height.to_px() + 10.0, foot);
    assert_eq!((top, cell.rect.height.to_px(), foot), (194.0, 24.0, 228.0));
    let (_, y, _) = find(&pages[1], &document, "b2").expect("row below the break");
    assert_eq!(y, 30.0);
}

#[test]
fn moves_a_row_whose_cell_padding_does_not_fit_below_a_spanning_cell_that_breaks() {
    // The second row's line fits, its bottom padding does not: the row moves to the next page,
    // although the cell spanning both rows breaks inside its content, as in Chrome.
    let (document, pages) = render_document(
        "<style>@page { size: 400px 300px; margin: 0 } body { margin: 0; font: 16px/20px Inter }
         table { border-collapse: collapse; width: 100% }
         td { border: 1px solid; padding: 2px; vertical-align: middle }</style>
         <div style='height: 9px'></div><table>
         <tr><td>1</td><td>r</td></tr><tr><td>2</td><td>r</td></tr><tr><td>3</td><td>r</td></tr>
         <tr><td>4</td><td>r</td></tr><tr><td>5</td><td>r</td></tr><tr><td>6</td><td>r</td></tr>
         <tr><td>7</td><td>r</td></tr><tr><td>8</td><td>r</td></tr><tr><td>9</td><td>r</td></tr>
         <tr><td rowspan=2>G<br>3<br>a<br>b</td><td class=first>first</td></tr>
         <tr><td class=second>second</td></tr></table>",
    );
    assert_eq!(page_of(&document, &pages, "first"), Some(0));
    assert_eq!(page_of(&document, &pages, "second"), Some(1));
}

#[test]
fn repeats_a_collapsed_header_below_the_tables_top_border() {
    // The repeated header draws the table's top border (half the widest top border of its row)
    // again above it, as on the first page.
    let rows = "<tr><td>r</td><td>x</td></tr>".repeat(30);
    let (document, pages) = render_document(&format!(
        "<style>@page {{ size: 400px 300px; margin: 10px }} body {{ margin: 0; font: 12px/16px Inter }}
         table {{ border-collapse: collapse }} td {{ border: 3px solid; padding: 2px }}</style>
         <table><thead><tr><td class=head>H1</td><td style='border-top-width: 14px'>H2</td></tr></thead>
         {rows}</table>"
    ));
    assert!(pages.len() > 1);
    let first = find(&pages[0], &document, "head").map(|(_, y, _)| y);
    let second = find(&pages[1], &document, "head").map(|(_, y, _)| y);
    assert!(first.is_some());
    assert_eq!(first, second);
}

#[test]
fn places_every_cell_of_long_collapsed_tables_with_shared_border_styles() {
    use crate::fragment::{Fragment, FragmentKind};
    use fagus_style::values::{Display, InternalDisplay};
    use std::sync::Arc;

    /// The cells and the pieces of collapsed borders below `fragment`.
    fn walk<'a>(
        fragment: &'a Fragment,
        cells: &mut usize,
        pieces: &mut Vec<&'a Arc<fagus_style::ComputedStyle>>,
    ) {
        if let FragmentKind::Box { style, node, .. } = &fragment.kind {
            if style.display == Display::Internal(InternalDisplay::TableCell) {
                *cells += 1;
            } else if node.is_none() && (style.border_width.top > 0.0 || style.border_width.left > 0.0) {
                pieces.push(style);
            }
        }
        for child in &fragment.children {
            walk(child, cells, pieces);
        }
    }
    let rows = "<tr><td>A</td><td>B</td></tr>".repeat(40);
    let (_, pages) = render(
        0,
        &format!("<table style='border-collapse: collapse'>{rows}</table>"),
    );
    assert!(pages.len() > 1);
    let (mut cells, mut pieces) = (0, Vec::new());
    for page in &pages {
        for fragment in &page.fragments {
            walk(fragment, &mut cells, &mut pieces);
        }
    }
    assert_eq!(cells, 80);
    // Each border and direction has one style, however many pieces draw it.
    let mut styles: Vec<&Arc<fagus_style::ComputedStyle>> = Vec::new();
    for piece in &pieces {
        if !styles.iter().any(|style| Arc::ptr_eq(style, piece)) {
            styles.push(piece);
        }
    }
    assert!(pieces.len() > 100, "{} pieces", pieces.len());
    assert!(styles.len() <= 4, "{} styles", styles.len());
}

#[test]
fn shares_the_collapsed_styles_of_alike_cells() {
    use crate::fragment::FragmentKind;
    use std::sync::Arc;
    /// The styles of the fragments of the cells of class `a`.
    fn cells<'f>(
        fragment: &'f crate::Fragment,
        document: &fagus_dom::Document,
        out: &mut Vec<&'f Arc<fagus_style::ComputedStyle>>,
    ) {
        if let FragmentKind::Box {
            style,
            node: Some(node),
            ..
        } = &fragment.kind
            && document
                .element(*node)
                .is_some_and(|data| data.attribute("class") == Some("a"))
        {
            out.push(style);
        }
        for child in &fragment.children {
            cells(child, document, out);
        }
    }
    // Cells alike in a column share their style with collapsed borders.
    let rows = "<tr><td class=a>A</td><td>B</td></tr>".repeat(4);
    let (document, pages) = render(
        0,
        &format!("<table style='border-collapse: collapse'>{rows}</table>"),
    );
    let mut styles = Vec::new();
    for fragment in &pages[0].fragments {
        cells(fragment, &document, &mut styles);
    }
    assert_eq!(styles.len(), 4);
    // The first and last rows' cells have the table's border above or below them, those
    // between share their style.
    assert!(Arc::ptr_eq(styles[1], styles[2]));
    assert!(!Arc::ptr_eq(styles[0], styles[1]));
    assert_eq!(styles[1].border_width.top, 1.0);
}

#[test]
fn ends_a_table_whose_row_breaks_at_the_end_of_the_page() {
    // The row 400px tall breaks at the end of the page area (260px tall): the table ends there,
    // without the spacing below the row (CSS2 pagination tests, `border=1`).
    let (document, pages) = render(
        0,
        "<table class=t border=1><tr><td><div style='height: 400px'></div></td></tr></table>",
    );
    assert_eq!(pages.len(), 2);
    let (_, y, table) = find(&pages[0], &document, "t").expect("table on the first page");
    assert_eq!(y + table.rect.height.to_px(), 280.0);
}

#[test]
fn breaks_tables_that_avoid_breaks_at_forced_breaks_between_their_rows() {
    // Forced breaks before the rows break the table though it avoids breaks inside, as in
    // Chrome: each row goes on a page of its own.
    let (document, pages) = render(
        0,
        "<p>one</p><table style='break-inside: avoid'><tr style='break-before: page'><td class=a>A</td></tr>\
         <tr style='break-before: page'><td class=b>B</td></tr><tr style='break-before: page'><td class=c>C</td></tr></table>",
    );
    assert_eq!(pages.len(), 4);
    assert_eq!(
        ["a", "b", "c"].map(|class| page_of(&document, &pages, class)),
        [Some(1), Some(2), Some(3)]
    );
}

#[test]
fn clones_the_border_and_padding_of_tables_on_every_page() {
    // With `box-decoration-break: clone`, every part of the table has its border (2px) and
    // padding (10px), and the spacing (4px) only where the table starts and ends: the page
    // area (260px) leaves the cell 260 - 12 - 4 - 12 = 232px on the first page, and the part
    // on the second page starts with the border and padding.
    let (document, pages) = render(
        0,
        "<table class=t style='padding: 10px; border-spacing: 4px; box-decoration-break: clone'>
         <tr><td style='padding: 0; border: 0'><div style='height: 300px; width: 10px'></div></td></tr></table>",
    );
    assert_eq!(pages.len(), 2);
    let (_, _, first) = find(&pages[0], &document, "t").expect("table on the first page");
    assert_eq!(first.rect.height.to_px(), 260.0);
    let (_, _, second) = find(&pages[1], &document, "t").expect("table on the second page");
    // 12px of border and padding, the rest of the cell (68px), 4px of spacing and 12px.
    assert_eq!(second.rect.height.to_px(), 12.0 + 68.0 + 4.0 + 12.0);
}

/// Renders `body` on pages 300px × 260px with margins of 20px.
fn render_small(body: &str) -> (fagus_dom::Document, Vec<Page>) {
    render_document(&format!(
        "<style>@page {{ size: 300px 260px; margin: 20px }} body {{ margin: 0; font: 16px/20px Inter }}
         </style>{body}"
    ))
}

#[test]
fn starts_tables_below_a_header_or_captions_where_the_first_row_moves() {
    // Below the top padding of a box around it, the first row would break only in its cell's
    // bottom padding: it moves to the next page, the header staying on the first with the
    // table's box, which goes on below it.
    let row = "<tr><td style='padding: 0 0 24px' class=sp><div style='height: 160px'>sp</div></td></tr>
               <tr><td class=next>next</td></tr>";
    let (document, pages) = render_small(&format!(
        "<div style='padding-top: 24px'><table cellpadding=0 cellspacing=0 style='width: 100%'>
         <thead><tr><td class=head>head</td></tr></thead>{row}</table></div>"
    ));
    assert_eq!(pages.len(), 3);
    assert_eq!(page_of(&document, &pages, "head"), Some(0));
    assert_eq!(page_of(&document, &pages, "sp"), Some(1));
    assert_eq!(page_of(&document, &pages, "next"), Some(2));
    // With a caption, the table's box moves to the next page, and the box around ends below
    // the caption on the first, as in Chrome.
    let (document, pages) = render_small(&format!(
        "<div class=wrap style='padding-top: 24px'><table cellpadding=0 cellspacing=0 style='width: 100%'>
         <caption>cap</caption>{row}</table></div>"
    ));
    assert_eq!(pages.len(), 2);
    assert_eq!(page_of(&document, &pages, "sp"), Some(1));
    let (_, _, wrap) = find(&pages[0], &document, "wrap").expect("box");
    assert_eq!(wrap.rect.height.to_px(), 44.0);
}

#[test]
fn ends_a_resumed_table_below_its_rows_where_only_its_bottom_border_goes_on() {
    let (document, pages) = render_small(
        "<table class=t style='width: 100%; border: 3px solid blue'>
         <tr><td style='padding: 0'><div style='height: 250px'>a</div></td></tr>
         <tr><td style='padding: 0 0 24px'><div style='height: 170px'>sp</div></td></tr>
         <tr><td>next</td></tr></table>",
    );
    assert_eq!(pages.len(), 4);
    // The rows end 218px down the third page, without the spacing below them; the bottom
    // border goes on alone.
    let (_, _, table) = find(&pages[2], &document, "t").expect("table");
    assert_eq!(table.rect.height.to_px(), 218.0);
}

#[test]
fn aligns_the_cells_that_end_in_a_broken_row_grown_past_its_first_page_height() {
    // The table starts on the first page at its full width and goes on beside a float on the
    // second, where the last row's cells wrap onto more lines than its height there (one line)
    // holds: the row breaks with the page, its third cell going on to the third page, and the
    // cells that end are as tall as the row's part on the page, with their content aligned in
    // the middle of it, as in Chrome.
    let rows = "<tr><td><i></i></td><td><i></i></td><td><i></i></td></tr>".repeat(14);
    let document = format!(
        "<style>@page {{ size: 400px 300px; margin: 20px }} body {{ margin: 0 }}
         table {{ border-spacing: 0 }} td {{ padding: 0; line-height: 0; font-size: 0 }}
         i {{ display: inline-block; vertical-align: top; width: 50px; height: 20px }}</style>
         <div style='height: 200px'></div>
         <div style='float: right; width: 200px; height: 100px; contain: size'></div>
         <table>{rows}<tr><td><i class=a></i> <i></i></td><td><i class=b></i></td>
         <td><i></i> <i></i> <i class=c></i></td></tr></table>"
    );
    let (document, pages) = render_document(&document);
    assert_eq!(pages.len(), 3);
    let (_, a, _) = find(&pages[1], &document, "a").expect("a");
    let (_, b, _) = find(&pages[1], &document, "b").expect("b");
    assert_eq!((a, b), (240.0, 250.0));
    let (_, c, _) = find(&pages[2], &document, "c").expect("c");
    assert_eq!(c, 20.0);
}

/// The widths of the horizontal pieces of collapsed borders that the tables on `page` draw, with
/// the place of their middle from the top of the page.
fn collapsed_lines(page: &Page) -> Vec<(f32, f32)> {
    use crate::fragment::{Fragment, FragmentKind};
    fn walk(fragment: &Fragment, top: f32, out: &mut Vec<(f32, f32)>) {
        let y = top + fragment.rect.y.to_px();
        if let FragmentKind::Box {
            style, node: None, ..
        } = &fragment.kind
            && style.border_width.top > 0.0
            && style.border_style.top.is_visible()
        {
            out.push((style.border_width.top, y + style.border_width.top / 2.0));
        }
        for child in &fragment.children {
            walk(child, y, out);
        }
    }
    let mut out = Vec::new();
    for fragment in &page.fragments {
        walk(fragment, 0.0, &mut out);
    }
    out
}

/// The widths of the horizontal collapsed borders on `page`, each once.
fn collapsed_widths(page: &Page) -> Vec<f32> {
    let mut widths: Vec<f32> = collapsed_lines(page)
        .into_iter()
        .map(|(width, _)| width)
        .collect();
    widths.sort_by(f32::total_cmp);
    widths.dedup();
    widths
}

#[test]
fn collapses_borders_without_the_carry_over_rows_where_they_do_not_show() {
    // The header's carry-over row (6px below it) shows only after the table broke, the
    // footer's (5px above it) only before it ends: where they do not show, the head row's 2px
    // border meets the first row, and the last row the 3px border of the total.
    let table = |rows: usize| {
        format!(
            "<style>@page {{ size: 400px 300px; margin: 20px }} body {{ margin: 0; font: 14px/20px Inter }}
             table {{ border-collapse: collapse; width: 100% }} td, th {{ padding: 0 }}
             th {{ border-bottom: 2px solid }}
             .carry {{ -fagus-repeat: at-break }}
             thead .carry td {{ border-bottom: 6px solid red }}
             tfoot .carry td {{ border-top: 5px solid lime }}
             .sum td {{ border-top: 3px solid blue }}</style>
             <table><thead><tr><th>Item</th></tr><tr class=carry><td>Brought forward</td></tr></thead>
             <tfoot><tr class=carry><td>Carried over</td></tr><tr class=sum><td>Total</td></tr></tfoot>
             <tbody>{}</tbody></table>",
            "<tr><td>r</td></tr>".repeat(rows)
        )
    };
    let (_, pages) = render_document(&table(30));
    assert!(pages.len() > 2);
    let last = pages.len() - 1;
    assert_eq!(collapsed_widths(&pages[0]), [2.0, 3.0, 5.0]);
    assert_eq!(collapsed_widths(&pages[1]), [2.0, 3.0, 5.0, 6.0]);
    assert_eq!(collapsed_widths(&pages[last]), [2.0, 3.0, 6.0]);
    // The line below the head row is drawn once, not again as the top of the first row.
    let below_head = |page: &Page| {
        collapsed_lines(page)
            .into_iter()
            .filter(|&(width, _)| (width - 2.0).abs() < 0.01)
            .count()
    };
    assert_eq!(below_head(&pages[0]), 1);
    // A table on one page shows neither.
    let (_, pages) = render_document(&table(3));
    assert_eq!(pages.len(), 1);
    assert_eq!(collapsed_widths(&pages[0]), [2.0, 3.0]);
}

#[test]
fn collapses_borders_without_a_carry_over_row_group_where_it_does_not_show() {
    // A header that shows only after the table broke: on the first page, the table's 4px top
    // border is that of the first row, not the 6px border below the header.
    let (_, pages) = render_document(&format!(
        "<style>@page {{ size: 400px 300px; margin: 20px }} body {{ margin: 0; font: 14px/20px Inter }}
         table {{ border-collapse: collapse; width: 100%; border-top: 4px solid }} td {{ padding: 0 }}
         thead {{ -fagus-repeat: at-break }} thead td {{ border-bottom: 6px solid red }}</style>
         <table><thead><tr><td>Brought forward</td></tr></thead><tbody>{}</tbody></table>",
        "<tr><td>r</td></tr>".repeat(30)
    ));
    assert!(pages.len() > 1);
    let first = collapsed_lines(&pages[0]);
    assert_eq!(first.iter().map(|&(width, _)| width).collect::<Vec<_>>(), [4.0]);
    assert!(collapsed_widths(&pages[1]).contains(&6.0));
}

#[test]
fn draws_the_borders_of_a_breaking_row_without_a_carry_over_row_above_it() {
    // The first row breaks on the first page, where its cell draws its collapsed borders
    // itself: its top border is the head row's 2px border, not the 6px one below the
    // carry-over row, centered on the line as the table draws it.
    let lines = "x<br>".repeat(30);
    let (document, pages) = render_document(&format!(
        "<style>@page {{ size: 400px 300px; margin: 20px }} body {{ margin: 0; font: 14px/20px Inter }}
         table {{ border-collapse: collapse; width: 100% }} td, th {{ padding: 0 }}
         th {{ border-bottom: 2px solid }}
         .carry {{ -fagus-repeat: at-break }} .carry td {{ border-bottom: 6px solid red }}</style>
         <table><thead><tr><th>Item</th></tr><tr class=carry><td>Brought forward</td></tr></thead>
         <tbody><tr><td class=big>{lines}</td></tr></tbody></table>"
    ));
    assert!(pages.len() > 1);
    let (_, top, cell) = find(&pages[0], &document, "big").expect("cell");
    let crate::fragment::FragmentKind::Box { style, .. } = &cell.kind else {
        panic!("a cell box");
    };
    assert_eq!(style.border_width.top, 2.0);
    let head = collapsed_lines(&pages[0])
        .into_iter()
        .find(|&(width, _)| (width - 2.0).abs() < 0.01)
        .map(|(_, middle)| middle);
    assert_eq!(head, Some(top + 1.0));
}

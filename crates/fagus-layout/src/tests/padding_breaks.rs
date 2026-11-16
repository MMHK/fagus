//! Where bottom padding that does not fit at the end of a page goes, and other breaks in tables,
//! columns and flex containers, as Chrome prints them. The pages are 300px × 260px with margins
//! of 20px (a page area 220px tall); lines are 20px tall. The numbers are Chrome's.

use super::{contains_text, find, render_document};
use crate::fragment::Page;

/// Renders `content` on pages 300px × 260px with margins of 20px.
fn render(content: &str) -> (fagus_dom::Document, Vec<Page>) {
    render_document(&format!(
        "<style>@page {{ size: 300px 260px; margin: 20px }} body {{ margin: 0; font: 16px/20px Inter }}
         p {{ margin: 0 }}</style>{content}"
    ))
}

/// Lines `l<first>` to `l<last>` of a paragraph.
fn lines(first: usize, last: usize) -> String {
    let mut out = String::new();
    for n in first..=last {
        out.push('l');
        out.push_str(&n.to_string());
        out.push_str("<br>");
    }
    out
}

/// The number of the page (from 0) whose text has `text` in it.
fn page_with(pages: &[Page], text: &str) -> Option<usize> {
    pages.iter().position(|page| {
        page.fragments
            .iter()
            .any(|fragment| contains_text(fragment, text))
    })
}

#[test]
fn adds_a_page_for_the_bottom_padding_of_a_cell_past_the_last_page() {
    // The cell's content fills the second page; Chrome does not break a cell before its bottom
    // padding, which reaches onto a third page.
    let (_, pages) = render(&format!(
        "<table cellpadding=0 cellspacing=0 style='width: 100%'><tr><td style='padding: 24px 0'>
         <div>{}</div></td></tr></table>",
        lines(1, 20)
    ));
    assert_eq!(pages.len(), 3);
}

#[test]
fn moves_a_row_that_breaks_only_in_its_bottom_padding_below_the_padding_of_a_box() {
    // Only the top padding of the box around the table lies above the row: the row moves on all
    // the same, the table's box starting on the first page.
    let (document, pages) = render(
        "<div style='padding-top: 24px'><table class=table cellpadding=0 cellspacing=0>
         <tr><td style='padding-bottom: 24px'><div class=row style='height: 180px'></div></td></tr>
         <tr><td>next</td></tr></table></div>",
    );
    assert!(find(&pages[0], &document, "table").is_some());
    assert!(find(&pages[0], &document, "row").is_none());
    let (_, y, _) = find(&pages[1], &document, "row").expect("the row on the second page");
    assert!((y - 20.0).abs() < 0.01, "{y}");
}

#[test]
fn moves_bottom_padding_alone_below_a_child_that_goes_on_from_the_page_before() {
    // The paragraph started on the first page: Chrome breaks nowhere inside it to keep its last
    // lines with the padding, which goes to the third page alone.
    let (_, pages) = render(&format!(
        "<div style='padding: 24px 0'><div>{}</div></div>",
        lines(1, 20)
    ));
    assert_eq!(pages.len(), 3);
    assert_eq!(page_with(&pages, "l20"), Some(1));
    // A paragraph that starts on the page has its last lines moved along with the padding.
    let (_, pages) = render(&format!(
        "<div style='padding: 24px 0'><div>{}</div><div>{}</div></div>",
        lines(1, 12),
        lines(13, 20)
    ));
    assert_eq!(page_with(&pages, "l18"), Some(1));
    assert_eq!(page_with(&pages, "l19"), Some(2));
}

#[test]
fn breaks_boxes_early_to_the_end_of_the_page() {
    // The paragraph breaks before its last lines, which move with the bottom padding of the box
    // around it, and still reaches down to the end of the page.
    let (document, pages) = render(&format!(
        "<div style='padding: 24px 0'><div class=inner>{}</div></div>",
        lines(1, 9)
    ));
    assert_eq!(page_with(&pages, "l8"), Some(1));
    let (_, y, inner) = find(&pages[0], &document, "inner").expect("the paragraph");
    assert!((y + inner.rect.height.to_px() - 240.0).abs() < 0.01, "{y}");
}

#[test]
fn puts_the_bottom_border_of_a_table_alone_below_rows_that_go_on() {
    // The rows of the table's body go on from the page before: its last row stays, and the
    // spacing and border below it go to the next page.
    let (document, pages) = render(
        "<table style='width: 100%; border: 3px solid'><tr><td style='padding: 0'>
         <div style='height: 250px'></div></td></tr>
         <tr><td style='padding: 0 0 24px'><div style='height: 170px'></div></td></tr>
         <tr><td class=last>next</td></tr></table>",
    );
    assert_eq!(pages.len(), 4);
    assert!(find(&pages[2], &document, "last").is_some());
}

#[test]
fn grows_table_rows_in_columns_to_their_monolithic_content() {
    // Each box of size containment overflows its column; the table grows to hold it, and the
    // second starts at the top of the second column.
    let (document, pages) = render(
        "<div style='width: 100px; columns: 2; gap: 0; column-fill: auto; height: 60px'>
         <div style='display: table; width: 100%'>
         <div class=a style='contain: size; height: 100px'></div>
         <div class=b style='contain: size; height: 100px'></div></div></div>",
    );
    let (ax, ay, _) = find(&pages[0], &document, "a").expect("the first box");
    let (bx, by, _) = find(&pages[0], &document, "b").expect("the second box");
    assert!((ay - by).abs() < 0.01, "{ay} {by}");
    assert!((bx - ax - 50.0).abs() < 0.01, "{ax} {bx}");
}

#[test]
fn gives_column_flex_items_with_a_content_flex_basis_the_height_of_their_content() {
    let (document, pages) = render(
        "<div style='display: flex; flex-direction: column; width: 100px'>
         <div class=item style='flex-basis: content; height: 10px'>
         <div style='height: 50px'></div><div style='height: 100px'></div></div></div>",
    );
    let (_, _, item) = find(&pages[0], &document, "item").expect("the item");
    assert!((item.rect.height.to_px() - 150.0).abs() < 0.01);
}

#[test]
fn ends_flex_lines_in_columns_of_no_height() {
    // The first item does not fit below its top margin in the first column; it goes on at the
    // top of the second (which starts 100px to the right) rather than in ever more columns.
    let (document, pages) = render(
        "<div style='columns: 2; gap: 0; column-fill: auto; width: 200px; height: 0'>
         <div style='display: flex; width: 100px'>
         <div class=a style='width: 50px; contain: size; height: 10px; margin-top: 5px'></div>
         <div style='width: 50px; contain: size; height: 20px'></div></div></div>",
    );
    assert_eq!(pages.len(), 1);
    let (x, y, _) = find(&pages[0], &document, "a").expect("the first item");
    assert!((x - 120.0).abs() < 0.01 && (y - 20.0).abs() < 0.01, "{x} {y}");
}

#[test]
fn balances_columns_around_positioned_boxes_that_cannot_break() {
    // The positioned box of size containment starts halfway down the first column: the columns
    // are as tall as its end, as Chrome balances them.
    let (document, pages) = render(
        "<div class=columns style='width: 200px; columns: 2'>
         <div style='contain: size; height: 100px'></div>
         <div style='position: relative; height: 100px'>
         <div style='position: absolute; contain: size; height: 100px'></div></div>
         <div style='position: relative'>
         <div style='position: absolute; contain: size; height: 100px'></div></div></div>",
    );
    let (_, _, columns) = find(&pages[0], &document, "columns").expect("the columns");
    assert!((columns.rect.height.to_px() - 200.0).abs() < 0.01);
}

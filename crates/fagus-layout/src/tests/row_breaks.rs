//! Which break Chrome takes around rows of tables, grids and flex containers and around
//! columns, and how what goes on after the break is sized. Each case puts a spacer of `height`
//! above the content on pages 400px × 300px with margins of 20px; the numbers are Chrome's,
//! measured with the same font.

use super::{find, render_document};
use crate::fragment::Page;
use std::fmt::Write;

/// Renders `content` below a spacer `height` pixels tall.
fn render(height: u32, content: &str) -> (fagus_dom::Document, Vec<Page>) {
    render_document(&format!(
        "<style>@page {{ size: 400px 300px; margin: 20px }} body {{ margin: 0; font: 14px/20px Inter }}
         p {{ margin: 0 }} i {{ display: block }}</style>
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
fn moves_a_grid_row_whose_stretched_item_avoids_breaking_inside() {
    // The second item fits, but stretched to its row it would break below its content.
    let (document, pages) = render(
        100,
        "<div style='display: grid; grid-template-columns: 1fr 1fr'><p class=a>A</p><p>B</p>
         <div class=long><i style='height: 60px'></i><i style='height: 60px'></i><i style='height: 60px'></i></div>
         <div class=kept style='break-inside: avoid'><i style='height: 30px'></i></div></div>",
    );
    assert_eq!(page_of(&document, &pages, "a"), Some(0));
    assert_eq!(page_of(&document, &pages, "long"), Some(1));
    assert_eq!(page_of(&document, &pages, "kept"), Some(1));
}

#[test]
fn moves_columns_that_would_break_for_the_bottom_border_of_a_box_around() {
    // The columns fit, the box's bottom padding and border do not: Chrome finds no break inside
    // the columns that leaves room for them and moves the box.
    let (document, pages) = render(
        160,
        "<div class=box style='border: 1px solid; padding: 10px'><div style='columns: 2; column-gap: 0'>
         <p>one</p><p>two</p><p>three</p><p>four</p><p>five</p><p>six</p><p>seven</p><p>eight</p>
         </div></div>",
    );
    assert_eq!(page_of(&document, &pages, "box"), Some(1));
}

#[test]
fn moves_a_table_row_whose_cell_breaks_only_in_its_bottom_padding() {
    // The first cell breaks between its lines, the second only in its bottom padding, which makes
    // the row's break as bad as that: the table moves.
    let (document, pages) = render(
        175,
        "<table style='border-collapse: collapse'><tr>
         <td class=a style='padding: 0; vertical-align: top'><p>one</p><p>two</p><p>three</p><p>four</p><p>five</p></td>
         <td style='padding: 0 0 10px; vertical-align: top'><p>a</p><p>b</p><p>c</p><p>d</p></td></tr></table>",
    );
    assert_eq!(page_of(&document, &pages, "a"), Some(1));
}

#[test]
fn keeps_the_row_heights_of_a_table_that_goes_on_wider_than_it_started() {
    // Beside the float, each row has two lines; below it, on the next page, the table is as
    // wide as the page and its text takes one line, but its rows keep their heights.
    let rows = (1..=8).fold(String::new(), |mut rows, row| {
        let _ = write!(
            rows,
            "<tr><td class=r{row}>one two three four five six seven</td></tr>"
        );
        rows
    });
    let (document, pages) = render_document(&format!(
        "<style>@page {{ size: 400px 300px; margin: 20px }} body {{ margin: 0; font: 14px/20px Inter }}
         td {{ border: 1px solid; padding: 0 }}</style>
         <div style='float: left; width: 150px; height: 240px'></div>
         <table style='border-collapse: collapse'>{rows}</table>"
    ));
    assert_eq!(pages.len(), 2);
    let (_, _, first) = find(&pages[0], &document, "r1").expect("first row");
    let (_, _, last) = find(&pages[1], &document, "r8").expect("last row");
    assert!(last.rect.width > first.rect.width);
    assert_eq!(last.rect.height, first.rect.height);
}

#[test]
fn stretched_cloned_flex_items_go_on_with_their_line() {
    // The first item's content needs 248px on the next page with its cloned frame, the second
    // one less: both end where the first one does, as in Chrome.
    let (document, pages) = render_document(
        "<style>@page { size: A4; margin: 18mm 14mm } body { margin: 0 }
         .box { box-decoration-break: clone; border: 2px solid; padding: 8px 12px; margin: 0 0 16px }
         i { display: block; margin: 0 0 8px } p { margin: 0 }</style>
         <div style='height: 850px'></div>
         <div style='display: flex; gap: 12px'>
         <div class='box one' style='flex: 1'><i style='height: 116px'></i><i style='height: 116px'></i><i style='height: 96px'></i></div>
         <div class='box two' style='flex: 1'><i style='height: 77px'></i><i style='height: 39px'></i></div></div>
         <p>After</p>",
    );
    let height = |class: &str| {
        let (_, _, fragment) = find(&pages[1], &document, class).expect("item");
        fragment.rect.height.to_px()
    };
    assert!((height("one") - 248.0).abs() < 0.5, "{}", height("one"));
    assert!((height("two") - 248.0).abs() < 0.5, "{}", height("two"));
}

#[test]
fn lays_out_deeply_nested_tables_that_break_in_time() {
    // Every level of the tables lays out its cells again while it finds its break; without the
    // layouts of the cells kept, that takes time growing exponentially with the depth.
    let depth = 240;
    let html = format!(
        "<style>@page {{ size: A4; margin: 15mm }} body {{ font: 10pt sans-serif }} td {{ border: 1px solid }}</style>{}deep{}",
        "<table><tr><td>a b c</td><td>d e f</td></tr><tr><td>x ".repeat(depth),
        "</td><td>y</td></tr></table>".repeat(depth)
    );
    // As deep as the engine renders it, on a stack as large as the engine's.
    let pages = std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(move || {
            let start = std::time::Instant::now();
            let (_, pages) = render_document(&html);
            (pages.len(), start.elapsed())
        })
        .expect("thread")
        .join()
        .expect("layout");
    assert!(pages.0 > 1);
    assert!(pages.1 < std::time::Duration::from_secs(20));
}

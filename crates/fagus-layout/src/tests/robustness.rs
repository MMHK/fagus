//! Documents that once made layout panic, take time growing with the square of their size or
//! take gigabytes: they lay out, in bounded time and memory. Time is checked in optimized builds
//! only, against the same document a quarter as large (see [`render_linearly`]): a limit on the
//! time itself fails on a machine under load.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::{find, render, render_document, render_document_in, test_fonts};
use crate::Images;
use crate::fragment::{Fragment, FragmentKind, Page};

/// How many times as long as a quarter of a document the whole may take: four times where time
/// grows linearly with the size of the document, sixteen where it grows with its square.
const QUARTER_RATIO: u32 = 10;

/// How long a document may take at all, however long a quarter of it takes: a hundred times
/// what any takes on an idle machine, for layouts that would take minutes or hours.
const LIMIT: Duration = Duration::from_secs(60);

/// Renders `document(size)` and checks (in optimized builds) that it takes less than
/// [`QUARTER_RATIO`] times as long as `document(size / 4)`, and less than [`LIMIT`]. Both are
/// timed in the same test, so that load on the machine slows both; where the first comparison
/// fails, each is timed up to five times more and the fastest times compared, so that bursts of
/// load slowing only one do not fail the test.
fn render_linearly(document: impl Fn(usize) -> String, size: usize) -> Vec<Page> {
    let fonts = test_fonts();
    let time = |size: usize| {
        let html = document(size);
        let start = Instant::now();
        let (_, pages) = render_document_in(&html, &fonts, |_| Images::default());
        (start.elapsed(), pages)
    };
    let (mut quarter, _) = time(size / 4);
    let (mut whole, pages) = time(size);
    if !cfg!(debug_assertions) {
        let slow = |whole: Duration, quarter: Duration| whole >= quarter * QUARTER_RATIO;
        for _ in 0..5 {
            if !slow(whole, quarter) {
                break;
            }
            quarter = quarter.min(time(size / 4).0);
            whole = whole.min(time(size).0);
        }
        assert!(
            !slow(whole, quarter) && whole < LIMIT,
            "took {whole:?}, a quarter of the document {quarter:?}"
        );
    }
    pages
}

/// The fragments of a tree, itself included.
fn count(fragment: &Fragment) -> usize {
    1 + fragment.children.iter().map(count).sum::<usize>()
}

/// The glyphs of the text fragments of a tree.
fn glyphs(fragment: &Fragment) -> usize {
    let own = match &fragment.kind {
        FragmentKind::Text(run) => run.glyphs.len(),
        _ => 0,
    };
    own + fragment.children.iter().map(glyphs).sum::<usize>()
}

#[test]
fn repeats_leaders_of_absurdly_wide_lines_a_bounded_number_of_times() {
    // A margin of -1e30px makes the line huge; repeating the leader across it asked for more
    // memory than there is ("capacity overflow").
    let pages = render("<style>@page { margin: -1e30px } p::after { content: leader('.') }</style><p>x</p>");
    let total: usize = pages.iter().flat_map(|page| &page.fragments).map(glyphs).sum();
    assert!(total <= 10_001, "{total} glyphs");
}

#[test]
fn lays_out_paragraphs_of_many_pages_in_linear_time() {
    // Each page shaped the whole paragraph and broke all of its lines again.
    let pages = render_linearly(
        |size| {
            let words = "lorem ipsum dolor sit amet ".repeat(size);
            format!("<style>p {{ orphans: 3; widows: 3 }}</style><p>{words}</p>")
        },
        30_000,
    );
    assert!(pages.len() > 100);
}

#[test]
fn lays_out_text_without_spaces_in_linear_time() {
    // A run of CJK text is one word with a break opportunity after each character; it was cut
    // into pieces one at a time, copying the rest each time.
    let text = |size| "日本語の文章。".repeat(size);
    render_linearly(|size| format!("<p lang=ja>{}</p>", text(size)), 10_000);
    render_linearly(
        |size| format!("<p lang=ja style='writing-mode: vertical-rl'>{}</p>", text(size)),
        10_000,
    );
}

#[test]
fn lays_out_long_list_items_and_ruby_in_linear_time() {
    let ruby = |size| format!("<ruby>漢<rt>{}</rt></ruby>", "か".repeat(1000)).repeat(size);
    render_linearly(|size| format!("<p>{}</p>", ruby(size)), 300);
    let item = |size| "lorem ipsum dolor sit amet ".repeat(size);
    render_linearly(|size| format!("<ol><li>{}</li></ol>", item(size)), 20_000);
}

#[test]
fn places_cells_that_span_thousands_of_rows_and_columns() {
    // A slot per column and row for the cells spanning them took gigabytes, and finding the
    // rows that span ones hold together looked at every cell for every row on every page.
    let first: String = "<td colspan=1000 rowspan=65534>a</td>".repeat(20);
    let table = |size: usize, collapse: &str| {
        let rows = "<tr><td>b</td></tr>".repeat(size);
        format!("<table border=1 style='border-collapse: {collapse}'><tr>{first}</tr>{rows}</table>")
    };
    render_linearly(|size| table(size, "separate"), 5_000);
    render_linearly(|size| table(size, "collapse"), 5_000);
}

#[test]
fn places_cells_after_cells_spanning_from_rows_above() {
    // Cells skip the slots that cells from rows above take (`a` in the first column of two rows,
    // `c` in the third of three), as in HTML's table model; `e` goes after `c`, `g` too.
    let (document, pages) = render_document(
        "<table style='border-spacing: 0'><tr><td rowspan=2 class=a style='width: 10px; padding: 0'>a</td>
         <td style='width: 20px; padding: 0'>b</td><td rowspan=3 class=c style='width: 30px; padding: 0'>c</td></tr>
         <tr><td class=d style='padding: 0'>d</td><td class=e style='width: 40px; padding: 0'>e</td></tr>
         <tr><td class=f style='padding: 0'>f</td><td class=g style='padding: 0'>g</td></tr></table>",
    );
    let x = |class: &str| find(&pages[0], &document, class).map(|(x, _, _)| x);
    let left = x("a").expect("a");
    assert_eq!(x("d").map(|x| x - left), Some(10.0));
    assert_eq!(x("c").map(|x| x - left), Some(30.0));
    assert_eq!(x("e").map(|x| x - left), Some(60.0));
    assert_eq!(x("f").map(|x| x - left), Some(0.0));
    assert_eq!(x("g").map(|x| x - left), Some(10.0));
}

#[test]
fn stops_paginating_once_the_pages_hold_millions_of_fragments() {
    // The header of a thousand cells with collapsed borders repeats on each of ten thousand
    // pages: six gigabytes.
    let pages = render(
        "<table border=1 style='border-collapse: collapse'><thead><tr><th colspan=1000>h</th></tr></thead>
         <tbody><tr><td style='padding: 1e30px'>b</td></tr></tbody></table>",
    );
    let fragments: usize = pages.iter().flat_map(|page| &page.fragments).map(count).sum();
    assert!(pages.len() < 10_000);
    assert!(fragments < 1_100_000, "{fragments} fragments");
}

#[test]
fn looks_for_lines_below_blocks_only_where_a_marker_waits() {
    // Every block child of a block walked the content after it for lines, for a list marker that
    // was not there: twenty thousand paragraphs (each a sibling) took minutes.
    let pages = render_linearly(|size| format!("<p>{}", "<a><p>".repeat(size)), 20_000);
    assert!(!pages.is_empty());
}

#[test]
fn lays_out_flex_and_grid_containers_of_many_pages_in_linear_time() {
    // A container that breaks across pages was laid out whole again on each page, and the grid
    // items nested in it did the same: a grid of 8000 rows took 9 seconds, nested four deep 32.
    let grid = |depth: usize, size: usize| {
        let open = "<g>".repeat(depth);
        let close = "</g>".repeat(depth);
        format!(
            "<style>g {{ display: grid }}</style>{open}{}{close}",
            "<g>x</g>".repeat(size)
        )
    };
    let pages = render_linearly(|size| grid(1, size), 8_000);
    assert!(pages.len() > 100);
    render_linearly(|size| grid(4, size), 8_000);
    render_linearly(
        |size| {
            format!(
                "<style>f {{ display: flex; flex-wrap: wrap }} i {{ width: 30% }}</style><f>{}</f>",
                "<i>x</i>".repeat(size)
            )
        },
        16_000,
    );
    // A column with a minimum height copied the fragments of all its items on every page.
    render_linearly(
        |size| {
            format!(
                "<style>f {{ display: flex; flex-direction: column; min-height: 10px }}</style><f>{}</f>",
                "<i>x</i>".repeat(size)
            )
        },
        16_000,
    );
}

#[test]
fn lays_out_grids_nested_in_rows_too_short_for_them_in_linear_time() {
    // Each grid shrinks to its fixed row, a second layout of it, which laid out the grids in it
    // twice again: time grew exponentially with the depth, which mistyped documents reach.
    render_linearly(
        |depth| {
            format!(
                "<style>g {{ display: grid; grid-template-columns: repeat(3, 1fr); grid-auto-rows: 10px; \
                 padding: 2px }}</style>{}x{}",
                "<g><i>a</i><i>b</i>".repeat(depth),
                "</g>".repeat(depth)
            )
        },
        40,
    );
}

#[test]
fn finds_the_rows_of_grid_lines_of_thousands_of_items_in_linear_time() {
    // Items past the last of the ten thousand rows a grid may have share that row, one line of
    // thousands of items: whether each starts or ends the line looked at all the others, on
    // every page. Sixteen thousand items took 97 seconds, four thousand (in rows of their own)
    // a few.
    render_linearly(
        |size| {
            format!(
                "<style>g {{ display: grid }}</style><g>{}</g>",
                "<g>x</g>".repeat(size)
            )
        },
        16_000,
    );
}

#[test]
fn numbers_the_pages_of_containers_laid_out_once_for_many_pages() {
    // The layout of a grid that breaks across pages is kept from page to page, but not where
    // its content shows the page number.
    let (document, pages) = render_document(&format!(
        "<style>@page {{ size: 300px 200px; margin: 0 }} body {{ margin: 0; font: 16px/20px Inter }}
           .grid {{ display: grid }} .grid i::after {{ content: counter(page) }}</style>
         <div class=grid>{}</div>",
        "<i>x</i>".repeat(30)
    ));
    assert_eq!(pages.len(), 3);
    for (index, page) in pages.iter().enumerate() {
        let (_, _, grid) = find(page, &document, "grid").expect("grid");
        for number in 1..=3 {
            let shown = super::contains_text(grid, &number.to_string());
            assert_eq!(shown, number == index + 1, "page {index}, number {number}");
        }
    }
}

#[test]
fn shares_fixed_boxes_and_page_styles_between_pages() {
    // Every page laid out its fixed boxes again and made the styles of its page and margin
    // boxes anew: a header of a few hundred words took gigabytes on ten thousand pages.
    let (document, pages) = render_document(&format!(
        "<style>@page {{ size: 400px 300px; @top-center {{ content: counter(page) }} }}
           p {{ break-after: page }}</style>
         <div class=header style='position: fixed; top: 0'>{}</div>{}",
        "header words ".repeat(50),
        "<p>text</p>".repeat(3)
    ));
    assert_eq!(pages.len(), 3);
    let header = |page: usize| find(&pages[page], &document, "header").expect("header").2;
    assert!(header(0).children.shared_with(&header(2).children));
    assert!(Arc::ptr_eq(&pages[0].style, &pages[2].style));
    // The margin boxes still number their pages.
    assert!(super::margin_text(&pages[2]).contains('3'));
}

#[test]
fn lays_out_paragraphs_of_inline_tables_in_linear_time() {
    // Each page shaped the whole paragraph again, laying out every inline table in it: fifty
    // thousand took half a minute.
    let pages = render_linearly(
        |size| {
            format!(
                "<style>@page {{ size: 300px 300px }} p {{ display: inline-table }}
                   p::before {{ content: 'x' }}</style>{}",
                "<p></p>".repeat(size)
            )
        },
        16_000,
    );
    assert!(pages.len() > 50);
}

#[test]
fn measures_nested_balanced_columns_in_linear_time() {
    // Columns nested in the single column in which the columns around them are measured were
    // laid out and balanced to their end on every page, whatever the room on the page.
    let pages = render_linearly(
        |size| {
            format!(
                "<style>dl {{ columns: 2 }}</style><dl><dl><dl>{}</dl></dl></dl>",
                "<p>x</p>".repeat(size)
            )
        },
        4_000,
    );
    assert!(pages.len() > 5);
}

#[test]
fn shapes_runs_of_preserved_spaces_once() {
    // `break-spaces` breaks after each of a megabyte of spaces, which were shaped one by one,
    // and again on the second page.
    let pages = render_linearly(
        |size| {
            format!(
                "<p style='white-space: break-spaces'>{}</p>",
                format!("{}x", " ".repeat(size / 10)).repeat(10)
            )
        },
        400_000,
    );
    assert!(pages.len() > 10);
}

#[test]
fn leaves_rows_without_outlines_without_outline_boxes() {
    // Anonymous rows keep the initial outline width (with the style `none`): each part of a
    // table on a page had a box of its own for their outlines.
    fn outline_boxes(fragment: &Fragment) -> usize {
        let own = matches!(&fragment.kind, FragmentKind::Box { style, node: None, .. }
            if style.outline_width > 0.0);
        usize::from(own) + fragment.children.iter().map(outline_boxes).sum::<usize>()
    }
    let pages = render("<p style='display: inline-table'>x</p>".repeat(3).as_str());
    let total: usize = pages
        .iter()
        .flat_map(|page| &page.fragments)
        .map(outline_boxes)
        .sum();
    assert_eq!(total, 0);
}

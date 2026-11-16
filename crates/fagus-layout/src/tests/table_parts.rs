//! Table cells and the backgrounds of rows as Chrome lays them out (from the Web Platform Tests
//! in css-tables).

use fagus_style::values::Rgba;

use super::{find, render_document};
use crate::{Fragment, FragmentKind};

fn page(html: &str) -> String {
    format!(
        "<!DOCTYPE html><style>@page {{ size: 600px 400px; margin: 0 }} body {{ margin: 0; font: 12px/16px Inter }}</style>{html}"
    )
}

#[test]
fn measures_cells_without_the_percentage_heights_of_scroll_containers() {
    // A scroll container with a percentage height counts as having none while the cell is
    // measured: the cell is as tall as its own height says, and the scroll container then fills
    // it (table-cell-child-overflow-measure).
    let (document, pages) = render_document(&page(
        "<div class=c style='width: 100px; height: 100px; display: table-cell'>
           <div class=s style='overflow: auto; height: 100%'><div style='height: 200px'></div></div></div>",
    ));
    let (_, _, cell) = find(&pages[0], &document, "c").expect("cell");
    assert_eq!(cell.rect.height.to_px(), 100.0);
    let (_, _, scroller) = find(&pages[0], &document, "s").expect("scroller");
    assert_eq!(scroller.rect.height.to_px(), 100.0);
    // Its minimum height still counts (percentage-sizing-of-table-cell-007).
    let (document, pages) = render_document(&page(
        "<table style='height: 100%'><td class=c><div style='width: 100px; min-height: 100px; height: 100%; overflow-y: scroll'>
           <div style='height: 200px'></div></div></td></table>",
    ));
    let (_, _, cell) = find(&pages[0], &document, "c").expect("cell");
    assert_eq!(cell.rect.height.to_px(), 102.0);
}

#[test]
fn row_groups_of_fixed_heights_give_their_rows_the_room_left() {
    // A body 170px tall shares the room its two rows leave between them by their heights, a
    // group of divs as well; a group too short for its rows keeps them (Chrome 157, probe os4;
    // WPT css-break table/overflow-scroll-section).
    let (document, pages) = render_document(&page(
        "<table style='border-spacing: 0'><tbody style='height: 170px'>
           <tr><td class=a style='padding: 0'>a</td></tr><tr><td class=b style='padding: 0'>b</td></tr></tbody></table>
         <div style='display: table'><div style='display: table-row-group; height: 120px'>
           <div style='display: table-row'><div class=c style='display: table-cell'>c</div></div></div></div>
         <table style='border-spacing: 0'><tbody style='height: 5px'><tr><td class=d style='padding: 0'>d</td></tr></tbody></table>",
    ));
    for (class, y, height) in [
        ("a", 0.0, 85.0),
        ("b", 85.0, 85.0),
        ("c", 170.0, 120.0),
        ("d", 290.0, 16.0),
    ] {
        let (_, top, cell) = find(&pages[0], &document, class).expect("cell");
        assert_eq!((top, cell.rect.height.to_px()), (y, height), "{class}");
    }
}

/// The backgrounds drawn in a fragment tree, with their rectangles relative to the page: the
/// boxes without an element that have a background color.
fn colored(fragment: &Fragment, origin: (f32, f32), out: &mut Vec<(f32, f32, f32, f32, Rgba)>) {
    let (x, y) = (
        origin.0 + fragment.rect.x.to_px(),
        origin.1 + fragment.rect.y.to_px(),
    );
    if let FragmentKind::Box {
        style, node: None, ..
    } = &fragment.kind
        && !style.background_color.is_transparent()
    {
        out.push((
            x,
            y,
            fragment.rect.width.to_px(),
            fragment.rect.height.to_px(),
            style.background_color,
        ));
    }
    for child in &fragment.children {
        colored(child, (x, y), out);
    }
}

#[test]
fn fills_cells_that_span_rows_with_the_color_of_their_row() {
    // The row's image is placed in the row, but its color fills all of a cell that starts in
    // it, also below the row (tentative/paint/background-image-row).
    let (_, pages) = render_document(&page(
        "<style>td { padding: 0 }</style><table style='border-spacing: 0'>
           <tr style='background: linear-gradient(red, blue) no-repeat, lime'><td style='height: 20px; width: 20px'></td><td rowspan=2 style='width: 20px'></td></tr>
           <tr><td style='height: 30px'></td></tr></table>",
    ));
    let mut found = Vec::new();
    colored(&pages[0].fragments[0], (0.0, 0.0), &mut found);
    let lime = Rgba::from_u8(0, 255, 0, 1.0);
    let heights: Vec<f32> = found
        .iter()
        .filter(|(.., color)| *color == lime)
        .map(|&(_, _, _, height, _)| height)
        .collect();
    assert!(heights.contains(&50.0), "{found:?}");
}

#[test]
fn ends_row_backgrounds_halfway_into_collapsed_borders() {
    // A row of two 50px cells in a table with a 10px outer border, collapsed: the row's color
    // reaches halfway into that border: 110px wide and 60px tall from (5, 5) (Chrome 157).
    let (_, pages) = render_document(&page(
        "<style>td { width: 50px; height: 50px; padding: 0 }</style>
         <table style='border: 10px solid transparent; border-collapse: collapse'>
           <tr style='background: lime'><td></td><td></td></tr></table>",
    ));
    let mut found = Vec::new();
    colored(&pages[0].fragments[0], (0.0, 0.0), &mut found);
    let lime = Rgba::from_u8(0, 255, 0, 1.0);
    let rows: Vec<(f32, f32, f32, f32)> = found
        .iter()
        .filter(|(.., color)| *color == lime)
        .map(|&(x, y, width, height, _)| (x, y, width, height))
        .collect();
    let left = rows.iter().map(|row| row.0).fold(f32::INFINITY, f32::min);
    let right = rows.iter().map(|row| row.0 + row.2).fold(0.0, f32::max);
    assert_eq!((left, right - left), (5.0, 110.0), "{rows:?}");
    assert!(
        rows.iter()
            .all(|row| (row.1 - 5.0).abs() < 0.01 && (row.3 - 60.0).abs() < 0.01),
        "{rows:?}"
    );
}

#[test]
fn grows_vertical_tables_to_fit_their_columns() {
    // A table whose lines run down the page is as long as its content across them, however
    // small its `inline-size` or `max-inline-size` (table-intrinsic-size-003 and -004).
    for size in ["inline-size", "max-inline-size"] {
        let (document, pages) = render_document(&page(&format!(
            "<div style='width: min-content'><div class=t style='writing-mode: vertical-rl; display: table; {size}: 30px; block-size: 100px; border-inline: 10px solid'>
               <div style='inline-size: 80px; block-size: 100px'></div></div></div>"
        )));
        let (_, _, table) = find(&pages[0], &document, "t").expect("table");
        assert_eq!(
            (table.rect.width.to_px(), table.rect.height.to_px()),
            (100.0, 100.0),
            "{size}"
        );
    }
}

#[test]
fn shares_the_rest_among_percentage_columns_by_their_percentages() {
    // Six columns of 13 %, the first at least 152px wide for its content: what the table has
    // beyond their percentages goes to all six alike (their percentages are), as in Chrome,
    // not in proportion to their widths. The likert scale of a printed survey.
    let cells = "<td class=b style='width: 13%'></td>".repeat(5);
    let (document, pages) = render_document(&page(&format!(
        "<table style='width: 600px; border-spacing: 0'><tr>\
         <td class=a style='width: 13%'><div style='width: 150px'></div></td>{cells}</tr></table>"
    )));
    let (_, _, first) = find(&pages[0], &document, "a").expect("first cell");
    let (_, _, other) = find(&pages[0], &document, "b").expect("other cell");
    // 152 + 5 × 78 = 542 at their percentages; the 58 left go to each by a sixth.
    assert!(
        (first.rect.width.to_px() - (152.0 + 58.0 / 6.0)).abs() < 0.1,
        "{}",
        first.rect.width.to_px()
    );
    assert!(
        (other.rect.width.to_px() - (78.0 + 58.0 / 6.0)).abs() < 0.1,
        "{}",
        other.rect.width.to_px()
    );
}

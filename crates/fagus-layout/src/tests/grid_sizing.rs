//! Grid sizing and placement that the Web Platform Tests of `css/css-grid` measure, as Chrome 157
//! prints it: widths that follow from the height of the rows, collapsed `auto-fit` rows, line
//! names in repetitions and subgrids, `grid` shorthands with `subgrid`, flexible rows whose
//! factors add up to less than one, stretched replaced elements, the painting order of items and
//! the first line of the block around a grid. The numbers are Chrome's.

use std::sync::Arc;

use super::{find, render_document_with};
use crate::{Image, ImageFormat, Images};

/// The left, top, width and height of the boxes with the given classes on the first page, from
/// the top left corner of the page area.
fn boxes(html: &str, classes: &[&str]) -> Vec<[f32; 4]> {
    boxes_with(html, classes, Images::default())
}

/// [`boxes`] with images.
fn boxes_with(html: &str, classes: &[&str], images: Images) -> Vec<[f32; 4]> {
    let (document, pages) = render_document_with(
        &format!(
            "<!DOCTYPE html><style>body {{ margin: 0; font: 12px/16px Inter }}</style><div class=origin></div>{html}"
        ),
        |_| images,
    );
    let (left, top, _) = find(&pages[0], &document, "origin").expect("origin");
    classes
        .iter()
        .map(|class| {
            let (x, y, fragment) = find(&pages[0], &document, class).expect(class);
            [
                x - left,
                y - top,
                fragment.rect.width.to_px(),
                fragment.rect.height.to_px(),
            ]
        })
        .collect()
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.1
}

#[test]
fn sizes_columns_from_the_height_of_the_rows() {
    // An item with a ratio and a percentage height in a fixed row is as wide as the row makes
    // it (WPT `css-grid/grid-items/aspect-ratio-001`); in an `auto` row, the columns are sized
    // again with the row the other item gives (`grid-item-inline-contribution-002`).
    let found = boxes(
        "<div style='display: inline-grid; grid-template-rows: 100px'>
           <div class=fixed style='aspect-ratio: 1 / 1; height: 100%'></div>
         </div>
         <div style='display: grid; width: 0; grid-template: auto / auto auto'>
           <div class=again style='aspect-ratio: 1 / 1; height: 100%'></div>
           <div class=tall style='height: 80px'></div>
         </div>",
        &["fixed", "again", "tall"],
    );
    assert!(near(found[0][2], 100.0) && near(found[0][3], 100.0), "{found:?}");
    assert!(near(found[1][2], 80.0) && near(found[1][3], 80.0), "{found:?}");
    assert!(near(found[2][0], 80.0), "{found:?}");
}

#[test]
fn stretches_replaced_items_through_their_ratio() {
    // An image stretched along the column takes its width from the area's height (WPT
    // `css-grid/alignment/replaced-alignment-with-aspect-ratio-009`); stretched both ways it
    // fills the area, also where that makes it shorter (`…-003`).
    let mut images = Images::default();
    images.insert(
        "pic.png",
        Arc::new(Image {
            data: Arc::new(Vec::new()),
            format: ImageFormat::Png,
            width: 50.0,
            height: 50.0,
            natural: (true, true),
            ratio: Some(1.0),
            inline: false,
            orientation: 1,
            filters: Arc::from([]),
            pad: 0,
        }),
    );
    images.insert(
        "tall.png",
        Arc::new(Image {
            data: Arc::new(Vec::new()),
            format: ImageFormat::Png,
            width: 10.0,
            height: 20.0,
            natural: (true, true),
            ratio: Some(0.5),
            inline: false,
            orientation: 1,
            filters: Arc::from([]),
            pad: 0,
        }),
    );
    let found = boxes_with(
        "<div style='display: grid; grid-template: 100px / 200px; width: 200px'>
           <img class=ratio src=pic.png style='align-self: stretch'>
         </div>
         <div style='display: grid; grid-template: 100px / 100px; width: 100px'>
           <img class=both src=tall.png style='place-self: stretch'>
         </div>",
        &["ratio", "both"],
        images,
    );
    assert!(near(found[0][2], 100.0) && near(found[0][3], 100.0), "{found:?}");
    assert!(near(found[1][2], 100.0) && near(found[1][3], 100.0), "{found:?}");
}

#[test]
fn collapses_empty_auto_fit_rows() {
    // Of `repeat(auto-fit, 25px)` rows in 200px, those without items collapse, and
    // `space-evenly` spreads the others (WPT `css-grid/alignment/grid-content-distribution-with-collapsed-tracks-008`).
    let found = boxes(
        "<div style='display: grid; width: 200px; height: 200px; grid-template-columns: repeat(auto-fit, 25px);
                     grid-template-rows: repeat(auto-fit, 25px); place-content: space-evenly'>
           <div class=a style='grid-row: 1; grid-column: 2'></div>
           <div class=b style='grid-row: 3; grid-column: 5'></div>
           <div style='grid-row: 4; grid-column: 5'></div>
           <div style='grid-row: 6; grid-column: 1'></div>
           <div class=c style='grid-row: 6; grid-column: 7'></div>
         </div>",
        &["a", "b", "c"],
    );
    // Four rows and four columns are left, with 20px around each.
    assert_eq!(
        found.iter().map(|found| (found[0], found[1])).collect::<Vec<_>>(),
        [(65.0, 20.0), (110.0, 65.0), (155.0, 155.0)]
    );
}

#[test]
fn names_lines_in_and_after_automatic_repetitions() {
    // `[a-start] repeat(auto-fill, 10px [a-start]) [a-end]`: the name after the repetition is
    // the last line, the names inside it repeat (WPT `css-grid/placement/grid-placement-using-named-grid-lines-002`).
    let found = boxes(
        "<div style='display: grid; width: 100px; height: 100px;
                     grid-template-columns: [a-start] repeat(auto-fill, 10px) [a-end];
                     grid-template-rows: [a-start] repeat(auto-fill, 10px [a-start]) [a-end]'>
           <div class=area style='grid-area: a'></div>
           <div class=third style='grid-row: a-start 3; grid-column: 1'></div>
         </div>",
        &["area", "third"],
    );
    assert!(near(found[0][2], 100.0) && near(found[0][3], 100.0), "{found:?}");
    assert!(near(found[1][1], 20.0), "{found:?}");
}

#[test]
fn takes_subgrids_from_grid_shorthands() {
    // `grid: subgrid / subgrid` makes a subgrid: its items take the parent's tracks (WPT
    // `css-grid/subgrid/grid-gap-001`), and `repeat(auto-fill, [x])` names as many lines as
    // it spans (`repeat-auto-fill-002`).
    let found = boxes(
        "<div style='display: grid; grid-template: 20px 30px / 40px 50px'>
           <div style='display: grid; grid: subgrid / subgrid; grid-area: 1 / 1 / 3 / 3'>
             <div class=second style='grid-area: 2 / 2'></div>
           </div>
         </div>
         <div style='display: grid; grid-template-columns: repeat(4, 10px)'>
           <div style='display: grid; grid-template-columns: subgrid repeat(auto-fill, [x]); grid-column: span 4'>
             <div class=named style='grid-column: x 3 / x 4'></div>
           </div>
         </div>",
        &["second", "named"],
    );
    assert_eq!(found[0], [40.0, 20.0, 50.0, 30.0]);
    assert!(near(found[1][0], 20.0) && near(found[1][2], 10.0), "{found:?}");
}

#[test]
fn sizes_flexible_rows_below_one_again_in_the_height_they_give() {
    // Two rows of `minmax(0, 0.4fr)` for an item 100px tall make the grid 80px tall, and are
    // 32px each in it (WPT `css-grid/layout-algorithm/flex-sizing-rows-indefinite-height-002`).
    let found = boxes(
        "<div class=grid style='display: grid; width: 100px; grid-template-rows: repeat(2, minmax(0, 0.4fr))'>
           <div class=item style='grid-row: 2; height: 100px'></div>
         </div>",
        &["grid", "item"],
    );
    assert!(near(found[0][3], 80.0), "{found:?}");
    assert!(near(found[1][1], 32.0), "{found:?}");
}

#[test]
fn keeps_aligned_items_at_least_as_wide_as_their_content() {
    // A start-aligned item in a 10px column is as wide as its min-content width (WPT
    // `css-grid/grid-items/grid-minimum-size-grid-items-014`); preserved white space alone
    // makes no item, which would add a row (`whitespace-in-grid-item-001`).
    let found = boxes(
        "<div class=grid style='display: grid; grid: 10px / 10px; white-space: pre'>
           <div class=item style='justify-self: start; align-self: start'><div style='width: 100px; height: 100px'></div></div> &#9;
         </div>",
        &["grid", "item"],
    );
    assert!(near(found[1][2], 100.0), "{found:?}");
    assert!(near(found[0][3], 10.0), "{found:?}");
}

#[test]
fn paints_items_in_document_order_whatever_their_rows() {
    // The item placed in the second row comes first in the document and paints first, under
    // the one in the first row that overlaps it (WPT `css-grid/grid-items/grid-items-painting-order-dom-001`).
    let (document, pages) = render_document_with(
        "<!DOCTYPE html><style>body { margin: 0 }</style>
         <div class=grid style='display: grid; grid-template: 50px 50px / 50px'>
           <div class=low style='grid-row: 2; width: 100px; height: 100px'></div>
           <div class=high style='grid-row: 1 / 3; width: 100px; height: 100px'></div>
         </div>",
        |_| Images::default(),
    );
    let (_, _, grid) = find(&pages[0], &document, "grid").expect("grid");
    let classes: Vec<String> = grid
        .children
        .iter()
        .filter_map(|child| match &child.kind {
            crate::FragmentKind::Box { node: Some(node), .. } => document
                .element(*node)
                .and_then(|data| data.classes.first().map(ToString::to_string)),
            _ => None,
        })
        .collect();
    assert_eq!(classes, ["low", "high"]);
}

#[test]
fn keeps_the_first_line_of_a_block_out_of_grid_items() {
    fn sizes(fragment: &crate::Fragment, found: &mut Vec<f32>) {
        if let crate::FragmentKind::Text(run) = &fragment.kind {
            found.push(run.size);
        }
        for child in &fragment.children {
            sizes(child, found);
        }
    }
    // A grid is no block container: the first line of the body does not reach into its items
    // (WPT `css-grid/grid-model/grid-first-line-002`).
    let (document, pages) = render_document_with(
        "<!DOCTYPE html><style>body { margin: 0; font: 16px Inter } body::first-line { font-size: 40px }</style>
         <div class=grid style='display: grid'><p class=text style='margin: 0'>Text in a grid item</p></div>",
        |_| Images::default(),
    );
    let (_, _, text) = find(&pages[0], &document, "text").expect("text");
    let mut found = Vec::new();
    sizes(text, &mut found);
    assert!(
        !found.is_empty() && found.iter().all(|&size| near(size, 16.0)),
        "{found:?}"
    );
}

//! Self-alignment of grid and flex items that the Web Platform Tests of `css/css-align` and
//! `css/css-grid` measure, as Chrome 157 prints it: `safe` alignment, `start` and `end` under
//! `flex-wrap: wrap-reverse`, `left`, `right`, `self-start` and `self-end` in right-to-left grids,
//! baseline alignment along the row, static positions of absolutely positioned children of grids,
//! grids that cannot be subgrids, and baselines in vertical grids. The numbers are Chrome's.

use super::{find, render_document};

/// The left, top, width and height of the boxes with the given classes on the first page, from
/// the top left corner of the page area.
fn boxes(html: &str, classes: &[&str]) -> Vec<[f32; 4]> {
    let (document, pages) = render_document(&format!(
        "<!DOCTYPE html><style>body {{ margin: 0; font: 12px/16px Inter }}</style><div class=origin></div>{html}"
    ));
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
fn safe_alignment_starts_items_that_overflow() {
    // An item 40px tall in a 30px area: `safe end` and `safe center` put it at the start, the
    // others let it overflow at the start (WPT `css-align/self-alignment/self-align-safe-unsafe-
    // grid-001`, `…-flex-001`); along the row in a right-to-left grid the start is the right
    // (`…-grid-003`).
    let found = boxes(
        "<style>.g { display: grid; grid: 30px / 30px; width: 30px; height: 30px }
                .f { display: flex; width: 30px; height: 30px }
                .t { height: 40px; width: 10px }
                .w { width: 40px; height: 10px }</style>
         <div class=g><div class='a t' style='align-self: safe end'></div></div>
         <div class=g><div class='b t' style='align-self: end'></div></div>
         <div class=g><div class='c t' style='align-self: safe center'></div></div>
         <div class=g><div class='d t' style='align-self: unsafe center'></div></div>
         <div class=f><div class='e t' style='align-self: safe end'></div></div>
         <div class=f><div class='h t' style='align-self: end'></div></div>
         <div class=g style='direction: rtl'><div class='i w' style='justify-self: safe end'></div></div>
         <div class=g style='direction: rtl'><div class='j w' style='justify-self: end'></div></div>",
        &["a", "b", "c", "d", "e", "h", "i", "j"],
    );
    assert!(near(found[0][1], 0.0), "{found:?}");
    assert!(near(found[1][1], 20.0), "{found:?}");
    assert!(near(found[2][1], 60.0), "{found:?}");
    assert!(near(found[3][1], 85.0), "{found:?}");
    assert!(near(found[4][1], 120.0), "{found:?}");
    assert!(near(found[5][1], 140.0), "{found:?}");
    // Right-aligned at the start (the right), or past the left side at the end.
    assert!(near(found[6][0], -10.0), "{found:?}");
    assert!(near(found[7][0], 0.0), "{found:?}");
}

#[test]
fn start_and_end_go_by_the_writing_mode_in_wrap_reverse_lines() {
    // `wrap-reverse` turns the cross axis over for `flex-start` but not for `start` (WPT
    // `css-align/self-alignment/self-align-start-end-flex-001`).
    let found = boxes(
        "<div style='display: flex; flex-wrap: wrap-reverse; height: 100px; width: 200px'>
           <div class=a style='align-self: flex-start; width: 20px; height: 10px'></div>
           <div class=b style='align-self: start; width: 20px; height: 10px'></div>
           <div class=c style='align-self: flex-end; width: 20px; height: 10px'></div>
           <div class=d style='align-self: end; width: 20px; height: 10px'></div>
         </div>",
        &["a", "b", "c", "d"],
    );
    assert!(near(found[0][1], 90.0), "{found:?}");
    assert!(near(found[1][1], 0.0), "{found:?}");
    assert!(near(found[2][1], 0.0), "{found:?}");
    assert!(near(found[3][1], 90.0), "{found:?}");
}

#[test]
fn justifies_by_sides_and_own_direction_in_right_to_left_grids() {
    // `left` and `right` go by the sides, `self-start` and `self-end` by the item's direction
    // (WPT `css-align/self-alignment/block-justify-self`).
    let found = boxes(
        "<div style='display: grid; width: 300px; direction: rtl'>
           <div class=a style='justify-self: left; width: 100px; height: 5px'></div>
           <div class=b style='justify-self: right; width: 100px; height: 5px'></div>
           <div class=c style='justify-self: start; width: 100px; height: 5px'></div>
           <div class=d style='justify-self: self-start; direction: ltr; width: 100px; height: 5px'></div>
           <div class=e style='justify-self: self-end; direction: ltr; width: 100px; height: 5px'></div>
         </div>",
        &["a", "b", "c", "d", "e"],
    );
    let lefts: Vec<f32> = found.iter().map(|found| found[0]).collect();
    for (left, expected) in lefts.iter().zip([0.0, 200.0, 200.0, 0.0, 200.0]) {
        assert!(near(*left, expected), "{lefts:?}");
    }
}

#[test]
fn baseline_justified_items_keep_their_width() {
    // `justify-self: baseline` does not stretch an item: it is as wide as its content and
    // starts the column (WPT `css-grid/alignment/self-baseline/grid-self-baseline-horiz-004`).
    let found = boxes(
        "<div style='display: grid; width: 300px; justify-items: baseline'>
           <div class=a><span style='display: inline-block; width: 40px; height: 10px'></span></div>
         </div>",
        &["a"],
    );
    assert!(near(found[0][0], 0.0) && near(found[0][2], 40.0), "{found:?}");
}

#[test]
fn aligns_absolutely_positioned_children_in_the_content_box_of_their_grid() {
    // A child of a grid that is not its containing block has its static position aligned by
    // `justify-self` and `align-self` in the grid's content box, whatever its tracks: on the
    // left for `end` in a right-to-left grid.
    let found = boxes(
        "<style>.g { display: grid; grid-template-columns: 100px 100px; width: 300px; height: 40px;
                     padding: 5px 7px 11px 13px; border: 3px solid }
                .a { position: absolute; width: 50px; height: 20px }</style>
         <div class=g><div class='a e' style='justify-self: end; align-self: end'></div></div>
         <div class=g style='direction: rtl'><div class='a r' style='justify-self: end'></div></div>
         <div class=g style='direction: rtl'><div class='a s' style='justify-self: center'></div></div>",
        &["e", "r", "s"],
    );
    assert!(near(found[0][0], 266.0) && near(found[0][1], 28.0), "{found:?}");
    assert!(near(found[1][0], 16.0), "{found:?}");
    assert!(near(found[2][0], 141.0), "{found:?}");
}

#[test]
fn independent_grids_are_not_subgrids() {
    // A `subgrid` that is absolutely positioned or contains its layout is `none`: its item takes
    // its first own row, not the parent's empty one (WPT `css-grid/subgrid/independent-
    // formatting-context`); size containment alone keeps it a subgrid.
    let found = boxes(
        "<style>.g { position: relative; display: grid; grid-template-rows: 0px 100px; width: 100px }
                .s { display: grid; grid-template-rows: subgrid; grid-row: 1 / 3; width: 100px;
                     height: 100px }
                .i { grid-row: 2 }</style>
         <div class=g><div class=s style='position: absolute'><div class='i a'>x</div></div></div>
         <div class=g><div class=s style='contain: layout'><div class='i b'>x</div></div></div>
         <div class=g><div class=s style='container-type: size'><div class='i c'>x</div></div></div>
         <div class=g><div class=s style='contain: size'><div class='i d'>x</div></div></div>",
        &["a", "b", "c", "d"],
    );
    // In two `auto` rows of their own, which share the free space, the items are 58px tall
    // 42px down; in the parent's rows the item fills the second, 100px tall.
    for (found, top) in found[..3].iter().zip([42.0, 142.0, 242.0]) {
        assert!(near(found[1], top) && near(found[3], 58.0), "{found:?}");
    }
    assert!(near(found[3][1], 300.0) && near(found[3][3], 100.0), "{found:?}");
}

#[test]
fn aligns_baselines_in_vertical_grids() {
    // Items of a `vertical-lr` grid that share a row align their baselines across it, which lie
    // their descent from their left sides (WPT `css-grid/alignment/self-baseline/
    // grid-self-baseline-vertical-lr-001`).
    let found = boxes(
        "<div style='display: grid; writing-mode: vertical-lr; grid-auto-flow: column;
                     align-items: baseline; justify-content: start; align-content: start;
                     text-orientation: sideways; font-family: Inter; line-height: 1'>
           <div class=a style='font-size: 20px'>x</div>
           <div class=b style='font-size: 40px'>x</div>
         </div>",
        &["a", "b"],
    );
    assert!(near(found[0][0], 3.0) && near(found[1][0], 0.0), "{found:?}");
}

#[test]
fn negative_lines_add_implicit_tracks_before_the_explicit_grid() {
    // Line -7 of five explicit columns is one before the first: a column of `grid-auto-columns`
    // comes before them, and the empty `auto-fit` columns collapse with their gaps (WPT
    // `css-grid/grid-definition/grid-auto-fit-implicit-and-spanning-001`).
    let found = boxes(
        "<style>.g { display: grid; width: 300px; column-gap: 10px;
                     grid-template-columns: repeat(auto-fit, 50px); grid-auto-columns: 30px }
                .g > div { height: 50px }</style>
         <div class=g><div class=a style='grid-column: -7'></div><div class=b style='grid-column: 3'></div></div>
         <div class=g><div class=c style='grid-column: span 2 / 1'></div><div class=d style='grid-column: 1'></div></div>",
        &["a", "b", "c", "d"],
    );
    assert!(near(found[0][0], 0.0) && near(found[0][2], 30.0), "{found:?}");
    assert!(near(found[1][0], 40.0) && near(found[1][2], 50.0), "{found:?}");
    // Two columns of 30px before the first line, and the first explicit one after them.
    assert!(near(found[2][0], 0.0) && near(found[2][2], 70.0), "{found:?}");
    assert!(near(found[3][0], 80.0) && near(found[3][2], 50.0), "{found:?}");
}

#[test]
fn places_items_densely_with_dense_alone() {
    // `grid-auto-flow: dense` (which lightningcss rejects alone) fills the third column beside a
    // card spanning three rows, before items placed down the second (WPT `css-grid/subgrid/
    // subgrid-item-block-size-001`).
    let found = boxes(
        "<style>.g { display: grid; grid-template-columns: repeat(3, 100px); grid-auto-flow: dense; gap: 10px }
                .card { grid-row: auto / span 3; height: 10px } .i { grid-column: 2; height: 10px }</style>
         <div class=g><div class='card a'></div><div class=i></div><div class=i></div><div class=i></div>
         <div class='card b'></div></div>",
        &["a", "b"],
    );
    assert!(near(found[1][0], 220.0) && near(found[1][1], 0.0), "{found:?}");
}

#[test]
fn subgrids_fill_the_tracks_they_span() {
    // A subgrid ignores its own size, its limits and its alignment along a subgridded axis
    // (WPT `css-grid/subgrid/subgrid-stretch`).
    let found = boxes(
        "<style>.g { display: grid; grid-template: 100px / 100px; width: 100px }
                .r { display: grid; grid-template-rows: subgrid; align-self: baseline }
                .c { display: grid; grid-template-columns: subgrid; justify-self: baseline }</style>
         <div class=g><div class='r a' style='height: 50px'></div></div>
         <div class=g><div class='r b' style='min-height: 150px'></div></div>
         <div class=g><div class='c d' style='max-width: 50px'></div></div>
         <div class=g><div class='c e' style='width: 150px'></div></div>",
        &["a", "b", "d", "e"],
    );
    for (found, top) in found.iter().zip([0.0, 100.0, 200.0, 300.0]) {
        assert!(
            near(found[1], top) && near(found[2], 100.0) && near(found[3], 100.0),
            "{found:?}"
        );
    }
}

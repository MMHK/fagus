//! Flex containers that wrap (columns at a definite or maximum height, `wrap-reverse`,
//! `flex-wrap: balance`), tables as flex items and `stretch` heights, as Chrome lays them out.
//! The numbers are Chrome's.

use super::{find, render_document};

/// The positions and sizes of the boxes with the given classes on a page without margins, in
/// CSS px.
fn boxes(html: &str, classes: &[&str]) -> Vec<(f32, f32, f32, f32)> {
    let (document, pages) = render_document(&format!(
        "<!DOCTYPE html><style>@page {{ margin: 0 }} body {{ margin: 0; font: 12px/16px Inter }}</style>{html}"
    ));
    classes
        .iter()
        .map(|class| {
            let (x, y, fragment) = find(&pages[0], &document, class).expect(class);
            (x, y, fragment.rect.width.to_px(), fragment.rect.height.to_px())
        })
        .collect()
}

fn near(found: (f32, f32, f32, f32), wanted: (f32, f32, f32, f32)) -> bool {
    (found.0 - wanted.0).abs() < 0.1
        && (found.1 - wanted.1).abs() < 0.1
        && (found.2 - wanted.2).abs() < 0.1
        && (found.3 - wanted.3).abs() < 0.1
}

#[test]
fn sizes_a_wrapping_column_by_its_lines() {
    // Two lines of 50px wide items, 80px apart: the max-content width holds both
    // (WPT `col-wrap-001`, `col-wrap-020`), at a fixed or a maximum height.
    let html = "<div class=a style='display: flex; flex-flow: column wrap; height: 100px; width: max-content; column-gap: 80px'>
           <div style='width: 10px; flex: 0 0 100px; min-height: 0'></div>
           <div style='width: 10px; flex: 0 0 100px; min-height: 0'></div>
         </div>
         <div class=b style='display: flex; flex-flow: column wrap; max-height: 100px; width: max-content'>
           <div style='width: 40px; flex: 0 0 100px; min-height: 0'></div>
           <div style='width: 30px; flex: 0 0 100px; min-height: 0'></div>
         </div>";
    let found = boxes(html, &["a", "b"]);
    assert!((found[0].2 - 100.0).abs() < 0.1, "{found:?}");
    assert!(
        (found[1].2 - 70.0).abs() < 0.1 && (found[1].3 - 100.0).abs() < 0.1,
        "{found:?}"
    );
}

#[test]
fn wraps_a_column_at_its_maximum_height_and_flexes_its_lines() {
    // WPT `flexbox-flex-wrap-flexing-002`: the first item grows to the height of the second's
    // line, the lines share the width.
    let html = "<div style='display: flex; flex-flow: column wrap; max-height: 100px; width: 100px'>
           <div class=a style='width: 50px; height: 50px; flex-grow: 1'></div>
           <div class=b style='width: 50px; height: 100px'></div>
         </div>";
    let found = boxes(html, &["a", "b"]);
    assert!(near(found[0], (0.0, 0.0, 50.0, 100.0)), "{found:?}");
    assert!(near(found[1], (50.0, 0.0, 50.0, 100.0)), "{found:?}");
}

#[test]
fn reverses_the_margin_boxes_of_lines_that_wrap_in_reverse() {
    // The right margin stays on the right of each item, the top margin above it.
    let html = "<div style='display: flex; flex-flow: column wrap-reverse; height: 100px; width: 200px'>
           <div class=a style='height: 100px; margin-right: 10px'></div>
           <div class=b style='height: 100px; margin-right: 10px'></div>
         </div>
         <div style='display: flex; flex-wrap: wrap-reverse; width: 100px'>
           <div class=c style='width: 100px; height: 20px; margin-top: 10px'></div>
           <div class=d style='width: 100px; height: 20px; margin-top: 10px'></div>
         </div>";
    let found = boxes(html, &["a", "b", "c", "d"]);
    assert!(near(found[0], (100.0, 0.0, 90.0, 100.0)), "{found:?}");
    assert!(near(found[1], (0.0, 0.0, 90.0, 100.0)), "{found:?}");
    assert!(near(found[2], (0.0, 140.0, 100.0, 20.0)), "{found:?}");
    assert!(near(found[3], (0.0, 110.0, 100.0, 20.0)), "{found:?}");
}

#[test]
fn balances_the_lines_of_a_flex_container() {
    // WPT `balance-001`: four items that would wrap three and one go two and two; with a
    // `flex-line-count` of 4, one a line.
    let html = "<div style='display: flex; flex-wrap: balance; width: 100px; gap: 10px'>
           <div class=a style='width: 25px; height: 25px'></div><div style='width: 25px; height: 25px'></div>
           <div class=c style='width: 25px; height: 25px'></div><div style='width: 25px; height: 25px'></div>
         </div>
         <div style='display: flex; flex-wrap: balance; flex-line-count: 4; width: 200px'>
           <div style='width: 100px; height: 25px'></div><div style='width: 100px; height: 25px'></div>
           <div style='width: 100px; height: 25px'></div><div class=d style='width: 100px; height: 25px'></div>
         </div>
         <div class=e style='display: inline-flex; flex-wrap: balance; flex-line-count: 2; gap: 20px'>
           <div style='width: 40px; height: 10px'></div><div style='width: 40px; height: 10px'></div>
           <div style='width: 40px; height: 10px'></div>
         </div>";
    let found = boxes(html, &["a", "c", "d", "e"]);
    assert!(near(found[0], (0.0, 0.0, 25.0, 25.0)), "{found:?}");
    assert!(near(found[1], (0.0, 35.0, 25.0, 25.0)), "{found:?}");
    assert!((found[2].1 - 135.0).abs() < 0.1, "{found:?}");
    // The max-content width of a balanced row of two lines: its widest line.
    assert!((found[3].2 - 100.0).abs() < 0.1, "{found:?}");
}

#[test]
fn sizes_tables_in_flex_containers_by_their_flex_bases() {
    // WPT `table-as-item-specified-width`, `-specified-height`: the flex basis wins over the
    // table's own width and height; `table-as-item-min-content-height-1`: a table does not
    // shrink below its rows, and the next item follows it.
    let html = "<div style='display: flex'>
           <div class=a style='display: table; width: 500px; height: 100px; flex: 0 0 100px'></div>
         </div>
         <div style='display: flex; flex-direction: column; width: 100px'>
           <div class=b style='display: table; width: 100px; height: 500px; flex: 0 0 100px'></div>
         </div>
         <div style='display: flex; flex-direction: column; height: 0; width: 100px'>
           <div class=c style='display: table; min-height: 0; width: 100px'>
             <div style='display: table-cell'><div style='height: 50px'></div></div>
           </div>
           <div class=d style='flex: 0 0 50px'></div>
         </div>";
    let found = boxes(html, &["a", "b", "c", "d"]);
    assert!((found[0].2 - 100.0).abs() < 0.1, "{found:?}");
    assert!((found[1].3 - 100.0).abs() < 0.1, "{found:?}");
    assert!((found[2].3 - 50.0).abs() < 0.1, "{found:?}");
    assert!((found[3].1 - found[2].1 - 50.0).abs() < 0.1, "{found:?}");
}

#[test]
fn takes_the_content_height_as_an_automatic_flex_basis() {
    // A minimum height does not make the flex basis: both items grow from about nothing to
    // share the column (WPT `table-as-item-min-height-1` with a block).
    let html = "<div style='display: flex; flex-direction: column; height: 200px'>
           <div class=a style='min-height: 80px; flex: 1 0 auto'><div style='height: 1px'></div></div>
           <div class=b style='flex: 1 0 1px'></div>
         </div>";
    let found = boxes(html, &["a", "b"]);
    assert!((found[0].3 - 100.0).abs() < 0.1, "{found:?}");
    assert!((found[1].3 - 100.0).abs() < 0.1, "{found:?}");
}

#[test]
fn stretches_absolutely_positioned_boxes_from_their_static_position() {
    // WPT `css-sizing/stretch/positioned-non-replaced-1`: `height: stretch` with both vertical
    // insets `auto` fills the containing block below the static position.
    let html = "<div style='position: relative; width: 150px; height: 150px'>
           <div style='margin-left: 50px; padding-top: 50px'>
             <div class=a style='position: absolute; width: stretch; height: stretch'></div>
           </div>
         </div>";
    let found = boxes(html, &["a"]);
    assert!(near(found[0], (50.0, 50.0, 100.0, 100.0)), "{found:?}");
}

#[test]
fn stretches_items_to_a_single_line_within_its_maximum_height() {
    // WPT `flexbox-definite-sizes-003`: the line is no taller than the container's maximum
    // height, and the stretched item's percentage maximum refers to it.
    let html = "<div style='display: flex; width: 100px; max-height: 100px'>
           <div class=a style='display: flex; width: 100px; align-items: flex-end'>
             <div class=b style='width: 100px; max-height: 100%'><div style='height: 200px'></div></div>
           </div>
         </div>";
    let found = boxes(html, &["a", "b"]);
    assert!((found[0].3 - 100.0).abs() < 0.1, "{found:?}");
    assert!((found[1].3 - 100.0).abs() < 0.1, "{found:?}");
}

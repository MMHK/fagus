//! Layout that the WPT tests of grid lanes measure, which Chrome 157 lays out as blocks (it
//! does not know `display: grid-lanes` without experimental features): `justify-self` and
//! `justify-items` on block-level boxes, `flow-start` and `flow-end` in `align-content`, the
//! line of an empty editing host and boxes whose top margin takes them past the end of the
//! page. The numbers are Chrome's.

use super::{find, render_document};

/// The left, top, width and height of the boxes with the given classes on the first page, from the top
/// left corner of the page area.
fn boxes(html: &str, classes: &[&str]) -> Vec<[f32; 4]> {
    let (document, pages) = render_document(&format!(
        "<!DOCTYPE html><style>body {{ margin: 0 }} i {{ display: inline-block; width: 40px; height: 10px }}</style><div class=origin></div>{html}"
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

#[test]
fn justify_self_shrinks_and_aligns_block_level_boxes() {
    let found = boxes(
        "<div style='width: 300px'>
           <div class=start style='justify-self: start'><i></i></div>
           <div class=center style='justify-self: center'><i></i></div>
           <div class=end style='justify-self: end'><i></i></div>
           <div class=right style='justify-self: right; margin-right: 20px'><i></i></div>
           <div class=stretch style='justify-self: stretch'><i></i></div>
         </div>",
        &["start", "center", "end", "right", "stretch"],
    );
    let lefts: Vec<[f32; 2]> = found.iter().map(|b| [b[0], b[2]]).collect();
    assert_eq!(
        lefts,
        vec![
            [0.0, 40.0],
            [130.0, 40.0],
            [260.0, 40.0],
            [240.0, 40.0],
            [0.0, 300.0]
        ]
    );
}

#[test]
fn justify_items_applies_to_children_unless_auto_margins_or_normal() {
    let found = boxes(
        "<div style='width: 300px; justify-items: center'>
           <div class=auto><i></i></div>
           <div class=normal style='justify-self: normal'><i></i></div>
           <div class=margin style='width: 100px; margin-right: auto'></div>
           <div class=fixed style='width: 100px'></div>
         </div>",
        &["auto", "normal", "margin", "fixed"],
    );
    assert_eq!(found[0][0], 130.0);
    assert_eq!(found[0][2], 40.0);
    assert_eq!((found[1][0], found[1][2]), (0.0, 300.0));
    assert_eq!(found[2][0], 0.0);
    assert_eq!(found[3][0], 100.0);
}

#[test]
fn justify_self_overflows_the_start_unless_safe() {
    // Without `safe`, an end-aligned box wider than its container sticks out on the left.
    let found = boxes(
        "<div style='width: 300px'>
           <div class=unsafe style='justify-self: end; width: 400px; height: 5px'></div>
           <div class=safe style='justify-self: safe end; width: 400px; height: 5px'></div>
         </div>",
        &["unsafe", "safe"],
    );
    assert_eq!(found[0][0], -100.0);
    assert_eq!(found[1][0], 0.0);
}

#[test]
fn justify_self_stretch_stretches_block_level_form_controls() {
    // A block-level `<textarea>` shrinks to fit, unless `justify-self: stretch` stretches it.
    let found = boxes(
        "<div style='width: 300px'>
           <textarea class=normal style='display: block; box-sizing: border-box'></textarea>
           <textarea class=stretch style='display: block; box-sizing: border-box; justify-self: stretch'></textarea>
         </div>",
        &["normal", "stretch"],
    );
    assert!(found[0][2] < 300.0);
    assert_eq!(found[1][2], 300.0);
}

#[test]
fn justify_items_end_goes_left_right_to_left() {
    let found = boxes(
        "<div style='width: 300px; direction: rtl; justify-items: end'>
           <div class=end><i></i></div>
           <div class=left style='justify-self: right'><i></i></div>
         </div>",
        &["end", "left"],
    );
    assert_eq!(found[0][0], 0.0);
    assert_eq!(found[1][0], 260.0);
}

#[test]
fn justify_self_aligns_absolutely_positioned_boxes_at_their_static_position() {
    // The static position of a box that was block-level lies on a line across the content box,
    // which `justify-self` aligns it in.
    let found = boxes(
        "<div style='position: relative; width: 300px; height: 50px'>
           <div class=end style='position: absolute; justify-self: end; width: 60px; height: 10px'></div>
           <div class=center style='position: absolute; justify-self: center; width: 60px; height: 10px'></div>
         </div>
         <div style='position: relative; width: 300px; height: 50px; justify-items: end'>
           <div class=items style='position: absolute; width: 60px; height: 10px'></div>
           <div class=inset style='position: absolute; left: 30px; width: 60px; height: 10px'></div>
         </div>",
        &["end", "center", "items", "inset"],
    );
    assert_eq!(found[0][0], 240.0);
    assert_eq!(found[1][0], 120.0);
    assert_eq!(found[2][0], 240.0);
    assert_eq!(found[3][0], 30.0);
}

#[test]
fn flow_end_aligns_block_content_at_the_end() {
    let found = boxes(
        "<div style='height: 100px; align-content: flow-end'><div class=end style='height: 20px'></div></div>
         <div style='height: 100px; align-content: flow-start'><div class=start style='height: 20px'></div></div>",
        &["end", "start"],
    );
    assert_eq!(found[0][1], 80.0);
    assert_eq!(found[1][1], 100.0);
}

#[test]
fn flow_start_and_end_follow_reversed_flex_directions() {
    // As `flex-start` and `flex-end` (WPT `css-flexbox/alignment/flow-start-flow-end-reverse`).
    let found = boxes(
        "<div style='display: flex; flex-direction: row-reverse; width: 80px; justify-content: flow-start'><i class=start></i></div>
         <div style='display: flex; flex-direction: row-reverse; width: 80px; justify-content: flow-end'><i class=end></i></div>",
        &["start", "end"],
    );
    assert_eq!(found[0][0], 40.0);
    assert_eq!(found[1][0], 0.0);
}

#[test]
fn empty_editing_hosts_have_a_line() {
    let found = boxes(
        "<div class=host contenteditable style='line-height: 20px'></div>
         <div class=spaces contenteditable=true style='line-height: 20px'>  </div>
         <div class=off contenteditable=false style='line-height: 20px'></div>
         <div class=wbr style='line-height: 20px'><wbr></div>",
        &["host", "spaces", "off", "wbr"],
    );
    assert_eq!(found[0][3], 20.0);
    assert_eq!(found[1][3], 20.0);
    assert_eq!(found[2][3], 0.0);
    // A line of nothing but a zero width space is a line too.
    assert_eq!(found[3][3], 20.0);
}

#[test]
fn moves_boxes_whose_margin_passes_the_end_of_the_first_page_to_the_next() {
    // The margin takes the box past the end of the first page, which stays empty; the margin is
    // truncated on the second, where the box starts and breaks onto a third.
    let (document, pages) = render_document(
        "<style>@page { size: 400px 300px; margin: 20px } body { margin: 0 }</style>
         <div class=box style='margin-top: 500px; height: 400px; width: 25px'></div>",
    );
    assert_eq!(pages.len(), 3);
    assert!(
        find(&pages[0], &document, "box").is_none_or(|(_, _, fragment)| fragment.rect.height.to_px() == 0.0)
    );
    let (_, top, fragment) = find(&pages[1], &document, "box").expect("box on the second page");
    assert_eq!((top, fragment.rect.height.to_px()), (20.0, 260.0));
    let (_, _, rest) = find(&pages[2], &document, "box").expect("box on the third page");
    assert_eq!(rest.rect.height.to_px(), 140.0);
}

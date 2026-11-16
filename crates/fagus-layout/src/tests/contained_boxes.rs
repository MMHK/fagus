//! Size and layout containment on grids, fieldsets, buttons, selects and multi-column
//! containers, boxes that keep their display type, baselines of scroll containers, and the
//! boxes of scroll markers and scroll buttons, as Chrome 157 prints them (WPT `css-contain`,
//! `css-display`, `css-overflow/scroll-markers`). The numbers are Chrome's.

use super::{find, render_document};
use crate::{Fragment, FragmentKind};

/// The left, top, width and height of the boxes with the given classes on the first page, from
/// the top left corner of the page area.
fn boxes(html: &str, classes: &[&str]) -> Vec<[f32; 4]> {
    let (document, pages) = render_document(&format!(
        "<!DOCTYPE html><style>@page {{ size: 600px 800px; margin: 0 }} body {{ margin: 0; font: 16px/1 Inter }}</style>{html}"
    ));
    classes
        .iter()
        .map(|class| {
            let (x, y, fragment) = find(&pages[0], &document, class).expect(class);
            [x, y, fragment.rect.width.to_px(), fragment.rect.height.to_px()]
        })
        .collect()
}

/// Whether `a` and `b` are within a hundredth of a pixel of each other.
fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

#[test]
fn keeps_the_template_tracks_of_a_grid_with_size_containment() {
    // The grid sizes its tracks without its items: 0 + 20 + 80 wide, 40 + 20 + 40 high (WPT
    // `contain-size-grid-002`).
    let found = boxes(
        "<div class=g style='position: absolute; contain: size; display: grid; gap: 20px;
           grid-template-columns: auto 80px; grid-template-rows: 40px 40px; font-size: 100px'>&nbsp;</div>",
        &["g"],
    );
    assert_eq!(found[0][2..], [100.0, 100.0]);
}

#[test]
fn sizes_fieldsets_with_size_containment_by_their_padding() {
    let found = boxes(
        "<fieldset class=a style='display: inline-block; contain: size; margin: 0'>
           <legend>legend</legend><div style='width: 50px; height: 50px'></div></fieldset><br>
         <fieldset class=b style='width: 100px; margin: 0'><legend>legend</legend>content</fieldset>
         <fieldset class=c style='width: 100px; height: 40px; margin: 0'><legend>legend</legend>content</fieldset>
         <fieldset class=d style='width: 100px; height: 40px; box-sizing: border-box; margin: 0'>
           <legend>legend</legend>content</fieldset>",
        &["a", "b", "c", "d"],
    );
    // The padding (0.75em at the sides, 0.35em and 0.625em above and below) and the border,
    // without the legend (WPT `contain-size-fieldset-003`).
    assert!(
        near(found[0][2], 28.0) && near(found[0][3], 19.6),
        "{:?}",
        found[0]
    );
    // Without containment the fieldset keeps its padding beside a `width`, and the padding
    // below the legend outside a `height`.
    assert!(near(found[1][2], 128.0), "{:?}", found[1]);
    assert!(near(found[2][3], 59.6), "{:?}", found[2]);
    assert_eq!(found[3][2..], [100.0, 40.0]);
}

#[test]
fn aligns_fieldsets_by_the_content_below_their_legend() {
    // The legend gives no baseline: a fieldset without a border aligns by its content, below
    // the legend and the padding (WPT `contain-size-fieldset-004`).
    let found = boxes(
        "<div class=row style='display: flex; align-items: baseline'><span class=t>A</span>
           <fieldset class=f style='contain: size; border: none; margin: 0'>
             <legend style='font-size: 16px; line-height: 16px'>L</legend><div>B</div></fieldset></div>
         <div class=row2 style='display: flex; align-items: baseline'><span class=t2>A</span>
           <fieldset class=f2 style='contain: size; border: none; margin: 0'><div>B</div></fieldset></div>",
        &["t", "f", "t2", "f2"],
    );
    // The text sits as far below the fieldset's top as the legend is high, more than without
    // one.
    assert!(
        near((found[0][1] - found[1][1]) - (found[2][1] - found[3][1]), 16.0),
        "{found:?}"
    );
}

#[test]
fn centers_legends_lower_than_the_top_border() {
    // The content starts below the top border, the legend in its middle (WPT
    // `contain-inline-size-fieldset`).
    let found = boxes(
        "<fieldset class=f style='border: 20px solid; padding: 0; margin: 0'>
           <legend class=l style='padding: 0'><div style='width: 200px; height: 2px'></div></legend>
           <div class=c style='height: 10px'></div></fieldset>",
        &["f", "l", "c"],
    );
    assert_eq!(found[1][1] - found[0][1], 9.0);
    assert_eq!(found[2][1] - found[0][1], 20.0);
}

#[test]
fn makes_inline_fieldsets_and_buttons_inline_blocks() {
    // A fieldset or button with `display: inline` is an inline-block, one with a table display
    // a block (WPT `display-inline-on-block-container`, `table-display-on-fieldset`).
    let found = boxes(
        "<div style='font-size: 0; line-height: 0'><i style='display: inline-block; width: 20px; height: 20px'></i><fieldset
           class=f style='display: inline; width: 40px; height: 40px; margin: 0; padding: 0; border: none'></fieldset><button
           class=b style='display: inline; width: 40px; height: 40px; margin: 0; padding: 0; border: none'></button></div>
         <div style='font-size: 0; line-height: 0'><span><fieldset class=t style='display: table-cell; width: 40px;
           height: 20px; margin: 0; padding: 0; border: none'></fieldset></span><i class=i style='display: inline-block;
           width: 20px; height: 20px'></i></div>",
        &["f", "b", "t", "i"],
    );
    assert_eq!(found[0], [20.0, 0.0, 40.0, 40.0]);
    assert_eq!(found[1], [60.0, 0.0, 40.0, 40.0]);
    // The table cell is a block in the span: the next box goes below it.
    assert_eq!(found[2][2..], [40.0, 20.0]);
    assert_eq!(found[3][1] - found[2][1], 20.0);
}

#[test]
fn sizes_contained_selects_multicols_and_orthogonal_buttons_as_empty() {
    let found = boxes(
        "<select class=a style='contain: size'><option>a long option text</option></select><br>
         <select class=b></select><br>
         <select class=c size=4><option style='display: none'>a long option text</option></select><br>
         <select class=d size=4></select><br>
         <div class=m style='float: left; contain: size; columns: 3; column-gap: 16px'>text</div>
         <button class=v style='contain: size; writing-mode: vertical-lr; border: 10px solid; padding: 0;
           margin: 0'><div style='width: 100px; height: 100px'></div></button>",
        &["a", "b", "c", "d", "m", "v"],
    );
    // A select as wide as one without options (WPT `contain-size-select-elem-001`); options
    // that are not displayed show no line (`select-4-option-optgroup-display-none`).
    assert_eq!(found[0][2], found[1][2]);
    assert_eq!(found[2][2], found[3][2]);
    // The gaps between the columns stay (WPT `contain-size-multicol-002`).
    assert_eq!(found[4][2], 32.0);
    // An orthogonal button is as wide as its border (WPT `contain-size-button-002`).
    assert_eq!(found[5][2], 20.0);
}

#[test]
fn keeps_the_baseline_of_a_scroll_container_within_its_box() {
    // A scroll container aligns by its bottom border edge where its content's baseline lies
    // further down, like an empty box (WPT `contain-size-block-002`).
    let found = boxes(
        "<div style='display: flex; align-items: baseline'>A<div class=s style='overflow: hidden;
           contain: size; border: 2px solid'><div>inner<br>text</div></div></div>
         <div style='display: flex; align-items: baseline'>A<div class=e style='border: 2px solid'></div></div>",
        &["s", "e"],
    );
    assert_eq!(found[0][3], 4.0);
    assert!(near(found[1][1] - found[0][1], 16.0), "{found:?}");
}

#[test]
fn ignores_forced_breaks_at_the_start_of_layout_containment() {
    // A forced break before the first child of a box with layout containment passes on to no
    // break point (WPT `contain-layout-breaks-002`).
    let found = boxes(
        "<div style='columns: 2; column-gap: 0; column-fill: auto; width: 200px; height: 300px'>
           <div style='border-top: 50px solid'></div>
           <div class=b style='border-top: 50px solid; contain: layout'>
             <div class=o style='border-top: 50px solid; break-before: column'></div></div></div>",
        &["b", "o"],
    );
    assert_eq!(found[0][..2], [0.0, 50.0]);
    assert_eq!(found[1][..2], [0.0, 100.0]);
}

#[test]
fn moves_boxes_of_no_width_below_floats_that_close_in() {
    // Floats on both sides leave less than nothing between them: a box of no width does not
    // fit there either (WPT `contain-inline-size-bfc-floats-001`).
    let found = boxes(
        "<div style='width: 400px'>
           <div style='float: right; width: 200px; height: 100px'></div>
           <div style='float: left; width: 250px; height: 100px'></div>
           <div style='float: right; width: 300px; height: 100px'></div>
           <div class=c style='contain: inline-size; display: flow-root; width: fit-content'>
             <div style='height: 150px'></div></div></div>",
        &["c"],
    );
    assert_eq!(found[0][1], 200.0);
}

#[test]
fn sizes_lines_by_the_text_of_elements_with_display_contents() {
    // The text of an element with `display: contents` has its own line height (WPT
    // `display-contents-line-height`).
    let found = boxes(
        "<div class=p><span style='display: contents; font-size: 30px; line-height: 50px'>A</span><br><span
           style='display: contents; font-size: 30px; line-height: 50px'>B</span></div>",
        &["p"],
    );
    assert_eq!(found[0][3], 100.0);
}

#[test]
fn clips_the_captions_of_tables_that_clip_their_overflow() {
    // The table wrapper box clips them too (WPT `contain-paint-table-002`).
    fn clips(fragment: &Fragment) -> bool {
        matches!(&fragment.kind, FragmentKind::Box { style, node: None, .. }
            if style.overflow_x == fagus_style::values::Overflow::Clip)
            && fragment.children.len() == 2
            || fragment.children.iter().any(clips)
    }
    let (_, pages) = render_document(
        "<div style='display: table; contain: paint; width: 100px'><div style='display: table-caption'>caption</div>
           <div style='display: table-cell'>cell</div></div>",
    );
    assert!(pages[0].fragments.iter().any(clips));
}

#[test]
fn generates_scroll_marker_groups_and_scroll_buttons() {
    let found = boxes(
        "<style>
           .sc { overflow: hidden; height: 40px; scroll-marker-group: before }
           .sc::scroll-marker-group { display: block; height: 20px }
           .sc > div::scroll-marker { content: 'm'; display: inline-block; width: 10px; height: 100px }
           .v { scroll-marker-group: before; height: 40px }
           .n::scroll-button(*) { content: ''; display: block; height: 10px; border: none; padding: 0 }
         </style>
         <div class=sc><div>a</div><div>b</div></div>
         <div class=v><div>a</div></div>
         <div class=n></div><div class=after></div>",
        &["sc", "v", "n", "after"],
    );
    // The group goes before the scroll container, as high as its `height` says: size
    // containment leaves its markers out (WPT `scroll-marker-002`).
    assert_eq!(found[0][1], 20.0);
    // An element that does not scroll has none.
    assert_eq!(found[1][1], 60.0);
    // Four buttons come after the element.
    assert_eq!(found[3][1] - found[2][1], 40.0);
}

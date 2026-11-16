//! Containment, `content-visibility`, `margin-trim`, static positions and the propagation of
//! `overflow` and `writing-mode` to the viewport, as Chrome 157 prints them (WPT `css-contain`,
//! `css-box/margin-trim`, `css-position/static-position`). The numbers are Chrome's.

use std::sync::Arc;

use super::{find, render_document, render_document_with};
use crate::{FragmentKind, Image, ImageFormat, Images};

/// The left, top, width and height of the boxes with the given classes on the first page, from
/// the top left corner of the page area.
fn boxes(html: &str, classes: &[&str]) -> Vec<[f32; 4]> {
    boxes_with(html, classes, Images::default())
}

/// Like [`boxes`], with `images`.
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

/// A PNG image of `width` × `height` pixels.
fn image(width: f32, height: f32) -> Arc<Image> {
    Arc::new(Image {
        data: Arc::new(Vec::new()),
        format: ImageFormat::Png,
        width,
        height,
        natural: (true, true),
        ratio: Some(width / height),
        inline: false,
        orientation: 1,
        filters: Arc::from([]),
        pad: 0,
    })
}

#[test]
fn sizes_replaced_elements_with_size_containment_as_empty() {
    // An image with size containment has no natural size or ratio: it is as large as
    // `contain-intrinsic-size` says, else empty (WPT `css-contain/contain-size-replaced-*`).
    let mut images = Images::default();
    images.insert("pic.png", image(100.0, 50.0));
    let found = boxes_with(
        "<img class=a src=pic.png style='display: block; contain: size'>
         <img class=b src=pic.png style='display: block; contain: size; contain-intrinsic-size: 30px 20px'>
         <img class=c src=pic.png style='display: block; contain: size; width: 40px'>
         <img class=d src=pic.png style='display: block'>
         <canvas class=e width=20 height=10 style='display: block; contain: size; width: 100px'></canvas>",
        &["a", "b", "c", "d", "e"],
        images,
    );
    assert_eq!(found[0][2..], [0.0, 0.0]);
    assert_eq!(found[1][2..], [30.0, 20.0]);
    // A given width does not bring the ratio back.
    assert_eq!(found[2][2..], [40.0, 0.0]);
    assert_eq!(found[3][2..], [100.0, 50.0]);
    // A canvas keeps the ratio of its `width` and `height` attributes (WPT `css-flexbox/
    // canvas-contain-size`).
    assert_eq!(found[4][2..], [100.0, 50.0]);
}

#[test]
fn layout_containment_makes_a_formatting_context_and_no_baseline() {
    // A box with layout containment avoids floats as a new formatting context does (WPT
    // `contain-layout-ifc-022`), and an inline block with it sits on its bottom margin edge
    // (`contain-layout-baseline-005`).
    let found = boxes(
        "<div class=float style='float: left; width: 50px; height: 20px'></div>
         <div class=contained style='contain: layout; height: 10px'></div>
         <div style='clear: both; font: 20px/30px Inter'>x<span class=ib style='display: inline-block;
           contain: layout; width: 10px; height: 10px'>y</span></div>",
        &["float", "contained", "ib"],
    );
    assert_eq!(found[1][0], 50.0);
    // The line box is 30px tall with the baseline 5px below the middle of the text; the inline
    // block's bottom edge sits on it.
    let line_top = 20.0;
    let baseline = found[2][1] + found[2][3];
    assert!(
        baseline > line_top + 15.0 && baseline < line_top + 30.0,
        "{baseline}"
    );
}

#[test]
fn ignores_layout_containment_on_inline_boxes() {
    // Layout and paint containment do not apply to a non-atomic inline box, which is then no
    // containing block of an absolutely positioned box in it (WPT `contain-paint-011`).
    let found = boxes(
        "<div class=cb style='position: relative; width: 100px; height: 100px; margin-left: 20px'>
           <span style='contain: paint'><div class=abs style='position: absolute; bottom: 0;
             right: 0; width: 100%; height: 100%'></div></span></div>",
        &["cb", "abs"],
    );
    assert_eq!(found[1], [20.0, 0.0, 100.0, 100.0]);
}

#[test]
fn skips_the_contents_of_boxes_with_hidden_content_visibility() {
    // `content-visibility: hidden` lays out nothing inside the box, which has size containment:
    // an auto height is 0. `auto` shows the contents in print, as Chrome does, and an inline box
    // ignores it.
    let found = boxes(
        "<div class=hidden style='content-visibility: hidden'><div class=inner style='height: 50px'></div></div>
         <div class=sized style='content-visibility: hidden; height: 40px'>text</div>
         <div class=auto style='content-visibility: auto'><div style='height: 50px'></div></div>
         <div><span class=span style='content-visibility: hidden'>text</span></div>",
        &["hidden", "sized", "auto", "span"],
    );
    assert_eq!(found[0][3], 0.0);
    assert_eq!(found[1][3], 40.0);
    assert_eq!(found[2][3], 50.0);
    assert!(found[3][2] > 0.0);
    let (document, pages) = render_document(
        "<div style='content-visibility: hidden'><div class=inner style='height: 50px'></div></div>",
    );
    assert!(find(&pages[0], &document, "inner").is_none());
}

#[test]
fn trims_the_block_margins_of_first_and_last_children() {
    // `margin-trim: block` trims the top margin of the first child and the bottom margin of the
    // last child at the edges of the container (WPT `css-box/margin-trim/block-container-*`).
    let found = boxes(
        "<div class=box style='margin-trim: block; border: 1px solid'>
           <div class=first style='margin: 20px 0; height: 10px'></div>
           <div class=last style='margin: 20px 0; height: 10px'></div>
         </div>
         <div class=after style='height: 10px'></div>",
        &["box", "first", "last", "after"],
    );
    assert_eq!(found[1][1], 1.0);
    assert_eq!(found[2][1], 31.0);
    // 10 + 20 + 10 between the borders.
    assert_eq!(found[0][3], 42.0);
    assert_eq!(found[3][1], 42.0);
    // Only the start: the last child's margin stays inside the bordered box.
    let found = boxes(
        "<div class=box style='margin-trim: block-start; border: 1px solid'>
           <div style='margin: 20px 0; height: 10px'></div>
         </div>",
        &["box"],
    );
    assert_eq!(found[0][3], 32.0);
    // An empty child at the end has its margins trimmed too, and so do the margins inside a box
    // whose children are all empty, which keeps its own.
    let found = boxes(
        "<div style='display: flow-root'>
           <div class=trim style='margin-top: 30px; margin-trim: block-end'><div style='margin-top: 200px'></div></div>
           <div class=next style='height: 10px'></div>
         </div>",
        &["trim", "next"],
    );
    assert_eq!(found[1][1], 30.0);
}

#[test]
fn places_inline_level_absolute_boxes_among_blocks_as_in_a_line() {
    // An absolutely positioned box that was inline-level starts where a line would put it:
    // beside the floats and aligned by `text-align` (WPT `css-position/static-position/
    // inline-level-absolute-in-block-level-context-*`).
    let html = |align: &str, direction: &str| {
        format!(
            "<div style='position: relative; width: 100px; text-align: {align}; direction: {direction}'>
               <div style='height: 50px'></div>
               <div style='float: left; width: 50px; height: 50px'></div>
               <span class=abs style='position: absolute; width: 20px; height: 20px'></span>
               <div class=block style='position: absolute; width: 20px; height: 20px'></div>
             </div>"
        )
    };
    let x = |align: &str, direction: &str| {
        let found = boxes(&html(align, direction), &["abs", "block"]);
        (found[0][0], found[1][0])
    };
    assert_eq!(x("left", "ltr"), (50.0, 0.0));
    assert_eq!(x("center", "ltr"), (75.0, 0.0));
    assert_eq!(x("right", "ltr"), (100.0, 0.0));
    // Right to left, the box ends at the point.
    assert_eq!(x("right", "rtl"), (80.0, 80.0));
    assert_eq!(x("center", "rtl"), (55.0, 80.0));
    assert_eq!(x("left", "rtl"), (30.0, 80.0));
}

#[test]
fn keeps_overflow_and_writing_mode_on_a_contained_body() {
    // Containment on the root or the body keeps the body's `overflow` and `writing-mode` from
    // the viewport and the root (WPT `contain-body-overflow-*`, `contain-html-w-m-*`): the body
    // clips its content and the root's lines stay horizontal.
    let (document, pages) = render_document(
        "<!DOCTYPE html><html class=root style='contain: layout'><body class=body style='margin: 0;
           overflow: hidden; width: 50px; height: 50px; writing-mode: vertical-rl'><div
           style='writing-mode: horizontal-tb; width: 100px; height: 10px'></div></body></html>",
    );
    let style = |class: &str| {
        let (_, _, fragment) = find(&pages[0], &document, class).expect(class);
        let FragmentKind::Box { style, .. } = &fragment.kind else {
            panic!("{class} is no box");
        };
        Arc::clone(style)
    };
    assert_eq!(style("body").overflow_x, fagus_style::values::Overflow::Hidden);
    assert!(!style("root").writing_mode.is_vertical());
}

#[test]
fn keeps_sticky_boxes_inside_their_scroll_container() {
    // A sticky box in a scroll container moves by its insets to stay in the container's
    // content box, within its containing block (WPT `css-position/sticky/position-sticky-
    // margins-002`, `-003`); an `auto` margin does not hold it back.
    let found = boxes(
        "<div style='width: 100px; height: 100px; overflow: hidden'>
           <div style='width: 500px'><div class=right style='position: sticky; right: 0;
             margin-left: auto; width: 100px; height: 20px'></div></div>
         </div>
         <div style='width: 100px; height: 100px; overflow: hidden'>
           <div style='display: flex; flex-direction: column; height: 500px'>
             <div class=bottom style='position: sticky; bottom: 0; margin-top: auto; height: 30px'></div>
           </div>
         </div>
         <div class=outside style='position: sticky; top: 50px; height: 10px'></div>",
        &["right", "bottom", "outside"],
    );
    assert_eq!(found[0][0], 0.0);
    assert_eq!(found[1][1], 170.0);
    // Outside a scroll container, it stays where it is.
    assert_eq!(found[2][1], 200.0);
}

//! Positioned roots, relative offsets, blocks inside inline boxes and frames as Chrome lays
//! them out (from the Web Platform Tests in CSS2/abspos, CSS2/visuren, CSS2/box-display and
//! CSS2/normal-flow).

use fagus_dom::Document;
use fagus_style::values::Rgba;

use super::{find, render_document};
use crate::{Fragment, FragmentKind, Page};

fn page(html: &str) -> String {
    format!(
        "<!DOCTYPE html><style>@page {{ size: 600px 400px; margin: 0 }} body {{ margin: 0; font: 12px/16px Inter }}</style>{html}"
    )
}

/// The position and size of the box with class `class`, in CSS px.
fn bounds(pages: &[Page], document: &Document, class: &str) -> (f32, f32, f32, f32) {
    let (x, y, fragment) = find(&pages[0], document, class).expect(class);
    (x, y, fragment.rect.width.to_px(), fragment.rect.height.to_px())
}

fn layout_box(html: &str, class: &str) -> (f32, f32, f32, f32) {
    let (document, pages) = render_document(&page(html));
    bounds(&pages, &document, class)
}

#[test]
fn places_a_positioned_root_in_the_initial_containing_block() {
    // An absolutely positioned root is placed at its offsets in the page area and shrinks to fit
    // its content (abspos-containing-block-initial-004a).
    let (x, y, width, _) = layout_box(
        "<html class=r style='position: absolute; left: 50px; top: 30px; border: 4px solid'><span>ab</span>",
        "r",
    );
    assert_eq!((x, y), (50.0, 30.0));
    assert!(width < 50.0, "{width}");
    // From the right and the bottom.
    let placed = layout_box(
        "<html class=r style='position: absolute; right: 50px; bottom: 30px; width: 200px; height: 100px'>",
        "r",
    );
    assert_eq!(placed, (350.0, 270.0, 200.0, 100.0));
    // Both sides given, it is as wide as they leave it.
    let (_, _, width, _) = layout_box(
        "<html class=r style='position: absolute; left: 50px; right: 50px'><span>ab</span>",
        "r",
    );
    assert_eq!(width, 500.0);
    // A relatively positioned root moves by its offsets, as wide as the page area
    // (abspos-containing-block-initial-007).
    let moved = layout_box(
        "<html class=r style='position: relative; left: 50px; top: 30px; height: 100px'>",
        "r",
    );
    assert_eq!(moved, (50.0, 30.0, 600.0, 100.0));
}

#[test]
fn gives_the_body_background_to_the_canvas_whatever_the_root_display() {
    // A table root wraps its children in anonymous boxes, and an absolutely positioned body is
    // none of them; its background still goes to the canvas (abspos-containing-block-initial-004e).
    let (document, pages) = render_document(&page(
        "<html style='display: table'><body class=b style='position: absolute; width: 100px; height: 100px; background: yellow'>",
    ));
    let canvas = pages[0].canvas.as_ref().expect("a canvas");
    assert_eq!(canvas.background_color, Rgba::from_u8(255, 255, 0, 1.0));
    let (_, _, body) = find(&pages[0], &document, "b").expect("body");
    let FragmentKind::Box { style, .. } = &body.kind else {
        panic!("a box");
    };
    assert_eq!(style.background_color, Rgba::TRANSPARENT);
}

#[test]
fn resolves_percentages_of_relative_offsets_against_definite_heights_only() {
    // The containing block's height is definite: `top: 50%` is half of it.
    let fixed = layout_box(
        "<div style='height: 80px'><div class=a style='position: relative; top: 50%; height: 10px'></div></div>",
        "a",
    );
    assert_eq!(fixed.1, 40.0);
    // Its height depends on its content: the percentage is `auto`, as in Blink (top-115).
    let auto = layout_box(
        "<div><div class=a style='position: relative; top: 50%; height: 10px'></div><div style='height: 40px'></div></div>",
        "a",
    );
    assert_eq!(auto.1, 0.0);
    // Nor do inline boxes in such a block move by it.
    let inline = layout_box(
        "<div><span class=a style='position: relative; top: 50%'>a</span><div style='height: 40px'></div></div>",
        "a",
    );
    assert!(inline.1 < 1.0, "{inline:?}");
    // A stretched flex item has the height of its line.
    let item = layout_box(
        "<div style='display: flex; height: 80px'><div><div class=a style='position: relative; top: 50%; height: 10px'></div></div></div>",
        "a",
    );
    assert_eq!(item.1, 40.0);
}

/// The fragments of the element with class `class`, each part of it, with their positions.
fn parts<'p>(
    fragment: &'p Fragment,
    document: &Document,
    class: &str,
    origin: (f32, f32),
    out: &mut Vec<(f32, f32, &'p Fragment)>,
) {
    let x = origin.0 + fragment.rect.x.to_px();
    let y = origin.1 + fragment.rect.y.to_px();
    if let FragmentKind::Box { node: Some(node), .. } = &fragment.kind
        && document
            .element(*node)
            .is_some_and(|data| data.classes.iter().any(|name| &**name == class))
    {
        out.push((x, y, fragment));
    }
    for child in &fragment.children {
        parts(child, document, class, (x, y), out);
    }
}

#[test]
fn splits_inline_boxes_around_blocks_and_moves_the_blocks_with_them() {
    // The block inside a relatively positioned inline box moves with it, and the inline box goes
    // on after the block, without the border at its start (block-in-inline-relpos-001).
    let (document, pages) = render_document(&page(
        "<div style='width: 300px'>A <span class=s style='position: relative; left: 40px; top: 10px; border-left: 3px solid; background: yellow'>B
           <div class=c style='width: 100px'>C</div> B</span> A</div>",
    ));
    let block = bounds(&pages, &document, "c");
    assert_eq!((block.0, block.1), (40.0, 26.0));
    let mut found = Vec::new();
    parts(&pages[0].fragments[0], &document, "s", (0.0, 0.0), &mut found);
    assert_eq!(found.len(), 2, "the parts before and after the block");
    let border = |fragment: &Fragment| match &fragment.kind {
        FragmentKind::Box { style, .. } => (style.border_width.left, style.background_color),
        _ => panic!("a box"),
    };
    let yellow = Rgba::from_u8(255, 255, 0, 1.0);
    assert_eq!(border(found[0].2), (3.0, yellow));
    assert_eq!(border(found[1].2), (0.0, yellow));
    // The part after the block is on the line below it, moved as well.
    assert_eq!((found[1].0, found[1].1), (40.0, 42.0));
    // Between two blocks, an inline box with borders makes no line of its own.
    let (document, pages) = render_document(&page(
        "<span style='border: 2px solid'><div class=a style='height: 10px'></div> <div class=b style='height: 10px'></div></span>",
    ));
    assert_eq!(
        bounds(&pages, &document, "b").1 - bounds(&pages, &document, "a").1,
        10.0
    );
}

#[test]
fn lays_out_frames_as_empty_replaced_boxes() {
    // An `<iframe>`, whose document is not loaded, is 300 by 150 pixels in a border of 2 (its
    // natural size, without a ratio: a width leaves it as tall; block-replaced-height-004).
    assert_eq!(layout_box("<iframe class=f></iframe>", "f").2, 304.0);
    let (_, _, width, height) = layout_box("<iframe class=f width=100 style='display: block'></iframe>", "f");
    assert_eq!((width, height), (104.0, 154.0));
    let (_, _, width, height) = layout_box("<iframe class=f frameborder=0></iframe>", "f");
    assert_eq!((width, height), (300.0, 150.0));
}

#[test]
fn aligns_the_static_position_of_positioned_flex_children() {
    // A child without insets is placed as the container's only item would be (CSS Flexbox 1,
    // §4.1): by `justify-content` along the main axis and `align-self` across.
    let flex = |container: &str, child: &str| {
        let (x, y, ..) = layout_box(
            &format!(
                "<div style='display: flex; position: relative; width: 200px; height: 100px; {container}'>
                   <div class=a style='position: absolute; width: 40px; height: 20px; {child}'></div></div>"
            ),
            "a",
        );
        (x, y)
    };
    assert_eq!(
        flex("justify-content: center; align-items: center", ""),
        (80.0, 40.0)
    );
    assert_eq!(
        flex("justify-content: flex-end; align-items: flex-end", ""),
        (160.0, 80.0)
    );
    assert_eq!(flex("flex-direction: row-reverse", ""), (160.0, 0.0));
    assert_eq!(
        flex(
            "flex-direction: column; justify-content: center; align-items: flex-end",
            ""
        ),
        (160.0, 40.0)
    );
    assert_eq!(
        flex(
            "justify-content: space-around; align-items: center",
            "align-self: flex-start"
        ),
        (80.0, 0.0)
    );
    // An inset of its own wins in its axis.
    assert_eq!(
        flex("justify-content: center; align-items: center", "top: 5px"),
        (80.0, 5.0)
    );
}

#[test]
fn moves_block_level_static_positions_below_an_inline_box_start() {
    // A box that was block-level inside an inline box whose start has a border is after
    // content: its static position is at the start of the next line, below the line, even
    // where a negative margin takes the border's room back (static-inside-inline-002).
    let placed = layout_box(
        "<div><span style='line-height: 100px; border-left: 100px solid; margin-left: -100px'>
           <div class=a style='position: absolute; width: 10px; height: 10px'></div>X</span></div>",
        "a",
    );
    assert_eq!((placed.0, placed.1), (0.0, 100.0));
}

#[test]
fn places_absolutely_positioned_tables_by_the_width_they_take() {
    // The table lays itself out in its containing block, 100px wide, and is placed by that
    // width against its right inset (absolute-tables-011).
    let placed = layout_box(
        "<div style='position: relative; width: 100px; height: 100px; margin-left: 200px'>
           <table class=t style='position: absolute; right: -100px; border-spacing: 0'>
           <td style='padding: 0'>X X X X X X X X X X X X X X X X X X X X X X X</td></table></div>",
        "t",
    );
    assert_eq!((placed.0 + placed.2, placed.2), (400.0, 100.0));
}

#[test]
fn places_fixed_boxes_inside_absolute_ones_at_the_page_area_corner() {
    // A fixed box without insets inside an absolutely positioned one has no static position that
    // a page holds: Chrome puts it at the corner of the page area (static-fixed-inside-abspos).
    let placed = layout_box(
        "<div style='height: 50px'></div><div style='position: absolute; left: 20px; top: 30px; width: 50px; height: 50px'>
           <div class=f style='position: fixed; width: 30px; height: 30px'></div></div>",
        "f",
    );
    assert_eq!(placed, (0.0, 0.0, 30.0, 30.0));
}

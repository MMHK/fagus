//! Counters, clearance, floats, table column widths and `<object>` images as Chrome lays them
//! out (from the Web Platform Tests in css-lists, CSS2/floats-clear, CSS2/floats, css-tables,
//! CSS2/tables and CSS2/normal-flow).

use std::sync::Arc;

use super::{find, render_document, render_document_with};
use crate::{Fragment, FragmentKind, Image, ImageFormat, Images};

fn page(html: &str) -> String {
    format!(
        "<!DOCTYPE html><style>@page {{ margin: 0 }} body {{ margin: 0; font: 12px/16px Inter }}</style>{html}"
    )
}

/// The positions and sizes of the boxes with the given classes, in CSS px.
fn boxes(html: &str, classes: &[&str]) -> Vec<(f32, f32, f32, f32)> {
    let (document, pages) = render_document(&page(html));
    classes
        .iter()
        .map(|class| {
            let (x, y, fragment) = find(&pages[0], &document, class).expect(class);
            (x, y, fragment.rect.width.to_px(), fragment.rect.height.to_px())
        })
        .collect()
}

/// The text runs of a fragment tree, in order.
fn texts(fragment: &Fragment, out: &mut Vec<String>) {
    if let FragmentKind::Text(run) = &fragment.kind {
        out.push(run.text.clone());
    }
    for child in &fragment.children {
        texts(child, out);
    }
}

fn all_texts(html: &str) -> Vec<String> {
    let (_, pages) = render_document(&page(html));
    let mut out = Vec::new();
    for fragment in &pages[0].fragments {
        texts(fragment, &mut out);
    }
    out
}

#[test]
fn counters_start_where_they_are_first_incremented() {
    // A counter no element resets starts at the element that increments it: each heading's
    // `::before` has its own, and siblings share the first one's, which ends with their parent.
    let found = all_texts(
        "<style>h2 { font-size: 12px; margin: 0 } h2::before { counter-increment: h; content: 'h' counter(h) }
           p { margin: 0; counter-increment: q } p::before { content: 'q' counters(q, '.') }
           i::before { content: 'r' counters(r, '.') }</style>
         <h2>A</h2><h2>B</h2>
         <div><p>1</p><p>2</p></div><div><p>3</p></div>
         <i>x</i>",
    );
    for expected in ["h1", "q1", "q2", "r0"] {
        assert!(
            found.iter().any(|text| text == expected),
            "{expected} in {found:?}"
        );
    }
    assert!(
        !found.iter().any(|text| text == "h2" || text == "q3"),
        "{found:?}"
    );
}

#[test]
fn list_markers_count_apart_from_the_list_item_counter() {
    // Chrome numbers markers from `start`, the item count of a reversed list and `value`, while
    // `counter(list-item)` counts on from the list's reset (WPT counter-list-item).
    let found = all_texts(
        "<style>li::after { content: '[' counter(list-item) ']' }</style>
         <ol reversed><li>a<li>b</ol>
         <ol><li>c<li value=30>d<li>e</ol>",
    );
    for expected in ["2. ", "[0]", "1. ", "[-1]", "30. ", "[2]", "31. ", "[3]"] {
        assert!(
            found.iter().any(|text| text == expected),
            "{expected:?} in {found:?}"
        );
    }
}

#[test]
fn replaced_elements_in_cells_of_a_fixed_height_take_percentages_of_it() {
    // WPT percent-height-replaced-in-percent-cell-002: the canvas is capped by the cell's
    // height, not stretched to the row it makes.
    let found = boxes(
        "<div class=cell style='display: table-cell; height: 100px; line-height: 0'>
           <canvas class=canvas width=200 height=200 style='max-height: 100%'></canvas></div>",
        &["canvas", "cell"],
    );
    assert_eq!((found[0].2, found[0].3), (100.0, 100.0), "{found:?}");
    assert_eq!(found[1].3, 100.0, "{found:?}");
}

#[test]
fn an_empty_cleared_first_child_does_not_join_its_parents_margin() {
    // The cleared box's top margin stays above it, and what follows counts its margins from
    // there, as for any other empty box with clearance.
    let found = boxes(
        "<div style='display: flow-root'><div>
           <div style='float: left; width: 50px; height: 100px'></div>
           <div style='clear: left; margin: 20px 0 0'></div>
           <div class=next style='margin-top: 10px; height: 5px'></div>
         </div></div>",
        &["next"],
    );
    assert_eq!(found[0].1, 100.0, "{found:?}");
}

#[test]
fn new_formatting_contexts_fit_beside_floats_from_their_start_edge() {
    // WPT floats-wrap-bfc-with-margin-005: in a right-to-left block, the box starts at the right
    // edge, and its margin there leaves it no room before the float on the left.
    let found = boxes(
        "<div style='display: flow-root; width: 100px; direction: rtl'>
           <div style='float: left; width: 50px; height: 40px'></div>
           <div class=rtl style='display: flow-root; height: 60px; margin-right: 51px'></div></div>
         <div style='display: flow-root; width: 100px'>
           <div style='float: left; width: 50px; height: 40px'></div>
           <div class=ltr style='display: flow-root; height: 60px; margin-right: 51px'></div></div>",
        &["rtl", "ltr"],
    );
    assert_eq!(found[0].1, 40.0, "{found:?}");
    assert_eq!(found[1].1, 100.0, "beside the float, without width: {found:?}");
}

#[test]
fn spanning_cells_widen_auto_columns_first() {
    // WPT colspan-004.
    let found = boxes(
        "<table style='width: 110px' cellspacing=0 cellpadding=0>
           <tr><td colspan=2 style='width: 100px'></td><td colspan=2></td>
           <tr><td style='width: 5px'></td><td colspan=2 class=green><div style='height: 10px'></div></td>
             <td style='width: 5px'></td></table>",
        &["green"],
    );
    assert_eq!((found[0].0, found[0].2), (5.0, 100.0), "{found:?}");
}

#[test]
fn percentage_cells_of_a_fixed_layout_add_their_padding() {
    // WPT fixed-table-layout-025.
    let found = boxes(
        "<table style='table-layout: fixed; width: 100px; border-spacing: 0'>
           <tr><td style='padding: 5px 25px'></td><td class=middle style='width: 50%; padding: 5px 25px'></td>
             <td style='padding: 5px 25px'></td></table>",
        &["middle"],
    );
    assert_eq!(found[0].2, 100.0, "{found:?}");
}

#[test]
fn columns_take_the_minimum_widths_of_their_col_elements() {
    // WPT col-definite-min-size-001: the `min-width` of a `<col>` widens its column, and
    // columns of `<col>` elements without a width after the cells' are dropped.
    let found = boxes(
        "<table class=table style='border-spacing: 10px'>
           <col style='min-width: 100px'><col style='min-width: 100px'><col><col style='min-width: 100px'>
           <tr><td class=cell style='padding: 0'>1</td><td style='padding: 0'>2</td></table>",
        &["cell", "table"],
    );
    assert_eq!(found[0].2, 100.0, "{found:?}");
    assert_eq!(found[1].2, 230.0, "{found:?}");
}

#[test]
fn tables_with_collapsed_borders_have_no_padding_in_floats() {
    // WPT collapsing-border-model-013.
    let found = boxes(
        "<div class=float style='float: left'>
           <table style='border-collapse: collapse; width: 100px; height: 100px; padding: 100px'></table></div>",
        &["float"],
    );
    assert_eq!((found[0].2, found[0].3), (100.0, 100.0), "{found:?}");
}

#[test]
#[allow(clippy::items_after_statements, reason = "a helper next to its use")]
fn rows_and_row_groups_draw_their_outlines() {
    let (_, pages) = render_document(&page(
        "<table style='border-spacing: 10px'><tbody style='outline: 4px solid green'>
           <tr style='outline: 2px solid blue'><td style='padding: 0'><div style='width: 50px; height: 20px'></div></td>
         </table>",
    ));
    fn outlines(fragment: &Fragment, out: &mut Vec<(f32, f32)>) {
        if let FragmentKind::Box {
            style, node: None, ..
        } = &fragment.kind
            && style.outline_width > 0.0
        {
            out.push((style.outline_width, fragment.rect.width.to_px()));
        }
        for child in &fragment.children {
            outlines(child, out);
        }
    }
    let mut found = Vec::new();
    outlines(&pages[0].fragments[0], &mut found);
    assert_eq!(found, [(4.0, 50.0), (2.0, 50.0)]);
}

#[test]
#[allow(clippy::items_after_statements, reason = "a helper next to its use")]
fn hidden_tables_draw_no_collapsed_borders() {
    // WPT visibility-hidden-collapsed-borders.
    let (_, pages) = render_document(&page(
        "<table style='border: solid red 20px; border-collapse: collapse; width: 100px; height: 100px;
           visibility: hidden'><td></td></table>",
    ));
    fn borders(fragment: &Fragment) -> usize {
        let own = matches!(&fragment.kind, FragmentKind::Box { style, node: None, .. }
            if style.border_width.top > 0.0 || style.border_width.left > 0.0);
        usize::from(own) + fragment.children.iter().map(borders).sum::<usize>()
    }
    assert_eq!(borders(&pages[0].fragments[0]), 0);
}

#[test]
fn objects_show_their_images() {
    let (document, pages) = render_document_with(
        &page(
            "<object class=image data=square.png width=40></object>
             <object class=page data=square.png type=text/html><span class=fallback>x</span></object>",
        ),
        |_| {
            let mut images = Images::default();
            images.insert(
                "square.png",
                Arc::new(Image {
                    data: Arc::new(Vec::new()),
                    format: ImageFormat::Png,
                    width: 20.0,
                    height: 20.0,
                    natural: (true, true),
                    ratio: Some(1.0),
                    inline: false,
                    orientation: 1,
                    filters: Arc::from([]),
                    pad: 0,
                }),
            );
            images
        },
    );
    let (_, _, image) = find(&pages[0], &document, "image").expect("image");
    assert_eq!(
        (image.rect.width.to_px(), image.rect.height.to_px()),
        (40.0, 40.0)
    );
    assert!(matches!(image.children[0].kind, FragmentKind::Image(_)));
    // An object of another type shows its content.
    assert!(find(&pages[0], &document, "fallback").is_some());
}

#[test]
fn boxes_without_room_beside_floats_leave_their_margin_out() {
    // WPT new-fc-separates-from-float-3: the box does not fit beside the float its margin would
    // take along: the margin does not count, and the box goes below the float at the top.
    let found = boxes(
        "<div style='display: flow-root; width: 100px'><div>
           <div class=float style='float: right; width: 50px; height: 30px'></div>
           <div><div class=box style='overflow: hidden; width: 100px; height: 70px; margin-top: 100px'></div></div>
         </div></div>",
        &["float", "box"],
    );
    assert_eq!(found[0].1, 0.0, "{found:?}");
    assert_eq!(found[1].1, 30.0, "{found:?}");
}

#[test]
fn tables_beside_floats_need_room_all_down() {
    // WPT table-sizing-with-adjacent-floats: beside the first float the table would reach the
    // wider one below it, so it takes the narrower room beside both.
    let found = boxes(
        "<div style='width: 250px'>
           <div style='float: right; width: 50px; height: 50px'></div>
           <div style='float: right; clear: right; width: 200px; height: 50px'></div>
           <table class=table style='border-spacing: 0'><td style='width: 100px; height: 100px; padding: 0'></td></table>
         </div>",
        &["table"],
    );
    assert_eq!(
        (found[0].0, found[0].1, found[0].2),
        (0.0, 0.0, 50.0),
        "{found:?}"
    );
}

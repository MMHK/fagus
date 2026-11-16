//! Vertical writing modes and orthogonal flows as Chrome 157 prints them (from the Web Platform
//! Tests in css-writing-modes and css-break): the principal writing mode, relative and absolute
//! offsets in vertical and right-to-left containing blocks, percentages of orthogonal flows,
//! vertical tables, broken boxes and inline blocks in vertical lines.

use fagus_dom::Document;

use super::{find, render_document};
use crate::{Fragment, FragmentKind, Page, VisibleSides};

fn page(html: &str) -> String {
    format!(
        "<!DOCTYPE html><style>@page {{ size: 600px 400px; margin: 0 }} body {{ margin: 0; font: 12px/16px Inter }}</style>{html}"
    )
}

/// The position and size of the box with class `class` on page `index`, in CSS px.
fn bounds_on(pages: &[Page], index: usize, document: &Document, class: &str) -> (f32, f32, f32, f32) {
    let (x, y, fragment) = find(&pages[index], document, class).expect(class);
    (x, y, fragment.rect.width.to_px(), fragment.rect.height.to_px())
}

fn layout_box(html: &str, class: &str) -> (f32, f32, f32, f32) {
    let (document, pages) = render_document(&page(html));
    bounds_on(&pages, 0, &document, class)
}

#[test]
fn lays_out_the_root_in_the_writing_mode_and_direction_of_the_body() {
    // A vertical root with a horizontal body lays out horizontally, as Chrome gives the body's
    // writing mode to the viewport (wm-propagation-body-040).
    let (x, y, _, _) = layout_box(
        "<html style='writing-mode: vertical-rl'><body style='writing-mode: horizontal-tb'>\
         <div class=a style='width: 50px; height: 30px'></div>",
        "a",
    );
    assert_eq!((x, y), (0.0, 0.0));
    // A right-to-left body of a fixed width sits on the right of the page.
    let body = layout_box("<body class=b dir=rtl style='width: 300px; height: 20px'>", "b");
    assert_eq!(body, (300.0, 0.0, 300.0, 20.0));
}

#[test]
fn lets_the_start_inset_of_the_containing_block_win() {
    // Both insets of an absolutely positioned box in a right-to-left containing block: the
    // left one gives way, also for a left-to-right box.
    for own in ["", " dir=ltr"] {
        let html = format!(
            "<div dir=rtl style='position: relative; width: 200px; height: 100px'>\
             <div class=a{own} style='position: absolute; left: 10px; right: 20px; width: 50px; height: 20px'></div></div>"
        );
        assert_eq!(layout_box(&html, "a").0, 130.0, "{own}");
    }
    // In a vertical right-to-left one, the top inset gives way.
    let abs = layout_box(
        "<div dir=rtl style='position: relative; writing-mode: vertical-lr; width: 100px; height: 200px'>\
         <div class=a style='position: absolute; top: 10px; bottom: 20px; height: 50px; width: 20px'></div></div>",
        "a",
    );
    assert_eq!((abs.1, abs.3), (130.0, 50.0));
    // Relative offsets: in `vertical-rl` the right one (the block-start side) wins, in a
    // right-to-left `vertical-lr` box the bottom one.
    let rl = layout_box(
        "<div style='writing-mode: vertical-rl; width: 100px; height: 200px'>\
         <div class=r style='position: relative; left: 10px; right: 30px; width: 20px; height: 50px'></div></div>",
        "r",
    );
    assert_eq!(rl.0, 50.0);
    let lr = layout_box(
        "<div dir=rtl style='writing-mode: vertical-lr; width: 100px; height: 200px'>\
         <div class=r style='position: relative; top: 10px; bottom: 30px; width: 20px; height: 50px'></div></div>",
        "r",
    );
    assert_eq!(lr.1, 120.0);
}

#[test]
fn offsets_horizontal_boxes_in_a_vertical_document_across_the_page() {
    // `left` moves a box in a horizontal box of a vertical document to the right, not down.
    let (x, y, _, _) = layout_box(
        "<html style='writing-mode: vertical-rl'>\
         <div style='writing-mode: horizontal-tb; width: 200px; height: 100px'>\
         <div class=r style='position: relative; left: 10px; width: 50px; height: 20px'></div></div>",
        "r",
    );
    assert_eq!((x, y), (410.0, 0.0));
}

#[test]
fn places_absolute_boxes_in_the_whole_vertical_containing_block() {
    // The containing block is the flow root's box, which its empty content does not fill.
    let abs = layout_box(
        "<div style='position: relative; writing-mode: vertical-rl; width: 100px; height: 200px'>\
         <div class=a style='position: absolute; top: 10px; left: 0; width: 20px; height: 50px'></div></div>",
        "a",
    );
    assert_eq!(abs, (0.0, 10.0, 20.0, 50.0));
}

#[test]
fn resolves_percentages_of_orthogonal_flows_against_the_room_they_have() {
    // A percentage height of a vertical box in a block of an automatic height refers to the
    // page area (sizing-orthog-prct-vrl-in-htb-001).
    let (_, _, _, height) = layout_box(
        "<div><div class=v style='writing-mode: vertical-rl; height: 50%'>a b c</div></div>",
        "v",
    );
    assert_eq!(height, 200.0);
    // Percentage margins refer to the inline size of the containing block, and leave the lines
    // that much less room (sizing-orthogonal-percentage-margin-001).
    let (x, y, _, height) = layout_box(
        "<div style='width: 500px; height: 200px'>\
         <div class=v style='writing-mode: vertical-lr; margin: 10%; font-size: 25px'>\
         aa bb cc dd ee ff gg hh ii jj kk ll mm nn</div></div>",
        "v",
    );
    assert_eq!((x, y, height), (50.0, 50.0, 100.0));
}

#[test]
fn lays_out_vertical_tables_along_their_lines() {
    // `border-spacing` gives the room along the lines first (border-spacing-vrl-002).
    let (document, pages) = render_document(&page(
        "<table style='writing-mode: vertical-rl; border-spacing: 10px 0'>\
         <tr><td class=a style='padding: 0; width: 20px; height: 30px'></td>\
         <td class=b style='padding: 0; width: 20px; height: 30px'></td></tr></table>",
    ));
    let a = bounds_on(&pages, 0, &document, "a");
    let b = bounds_on(&pages, 0, &document, "b");
    assert_eq!(b.1 - (a.1 + a.3), 10.0);
    assert_eq!(a.0, b.0);
    // A table of a set height spreads its columns over it.
    let (document, pages) = render_document(&page(
        "<table class=t style='writing-mode: vertical-rl; height: 100px; border-spacing: 0'>\
         <tr><td class=a style='padding: 0'></td><td class=b style='padding: 0'></td></tr></table>",
    ));
    let a = bounds_on(&pages, 0, &document, "a");
    let b = bounds_on(&pages, 0, &document, "b");
    assert_eq!(a.3 + b.3, 100.0);
    // Collapsed borders: the table's and its cells' meet (contiguous-floated-table-vrl-002).
    let (_, _, width, _) = layout_box(
        "<table class=t style='writing-mode: vertical-rl; height: 100px; border-collapse: collapse;\
         border-left: 10px solid red; border-right: 25px solid blue'>\
         <tr><td style='padding: 0; border-left: 5px solid; border-right: 25px solid green'></td></tr></table>",
        "t",
    );
    assert_eq!(width, 35.0);
}

/// The sides drawn of the fragment of the element with class `class` on page `index`.
fn sides_on(pages: &[Page], index: usize, document: &Document, class: &str) -> VisibleSides {
    let (_, _, fragment) = find(&pages[index], document, class).expect(class);
    match &fragment.kind {
        FragmentKind::Box { sides, .. } => *sides,
        _ => panic!("{class} is no box"),
    }
}

#[test]
fn leaves_out_the_borders_of_broken_vertical_boxes_on_their_block_sides() {
    // A box of a `vertical-rl` document broken across pages: its first fragment has no left
    // border (its block-end side), its last no right one (block-001-wm-vrl-print).
    let (document, pages) = render_document(&page(
        "<html style='writing-mode: vertical-rl'>\
         <div class=b style='width: 1000px; height: 100px; border: 5px solid'></div>",
    ));
    assert!(pages.len() >= 2);
    let first = sides_on(&pages, 0, &document, "b");
    assert!(
        first.right && first.top && first.bottom && !first.left,
        "{first:?}"
    );
    let last = sides_on(&pages, pages.len() - 1, &document, "b");
    assert!(last.left && last.top && last.bottom && !last.right, "{last:?}");
}

/// The text runs of a fragment and its descendants with their positions on the page.
fn runs<'f>(fragment: &'f Fragment, origin: (f32, f32), out: &mut Vec<(f32, f32, &'f Fragment)>) {
    let x = origin.0 + fragment.rect.x.to_px();
    let y = origin.1 + fragment.rect.y.to_px();
    if matches!(fragment.kind, FragmentKind::Text(_)) {
        out.push((x, y, fragment));
    }
    for child in &fragment.children {
        runs(child, (x, y), out);
    }
}

#[test]
fn aligns_inline_blocks_in_vertical_lr_lines_on_their_last_line() {
    // In `vertical-lr` lines, an inline block sits on the baseline of its last line, whose
    // over side is on the right (inline-block-alignment-003).
    let (document, pages) = render_document(&page(
        "<div style='writing-mode: vertical-lr; font: 20px/1 Inter'>A<div class=i style='display: inline-block;\
         padding-left: 10px; font-size: 40px'><span style='display: block'>F</span>\
         <span style='display: block'>L</span></div></div>",
    ));
    let mut found = Vec::new();
    for fragment in &pages[0].fragments {
        runs(fragment, (0.0, 0.0), &mut found);
    }
    let center = |text: &str| {
        let (x, _, fragment) = found
            .iter()
            .find(|(_, _, fragment)| matches!(&fragment.kind, FragmentKind::Text(run) if run.text == text))
            .expect(text);
        x + fragment.rect.width.to_px() / 2.0
    };
    assert!(
        (center("A") - center("L")).abs() < 1.0,
        "{} {}",
        center("A"),
        center("L")
    );
    let (x, _, width, _) = bounds_on(&pages, 0, &document, "i");
    assert!(center("L") > x + width / 2.0);
}

#[test]
fn puts_boxes_of_other_writing_modes_beside_floats() {
    // A flow root avoids floats as other new formatting contexts do, in the first place where
    // it fits (srl-alongside-vrl-floats).
    let (document, pages) = render_document(&page(
        "<html style='writing-mode: vertical-rl'><body style='height: 400px'>\
         <div class=f style='float: left; width: 50px; height: 50%'></div>\
         <div class=g style='float: left; clear: left; width: 50px; height: 25%'></div>\
         <div class=o style='writing-mode: sideways-rl; width: 50px; height: 75%'></div>",
    ));
    let g = bounds_on(&pages, 0, &document, "g");
    let o = bounds_on(&pages, 0, &document, "o");
    assert_eq!((o.0, o.1, o.3), (g.0, g.1 + g.3, 300.0));
    // A percentage width beside floats still refers to the containing block.
    let (x, _, width, _) = layout_box(
        "<div style='width: 400px'><div style='float: left; width: 25%; height: 50px'></div>\
         <div class=b style='overflow: hidden; width: 75%; height: 30px'></div></div>",
        "b",
    );
    assert_eq!((x, width), (100.0, 300.0));
}

#[test]
fn keeps_a_float_beside_the_line_whose_spaces_hang_before_it() {
    // A float after a word and a space fits beside the word, the space hanging: it stays on
    // that line, also where the line ends right before it (float-vrl-012, in both modes).
    for mode in ["horizontal-tb", "vertical-rl"] {
        let (x, y, _, _) = layout_box(
            &format!(
                "<div style='writing-mode: {mode}; width: 40px; height: 40px; font: 20px/1 Inter'>\
                 W <span class=f style='float: left; width: 20px; height: 20px'></span> W</div>"
            ),
            "f",
        );
        let expected = if mode == "horizontal-tb" {
            (0.0, 0.0)
        } else {
            (20.0, 0.0)
        };
        assert_eq!((x, y), expected, "{mode}");
    }
}

#[test]
fn moves_a_box_of_other_lines_that_does_not_fit_below_a_margin_to_the_next_page() {
    // An orthogonal flow root is monolithic: below the top margin of the page's first box it
    // goes to the next page as a whole (sizing-orthog-vrl-in-htb-004).
    let (document, pages) = render_document(&page(
        "<body style='margin-top: 100px'><div class=v style='writing-mode: vertical-rl; height: 350px'>a</div>",
    ));
    assert_eq!(pages.len(), 2);
    assert!(find(&pages[0], &document, "v").is_none());
    assert_eq!(bounds_on(&pages, 1, &document, "v").1, 0.0);
}

#[test]
fn sizes_vertical_text_areas_and_list_boxes_across_their_lines() {
    // Rows stack across, columns run down (textarea-appearance-native-vrl).
    let (_, _, width, height) = layout_box(
        "<textarea class=t style='writing-mode: vertical-rl; font: 10px/10px Inter; border: 0; padding: 0;\
         overflow: hidden' rows=3 cols=10></textarea>",
        "t",
    );
    assert_eq!(width, 30.0);
    assert!(height > width, "{height}");
    let (_, _, width, height) = layout_box(
        "<select class=s multiple style='writing-mode: vertical-lr; border: 0; padding: 0'>\
         <option>One<option>Two<option>Three<option>Four<option>Five</select>",
        "s",
    );
    assert!(width > 4.0 * 10.0 && width < 4.0 * 20.0, "{width}");
    assert!(height > 15.0 && height < width, "{height}");
}

#[test]
fn ends_overflowing_vertical_lines_of_a_clipping_box_with_an_ellipsis() {
    // A vertical inline block that clips its lines in a horizontal one (text-overflow-ellipsis-
    // vertical-001).
    let (document, pages) = render_document(&page(
        "<div class=v style='display: inline-block; writing-mode: vertical-lr; height: 60px; font: 20px/1 Inter;\
         white-space: pre; overflow: hidden; text-overflow: ellipsis'>abcdefghij</div>",
    ));
    let (_, _, fragment) = find(&pages[0], &document, "v").expect("v");
    let mut found = Vec::new();
    runs(fragment, (0.0, 0.0), &mut found);
    assert!(
        found
            .iter()
            .any(|(_, _, run)| matches!(&run.kind, FragmentKind::Text(run) if run.text.contains('\u{2026}'))),
        "no ellipsis"
    );
}

#[test]
fn fills_the_columns_of_a_vertical_multicol_box_of_a_set_width_in_turn() {
    // `column-fill: auto` in a vertical box in a horizontal document: the columns are as wide as
    // the box, the last takes what is left (borders-006).
    let (document, pages) = render_document(&page(
        "<div style='columns: 3; column-gap: 10px; column-fill: auto; writing-mode: vertical-lr;\
         height: 320px; width: 100px'><div class=b style='width: 250px'></div></div>",
    ));
    let widths: Vec<f32> = pages[0]
        .fragments
        .iter()
        .flat_map(|fragment| {
            let mut found = Vec::new();
            collect_widths(fragment, &document, "b", &mut found);
            found
        })
        .collect();
    assert_eq!(widths, [100.0, 100.0, 50.0]);
}

/// The widths of the fragments of the element with class `class`.
fn collect_widths(fragment: &Fragment, document: &Document, class: &str, out: &mut Vec<f32>) {
    if let FragmentKind::Box { node: Some(node), .. } = &fragment.kind
        && document
            .element(*node)
            .is_some_and(|data| data.classes.iter().any(|name| &**name == class))
    {
        out.push(fragment.rect.width.to_px());
    }
    for child in &fragment.children {
        collect_widths(child, document, class, out);
    }
}

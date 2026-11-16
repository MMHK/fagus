//! Clearance, margins and floats in block layout, and table sizes, as Chrome lays them out
//! (from the Web Platform Tests in css/CSS2/floats-clear, floats, normal-flow and css-tables).

use super::{find, render_document};

/// The positions and sizes of the boxes with the given classes, in CSS px.
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

#[test]
fn margins_past_the_floats_leave_no_clearance() {
    // The cleared box's margin takes it past the float: no clearance, and the margin collapses
    // with its parent's (WPT no-clearance-due-to-large-margin).
    let found = boxes(
        "<div style='float: left; width: 100px; height: 100px'></div>
         <div style='padding-top: 1px'>
           <div class=parent><div class=cleared style='clear: left; margin-top: 150px'>x</div></div>
         </div>",
        &["parent", "cleared"],
    );
    assert_eq!(found[0].1, 151.0, "{found:?}");
    assert_eq!(found[1].1, 151.0, "{found:?}");
    assert_eq!(found[0].3, 16.0, "{found:?}");
}

#[test]
fn margins_of_an_empty_cleared_box_stay_in_its_parent() {
    // WPT margin-collapse-clear-012: 100px of float, then the 140px margin below the cleared box
    // less its own 40px top margin.
    let found = boxes(
        "<div class=parent style='border-top: 1px solid; width: 200px'>
           <div style='float: left; width: 100px; height: 100px'></div>
           <div style='clear: left; margin: 40px 0 80px'></div>
           <div style='margin-bottom: 140px'></div>
         </div>
         <div class=next style='height: 10px'></div>",
        &["parent", "next"],
    );
    assert_eq!(found[0].3, 201.0, "{found:?}");
    assert_eq!(found[1].1, 201.0, "{found:?}");
}

#[test]
fn margins_collapse_through_a_box_of_zero_height() {
    // WPT margin-collapse-through-zero-height-block.
    let found = boxes(
        "<div style='overflow: hidden'>
           <div style='margin-bottom: 200px'></div>
           <div style='height: 0'></div>
           <div class=after style='height: 20px; margin-top: 100px'></div>
         </div>",
        &["after"],
    );
    assert_eq!(found[0].1, 200.0, "{found:?}");
}

#[test]
fn a_maximum_height_keeps_the_last_margin_inside() {
    // WPT max-height-separates-margin.
    let found = boxes(
        "<div style='max-height: 50px'><div style='height: 51px; margin-bottom: 10px'></div></div>
         <div class=next style='height: 10px'></div>",
        &["next"],
    );
    assert_eq!(found[0].1, 50.0, "{found:?}");
}

#[test]
fn new_formatting_contexts_go_beside_floats_as_in_chrome() {
    let found = boxes(
        "<div style='width: 300px; overflow: hidden'>
           <div style='float: left; width: 100px; height: 40px'></div>
           <div class=wide style='overflow: hidden'><span style='display: inline-block; width: 250px'>x</span></div>
         </div>
         <div style='width: 300px; overflow: hidden'>
           <div style='float: left; width: 100px; height: 40px'></div>
           <div class=margin style='overflow: hidden; margin-left: 30px'>x</div>
         </div>
         <div style='width: 300px; overflow: hidden'>
           <div style='float: left; width: 100px; height: 40px'></div>
           <div class=fixed style='overflow: hidden; width: 200px; margin-right: 10px'>x</div>
         </div>
         <div style='width: 300px; overflow: hidden'>
           <div style='float: left; width: 100px; height: 40px'></div>
           <div class=below style='overflow: hidden; width: 201px'>x</div>
         </div>",
        &["wide", "margin", "fixed", "below"],
    );
    // An auto width fills the room beside the float, however wide its content.
    assert_eq!((found[0].0, found[0].2), (100.0, 200.0), "{found:?}");
    // A margin overlaps the float.
    assert_eq!((found[1].0, found[1].2), (100.0, 200.0), "{found:?}");
    // The end margin needs no room beside the float either.
    assert_eq!(found[2].0, 100.0, "{found:?}");
    // Only a border box too wide for the room goes below the float.
    assert_eq!(found[3].0, 0.0, "{found:?}");
    assert_eq!(found[3].1 - found[2].1, 80.0, "{found:?}");
}

#[test]
fn a_float_wider_than_its_containing_block_stays_beside_floats_outside_it() {
    // WPT floats-rule3-outside-left-002.
    let found = boxes(
        "<div style='float: left; width: 500px; height: 500px'>
           <div style='float: right; width: 50px; height: 300px'></div>
           <div style='margin-right: 100px'><div class=wide style='float: left; width: 475px; height: 10px'></div></div>
         </div>",
        &["wide"],
    );
    assert_eq!((found[0].0, found[0].1), (0.0, 0.0), "{found:?}");
}

#[test]
fn tables_take_minimum_and_maximum_heights() {
    let found = boxes(
        "<table class=min style='min-height: 100px; border-spacing: 0'><tr><td style='padding: 0'>x</td></tr></table>
         <table class=max style='height: 300px; max-height: 50px; border-spacing: 0'><tr><td style='padding: 0'>x</td></tr></table>",
        &["min", "max"],
    );
    assert_eq!(found[0].3, 100.0, "{found:?}");
    assert_eq!(found[1].3, 50.0, "{found:?}");
}

#[test]
fn tables_without_rows_take_their_height() {
    let found = boxes(
        "<table class=collapsed style='border-collapse: collapse; box-sizing: content-box; width: 100px; height: 100px; padding: 100px'></table>
         <table class=separate style='width: 100px; height: 50px; border: 3px solid; padding: 5px; border-spacing: 10px'></table>
         <table class=auto style='width: 100px; border: 3px solid; padding: 5px; border-spacing: 10px'><tr></tr></table>
         <div class=header style='display: table; width: 100px; height: 100px'>
           <div style='display: table-header-group; height: 20px'><div></div></div>
         </div>",
        &["collapsed", "separate", "auto", "header"],
    );
    // No padding in the collapsing border model, and no border spacing without columns.
    assert_eq!((found[0].2, found[0].3), (100.0, 100.0), "{found:?}");
    assert_eq!(found[1].3, 50.0, "{found:?}");
    assert_eq!(found[2].3, 16.0, "{found:?}");
    // Without body rows, the header rows take the table's height.
    assert_eq!(found[3].3, 100.0, "{found:?}");
}

#[test]
fn captions_keep_the_margins_of_their_content() {
    let found = boxes(
        "<table><caption class=caption><div style='margin-top: 50px'><div style='margin-top: 50px'></div></div></caption></table>",
        &["caption"],
    );
    assert_eq!(found[0].3, 50.0, "{found:?}");
}

#[test]
fn collapsed_columns_and_rows_take_no_room() {
    let found = boxes(
        "<table class=columns style='border-spacing: 10px'>
           <col><col style='visibility: collapse'><col>
           <tr><td class=first style='width: 50px; padding: 0'></td><td style='width: 50px; padding: 0'></td><td class=third style='width: 50px; padding: 0'></td></tr>
         </table>
         <table class=rows style='border-spacing: 10px'>
           <tr><td style='height: 20px; padding: 0'></td></tr>
           <tr style='visibility: collapse'><td style='height: 20px; padding: 0'></td></tr>
           <tr><td style='height: 20px; padding: 0'></td></tr>
         </table>
         <table class=text style='border-spacing: 10px'>
           <tr><td style='height: 20px; padding: 0'></td></tr>
           <tr style='visibility: collapse'><td style='height: 20px; padding: 0'>x</td></tr>
           <tr><td style='height: 20px; padding: 0'></td></tr>
         </table>",
        &["columns", "first", "third", "rows", "text"],
    );
    assert_eq!(found[0].2, 130.0, "{found:?}");
    assert_eq!(found[2].0 - found[1].0, 60.0, "{found:?}");
    assert_eq!(found[3].3, 70.0, "{found:?}");
    // In print, Chrome keeps the height of the content of a collapsed row (without spacing).
    assert_eq!(found[4].3, 86.0, "{found:?}");
}

#[test]
fn body_background_stays_on_a_contained_body() {
    let (_, pages) = render_document(
        "<!DOCTYPE html><style>body { contain: paint; background: green }</style><body>x</body>",
    );
    assert!(pages[0].canvas.is_none());
    let (_, pages) =
        render_document("<!DOCTYPE html><style>body { background: green }</style><body>x</body>");
    assert!(pages[0].canvas.is_some());
}

#[test]
fn rows_cast_their_shadows_before_their_cells() {
    use crate::{Fragment, FragmentKind};
    // Each row's shadow is a box of the row (between the outer spacing) before its cells, its
    // outer shadows alone.
    fn walk(fragment: &Fragment, out: &mut Vec<(String, f32, f32, f32)>) {
        if let FragmentKind::Box { style, node, .. } = &fragment.kind {
            let kind = if !style.box_shadow.is_empty() && node.is_none() {
                assert!(style.box_shadow.iter().all(|shadow| !shadow.inset));
                "shadow"
            } else {
                "box"
            };
            out.push((
                kind.to_owned(),
                fragment.rect.x.to_px(),
                fragment.rect.y.to_px(),
                fragment.rect.width.to_px(),
            ));
        }
        for child in &fragment.children {
            walk(child, out);
        }
    }
    let (_, pages) = render_document(
        "<!DOCTYPE html><style>@page { margin: 0 } body { margin: 0 } \
         tr { box-shadow: 0 0 5px red, inset 0 0 2px blue } td { width: 50px; height: 20px; padding: 0 }</style>\
         <table style='border-spacing: 2px'><tr><td></td><td></td></tr><tr><td></td><td></td></tr></table>",
    );
    let mut found = Vec::new();
    for fragment in &pages[0].fragments {
        walk(fragment, &mut found);
    }
    let shadows: Vec<_> = found.iter().filter(|(kind, ..)| kind == "shadow").collect();
    assert_eq!(shadows.len(), 2, "{found:?}");
    assert_eq!((shadows[0].1, shadows[0].3), (2.0, 102.0), "{found:?}");
    assert!(shadows[1].2 > shadows[0].2, "{found:?}");
}

#[test]
fn inline_list_items_have_their_markers_inside() {
    let found = boxes(
        "<ol><li class=item style='display: inline list-item'>x</li></ol>
         <ol><li class=plain style='display: inline'>x</li></ol>",
        &["item", "plain"],
    );
    assert!(found[0].2 > found[1].2 + 5.0, "{found:?}");
}

#[test]
fn canvases_are_transparent_replaced_elements() {
    let found = boxes(
        "<canvas class=plain></canvas>
         <div style='width: 50px'><canvas class=bounded width=200 height=100 style='max-width: 100%'></canvas></div>",
        &["plain", "bounded"],
    );
    assert_eq!((found[0].2, found[0].3), (300.0, 150.0), "{found:?}");
    // The ratio of its attributes holds where a maximum width bounds it.
    assert_eq!((found[1].2, found[1].3), (50.0, 25.0), "{found:?}");
}

#[test]
fn percentage_widths_beside_floats_refer_to_the_containing_block() {
    // WPT floats-wrap-bfc-005: half of 300px does not fit beside a float 200px wide.
    let found = boxes(
        "<div style='width: 300px; overflow: hidden'>
           <div style='float: left; width: 200px; height: 20px'></div>
           <div class=half style='overflow: hidden; width: 50%; height: 20px'></div>
         </div>",
        &["half"],
    );
    assert_eq!(
        (found[0].0, found[0].1, found[0].2),
        (0.0, 20.0, 150.0),
        "{found:?}"
    );
}

#[test]
fn striped_cells_with_collapsed_borders_share_their_styles() {
    use crate::fragment::FragmentKind;
    let rows = "<tr><td class=c0>x</td><td>y</td></tr><tr><td class=c1>x</td><td>y</td></tr>
        <tr><td class=c2>x</td><td>y</td></tr><tr><td class=c3>x</td><td>y</td></tr>
        <tr><td class=c4>x</td><td>y</td></tr><tr><td class=c5>x</td><td>y</td></tr>";
    let (document, pages) = render_document(&format!(
        "<!DOCTYPE html><style>@page {{ margin: 0 }} body {{ margin: 0; font: 12px/16px Inter }}
         table {{ border-collapse: collapse }} td {{ border: 2px solid rgb(0, 0, 255) }}
         tr:nth-child(even) td {{ background: rgb(240, 240, 240) }}</style><table>{rows}</table>"
    ));
    let style = |row: usize| {
        let (_, _, fragment) = find(&pages[0], &document, &format!("c{row}")).expect("cell");
        let FragmentKind::Box { style, .. } = &fragment.kind else {
            panic!("not a box")
        };
        std::sync::Arc::clone(style)
    };
    // The middle rows have the same borders; the even and the odd ones a style each.
    assert!(std::sync::Arc::ptr_eq(&style(1), &style(3)));
    assert!(std::sync::Arc::ptr_eq(&style(2), &style(4)));
    assert!(!std::sync::Arc::ptr_eq(&style(2), &style(3)));
    for row in 1..5 {
        assert_eq!(style(row).border_width.top, 2.0);
    }
    assert_ne!(style(1).background_color, style(2).background_color);
}

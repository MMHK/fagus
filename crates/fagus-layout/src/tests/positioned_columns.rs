//! Columns as Chrome prints them: absolutely positioned boxes placed in the columns laid end to
//! end and broken across them, columns inside columns, rows of columns that wrap, and empty
//! flex containers that break like empty blocks. The numbers are Chrome's (WPT css-break and
//! css-multicol tests).

use super::render_document;
use crate::fragment::{Fragment, FragmentKind, Page};

/// Renders `body` on pages 400px × 300px without margins.
fn render(css: &str, body: &str) -> (fagus_dom::Document, Vec<Page>) {
    render_document(&format!(
        "<style>@page {{ size: 400px 300px; margin: 0 }} body {{ margin: 0; font: 16px/20px Inter }}
         p {{ margin: 0 }} {css}</style>{body}"
    ))
}

/// The border boxes (x, y, width, height) of the fragments of the elements with class `class`
/// on `page`, in page coordinates.
fn boxes(document: &fagus_dom::Document, page: &Page, class: &str) -> Vec<(f32, f32, f32, f32)> {
    fn walk(
        fragment: &Fragment,
        document: &fagus_dom::Document,
        class: &str,
        origin: (f32, f32),
        out: &mut Vec<(f32, f32, f32, f32)>,
    ) {
        let x = origin.0 + fragment.rect.x.to_px();
        let y = origin.1 + fragment.rect.y.to_px();
        if let FragmentKind::Box { node: Some(node), .. } = &fragment.kind
            && document
                .element(*node)
                .is_some_and(|data| data.classes.iter().any(|name| &**name == class))
        {
            out.push((x, y, fragment.rect.width.to_px(), fragment.rect.height.to_px()));
        }
        for child in &fragment.children {
            walk(child, document, class, (x, y), out);
        }
    }
    let mut out = Vec::new();
    for fragment in &page.fragments {
        walk(fragment, document, class, (0.0, 0.0), &mut out);
    }
    out
}

#[test]
fn breaks_absolute_boxes_across_columns_and_adds_columns_after_the_last() {
    // The box starts 50px into its containing block, which starts the second of four columns
    // 100px tall: it fills the rest of that column and the two after it, and the column added
    // after the last holds its last 50px.
    let (document, pages) = render(
        ".mc { columns: 4; column-gap: 0; column-fill: auto; width: 200px; height: 100px }
         .rel { position: relative } .abs { position: absolute; top: 50px; width: 50px; height: 300px }",
        "<div class=mc><div style='height: 100px'></div><div class=rel><div class=abs></div></div></div>",
    );
    assert_eq!(pages.len(), 1);
    assert_eq!(
        boxes(&document, &pages[0], "abs"),
        [
            (50.0, 50.0, 50.0, 50.0),
            (100.0, 0.0, 50.0, 100.0),
            (150.0, 0.0, 50.0, 100.0),
            (200.0, 0.0, 50.0, 50.0)
        ]
    );
}

#[test]
fn breaks_absolute_boxes_where_their_overflow_crosses_the_end_of_a_column() {
    // A box 10px tall whose content reaches 250px down breaks like its content.
    let (document, pages) = render(
        ".mc { columns: 2; column-gap: 0; column-fill: auto; width: 100px; height: 100px }
         .rel { position: relative } .abs { position: absolute; width: 50px; height: 10px }
         .inner { height: 250px; width: 25px }",
        "<div class=mc><div class=rel><div class=abs><div class=inner></div></div></div></div>",
    );
    let inner = boxes(&document, &pages[0], "inner");
    assert_eq!(
        inner
            .iter()
            .map(|&(x, y, _, height)| (x, y, height))
            .collect::<Vec<_>>(),
        [(0.0, 0.0, 100.0), (50.0, 0.0, 100.0), (100.0, 0.0, 50.0)]
    );
}

#[test]
fn keeps_absolute_boxes_in_clipped_containing_blocks_in_the_last_column() {
    // The containing block clips its overflow: what does not fit in its two columns stays in
    // the second one, clipped there, instead of adding columns.
    let (document, pages) = render(
        ".mc { columns: 2; column-gap: 0; column-fill: auto; width: 100px; height: 100px }
         .rel { position: relative; overflow: clip; height: 200px } .abs { position: absolute; width: 50px; height: 500px }",
        "<div class=mc><div class=rel><div class=abs></div></div></div>",
    );
    assert_eq!(
        boxes(&document, &pages[0], "abs"),
        [(0.0, 0.0, 50.0, 100.0), (50.0, 0.0, 50.0, 400.0)]
    );
}

#[test]
fn balances_columns_around_absolute_boxes() {
    // The columns grow from the 63px of their content to hold the box below its placeholder.
    let (document, pages) = render(
        ".mc { columns: 1 } .rel { position: relative; height: 13px } .abs { position: absolute; width: 100%; height: 50px }",
        "<div class=mc><div style='height: 50px'></div><div class=rel><div class=abs></div></div></div>",
    );
    assert_eq!(boxes(&document, &pages[0], "mc")[0].3, 100.0);
    assert_eq!(boxes(&document, &pages[0], "abs"), [(0.0, 50.0, 400.0, 50.0)]);
}

#[test]
fn breaks_flex_containers_without_items_like_empty_blocks() {
    // The empty container 150px tall breaks at the end of the first page instead of moving
    // to the next one.
    let (document, pages) = render(
        ".flex { display: flex; height: 150px; width: 20px }",
        "<div style='height: 200px'></div><div class=flex></div>",
    );
    assert_eq!(pages.len(), 2);
    assert_eq!(boxes(&document, &pages[0], "flex"), [(0.0, 200.0, 20.0, 100.0)]);
    assert_eq!(boxes(&document, &pages[1], "flex"), [(0.0, 0.0, 20.0, 50.0)]);
}

#[test]
fn lays_out_columns_inside_columns() {
    // Each of the two outer columns holds two inner ones 50px tall.
    let (document, pages) = render(
        ".mc { columns: 2; column-gap: 0; column-fill: auto; width: 200px; height: 50px }
         .inner { columns: 2; column-gap: 0; column-fill: auto } .b { height: 200px }",
        "<div class=mc><div class=inner><div class=b></div></div></div>",
    );
    assert_eq!(
        boxes(&document, &pages[0], "b"),
        [
            (0.0, 0.0, 50.0, 50.0),
            (50.0, 0.0, 50.0, 50.0),
            (100.0, 0.0, 50.0, 50.0),
            (150.0, 0.0, 50.0, 50.0)
        ]
    );
}

#[test]
fn wraps_rows_of_columns_below_one_another() {
    // Rows of two columns 45px tall, 10px apart: the content 180px tall takes two rows.
    let (document, pages) = render(
        ".mc { columns: 2; column-fill: auto; width: 100px; gap: 10px 20px; column-height: 45px; column-wrap: wrap }
         .b { height: 180px }",
        "<div class=mc><div class=b></div></div><p class=after>after</p>",
    );
    assert_eq!(
        boxes(&document, &pages[0], "b"),
        [
            (0.0, 0.0, 40.0, 45.0),
            (60.0, 0.0, 40.0, 45.0),
            (0.0, 55.0, 40.0, 45.0),
            (60.0, 55.0, 40.0, 45.0)
        ]
    );
    assert_eq!(boxes(&document, &pages[0], "mc")[0].3, 100.0);
}

#[test]
fn places_fixed_boxes_inside_transformed_boxes_in_them() {
    // The transformed box is the containing block of the fixed box, which shows once, in it,
    // instead of on every page.
    let (document, pages) = render(
        ".t { transform: translateX(0); margin-top: 50px; height: 100px }
         .f { position: fixed; top: 10px; left: 20px; width: 30px; height: 30px }",
        "<div class=t><div class=f></div></div><div style='height: 600px'></div>",
    );
    assert_eq!(pages.len(), 3);
    assert_eq!(boxes(&document, &pages[0], "f"), [(20.0, 60.0, 30.0, 30.0)]);
    assert_eq!(boxes(&document, &pages[1], "f"), []);
}

#[test]
fn balances_the_columns_of_absolutely_positioned_containers() {
    // Four lines 50px tall in two columns 50px wide (`column-count` 2, as `column-width`
    // allows five): the columns are 100px tall.
    let (document, pages) = render(
        ".mc { position: absolute; width: 100px; line-height: 50px; columns: 2 20px; column-gap: 0;
           orphans: 1; widows: 1 }",
        "<div class=mc><br><br><br><br></div>",
    );
    assert_eq!(boxes(&document, &pages[0], "mc"), [(0.0, 0.0, 100.0, 100.0)]);
}

#[test]
fn places_absolute_boxes_in_balanced_rows_above_spanners() {
    // The row above the spanner is balanced to 80px (the box 160px tall needs two columns of
    // that), and the box breaks at its end instead of reaching into the spanner.
    let (document, pages) = render(
        ".mc { columns: 2; width: 100px; height: 100px; column-fill: auto; column-gap: 0 }
         .rel { position: relative; height: 100px } .abs { position: absolute; width: 50px; height: 160px }
         .span { column-span: all; height: 20px }",
        "<div class=mc><div class=rel><div class=abs></div></div><div class=span></div></div>",
    );
    assert_eq!(
        boxes(&document, &pages[0], "abs"),
        [(0.0, 0.0, 50.0, 80.0), (50.0, 0.0, 50.0, 80.0)]
    );
    assert_eq!(boxes(&document, &pages[0], "span"), [(0.0, 80.0, 100.0, 20.0)]);
}

#[test]
fn places_absolute_boxes_in_inline_containing_blocks_across_columns() {
    // The inline box starts at the top of the second column and ends at the bottom of the
    // fourth: a box at its bottom fills the last two columns, one 100px above its top the
    // first two. (In standards mode: in quirks mode Fagus places the fragments of the inline
    // box on lines of nothing but inline blocks otherwise than Chrome.)
    let (document, pages) = render_document(&format!(
        "<!doctype html><style>@page {{ size: 400px 300px; margin: 0 }} body {{ margin: 0; font: 16px/20px Inter }}
         .mc {{ columns: 4; column-gap: 0; column-fill: auto; width: 100px; height: 100px }}
         .rel {{ position: relative }} .ib {{ display: inline-block; width: 25px; height: 20px; vertical-align: top }}
         .abs {{ position: absolute; width: 25px; height: 200px }}</style>
         <div class=mc><div style='height: 100px'></div><span class=rel>{}<div class='abs a' style='bottom: 0'></div>{}<div class='abs b' style='top: -100px'></div></span></div>",
        "<span class=ib></span> ".repeat(7),
        "<span class=ib></span> ".repeat(8),
    ));
    assert_eq!(
        boxes(&document, &pages[0], "a"),
        [(50.0, 0.0, 25.0, 100.0), (75.0, 0.0, 25.0, 100.0)]
    );
    assert_eq!(
        boxes(&document, &pages[0], "b"),
        [(0.0, 0.0, 25.0, 100.0), (25.0, 0.0, 25.0, 100.0)]
    );
}

#[test]
fn makes_columns_as_tall_as_the_page_where_forced_breaks_need_more() {
    // Two columns hold the first two blocks; the third goes on on the next page (balanced
    // there, half of it in each column), and the columns on this one take the whole page, as
    // in Chrome.
    let (document, pages) = render(
        ".mc { columns: 2; column-gap: 0; width: 100px } .b { break-before: column }",
        "<div class=mc><div style='height: 100px'></div><div class=b style='height: 90px'></div>\
         <div class=b style='height: 10px'></div></div>",
    );
    assert_eq!(pages.len(), 2);
    assert_eq!(boxes(&document, &pages[0], "mc"), [(0.0, 0.0, 100.0, 300.0)]);
    assert_eq!(boxes(&document, &pages[1], "mc"), [(0.0, 0.0, 100.0, 5.0)]);
}

#[test]
fn sizes_columns_to_fit_their_spanners() {
    // The spanner 100px wide sizes the container on its own, not as a column of three.
    let (document, pages) = render(
        ".mc { width: fit-content; columns: 3 } .s { column-span: all } .b { width: 100px; height: 10px }",
        "<div class=mc><div><div class=s><div class=b></div></div></div></div>",
    );
    assert_eq!(boxes(&document, &pages[0], "mc")[0].2, 100.0);
    // With a `column-width` and no count, the max-content width is one column of it.
    let (document, pages) = render(
        ".mc { float: left; column-width: 50px; column-gap: 0 } .b { width: 120px; height: 10px }",
        "<div class=mc><div class=b></div></div>",
    );
    assert_eq!(boxes(&document, &pages[0], "mc")[0].2, 50.0);
}

#[test]
fn places_absolute_boxes_in_unbalanced_columns_as_tall_as_the_room_around_them() {
    // Columns filled in turn inside columns 100px tall are fragmentainers as tall as the room
    // the outer column leaves them, whatever their content: the 400px box fills two of them in
    // each outer column (WPT out-of-flow-in-multicolumn-023).
    let (document, pages) = render(
        ".mc { columns: 2; column-fill: auto; column-gap: 0 } .outer { width: 100px; height: 100px }
         .inner { width: 50px } .abs { position: absolute; width: 25px; height: 400px }",
        "<div class='mc outer'><div class='mc inner'><div style='position: relative'><div class=abs>
         </div></div></div></div>",
    );
    assert_eq!(pages.len(), 1);
    assert_eq!(
        boxes(&document, &pages[0], "abs"),
        [
            (0.0, 0.0, 25.0, 100.0),
            (25.0, 0.0, 25.0, 100.0),
            (50.0, 0.0, 25.0, 100.0),
            (75.0, 0.0, 25.0, 100.0)
        ]
    );
}

#[test]
fn places_absolute_boxes_in_their_grid_areas() {
    // `grid-area: 2 / 2` starts at the second column and row (40px and 30px into the content
    // box, 5px into the padding box) and ends at the padding edges (its end lines are `auto`),
    // where the box aligns to the end; one between lines reaches the padding edges where they
    // are `auto`; a child of a grid that is not its containing block is aligned in the content
    // box (as Chrome prints them).
    let (document, pages) = render(
        ".g { position: relative; display: grid; grid-template: 30px 70px / 40px 60px; width: 100px;
              height: 100px; padding: 5px }
         .a { position: absolute; width: 20px; height: 20px }",
        "<div class=g><div class='a end' style='grid-area: 2 / 2; align-self: end; justify-self: end'></div>
         <div class='a edges' style='grid-column: 2 / auto; inset: 0; width: auto; height: auto'></div></div>
         <div style='display: grid; place-items: center; width: 100px; height: 100px'>
         <div class='a centered'></div></div>",
    );
    assert_eq!(boxes(&document, &pages[0], "end"), [(90.0, 90.0, 20.0, 20.0)]);
    assert_eq!(boxes(&document, &pages[0], "edges"), [(45.0, 0.0, 65.0, 110.0)]);
    assert_eq!(
        boxes(&document, &pages[0], "centered"),
        [(40.0, 150.0, 20.0, 20.0)]
    );
}

#[test]
fn breaks_grid_rows_without_items_anywhere() {
    // The empty second row (50px to 150px) breaks at the end of the first column: the box in
    // the third row's area starts halfway down the second (WPT grid-item-oof-006).
    let (document, pages) = render(
        ".mc { columns: 2; column-fill: auto; column-gap: 0; width: 100px; height: 100px }
         .abs { position: absolute; inset: 0; grid-row-start: 3 }",
        "<div class=mc><div style='position: relative; display: grid; grid-template-rows: 50px 100px 50px'>
         <div><div class=abs></div></div></div></div>",
    );
    assert_eq!(boxes(&document, &pages[0], "abs"), [(50.0, 50.0, 50.0, 50.0)]);
}

#[test]
fn places_boxes_of_the_multicol_container_in_the_column_of_their_static_position() {
    // The containing block is the multi-column container itself: Chrome places a box by its
    // static position in the column that lies in (100px down a grid 400px tall that aligns it
    // at the end or in the middle: the fourth column, the second 50px down) and does not
    // break it across columns.
    let css = ".mc { columns: 4; column-gap: 0; column-fill: auto; width: 100px; height: 100px;
               position: relative } .abs { position: absolute; width: 25px; height: 100px }";
    let grid = |align: &str| {
        format!(
            "<div class=mc><div style='display: grid; height: 400px; align-items: {align}'>
             <div class=abs></div><div style='height: 100px'></div><div style='height: 100px'></div>
             <div style='height: 100px'></div><div style='height: 100px'></div></div></div>"
        )
    };
    let (document, pages) = render(css, &grid("end"));
    assert_eq!(boxes(&document, &pages[0], "abs"), [(75.0, 0.0, 25.0, 100.0)]);
    let (document, pages) = render(css, &grid("center"));
    assert_eq!(boxes(&document, &pages[0], "abs"), [(25.0, 50.0, 25.0, 100.0)]);
}

#[test]
fn moves_fixed_boxes_by_the_relative_offsets_between_them_and_their_containing_block() {
    // The fixed box's containing block is the transformed box; the relatively positioned box
    // between them moves its static position by its offset (20px, 30px), and it breaks across
    // the columns from there (WPT css-break out-of-flow-in-multicolumn-073; Chrome 157).
    let (document, pages) = render(
        ".mc { columns: 2; column-gap: 0; column-fill: auto; width: 100px; height: 100px }
         .rel { position: relative; top: 30px; left: 20px } .abs { position: absolute }
         .fixed { position: fixed; width: 50px; height: 100px }",
        "<div class=mc><div style='transform: translateX(0)'><div class=rel><div class=abs>
         <div class=fixed></div></div></div></div></div>",
    );
    assert_eq!(
        boxes(&document, &pages[0], "fixed"),
        [(20.0, 30.0, 50.0, 70.0), (70.0, 0.0, 50.0, 30.0)]
    );
}

#[test]
fn places_fixed_boxes_in_columns_where_their_containing_block_has_none() {
    // The containing block (the transformed box) ends in the first column; the absolutely
    // positioned box around the fixed one fills the first and second, and the fixed box's
    // static position is at the end of the second: it goes in the third column, the one added
    // after the last (WPT css-break out-of-flow-in-multicolumn-032; Chrome 157).
    let (document, pages) = render(
        ".mc { columns: 2; column-gap: 0; column-fill: auto; width: 100px; height: 100px }
         .rel { position: relative } .abs { position: absolute; width: 50px; height: 200px }
         .fixed { position: fixed; width: 50px; height: 100px }",
        "<div class=mc><div style='transform: translateX(0)'><div class=rel><div class=abs>
         <div style='height: 200px'></div><div class=fixed></div></div></div></div></div>",
    );
    assert_eq!(boxes(&document, &pages[0], "fixed"), [(100.0, 0.0, 50.0, 100.0)]);
}

#[test]
fn breaks_absolute_boxes_inside_boxes_that_clip_only_their_inline_overflow() {
    // `overflow-x: clip` clips nothing in the block direction: the box inside breaks across the
    // three columns as without it (WPT css-break out-of-flow-in-multicolumn-109; Chrome 157).
    let (document, pages) = render(
        ".mc { columns: 3; gap: 20px; column-fill: auto; width: 100px; height: 100px }
         .abs { position: absolute; width: 100%; height: 300px }",
        "<div class=mc><div style='overflow-x: clip'><div style='position: relative'>
         <div class=abs></div></div></div></div>",
    );
    assert_eq!(
        boxes(&document, &pages[0], "abs"),
        [
            (0.0, 0.0, 20.0, 100.0),
            (40.0, 0.0, 20.0, 100.0),
            (80.0, 0.0, 20.0, 100.0)
        ]
    );
}

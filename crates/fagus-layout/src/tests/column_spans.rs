//! Columns as Chrome prints them: balancing past avoided breaks, baselines, spanners in boxes of
//! a fixed height and in content that overflows them, rows that move below a spanner, page
//! names, cloned decorations, rules, table cells and columns without width or height. The
//! numbers are Chrome's (WPT css-multicol and css-break tests, and probes).

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

/// The rules between columns on `page` (boxes without a node that draw only a left border),
/// as (x, y, height).
fn rules(page: &Page) -> Vec<(f32, f32, f32)> {
    fn walk(fragment: &Fragment, origin: (f32, f32), out: &mut Vec<(f32, f32, f32)>) {
        let x = origin.0 + fragment.rect.x.to_px();
        let y = origin.1 + fragment.rect.y.to_px();
        if let FragmentKind::Box {
            style, node: None, ..
        } = &fragment.kind
            && style.border_width.left > 0.0
            && style.border_width.top == 0.0
            && fragment.children.is_empty()
        {
            out.push((x, y, fragment.rect.height.to_px()));
        }
        for child in &fragment.children {
            walk(child, (x, y), out);
        }
    }
    let mut out = Vec::new();
    for fragment in &page.fragments {
        walk(fragment, (0.0, 0.0), &mut out);
    }
    out
}

#[test]
fn balances_columns_past_a_break_that_break_before_avoids() {
    // The box with `break-before: avoid` cannot start the second column, and an empty box 50px
    // tall does not break so that it would: the columns grow by the room the box needed, 25px,
    // from an even share of 75px (WPT css-multicol/balance-break-avoidance-002).
    let (document, pages) = render(
        ".mc { width: 100px; columns: 2; column-gap: 0 } .a { height: 25px } .b { height: 50px }
         .v { break-before: avoid }",
        "<div class=mc><div class=a></div><div class=b></div><div class='a v'></div>
         <div class=b></div></div>",
    );
    assert_eq!(boxes(&document, &pages[0], "mc")[0].3, 100.0);
    assert_eq!(boxes(&document, &pages[0], "v"), [(0.0, 75.0, 50.0, 25.0)]);
}

#[test]
fn takes_the_highest_first_baseline_of_the_columns() {
    // The first column holds only a box without lines; the first line of the second one gives
    // the baseline, at the same height as that of the box beside the columns
    // (WPT css-multicol/baseline-002).
    let (document, pages) = render(
        ".f { display: flex; align-items: baseline; line-height: 32px } .a { width: 50px; height: 100px }
         .mc { columns: 3; orphans: 1; widows: 1; width: 50px; height: 100px } .x { break-inside: avoid; height: 32px }",
        "<div class=f><div class=a><br></div><div class=mc><div class=x></div><br><br><br></div></div>",
    );
    assert_eq!(boxes(&document, &pages[0], "a")[0].1, 0.0);
    assert_eq!(boxes(&document, &pages[0], "mc")[0].1, 0.0);
}

#[test]
fn spans_no_columns_from_inside_a_box_that_contains_fixed_boxes() {
    // A filter makes the box around the heading the containing block of fixed boxes: the
    // heading is balanced in the columns like any block (WPT css-multicol/multicol-span-all-010).
    let (document, pages) = render(
        ".mc { columns: 3; column-gap: 0; width: 300px } .h { column-span: all; height: 21px }",
        "<div class=mc><div style='filter: contrast(100%)'><div class=h></div></div></div>",
    );
    assert_eq!(
        boxes(&document, &pages[0], "h"),
        [
            (0.0, 0.0, 100.0, 7.0),
            (100.0, 0.0, 100.0, 7.0),
            (200.0, 0.0, 100.0, 7.0)
        ]
    );
}

#[test]
fn shares_the_height_of_a_box_around_spanners_out_over_its_rows() {
    // A box 350px tall around blocks 200px tall and two spanners: 100px in each column of the
    // first row, 100px and 50px in the second; the last block overflows it in the third (WPT
    // css-multicol/multicol-span-all-children-height-004b).
    let (document, pages) = render(
        ".mc { columns: 2; column-gap: 0; width: 400px } .c { height: 350px } .b { width: 100px; height: 200px }
         .s { column-span: all; height: 50px }",
        "<div class=mc><div class=c><div class=b></div><div class=s></div><div class=b></div>
         <div class=s></div><div class=b></div></div></div>",
    );
    let heights: Vec<f32> = boxes(&document, &pages[0], "c").iter().map(|b| b.3).collect();
    assert_eq!(heights, [100.0, 100.0, 100.0, 50.0]);
    let spanners: Vec<f32> = boxes(&document, &pages[0], "s").iter().map(|b| b.1).collect();
    assert_eq!(spanners, [100.0, 250.0]);
}

#[test]
fn stops_content_overflowing_a_box_at_a_spanner_in_it() {
    // A box without height holds a block 60px tall and a spanner: the block is balanced in two
    // columns 30px tall, the spanner follows them, and the box after it starts the next row
    // (WPT css-multicol/multicol-span-all-children-height-012).
    let (document, pages) = render(
        ".mc { columns: 2; gap: 0; width: 100px } .z { height: 0 } .b { height: 60px }
         .s { column-span: all; height: 50px } .r { height: 40px }",
        "<div class=mc><div class=z><div class=b></div><div class=s></div></div><div class=r></div></div>",
    );
    assert_eq!(
        boxes(&document, &pages[0], "b"),
        [(0.0, 0.0, 50.0, 30.0), (50.0, 0.0, 50.0, 30.0)]
    );
    assert_eq!(boxes(&document, &pages[0], "s"), [(0.0, 30.0, 100.0, 50.0)]);
    assert_eq!(
        boxes(&document, &pages[0], "r"),
        [(0.0, 80.0, 50.0, 20.0), (50.0, 80.0, 50.0, 20.0)]
    );
}

#[test]
fn ignores_page_names_in_columns() {
    // A box of another page name inside columns breaks neither the page nor the column
    // (WPT css-multicol/named-page).
    let (document, pages) = render(
        ".mc { columns: 1; column-fill: auto; width: 100px; height: 100px } .b { height: 50px }
         .n { page: other }",
        "<div class=mc><div class=b></div><div class='b n'></div></div>",
    );
    assert_eq!(pages.len(), 1);
    assert_eq!(boxes(&document, &pages[0], "n"), [(0.0, 50.0, 100.0, 50.0)]);
}

/// A spanner at `top`, a heading that avoids a break after it and a paragraph with `orphans`
/// and `widows` of `lines`, in two columns on a page 300px tall.
fn row_below_spanner(top: u32, lines: u32) -> (fagus_dom::Document, Vec<Page>) {
    render(
        &format!(
            "p {{ orphans: {lines}; widows: {lines} }} .mc {{ columns: 2; column-gap: 0 }}
             .s {{ column-span: all; height: 20px }} .h {{ break-after: avoid }}"
        ),
        &format!(
            "<div style='height: {top}px'></div><div class=mc><div class=s></div><div class=h>Heading</div>
             <p>aa bb cc dd ee ff gg hh ii jj kk ll mm nn oo pp qq rr ss tt uu vv ww xx yy zz ab bc
             cd de ef fg gh hi ij jk kl lm mn no op pq qr rs st tu uv vw wx xy</p></div>"
        ),
    )
}

#[test]
fn moves_a_row_below_a_spanner_whose_last_column_breaks_poorly() {
    // Below the spanner, room for two lines: the heading and one line of the paragraph in the
    // first column, two in the second, fewer than `orphans: 4` asks for. Chrome keeps the
    // spanner on the page and moves the row to the next (audit18 b13).
    let (document, pages) = row_below_spanner(240, 4);
    assert_eq!(boxes(&document, &pages[0], "s"), [(0.0, 240.0, 400.0, 20.0)]);
    assert_eq!(boxes(&document, &pages[1], "h")[0].1, 0.0);
}

#[test]
fn moves_columns_whose_first_column_alone_breaks_poorly() {
    // With `orphans: 2`, the second column breaks well, the first one does not: Chrome moves
    // all of the container to the next page.
    let (document, pages) = row_below_spanner(240, 2);
    assert_eq!(boxes(&document, &pages[0], "s"), []);
    assert_eq!(boxes(&document, &pages[1], "s"), [(0.0, 0.0, 400.0, 20.0)]);
}

#[test]
fn repeats_cloned_decorations_of_columns_in_each_column_around_them() {
    // Columns 300px tall with a bottom border that clones go on in outer columns 125px tall:
    // 100px of them and the border in each (WPT css-multicol/multicol-breaking-004).
    let (document, pages) = render(
        ".o { columns: 3; column-fill: auto; column-gap: 0; width: 300px; height: 125px }
         .i { columns: 2; height: 300px; border-bottom: 25px solid; box-decoration-break: clone }",
        "<div class=o><div class=i>a<br>b<br>c<br>d<br>e<br>f<br>g<br>h<br>i<br>j<br>k<br>l</div></div>",
    );
    let parts: Vec<(f32, f32)> = boxes(&document, &pages[0], "i")
        .iter()
        .map(|b| (b.0, b.3))
        .collect();
    assert_eq!(parts, [(0.0, 125.0), (100.0, 125.0), (200.0, 125.0)]);
}

#[test]
fn draws_the_rules_of_the_last_row_down_to_the_end_of_the_container() {
    // Balanced columns two lines tall in a container 300px tall: the rule is as tall as the
    // container, as in Chrome.
    let (_, pages) = render(
        ".mc { columns: 2; column-rule: 2px solid; height: 200px; width: 400px }",
        "<div class=mc>a<br>b<br>c</div>",
    );
    assert_eq!(rules(&pages[0]), [(199.0, 0.0, 200.0)]);
}

#[test]
fn lays_out_table_cells_in_columns() {
    // Six lines in six columns of a table cell 120px wide (WPT css-multicol/table/
    // multicol-table-cell-001).
    let (document, pages) = render(
        ".c { display: table-cell; columns: 6; column-gap: 0; width: 120px; orphans: 1; widows: 1 }",
        "<div class=c>a<br>b<br>c<br>d<br>e<br>f</div>",
    );
    assert_eq!(boxes(&document, &pages[0], "c")[0].3, 20.0);
}

#[test]
fn makes_as_many_columns_as_asked_for_without_width() {
    // Two columns of no width hold a block 200px tall 100px each (WPT css-multicol/
    // multicol-fill-balance-006).
    let (document, pages) = render(
        ".mc { columns: 2; width: 0 } .b { width: 100px; height: 200px }",
        "<div class=mc><div class=b></div></div>",
    );
    assert_eq!(boxes(&document, &pages[0], "mc")[0].3, 100.0);
}

#[test]
fn breaks_a_float_into_columns_a_pixel_tall_in_a_container_without_height() {
    // Columns of a container 0px tall are a pixel tall: a float 10px tall takes ten of them
    // (WPT css-multicol/multicol-zero-height-001).
    let (document, pages) = render(
        ".mc { height: 0; width: 200px; columns: 10; column-gap: 0; orphans: 1; widows: 1 }
         .f { float: left; width: 100%; height: 10px }",
        "<div class=mc><div class=f></div></div>",
    );
    assert_eq!(pages.len(), 1);
    let floats = boxes(&document, &pages[0], "f");
    assert_eq!(floats.len(), 10);
    assert_eq!(floats[9], (180.0, 0.0, 20.0, 1.0));
}

#[test]
fn breaks_a_line_of_a_column_flex_container_before_an_item_it_may_break_before() {
    // The break before the last item of the line is avoided: the line breaks before the item
    // above it instead, and the balanced columns grow to hold it (WPT css-break/flexbox/
    // multi-line-column-flex-fragmentation-034).
    let (document, pages) = render(
        ".mc { width: 100px; columns: 2; column-gap: 0 }
         .fx { display: flex; flex-direction: column; flex-wrap: wrap; height: 200px }
         .fx > div { width: 25px; flex: none } .v { break-before: avoid; break-inside: avoid }",
        "<div class=mc><div class=fx><div style='height: 25px'></div><div style='height: 25px'></div>
         <div style='height: 50px'></div><div style='height: 25px; break-before: avoid'></div>
         <div style='height: 75px'></div><div style='height: 25px'></div><div style='height: 25px'></div>
         <div style='height: 50px'></div><div class=v style='height: 50px'></div>
         <div class=w style='height: 50px'></div></div></div>",
    );
    assert_eq!(boxes(&document, &pages[0], "v"), [(25.0, 100.0, 25.0, 50.0)]);
    assert_eq!(boxes(&document, &pages[0], "w")[0].0, 75.0);
}

#[test]
fn balances_columns_around_columns_that_hold_only_an_absolutely_positioned_box() {
    // The box 400px tall is all there is in the inner columns: it makes them 200px tall
    // measured unbroken, the outer columns 100px tall, and goes on in four inner columns 100px
    // tall (WPT css-break/out-of-flow-in-multicolumn-071).
    let (document, pages) = render(
        ".mc { columns: 2; column-gap: 0 } .o { width: 100px } .i { width: 50px; column-fill: auto }
         .a { position: absolute; height: 400px; width: 25px }",
        "<div class='mc o'><div class='mc i'><div style='position: relative'><div class=a></div></div></div></div>",
    );
    let parts: Vec<(f32, f32)> = boxes(&document, &pages[0], "a")
        .iter()
        .map(|b| (b.0, b.3))
        .collect();
    assert_eq!(parts, [(0.0, 100.0), (25.0, 100.0), (50.0, 100.0), (75.0, 100.0)]);
}

#[test]
fn keeps_a_float_before_a_spanner_in_the_row_above_it() {
    // The float's line cannot break: the row above the spanner grows to hold it, rather than
    // letting it go on below the spanner (WPT css-multicol/multicol-span-float-001).
    let (document, pages) = render(
        ".mc { columns: 2; column-gap: 0; width: 200px } .f { float: right; width: 200px }
         .s { column-span: all; height: 10px }",
        "<div class=mc><div class=f>Pink</div><div class=s></div>a b</div>",
    );
    assert_eq!(boxes(&document, &pages[0], "f")[0].1, 0.0);
    assert_eq!(boxes(&document, &pages[0], "s"), [(0.0, 20.0, 200.0, 10.0)]);
}

#[test]
fn starts_a_flex_item_pushed_to_a_column_a_pixel_tall_at_its_top() {
    // In columns a pixel tall, the first item's top margin pushes it to the second column,
    // where the break truncates it (WPT css-break/flexbox/single-line-row-flex-fragmentation-044).
    let (document, pages) = render(
        ".mc { columns: 2; column-gap: 0; column-fill: auto; width: 200px; height: 0 }
         .fx { display: flex; width: 100px } .fx > div { width: 50px; contain: size }",
        "<div class=mc><div class=fx><div class=p style='height: 10px; margin-top: 5px'></div>
         <div class=o style='height: 20px'></div></div></div>",
    );
    assert_eq!(pages.len(), 1);
    assert_eq!(boxes(&document, &pages[0], "o"), [(50.0, 0.0, 50.0, 20.0)]);
    assert_eq!(boxes(&document, &pages[0], "p"), [(100.0, 0.0, 50.0, 10.0)]);
}

#[test]
fn keeps_the_margins_of_spanners_apart_across_a_box_between_them() {
    // The second spanner is the first child of a box: its margin does not push the box past
    // the end of an empty row (which moved the rest to the next page), and the row of the box
    // keeps it apart from the first spanner's (WPT css-multicol/
    // multicol-span-all-margin-nested-001).
    let (document, pages) = render(
        ".mc { columns: 4; column-gap: 0; width: 160px; orphans: 1; widows: 1 }
         .s { margin: 20px 0; column-span: all; height: 20px }",
        "<div class=mc>a b c d<div class='s a'></div><div><div class='s b'></div></div>e f g h</div>",
    );
    assert_eq!(pages.len(), 1);
    assert_eq!(boxes(&document, &pages[0], "a")[0].1, 40.0);
    assert_eq!(boxes(&document, &pages[0], "b")[0].1, 100.0);
}

#[test]
fn collapses_the_margins_of_spanners_in_one_box() {
    // Between two spanners in one box, the box goes on without content: their margins collapse
    // (WPT css-multicol/multicol-span-all-margin-003).
    let (document, pages) = render(
        ".mc { columns: 3; width: 100px } .s { column-span: all; height: 10px }",
        "<div class=mc><div><div class='s a' style='margin-bottom: 50px'></div>
         <div class='s b' style='margin-top: 80px'></div></div></div>",
    );
    assert_eq!(boxes(&document, &pages[0], "b")[0].1, 90.0);
    assert_eq!(boxes(&document, &pages[0], "mc")[0].3, 100.0);
}

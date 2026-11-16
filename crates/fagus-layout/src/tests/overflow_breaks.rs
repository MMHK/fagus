//! Breaks as Chrome prints them where content overflows its box, where percentages and forced
//! breaks reach past the box they are given on, and where containing blocks of positioned boxes
//! are not their parents. The numbers are Chrome's (WPT css-break, css-multicol and CSS2
//! pagination tests, and probes).

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

const COLUMNS: &str = ".mc { columns: 2; column-gap: 0; column-fill: auto; width: 100px; height: 100px }";

#[test]
fn breaks_the_content_overflowing_a_column_flex_item() {
    // The first item is 10px tall; the content below its top 20px goes on in the next column.
    let (document, pages) = render(
        &format!(
            "{COLUMNS} .flex {{ display: flex; flex-direction: column; height: 200px; width: 50px }} .flex > div {{ height: 10px }}"
        ),
        "<div class=mc><div class=flex><div><div style='height: 20px'></div><div class=rest style='height: 180px'></div></div><div></div></div></div>",
    );
    assert_eq!(pages.len(), 1);
    assert_eq!(
        boxes(&document, &pages[0], "rest"),
        [(0.0, 20.0, 50.0, 80.0), (50.0, 0.0, 50.0, 100.0)]
    );
}

#[test]
fn breaks_the_content_overflowing_a_row_flex_item() {
    // The first line's item is 100px tall, its content 200px: the rest goes on in the next
    // column, beside the second line.
    let (document, pages) = render(
        &format!(
            "{COLUMNS} .flex {{ display: flex; flex-wrap: wrap; height: 200px; width: 50px }} .flex > div {{ height: 100px; width: 50px }}"
        ),
        "<div class=mc><div class=flex><div><div style='height: 100px'></div><div class=rest style='height: 100px'></div></div><div></div></div></div>",
    );
    assert_eq!(boxes(&document, &pages[0], "rest"), [(50.0, 0.0, 50.0, 100.0)]);
}

#[test]
fn balances_columns_with_the_content_overflowing_its_box() {
    // A box 50px tall with content 150px tall: the columns are 75px tall.
    let (document, pages) = render(
        ".mc { columns: 2; column-gap: 0; width: 100px }",
        "<div class=mc><div style='height: 50px'><div class=inner style='height: 150px'></div></div></div>",
    );
    assert_eq!(pages.len(), 1);
    assert_eq!(
        boxes(&document, &pages[0], "inner"),
        [(0.0, 0.0, 50.0, 75.0), (50.0, 0.0, 50.0, 75.0)]
    );
    assert_eq!(boxes(&document, &pages[0], "mc")[0].3, 75.0);
}

#[test]
fn resolves_percentage_heights_of_floats_against_their_block() {
    // The float is 60% of the body's 300px, does not fit below the paragraph and avoids
    // breaks inside: it moves to the next page with its height. The float after text, in an
    // anonymous block, refers to the body too.
    let (document, pages) = render(
        "html, body { height: 100% } .p { height: 60% } .float { float: left; width: 50px; height: 60%; break-inside: avoid }",
        "<div class=p></div><div class=float></div>",
    );
    assert_eq!(pages.len(), 2);
    assert_eq!(boxes(&document, &pages[1], "float"), [(0.0, 0.0, 50.0, 180.0)]);
    let (document, pages) = render(
        "html, body { height: 100% } .float { float: left; width: 50px; height: 50% }",
        "text<div class=float></div>",
    );
    assert_eq!(boxes(&document, &pages[0], "float")[0].3, 150.0);
}

#[test]
fn takes_a_forced_break_from_the_first_row_of_a_grid_over_an_avoided_one() {
    // The heading avoids a break after it, but the second item of the grid's first row forces
    // one before it: the heading stays at the end of the first page.
    let (document, pages) = render(
        ".h { height: 20px; break-after: avoid } .grid { display: grid; grid-template-columns: 1fr 1fr }
         .grid > div { height: 100px }",
        "<div style='height: 250px'></div><div class=h></div>
         <div class=grid><div></div><div style='break-before: page'></div></div>",
    );
    assert_eq!(pages.len(), 2);
    assert_eq!(boxes(&document, &pages[0], "h"), [(0.0, 250.0, 400.0, 20.0)]);
    assert_eq!(boxes(&document, &pages[1], "grid")[0].1, 0.0);
}

#[test]
fn places_fixed_boxes_in_transformed_boxes_across_columns() {
    // The fixed box's containing block is the transformed box, in the third and fourth column
    // (the first two lie left of the page): the fixed box fills them from its top.
    let (document, pages) = render(
        ".mc { columns: 2; column-fill: auto; column-gap: 0; height: 100px; width: 100px; margin-left: 100px }
         .rel { position: relative } .abs { position: absolute; width: 50px; height: 200px }
         .fixed { position: fixed; width: 50px; height: 200px; top: 0 }",
        "<div class=mc><div style='height: 200px'></div><div style='transform: translateX(0); height: 200px'>
         <div style='height: 200px'></div><div class=rel><div style='height: 200px'></div>
         <div class=abs><div class=fixed></div></div></div></div></div>",
    );
    assert_eq!(pages.len(), 1);
    assert_eq!(
        boxes(&document, &pages[0], "fixed"),
        [(200.0, 0.0, 50.0, 100.0), (250.0, 0.0, 50.0, 100.0)]
    );
}

#[test]
fn contains_fixed_boxes_in_boxes_that_will_change_their_transform() {
    // Not repeated on the second page, and placed in the box rather than the page.
    let (document, pages) = render(
        ".fixed { position: fixed; top: 10px; left: 10px; width: 20px; height: 20px }",
        "<div style='will-change: transform; margin: 50px 0 0 50px'><div class=fixed></div></div>
         <div style='height: 500px'></div>",
    );
    assert_eq!(pages.len(), 2);
    assert_eq!(boxes(&document, &pages[0], "fixed"), [(60.0, 60.0, 20.0, 20.0)]);
    assert_eq!(boxes(&document, &pages[1], "fixed"), []);
}

#[test]
fn resolves_percentage_heights_in_columns_against_the_container() {
    // Half of the container's 200px, balanced in the columns above the spanner; the
    // spanner's percentage refers to the container too.
    let (document, pages) = render(
        ".mc { columns: 2; column-gap: 0; column-fill: auto; width: 100px; height: 200px }
         .half { height: 50% } .span { column-span: all; height: 25% }",
        "<div class=mc><div class=half></div><div class=span></div></div>",
    );
    assert_eq!(
        boxes(&document, &pages[0], "half"),
        [(0.0, 0.0, 50.0, 50.0), (50.0, 0.0, 50.0, 50.0)]
    );
    assert_eq!(boxes(&document, &pages[0], "span")[0].3, 50.0);
}

#[test]
fn slices_the_backgrounds_of_rows_broken_across_pages() {
    // The image of the row's background is positioned in the whole row: on the second page it
    // starts above the part of the row there, by the part on the first.
    fn image_tops(fragment: &Fragment, out: &mut Vec<f32>) {
        if let FragmentKind::Box {
            style, node: None, ..
        } = &fragment.kind
            && style
                .background_image
                .iter()
                .any(|image| !matches!(image, fagus_style::values::Image::None))
        {
            out.push(fragment.rect.y.to_px());
        }
        for child in &fragment.children {
            image_tops(child, out);
        }
    }
    let (_, pages) = render(
        "table { border-spacing: 0 } tr { height: 400px; background: linear-gradient(red, blue) }",
        "<table><tr><td>a</td></tr></table>",
    );
    assert_eq!(pages.len(), 2);
    let tops = |page: &Page| {
        let mut out = Vec::new();
        for fragment in &page.fragments {
            image_tops(fragment, &mut out);
        }
        out
    };
    assert_eq!(tops(&pages[0]), [0.0]);
    assert_eq!(tops(&pages[1]), [-300.0]);
}

#[test]
fn moves_a_box_that_clears_floats_going_on_to_the_next_column_below_them() {
    // The float breaks across three columns 85px tall (balanced); the box that clears it goes
    // below its end in the third, not at the top of the second or third.
    let (document, pages) = render(
        ".mc { columns: 3; column-gap: 0; width: 300px; height: 100px }
         .float { float: left; width: 15px; height: 240px } .clear { clear: left; height: 5px }",
        "<div class=mc><div style='height: 10px'></div><div><div class=float></div></div>
         <div class=clear></div></div>",
    );
    assert_eq!(boxes(&document, &pages[0], "clear"), [(200.0, 80.0, 100.0, 5.0)]);
}

#[test]
fn moves_the_bottom_caption_of_an_empty_table_to_the_next_column() {
    // The table fills three columns; its caption starts the fourth.
    let (document, pages) = render(
        ".mc { columns: 4; column-gap: 0; column-fill: auto; width: 100px; height: 100px }
         .caption { display: table-caption; caption-side: bottom; height: 100px }",
        "<div class=mc><div style='display: table; height: 300px; width: 100%'><div class=caption></div></div></div>",
    );
    assert_eq!(boxes(&document, &pages[0], "caption"), [(75.0, 0.0, 25.0, 100.0)]);
}

#[test]
fn continues_the_items_of_a_column_flex_container_that_ends() {
    // The container ends with the first column; the second line of its item goes on in the
    // next one, in a part of the container without height, as in Chrome.
    let (document, pages) = render(
        &format!(
            "{COLUMNS} .flex {{ display: flex; flex-direction: column; height: 100px }} .flex > div {{ line-height: 0 }}"
        ),
        "<div class=mc><div class=flex><div class=item><div style='display: inline-block; width: 50px; height: 50px'></div>
         <div class=rest style='display: inline-block; width: 50px; height: 100px'></div></div></div></div>",
    );
    assert_eq!(pages.len(), 1);
    assert_eq!(boxes(&document, &pages[0], "rest"), [(50.0, 0.0, 50.0, 100.0)]);
    assert_eq!(
        boxes(&document, &pages[0], "flex"),
        [(0.0, 0.0, 50.0, 100.0), (50.0, 0.0, 50.0, 0.0)]
    );
}

#[test]
fn keeps_the_height_of_column_flex_items_that_shrink() {
    // The first item can shrink and has a minimum height: it keeps its 10px where its content
    // breaks, the content overflowing it, and the second item follows it in the first column,
    // where it shrinks to the rest (its automatic minimum is its empty content's height), as in
    // Chrome 157.
    let (document, pages) = render(
        &format!("{COLUMNS} .flex {{ display: flex; flex-direction: column; height: 100px }}"),
        "<div class=mc><div class=flex><div class=first style='flex-basis: 10px; min-height: 10px'>
         <div style='contain: size; height: 50px'></div><div class=rest style='contain: size; height: 100px'></div></div>
         <div class=second style='height: 190px'></div></div></div>",
    );
    assert_eq!(boxes(&document, &pages[0], "first")[0], (0.0, 0.0, 50.0, 10.0));
    assert_eq!(boxes(&document, &pages[0], "rest"), [(50.0, 0.0, 50.0, 100.0)]);
    assert_eq!(boxes(&document, &pages[0], "second"), [(0.0, 10.0, 50.0, 90.0)]);
}

#[test]
fn ends_row_flex_containers_of_a_fixed_height_where_their_items_overflow() {
    // The container is 70px tall; its item's content (140px) goes on in the next column
    // without the container.
    let (document, pages) = render(
        &format!(
            "{COLUMNS} .flex {{ display: flex; height: 70px }} .flex > div {{ flex: 1 }} .flex > div > div {{ height: 140px }}"
        ),
        "<div class=mc><div class=flex><div><div class=content></div></div></div></div>",
    );
    assert_eq!(
        boxes(&document, &pages[0], "flex"),
        [(0.0, 0.0, 50.0, 70.0), (50.0, 0.0, 50.0, 0.0)]
    );
    assert_eq!(
        boxes(&document, &pages[0], "content"),
        [(0.0, 0.0, 50.0, 100.0), (50.0, 0.0, 50.0, 40.0)]
    );
}

#[test]
fn drops_the_overflowing_content_of_items_that_a_box_clips() {
    // The flex container ends with the third column; its items below that overflow it, and
    // the box around it clips them: no column holds them.
    let (document, pages) = render(
        ".mc { columns: 5; column-gap: 0; column-fill: auto; width: 100px; height: 100px }
         .flex { display: flex; flex-direction: column; height: 300px } .flex > div { flex-shrink: 0 }",
        "<div class=mc><div style='overflow: clip'><div class=flex><div style='height: 300px'></div>
         <div class=below style='height: 200px'></div></div></div></div>",
    );
    assert_eq!(pages.len(), 1);
    assert_eq!(
        boxes(&document, &pages[0], "below"),
        Vec::<(f32, f32, f32, f32)>::new()
    );
}

#[test]
fn breaks_the_captions_above_a_table() {
    // The caption's content goes on in the next column, the rows below it there.
    let (document, pages) = render(
        COLUMNS,
        "<div class=mc><div style='display: table; width: 100%'>
         <div style='display: table-caption'><div style='height: 60px'></div><div class=second style='height: 80px'></div></div>
         <div style='display: table-row'><div class=cell style='display: table-cell; height: 10px'></div></div></div></div>",
    );
    assert_eq!(pages.len(), 1);
    assert_eq!(
        boxes(&document, &pages[0], "second"),
        [(0.0, 60.0, 50.0, 40.0), (50.0, 0.0, 50.0, 40.0)]
    );
    assert_eq!(boxes(&document, &pages[0], "cell"), [(50.0, 40.0, 50.0, 10.0)]);
}

#[test]
fn breaks_the_captions_below_a_table() {
    let (document, pages) = render(
        COLUMNS,
        "<div class=mc><div style='display: table; width: 100%'>
         <div style='display: table-caption; caption-side: bottom'><div class=tail style='height: 120px'></div></div>
         <div style='display: table-row'><div style='display: table-cell; height: 30px'></div></div></div></div>",
    );
    assert_eq!(
        boxes(&document, &pages[0], "tail"),
        [(0.0, 30.0, 50.0, 70.0), (50.0, 0.0, 50.0, 50.0)]
    );
}

#[test]
fn gives_the_free_space_of_grid_areas_to_auto_margins() {
    // The item 100px tall at the end of its 200px row starts the second column.
    let (document, pages) = render(
        ".mc { columns: 2; column-gap: 0; width: 100px; height: 100px }",
        "<div class=mc><div style='display: grid; grid-template-rows: 200px'>
         <div class=end style='height: 100px; margin-top: auto'></div></div></div>",
    );
    assert_eq!(boxes(&document, &pages[0], "end"), [(50.0, 0.0, 50.0, 100.0)]);
    // Both auto: the item is centered, not stretched.
    let (document, pages) = render(
        "",
        "<div style='display: grid; grid-template-rows: 200px'><div class=middle style='margin: auto 0'><p>x</p></div></div>",
    );
    assert_eq!(boxes(&document, &pages[0], "middle"), [(0.0, 90.0, 400.0, 20.0)]);
}

//! Flex, grid and table containers broken across columns as Chrome breaks them: the flex lines
//! of a wrapping column (each a parallel flow that starts at the top of the column), the break
//! values their items pass on to the container, monolithic content that takes a container's
//! fragment down with it, `align-content` in wrapping rows and percentage heights inside flex
//! items. The numbers are Chrome's.

use super::{FragmentKind, render_document};
use crate::fragment::{Fragment, Page};

/// The border boxes of the elements with class `class`, in order: page (from 0), left, top,
/// width and height in the page.
fn boxes(document: &fagus_dom::Document, pages: &[Page], class: &str) -> Vec<(usize, f32, f32, f32, f32)> {
    fn walk(
        fragment: &Fragment,
        document: &fagus_dom::Document,
        class: &str,
        (page, x, y): (usize, f32, f32),
        out: &mut Vec<(usize, f32, f32, f32, f32)>,
    ) {
        let x = x + fragment.rect.x.to_px();
        let y = y + fragment.rect.y.to_px();
        if let FragmentKind::Box { node: Some(node), .. } = &fragment.kind
            && document
                .element(*node)
                .is_some_and(|data| data.classes.iter().any(|name| &**name == class))
        {
            out.push((
                page,
                x,
                y,
                fragment.rect.width.to_px(),
                fragment.rect.height.to_px(),
            ));
        }
        for child in &fragment.children {
            walk(child, document, class, (page, x, y), out);
        }
    }
    let mut out = Vec::new();
    for (index, page) in pages.iter().enumerate() {
        for fragment in &page.fragments {
            walk(fragment, document, class, (index, 0.0, 0.0), &mut out);
        }
    }
    out
}

/// Renders `body` on pages 400px × 300px without margins, with `css`.
fn render(css: &str, body: &str) -> (fagus_dom::Document, Vec<Page>) {
    render_document(&format!(
        "<!doctype html><style>@page {{ size: 400px 300px; margin: 0 }} body {{ margin: 0 }}
         .mc {{ columns: 2; column-gap: 0; column-fill: auto; width: 100px; height: 100px }} {css}</style>
         {body}"
    ))
}

#[test]
fn the_last_items_of_the_lines_of_a_wrapping_column_pass_their_break_after_on() {
    // The second item ends the first flex line and avoids the break after it: the container
    // passes that on, and the break before it is taken instead, as in Chrome.
    let (document, pages) = render(
        ".flex { display: flex; flex-flow: column wrap; height: 50px }",
        "<div class=mc><div style='height: 50px'></div><div class=flex>
           <div style='height: 25px'></div><div style='height: 25px; break-after: avoid'></div>
           <div style='height: 50px'></div></div><div class=after style='height: 50px'></div></div>",
    );
    let flex = boxes(&document, &pages, "flex");
    assert_eq!(flex.len(), 1);
    assert_eq!((flex[0].1, flex[0].2), (50.0, 0.0));
    assert_eq!(boxes(&document, &pages, "after")[0].2, 50.0);
}

#[test]
fn items_of_a_wrapping_column_resolve_percentage_heights_against_it() {
    // The first item is as tall as the container and fills a line of its own; the second, as
    // tall as its minimum height after flexing, gives its child a definite height to take a
    // percentage of.
    let (document, pages) = render(
        ".flex { display: flex; flex-flow: column wrap; height: 200px }",
        "<div class=mc><div class=flex><div class=a style='height: 100%'></div>
           <div style='min-height: 100px'><div class=b style='height: 200%'></div></div></div></div>",
    );
    let a = boxes(&document, &pages, "a");
    assert_eq!(a.iter().map(|b| b.4).sum::<f32>(), 200.0);
    assert_eq!((a[0].1, a[0].3), (0.0, 25.0));
    let b = boxes(&document, &pages, "b");
    assert_eq!(b.iter().map(|b| b.4).sum::<f32>(), 200.0);
    assert_eq!(b[0].1, 25.0);
}

#[test]
fn each_line_of_a_wrapping_column_starts_at_the_top_of_the_column() {
    // The first item of the second line breaks between its two lines, which leaves fewer lines
    // than `orphans` asks for: at the top of the column it may, as in Chrome; it does not move
    // to the next column (nor make columns without end).
    let (document, pages) = render(
        ".flex { display: flex; flex-flow: column wrap; height: 200px } .flex > div { width: 25px; line-height: 0 }
         span { display: inline-block; width: 25px }",
        "<div class=mc><div class=flex>
           <div style='height: 110px'><span style='height: 80px'></span><span style='height: 30px'></span></div>
           <div style='height: 90px'></div>
           <div class=c style='height: 110px'><span style='height: 30px'></span><span style='height: 80px'></span></div>
           <div style='height: 90px'></div></div></div>",
    );
    assert_eq!(pages.len(), 1);
    let c = boxes(&document, &pages, "c");
    assert_eq!((c[0].1, c[0].2, c[0].4), (25.0, 0.0, 100.0));
    assert_eq!((c[1].1, c[1].2), (75.0, 0.0));
}

#[test]
fn items_avoiding_breaks_inside_break_at_the_top_of_a_column_line() {
    // Each first item is taller than the column: it breaks inside all the same, and the item
    // after it moves to the next column.
    let (document, pages) = render(
        ".mc { columns: 5; width: 100px } .flex { display: flex; flex-flow: column wrap; height: 500px }
         .flex > div { width: 10px; break-inside: avoid }",
        "<div class=mc><div class=flex><div class=a style='height: 250px'></div>
           <div class=b style='height: 200px'></div><div style='height: 120px'></div>
           <div style='height: 180px'></div><div style='height: 100px'></div></div></div>",
    );
    let a = boxes(&document, &pages, "a");
    assert_eq!(
        a.iter().map(|b| (b.1, b.4)).collect::<Vec<_>>(),
        [(0.0, 100.0), (20.0, 100.0), (40.0, 50.0)]
    );
    let b = boxes(&document, &pages, "b");
    assert_eq!((b[0].1, b[0].2), (60.0, 0.0));
}

#[test]
fn a_wrapping_column_reverse_fills_its_lines_in_order_from_the_bottom() {
    let (document, pages) = render(
        ".flex { display: flex; flex-flow: column-reverse wrap; height: 200px } .flex > div { height: 50px; width: 25px }",
        "<div class=mc><div class=flex><div class=a></div><div></div><div></div><div class=d></div>
           <div class=e></div><div></div><div></div><div></div></div></div>",
    );
    let place = |class| {
        let found = boxes(&document, &pages, class);
        (found[0].1, found[0].2)
    };
    // The first item at the bottom of the first line, in the second column.
    assert_eq!(place("a"), (50.0, 50.0));
    assert_eq!(place("d"), (0.0, 0.0));
    assert_eq!(place("e"), (75.0, 50.0));
}

#[test]
fn monolithic_content_takes_flex_and_grid_containers_down_with_it() {
    // The contained boxes are taller than the 60px columns: the containers' fragments end
    // where they do, and the rest of the container goes on below it in the next column.
    let (document, pages) = render(
        ".mc { height: 60px } .c > div { contain: size; height: 100px }
         .flex { display: flex; flex-flow: column; height: 200px } .grid { display: grid }",
        "<div class=mc><div class='c flex'><div></div><div></div></div></div>
         <div class=mc><div class='c grid'><div></div><div></div></div></div>",
    );
    for class in ["flex", "grid"] {
        let found = boxes(&document, &pages, class);
        // (Parts without height follow where the content overflows the container.)
        let heights: Vec<f32> = found.iter().map(|b| b.4).filter(|&height| height > 0.0).collect();
        assert_eq!(heights, [100.0, 100.0], "{class}");
        assert_eq!(found[1].1, 50.0, "{class}");
    }
}

#[test]
fn table_rows_end_with_monolithic_content_taller_than_the_column() {
    let (document, pages) = render(
        ".mc { columns: 1; height: 60px } .table { display: table; width: 100% }",
        "<div class=mc><div class=table><div style='contain: size; height: 100px'></div></div></div>",
    );
    let table = boxes(&document, &pages, "table");
    assert_eq!(table.iter().map(|b| b.4).collect::<Vec<_>>(), [100.0]);
}

#[test]
fn a_box_whose_top_border_does_not_fit_moves_to_the_next_page() {
    let (document, pages) = render(
        "",
        "<div style='height: 295px'></div><div class=box style='border: 10px solid; height: 80px'></div>",
    );
    let found = boxes(&document, &pages, "box");
    assert_eq!(found.len(), 1);
    assert_eq!((found[0].0, found[0].2, found[0].4), (1, 0.0, 100.0));
}

#[test]
fn wrapping_rows_share_the_height_of_the_container_by_align_content() {
    let (document, pages) = render(
        ".flex { display: flex; flex-wrap: wrap; width: 100px; height: 200px } .flex > div { width: 100px }
         .center { align-content: center; height: 100px } .center > div { height: 20px }",
        "<div class=flex><div class=a></div><div class=b></div></div>
         <div class='flex center'><div class=c></div><div class=d></div></div>",
    );
    let get = |class| boxes(&document, &pages, class)[0];
    // Stretched by default: each line takes half of the height, its items with it.
    assert_eq!((get("a").2, get("a").4), (0.0, 100.0));
    assert_eq!((get("b").2, get("b").4), (100.0, 100.0));
    // Centered: the lines move down by half of the room they leave.
    assert_eq!((get("c").2, get("d").2), (230.0, 250.0));
}

#[test]
fn percentage_heights_in_flex_items_refer_to_their_flexed_or_stretched_height() {
    let (document, pages) = render(
        ".col { display: flex; flex-direction: column; height: 200px } .row { display: flex }",
        "<div class=col><div style='min-height: 100px'><div class=a style='height: 50%'></div></div></div>
         <div class=row><div style='width: 100px'><div class=b style='height: 50%'></div></div>
           <div style='width: 100px; height: 120px'></div></div>",
    );
    assert_eq!(boxes(&document, &pages, "a")[0].4, 50.0);
    assert_eq!(boxes(&document, &pages, "b")[0].4, 60.0);
}

#[test]
fn flex_items_with_percentage_heights_inside_do_not_grow_where_they_break() {
    // The item is stretched to the container's minimum height; its child, twice as tall,
    // overflows it rather than making it grow (Blink's
    // `MinBlockSizeShouldEncompassIntrinsicSize`).
    let (document, pages) = render(
        "",
        "<div class=mc><div style='display: flex; min-height: 200px'>
           <div class=item style='flex: 1'><div style='height: 200%'></div></div></div></div>",
    );
    let item = boxes(&document, &pages, "item");
    let heights: Vec<f32> = item.iter().map(|b| b.4).filter(|&height| height > 0.0).collect();
    assert_eq!(heights, [100.0, 100.0]);
}

#[test]
fn items_stretched_in_a_broken_flex_row_keep_their_height_for_their_own_items() {
    // The row is 200px tall and breaks 100px into it: the column item's last child keeps its
    // auto margin and the grid its `1fr` row on the next page, as in Chrome.
    let (document, pages) = render(
        ".row { display: flex; height: 200px } .row > div { width: 100px }
         .col { display: flex; flex-direction: column } .grid { display: grid; grid-template-rows: auto 1fr auto }
         .col div, .grid div:first-child, .b { height: 20px }",
        "<div style='height: 200px'></div><div class=row>
           <div class=col><div></div><div class=a style='margin-top: auto'></div></div>
           <div class=grid><div></div><div></div><div class=b></div></div></div>",
    );
    for class in ["a", "b"] {
        let found = boxes(&document, &pages, class);
        assert_eq!((found[0].0, found[0].2), (1, 80.0), "{class}");
    }
}

#[test]
fn breaks_flex_and_grid_items_at_forced_breaks_inside_them() {
    // Chrome breaks an item where a forced break inside it falls, in a row flex container and
    // a grid; the next flex line follows the rest of the item on the next page.
    for container in ["display: flex", "display: flex; flex-wrap: wrap", "display: grid"] {
        let (document, pages) = render_document(&format!(
            "<!doctype html><style>@page {{ size: 400px 300px; margin: 20px }}
             body {{ margin: 0; font: 16px/20px Inter }} p {{ margin: 0 }}</style>
             <div style='{container}'><div style='width: 100%'><p>y</p>
             <p class=x style='break-before: page'>x</p><p>z</p></div>
             <div class=next>next</div></div><p class=after>after</p>"
        ));
        assert_eq!(pages.len(), 2, "{container}");
        assert_eq!(boxes(&document, &pages, "x")[0].0, 1, "{container}");
        let next = boxes(&document, &pages, "next")[0];
        if container != "display: flex" {
            assert_eq!((next.0, next.2), (1, 60.0), "{container}");
        }
    }
}

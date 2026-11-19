//! Positioned boxes and clipped or clamped boxes in cases that documents meet rarely: roots that
//! are absolutely positioned, percentages of the offsets of table parts, sticky boxes outside
//! scroll containers, out-of-flow boxes at the ends of lines and in inline boxes that blocks
//! split. The numbers are Chrome's (157), measured with the same documents.

use super::{find, render_document};

/// Renders `body` on pages 400px × 300px without margins.
fn render(css: &str, body: &str) -> (fagus_dom::Document, Vec<crate::fragment::Page>) {
    render_document(&format!(
        "<!DOCTYPE html><style>@page {{ size: 400px 300px; margin: 0 }} body {{ margin: 0; font: 16px/20px Inter }} {css}</style>{body}"
    ))
}

#[test]
fn positioned_roots_fill_the_height_their_insets_leave() {
    // A root positioned between `top` and `bottom` is as tall as they leave of the page area,
    // and a flex root stretches its items over that height (WPT css-position
    // position-absolute-root-element-flex).
    for display in ["block", "flex"] {
        let (document, pages) = render_document(&format!(
            "<!DOCTYPE html><html class=r style='display: {display}; position: absolute; left: 10px; right: 20px; \
               top: 30px; bottom: 40px; border: 5px solid; box-sizing: border-box'>
             <style>@page {{ size: 400px 300px; margin: 0 }}</style>
             <body class=b style='margin: 8px'>Text</body></html>"
        ));
        let (x, y, root) = find(&pages[0], &document, "r").expect("root");
        assert_eq!((x, y), (10.0, 30.0), "{display}");
        assert_eq!(
            (root.rect.width.to_px(), root.rect.height.to_px()),
            (370.0, 230.0),
            "{display}"
        );
        let (_, _, body) = find(&pages[0], &document, "b").expect("body");
        let height = if display == "flex" { 204.0 } else { 20.0 };
        assert_eq!(body.rect.height.to_px(), height, "{display}");
    }
}

#[test]
fn table_parts_resolve_percentage_offsets_against_specified_heights() {
    // `top: 100%` of a row is the height its row group specifies, of a row group the table's,
    // of a cell its row's; without one it is `auto` (WPT css-position position-relative-008 …
    // -013).
    let cases = [
        (
            "<tbody style='height: 10px'><tr style='position: relative; top: 100%'>",
            10.0,
        ),
        ("<tbody><tr style='position: relative; top: 100%'>", 0.0),
        ("<tbody style='position: relative; top: 100%'><tr>", 10.0),
    ];
    for (parts, y) in cases {
        let height = if parts.starts_with("<tbody style='p") {
            "height: 10px"
        } else {
            ""
        };
        let (document, pages) = render(
            "table { border-spacing: 0 } td { padding: 0 } .g { width: 50px; height: 30px }",
            &format!("<table style='{height}'>{parts}<td><div class=g></div></td></tr></tbody></table>"),
        );
        let (_, top, _) = find(&pages[0], &document, "g").expect("cell content");
        assert_eq!(top, y, "{parts}");
    }
    let (document, pages) = render(
        "table { border-spacing: 0 } td { padding: 0 } .g { width: 50px; height: 30px }",
        "<table><tr style='height: 10px'><td style='position: relative; top: 100%'><div class=g></div></td></tr></table>",
    );
    let (_, top, _) = find(&pages[0], &document, "g").expect("cell content");
    assert_eq!(top, 10.0);
}

#[test]
fn sticky_boxes_stick_in_the_window_and_in_fixed_boxes() {
    // Outside scroll containers, the window Chrome prints from (380px tall) is the viewport of
    // the flow, at the start of the first page; that of a fixed box's content is the page area.
    let (document, pages) = render(
        ".t { position: sticky; top: 30px; height: 20px; width: 50px }
         .s { position: sticky; bottom: 0; height: 40px }",
        "<div style='height: 500px'><div class=t></div></div>
         <div style='position: fixed; top: 0; left: 200px; width: 100px; height: 400px'>
           <div style='height: 320px'></div><div class=s></div></div>",
    );
    let (_, top, _) = find(&pages[0], &document, "t").expect("sticky box in the flow");
    assert_eq!(top, 30.0);
    let (_, top, _) = find(&pages[0], &document, "s").expect("sticky box in a fixed box");
    assert_eq!(top, 260.0);
    // Below the window, a sticky box keeps its place.
    let (document, pages) = render(
        ".s { position: sticky; bottom: 50px; height: 40px }",
        "<div style='height: 250px'><div style='height: 180px'></div><div class=s></div></div>",
    );
    let (_, top, _) = find(&pages[0], &document, "s").expect("sticky box");
    assert_eq!(top, 180.0);
}

#[test]
fn sticky_boxes_stick_in_the_window_in_the_flow_of_pages() {
    // A sticky box on a later page moves in the flow of pages laid end to end, where the window
    // lies at the start of the first page, within its containing block whole: from 600px down
    // that flow (the top of the third page) to 380px less its `bottom` and height, 310px up,
    // across the end of the first page (page4 probes st8, st9; Chrome 157 on Linux).
    let (document, pages) = render(
        ".c { height: 700px; width: 200px } .s { position: sticky; bottom: 50px; height: 40px }",
        "<div class=c><div style='height: 600px'></div><div class=s></div></div>",
    );
    assert_eq!(pages.len(), 3);
    let (_, top, _) = find(&pages[2], &document, "s").expect("sticky box");
    assert_eq!(top, -310.0);
    // Chrome keeps the offsets of the screen it loads the document on: where rules for the
    // screen only lay the document out otherwise there, the box keeps its place (corpus
    // report-ledger-sticky, whose table scrolls on the screen).
    let (document, pages) = render(
        ".c { height: 700px; width: 200px } .s { position: sticky; bottom: 50px; height: 40px }
         @media screen { .c { height: 100px; overflow: auto } }",
        "<div class=c><div style='height: 600px'></div><div class=s></div></div>",
    );
    let (_, top, _) = find(&pages[2], &document, "s").expect("sticky box, screen rules");
    assert_eq!(top, 0.0);
    // A sticky box in an absolutely positioned box has the window as its viewport too (page4
    // probe st3).
    let (document, pages) = render(
        ".t { position: sticky; top: 30px; height: 20px; width: 50px }",
        "<div style='position: absolute; top: 0; width: 200px; height: 200px'><div class=t></div></div>",
    );
    let (_, top, _) = find(&pages[0], &document, "t").expect("sticky box in an absolute box");
    assert_eq!(top, 30.0);
}

#[test]
fn maximum_heights_cap_stretched_absolute_boxes() {
    // A box between `top: 0` and `bottom: 0` is no taller than its `max-height`, a content-based
    // one its content (WPT css-position position-absolute-fit-content).
    for (max, height) in [("50px", 50.0), ("fit-content", 100.0), ("none", 200.0)] {
        let (document, pages) = render(
            ".c { position: relative; width: 100px; height: 200px }
             .a { position: absolute; inset: 0 }",
            &format!(
                "<div class=c><div class=a style='max-height: {max}'><div style='height: 100px'></div></div></div>"
            ),
        );
        let (_, _, fragment) = find(&pages[0], &document, "a").expect("positioned box");
        assert_eq!(fragment.rect.height.to_px(), height, "{max}");
    }
}

#[test]
fn out_of_flow_boxes_after_a_space_stay_at_the_end_of_the_line() {
    // A line may break after such a box where it may break before it: after the space before
    // it, or after a space that collapsed to nothing in an inline box opened before it; the
    // box then stands where the line ends, before the space removed there (Blink's
    // `HandleOutOfFlowPositioned`). Directly after the start of an inline box, it goes on to
    // the next line with the box.
    let css = "div { width: 60px } .a { position: absolute; width: 10px; height: 10px }";
    let (document, pages) = render(css, "<div>Wwww <i class=a></i>Wwww</div>");
    let (_, y, _) = find(&pages[0], &document, "a").expect("after a space");
    assert_eq!(y, 0.0);
    let (document, pages) = render(css, "<div>Wwww <span> <i class=a></i>Wwww</span></div>");
    let (_, y, _) = find(&pages[0], &document, "a").expect("after a collapsed space");
    assert_eq!(y, 0.0);
    let (document, pages) = render(css, "<div>Wwww <span><i class=a></i>Wwww</span></div>");
    let (_, y, _) = find(&pages[0], &document, "a").expect("after an inline box's start");
    assert_eq!(y, 20.0);
}

#[test]
fn inline_boxes_split_by_blocks_contain_from_their_first_line() {
    // The positioned inline box starts on the line before the block inside it: its containing
    // block starts there (WPT css-position position-absolute-in-inline-003).
    let (document, pages) = render(
        ".a { position: absolute; top: 0; width: 10px; height: 10px }",
        "<div style='width: 100px'><div style='height: 60px'></div>B
           <span style='position: relative'> <div></div> AA <div class=a></div></span></div>",
    );
    let (_, y, _) = find(&pages[0], &document, "a").expect("positioned box");
    assert!((y - 60.0).abs() < 1.0, "{y}");
}

#[test]
fn inline_boxes_hang_from_the_top_of_lines_without_content() {
    // Empty inline boxes on a line of nothing else start at its top, and so does the containing
    // block of a positioned one (WPT css-position nested-inline-abspos-child).
    let (document, pages) = render(
        ".a { position: absolute; top: 0; width: 10px; height: 10px }",
        "<div style='height: 40px'></div><div><span><span><span style='position: relative'>
           <div class=a></div></span></span></span></div>",
    );
    let (_, y, _) = find(&pages[0], &document, "a").expect("positioned box");
    assert!((y - 40.0).abs() < 1.0, "{y}");
}

#[test]
fn clamped_boxes_of_a_definite_height_keep_it() {
    // WPT css-overflow webkit-line-clamp-040: the box shows two lines but stays 80px tall.
    let (document, pages) = render(
        ".c { display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 2;
              font: 16px/32px Inter; white-space: pre }",
        "<div class=c style='height: 5em'>1\n2\n3\n4</div><div class=c>1\n2\n3\n4</div>",
    );
    let (_, _, sized) = find(&pages[0], &document, "c").expect("clamped box");
    assert_eq!(sized.rect.height.to_px(), 80.0);
    let heights: Vec<f32> = pages[0].fragments[0]
        .children
        .iter()
        .flat_map(|body| body.children.iter())
        .map(|child| child.rect.height.to_px())
        .collect();
    assert_eq!(heights, [80.0, 64.0]);
}

#[test]
fn clipping_boxes_make_no_formatting_context() {
    // `overflow: clip` keeps a box in the formatting context around it: it lies beside the
    // float, its lines beside it, and the margin of its first child collapses through it.
    let (document, pages) = render(
        ".f { float: left; width: 100px; height: 50px }",
        "<div class=f></div><div class=c style='overflow: clip'>clip</div>",
    );
    let (x, _, clipped) = find(&pages[0], &document, "c").expect("clipping box");
    assert_eq!((x, clipped.rect.width.to_px()), (0.0, 400.0));
    let (document, pages) = render(
        "",
        "<div style='height: 10px'></div>
         <div class=m style='overflow: clip; margin-top: 20px'><div style='margin-top: 20px'>x</div></div>",
    );
    let (_, y, margin) = find(&pages[0], &document, "m").expect("box with a margin");
    assert_eq!((y, margin.rect.height.to_px()), (30.0, 20.0));
}

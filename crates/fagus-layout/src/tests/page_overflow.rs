//! Content that overflows its page, and the last gaps in breaking content across pages, as
//! Chrome prints them: monolithic content taller than a page (shown on the pages after it, the
//! content after it pushed below it), boxes that only overflow, flex containers that cross the
//! end of a page, and columns that continue on a page. The numbers are Chrome's, measured with
//! the same font.

use super::render_document;
use crate::fragment::{Fragment, FragmentKind, Page};

/// Renders `body` on pages 400px × 300px without margins.
fn render(css: &str, body: &str) -> Vec<Page> {
    render_document(&format!(
        "<style>@page {{ size: 400px 300px; margin: 0 }} body {{ margin: 0; font: 16px/20px Inter }}
         p {{ margin: 0 }} {css}</style>{body}"
    ))
    .1
}

/// Lines `x01` to `x{count}`, one below the other.
fn numbered(count: usize) -> String {
    (1..=count)
        .map(|line| format!("x{line:02}"))
        .collect::<Vec<_>>()
        .join("<br>")
}

/// The first page (from 0) with the text `text`, and where the line with it lies on that page.
fn place(pages: &[Page], text: &str) -> Option<(usize, f32, f32)> {
    fn walk(fragment: &Fragment, text: &str, origin: (f32, f32)) -> Option<(f32, f32)> {
        if let FragmentKind::Text(run) = &fragment.kind
            && run.text.contains(text)
        {
            return Some((
                origin.0 + fragment.rect.x.to_px(),
                origin.1 + fragment.rect.y.to_px(),
            ));
        }
        let origin = (
            origin.0 + fragment.rect.x.to_px(),
            origin.1 + fragment.rect.y.to_px(),
        );
        fragment
            .children
            .iter()
            .find_map(|child| walk(child, text, origin))
    }
    pages.iter().enumerate().find_map(|(index, page)| {
        let (x, y) = walk(page.fragments.first()?, text, (0.0, 0.0))?;
        Some((index, x, y))
    })
}

/// The pages whose content reaches onto page `page`.
fn carried(pages: &[Page], page: usize) -> Vec<usize> {
    pages[page].carried.iter().map(|carried| carried.page).collect()
}

#[test]
fn pushes_the_content_after_monolithic_content_taller_than_the_page() {
    // The box moves to the second page and reaches 400px below its end: the third page shows
    // its middle, the fourth its end, and the paragraph after it follows 100px down.
    let pages = render(
        "",
        "<p>before</p><div style='contain: size; height: 700px'></div><p>after</p>",
    );
    assert_eq!(pages.len(), 4);
    assert_eq!(place(&pages, "after"), Some((3, 0.0, 100.0)));
    assert_eq!(carried(&pages, 1), [0_usize; 0]);
    assert_eq!(carried(&pages, 2), [1]);
    assert_eq!(carried(&pages, 3), [1]);
    let shown = pages[3].carried[0];
    assert_eq!(
        (shown.shift.1.to_px(), shown.from.to_px(), shown.to.to_px()),
        (-600.0, 600.0, 900.0)
    );
}

#[test]
fn pushes_the_content_after_a_line_taller_than_the_page() {
    let pages = render(
        "",
        "<p>before</p><div style='display: inline-block; vertical-align: top; width: 50px; height: 700px'></div>
         <p>after</p>",
    );
    assert_eq!(pages.len(), 4);
    assert_eq!(place(&pages, "after"), Some((3, 0.0, 100.0)));
}

#[test]
fn keeps_monolithic_content_that_overflows_a_box_of_a_fixed_height() {
    // The box fits on the first page and keeps its content there, which overflows it, the page
    // and the two pages added for it; the paragraph after the box stays below it.
    let pages = render(
        "",
        "<p>before</p><div style='height: 50px'><div style='contain: size; height: 700px'></div></div>
         <p>after</p>",
    );
    assert_eq!(pages.len(), 3);
    assert_eq!(place(&pages, "after"), Some((0, 0.0, 70.0)));
    assert_eq!(carried(&pages, 1), [0]);
    assert_eq!(carried(&pages, 2), [0]);
}

#[test]
fn adds_pages_for_content_moved_below_the_last_page() {
    let pages = render(
        "",
        "<p>before</p><p style='position: relative; top: 600px'>moved</p><p>after</p>",
    );
    assert_eq!(pages.len(), 3);
    assert_eq!(place(&pages, "moved"), Some((0, 0.0, 620.0)));
    assert_eq!(place(&pages, "after"), Some((0, 0.0, 40.0)));
    assert_eq!(carried(&pages, 1), [0_usize; 0]);
    assert_eq!(carried(&pages, 2), [0]);
}

#[test]
fn breaks_boxes_that_avoid_breaks_inside_but_are_taller_than_a_page() {
    // The box moves to the next page and breaks between its lines there.
    let pages = render(
        "",
        &format!(
            "<p>before</p><div style='break-inside: avoid; padding-top: 7px'>{}</div>",
            numbered(30)
        ),
    );
    assert_eq!(pages.len(), 4);
    assert_eq!(place(&pages, "x01"), Some((1, 0.0, 7.0)));
    assert_eq!(place(&pages, "x14"), Some((1, 0.0, 267.0)));
    assert_eq!(place(&pages, "x15"), Some((2, 0.0, 0.0)));
}

#[test]
fn breaks_flex_containers_that_cross_the_end_of_the_page() {
    // The item fits on the page, the container does not: it breaks below the item.
    let pages = render(
        "",
        "<div style='height: 210px'></div><div style='display: flex; height: 100px'><p>top</p></div>
         <p>after</p>",
    );
    assert_eq!(place(&pages, "top"), Some((0, 0.0, 210.0)));
    assert_eq!(place(&pages, "after"), Some((1, 0.0, 10.0)));
    // An item that does not fit goes on at the top of the next page, the container with it.
    let pages = render(
        "",
        "<div style='height: 210px'></div>
         <div style='display: flex; height: 100px; align-items: flex-end'><p>end</p></div>",
    );
    assert_eq!(pages.len(), 2);
    assert_eq!(place(&pages, "end"), Some((1, 0.0, 0.0)));
}

#[test]
fn keeps_orphans_in_the_columns_of_a_continued_paragraph() {
    // The paragraph's last three lines are balanced on the second page: two in the first column
    // (as `orphans` asks), one in the second.
    let pages = render(
        "",
        "<div style='columns: 2; column-gap: 0'>
           <div style='column-span: all; height: 260px'>spanner</div>
           <div style='break-after: avoid'>Heading</div>
           <p>aa bb cc dd ee ff gg hh ii jj kk ll mm nn oo pp qq rr ss tt uu vv ww xx yy zz ab bc cd
              de ef fg gh hi ij jk kl lm mn no op pq qr rs st tu uv vw wx xy</p></div>",
    );
    assert_eq!(pages.len(), 2);
    assert_eq!(place(&pages, "bc"), Some((1, 0.0, 0.0)));
    assert_eq!(place(&pages, "lm"), Some((1, 0.0, 20.0)));
    assert_eq!(place(&pages, "uv"), Some((1, 200.0, 0.0)));
}

#[test]
fn keeps_the_margins_below_a_spanner_that_ends_the_page() {
    // The break below the spanner is the spanner's: the heading on the next page keeps its top
    // margin.
    let pages = render(
        "h2 { column-span: all; margin: 0; font: inherit; height: 260px }
         h3 { margin: 10px 0 0; font: inherit }",
        "<div style='columns: 2; column-gap: 0'><p>aa bb cc dd ee ff gg hh ii jj kk</p><h2>Spanner</h2>
           <h3>Head</h3><p>ll mm nn oo pp qq rr ss tt uu vv ww xx yy zz</p></div>",
    );
    assert_eq!(pages.len(), 2);
    assert_eq!(place(&pages, "Head"), Some((1, 0.0, 10.0)));
}

#[test]
fn moves_monolithic_content_below_the_top_padding_to_the_next_page() {
    // At the top of the page, but below its parent's top padding: the box moves on, the padding
    // stays (Blink breaks before content that does not fit unless it starts the page).
    let pages = render(
        "",
        "<div style='padding-top: 10px'><div class=box style='contain: size; height: 295px'></div></div>
         <p>after</p>",
    );
    assert_eq!(pages.len(), 3);
    assert_eq!(place(&pages, "after"), Some((2, 0.0, 0.0)));
    // Where nothing lies above it, it stays and overflows the page.
    let pages = render(
        "",
        "<div><div style='contain: size; height: 305px'></div></div><p>after</p>",
    );
    assert_eq!(place(&pages, "after"), Some((1, 0.0, 5.0)));
}

#[test]
fn moves_a_first_line_below_the_body_margin_to_the_next_page() {
    // The line taller than the page less the body's margin leaves the first page empty.
    let pages = render(
        "body { margin: 8px }",
        "<p style='font-size: 200px; line-height: 295px'>X</p><p>after</p>",
    );
    assert_eq!(pages.len(), 3);
    assert_eq!(place(&pages, "X").map(|(page, ..)| page), Some(1));
    assert_eq!(place(&pages, "after").map(|(page, ..)| page), Some(2));
}

#[test]
fn keeps_monolithic_content_at_the_very_top_below_empty_boxes() {
    let pages = render(
        "",
        "<div></div><div style='contain: size; height: 305px'></div><p>after</p>",
    );
    assert_eq!(pages.len(), 2);
    assert_eq!(place(&pages, "after"), Some((1, 0.0, 5.0)));
}

#[test]
fn moves_monolithic_content_below_a_margin_kept_after_a_forced_break() {
    // The margin kept after the forced break pushes the box to the page after, where the
    // margin is truncated.
    let pages = render(
        "",
        "<p>x</p><div style='break-before: page; margin-top: 10px; contain: size; height: 295px'></div>
         <p>after</p>",
    );
    assert_eq!(pages.len(), 4);
    assert_eq!(place(&pages, "after"), Some((3, 0.0, 0.0)));
}

#[test]
fn keeps_monolithic_content_below_padding_repeated_on_every_page() {
    // Cloned top padding is above the box on every page: it stays rather than moving on forever.
    let pages = render(
        "",
        "<div style='box-decoration-break: clone; padding-top: 10px'><p>x</p>
         <div style='contain: size; height: 295px'></div><p>after</p></div>",
    );
    assert_eq!(pages.len(), 3);
    assert_eq!(place(&pages, "after").map(|(page, ..)| page), Some(2));
}

/// A table whose first cell holds monolithic content `height` pixels tall and whose second has
/// a bottom padding of 7px, below a paragraph.
fn overflowing_table(height: u32) -> String {
    format!(
        "<p>before</p><table style='border-collapse: collapse'><tr><td style='padding: 0'>
         <div style='contain: size; width: 100px; height: {height}px'></div></td>
         <td style='padding: 0 0 7px'>cell</td></tr></table><p>after</p>"
    )
}

#[test]
fn continues_table_rows_below_monolithic_overflow() {
    // The row moves to the second page, where its first cell reaches 40px into the third: the
    // padding of the second cell goes on below that, and the paragraph after it.
    let pages = render("", &overflowing_table(340));
    assert_eq!(pages.len(), 3);
    assert_eq!(place(&pages, "after"), Some((2, 0.0, 47.0)));
}

#[test]
fn continues_table_rows_after_a_page_that_monolithic_overflow_covers() {
    // The cell reaches 500px past the second page: the third is left to it, and the row goes on
    // at the top of the fourth, over the rest of it.
    let pages = render("", &overflowing_table(800));
    assert_eq!(pages.len(), 4);
    assert_eq!(place(&pages, "after"), Some((3, 0.0, 7.0)));
}

#[test]
fn keeps_a_repeated_header_at_the_top_of_a_page_with_monolithic_overflow() {
    let pages = render(
        "",
        "<p>before</p><table style='border-collapse: collapse'><thead><tr><td>head</td></tr></thead>
         <tr><td style='padding: 0'><div style='contain: size; width: 100px; height: 340px'></div></td></tr>
         <tr><td>row</td></tr></table>",
    );
    // The overflow reaches 62px into the third page, where the header comes first over it and
    // the next row below it.
    assert_eq!(pages.len(), 3);
    assert_eq!(place(&pages, "head").map(|(page, ..)| page), Some(1));
    let head = place(&pages[2..], "head").map(|(_, _, y)| y);
    assert_eq!(head, Some(1.0));
    assert_eq!(place(&pages, "row"), Some((2, 1.0, 63.0)));
}

#[test]
fn leaves_the_next_page_to_overflow_that_ends_right_at_the_end_of_a_page() {
    // The box moves to the second page and ends with the third: the fourth is left to it too.
    let pages = render(
        "",
        "<p>before</p><div><div style='contain: size; height: 600px'></div></div><p>after</p>",
    );
    assert_eq!(pages.len(), 5);
    assert_eq!(place(&pages, "after"), Some((4, 0.0, 0.0)));
}

#[test]
fn moves_monolithic_flex_items_and_floats_below_a_margin_to_the_next_page() {
    // Below the margin, the item does not fit: it moves on with its container.
    let pages = render(
        "",
        "<div style='margin-top: 8px; display: flex'><div style='contain: size; width: 100px; height: 295px'></div></div>
         <p>after</p>",
    );
    assert_eq!(pages.len(), 3);
    assert_eq!(place(&pages, "after"), Some((2, 0.0, 0.0)));
    // A float moves on alone, the text beside it staying on the page.
    let pages = render(
        "",
        "<div style='margin-top: 8px'><div style='float: left; contain: size; width: 100px; height: 295px'></div>
         <p>beside</p></div>",
    );
    assert_eq!(pages.len(), 2);
    assert_eq!(place(&pages, "beside"), Some((0, 0.0, 8.0)));
}

#[test]
fn breaks_cells_taller_than_their_content_in_their_bottom_border() {
    // The last row fits but for the bottom half of its border: its cell, of a fixed height,
    // breaks there, as in Chrome, rather than moving its row to the next page. (Without a
    // doctype the height of a cell includes the halves of its collapsed borders: rows of 40px.)
    let pages = render(
        "td { padding: 0; border: 10px solid; height: 40px; font: 12px/16px Inter }",
        "<div style='height: 180px'></div><table style='border-collapse: collapse'><tr><td>a</td></tr>
         <tr><td>b</td></tr><tr><td>c</td></tr></table><p>after</p>",
    );
    assert_eq!(pages.len(), 2);
    assert_eq!(place(&pages, "c").map(|(page, ..)| page), Some(0));
    assert_eq!(place(&pages, "after").map(|(page, ..)| page), Some(1));
}

#[test]
fn cuts_content_off_at_the_unrounded_end_of_the_page_area() {
    // A4 with margins of 20mm leaves 971.34 pixels, laid out 972 tall: Chrome cuts the content
    // off at 971.34, so that what starts in the last pixel row of the page shows only a sliver
    // there (KaTeX's radicals at the end of a page).
    let (_, pages) = render_document("<style>@page { size: A4; margin: 20mm }</style><p>x</p>");
    let area = pages[0].area;
    assert!((area.height.to_px() - 971.34).abs() < 0.02, "{area:?}");
    assert_eq!(area.y.to_px(), 76.0);
}

#[test]
fn leaves_no_page_to_overflow_in_the_flow_that_ends_right_at_the_end_of_a_page() {
    // The box after the paragraph moves to the second page and ends with the third; the
    // paragraph after it goes on at the top of the fourth, as in Chrome 157, where the box that
    // holds the overflowing content goes on after it. So do lines taller than the page (WPT
    // css-page monolithic-overflow-024).
    let pages = render(
        "",
        "<p>before</p><div style='contain: size; height: 600px'></div><p>after</p>",
    );
    assert_eq!(pages.len(), 4);
    assert_eq!(place(&pages, "after"), Some((3, 0.0, 0.0)));
    let pages = render("body { line-height: 600px }", "one<br>two");
    assert_eq!(place(&pages, "two").map(|(page, ..)| page), Some(2));
}

#[test]
fn repeats_a_footer_at_the_end_of_a_page_that_monolithic_overflow_covers() {
    // The content reaches from the first page to 5px above the footer on the third: on the
    // second, which it covers, the footer repeats at the end of the page, as on the others
    // (WPT css-page monolithic-overflow-017; Chrome 157).
    let pages = render(
        "",
        "<div style='display: table; width: 100%'>
           <div style='display: table-footer-group; break-inside: avoid'>footer</div>
           <div style='contain: size; height: 875px'></div>PASS</div>",
    );
    assert_eq!(pages.len(), 4);
    let footer = |page: usize| place(&pages[page..=page], "footer").map(|(_, _, y)| y);
    assert_eq!(footer(1), Some(280.0));
}

#[test]
fn pushes_the_content_of_floats_and_positioned_boxes_below_their_monolithic_overflow() {
    // The box with size containment reaches 750px down from the top of the first page: the
    // text after it in the float, or in the absolutely positioned box, goes on 150px down the
    // third page, the second showing an empty part of the box (WPT css-page
    // monolithic-overflow-012 and -013; Chrome 157).
    for outer in ["float: left; width: 100%", "position: absolute; width: 100%"] {
        let pages = render(
            "",
            &format!(
                "<div style='{outer}'><div style='contain: size; width: 50px; height: 750px'></div>after</div>"
            ),
        );
        assert_eq!(pages.len(), 3, "{outer}");
        assert_eq!(place(&pages, "after"), Some((2, 0.0, 150.0)), "{outer}");
    }
}

#[test]
fn adds_no_page_for_a_last_line_that_ends_right_at_the_end_of_a_page() {
    // Two lines 600px tall on pages 300px tall: four pages, none added below the last line
    // (WPT css-page monolithic-overflow-024; Chrome 157).
    let pages = render("body { line-height: 600px }", "one<br>two");
    assert_eq!(pages.len(), 4);
}

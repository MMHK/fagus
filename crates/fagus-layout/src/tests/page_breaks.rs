//! Where long documents break as Chrome prints them: the bottom border and padding of boxes
//! whose height does not follow their content (stretched flex and grid items, items of column
//! flex containers, boxes of a fixed or minimum height), `orphans` and `widows` where the bottom
//! border and padding push lines on, rows joined by a cell that spans them, and boxes positioned
//! in the initial containing block. The expectations are Chrome's, measured with the same font.

use super::{contains_text, find, render_document};
use crate::fragment::Page;

/// Renders `body` on pages 400px × 300px without margins, below a spacer `above` pixels tall.
fn render(css: &str, above: f32, body: &str) -> (fagus_dom::Document, Vec<Page>) {
    render_document(&format!(
        "<style>@page {{ size: 400px 300px; margin: 0 }} body {{ margin: 0; font: 16px/20px Inter }}
         p {{ margin: 0 }} {css}</style><div style='height: {above}px'></div>{body}"
    ))
}

/// Lines `x01` to `x{count}`, one below the other.
fn numbered(count: usize) -> String {
    (1..=count)
        .map(|line| format!("x{line:02}"))
        .collect::<Vec<_>>()
        .join("<br>")
}

/// The page (from 0) whose content has the text `text`.
fn page_with(pages: &[Page], text: &str) -> Option<usize> {
    pages.iter().position(|page| {
        page.fragments
            .first()
            .is_some_and(|content| contains_text(content, text))
    })
}

/// A box with a bottom border and padding below fourteen lines in a paragraph with a bottom
/// margin of `margin`, whose last line ends at the end of the first page.
fn framed(margin: u32) -> String {
    format!(
        "<div class=item style='border: 1px solid; padding: 5px 0 15px'>
           <p style='margin-bottom: {margin}px'>{}</p></div>",
        numbered(14)
    )
}

#[test]
fn breaks_stretched_items_below_the_truncated_margin_of_their_content() {
    // The item is as tall as its line, and so as tall as its content with the margin: the
    // margin, truncated at the break, leaves room for a break before the bottom padding.
    let row = "<div style='display: flex'>{}</div>";
    let (_, pages) = render("", 14.0, &row.replace("{}", &framed(10)));
    assert_eq!(pages.len(), 2);
    assert_eq!(page_with(&pages, "x14"), Some(0));
    // Without the margin there is no such room: the box breaks before its last lines.
    let (_, pages) = render("", 14.0, &row.replace("{}", &framed(0)));
    assert_eq!(page_with(&pages, "x13"), Some(1));
    // The same for grid items, which are stretched to their rows.
    let grid = format!("<div style='display: grid'>{}<div>after</div></div>", framed(10));
    let (_, pages) = render("", 14.0, &grid);
    assert_eq!(page_with(&pages, "x14"), Some(0));
    assert_eq!(page_with(&pages, "after"), Some(1));
}

#[test]
fn breaks_items_of_column_flex_containers_below_the_truncated_margin() {
    let column = format!(
        "<div style='display: flex; flex-direction: column'>{}<div>after</div></div>",
        framed(10)
    );
    let (_, pages) = render("", 14.0, &column);
    assert_eq!(page_with(&pages, "x14"), Some(0));
    assert_eq!(page_with(&pages, "after"), Some(1));
    // In block flow, the box is as tall as its content and breaks before its last lines.
    let (_, pages) = render("", 14.0, &framed(10));
    assert_eq!(page_with(&pages, "x13"), Some(1));
}

#[test]
fn breaks_boxes_of_a_fixed_or_minimum_height_below_their_content() {
    // The content box ends where the lines do: no room for a break before the bottom padding.
    let sized =
        |style: &str| framed(10).replace("padding: 5px 0 15px'", &format!("padding: 5px 0 15px; {style}'"));
    let (_, pages) = render("", 14.0, &sized("height: 280px"));
    assert_eq!(page_with(&pages, "x13"), Some(1));
    // Taller than the lines with the truncated margin: the box breaks below them.
    let (document, pages) = render(
        "",
        14.0,
        &format!("{}<p class=after>after</p>", sized("height: 296px")),
    );
    assert_eq!(page_with(&pages, "x14"), Some(0));
    // The rest of the box (its height and bottom padding and border) comes first on the next
    // page: 14 + 1 + 5 + 296 + 15 + 1 = 332.
    let (_, y, _) = find(&pages[1], &document, "after").expect("paragraph after the box");
    assert_eq!(y, 32.0);
    let (_, pages) = render(
        "",
        4.0,
        &sized("min-height: 295px")
            .replace("15px", "5px")
            .replace("10px", "20px"),
    );
    assert_eq!(page_with(&pages, "x14"), Some(0));
}

#[test]
fn keeps_orphans_and_widows_where_the_bottom_padding_pushes_the_last_line_on() {
    // Lines of a box whose own bottom border and padding do not fit below its last line, which
    // ends at the end of the page.
    let lines = |count: usize, above: f32, style: &str| {
        render(
            "",
            above,
            &format!(
                "<div style='border: 1px solid; padding: 5px 0 15px; {style}'>{}</div>",
                numbered(count)
            ),
        )
        .1
    };
    // Two lines go on for the widows, as the orphans allow.
    let pages = lines(4, 214.0, "");
    assert_eq!(page_with(&pages, "x03"), Some(1));
    assert_eq!(page_with(&pages, "x02"), Some(0));
    // Where no break keeps both, the last line goes on alone.
    let pages = lines(3, 234.0, "");
    assert_eq!(page_with(&pages, "x02"), Some(0));
    assert_eq!(page_with(&pages, "x03"), Some(1));
    let pages = lines(4, 214.0, "widows: 3");
    assert_eq!(page_with(&pages, "x03"), Some(0));
    assert_eq!(page_with(&pages, "x04"), Some(1));
    let pages = lines(5, 194.0, "widows: 3");
    assert_eq!(page_with(&pages, "x02"), Some(0));
    assert_eq!(page_with(&pages, "x03"), Some(1));
}

#[test]
fn breaks_before_a_paragraph_rather_than_leaving_a_widow() {
    // The card's bottom margin, padding and border do not fit below the paragraph: breaking
    // inside it would leave one line, so the card breaks between its title and the paragraph.
    let card = format!(
        "<div style='border: 1px solid; padding: 11px'><div>title</div>
           <p style='margin: 13px 0'>{}</p></div>",
        numbered(3)
    );
    let (_, pages) = render("", 180.0, &card);
    assert_eq!(page_with(&pages, "title"), Some(0));
    assert_eq!(page_with(&pages, "x01"), Some(1));
    // Without the title, the break is inside the paragraph.
    let alone = card.replace("<div>title</div>", "");
    let (_, pages) = render("", 213.0, &alone);
    assert_eq!(page_with(&pages, "x02"), Some(0));
    assert_eq!(page_with(&pages, "x03"), Some(1));
}

#[test]
fn breaks_between_rows_that_a_cell_spans() {
    // The rows that the first cell joins reach half a border past the end of the page.
    let (_, pages) = render(
        "table { border-collapse: collapse; width: 100% }
         td { border: 1px solid; padding: 4px; vertical-align: top }",
        135.0,
        "<table><tr><td rowspan=5>group</td><td>r1</td></tr><tr><td>r2</td></tr>
           <tr><td>r3</td></tr><tr><td>r4<br>more</td></tr><tr><td>r5</td></tr></table>",
    );
    assert_eq!(pages.len(), 2);
    assert_eq!(page_with(&pages, "r4"), Some(0));
    assert_eq!(page_with(&pages, "r5"), Some(1));
}

#[test]
fn sizes_fixed_boxes_by_the_first_page_area() {
    let (document, pages) = render_document(&format!(
        "<style>@page {{ size: 400px 300px; margin: 20px }} @page :first {{ margin-top: 40px }}
           body {{ margin: 0; font: 16px/20px Inter }} p {{ margin: 0 }}
           .foot {{ position: fixed; bottom: 0; right: 0; height: 20px; width: 50px }}</style>
         <div class=foot></div>{}",
        "<p>line</p>".repeat(30)
    ));
    assert!(pages.len() > 1);
    // The first page area is 360px × 240px, the others 360px × 260px.
    let (x, y, _) = find(&pages[0], &document, "foot").expect("footer on the first page");
    assert_eq!((x, y), (330.0, 260.0));
    let (x, y, _) = find(&pages[1], &document, "foot").expect("footer on the second page");
    assert_eq!((x, y), (330.0, 240.0));
}

#[test]
fn places_boxes_in_the_initial_containing_block_relative_to_the_first_page() {
    // Their placeholder is on the last page; `top` places them on the first and second.
    let (document, pages) = render_document(&format!(
        "<style>@page {{ size: 400px 300px; margin: 20px }}
           body {{ margin: 0; font: 16px/20px Inter }} p {{ margin: 0 }}
           .near {{ position: absolute; top: 10px; left: 100px }}
           .far {{ position: absolute; top: 300px; left: 100px }}</style>{}
         <div class=near>near</div><div class=far>far</div>",
        "<p>line</p>".repeat(30)
    ));
    assert_eq!(pages.len(), 3);
    let (_, y, _) = find(&pages[0], &document, "near").expect("on the first page");
    assert_eq!(y, 30.0);
    // 300px from the top of the first page area of 260px: 40px down the second.
    let (_, y, _) = find(&pages[1], &document, "far").expect("on the second page");
    assert_eq!(y, 60.0);
    assert!(find(&pages[2], &document, "near").is_none());
}

#[test]
fn moves_a_spanned_row_that_breaks_only_in_its_bottom_padding() {
    // The fourth row's line fits on the first page, its bottom padding does not, and the rows
    // below it start on the next page: the row goes there with them.
    let (_, pages) = render(
        "table { border-collapse: collapse; width: 100% }
         td { border: 1px solid; padding: 4px; vertical-align: top }",
        185.0,
        "<table><tr><td rowspan=5>group</td><td>r1</td></tr><tr><td>r2</td></tr>
           <tr><td>r3</td></tr><tr><td>r4</td></tr><tr><td>r5</td></tr></table>",
    );
    assert_eq!(page_with(&pages, "r3"), Some(0));
    assert_eq!(page_with(&pages, "r4"), Some(1));
}

#[test]
fn repeats_the_footer_below_the_rows_that_a_spanning_cell_joins() {
    // The fifth row moves to the next page; the footer follows the fourth row, and the cell
    // that spans the rows ends with it.
    let (document, pages) = render(
        "table { border-collapse: collapse; width: 100% }
         td { border: 1px solid; padding: 4px; vertical-align: top }",
        150.0,
        "<table><tfoot><tr><td class=foot colspan=2>foot</td></tr></tfoot>
           <tbody><tr><td class=group rowspan=5>group</td><td>r1</td></tr><tr><td>r2</td></tr>
           <tr><td>r3</td></tr><tr><td class=fourth>r4</td></tr><tr><td>r5</td></tr></tbody></table>",
    );
    assert_eq!(page_with(&pages, "r5"), Some(1));
    let (_, fourth, row) = find(&pages[0], &document, "fourth").expect("fourth row");
    let (_, foot, _) = find(&pages[0], &document, "foot").expect("repeated footer");
    assert_eq!(foot, fourth + row.rect.height.to_px() - 1.0);
    let (_, top, group) = find(&pages[0], &document, "group").expect("spanning cell");
    assert_eq!(top + group.rect.height.to_px(), foot + 1.0);
}

#[test]
fn carries_row_gaps_across_page_ends_by_what_broken_rows_grew() {
    // The first row breaks after four lines, 16px above the end of the first page, and so grows
    // by 16px. On the second page the gap below the fifth row crosses the end of the page by
    // 5px: Chrome leaves out only what of it the grid laid out whole has below the end (none),
    // so the sixth row starts 5px down the third page.
    let rows = (2..=12)
        .map(|row| format!("<div class=r{row}>r{row}</div>"))
        .collect::<Vec<_>>()
        .concat();
    let grid = format!(
        "<div style='display: grid; row-gap: 12px'><div>{}</div>{rows}</div>",
        numbered(10)
    );
    let css = ".r2, .r3, .r4, .r5, .r6, .r7, .r8, .r9, .r10, .r11, .r12, div > div { padding-bottom: 8px; border-bottom: 1px solid }";
    let (document, pages) = render(css, 204.0, &grid);
    let (_, y, _) = find(&pages[1], &document, "r5").expect("fifth row on the second page");
    assert_eq!(y, 264.0);
    let (_, y, _) = find(&pages[2], &document, "r6").expect("sixth row on the third page");
    assert_eq!(y, 5.0);
    // Without rows that grew, the gap that crosses the end of a page is left out: the ninth row
    // ends 8px above the end of the first page, the gap below it 6px below.
    let rows = (1..=12)
        .map(|row| format!("<div class=r{row}>r{row}</div>"))
        .collect::<Vec<_>>()
        .concat();
    let (document, pages) = render(
        "",
        0.0,
        &format!("<div style='display: grid; row-gap: 14px'>{rows}</div>"),
    );
    let (_, y, _) = find(&pages[1], &document, "r10").expect("tenth row on the second page");
    assert_eq!(y, 0.0);
}

#[test]
fn counts_no_empty_line_among_the_widows() {
    // The empty inline after the last `<br>` makes a line without content or height: Chrome
    // does not count it among the widows, so two lines with text go on to the third page.
    let (_, pages) = render(
        "",
        0.0,
        &format!(
            "<div style='border: 10px solid'>{}<br><span></span></div>",
            numbered(30)
        ),
    );
    assert_eq!(pages.len(), 3);
    assert_eq!(page_with(&pages, "x29"), Some(2));
    assert_eq!(page_with(&pages, "x28"), Some(1));
}

#[test]
fn moves_rows_that_a_cell_spans_on_where_the_first_does_not_fit() {
    // The first row of a cell spanning two rows does not fit with its bottom padding and
    // border: the cells move to the next page with their rows, rather than leave the padding
    // and border of the row alone there, in both border models (Chrome's numbers).
    for (collapse, fits) in [("collapse", 241.0), ("separate", 236.0)] {
        let rows = |above: f32| {
            render(
                &format!(
                    "table {{ border-collapse: {collapse}; width: 100% }}
                     td {{ border: 1px solid; padding: 4px; vertical-align: top }}"
                ),
                above,
                "<table><tr><td>A</td><td>a1</td></tr><tr><td rowspan=2>B</td><td>b1</td></tr>
                   <tr><td>b2</td></tr><tr><td>C</td><td>c1</td></tr></table>",
            )
            .1
        };
        let pages = rows(fits);
        assert_eq!(page_with(&pages, "b1"), Some(0), "{collapse}");
        assert_eq!(page_with(&pages, "b2"), Some(1), "{collapse}");
        let pages = rows(fits + 1.0);
        assert_eq!(page_with(&pages, "a1"), Some(0), "{collapse}");
        assert_eq!(page_with(&pages, "b1"), Some(1), "{collapse}");
    }
}

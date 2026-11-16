//! Breaks that Chrome makes differently from the general rules: the baseline of inline tables
//! without cells, table cells whose bottom padding does not fit, the rows below monolithic
//! content that reaches past the end of a page (with the spacing between them), and the content
//! after such overflow in cells, column flex containers and boxes that clone their decorations.
//! The numbers are Chrome's, measured with the same font.

use std::sync::Arc;

use super::{FragmentKind, find, render_document, render_document_with};
use crate::fragment::{Fragment, Page};
use crate::{Image, ImageFormat, Images};

/// The baseline of each text run of `fragment` whose text is `text`, from the page's top.
fn baselines(fragment: &Fragment, text: &str, origin: f32, out: &mut Vec<f32>) {
    let y = origin + fragment.rect.y.to_px();
    if let FragmentKind::Text(run) = &fragment.kind
        && run.text.trim() == text
    {
        out.push(y + run.baseline.to_px());
    }
    for child in &fragment.children {
        baselines(child, text, y, out);
    }
}

/// The baseline of the first run of `text` on the first page.
fn baseline(pages: &[Page], text: &str) -> f32 {
    let mut out = Vec::new();
    for fragment in &pages[0].fragments {
        baselines(fragment, text, 0.0, &mut out);
    }
    *out.first().expect("text")
}

/// Renders `body` on pages 400px × 300px with margins of `margin` pixels, with `photo.png` a
/// square image (the documents size it).
fn render(margin: u32, css: &str, body: &str) -> (fagus_dom::Document, Vec<Page>) {
    render_document_with(
        &format!(
            "<!doctype html><style>@page {{ size: 400px 300px; margin: {margin}px }}
             body {{ margin: 0; font: 16px/20px Inter }}
             p {{ margin: 0 }} img {{ display: block; width: 200px }} {css}</style>{body}"
        ),
        |_| {
            let mut images = Images::default();
            images.insert(
                "photo.png",
                Arc::new(Image {
                    data: Arc::new(Vec::new()),
                    format: ImageFormat::Png,
                    width: 200.0,
                    height: 200.0,
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
    )
}

/// The page (from 0) of the element with class `class` and the top of its border box in the
/// page area (whose margins are `margin` pixels).
fn top_of(document: &fagus_dom::Document, pages: &[Page], class: &str, margin: f32) -> Option<(usize, f32)> {
    pages
        .iter()
        .enumerate()
        .find_map(|(index, page)| find(page, document, class).map(|(_, y, _)| (index, y - margin)))
}

#[test]
fn inline_tables_without_cells_sit_on_the_baseline_of_their_text() {
    // The table box has padding and its text goes into an anonymous cell, whose first line gives
    // the table its baseline, as in Chrome (not the bottom edge of the table).
    let (_, pages) = render_document(
        "<style>body { font: 14px Inter; margin: 0 }</style>
         <div>a <span style='display: inline-table; border: 1px solid; padding: 5px'>T</span> c</div>",
    );
    assert!((baseline(&pages, "a") - baseline(&pages, "T")).abs() < 0.01);
}

/// A table whose second row breaks on the first page: its first cell ends on the second page
/// with 30px of bottom padding that do not fit there, the other only with its own.
fn tall_row(header: &str, spacer: u32) -> String {
    let lines: Vec<String> = (1..=25).map(|line| format!("line {line}")).collect();
    format!(
        "<p style='margin: 0 0 {spacer}px'>x</p><table>{header}<tr><td>x</td><td>y</td></tr>
         <tr><td class=t>{}</td><td class=n>tall neighbour</td></tr><tr><td class=z>z</td><td>w</td></tr></table>",
        lines.join("<br>")
    )
}

const TALL_ROW_CSS: &str = "body { font: 12px/16px Inter } td { padding: 2px; border: 3px solid }
    th { font-weight: normal; border: 3px solid } .t { padding-bottom: 30px }";

#[test]
fn lays_out_a_cell_whole_where_its_bottom_padding_does_not_fit() {
    // Chrome does not break the cell before its padding: it reaches 25px into the third page,
    // below which the rest of the other cell (its padding and border) and the next row follow.
    let (document, pages) = render(10, TALL_ROW_CSS, &tall_row("", 99));
    assert_eq!(pages.len(), 3);
    assert_eq!(top_of(&document, &pages, "z", 10.0), Some((2, 32.0)));
    // With a repeated header, the rows start below the overflow or the header, whichever ends
    // lower, with the spacing below the header: below the header where the cell reaches 19px
    // into the page, below the overflow where it reaches 49px.
    let header = "<thead><tr><th>H1</th><th>H2</th></tr></thead>";
    let (document, pages) = render(10, TALL_ROW_CSS, &tall_row(header, 30));
    assert_eq!(top_of(&document, &pages, "z", 10.0), Some((2, 33.0)));
    let css = format!("{TALL_ROW_CSS} .t {{ padding-bottom: 60px }}");
    let (document, pages) = render(10, &css, &tall_row(header, 30));
    assert_eq!(top_of(&document, &pages, "z", 10.0), Some((2, 58.0)));
}

/// A table whose first row holds an image `height` pixels tall, with `cell` the style of the
/// cell beside it.
fn image_row(spacing: u32, header: bool, height: u32, cell: &str, row2: bool) -> String {
    format!(
        "<p>before one</p><table style='border-spacing: {spacing}px'>{}<tr><td style='padding: 0'>
         <img src=photo.png style='height: {height}px'></td><td style='{cell}'>cell</td></tr>{}</table>
         <p class=after>after</p>",
        if header {
            "<thead><tr><td>head</td></tr></thead>"
        } else {
            ""
        },
        if row2 {
            "<tr><td class=row2>row2</td><td>x</td></tr>"
        } else {
            ""
        },
    )
}

#[test]
fn keeps_the_spacing_below_rows_that_end_with_monolithic_overflow() {
    // The image ends 70px into the third page: the next row follows it below the spacing,
    // below a repeated header twice that (Blink lays the header over the overflow).
    let (document, pages) = render(20, "", &image_row(4, true, 300, "padding: 0", true));
    assert_eq!(top_of(&document, &pages, "row2", 20.0), Some((2, 78.0)));
    // Without the header, the image moves to the third page and ends 40px into the fourth.
    let (document, pages) = render(20, "", &image_row(4, false, 300, "padding: 0", true));
    assert_eq!(top_of(&document, &pages, "row2", 20.0), Some((3, 44.0)));
}

#[test]
fn starts_the_rows_after_overflow_that_covers_a_page_with_the_rest_of_their_cells() {
    // The image covers the third and fourth pages; on the fifth, the bottom padding of the
    // other cell comes first, and the table ends below it without the spacing.
    let (document, pages) = render(20, "", &image_row(10, false, 700, "padding: 0 0 5px", false));
    assert_eq!(pages.len(), 5);
    assert_eq!(top_of(&document, &pages, "after", 20.0), Some((4, 5.0)));
    // A row below it follows below the spacing.
    let (document, pages) = render(20, "", &image_row(10, false, 700, "padding: 0 0 5px", true));
    assert_eq!(top_of(&document, &pages, "row2", 20.0), Some((4, 15.0)));
    // With a repeated header, that rest lies under it, and the row comes below the header and
    // the spacing twice; where no row follows, no header repeats and the table ends below the
    // rest and the spacing.
    let (document, pages) = render(20, "", &image_row(10, true, 700, "padding: 0 0 5px", true));
    assert_eq!(top_of(&document, &pages, "row2", 20.0), Some((3, 42.0)));
    let (document, pages) = render(20, "", &image_row(10, true, 700, "padding: 0 0 5px", false));
    assert_eq!(top_of(&document, &pages, "after", 20.0), Some((3, 15.0)));
}

#[test]
fn continues_a_cell_below_its_monolithic_overflow() {
    // The paragraph below the image goes on below the image's end on the next page, as do the
    // padding and border of a cell beside that goes on onto a page the image covers.
    let (document, pages) = render(
        20,
        "",
        "<p>before one</p><table style='border-collapse: collapse'><tr><td style='padding: 0'>
         <img src=photo.png style='height: 300px'><p class=below>below</p></td>
         <td style='padding: 0'>cell</td></tr></table>",
    );
    assert_eq!(top_of(&document, &pages, "below", 20.0), Some((2, 40.0)));
    let (document, pages) = render(
        20,
        "",
        "<p>before one</p><table style='border-collapse: collapse'><tr><td style='padding: 0 0 5px'>
         <img src=photo.png style='height: 700px'></td><td style='padding: 0 0 9px'>
         <div style='height: 270px'></div></td></tr></table><p class=after>after</p>",
    );
    assert_eq!(top_of(&document, &pages, "after", 20.0), Some((3, 24.0)));
}

#[test]
fn moves_the_items_of_column_flex_containers_below_monolithic_overflow() {
    let (document, pages) = render(
        20,
        "",
        "<p>before one</p><div style='display: flex; flex-direction: column'>
         <img src=photo.png style='height: 300px'><p class=row2>row2</p></div>",
    );
    assert_eq!(top_of(&document, &pages, "row2", 20.0), Some((2, 40.0)));
    // Ending with the image, the container leaves a page to it below the paragraph after it.
    let (_, pages) = render(
        20,
        "",
        "<p>before one</p><div style='display: flex; flex-direction: column'>
         <img src=photo.png style='height: 700px'></div><p>after</p>",
    );
    assert_eq!(pages.len(), 5);
}

#[test]
fn keeps_monolithic_content_in_boxes_that_clone_their_decorations() {
    // At the top of the page the image stays in the box, whose cloned bottom border reaches
    // 5px into the next page, below which the box goes on.
    let (document, pages) = render(
        20,
        "",
        "<p>before</p><div style='box-decoration-break: clone; border: 5px solid'>
         <img src=photo.png style='height: 255px'><p class=x>x</p></div>",
    );
    assert_eq!(pages.len(), 3);
    assert_eq!(top_of(&document, &pages, "x", 20.0), Some((2, 10.0)));
    // The bottom margin of the last line does not need room above cloned padding and border:
    // the box ends on the page.
    let lines: Vec<String> = (1..=4).map(|line| format!("line {line}")).collect();
    let (document, pages) = render(
        20,
        "",
        &format!(
            "<div style='height: 150px'></div><div style='box-decoration-break: clone; border: 3px solid;
             padding: 10px'><p style='margin-bottom: 9px'>{}</p></div><p class=after>after</p>",
            lines.join("<br>")
        ),
    );
    assert_eq!(top_of(&document, &pages, "after", 20.0), Some((1, 0.0)));
}

#[test]
fn ruby_annotations_belong_to_their_lines_across_pages() {
    // Ruby columns of inline blocks 75px tall with annotations 25px tall, one per line: the
    // annotations of the second line do not reach into the first, whose blocks fill it (Chrome
    // 157, probe ru1); on pages 175px tall, a line goes on to the next page where its
    // annotations under it do not fit, and a line at the top of a page stays there with the
    // annotations over it even where it does not fit (WPT css-break ruby-000, -001, -003).
    let ruby = |position: &str, page: u32, base: u32| {
        render_document(&format!(
            "<!DOCTYPE html><style>@page {{ size: 300px {page}px; margin: 0 }} body {{ margin: 0; orphans: 1; widows: 1 }}
             ruby {{ ruby-position: {position} }} ruby div {{ display: inline-block; vertical-align: top; width: 25px }}
             .m {{ height: {base}px }} .n {{ height: 25px }}</style>
             <div><ruby><div class=m></div><rt><div class=n></div></rt></ruby><br>
             <ruby><div class='m second'></div><rt><div class=n></div></rt></ruby><br></div>"
        ))
    };
    let (document, pages) = ruby("over", 400, 75);
    let (_, top, _) = find(&pages[0], &document, "second").expect("second base");
    assert_eq!(top, 125.0);
    let (document, pages) = ruby("under", 175, 75);
    assert_eq!(pages.len(), 2);
    let (_, top, _) = find(&pages[1], &document, "second").expect("second base, next page");
    assert_eq!(top, 0.0);
    let (document, pages) = ruby("over", 60, 50);
    let (_, top, _) = find(&pages[0], &document, "m").expect("first base, first page");
    assert_eq!(top, 25.0);
}

#[test]
fn forced_breaks_inside_boxes_laid_out_whole_break_nothing() {
    // The forced break after the last child of a box with size containment does not pass on to
    // the box: the block after it follows on the same page (WPT css-break
    // monolithic-content-with-forced-break-001, Chrome 157).
    let (document, pages) = render_document(
        "<!DOCTYPE html><style>@page { size: 300px 200px; margin: 0 } body { margin: 0 }</style>
         <div style='contain: size; height: 50px'><div style='height: 50px; break-after: page'></div></div>
         <div class=after style='height: 50px'></div>",
    );
    assert_eq!(pages.len(), 1);
    let (_, top, _) = find(&pages[0], &document, "after").expect("block after");
    assert_eq!(top, 50.0);
}

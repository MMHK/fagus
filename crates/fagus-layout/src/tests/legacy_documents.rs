//! Layout of legacy templates (from dompdf, wkhtmltopdf and mPDF days) as Chrome lays them out:
//! the `align` attributes that also place blocks, `<br clear>`, tables with `frame`, `rules` and
//! more percentages than fit, and the quirks of quirks and limited-quirks mode. The numbers are
//! Chrome's, measured.

use super::{find, line_boxes, render_document};

const LIMITED_QUIRKS: &str = "<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Transitional//EN\" \
     \"http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd\">";

#[test]
fn align_attributes_center_blocks() {
    // `align=center` on a cell and `<center>` center block-level children of a fixed width as
    // `-webkit-center` does; a table among them keeps its own text at the start. A child with an
    // auto margin follows its margin.
    let (document, pages) = render_document(
        "<!DOCTYPE html><style>@page { size: 600px 400px; margin: 0 } body { margin: 0; font: 10px/10px Inter }
           .b { width: 100px; height: 10px }</style>
         <table style='width: 400px; border-spacing: 0'><tr><td align=center style='padding: 0'>
           <div class='b in-cell'></div><table class=inner width=200><tr><td class=cell>x</td></tr></table>
         </td></tr></table>
         <center style='width: 400px'><div class='b centered'></div><div class='b pushed' style='margin-left: auto'></div></center>
         <div align=right style='width: 400px'><div class='b right'></div></div>",
    );
    let x = |class: &str| find(&pages[0], &document, class).expect(class).0;
    assert_eq!(x("in-cell"), 150.0);
    assert_eq!(x("inner"), 100.0);
    assert_eq!(x("centered"), 150.0);
    assert_eq!(x("pushed"), 300.0);
    assert_eq!(x("right"), 300.0);
    // The inner table's cell text is not centered: it starts after the cell's padding.
    let (cell_x, _, cell) = find(&pages[0], &document, "cell").expect("cell");
    let mut lines = Vec::new();
    line_boxes(cell, (cell_x - cell.rect.x.to_px(), 0.0), &mut lines);
    assert!(lines.iter().all(|line| line.0 <= cell_x + 2.0), "{lines:?}");
}

#[test]
fn br_clear_moves_the_next_line_below_floats() {
    // `<br clear=all>` puts the line after it below the float, and the block reaches past the
    // float even where the break ends it.
    let (document, pages) = render_document(
        "<!DOCTYPE html><style>@page { size: 400px 400px; margin: 0 } body { margin: 0; font: 10px/10px Inter }
           .f { float: left; width: 50px; height: 40px }</style>
         <div class=first><div class=f></div>a<br clear=all>b</div>
         <div class=second><div class=f></div>c<br clear=all></div><p class=next style='margin: 0'>d</p>",
    );
    let y = |class: &str| find(&pages[0], &document, class).expect(class).1;
    let (_, first_y, first) = find(&pages[0], &document, "first").expect("first");
    let mut lines = Vec::new();
    line_boxes(first, (0.0, first_y - first.rect.y.to_px()), &mut lines);
    assert_eq!(lines.iter().map(|line| line.1).collect::<Vec<_>>(), [0.0, 40.0]);
    assert_eq!(y("second"), 50.0);
    assert_eq!(y("next"), 90.0);
}

#[test]
fn line_height_quirk_leaves_out_the_strut() {
    // In limited-quirks (and quirks) mode a line box makes no room for the block's font where
    // the line holds no text of the block itself: an image alone (also with a collapsible space
    // after it), a smaller span alone; a `<br>` alone does.
    let html = format!(
        "{LIMITED_QUIRKS}<style>@page {{ size: 400px 400px; margin: 0 }} body {{ margin: 0; font: 20px/30px Inter }}
           img {{ display: inline-block; width: 30px; height: 10px }} .s {{ font-size: 10px; line-height: 12px }}</style>
         <div class=image><img> </div><div class=span><span class=s>x</span></div><div class=text>x</div>
         <div class=br><br></div><div class=both><img> x</div>"
    );
    let (document, pages) = render_document(&html);
    let height = |class: &str| {
        find(&pages[0], &document, class)
            .expect(class)
            .2
            .rect
            .height
            .to_px()
    };
    assert_eq!(height("image"), 10.0);
    assert_eq!(height("span"), 12.0);
    assert_eq!(height("text"), 30.0);
    assert_eq!(height("br"), 30.0);
    assert_eq!(height("both"), 30.0);
    // In standards mode the strut is there.
    let (document, pages) = render_document(&html.replacen(LIMITED_QUIRKS, "<!DOCTYPE html>", 1));
    assert_eq!(
        find(&pages[0], &document, "image")
            .expect("image")
            .2
            .rect
            .height
            .to_px(),
        30.0
    );
}

#[test]
fn quirks_mode_fills_the_page_and_passes_percentages_on() {
    // Without a doctype `html` and `body` fill the page area and percentage heights refer to
    // it, through boxes of an automatic height; so do the percentage heights of tables.
    let (document, pages) = render_document(
        "<style>@page { size: 400px 800px; margin: 0 } body { margin: 10px; font: 10px/10px Inter }</style>
         <div class=quarter style='height: 25%'></div>
         <div style='padding: 5px'><div class=tenth style='height: 10%'></div></div>
         <table class=table style='height: 10%'><tr><td>x</td></tr></table>",
    );
    let height = |class: &str| {
        find(&pages[0], &document, class)
            .expect(class)
            .2
            .rect
            .height
            .to_px()
    };
    assert_eq!(height("quarter"), 195.0);
    assert_eq!(height("tenth"), 78.0);
    assert_eq!(height("table"), 78.0);
}

#[test]
fn quirks_mode_drops_default_margins_in_cells() {
    // The default margins of `p` and headings count for nothing at the start of a table cell
    // in quirks mode, nor those of `p` at its end, also past an empty paragraph; author margins
    // count. Tables set their text at the medium size.
    let (document, pages) = render_document(
        "<style>@page { size: 400px 400px; margin: 0 } body { margin: 0; font: 10px/10px Inter }
           td { padding: 0; vertical-align: top } p { line-height: 20px }</style>
         <table cellspacing=0><tr><td class=a><p>x</p></td><td class=b><p></p><p>x</p></td>
           <td class=c><p style='margin: 5px 0'>x</p></td></tr></table>",
    );
    let height = |class: &str| {
        find(&pages[0], &document, class)
            .expect(class)
            .2
            .rect
            .height
            .to_px()
    };
    assert_eq!(height("a"), 30.0);
    assert_eq!(height("c"), 30.0);
    // The second cell is as tall as the row; its paragraph starts at the top.
    let (_, cell_y, cell) = find(&pages[0], &document, "b").expect("b");
    let mut lines = Vec::new();
    line_boxes(cell, (0.0, cell_y - cell.rect.y.to_px()), &mut lines);
    assert_eq!(lines.first().map(|line| line.1), Some(cell_y));
}

#[test]
fn quirks_mode_cell_height_includes_padding_and_borders() {
    // Without a doctype the `height` of a cell is the height of its border box (as Excel's
    // web pages expect of their rows), with half of each collapsed border; with a doctype it is
    // the height of its content, unless `box-sizing` says otherwise. Values from Chrome.
    let html = "<style>@page { size: 400px 400px; margin: 0 } body { margin: 0; font: 10px/10px Inter }
           table { border-collapse: collapse } td { padding: 3px; border: 2px solid; font: 10px/10px Inter }
           .s td { box-sizing: border-box; border-width: 5px 1px 3px }</style>
         <div class=t><table><tr><td style='height: 30px'>x</td></tr><tr><td height=25>x</td></tr></table></div>
         <div class=after>y</div>";
    let after = |html: &str| {
        let (document, pages) = render_document(html);
        find(&pages[0], &document, "after").expect("after").1
    };
    // Rows of 30px and 25px and the outer halves of the table's borders.
    assert_eq!(after(html), 57.0);
    // 30px, 25px, 6px of padding and two halves of 2px borders each, and the outer halves.
    assert_eq!(after(&format!("<!DOCTYPE html>{html}")), 73.0);
    // Rows of 30px and 25px with borders of 5px and 3px, whose outer halves are the table's.
    assert_eq!(
        after(&format!("<!DOCTYPE html>{}", html.replace("class=t", "class=s"))),
        59.0
    );
}

#[test]
fn quirks_mode_drops_default_margins_at_the_end_of_body() {
    // The bottom margin of the last paragraph counts for nothing at the end of `body`: on the
    // second page, the body ends with the paragraph's line, its bottom border below it.
    let (document, pages) = render_document(
        "<style>@page { size: 400px 300px; margin: 0 }
           body { margin: 0; border-bottom: 4px solid; font: 16px/20px Inter }</style>
         <body class=body><div style='height: 400px'></div><p>Last</p></body>",
    );
    let (_, y, body) = find(&pages[1], &document, "body").expect("body");
    assert_eq!(y + body.rect.height.to_px(), 140.0);
}

#[test]
fn rules_collapse_borders_and_frame_draws_sides() {
    // `rules` collapses the borders: no spacing between cells, which share their rules.
    let (document, pages) = render_document(
        "<!DOCTYPE html><style>@page { size: 400px 400px; margin: 0 } body { margin: 0; font: 10px/10px Inter }</style>
         <table class=t border=1 frame=hsides rules=rows cellpadding=3 cellspacing=4 width=300>
           <tr><td>a</td></tr><tr><td>b</td></tr></table>",
    );
    let (_, _, table) = find(&pages[0], &document, "t").expect("table");
    // Two rows of 16px with 1px rules: the top and bottom frame, and one between.
    assert_eq!(table.rect.height.to_px(), 35.0);
}

#[test]
fn percentages_beyond_the_table_share_it() {
    // Columns of 50 % each that do not fit with the others at their minimum share the table,
    // all getting the same part of their share beyond their minimum (CSS Tables 3, §3.9.3).
    let (document, pages) = render_document(
        "<!DOCTYPE html><style>@page { size: 600px 400px; margin: 0 } body { margin: 0; font: 10px/10px Inter }
           td { padding: 0 }</style>
         <table width=400 cellspacing=0><tr><td class=a width='50%'>a</td><td class=b width=10>b</td>
           <td class=c width='50%'>c</td></tr></table>",
    );
    let width = |class: &str| {
        find(&pages[0], &document, class)
            .expect(class)
            .2
            .rect
            .width
            .to_px()
    };
    assert!(
        (width("a") - width("c")).abs() < 0.5,
        "{} {}",
        width("a"),
        width("c")
    );
    assert!(width("b") < 10.0, "{}", width("b"));
}

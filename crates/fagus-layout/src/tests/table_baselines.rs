//! Where table cells with `vertical-align: baseline` put their content when printed. Chrome
//! aligns them on the baseline of their row only where it lays the table out without block
//! fragmentation (Blink's `TableRowLayoutAlgorithm`): in an inline table and in boxes it lays
//! out whole (size containment, `line-clamp`, `position: fixed`, orthogonal flows); elsewhere
//! they stay at the top of their row, which keeps the height aligned cells give it. Measured in
//! Chrome 157 with the same fonts.

use super::{find, render_document};
use crate::block::first_baseline;
use fagus_style::values::WritingMode;

/// The baselines on the page of the first lines of the cells with classes `big` and `small`,
/// and the height of the `big` cell, in a document of `body`.
fn baselines(body: &str) -> (f32, f32, f32) {
    let (document, pages) = render_document(&format!(
        "<style>@page {{ size: 600px 1000px; margin: 20px }} body {{ margin: 0; font: 14px/20px Inter }}
         td {{ vertical-align: baseline; padding: 1px }} .big {{ font-size: 30px; line-height: 36px }}</style>
         {body}"
    ));
    let baseline = |class: &str| {
        let (_, y, cell) = find(&pages[0], &document, class).expect("cell");
        let line = first_baseline(cell, WritingMode::HorizontalTb).expect("line");
        (y + line.to_px(), cell.rect.height.to_px())
    };
    let (big, height) = baseline("big");
    (big, baseline("small").0, height)
}

const TABLE: &str = "<table><tr><td class=big>Big</td><td class=small>small</td></tr></table>";

#[test]
fn puts_baseline_cells_at_the_top_of_their_rows_in_print() {
    let (big, small, height) = baselines(TABLE);
    // Each cell's first line starts at the top of its content: the baselines lie the
    // difference of the half-leadings and ascents apart.
    assert!(big - small > 10.0, "{big} {small}");
    // The row is as tall as with aligned cells, which an inline table has.
    let (aligned_big, aligned_small, aligned_height) = baselines(&inline_table());
    assert!(
        (aligned_big - aligned_small).abs() < 0.01,
        "{aligned_big} {aligned_small}"
    );
    assert!(
        (height - aligned_height).abs() < 0.01,
        "{height} {aligned_height}"
    );
}

/// The table, as an inline table.
fn inline_table() -> String {
    TABLE.replace("<table>", "<table style='display: inline-table'>")
}

#[test]
fn aligns_baseline_cells_of_inline_tables_and_in_boxes_laid_out_whole() {
    for body in [
        format!("<p>Text {}</p>", inline_table()),
        format!(
            "<p><span style='display: inline-block'>{}</span></p>",
            inline_table()
        ),
        format!("<div style='contain: size; height: 100px'><div>{TABLE}</div></div>"),
        format!("<div style='position: fixed; top: 300px'>{TABLE}</div>"),
        format!(
            "<div style='display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 3'>{TABLE}</div>"
        ),
        format!(
            "<div style='writing-mode: vertical-rl; height: 200px'><div style='writing-mode: horizontal-tb'>{TABLE}</div></div>"
        ),
    ] {
        let (big, small, _) = baselines(&body);
        assert!((big - small).abs() < 0.01, "{body}: {big} {small}");
    }
}

#[test]
fn keeps_baseline_cells_at_the_top_inside_inline_blocks_and_flex_items() {
    for body in [
        format!("<p><span style='display: inline-block'>{TABLE}</span></p>"),
        format!("<div style='display: flex'>{TABLE}</div>"),
        // An inline table is blockified as a flex item.
        format!("<div style='display: flex'>{}</div>", inline_table()),
        format!("<div style='overflow: hidden; height: 100px'>{TABLE}</div>"),
        format!("<div style='contain: inline-size'>{TABLE}</div>"),
        // A table in a cell of an inline table.
        format!(
            "<p><span style='display: inline-table'><span style='display: table-cell'>{TABLE}</span></span></p>"
        ),
    ] {
        let (big, small, _) = baselines(&body);
        assert!(big - small > 10.0, "{body}: {big} {small}");
    }
}

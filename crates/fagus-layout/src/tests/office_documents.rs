//! Layouts that documents of office programs and exports from web applications use: Excel's
//! web pages, timesheets and ledgers with column widths, calendars with mini tables, Japanese
//! invoices. Measured in Chrome 157 with the same fonts.

use super::{find, line_boxes, render_document, runs};

#[test]
fn col_widths_give_way_to_the_content_of_other_columns() {
    // A `<col>` width is the column's max-content width, not its minimum: where the table is
    // too narrow for it and a word in another column, the columns with widths shrink in
    // proportion to what they have beyond their content (as in Blink).
    let (document, pages) = render_document(
        "<!DOCTYPE html><style>@page { size: 500px 400px; margin: 0 } body { margin: 0; font: 10px/12px Inter }
           table { border-collapse: collapse; width: 300px } td { padding: 0; border: 1px solid }</style>
         <table><colgroup><col style='width: 100px'><col><col style='width: 80px'></colgroup>
           <tr><td class=a>aaa bbb ccc</td><td class=b>Wohnungsbaugenossenschaft x</td><td class=c>ddd eee</td></tr></table>",
    );
    let x = |class: &str| find(&pages[0], &document, class).expect(class).0;
    let width = |class: &str| {
        find(&pages[0], &document, class)
            .expect(class)
            .2
            .rect
            .width
            .to_px()
    };
    // Chrome puts the second and third cells' text at 86.5 and 230.9.
    assert!((x("b") - 85.5).abs() < 1.0, "{}", x("b"));
    assert!((x("c") - 229.9).abs() < 1.0, "{}", x("c"));
    assert!(
        (x("c") + width("c") - 300.5).abs() < 1.0,
        "the table keeps its width"
    );
}

#[test]
fn tables_with_captions_align_on_their_first_row() {
    // In a flex row aligned by baselines and as an inline table, a table with a caption above
    // stands on the baseline of its first row, as without one.
    let (document, pages) = render_document(
        "<!DOCTYPE html><style>@page { size: 500px 400px; margin: 0 } body { margin: 0; font: 10px/12px Inter }
           .f { display: flex; align-items: baseline } table { border-collapse: collapse } td { padding: 2px }</style>
         <div class=f><span class=big style='font-size: 30px; line-height: 36px'>Big</span>
           <table><caption>Cap</caption><tr><td class=row>row</td></tr></table></div>
         <div class=p>text <table style='display: inline-table'><caption>Cap</caption><tr><td class=cell>row</td></tr></table></div>",
    );
    let baseline = |class: &str| {
        let (x, y, fragment) = find(&pages[0], &document, class).expect(class);
        let mut lines = Vec::new();
        line_boxes(
            fragment,
            (x - fragment.rect.x.to_px(), y - fragment.rect.y.to_px()),
            &mut lines,
        );
        lines[0].1
    };
    // Chrome draws "Big" on a baseline at 29px and the row's text at 27px (2px higher: the
    // row's baseline is the bottom of its cell's content box, not its text's), so the row's
    // line of 12px starts at 17px; with the caption's baseline it would be 7px lower.
    let big = find(&pages[0], &document, "big").expect("big").1;
    assert!(
        (baseline("row") - big - 17.0).abs() < 0.5,
        "{} {big}",
        baseline("row")
    );
    // The inline table's row is 2px above the line of "text" the same way, below its caption:
    // Chrome draws them on baselines at 62px and 60px, the row's line of 12px from 50px.
    assert!((baseline("cell") - 50.0).abs() < 0.5, "{}", baseline("cell"));
}

#[test]
fn vertical_flex_containers_align_items_across_their_lines() {
    // A seal of two vertical lines in a flex container of 80px by 80px, centred: the lines
    // stand in the middle of its width, as in Chrome.
    let (document, pages) = render_document(
        "<!DOCTYPE html><style>@page { size: 400px 400px; margin: 20px } body { margin: 0; font: 12px/18px Inter }
           .s { width: 80px; height: 80px; writing-mode: vertical-rl; display: flex; align-items: center;
             justify-content: center }</style>
         <div class=s>abc<br>de</div>",
    );
    let (x, y, seal) = find(&pages[0], &document, "s").expect("seal");
    let mut lines = Vec::new();
    line_boxes(
        seal,
        (x - seal.rect.x.to_px(), y - seal.rect.y.to_px()),
        &mut lines,
    );
    let left = lines.iter().map(|line| line.0).fold(f32::MAX, f32::min);
    let right = lines.iter().map(|line| line.0 + 18.0).fold(f32::MIN, f32::max);
    // Two lines of 18px in 80px leave 22px on each side.
    assert!((left - (x + 22.0)).abs() < 0.5, "{lines:?} at {x}");
    assert!((right - (x + 58.0)).abs() < 0.5, "{lines:?} at {x}");
}

#[test]
fn justification_spaces_out_ideographs() {
    // Justified Japanese text widens the gaps between its characters, not only its spaces
    // (`text-justify: auto`, as Blink's `ShapeResultSpacing`): the first line fills the
    // block, evenly, with nothing added after its last character.
    let (_, pages) = render_document(
        "<!DOCTYPE html><style>@page { size: 400px 400px; margin: 0 } body { margin: 0; font: 10px/12px Inter }
           p { width: 100px; text-align: justify; margin: 0 }</style>
         <p lang=ja>誠に恐れ入りますが振込手数料は貴社にてご負担くださいますようお願い申し上げます</p>",
    );
    let mut all = Vec::new();
    runs(&pages[0].fragments[0], &mut all);
    let advances: Vec<f32> = all[0].glyphs.iter().map(|glyph| glyph.advance).collect();
    // The ideographs the test fonts lack are 6.5625px wide: fifteen fit in 100px, and the
    // 1.5625px left go to the fourteen gaps between them.
    assert_eq!(advances.len(), 15, "{advances:?}");
    for advance in &advances[..14] {
        assert!((advance - (6.5625 + 1.5625 / 14.0)).abs() < 0.001, "{advances:?}");
    }
    assert!((advances[14] - 6.5625).abs() < 0.001, "{advances:?}");
}

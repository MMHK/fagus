//! Layout of the patterns invoice and report templates of shop, ERP and billing systems use:
//! payment providers' hosted invoices (fixed tables in auto tables), packing slips and brand
//! rows of an empty icon beside a label in inline flex containers, and totals tables of
//! `table-layout: fixed` with percentage cells. The numbers are Chrome's (157), measured with
//! the same fonts.

use super::{find, render_document};

/// The x position and width of the boxes with the given classes on the first page, in CSS px.
fn boxes(html: &str, classes: &[&str]) -> Vec<(f32, f32)> {
    let (document, pages) = render_document(&format!(
        "<!DOCTYPE html><style>@page {{ size: 600px 800px; margin: 0 }} body {{ margin: 0; font: 12px/16px Inter }}
         td {{ padding: 0 }} table {{ border-spacing: 0 }}</style>{html}"
    ));
    classes
        .iter()
        .map(|class| {
            let (x, _, fragment) = find(&pages[0], &document, class).expect(class);
            (x, fragment.rect.width.to_px())
        })
        .collect()
}

#[test]
fn takes_no_content_width_from_a_fixed_table_of_a_percentage_width() {
    // A line that does not wrap in a fixed table of `width: 100%` widens neither the cell of the
    // auto table around it nor that table: the fixed table's least width is its given columns',
    // its largest as wide as anything lets it be (Blink's `kTableMaxInlineSize`), so the outer
    // table, without a width of its own, fills the page.
    let found = boxes(
        "<table class=outer><tr><td><table class=inner style='width: 100%; table-layout: fixed'>
           <tr><td style='white-space: nowrap; overflow: hidden'>Additional seats for the analytics workspace
           of Harbor and Pine, retail operations and merchandising, with more words than fit</td>
           <td class=amount style='width: 80px'>$720.00</td></tr></table></td></tr></table>",
        &["outer", "inner", "amount"],
    );
    assert_eq!(found[0], (0.0, 600.0));
    assert_eq!(found[1], (0.0, 600.0));
    assert_eq!(found[2], (520.0, 80.0));
}

#[test]
fn shares_a_fixed_table_among_its_columns_as_chrome() {
    // Columns of fixed widths without an auto column grow in proportion to their widths;
    // percentages that do not fit are scaled down to the room there is, and percentages that
    // leave room grow without an auto column; content-box percentages take the cell's padding
    // on top.
    let found = boxes(
        "<table style='width: 400px; table-layout: fixed'><tr><td class=a style='width: 50px'>a</td>
           <td class=b style='width: 150px'>b</td></tr></table>
         <table style='width: 400px; table-layout: fixed'><tr><td class=c style='width: 60%'>c</td>
           <td class=d style='width: 60%'>d</td><td class=e>e</td></tr></table>
         <table style='width: 400px; table-layout: fixed'><tr><td class=f style='width: 500px'>f</td>
           <td class=g style='width: 100px'>g</td></tr></table>
         <table style='width: 400px; table-layout: fixed'><tr><td class=h style='width: 30%'>h</td>
           <td class=i style='width: 20%'>i</td></tr></table>
         <table style='width: 200px; table-layout: fixed'><tr><td class=j style='width: 50%; padding: 0 5px'>j</td>
           <td class=k style='width: 50%; padding: 0 5px'>k</td></tr></table>",
        &["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k"],
    );
    assert_eq!(found[0], (0.0, 100.0));
    assert_eq!(found[1], (100.0, 300.0));
    assert_eq!(found[2], (0.0, 200.0));
    assert_eq!(found[3], (200.0, 200.0));
    assert_eq!(found[4].1, 0.0);
    // Fixed widths wider than the table widen it to their sum.
    assert_eq!(found[5], (0.0, 500.0));
    assert_eq!(found[6], (500.0, 100.0));
    assert_eq!(found[7], (0.0, 240.0));
    assert_eq!(found[8], (240.0, 160.0));
    assert_eq!(found[9], (0.0, 100.0));
    assert_eq!(found[10], (100.0, 100.0));
}

#[test]
fn puts_the_baseline_of_a_flex_container_on_its_first_item() {
    // The first item of a flex or grid container gives it its baseline, also where another
    // item has lines: an empty icon before a label, its bottom edge. Inside an inline block,
    // a flex container gives its first baseline, not its last. The line is then as tall as the
    // icon and the strut's descent below it.
    let (document, pages) = render_document(
        "<!DOCTYPE html><style>@page { size: 600px 800px; margin: 0 } body { margin: 0; font: 16px Inter }
           .row { display: inline-flex; align-items: center; gap: 10px } i { display: block; width: 32px; height: 32px }
           .label { font-size: 14px; line-height: 20px }</style>
         <div class=a><span class=row><i></i><span class=label>Lumen Analytics</span></span></div>
         <div class=b><span class=row><span class=label>Lumen Analytics</span><i></i></span></div>
         <div class=c><span class=row style='flex-direction: row-reverse'><i></i><span class=label>reversed</span></span></div>
         <div class=d><span style='display: inline-block'><span style='display: flex; align-items: center'>
           <span class=label>y</span><i></i></span></span></div>
         <div class=e><span style='display: inline-grid; grid-template-columns: auto auto; align-items: center'>
           <i></i><span class=label>grid</span></span></div>",
    );
    let height = |class: &str| {
        find(&pages[0], &document, class)
            .expect(class)
            .2
            .rect
            .height
            .to_px()
    };
    // 32px of icon above the baseline and Inter's descent at 16px below it (3.875px), which
    // Chrome's line rounds to 36.
    let icon_on_baseline = height("a");
    assert!((icon_on_baseline - 36.0).abs() < 0.2, "{icon_on_baseline}");
    // A label first: its baseline lies inside the centred row, which is the line's height.
    assert!((height("b") - 32.0).abs() < 0.1, "{}", height("b"));
    // A reversed row starts from its end: the label, laid out on the left.
    assert!((height("c") - 32.0).abs() < 0.1, "{}", height("c"));
    assert!((height("d") - 32.0).abs() < 0.1, "{}", height("d"));
    assert!((height("e") - icon_on_baseline).abs() < 0.1, "{}", height("e"));
}

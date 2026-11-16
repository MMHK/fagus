//! Layout of what rich-text editors (`CKEditor` 5, Trix) and templates written for older PDF
//! engines (wkhtmltopdf, Bootstrap 3 with dompdf) rely on, as Chrome prints it. The
//! expectations are Chrome's, measured with the same font.

use super::{contains_text, find, render_document};

#[test]
fn floated_tables_resolve_percentages_against_their_containing_block() {
    // CKEditor 5's side image: `figure.image` is a table floated right with `max-width: 50%`,
    // its image `max-width: 100%` (and so compressible). The table is as wide as the image, not
    // half of it (its `max-width` is of the body, not of the float's own width); an auto margin
    // takes nothing and does not widen it.
    let (document, pages) = render_document(
        "<style>body { margin: 0; width: 600px } figure { display: table; float: right;
           clear: both; margin: 0; max-width: 50% }
         svg { display: block; width: 160px; height: 90px; max-width: 100%; min-width: 100% }</style>
         <figure class=a><svg></svg></figure>
         <figure class=b style='margin: 0 0 10px auto'><svg></svg></figure>
         <figure class=c style='max-width: 100px'><svg></svg></figure>",
    );
    let width = |class: &str| {
        find(&pages[0], &document, class)
            .expect(class)
            .2
            .rect
            .width
            .to_px()
    };
    assert!((width("a") - 160.0).abs() < 0.01, "{}", width("a"));
    assert!((width("b") - 160.0).abs() < 0.01, "{}", width("b"));
    // A maximum below the content still narrows the float (and the image with it).
    assert!((width("c") - 100.0).abs() < 0.01, "{}", width("c"));
}

#[test]
fn a_clearfix_after_floats_that_break_goes_below_them_on_the_next_page() {
    // Bootstrap 3's `.row:after { clear: both }` after two columns that break across pages, and
    // an empty `<div style='clear: both'>`: the content after them starts below the rest of the
    // floats on the next page, not beside them (Blink's
    // `ExclusionSpace::NeedsClearancePastFragmentainer`).
    for (css, clear) in [
        (".row:after { content: ' '; display: table; clear: both }", ""),
        ("", "<div style='clear: both'></div>"),
    ] {
        for above in [180, 200, 260] {
            let (document, pages) = render_document(&format!(
                "<style>@page {{ size: 400px 300px; margin: 20px }} body {{ margin: 0; font: 14px/20px Inter }}
                 .l {{ float: left; width: 60% }} .r {{ float: left; width: 40% }} {css}</style>
                 <div style='height: {above}px'></div>
                 <div class=row><div class=l>one<br>two<br>three<br>four<br>five</div>
                 <div class=r>alpha<br>beta<br>gamma</div></div>{clear}<div class=next>After</div>"
            ));
            let page = pages
                .iter()
                .position(|page| {
                    page.fragments
                        .first()
                        .is_some_and(|content| contains_text(content, "After"))
                })
                .expect("page with the text after the row");
            let (x, y, _) = find(&pages[page], &document, "next").expect("next");
            // On the page where the last float ends: at the left edge, below its last line.
            let (left_x, top, left) = find(&pages[page], &document, "l").expect("left float");
            assert!((x - left_x).abs() < 0.01, "{css}{clear} {above}: x {x}");
            assert!(
                y >= top + left.rect.height.to_px() - 0.01,
                "{css}{clear} {above}: {y} above the float ending at {}",
                top + left.rect.height.to_px()
            );
        }
    }
}

#[test]
fn a_webkit_box_flexes_its_items_by_box_flex() {
    // wkhtmltopdf templates: `display: -webkit-box` with `-webkit-box-flex: 1` on one item and
    // a fixed width on the other; the first takes the rest of the row, and the box packs and
    // aligns by `-webkit-box-pack` and `-webkit-box-align`.
    let (document, pages) = render_document(
        "<style>body { margin: 0; width: 600px; font: 16px/20px Inter }
         .row { display: -webkit-box } .grow { -webkit-box-flex: 1 } .fixed { width: 200px }
         .packed { display: -webkit-box; -webkit-box-pack: end; -webkit-box-align: center; height: 100px }</style>
         <div class=row><div class=grow>Left</div><div class=fixed>Right</div></div>
         <div class=packed><div class=end>End</div></div>",
    );
    let (row_x, _, _) = find(&pages[0], &document, "row").expect("row");
    let (grow_x, _, grow) = find(&pages[0], &document, "grow").expect("grow");
    let (fixed_x, _, _) = find(&pages[0], &document, "fixed").expect("fixed");
    assert!((grow_x - row_x).abs() < 0.01);
    assert!(
        (grow.rect.width.to_px() - 400.0).abs() < 0.01,
        "{}",
        grow.rect.width.to_px()
    );
    assert!((fixed_x - row_x - 400.0).abs() < 0.01, "{fixed_x}");
    let (packed_x, packed_y, _) = find(&pages[0], &document, "packed").expect("packed");
    let (end_x, end_y, end) = find(&pages[0], &document, "end").expect("end");
    assert!(
        (end_x + end.rect.width.to_px() - packed_x - 600.0).abs() < 0.01,
        "{end_x}"
    );
    assert!((end_y - packed_y - 40.0).abs() < 0.01, "{end_y}");
}

#[test]
fn a_break_before_the_first_content_after_fixed_boxes_makes_no_empty_page() {
    // dompdf templates put a fixed header and footer first and may start with `.page_break`
    // (`page-break-before: always`): the boxes out of flow are no content before the break, so
    // the first page is not left empty (Chrome).
    let (_, pages) = render_document(
        "<style>@page { size: 400px 300px; margin: 0 } body { margin: 20px; font: 16px/20px Inter }
         header, footer { position: fixed; left: 0; right: 0; height: 20px }
         header { top: 0 } footer { bottom: 0 } .page_break { page-break-before: always }</style>
         <header>Head</header><footer>Foot</footer><span style='position: absolute'>Abs</span>
         <div class=page_break></div><p>First</p><div class=page_break></div><p>Second</p>",
    );
    assert_eq!(pages.len(), 2);
    let first = pages[0].fragments.first().expect("content");
    assert!(contains_text(first, "First"));
}

#[test]
fn a_webkit_box_keeps_inline_children_in_one_item_and_orders_by_ordinal_group() {
    // Unlike a flex container, a `-webkit-box` does not blockify its children (Blink's
    // `BlockifiesChildren`): spans and the text around them are one anonymous item, which does
    // not flex. Block children go in the order of `-webkit-box-ordinal-group`.
    let (document, pages) = render_document(
        "<style>body { margin: 0; font: 16px/20px Inter } .box { display: -webkit-box; width: 300px }
         .a { -webkit-box-flex: 2 } .b { -webkit-box-flex: 1 }
         .x { -webkit-box-ordinal-group: 2 } .y { -webkit-box-ordinal-group: 1 }</style>
         <div class=box><span class=a>Big</span><span class=b>small</span></div>
         <div class=box><div class=x>second</div><div class=y>first</div></div>",
    );
    let (a_x, a_y, a) = find(&pages[0], &document, "a").expect("a");
    let (b_x, b_y, _) = find(&pages[0], &document, "b").expect("b");
    assert!((a_y - b_y).abs() < 0.01);
    assert!((b_x - a_x - a.rect.width.to_px()).abs() < 0.01, "{a_x} {b_x}");
    let (x_x, _, _) = find(&pages[0], &document, "x").expect("x");
    let (y_x, _, y) = find(&pages[0], &document, "y").expect("y");
    assert!((x_x - y_x - y.rect.width.to_px()).abs() < 0.01, "{x_x} {y_x}");
}

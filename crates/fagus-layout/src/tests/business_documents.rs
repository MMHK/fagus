//! Layout found printing business documents with Chrome: table cells that give up their
//! content's width (`max-width: 0` with `text-overflow: ellipsis`) and vertical labels placed
//! absolutely on shipping labels. The numbers are Chrome's, measured with the same font.

use super::{find, render_document};

#[test]
fn caps_what_cells_ask_of_their_columns_by_a_fixed_max_width() {
    // `max-width: 0` caps the cell's widths at its border and padding (4px each side, half of
    // each collapsed border of 1px): a percentage width keeps its column, a fixed one gives way,
    // and the cell fills its column either way.
    let widths = |cell: &str| {
        let (document, pages) = render_document(&format!(
            "<style>@page {{ size: 700px 300px; margin: 0 }} body {{ margin: 0; font: 11px Inter }}
               table {{ width: 600px; border-collapse: collapse }} td {{ border: 1px solid; padding: 2px 4px }}
               .name {{ max-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; {cell} }}</style>
             <table><tr><td>01</td><td class=name>Fjordline Logistics AS, group framework incl. subsidiaries</td>
             <td class=no>NO</td><td>412,800</td></tr></table>"
        ));
        let (x, _, name) = find(&pages[0], &document, "name").expect("cell");
        let (no, _, _) = find(&pages[0], &document, "no").expect("next cell");
        (name.rect.width.to_px(), no - x)
    };
    let (box_width, column) = widths("width: 34%");
    assert!((column - 204.0).abs() < 1.0, "{column}");
    assert!((box_width - column).abs() < 1.5, "{box_width} {column}");
    let (_, column) = widths("width: 200px");
    assert!((column - 9.0).abs() < 0.5, "{column}");
}

#[test]
fn sets_vertical_text_of_positioned_boxes_in_the_room_between_their_insets() {
    // A vertical note at `top: 50%` of a box 130px high has 65px for its lines (Chrome wraps it
    // into three); between `top: 20px` and `bottom: 10px` it is 100px long and as wide as the
    // two lines that make.
    let (document, pages) = render_document(
        "<style>@page { size: 600px 400px; margin: 0 } body { margin: 0; font: 10px/12px Inter }
           .cb { position: relative; width: 120px; height: 130px }
           .s { position: absolute; writing-mode: vertical-rl }</style>
         <div class=cb><div class='s half' style='right: 0; top: 50%'>REF ORDER 88213 · PO 2026-417</div></div>
         <div class=cb><div class='s both' style='right: 0; top: 20px; bottom: 10px'>REF ORDER 88213 · PO 2026-417</div></div>",
    );
    let (_, y, half) = find(&pages[0], &document, "half").expect("note");
    assert_eq!(y, 65.0);
    assert!(half.rect.height.to_px() <= 65.0, "{}", half.rect.height.to_px());
    assert_eq!(half.rect.width.to_px(), 36.0);
    let (_, _, both) = find(&pages[0], &document, "both").expect("note");
    assert_eq!((both.rect.width.to_px(), both.rect.height.to_px()), (24.0, 100.0));
}

//! The parts of table rows on the pages they break across, as Chrome prints them: cells beside
//! a forced break that end with the row's part, cells whose content grew after breaks going on
//! with their row, monolithic content under a repeated footer, cells that avoid breaks inside,
//! and breaks between the captions or blocks before an avoided break. The numbers are Chrome's
//! (Chrome 157, measured with the same sizes).

use super::{find, render_document};
use crate::fragment::{Fragment, FragmentKind, Page};

/// Renders `body` on pages 400px × 300px without margins, with cells of a 3px border and 4px
/// padding and a table with a 6px spacing.
fn render(css: &str, body: &str) -> (fagus_dom::Document, Vec<Page>) {
    render_document(&format!(
        "<style>@page {{ size: 400px 300px; margin: 0 }} body {{ margin: 0; font: 16px/20px Inter }}
         td {{ border: 3px solid blue; padding: 4px; vertical-align: top; background: #def }}
         table {{ border-spacing: 6px }} {css}</style>{body}"
    ))
}

/// The top and height of the element with class `class` on page `page`, and its fragment.
fn part<'p>(
    document: &fagus_dom::Document,
    pages: &'p [Page],
    page: usize,
    class: &str,
) -> Option<(f32, f32, &'p Fragment)> {
    let (_, y, fragment) = find(pages.get(page)?, document, class)?;
    Some((y, fragment.rect.height.to_px(), fragment))
}

/// Whether `fragment` draws its bottom border.
fn closed(fragment: &Fragment) -> bool {
    matches!(&fragment.kind, FragmentKind::Box { sides, .. } if sides.bottom)
}

#[test]
fn ends_cells_beside_a_forced_break_with_the_rows_part() {
    let (document, pages) = render(
        "",
        "<div style='height: 30px'></div>
         <table><tr><td>A1</td><td>B1</td></tr>
         <tr><td class=a><div>Before</div><div style='break-before: page'>After</div></td>
           <td class=b style='vertical-align: bottom'><div class=bc>Bottom</div></td>
           <td class=c style='vertical-align: middle'><div class=cc>Middle</div></td></tr>
         <tr><td class=d>A3</td><td>B3</td></tr></table>",
    );
    assert_eq!(pages.len(), 2);
    // The cells beside the break end at the end of the page with their bottom border, their
    // content aligned in that height, and have no part on the next page.
    let (a, a_height, a_fragment) = part(&document, &pages, 0, "a").expect("a");
    let (b, b_height, b_fragment) = part(&document, &pages, 0, "b").expect("b");
    assert_eq!((a, a + a_height), (76.0, 300.0));
    assert_eq!((b, b + b_height), (76.0, 300.0));
    assert!(!closed(a_fragment));
    assert!(closed(b_fragment));
    assert_eq!(part(&document, &pages, 0, "bc").map(|(y, ..)| y), Some(273.0));
    assert_eq!(part(&document, &pages, 0, "cc").map(|(y, ..)| y), Some(178.0));
    assert!(find(&pages[1], &document, "b").is_none());
    assert!(find(&pages[1], &document, "c").is_none());
    assert!(find(&pages[1], &document, "a").is_some());
    assert!(find(&pages[1], &document, "d").is_some());
}

#[test]
fn ends_cells_beside_a_forced_break_on_a_page_the_row_goes_on_to() {
    let lines: String = (0..18).map(|_| "<div style='height: 20px'></div>").collect();
    let (document, pages) = render(
        "",
        &format!(
            "<table><tr><td class=a>{lines}<div class=after style='break-before: page'>After</div></td>
             <td class=b>Short</td></tr></table>"
        ),
    );
    // The row breaks at the end of the first page, then at the forced break on the second,
    // where the short cell ends with the row's part; on the third only the first cell goes on.
    assert_eq!(pages.len(), 3);
    let (b, b_height, fragment) = part(&document, &pages, 1, "b").expect("b on the second page");
    assert_eq!((b, b + b_height), (0.0, 300.0));
    assert!(closed(fragment));
    assert!(find(&pages[2], &document, "b").is_none());
    assert!(find(&pages[2], &document, "after").is_some());
}

#[test]
fn keeps_collapsed_borders_beside_a_forced_break_without_backgrounds() {
    let (document, pages) = render(
        "table { border-collapse: collapse } tr.r { background: #fdd }",
        "<table><tr class=r><td class=a><div>Before</div><div style='break-before: page'>After</div></td>
           <td class=b>B</td></tr><tr><td>A2</td><td>B2</td></tr></table>",
    );
    assert_eq!(pages.len(), 2);
    // The cell beside the break goes on to the end of the page without its bottom border, and
    // on the next page draws only its side borders down to the end of the row's part there.
    let (_, _, fragment) = part(&document, &pages, 0, "b").expect("b");
    assert!(!closed(fragment));
    let (a, a_height, _) = part(&document, &pages, 1, "a").expect("a on the second page");
    let (b, b_height, fragment) = part(&document, &pages, 1, "b").expect("b on the second page");
    assert_eq!((b, b + b_height), (a, a + a_height));
    let FragmentKind::Box { style, sides, .. } = &fragment.kind else {
        panic!("b is a box");
    };
    assert!(style.background_color.a == 0.0 && !sides.top);
    // No background of its row is drawn behind it there either.
    let backgrounds = pages[1].fragments.iter().flat_map(all).filter(|fragment| {
        matches!(&fragment.kind, FragmentKind::Box { style, node: None, .. }
            if style.background_color.r > 0.9 && style.background_color.g < 0.9)
    });
    assert_eq!(backgrounds.count(), 1, "only behind the first cell");
}

/// `fragment` and all fragments below it.
fn all(fragment: &Fragment) -> Vec<&Fragment> {
    let mut out = vec![fragment];
    for child in &fragment.children {
        out.extend(all(child));
    }
    out
}

#[test]
fn takes_a_short_cell_along_to_where_its_rows_content_ends() {
    let (document, pages) = render(
        "",
        "<table><tr><td class=a><div style='height: 200px'></div>
           <div style='contain: size; height: 200px; width: 50px'></div>
           <div style='height: 150px'></div></td>
         <td class=b style='vertical-align: bottom'>Short</td></tr></table>",
    );
    // The monolithic block moves to the second page, so the first cell needs a third, where
    // the short one goes on as far as it: Chrome's row grows by what its cell's breaks took.
    assert_eq!(pages.len(), 3);
    let (a, a_height, _) = part(&document, &pages, 2, "a").expect("a on the third page");
    let (b, b_height, fragment) = part(&document, &pages, 2, "b").expect("b on the third page");
    assert_eq!((b, b + b_height), (a, a + a_height));
    assert_eq!(a_height, 57.0);
    assert!(closed(fragment));
}

#[test]
fn draws_a_repeated_footer_over_monolithic_content_that_reaches_into_its_room() {
    let (document, pages) = render(
        "",
        "<table><tr><td>Row one</td></tr>
         <tr><td class=a><div style='height: 200px'></div>
           <div style='contain: size; height: 280px; width: 50px'></div>
           <div class=tail style='height: 100px'></div></td></tr>
         <tfoot><tr><td class=foot>Foot</td></tr></tfoot></table>",
    );
    // The block that does not fit on the first page reaches below the room for the rows on the
    // second, into that of the footer, which keeps its place at the end of the page; the rest
    // of the cell goes on on the third.
    assert_eq!(pages.len(), 3);
    let (foot, foot_height, _) = part(&document, &pages, 1, "foot").expect("footer on the second page");
    assert_eq!(foot + foot_height, 294.0);
    assert_eq!(part(&document, &pages, 2, "tail").map(|(y, ..)| y), Some(0.0));
}

#[test]
fn moves_rows_with_a_cell_that_avoids_breaks_inside() {
    let (document, pages) = render(
        "",
        "<div style='height: 200px'></div>
         <table><tr><td>Row one</td></tr>
         <tr><td class=a style='break-inside: avoid; height: 150px'>Avoid</td><td class=b>Plain</td></tr></table>",
    );
    assert_eq!(pages.len(), 2);
    assert!(find(&pages[0], &document, "a").is_none());
    assert!(find(&pages[0], &document, "b").is_none());
    assert_eq!(part(&document, &pages, 1, "a").map(|(y, ..)| y), Some(0.0));
}

#[test]
fn breaks_between_the_blocks_of_a_caption_before_an_avoided_break() {
    for display in ["table-caption", "block"] {
        let (document, pages) = render(
            "",
            &format!(
                "<div style='height: 50px'></div>
                 <div style='display: {display}'><div class=one style='height: 100px'></div>
                   <div class=two style='height: 100px'></div></div>
                 <div style='break-before: avoid'><div class=three style='contain: size; height: 100px'></div></div>"
            ),
        );
        // Not in the empty space at the end of the second block, nor before the caption: between
        // its blocks, as in Chrome.
        assert_eq!(pages.len(), 2, "{display}");
        assert_eq!(
            part(&document, &pages, 0, "one").map(|(y, ..)| y),
            Some(50.0),
            "{display}"
        );
        assert_eq!(
            part(&document, &pages, 1, "two").map(|(y, ..)| y),
            Some(0.0),
            "{display}"
        );
        assert_eq!(
            part(&document, &pages, 1, "three").map(|(y, ..)| y),
            Some(100.0),
            "{display}"
        );
    }
}

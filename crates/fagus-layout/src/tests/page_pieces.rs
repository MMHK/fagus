//! Boxes across pages as Chrome prints them: absolutely positioned boxes of a transformed
//! container on pages where it has no fragment of its own, transformed boxes broken across
//! pages, boxes of a fixed height around monolithic content taller than a page, and table rows
//! below a row that a forced break splits. The numbers are Chrome's (Chrome 157, measured with
//! the same sizes).

use super::{find, render_document};
use crate::fragment::{FragmentKind, Page};

/// Whether `fragment` draws its top and bottom borders.
fn sides(fragment: &crate::fragment::Fragment) -> (bool, bool) {
    match &fragment.kind {
        FragmentKind::Box { sides, .. } => (sides.top, sides.bottom),
        _ => (false, false),
    }
}

/// The top and height of the element with class `class` on page `page`.
fn place(document: &fagus_dom::Document, pages: &[Page], page: usize, class: &str) -> Option<(f32, f32)> {
    let (_, y, fragment) = find(pages.get(page)?, document, class)?;
    Some((y, fragment.rect.height.to_px()))
}

#[test]
fn puts_positioned_boxes_outside_a_containers_pages_in_empty_pieces_of_it() {
    let (document, pages) = render_document(
        "<style>@page { size: 600px 400px; margin: 0 } body { margin: 0 }</style>
         <div style='height: 400px'></div>
         <div class=c style='transform: rotate(180deg); height: 1000px'>
           <div class=y style='position: absolute; width: 100%; top: -400px; height: 400px'></div>
           <div class=p style='position: absolute; width: 100%; height: 400px'></div>
           <div class=o style='position: absolute; width: 100%; top: 1200px; height: 400px'></div>
         </div>",
    );
    assert_eq!(pages.len(), 5);
    // The box placed above the container lands on the first page, in a fragment of the container
    // without height at the top of the page (which turns it around there), as does the one
    // placed below it on the fifth page.
    for (page, class) in [(0, "y"), (4, "o")] {
        let (_, top, container) = find(&pages[page], &document, "c").expect("container piece");
        assert_eq!((top, container.rect.height.to_px()), (0.0, 0.0));
        assert_eq!(sides(container), (false, false));
        assert!(
            container.children.iter().any(|child| matches!(&child.kind,
                FragmentKind::Box { node: Some(node), .. }
                    if document.element(*node).is_some_and(|data| data.classes.iter().any(|name| &**name == class))))
        );
        assert_eq!(place(&document, &pages, page, class), Some((0.0, 400.0)));
    }
    // The container's own fragments are where they were.
    assert_eq!(place(&document, &pages, 1, "c"), Some((0.0, 400.0)));
    assert_eq!(place(&document, &pages, 1, "p"), Some((0.0, 400.0)));
}

#[test]
fn breaks_transformed_positioned_boxes_across_pages() {
    let (document, pages) = render_document(
        "<style>@page { size: 400px 300px; margin: 0 } body { margin: 0 }</style>
         <div style='position: relative; height: 100px'>
           <div class=t style='position: absolute; top: 200px; width: 100px; height: 400px;
             transform: translateX(10px)'></div>
         </div>",
    );
    // A transform does not make a box monolithic (as in Blink): it breaks at the end of the page.
    assert_eq!(pages.len(), 2);
    assert_eq!(place(&document, &pages, 0, "t"), Some((200.0, 100.0)));
    assert_eq!(place(&document, &pages, 1, "t"), Some((0.0, 300.0)));
}

#[test]
fn ends_a_box_of_a_fixed_height_at_its_height_below_monolithic_overflow() {
    // WPT css-page monolithic-overflow-031: the box ends at its own height, its bottom border
    // halfway down the third page, as one fragment (not cloning its borders on each page), and
    // the content taller than it ends with the fifth page.
    let (document, pages) = render_document(
        "<style>@page { size: 400px 300px; margin: 0 } body { margin: 0 }</style>
         <div class=b style='box-decoration-break: clone; height: 675px; border: solid black;
           border-width: 50px 0'>
           <div class=m style='contain: size; width: 50%; height: 1450px'></div>
         </div>",
    );
    assert_eq!(pages.len(), 5);
    let (_, top, fragment) = find(&pages[0], &document, "b").expect("box");
    assert_eq!((top, fragment.rect.height.to_px()), (0.0, 775.0));
    assert_eq!(sides(fragment), (true, true));
    assert!(pages[1..].iter().all(|page| find(page, &document, "b").is_none()));
    assert_eq!(place(&document, &pages, 0, "m"), Some((50.0, 1450.0)));
}

#[test]
fn adds_no_page_for_a_box_laid_out_whole_that_ends_with_a_page() {
    let pages = super::render(
        "<style>@page { size: 400px 300px; margin: 0 } body { margin: 0 }</style>
         <div style='contain: size; height: 1500px'></div>",
    );
    assert_eq!(pages.len(), 5);
}

#[test]
fn moves_the_rows_below_a_forced_break_inside_a_row_to_the_next_page() {
    let (document, pages) = render_document(
        "<style>@page { size: 600px 400px; margin: 20px } body { margin: 0; font: 16px/20px Inter }
         td { border: 3px solid blue; padding: 4px; vertical-align: top }
         table { border-spacing: 6px; border: 2px solid green }</style>
         <div style='height: 40px'></div>
         <table>
         <tr><td class=span rowspan=2 style='vertical-align: middle'>Span</td>
           <td class=broken><div>Before</div><div style='break-before: page'>After</div></td>
           <td class=short>Short</td></tr>
         <tr><td class=second>Second row</td><td>R2</td></tr>
         <tr><td class=third>Row three A</td><td>Row three B</td><td>C3</td></tr>
         </table>",
    );
    assert_eq!(pages.len(), 2);
    // The cell spanning both rows keeps its own height on the first page and ends there; the
    // other cells of the first row end with the page.
    assert_eq!(place(&document, &pages, 0, "span"), Some((68.0, 94.0)));
    assert_eq!(place(&document, &pages, 0, "short"), Some((68.0, 312.0)));
    assert!(find(&pages[1], &document, "span").is_none());
    assert!(find(&pages[0], &document, "second").is_none());
    // On the next page the second row follows the rest of the broken cell, the third row it.
    assert_eq!(place(&document, &pages, 1, "broken"), Some((20.0, 27.0)));
    assert_eq!(place(&document, &pages, 1, "second").map(|(y, _)| y), Some(53.0));
    assert_eq!(place(&document, &pages, 1, "third").map(|(y, _)| y), Some(93.0));
}

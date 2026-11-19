//! Pages that grow with their content (`-fagus-page-height: fit-content`, a Fagus extension):
//! as tall as their content and margins, at least as tall as `size` says and at most as tall as
//! a PDF page may be.

use super::{find, margin_text, render, render_document};

/// A document of `lines` lines 20px tall on pages 400px wide and `height` tall with margins of
/// 20px that grow with their content, with `css`.
fn receipt(height: u32, lines: usize, css: &str) -> String {
    format!(
        "<style>@page {{ size: 400px {height}px; margin: 20px; -fagus-page-height: fit-content;
           @bottom-center {{ content: counter(page) ' of ' counter(pages) }} }}
         body {{ margin: 0; font: 14px/20px Inter }} {css}</style>{}<div class=end>Total</div>",
        "<div>Line</div>".repeat(lines.saturating_sub(1))
    )
}

#[test]
fn grows_a_page_with_its_content() {
    // Twenty lines take 400px: the page is that and its margins tall, one page with its footer
    // at its bottom.
    let (document, pages) = render_document(&receipt(100, 20, ""));
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].height.to_px(), 440.0);
    assert_eq!(pages[0].width.to_px(), 400.0);
    let (_, top, _) = find(&pages[0], &document, "end").expect("last line");
    assert_eq!(top, 400.0);
    assert_eq!(margin_text(&pages[0]), "1 of 1|");
    let footer = pages[0].fragments.last().expect("footer");
    assert_eq!((footer.rect.y + footer.rect.height).to_px(), 440.0);
}

#[test]
fn keeps_the_height_of_size_as_the_least() {
    let pages = render(&receipt(100, 1, ""));
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].height.to_px(), 100.0);
    // A height of nothing leaves the page as tall as its content needs.
    let pages = render(&receipt(0, 3, ""));
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].height.to_px(), 100.0);
}

#[test]
fn rounds_the_grown_page_up_to_whole_points() {
    // 441px of content and margins are 330.75pt: the page is 331pt tall.
    let pages = render(&receipt(100, 20, ".end { padding-bottom: 1px }"));
    assert!((pages[0].height.to_px() * 0.75 - 331.0).abs() < 0.01);
}

#[test]
fn ends_the_root_elements_margin_below_the_content() {
    // The bottom margin of the last line goes through the body to the root element.
    let pages = render(&receipt(100, 20, ".end { margin-bottom: 32px }"));
    assert_eq!(pages[0].height.to_px(), 472.0);
}

#[test]
fn grows_each_page_after_a_forced_break_with_its_own_content() {
    let pages = render(&receipt(100, 20, "div:nth-child(6) { break-before: page }"));
    assert_eq!(pages.len(), 2);
    // Five lines on the first page, at least the height of `size`.
    assert_eq!(pages[0].height.to_px(), 140.0);
    assert_eq!(pages[1].height.to_px(), 340.0);
    assert_eq!(margin_text(&pages[1]), "2 of 2|");
}

#[test]
fn stops_growing_at_the_tallest_page_a_pdf_holds() {
    // 14,400pt are 19,200px: 960 lines and the margins fill it, the rest goes on to the next.
    let pages = render(&receipt(100, 1000, ""));
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].height.to_px(), 19_200.0);
    assert_eq!(pages[1].height.to_px(), 40.0 + 20.0 * 42.0);
}

#[test]
fn places_fixed_boxes_on_the_grown_page() {
    let (document, pages) = render_document(&receipt(
        100,
        20,
        ".stamp { position: fixed; bottom: 0; height: 10px }</style><div class=stamp></div><style>",
    ));
    // At the bottom of the page area, 20px above the end of the page.
    let (_, top, _) = find(&pages[0], &document, "stamp").expect("fixed box");
    assert_eq!(top, 410.0);
}

#[test]
fn keeps_the_declared_page_in_vertical_writing_modes() {
    let pages = render(&receipt(100, 3, "html { writing-mode: vertical-rl }"));
    assert!(
        pages
            .iter()
            .all(|page| page.height == crate::units::Au::from_px(100.0))
    );
}

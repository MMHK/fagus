//! Breaks that leftover WPT css-break cases found: forced breaks inside table cells and flex
//! items, flex items that move to the next page alone or break although they avoid it, the
//! bottom padding of flex containers left alone on a page, and elements replaced by the image of
//! their `content`. The numbers are Chrome's, measured with the same font.

use std::sync::Arc;

use super::{FragmentKind, find, render_document, render_document_with};
use crate::fragment::Page;
use crate::{Image, ImageFormat, Images};

/// Renders `body` on pages 400px × 300px without margins.
fn render(body: &str) -> (fagus_dom::Document, Vec<Page>) {
    render_document(&format!(
        "<style>@page {{ size: 400px 300px; margin: 0 }} body {{ margin: 0; font: 16px/20px Inter }}</style>{body}"
    ))
}

/// The top of the border box of the element with class `class` on page `page`.
fn top(document: &fagus_dom::Document, pages: &[Page], page: usize, class: &str) -> Option<f32> {
    find(pages.get(page)?, document, class).map(|(_, y, _)| y)
}

/// The number of the page (from 0) the element with class `class` starts on.
fn page_of(document: &fagus_dom::Document, pages: &[Page], class: &str) -> Option<usize> {
    pages
        .iter()
        .position(|page| find(page, document, class).is_some())
}

#[test]
fn breaks_table_rows_at_forced_breaks_inside_their_cells() {
    let (document, pages) = render_document(
        "<style>@page { size: 400px 300px; margin: 20px } body { margin: 0; font: 16px/20px Inter }</style>
         <table><thead><tr><td>Head</td></tr></thead>
         <tr><td><div class=a>A</div><div class=b style='break-before: page'>B</div>
           <div class=c style='break-after: page'>C</div><div class=d>D</div></td>
           <td class=e>E</td></tr>
         <tr><td class=f>F</td></tr></table>",
    );
    // The row breaks at each forced break in its cell, though it would fit on the page; the
    // header repeats.
    assert_eq!(pages.len(), 3);
    assert_eq!(page_of(&document, &pages, "a"), Some(0));
    assert_eq!(page_of(&document, &pages, "b"), Some(1));
    assert_eq!(page_of(&document, &pages, "c"), Some(1));
    assert_eq!(page_of(&document, &pages, "d"), Some(2));
    assert_eq!(page_of(&document, &pages, "e"), Some(0));
    assert_eq!(page_of(&document, &pages, "f"), Some(2));
    let (_, y, _) = find(&pages[2], &document, "d").expect("d");
    assert!(y > 20.0 && y < 60.0, "{y}");
}

#[test]
fn moves_a_flex_item_that_does_not_fit_alone() {
    let (document, pages) = render(
        "<div style='height: 100px'></div>
         <div style='display: flex'>
         <div class=a style='margin-top: 100px; height: 100px; width: 50px'></div>
         <div class=b style='margin-top: 100px; height: 150px; width: 50px; contain: size'></div>
         <div class=c style='margin-top: 200px; height: 50px; width: 50px; contain: size'></div>
         <div class=d style='margin-top: 220px; height: 50px; width: 50px'></div></div>",
    );
    // The item that ends at the end of the page stays; the one that does not fit starts the
    // next page; those below keep their places in the line, from the end of the page on.
    assert_eq!(top(&document, &pages, 0, "a"), Some(200.0));
    assert_eq!(top(&document, &pages, 1, "b"), Some(0.0));
    assert_eq!(top(&document, &pages, 1, "c"), Some(0.0));
    assert_eq!(top(&document, &pages, 1, "d"), Some(20.0));
    assert!(find(&pages[1], &document, "a").is_none());
}

#[test]
fn breaks_a_flex_item_that_avoids_it_where_it_does_not_fit_at_the_top_of_a_page() {
    let (document, pages) = render(
        "<div style='display: flex; flex-wrap: wrap; border-top: 20px solid; width: 100px'>
         <div class=a style='width: 60%; height: 20px'></div>
         <div class=b style='break-inside: avoid; width: 60%; height: 630px'></div></div>",
    );
    // It moves to the next page, below nothing but the container's border there, and breaks.
    assert_eq!(pages.len(), 4);
    assert!(find(&pages[0], &document, "b").is_none());
    for (index, page) in pages.iter().enumerate().skip(1) {
        assert!(find(page, &document, "b").is_some(), "page {index}");
    }
    let (_, _, part) = find(&pages[1], &document, "b").expect("b");
    assert_eq!(part.rect.height.to_px(), 300.0);
}

#[test]
fn breaks_between_flex_lines_where_the_lines_of_an_item_force_a_break_before_it() {
    let (document, pages) = render(
        "<style>.f { display: flex; flex-wrap: wrap } .f > div { width: 50%; height: 20px }
           .f > .n { width: 100%; height: auto }</style>
         <div class=f><div class='f n'><div>1</div><div>2</div></div>
         <div class='f n second'><div>3</div><div style='break-before: page'>4</div></div></div>",
    );
    // The break before the second item of the first line of the nested container breaks
    // before that container, a line of the outer one.
    assert_eq!(pages.len(), 2);
    assert_eq!(top(&document, &pages, 1, "second"), Some(0.0));
}

#[test]
fn slices_the_bottom_padding_of_a_flex_container_left_alone_on_the_next_pages() {
    let (document, pages) = render(
        "<div style='height: 200px'></div>
         <div style='display: flex; padding-bottom: 500px'><div style='height: 150px'></div></div>
         <div class=after style='height: 10px'></div>",
    );
    // The item ends 50px into the second page; the padding does not fit below it there and
    // fills the third page and 200px of the fourth.
    assert_eq!(pages.len(), 4);
    assert_eq!(top(&document, &pages, 3, "after"), Some(200.0));
}

#[test]
fn replaces_elements_by_the_image_of_their_content() {
    let (document, pages) = render_document_with(
        "<style>@page { size: 400px 300px; margin: 0 } body { margin: 0 }
           .logo { content: url(logo.png) } .sized { width: 40px }</style>
         <h1 class=logo>Name</h1><div class='logo sized'>Text</div>
         <p class=missing style='content: url(missing.png)'>Kept</p>",
        |_| {
            let mut images = Images::default();
            images.insert(
                "logo.png",
                Arc::new(Image {
                    data: Arc::new(Vec::new()),
                    format: ImageFormat::Png,
                    width: 16.0,
                    height: 16.0,
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
    );
    // The heading shows the image at its natural size instead of its text; a width scales it.
    let (_, _, heading) = find(&pages[0], &document, "logo").expect("logo");
    assert!(
        matches!(heading.kind, FragmentKind::Image(_))
            || heading
                .children
                .iter()
                .any(|child| matches!(child.kind, FragmentKind::Image(_)))
    );
    assert_eq!(heading.rect.height.to_px(), 16.0);
    let (_, _, sized) = find(&pages[0], &document, "sized").expect("sized");
    assert_eq!(sized.rect.height.to_px(), 40.0);
    // An image that does not load leaves the element as it is.
    let (_, _, missing) = find(&pages[0], &document, "missing").expect("missing");
    assert!(super::contains_text(missing, "Kept"));
}

#[test]
fn moves_table_rows_that_avoid_the_break_between_them_together() {
    let rows: Vec<String> = (1..=13)
        .map(|row| format!("<tr class=r{row}><td class=c{row} style='height: 20px'>{row}</td></tr>"))
        .collect();
    let rows = rows.concat();
    let table = |css: &str| {
        render(&format!(
            "<style>table {{ border-spacing: 0 }} td {{ padding: 0 }} {css}</style>
             <div style='height: 50px'></div><table>{rows}</table>"
        ))
    };
    // Twelve rows fit; where the break after the twelfth is avoided, it moves along with the
    // thirteenth, and so does the eleventh where the break after it is avoided too.
    let (document, pages) = table("");
    assert_eq!(top(&document, &pages, 1, "c13"), Some(0.0));
    let (document, pages) = table(".r12 { break-after: avoid }");
    assert_eq!(top(&document, &pages, 1, "c12"), Some(0.0));
    assert_eq!(top(&document, &pages, 0, "c11"), Some(250.0));
    let (document, pages) = table(".r11 { break-after: avoid } .r13 { break-before: avoid }");
    assert_eq!(top(&document, &pages, 1, "c11"), Some(0.0));
    assert_eq!(top(&document, &pages, 0, "c10"), Some(230.0));
    // Where every break on the page is avoided, the table moves to the next page whole.
    let (document, pages) = table("tr { break-after: avoid }");
    assert_eq!(top(&document, &pages, 1, "c1"), Some(0.0));
}

#[test]
fn breaks_before_the_bottom_padding_rather_than_before_the_only_child() {
    let html = |above: u32| {
        render(&format!(
            "<div style='height: {above}px'></div><div style='border-top: 10px solid'>
             <div style='padding-bottom: 50px'>
             <div class=mono style='contain: size; height: 290px'></div></div></div>
             <div class=after style='height: 10px'></div>"
        ))
    };
    // The monolithic child fills the page below the border: no break lies between the border
    // and the box, so the box stays and its bottom padding goes on to the next page alone.
    let (document, pages) = html(0);
    assert_eq!(top(&document, &pages, 0, "mono"), Some(10.0));
    assert_eq!(top(&document, &pages, 1, "after"), Some(50.0));
    // Below other content, the box around moves to the next page whole, as in Chrome.
    let (document, pages) = html(100);
    assert_eq!(top(&document, &pages, 1, "mono"), Some(10.0));
}

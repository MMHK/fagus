//! Breaking content across pages as Chrome prints it: forced breaks that table rows, cells and
//! the items of the first and last rows of a grid pass on, the content that overflows a box of
//! fixed height, repeated table headers, and positioned boxes beside it. The numbers are
//! Chrome's, measured with the same font.

use super::{contains_text, find, render_document};
use crate::fragment::Page;

/// Renders `body` on pages 400px × 300px without margins.
fn render(body: &str) -> (fagus_dom::Document, Vec<Page>) {
    render_document(&format!(
        "<style>@page {{ size: 400px 300px; margin: 0 }} body {{ margin: 0; font: 16px/20px Inter }}
         p {{ margin: 0 }}</style>{body}"
    ))
}

/// The number of the page (from 0) the element with class `class` starts on.
fn page_of(document: &fagus_dom::Document, pages: &[Page], class: &str) -> Option<usize> {
    pages
        .iter()
        .position(|page| find(page, document, class).is_some())
}

#[test]
fn passes_breaks_of_rows_and_cells_on_to_the_table() {
    let (document, pages) = render(
        "<p>a</p>
         <table class=first><tr style='break-before: page'><td>b</td></tr><tr><td>c</td></tr></table>
         <table class=cell><tr><td>d</td><td style='break-before: page'>e</td></tr></table>
         <table class=last><tr><td>f</td></tr><tr style='break-after: page'><td>g</td></tr></table>
         <p class=after>h</p>",
    );
    assert_eq!(pages.len(), 4);
    assert_eq!(page_of(&document, &pages, "first"), Some(1));
    assert_eq!(page_of(&document, &pages, "cell"), Some(2));
    assert_eq!(page_of(&document, &pages, "after"), Some(3));
    let (_, y, _) = find(&pages[1], &document, "first").expect("table");
    assert_eq!(y, 0.0);
}

#[test]
fn breaks_tables_between_rows_and_row_groups() {
    let (document, pages) = render(
        "<table><tr><td>a</td></tr><tr style='break-after: page'><td class=one>b</td></tr>
           <tr><td class=two>c</td></tr>
           <tbody style='break-before: page'><tr><td class=three>d</td></tr></tbody></table>",
    );
    assert_eq!(pages.len(), 3);
    assert_eq!(page_of(&document, &pages, "one"), Some(0));
    assert_eq!(page_of(&document, &pages, "two"), Some(1));
    assert_eq!(page_of(&document, &pages, "three"), Some(2));
}

#[test]
fn passes_breaks_of_the_first_and_last_rows_of_grids_on() {
    let (document, pages) = render(
        "<p>a</p>
         <div class=first style='display: grid; grid-template-columns: 1fr 1fr'>
           <div>b</div><div style='break-before: page'>c</div><div>d</div></div>
         <div style='display: flex; flex-wrap: wrap'>
           <div style='width: 50%'>e</div><div style='width: 50%'>f</div>
           <div style='width: 50%; break-after: page'>g</div><div style='width: 50%'>h</div></div>
         <p class=after>i</p>",
    );
    assert_eq!(pages.len(), 3);
    assert_eq!(page_of(&document, &pages, "first"), Some(1));
    assert_eq!(page_of(&document, &pages, "after"), Some(2));
}

#[test]
fn starts_repeated_headers_at_the_top_of_the_page() {
    let rows = "<tr><td>row</td></tr>".repeat(20);
    let (document, pages) = render(&format!(
        "<table style='border-spacing: 5px 7px'><thead><tr><td class=head>h</td></tr></thead>
           <tbody>{rows}</tbody></table>"
    ));
    assert!(pages.len() > 1);
    // No spacing above the header on the next page, as in Chrome.
    let (_, y, _) = find(&pages[1], &document, "head").expect("repeated header");
    assert_eq!(y, 0.0);
}

#[test]
fn repeats_only_headers_up_to_a_quarter_of_the_page() {
    let rows = "<tr><td>row</td></tr>".repeat(20);
    // Two rows of 37px with 2px between them: 76px, more than 75px.
    for (height, repeats) in [(36.5, true), (37.0, false)] {
        let (document, pages) = render(&format!(
            "<table><thead><tr><td class=head style='padding: 0'><div style='height: {height}px'></div></td></tr>
               <tr><td style='padding: 0'><div style='height: {height}px'></div></td></tr></thead>
               <tbody>{rows}</tbody></table>"
        ));
        assert!(pages.len() > 1);
        assert_eq!(
            find(&pages[1], &document, "head").is_some(),
            repeats,
            "{height}px"
        );
    }
}

#[test]
fn continues_content_that_overflows_a_box_on_the_next_page() {
    let words = "word ".repeat(200);
    let (document, pages) = render(&format!(
        "<div style='height: 150px'></div>
         <div class=box style='height: 60px; width: 100px; border: 2px solid'>{words}</div>
         <p class=after>after</p>"
    ));
    // The box ends on the first page, and what follows it comes right below it; the content
    // that overflows it goes on at the top of the next pages, without it.
    let (_, y, fragment) = find(&pages[0], &document, "box").expect("box");
    assert_eq!((y, fragment.rect.height.to_px()), (150.0, 64.0));
    let (_, y, _) = find(&pages[0], &document, "after").expect("paragraph after the box");
    assert_eq!(y, 214.0);
    assert!(pages.len() > 2);
    let (_, y, fragment) = find(&pages[1], &document, "box").expect("overflowing content");
    assert_eq!((y, fragment.rect.height.to_px()), (0.0, 0.0));
    assert!(contains_text(fragment, "word"));
    // What a box clips is not continued.
    let (_, clipped) = render(&format!(
        "<div style='height: 150px'></div><div style='height: 60px; overflow: hidden'>{words}</div>"
    ));
    assert_eq!(clipped.len(), 1);
}

#[test]
fn stretches_tables_in_flex_columns_and_grids() {
    let (document, pages) = render(
        "<div style='display: flex; flex-direction: column; width: 300px'>
           <table class=stretched><tr><td>a</td></tr></table>
           <table class=start style='align-self: start'><tr><td>a</td></tr></table></div>
         <div style='display: grid; width: 300px'><table class=grid><tr><td>a</td></tr></table></div>",
    );
    let width = |class| {
        find(&pages[0], &document, class)
            .expect(class)
            .2
            .rect
            .width
            .to_px()
    };
    assert_eq!(width("stretched"), 300.0);
    assert_eq!(width("grid"), 300.0);
    assert!(width("start") < 50.0);
}

#[test]
fn places_inline_positioned_boxes_by_the_direction_of_their_run() {
    let (document, pages) = render(
        "<p dir=rtl style='width: 300px; position: relative'>alpha <span class=gamma>gamma</span>
         <span class=abs style='position: absolute'>X</span>delta</p>",
    );
    // In a left-to-right run of a right-to-left line, the box starts at its static position's
    // left edge, which is where `delta` starts.
    let (gamma_x, _, gamma) = find(&pages[0], &document, "gamma").expect("gamma");
    let (x, _, _) = find(&pages[0], &document, "abs").expect("positioned box");
    let space = x - (gamma_x + gamma.rect.width.to_px());
    assert!(space > 0.0 && space < 6.0, "{space}");
}

/// A multi-column container with a spanner `spanner` pixels tall, then a heading that avoids a
/// break after it (`gap` pixels above the next) and a paragraph of five lines in two columns;
/// `before` comes before the container.
fn heading_below_a_spanner(before: &str, spanner: u32, gap: u32) -> (fagus_dom::Document, Vec<Page>) {
    render(&format!(
        "{before}<div style='columns: 2; column-gap: 0'>
         <div class=spanner style='column-span: all; height: {spanner}px'></div>
         <div class=h style='break-after: avoid; margin-bottom: {gap}px'>Heading</div>
         <p>aa bb cc dd ee ff gg hh ii jj kk ll mm nn oo pp qq rr ss tt uu vv ww xx yy zz ab bc
         cd de ef fg gh hi ij jk kl lm mn no op pq qr rs st tu uv vw wx xy</p></div>"
    ))
}

#[test]
fn takes_a_poor_break_in_the_first_column_below_a_spanner() {
    // Two lines of room below the spanner: Chrome leaves the heading with one line of the
    // paragraph in the first column (fewer than `orphans`, which is better than a break after
    // the heading), and goes on in the second column.
    let (document, pages) = heading_below_a_spanner("", 260, 0);
    assert_eq!(page_of(&document, &pages, "h"), Some(0));
    assert!(contains_text(&pages[0].fragments[0], "aa"));
    assert!(contains_text(&pages[0].fragments[0], "kk"));
    assert!(!contains_text(&pages[0].fragments[0], "bc"));
    // No line fits below the heading, and the second column ends with two lines: the heading
    // stays alone in the first column.
    let (document, pages) = heading_below_a_spanner("", 255, 15);
    assert_eq!(page_of(&document, &pages, "h"), Some(0));
    assert!(contains_text(&pages[0].fragments[0], "kk"));
    assert!(!contains_text(&pages[0].fragments[0], "tt"));
    // One line and a half: the heading would stay alone in the first column, but the second
    // would end with a line fewer than `orphans` asks for: the row moves to the next page.
    let (document, pages) = heading_below_a_spanner("", 270, 0);
    assert_eq!(page_of(&document, &pages, "spanner"), Some(0));
    assert_eq!(page_of(&document, &pages, "h"), Some(1));
    // Where the container could start on the next page instead, all of it moves there.
    let (document, pages) = heading_below_a_spanner("<div style='height: 240px'></div>", 20, 0);
    assert_eq!(page_of(&document, &pages, "spanner"), Some(1));
    assert_eq!(page_of(&document, &pages, "h"), Some(1));
}

#[test]
fn carries_the_space_of_justify_content_but_not_the_row_gap_over_a_break_in_a_column() {
    // A column 155px tall whose second item, 135px down, moves to the next page: 100px into the
    // column, the space `space-between` puts above it goes on there (it starts 35px down); 50px
    // into it, with a row gap of 30px of which 10px fit on the first page, the other 20px do not
    // go on (it starts 135 - 50 - 20 = 65px down), whether `justify-content` or an auto margin
    // takes the free space (Chrome 157).
    for (top, style, last, expected) in [
        (200, "justify-content: space-between", "", 35.0),
        (250, "justify-content: space-between; gap: 30px", "", 65.0),
        (250, "gap: 30px", "margin-top: auto", 65.0),
    ] {
        let (document, pages) = render(&format!(
            "<div style='display: flex; flex-direction: column; height: 155px; margin-top: {top}px; {style}'>
               <div style='height: 40px'></div><div class=last style='height: 20px; {last}'></div></div>"
        ));
        assert_eq!(pages.len(), 2, "{style}");
        let (_, y, _) = find(&pages[1], &document, "last").expect("last item");
        assert!((y - expected).abs() < 0.1, "{style}: {y}");
    }
}

#[test]
fn keeps_a_stretched_column_items_place_after_a_break() {
    // The sidebar of a flex row is stretched to the row's height and pins its last item to its
    // bottom (`justify-content: space-between`); that item, moving to the next page, keeps its
    // place in the unbroken sidebar there (Chrome 157).
    let (document, pages) = render(
        "<div style='display: flex; margin-top: 200px'>
           <div style='flex: 2'>
             <div style='height: 30px; border-bottom: 1px solid; break-inside: avoid'></div>
             <div style='height: 30px; border-bottom: 1px solid; break-inside: avoid'></div>
             <div style='height: 30px; border-bottom: 1px solid; break-inside: avoid'></div>
             <div style='height: 30px; border-bottom: 1px solid; break-inside: avoid'></div>
             <div style='height: 30px; border-bottom: 1px solid; break-inside: avoid'></div>
           </div>
           <div style='flex: 1; display: flex; flex-direction: column; justify-content: space-between'>
             <div style='height: 40px'></div><div class=contact style='height: 20px'></div>
           </div>
         </div>",
    );
    assert_eq!(pages.len(), 2);
    let (_, y, _) = find(&pages[1], &document, "contact").expect("contact");
    assert!((y - 35.0).abs() < 0.1, "{y}");
}

//! What long documents need: grid rows of `minmax()` and `fr` sized without a definite height,
//! items that span them, and the breaks around rows whose items avoid breaking inside, as in a
//! price catalogue or a conference programme; rows of a table that go on in a cell laid out whole
//! below its last break; and words hyphenated before punctuation, as in justified reports. The
//! numbers are Chrome's.

use super::{line_texts, render_document};
use crate::fragment::{Fragment, FragmentKind, Page};

/// Renders `body` on pages 400px × `height` without margins, in lines of 20px.
fn render(height: u32, css: &str, body: &str) -> Vec<Page> {
    render_document(&format!(
        "<style>@page {{ size: 400px {height}px; margin: 0 }} body {{ margin: 0; font: 16px/20px Inter }}
         .g {{ display: grid; grid-template-columns: 50px 50px 50px; gap: 4px }} {css}</style>{body}"
    ))
    .1
}

/// The first page (from 0) with the text `text`, and the top of its line on that page.
fn place(pages: &[Page], text: &str) -> Option<(usize, f32)> {
    fn walk(fragment: &Fragment, text: &str, top: f32) -> Option<f32> {
        if let FragmentKind::Text(run) = &fragment.kind
            && run.text.contains(text)
        {
            return Some(top + fragment.rect.y.to_px());
        }
        let top = top + fragment.rect.y.to_px();
        fragment.children.iter().find_map(|child| walk(child, text, top))
    }
    pages
        .iter()
        .enumerate()
        .find_map(|(index, page)| Some((index, walk(page.fragments.first()?, text, 0.0)?)))
}

#[test]
fn grows_rows_of_fixed_minimums_to_their_items() {
    // Without a definite height, the free space is infinite: `minmax(40px, auto)` rows are as
    // tall as their items, and `1fr` rows as tall as the tallest of them.
    let pages = render(
        800,
        "",
        "<div class=g style='grid-template-rows: minmax(40px, auto) minmax(40px, auto)'>
         <div style='height: 70px'>a</div><div>b</div><div>c</div><div>d</div></div><p>after</p>",
    );
    assert_eq!(place(&pages, "d"), Some((0, 74.0)));
    assert_eq!(place(&pages, "after").map(|(_, y)| y), Some(130.0));
    let pages = render(
        800,
        "",
        "<div class=g style='grid-template-rows: 1fr 1fr'><div style='height: 30px'>a</div>
         <div>b</div><div>c</div><div style='height: 70px'>d</div></div><p>after</p>",
    );
    assert_eq!(place(&pages, "d"), Some((0, 74.0)));
    assert_eq!(place(&pages, "after").map(|(_, y)| y), Some(160.0));
}

#[test]
fn leaves_what_a_spanning_item_needs_to_the_rows_without_items_of_their_own() {
    // The item spanning two rows needs 6px more than they have: the first row, whose growth
    // limit no item of its own sets, takes them all.
    let pages = render(
        800,
        "",
        "<div class=g style='grid-template-rows: repeat(3, minmax(40px, auto))'>
         <div style='grid-row: 1 / span 2; height: 120px'>a</div>
         <div style='grid-row: 2; grid-column: 2; height: 70px'>b</div>
         <div style='grid-row: 3; grid-column: 3'>c</div></div>",
    );
    assert_eq!(place(&pages, "b"), Some((0, 50.0)));
    assert_eq!(place(&pages, "c"), Some((0, 124.0)));
    // Where both rows have items of their own, they share them.
    let pages = render(
        800,
        "",
        "<div class=g style='grid-template-rows: minmax(40px, auto) minmax(40px, auto)'>
         <div style='grid-row: span 2; height: 120px'>v</div>
         <div style='grid-row: 2; height: 70px'>w</div><div style='grid-row: 1'>x</div></div>",
    );
    assert_eq!(place(&pages, "w"), Some((0, 47.0)));
}

/// A grid of a heading that avoids a break after it, two rows of items, another such heading
/// and a row whose first item (eight lines) avoids breaking inside, below `top` pixels.
fn programme(top: u32) -> String {
    let lines = (1..=8)
        .map(|line| format!("g{line}"))
        .collect::<Vec<_>>()
        .join("<br>");
    format!(
        "<div style='height: {top}px'>top</div><div class=grid>
         <h2>A</h2><div class=it>a1<br>a2<br>a3<br>a4</div><div class=it>b1</div>
         <div class=it>c1<br>c2<br>c3<br>c4</div><div class=it>d1</div>
         <h2>B</h2><div class=it>{lines}</div><div class=it>h1</div></div>"
    )
}

const PROGRAMME: &str = ".grid { display: grid; grid-template-columns: 1fr 1fr; gap: 10px }
    h2 { grid-column: 1 / -1; margin: 0; height: 20px; font-size: 12px; break-after: avoid }
    .it { break-inside: avoid }";

#[test]
fn moves_a_grid_whose_avoided_row_would_break_inside_an_item_that_avoids_it() {
    // The row below heading B does not fit, and the break before it is avoided: Blink's grid
    // breaks inside its first item all the same, a break poorer than the one before the grid,
    // where the grid goes instead.
    let pages = render(380, PROGRAMME, &programme(30));
    assert_eq!(place(&pages, "top").map(|(page, _)| page), Some(0));
    assert_eq!(place(&pages, "A").map(|(page, _)| page), Some(1));
    // At the top of the page the grid breaks inside the item.
    let pages = render(380, PROGRAMME, &programme(0));
    assert_eq!(place(&pages, "g6").map(|(page, _)| page), Some(0));
    assert_eq!(place(&pages, "g7"), Some((1, 0.0)));
}

#[test]
fn moves_a_grid_whose_item_grid_would_break_in_its_padding() {
    // The items are grids with padding around a monolithic box: the item that avoids breaking
    // inside would break in its top padding, which moves the whole grid as well.
    let css = ".grid { display: grid; grid-template-columns: 1fr 1fr; gap: 10px }
        h2 { grid-column: 1 / -1; margin: 0; height: 20px; font-size: 12px; break-after: avoid }
        .it { break-inside: avoid; display: grid; grid-template-columns: 40px 1fr; padding: 4px }
        .it i { display: block; height: 60px; contain: size }";
    let body = "<div style='height: 180px'>top</div><div class=grid>
        <h2>A</h2><div class=it><i></i><div>a1</div></div><div class=it><i></i><div>b1</div></div>
        <h2>B</h2><div class=it><i></i><div>g1<br>g2<br>g3<br>g4<br>g5</div></div></div>";
    let pages = render(380, css, body);
    assert_eq!(place(&pages, "A").map(|(page, _)| page), Some(1));
}

#[test]
fn breaks_after_an_avoided_row_where_the_next_row_starts_at_the_end_of_the_page() {
    // The row below heading B starts exactly where the page ends: Blink's grid leaves it to the
    // next page without looking at the break before it, which heading B avoids.
    let pages = render(380, PROGRAMME, &programme(140));
    assert_eq!(place(&pages, "B").map(|(page, _)| page), Some(0));
    assert_eq!(place(&pages, "g1"), Some((1, 0.0)));
}

#[test]
fn goes_on_with_the_rows_of_a_table_in_a_cell_laid_out_whole() {
    // The cell around the inner table ends on the second page without room for its bottom
    // padding, so it is laid out whole there: the inner table's second row goes on after its
    // heading, which the first page holds, rather than from its start again.
    let lines = (1..=10)
        .map(|line| format!("p{line}"))
        .collect::<Vec<_>>()
        .join("<br>");
    let pages = render_document(&format!(
        "<style>@page {{ size: 300px 260px; margin: 20px }} body {{ margin: 0; font: 16px/20px Inter }}
         table {{ border-collapse: collapse; width: 100% }} td {{ padding: 0 }}
         h2 {{ margin: 0 0 8px; font-size: 20px; line-height: 26px }} p {{ margin: 0 }}</style>
         <table><tr><td style='padding-bottom: 24px'><table>
         <tr><td><div style='height: 190px'>spacer</div></td></tr>
         <tr><td><h2>Head</h2><p>{lines}</p></td></tr></table></td></tr></table>"
    ))
    .1;
    let mut texts = Vec::new();
    line_texts(&pages[1].fragments[0], &mut texts);
    assert_eq!(texts.first().map(String::as_str), Some("p1"));
    assert_eq!(texts.len(), 10);
}

/// The first line of a paragraph `width` pixels wide whose first line ends with `word`.
fn first_line(width: u32, word: &str) -> String {
    let pages = render_document(&format!(
        "<html lang=en><style>body {{ margin: 0; font: 10pt/1.5 Inter }} p {{ margin: 0; hyphens: auto }}</style>
         <p style='width: {width}px'>As of 31 December, launch amounted to EUR 68.3 million (prior year:
         EUR 316.9 {word} The end</p>"
    ))
    .1;
    let mut texts = Vec::new();
    line_texts(&pages[0].fragments[0], &mut texts);
    texts.into_iter().next().unwrap_or_default()
}

#[test]
fn moves_a_word_whose_letters_fit_before_two_punctuation_characters() {
    // The letters of "investment" fit, the parenthesis and period after them do not: the word
    // moves to the next line whole, as Chrome on macOS finds no hyphenation point there.
    let line = first_line(575, "investment).");
    assert!(line.ends_with("316.9"), "{line}");
    // Where the letters do not fit, it is hyphenated (at a soft hyphen in the text of the run);
    // after one punctuation character too.
    let line = first_line(568, "investment).");
    assert!(line.ends_with("invest\u{ad}"), "{line:?}");
    let line = first_line(573, "investment.");
    assert!(line.ends_with("invest\u{ad}"), "{line:?}");
}

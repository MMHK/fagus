//! Page sides, blank pages and the page counter: forced breaks to a left or right page insert a
//! blank page where needed (CSS Fragmentation 3, §3.1), which `@page :blank` matches; the page
//! counter takes `counter-reset`, `counter-set` and `counter-increment` in `@page` as Chrome
//! does; and running elements are measured with each page's number.

use super::{lines, margin_text, render};

/// Sections of pages 400px × 300px, with `css`, each `<section>` holding its text.
fn sections(css: &str, sections: &[(&str, &str)]) -> Vec<crate::fragment::Page> {
    use std::fmt::Write;
    let mut body = String::new();
    for (class, text) in sections {
        let _ = write!(body, "<section class='{class}'>{text}</section>");
    }
    render(&format!(
        "<style>@page {{ size: 400px 300px; margin: 50px; @top-center {{ content: counter(page); }} }}
         body {{ margin: 0; font: 16px Inter }} section + section {{ break-before: page; }}
         {css}</style>{body}"
    ))
}

#[test]
fn measures_running_elements_with_the_number_of_each_page() {
    // The column sized by its content is as wide as "Page 10 of 11" on page 10, so the number
    // does not wrap there.
    let css =
        ".footer { position: running(footer); display: grid; grid-template-columns: 1fr auto; width: 100% }
               .footer .page::after { content: 'Page ' counter(page) ' of ' counter(pages); }
               @page { @bottom-center { content: element(footer); } }";
    let mut body: Vec<(&str, &str)> = vec![(
        "",
        "<div class=footer><div>Left</div><div class=page></div></div>1",
    )];
    body.extend(std::iter::repeat_n(("", "x"), 10));
    let pages = sections(css, &body);
    assert_eq!(pages.len(), 11);
    for page in [&pages[0], &pages[9], &pages[10]] {
        let footer_lines: usize = page.fragments[1..].iter().map(lines).sum();
        assert_eq!(footer_lines, 3, "{}", margin_text(page));
    }
}

/// The text of the margin boxes of each page.
fn margins(pages: &[crate::fragment::Page]) -> Vec<String> {
    pages.iter().map(margin_text).collect()
}

#[test]
fn inserts_blank_pages_before_forced_breaks_to_a_side() {
    // Chrome takes these breaks for `page`; Fagus inserts a blank page where the next page is
    // on the other side, as the specification says (and WeasyPrint does).
    let pages = sections(
        "@page :blank { @top-center { content: 'blank ' counter(page); } }
         @page :left { @bottom-left { content: 'L'; } }
         @page :right { @bottom-right { content: 'R'; } }
         .right { break-before: right } .left { break-before: left }
         .recto { break-before: recto } .verso { break-before: verso }
         .after { break-after: right }",
        &[
            ("", "Intro"),
            ("right", "A"),
            ("right", "B"),
            ("left", "C"),
            ("verso", "D"),
            ("recto", "E"),
            ("", "X"),
            ("after", "F"),
            ("", "G"),
        ],
    );
    assert_eq!(
        margins(&pages),
        [
            "1|R|",
            "blank 2|L|",
            "3|R|",
            "blank 4|L|",
            "5|R|",
            "6|L|",
            "blank 7|R|",
            "8|L|",
            "9|R|",
            "10|L|",
            "11|R|",
            "blank 12|L|",
            "13|R|",
        ]
    );
    // The blank pages hold no content.
    assert!(pages[1].fragments[0].children.is_empty());
    assert_eq!(lines(&pages[2].fragments[0]), 1);
}

#[test]
fn starts_with_a_left_page_after_a_forced_break_to_the_left() {
    let css = "@page :left { @bottom-left { content: 'L'; } }
               @page :right { @bottom-right { content: 'R'; } }
               .left { break-before: left }";
    let pages = sections(css, &[("left", "A"), ("left", "B")]);
    assert_eq!(margins(&pages), ["1|L|", "2|R|", "3|L|"]);
    // Right to left, recto is the left page and verso the right one.
    let pages = render(
        "<html dir=rtl><style>@page { size: 400px 300px; @top-center { content: counter(page) } }
         @page :left { @bottom-left { content: 'L'; } } @page :right { @bottom-right { content: 'R'; } }
         body { font: 16px Inter } .recto { break-before: recto } .verso { break-before: verso }</style>
         <p>A</p><p class=verso>B</p><p class=verso>C</p><p class=recto>D</p>",
    );
    assert_eq!(margins(&pages), ["1|L|", "2|R|", "3|L|", "4|R|", "5|L|"]);
}

#[test]
fn counts_pages_with_the_counter_properties_of_page_rules_as_chrome() {
    let five = [
        ("", "1"),
        ("", "2"),
        ("chapter", "3"),
        ("chapter", "4"),
        ("", "5"),
    ];
    // Chrome 157's numbers: an increment carries on to the pages after, a reset holds on its own
    // page only.
    let pages = sections(
        "@page { counter-increment: page 2; } @page :first { counter-reset: page 5; }
         @page chap { counter-reset: page; } .chapter { page: chap }",
        &five,
    );
    assert_eq!(margins(&pages), ["7|", "2|", "2|", "2|", "4|"]);
    let pages = sections(
        "@page chap { counter-set: page 10; } .chapter { page: chap }",
        &five,
    );
    assert_eq!(margins(&pages), ["1|", "2|", "10|", "10|", "11|"]);
    let pages = sections(
        "@page chap { counter-reset: page 10; counter-set: page 20; } .chapter { page: chap }",
        &five,
    );
    assert_eq!(margins(&pages), ["1|", "2|", "20|", "20|", "3|"]);
    // `none` leaves the counter to count the page; a margin box counts on its own.
    let pages = sections(
        "@page :first { counter-increment: none; }
         @page chap { counter-reset: page 5; @top-center { counter-increment: page 100; } }
         .chapter { page: chap }",
        &five,
    );
    assert_eq!(margins(&pages), ["1|", "2|", "105|", "105|", "3|"]);
    // Elements of the document do not change it; the page numbers in the content follow it.
    let pages = sections(
        "@page :first { counter-set: page 7; } .chapter { counter-reset: page 1 }
         .chapter::after { content: ' on ' counter(page) }",
        &five,
    );
    assert_eq!(margins(&pages), ["7|", "8|", "9|", "10|", "11|"]);
    assert!(super::contains_text(&pages[2].fragments[0], "on 9"));
}

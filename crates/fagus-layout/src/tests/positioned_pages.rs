//! Absolutely positioned boxes on pages, as Chrome prints them: placed in their whole containing
//! block with the page areas laid end to end, on the page their top falls on, broken where they
//! cross the end of a page, and on pages added below the last. The numbers are Chrome's,
//! measured with the same font.

use super::{contains_text, find, render_document};
use crate::fragment::{FragmentKind, Page};

/// Renders `body` on pages 400px × 300px without margins.
fn render(css: &str, body: &str) -> (fagus_dom::Document, Vec<Page>) {
    render_document(&format!(
        "<style>@page {{ size: 400px 300px; margin: 0 }} body {{ margin: 0; font: 16px/20px Inter }}
         p {{ margin: 0 }} {css}</style>{body}"
    ))
}

/// Lines `x01` to `x{count}`, one below the other.
fn numbered(count: usize) -> String {
    (1..=count)
        .map(|line| format!("x{line:02}"))
        .collect::<Vec<_>>()
        .join("<br>")
}

#[test]
fn leaves_out_the_room_below_boxes_that_end_with_padding_on_the_next_page() {
    // The section's content ends 20px above the end of the first page, its 40px bottom padding
    // goes on the next, then a block of 50px. Where a box inside the containing block is placed
    // from the first page, the containing block is 20px shorter put together than its
    // fragments, and a rule at `bottom: 0` lies 69px down the second page, not 89px (Chrome 157;
    // Redoc's operation rules); placed from the second page only, it is as tall as they are.
    // After lines that leave room at the end of the page, that room counts either way.
    let rule = |section: &str, first: bool| {
        let rule = "<div class=r></div>";
        let (before, after) = if first { (rule, "") } else { ("", rule) };
        let (document, pages) = render(
            ".c { position: relative } .r { position: absolute; bottom: 0; left: 0; width: 10px; height: 1px }",
            &format!("<div class=c>{before}{section}<div style='height: 50px'></div>{after}</div>"),
        );
        let (_, y, _) = find(&pages[1], &document, "r").expect("rule");
        y
    };
    let padded = "<div style='padding-bottom: 40px'><div style='height: 280px'></div></div>";
    assert_eq!(rule(padded, true), 69.0);
    assert_eq!(rule(padded, false), 89.0);
    let lines =
        "<div style='padding-bottom: 40px; line-height: 40px'>a<br>a<br>a<br>a<br>a<br>a<br>a<br>a</div>";
    assert_eq!(rule(lines, true), 169.0);
}

#[test]
fn breaks_absolute_boxes_at_the_end_of_a_page() {
    // The box starts 10px above the end of the first page, too little for its first line: it
    // starts there empty, and its lines go on at the top of the next page, in the rest of its
    // height.
    let (document, pages) = render(
        ".u { position: absolute; top: 290px; left: 200px; height: 40px }",
        "<p>one</p><div class=u>STRAD<br>DLE</div>",
    );
    assert_eq!(pages.len(), 2);
    let (x, y, first) = find(&pages[0], &document, "u").expect("first part");
    assert_eq!((x, y, first.rect.height.to_px()), (200.0, 290.0, 10.0));
    assert!(!contains_text(first, "STRAD"));
    assert!(matches!(first.kind, FragmentKind::Box { sides, .. } if !sides.bottom));
    let (x, y, rest) = find(&pages[1], &document, "u").expect("second part");
    assert_eq!((x, y, rest.rect.height.to_px()), (200.0, 0.0, 30.0));
    assert!(contains_text(rest, "STRAD") && contains_text(rest, "DLE"));
}

#[test]
fn adds_pages_for_absolute_boxes_below_the_last() {
    let (document, pages) = render(
        ".far { position: absolute; top: 1000px; left: 10px }",
        "<p>one</p><div class=far>FAR</div>",
    );
    assert_eq!(pages.len(), 4);
    let (x, y, _) = find(&pages[3], &document, "far").expect("box on the fourth page");
    assert_eq!((x, y), (10.0, 100.0));
}

#[test]
fn places_absolute_boxes_in_their_containing_block_broken_across_pages() {
    // The containing block spans three pages; the box goes 350px below its top (Chrome leaves
    // out its top border there), on the second page, though its placeholder is on the third.
    let (document, pages) = render(
        ".rel { position: relative; border: 10px solid } .a { position: absolute; top: 350px; left: 200px }",
        &format!("<div class=rel>{}<div class=a>ABS</div></div>", numbered(30)),
    );
    assert_eq!(pages.len(), 3);
    let (x, y, _) = find(&pages[1], &document, "a").expect("box on the second page");
    assert_eq!((x, y), (210.0, 50.0));
    // Without a top inset, a box stays at its static position.
    let (document, pages) = render(
        ".rel { position: relative; border: 10px solid } .a { position: absolute; left: 200px }",
        &format!(
            "<div class=rel>{}<div class=a>ABS</div>{}</div>",
            numbered(20),
            numbered(5)
        ),
    );
    let (_, y, _) = find(&pages[1], &document, "a").expect("box at its static position");
    assert_eq!(y, 120.0);
}

#[test]
fn places_absolute_boxes_in_a_containing_block_that_ends_below_its_content() {
    // The containing block's part on the second page is only the rest of its height: Chrome
    // then starts it below its top border and ends it at the bottom of its bottom border.
    let box_at = |inset: &str| {
        let (document, pages) = render(
            &format!(
                ".rel {{ position: relative; border: 10px solid; height: 400px; box-sizing: border-box }}
                 .a {{ position: absolute; {inset}; left: 200px; height: 20px }}"
            ),
            "<div class=rel>x<div class=a>ABS</div></div>",
        );
        assert_eq!(pages.len(), 2);
        pages
            .iter()
            .enumerate()
            .find_map(|(index, page)| find(page, &document, "a").map(|(_, y, _)| (index, y)))
    };
    assert_eq!(box_at("top: 0"), Some((0, 10.0)));
    assert_eq!(box_at("bottom: 0"), Some((1, 80.0)));
}

/// The page positions (from the top of the page area) of the fragments of class `class` that
/// are children of the page's content.
fn tops(page: &Page, document: &fagus_dom::Document, class: &str) -> Vec<f32> {
    let content = &page.fragments[0];
    content
        .children
        .iter()
        .filter(|child| {
            matches!(&child.kind, FragmentKind::Box { node: Some(node), .. }
                if document.element(*node).is_some_and(|data| data.classes.iter().any(|name| &**name == class)))
        })
        .map(|child| child.rect.y.to_px())
        .collect()
}

#[test]
fn shows_fixed_boxes_above_the_page_area_on_the_page_before() {
    // Each page has the box 30px above its page area: the part that reaches into the page area
    // shows on its page, the rest at the bottom of the page before (the page area cuts off what
    // lies outside it). The last page has no page after it to show its own on.
    let (document, pages) = render(
        ".t { position: fixed; top: -30px; height: 50px; width: 100px }",
        &format!(
            "<div>{}</div><div class=t></div>",
            numbered(40).replace("<br>", "<br>\n")
        ),
    );
    assert_eq!(pages.len(), 3);
    assert_eq!(tops(&pages[0], &document, "t"), vec![270.0, -30.0]);
    assert_eq!(tops(&pages[1], &document, "t"), vec![270.0, -30.0]);
    assert_eq!(tops(&pages[2], &document, "t"), vec![-30.0]);
}

#[test]
fn sizes_fixed_boxes_by_the_first_page_on_pages_of_another_size() {
    // The second page is a named page turned upright: Chrome places the fixed footer in the
    // first page's viewport there too, 400px wide and its bottom 300px down.
    let (document, pages) = render(
        "@page tall { size: 300px 400px } .tall { page: tall }
         .f { position: fixed; bottom: 0; left: 0; right: 0; height: 20px }",
        "<div class=f></div><p>one</p><div class=tall>two</div>",
    );
    assert_eq!(pages.len(), 2);
    for page in &pages {
        let (_, y, footer) = find(page, &document, "f").expect("footer");
        assert_eq!((y, footer.rect.width.to_px()), (280.0, 400.0));
    }
}

#[test]
fn places_absolute_boxes_in_inline_boxes_broken_across_lines() {
    // The containing block of an inline box broken across lines reaches from the start of its
    // first fragment to the end of its last (their padding edges), as in Chrome.
    fn fragments(
        fragment: &crate::fragment::Fragment,
        origin: (f32, f32),
        out: &mut Vec<(f32, f32, f32, f32)>,
    ) {
        let (x, y) = (
            origin.0 + fragment.rect.x.to_px(),
            origin.1 + fragment.rect.y.to_px(),
        );
        if let FragmentKind::Box { style, .. } = &fragment.kind
            && style.position == fagus_style::values::Position::Relative
        {
            out.push((x, y, fragment.rect.width.to_px(), fragment.rect.height.to_px()));
        }
        for child in &fragment.children {
            fragments(child, (x, y), out);
        }
    }
    let (document, pages) = render(
        ".rel { position: relative; border: 2px solid } .a { position: absolute; width: 5px; height: 5px }",
        "<div style='width: 120px'>xx <span class=rel>one two three four five\
         <span class='a s' style='top: 0; left: 0'></span><span class='a e' style='bottom: 0; right: 0'></span></span></div>",
    );
    let mut rel = Vec::new();
    for fragment in &pages[0].fragments {
        fragments(fragment, (0.0, 0.0), &mut rel);
    }
    assert!(rel.len() >= 2, "the inline box is broken across lines");
    let (first, last) = (rel[0], rel[rel.len() - 1]);
    let (x, y, _) = find(&pages[0], &document, "s").expect("box at the start");
    assert_eq!((x, y), (first.0 + 2.0, first.1 + 2.0));
    let (x, y, _) = find(&pages[0], &document, "e").expect("box at the end");
    assert_eq!((x + 5.0, y + 5.0), (last.0 + last.2 - 2.0, last.1 + last.3 - 2.0));
}

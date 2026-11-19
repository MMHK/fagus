//! Footnotes (CSS Generated Content for Paged Media 3, §2): `float: footnote` moves an element
//! into the footnote area at the bottom of the page its call is on. The expected pages are those
//! `WeasyPrint` 66 makes of the same documents, unless a test says otherwise.

use super::render;
use crate::fragment::{Fragment, FragmentKind, Page};

/// The text runs of each page with the top of each, in pixels from the top of the page.
fn runs(pages: &[Page]) -> Vec<Vec<(String, f32)>> {
    fn walk(fragment: &Fragment, y: f32, out: &mut Vec<(String, f32)>) {
        let y = y + fragment.rect.y.to_px();
        if let FragmentKind::Text(run) = &fragment.kind {
            out.push((run.text.clone(), y));
        }
        for child in &fragment.children {
            walk(child, y, out);
        }
    }
    pages
        .iter()
        .map(|page| {
            let mut out = Vec::new();
            walk(&page.fragments[0], 0.0, &mut out);
            out
        })
        .collect()
}

/// The text of each page, its runs joined by `|`.
fn texts(pages: &[Page]) -> Vec<String> {
    runs(pages)
        .into_iter()
        .map(|runs| {
            runs.into_iter()
                .map(|(text, _)| text)
                .collect::<Vec<_>>()
                .join("|")
        })
        .collect()
}

/// Where the run with `text` starts on page `page`.
fn top(pages: &[Page], page: usize, text: &str) -> f32 {
    runs(pages)[page]
        .iter()
        .find_map(|(run, y)| (run == text).then_some(*y))
        .unwrap_or_else(|| panic!("`{text}` on page {page}"))
}

/// Pages of 400 × 300 pixels, 200 high inside their margins, with lines of 20 pixels and notes
/// of lines of 12.
fn paged(css: &str, body: &str) -> Vec<Page> {
    render(&format!(
        "<style>@page {{ size: 400px 300px; margin: 50px }}
         body {{ margin: 0; font: 16px/20px Inter }} p {{ margin: 0 }}
         .note {{ float: footnote; font-size: 10px; line-height: 12px }}
         {css}</style>{body}"
    ))
}

#[test]
fn moves_footnotes_to_the_bottom_of_the_page_of_their_call() {
    let pages = paged(
        "",
        "<p>One<span class=note>First note</span> two.</p><p>Three<span class=note>Second</span>.</p>",
    );
    assert_eq!(pages.len(), 1);
    // The calls stay in the text, raised; the footnotes come at the end of the page area, after
    // the `margin-top: 1em` of the area, with their markers.
    assert_eq!(texts(&pages), ["One|1| two.|Three|2|.|1. |First note|2. |Second"]);
    assert_eq!(top(&pages, 0, "One"), 50.0);
    assert!(top(&pages, 0, "1") > 50.0);
    assert_eq!(top(&pages, 0, "First note"), 226.0);
    assert_eq!(top(&pages, 0, "Second"), 238.0);
}

#[test]
fn moves_footnotes_that_do_not_fit_to_the_next_page() {
    // The footnotes of five and six do not fit below the lines up to theirs: they go on the
    // next page with the one called there, and their calls stay (`footnote-policy: auto`).
    let pages = paged(
        "",
        "<p>a<span class=note>n1</span></p><p>b<span class=note>n2</span></p>
         <p>c<span class=note>n3</span></p><p>d<span class=note>n4<br>x</span></p>
         <p>e<span class=note>n5<br>y<br>z</span></p><p>f<span class=note>n6</span></p>
         <p>g</p><p>h</p><p>i<span class=note>n7</span></p>",
    );
    assert_eq!(
        texts(&pages),
        [
            "a|1|b|2|c|3|d|4|e|5|f|6|1. |n1|2. |n2|3. |n3|4. |n4|x",
            "g|h|i|7|5. |n5|y|z|6. |n6|7. |n7",
        ]
    );
    assert_eq!(top(&pages, 0, "n1"), 190.0);
    assert_eq!(top(&pages, 1, "n5"), 190.0);
}

/// Ten lines, the last but one calling a footnote of three lines that does not fit below it.
fn with_policy(policy: &str) -> Vec<String> {
    texts(&paged(
        &format!(".note {{ footnote-policy: {policy} }}"),
        "<p>l1</p><p>l2</p><p>l3</p><p>l4</p><p>l5</p>
         <p>m1<br>m2<br>m3<br>m4<span class=note>A<br>B<br>C</span></p><p>z</p>",
    ))
}

#[test]
fn moves_the_line_or_the_block_of_the_call_with_its_footnote() {
    // `auto`: the footnote goes on the next page, its call stays. (WeasyPrint moves `z` along,
    // as it keeps the room of the area's margin once the footnote has moved.)
    assert_eq!(
        with_policy("auto"),
        ["l1|l2|l3|l4|l5|m1|m2|m3|m4|1|z", "1. |A|B|C"]
    );
    // `line`: the line of the call goes with it, and with that line the one before, as the
    // paragraph keeps two lines (`widows`).
    assert_eq!(
        with_policy("line"),
        ["l1|l2|l3|l4|l5|m1|m2", "m3|m4|1|z|1. |A|B|C"]
    );
    // `block`: the paragraph of the call goes with it.
    assert_eq!(
        with_policy("block"),
        ["l1|l2|l3|l4|l5", "m1|m2|m3|m4|1|z|1. |A|B|C"]
    );
}

#[test]
fn keeps_the_line_of_a_call_that_starts_the_page() {
    // A footnote that does not fit below the first line of a page goes on the next page; the
    // line stays, or no page would hold it.
    let pages = paged(
        ".note { footnote-policy: line; font-size: 16px; line-height: 20px }",
        "<p>a<span class=note>1<br>2<br>3<br>4<br>5<br>6<br>7<br>8<br>9<br>10</span></p><p>b</p>",
    );
    assert_eq!(texts(&pages)[0], "a|1|b");
    assert!(texts(&pages)[1].starts_with("1. |1|2"));
}

#[test]
fn breaks_footnotes_taller_than_a_page() {
    // The footnote moves to the next page, which it fills, and goes on on the page after.
    // (WeasyPrint lets it run off the top of the page.)
    let note: Vec<String> = (1..=20).map(|line| format!("t{line}")).collect();
    let pages = paged(
        "",
        &format!(
            "<p>a<span class=note>{}</span></p><p>b</p><p>c</p>",
            note.join("<br>")
        ),
    );
    assert_eq!(pages.len(), 3);
    assert_eq!(texts(&pages)[0], "a|1|b|c");
    // 16 pixels of margin above the area, then 15 lines of 12 pixels.
    assert_eq!(top(&pages, 1, "t1"), 66.0);
    assert_eq!(top(&pages, 1, "t15"), 234.0);
    assert!(texts(&pages)[1].ends_with("|t15"));
    // The rest at the end of the third page.
    assert_eq!(texts(&pages)[2], "t16|t17|t18|t19|t20");
    assert_eq!(top(&pages, 2, "t20"), 238.0);
}

#[test]
fn counts_footnotes_from_each_page_that_resets_the_counter() {
    let body = "<p>a<span class=note>n1</span></p><p>b<span class=note>n2</span></p>
                <p style='break-before: page'>c<span class=note>n3</span></p>";
    // Through the document, by default.
    assert_eq!(texts(&paged("", body))[1], "c|3|3. |n3");
    // From each page where `@page` resets it (Prince does this; WeasyPrint does not).
    let pages = paged(
        "@page { counter-reset: footnote }
         .note::footnote-call { content: '[' counter(footnote, lower-roman) ']' }
         .note::footnote-marker { content: counter(footnote, lower-alpha) ') ' }",
        body,
    );
    assert_eq!(texts(&pages), ["a|[i]|b|[ii]|a) |n1|b) |n2", "c|[i]|a) |n3"]);
    // From the value it is reset to, on the pages that reset it.
    let pages = paged("@page :first { counter-reset: footnote 10 }", body);
    assert_eq!(texts(&pages), ["a|11|b|12|11. |n1|12. |n2", "c|13|13. |n3"]);
}

#[test]
fn counts_footnotes_with_the_counters_of_the_document() {
    // `counter-reset: footnote` on an element starts the count again; a footnote that
    // increments the counter itself is not counted twice.
    let pages = paged(
        "section { counter-reset: footnote } .two { counter-increment: footnote 2 }",
        "<section><p>a<span class=note>n1</span><span class='note two'>n2</span></p></section>
         <section><p>b<span class=note>n3</span></p></section>",
    );
    assert_eq!(texts(&pages), ["a|1|3|b|1|1. |n1|3. |n2|1. |n3"]);
}

#[test]
fn styles_the_footnote_area() {
    let pages = paged(
        "@page { @footnote { border-top: 1px solid; padding-top: 4px; margin-top: 8px } }",
        "<p>a<span class=note>n1</span></p>",
    );
    assert_eq!(top(&pages, 0, "n1"), 238.0);
    // The area: 1 + 4 + 12 pixels at the end of the page area, after its margin.
    let content = &pages[0].fragments[0];
    let area = content.children.last().expect("area");
    let FragmentKind::Box {
        style, node: None, ..
    } = &area.kind
    else {
        panic!("the footnote area");
    };
    assert_eq!(style.border_width.top, 1.0);
    assert_eq!(
        (content.rect.y + area.rect.y).to_px() + area.rect.height.to_px(),
        250.0
    );
    assert_eq!(area.rect.height.to_px(), 17.0);
    // `max-height` leaves the footnotes that do not fit below it to the next page.
    let pages = paged(
        "@page { @footnote { max-height: 30px; margin: 0 } }",
        "<p>a<span class=note>n1</span><span class=note>n2</span><span class=note>n3</span></p>",
    );
    assert_eq!(texts(&pages), ["a|1|2|3|1. |n1|2. |n2", "3. |n3"]);
}

#[test]
fn shrinks_the_content_by_the_footnotes() {
    // A footnote of 4 lines leaves 200 - 16 - 48 = 136 pixels: six lines of text, the call on
    // the first.
    let lines = (2..=12)
        .map(|line| format!("<p>l{line}</p>"))
        .collect::<Vec<_>>()
        .concat();
    let pages = paged(
        "",
        &format!("<p>l1<span class=note>A<br>B<br>C<br>D</span></p>{lines}"),
    );
    assert_eq!(texts(&pages)[0], "l1|1|l2|l3|l4|l5|l6|1. |A|B|C|D");
    assert_eq!(texts(&pages)[1], "l7|l8|l9|l10|l11|l12");
}

#[test]
fn lays_out_footnotes_the_same_every_time() {
    let body = (1..=40)
        .map(|n| {
            format!(
                "<p>Line {n} of the text<span class=note>Note {n}{}</span></p>",
                "<br>more".repeat(n % 4)
            )
        })
        .collect::<Vec<_>>()
        .concat();
    let first = texts(&paged(".note { footnote-policy: line }", &body));
    assert!(first.len() > 5);
    assert_eq!(texts(&paged(".note { footnote-policy: line }", &body)), first);
    // Every footnote is on the page of its call.
    for (index, page) in first.iter().enumerate() {
        for call in page.split('|').filter(|run| run.parse::<u32>().is_ok()) {
            assert!(
                page.contains(&format!("{call}. |Note {call}")),
                "{call} on page {index}"
            );
        }
    }
}

#[test]
fn leaves_footnotes_in_flex_items_and_positioned_boxes_in_their_place() {
    // `float` does not apply to flex items nor to absolutely positioned boxes.
    let pages = paged(
        ".flex { display: flex }",
        "<div class=flex><span class=note>Item</span></div><p><span class=note style='position: absolute; top: 100px'>Placed</span></p>",
    );
    assert_eq!(texts(&pages), ["Item|Placed"]);
}

#[test]
fn shows_no_call_where_its_content_is_none() {
    let pages = paged(
        ".note::footnote-call { content: none } .note::footnote-marker { content: '* ' }",
        "<p>Text<span class=note>Note</span></p>",
    );
    assert_eq!(texts(&pages), ["Text|* |Note"]);
}

#[test]
fn sets_inline_and_compact_footnotes_side_by_side() {
    // Inline footnotes share lines; a compact one does where it fits on a line, and is a block
    // where it does not.
    let pages = paged(
        "@page { @footnote { font: 10px/12px Inter } }
         .inline { footnote-display: inline; margin-right: 4px }
         .compact { footnote-display: compact }",
        "<p>a<span class='note inline'>n1</span><span class='note inline'>n2</span>
         <span class='note compact'>n3</span><span class='note compact'>A long compact note that does not fit on one line of the footnote area at all</span></p>",
    );
    let runs = &runs(&pages)[0];
    let at = |text: &str| runs.iter().find(|(run, _)| run == text).map(|&(_, y)| y);
    assert_eq!(at("n1"), at("n2"));
    assert_eq!(at("n2"), at("n3"));
    assert!(at("4. ") > at("n3"));
    assert_eq!(texts(&pages).len(), 1);
}

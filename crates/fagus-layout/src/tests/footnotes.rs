//! Footnotes (CSS Generated Content for Paged Media 3, §2): `float: footnote` moves an element
//! into the footnote area at the bottom of the page its call is on. The expected pages are those
//! `WeasyPrint` 66 makes of the same documents, unless a test says otherwise; where a footnote
//! does not fit on the page of its call, Fagus breaks it as Prince does.

use std::fmt::Write as _;

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

/// Nine lines calling seven footnotes; the fifth does not fit below the lines up to its call.
const SEVEN_NOTES: &str = "<p>a<span class=note>n1</span></p><p>b<span class=note>n2</span></p>
    <p>c<span class=note>n3</span></p><p>d<span class=note>n4<br>x</span></p>
    <p>e<span class=note>n5<br>y<br>z</span></p><p>f<span class=note>n6</span></p>
    <p>g</p><p>h</p><p>i<span class=note>n7</span></p>";

#[test]
fn moves_footnotes_that_do_not_fit_to_the_next_page() {
    // Not even two lines of the fifth footnote fit below the lines up to its call, as `orphans`
    // asks: it goes on the next page with the sixth, after it, and the one called there; their
    // calls stay (`footnote-policy: auto`).
    let pages = paged(".note { orphans: 3 }", SEVEN_NOTES);
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

#[test]
fn breaks_footnotes_that_do_not_fit_below_their_call() {
    // Two lines of the fifth footnote fit below its call: they stay on its page, with the
    // content ending at the call's line, and the third goes on at the top of the next page's
    // footnote area, as in Prince. (WeasyPrint moves the footnote to the next page whole.)
    let pages = paged("", SEVEN_NOTES);
    assert_eq!(
        texts(&pages),
        [
            "a|1|b|2|c|3|d|4|e|5|1. |n1|2. |n2|3. |n3|4. |n4|x|5. |n5|y",
            "f|6|g|h|i|7|z|6. |n6|7. |n7",
        ]
    );
    assert_eq!(top(&pages, 0, "y"), 238.0);
    assert_eq!(top(&pages, 1, "z"), 214.0);
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
    // A footnote that does not fit below the first line of a page breaks there, as with
    // `footnote-policy: auto`; the line stays, or no page would hold it.
    let pages = paged(
        ".note { footnote-policy: line; font-size: 16px; line-height: 20px }",
        "<p>a<span class=note>1<br>2<br>3<br>4<br>5<br>6<br>7<br>8<br>9<br>10</span></p><p>b</p>",
    );
    assert_eq!(texts(&pages), ["a|1|1. |1|2|3|4|5|6|7", "b|8|9|10"]);
    // Where not even its first lines fit, it goes on the next page.
    let pages = paged(
        ".note { footnote-policy: block; font-size: 16px; line-height: 20px; orphans: 9 }",
        "<p>a<span class=note>1<br>2<br>3<br>4<br>5<br>6<br>7<br>8<br>9<br>10</span></p><p>b</p>",
    );
    assert_eq!(texts(&pages)[0], "a|1|b");
    assert!(texts(&pages)[1].starts_with("1. |1|2"));
}

#[test]
fn breaks_footnotes_taller_than_a_page() {
    // The footnote starts below its call and fills the page; the rest goes on at the top of the
    // footnote area of the next page, with the content after the call above it.
    // (WeasyPrint moves it to the next page and lets it run off the top.)
    let note: Vec<String> = (1..=40).map(|line| format!("t{line}")).collect();
    let pages = paged(
        "",
        &format!(
            "<p>a<span class=note>{}</span></p><p>b</p><p>c</p>",
            note.join("<br>")
        ),
    );
    assert_eq!(pages.len(), 3);
    // 16 pixels of margin above the area, then 13 lines of 12 pixels below the line of the
    // call.
    assert!(texts(&pages)[0].starts_with("a|1|1. |t1|"));
    assert!(texts(&pages)[0].ends_with("|t13"));
    assert_eq!(top(&pages, 0, "t1"), 86.0);
    // A page of footnote: the content waits below the call where the area fills it.
    assert!(texts(&pages)[1].starts_with("t14|"));
    assert!(texts(&pages)[1].ends_with("|t28"));
    assert_eq!(top(&pages, 1, "t14"), 66.0);
    assert_eq!(
        texts(&pages)[2],
        "b|c|t29|t30|t31|t32|t33|t34|t35|t36|t37|t38|t39|t40"
    );
    assert_eq!(top(&pages, 2, "t40"), 238.0);
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

#[test]
fn puts_the_footnotes_below_the_content_of_a_page_that_grows() {
    // Twenty lines end 450px down; the footnote area follows them and the page grows to hold it
    // and the bottom margin, instead of placing the area at the end of the tallest page.
    let lines = (1..20).fold(String::new(), |mut lines, line| {
        let _ = write!(lines, "<p>Line{line}</p>");
        lines
    });
    let pages = paged(
        "@page { size: 400px 100px; -fagus-page-height: fit-content }",
        &format!("<p>Call<span class=note>Notetext</span></p>{lines}"),
    );
    assert_eq!(pages.len(), 1);
    assert_eq!(top(&pages, 0, "Line19"), 430.0);
    let note = top(&pages, 0, "Notetext");
    assert!(note >= 450.0, "the footnote at {note} overlaps the content");
    let height = pages[0].height.to_px();
    assert!(
        (note + 12.0 + 50.0..note + 12.0 + 51.0).contains(&height),
        "page {height}px tall, footnote at {note}"
    );
}

/// `count` lines of a footnote, `prefix1` to `prefixN`, apart.
fn note_lines(prefix: &str, count: usize) -> String {
    (1..=count)
        .map(|line| format!("{prefix}{line}"))
        .collect::<Vec<_>>()
        .join("<br>")
}

#[test]
fn breaks_inline_footnotes_between_their_lines() {
    // Two inline footnotes share lines; the second does not fit below the call: the lines of
    // both that fit stay on the page, the rest goes on in the footnote area of the next page,
    // each line once.
    let pages = paged(
        "@page { @footnote { font: 10px/12px Inter } } .note { footnote-display: inline }",
        &format!(
            "<p>l1</p><p>l2</p><p>l3</p><p>l4</p><p>l5</p><p>a<span class=note>first</span> b<span class=note>{}</span></p><p>z</p>",
            note_lines("s", 12)
        ),
    );
    let all = texts(&pages);
    assert_eq!(pages.len(), 2, "{all:?}");
    // Below the six lines, 200 - 120 - 16 = 64 pixels: five lines of the footnotes.
    assert_eq!(all[0], "l1|l2|l3|l4|l5|a|1| b|2|1. |first|2. |s1|s2|s3|s4|s5");
    assert_eq!(top(&pages, 0, "first"), 190.0);
    assert_eq!(top(&pages, 0, "s5"), 238.0);
    assert_eq!(all[1], "z|s6|s7|s8|s9|s10|s11|s12");
    assert_eq!(top(&pages, 1, "s6"), 166.0);
}

#[test]
fn breaks_compact_footnotes_between_their_lines() {
    // Compact footnotes that fit on a line are inline, and break between lines as those do.
    let notes = (1..=80).fold(String::new(), |mut notes, note| {
        let _ = write!(notes, "<span class=note>c{note}</span>");
        notes
    });
    let pages = paged(
        "@page { @footnote { font: 10px/12px Inter; margin: 0 } }
         .note { footnote-display: compact; margin-right: 6px }
         .note::footnote-call { content: none }",
        &format!("<p>l1</p><p>l2</p><p>l3</p><p>l4</p><p>l5</p><p>l6</p><p>x{notes}</p><p>z</p>"),
    );
    let all = texts(&pages);
    assert_eq!(pages.len(), 2, "{all:?}");
    // Five lines of footnotes fit below the call, the others go on on the next page.
    let lines = |page: usize| {
        let mut tops: Vec<f32> = runs(&pages)[page]
            .iter()
            .filter(|(run, _)| run.starts_with('c'))
            .map(|&(_, y)| y)
            .collect();
        tops.dedup();
        tops
    };
    assert_eq!(lines(0), [190.0, 202.0, 214.0, 226.0, 238.0], "{all:?}");
    assert!(all[1].starts_with("z|"), "{all:?}");
    for note in 1..=80 {
        let text = format!("c{note}");
        let count = all
            .iter()
            .flat_map(|page| page.split('|'))
            .filter(|run| *run == text)
            .count();
        assert_eq!(count, 1, "{text} in {all:?}");
    }
}

#[test]
fn takes_the_line_of_a_call_below_footnotes_moved_to_its_page() {
    // The call starts the second page, below a footnote moved there from the first; its own
    // footnote does not fit beside that one, but on a page of its own below its line: the
    // paragraph waits for the third page, as `footnote-policy: block` asks.
    let body = format!(
        "<p>l1</p><p>l2</p><p>l3</p><p>l4</p><p>l5</p><p>l6</p><p>l7</p><p>l8</p>
         <p>l9<span class=note style='footnote-policy: auto; orphans: 9'>{}</span></p><p>l10</p>
         <p>m1<span class=note>{}</span><br>m2</p><p>z</p>",
        note_lines("a", 7),
        note_lines("b", 10)
    );
    let pages = paged(".note { footnote-policy: block }", &body);
    assert_eq!(
        texts(&pages),
        [
            "l1|l2|l3|l4|l5|l6|l7|l8|l9|1|l10",
            "1. |a1|a2|a3|a4|a5|a6|a7",
            "m1|2|m2|z|2. |b1|b2|b3|b4|b5|b6|b7|b8|b9|b10",
        ]
    );
    // With `auto`, the paragraph stays and the footnote breaks below it.
    let pages = paged(".note { footnote-policy: auto }", &body);
    assert_eq!(
        texts(&pages)[1],
        "m1|2|1. |a1|a2|a3|a4|a5|a6|a7|2. |b1|b2|b3|b4|b5|b6"
    );
}

#[test]
fn takes_the_line_of_a_call_in_a_paragraph_from_the_page_before() {
    // The paragraph of the call starts on the page before: the page cannot break before it
    // there any more, so `footnote-policy: block` takes the line of the call along, as `line`
    // does.
    let body = format!(
        "<p>l1</p><p>l2</p><p>l3</p><p>l4</p><p>l5</p><p>l6</p><p>l7</p>
         <p>m1<br>m2<br>m3<br>m4<br>m5<br>m6<br>m7<br>m8<br>m9<br>m10<br>m11<span class=note>{}</span><br>m12</p><p>z</p>",
        note_lines("n", 12)
    );
    let expected = [
        "l1|l2|l3|l4|l5|l6|l7|m1|m2|m3",
        "m4|m5|m6|m7|m8|m9|m10",
        "m11|1|m12|1. |n1|n2|n3|n4|n5|n6|n7|n8|n9|n10|n11|n12",
        "z",
    ];
    assert_eq!(texts(&paged(".note { footnote-policy: block }", &body)), expected);
    assert_eq!(texts(&paged(".note { footnote-policy: line }", &body)), expected);
}

#[test]
fn leaves_the_lines_of_calls_in_columns_in_place() {
    // The page does not end where a line in the second column does: the footnote moves to the
    // next page, as with `footnote-policy: auto`.
    let pages = paged(
        ".note { footnote-policy: line } .columns { columns: 2; column-fill: auto; height: 180px }",
        &format!(
            "<div class=columns><p>c1</p><p>c2</p><p>c3</p><p>c4</p><p>c5</p><p>c6</p><p>c7</p><p>c8</p><p>c9</p>
             <p>d1</p><p>d2<span class=note>{}</span></p><p>d3</p></div>",
            note_lines("n", 13)
        ),
    );
    assert_eq!(texts(&pages)[0], "c1|c2|c3|c4|c5|c6|c7|c8|c9|d1|d2|1|d3");
    assert!(texts(&pages)[1].starts_with("1. |n1|"));
}

/// Whether the text run `text` on `page` is in a multi-column container.
fn in_columns(page: &Page, text: &str) -> bool {
    fn walk(fragment: &Fragment, columns: bool, text: &str) -> bool {
        let columns =
            columns || matches!(&fragment.kind, FragmentKind::Box { style, .. } if style.is_multicol());
        match &fragment.kind {
            FragmentKind::Text(run) if run.text == text => columns,
            _ => fragment.children.iter().any(|child| walk(child, columns, text)),
        }
    }
    walk(&page.fragments[0], false, text)
}

/// The paragraphs `prefix1` to `prefix{count}`, the one at `call` calling `note`.
fn paragraphs(prefix: &str, count: usize, call: usize, note: &str) -> String {
    (1..=count)
        .map(|index| {
            if index == call {
                format!("<p>{prefix}{index}<span class=note>{note}</span></p>")
            } else {
                format!("<p>{prefix}{index}</p>")
            }
        })
        .collect()
}

#[test]
fn puts_footnotes_at_the_bottom_of_the_column_of_their_call() {
    // `float-reference: column` (CSS Page Floats 3), as Prince: each column has a footnote area
    // at its bottom for the footnotes called in it, styled as the page's. The columns are
    // balanced with them; the footnotes number on through the document, and a call outside
    // columns calls into the page's area.
    let pages = paged(
        ".note { float-reference: column } .columns { columns: 2; column-gap: 20px }",
        "<div class=columns><p>p1</p><p>p2<span class=note>A</span></p><p>p3</p>
         <p>p4</p><p>p5<span class=note>B</span></p><p>p6</p></div>
         <p>after<span class=note>C</span></p>",
    );
    assert_eq!(pages.len(), 1);
    assert_eq!(
        texts(&pages)[0],
        "p1|p2|1|p3|1. |A|p4|p5|2|p6|2. |B|after|3|3. |C"
    );
    // Both columns start at the top and end with their footnotes, at the same height.
    assert_eq!(top(&pages, 0, "p4"), top(&pages, 0, "p1"));
    assert_eq!(top(&pages, 0, "A"), top(&pages, 0, "B"));
    assert!(top(&pages, 0, "A") >= top(&pages, 0, "p3") + 20.0 + 16.0);
    // The paragraph after the columns comes below them, and its footnote at the end of the page.
    assert!(top(&pages, 0, "after") > top(&pages, 0, "A"));
    assert_eq!(top(&pages, 0, "C"), 238.0);
    assert!(in_columns(&pages[0], "A") && in_columns(&pages[0], "B") && !in_columns(&pages[0], "C"));
}

#[test]
fn ends_columns_above_the_footnotes_called_in_them() {
    // Columns filled in turn, 180px high: the footnote of three lines (with the area's margin
    // of 1em, 52px) leaves room for six lines in the first column, the rest goes on in the
    // second.
    let pages = paged(
        ".note { float-reference: column } .columns { columns: 2; column-fill: auto; height: 180px }",
        &format!(
            "<div class=columns>{}</div>",
            paragraphs("c", 12, 2, &note_lines("n", 3))
        ),
    );
    assert_eq!(
        texts(&pages)[0],
        "c1|c2|1|c3|c4|c5|c6|1. |n1|n2|n3|c7|c8|c9|c10|c11|c12"
    );
    assert_eq!(top(&pages, 0, "c7"), 50.0);
    assert_eq!(top(&pages, 0, "n1"), 50.0 + 180.0 - 36.0);
}

#[test]
fn moves_a_call_whose_footnote_does_not_fit_to_the_next_column() {
    // The footnote of ten lines does not fit below the fifth line: the column ends above the
    // line of its call, which starts the next column with the footnote below it. The rest goes
    // on in a column beside the container, which its height ends, as in Chrome.
    let pages = paged(
        ".note { float-reference: column } .columns { columns: 2; column-fill: auto; height: 180px }",
        &format!(
            "<div class=columns>{}</div>",
            paragraphs("c", 9, 5, &note_lines("n", 10))
        ),
    );
    assert_eq!(pages.len(), 1);
    assert_eq!(
        texts(&pages)[0],
        "c1|c2|c3|c4|c5|1|c6|1. |n1|n2|n3|n4|n5|n6|n7|n8|n9|n10|c7|c8|c9"
    );
    assert_eq!(top(&pages, 0, "c5"), 50.0);
    assert_eq!(top(&pages, 0, "c7"), 50.0);
    assert_eq!(top(&pages, 0, "n10"), 50.0 + 180.0 - 12.0);
}

#[test]
fn puts_footnotes_too_tall_for_their_column_into_the_page_area() {
    // Columns 60px high hold no footnote of four lines below the line of its call: it goes into
    // the footnote area of the page.
    let pages = paged(
        ".note { float-reference: column } .columns { columns: 2; height: 60px }",
        &format!(
            "<div class=columns>{}</div>",
            paragraphs("c", 4, 1, &note_lines("n", 4))
        ),
    );
    assert_eq!(pages.len(), 1);
    assert_eq!(texts(&pages)[0], "c1|1|c2|c3|c4|1. |n1|n2|n3|n4");
    assert_eq!(top(&pages, 0, "n4"), 238.0);
    assert!(!in_columns(&pages[0], "n4"));
}

#[test]
fn balances_columns_with_their_footnotes_across_pages() {
    // Columns that go on over pages: each footnote is in a column of the page of its call, once,
    // and the content holds all paragraphs once.
    let body: String = (1..=40)
        .map(|index| {
            if index % 7 == 3 {
                format!("<p>q{index}<span class=note>N{index}</span></p>")
            } else {
                format!("<p>q{index}</p>")
            }
        })
        .collect();
    let pages = paged(
        ".note { float-reference: column } .columns { columns: 2 }",
        &format!("<div class=columns>{body}</div>"),
    );
    let all = runs(&pages);
    for index in 1..=40 {
        let paragraph = format!("q{index}");
        let found: Vec<usize> = (0..pages.len())
            .filter(|&page| all[page].iter().any(|(text, _)| *text == paragraph))
            .collect();
        assert_eq!(found.len(), 1, "{paragraph}: {:?}", texts(&pages));
        if index % 7 == 3 {
            let note = format!("N{index}");
            assert!(
                all[found[0]].iter().any(|(text, _)| *text == note),
                "{note} on the page of its call: {:?}",
                texts(&pages)
            );
            assert!(in_columns(&pages[found[0]], &note), "{note} in a column");
        }
    }
}

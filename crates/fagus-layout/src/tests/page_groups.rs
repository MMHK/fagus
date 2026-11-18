//! `@page :nth()` and page groups (CSS Generated Content for Paged Media 3, §3): `:nth(An+B)`
//! counts the pages of the document, `:nth(An+B of name)` those of each page group of that name,
//! which an element whose `page` is not `auto` makes where a forced break comes before it.

use super::{margin_text, render};

/// The text of the margin boxes of each page.
fn margins(pages: &[crate::fragment::Page]) -> Vec<String> {
    pages.iter().map(margin_text).collect()
}

/// Pages 400px × 300px (200px of content) with `css` and `body`; each page shows its number at
/// the top.
fn paged(css: &str, body: &str) -> Vec<crate::fragment::Page> {
    render(&format!(
        "<style>@page {{ size: 400px 300px; margin: 50px; @top-center {{ content: counter(page); }} }}
         body {{ margin: 0; font: 16px Inter }} .tall {{ height: 350px }}
         {css}</style>{body}"
    ))
}

#[test]
fn counts_the_pages_of_the_document() {
    let pages = paged(
        "@page :nth(2n) { @top-right { content: 'even'; } }
         @page :nth(-n+2) { @top-left { content: 'early'; } }
         @page :nth(5) { @top-right { content: 'five'; } }
         p { break-before: page; margin: 0 }",
        "<p>1</p><p>2</p><p>3</p><p>4</p><p>5</p>",
    );
    assert_eq!(
        margins(&pages),
        ["early|1|", "early|2|even|", "3|", "4|even|", "5|five|"]
    );
}

#[test]
fn counts_the_pages_of_each_page_group() {
    // Each chapter is a page group of its own; the pages before the first are in none.
    let pages = paged(
        ".chapter { page: chapter; break-before: page }
         @page :nth(1 of chapter) { @top-left { content: 'opening'; } }
         @page :nth(2n of chapter) { @top-right { content: 'even'; } }",
        "<p>Title</p>
         <div class=chapter><div class=tall></div></div>
         <div class=chapter><div class=tall></div></div>",
    );
    assert_eq!(
        margins(&pages),
        ["1|", "opening|2|", "3|even|", "opening|4|", "5|even|"]
    );
}

#[test]
fn starts_a_group_only_with_a_forced_break() {
    // "When the page property is applied to an element that also has a forced break property
    // applied, a page group is created." Sections of the same page name without a break share
    // their pages and make no group.
    let pages = paged(
        ".slip { page: slip }
         .break { break-before: page }
         @page :nth(1 of slip) { @top-left { content: 'opening'; } }",
        "<p>Letter</p>
         <div class=slip><div class=tall></div></div>
         <div class='slip break'><div class=tall></div></div>",
    );
    assert_eq!(margins(&pages), ["1|", "2|", "3|", "opening|4|", "5|"]);
}

#[test]
fn counts_nested_groups_across_other_page_names() {
    // The example of the specification: `:nth(3 of body)` selects the third page of each
    // chapter, even if that page is a "broadside" page.
    let pages = paged(
        ".chapter { page: body; break-before: page }
         .broadside { page: broadside; break-before: page }
         @page :nth(3 of body) { @top-left { content: 'third'; } }
         @page :nth(1 of broadside) { @top-right { content: 'broadside'; } }",
        "<div class=chapter><p>One</p><div class=broadside><div class=tall></div></div></div>
         <div class=chapter><div class=tall></div><div class=broadside>Table</div></div>",
    );
    assert_eq!(
        margins(&pages),
        ["1|", "2|broadside|", "third|3|", "4|", "5|", "third|6|broadside|"]
    );
}

#[test]
fn counts_the_group_of_a_named_selector() {
    // "When the :nth() selector is applied to a named page, and that named page is part of a
    // page-group, it selects the nth page in the page group." `:first` stays the first page of
    // the document (the specification does not redefine it for page groups).
    let pages = paged(
        ".slip { page: slip; break-before: page }
         @page slip:nth(1) { @top-left { content: 'opening'; } }
         @page slip:first { @top-right { content: 'first'; } }",
        "<div class=slip>A</div><div class=slip><div class=tall></div></div>",
    );
    assert_eq!(margins(&pages), ["opening|1|first|", "opening|2|", "3|"]);
    // Without a group, it counts the pages of the document.
    let pages = paged(
        ".slip { page: slip }
         @page slip:nth(2) { @top-left { content: 'second'; } }",
        "<p>Letter</p><div class=slip><div class=tall></div></div>",
    );
    assert_eq!(margins(&pages), ["1|", "second|2|", "3|"]);
}

#[test]
fn leaves_blank_pages_before_a_group_out_of_it() {
    // The blank page that a break to a right page inserts before a chapter is not in it; one
    // inside a chapter is.
    let pages = paged(
        ".chapter { page: chapter; break-before: right }
         .part { break-before: right }
         @page :nth(1 of chapter) { @top-left { content: 'opening'; } }
         @page :nth(3 of chapter) { @top-right { content: 'third'; } }
         @page :blank { @bottom-center { content: 'blank'; } }",
        "<p>Title</p>
         <div class=chapter>A<div class=part>B</div></div>",
    );
    assert_eq!(
        margins(&pages),
        ["1|", "2|blank|", "opening|3|", "4|blank|", "5|third|"]
    );
}

#[test]
fn ranks_nth_as_first_and_its_group_name_as_a_page_name() {
    // `:nth(1 of slip)` (1, 1, 0) beats `slip` (1, 0, 0) and `slip:left` (1, 0, 1) in any order;
    // `:nth(1)` (0, 1, 0) loses to `slip`.
    let pages = paged(
        ".slip { page: slip; break-before: page }
         @page :nth(1 of slip) { @top-left { content: 'group'; } }
         @page slip:left { @top-left { content: 'left'; } }
         @page slip { @top-left { content: 'slip'; } }
         @page :nth(1) { @top-left { content: 'document'; } }",
        "<div class=slip>A</div><div class=slip><div class=tall></div></div>",
    );
    assert_eq!(margins(&pages), ["group|1|", "group|2|", "slip|3|"]);
}

#[test]
fn sizes_the_pages_of_a_group_by_its_rules() {
    let pages = paged(
        ".slip { page: slip; break-before: page }
         @page :nth(1 of slip) { size: 300px 400px }",
        "<p>Letter</p><div class=slip><div class=tall></div></div>",
    );
    let sizes: Vec<(f32, f32)> = pages
        .iter()
        .map(|page| (page.width.to_px().round(), page.height.to_px().round()))
        .collect();
    assert_eq!(sizes, [(400.0, 300.0), (300.0, 400.0), (400.0, 300.0)]);
}

//! Layout of documents styled with CSS frameworks and the stylesheets of Markdown renderers
//! (Bootstrap, Bulma, Pico, Tailwind, github-markdown-css) as Chrome prints them.

use super::{find, lines, render_document};

#[test]
fn spaces_of_break_spaces_do_not_hang() {
    // Inline code in github-markdown-css has `white-space: break-spaces`: the space after a word
    // takes room on the line, so a word whose space does not fit goes to the next line (Chrome).
    let width = {
        let (document, pages) = render_document(
            "<style>body { font: 16px Inter } span { float: left }</style>
             <span class=m>aaaa aaaa</span>",
        );
        find(&pages[0], &document, "m")
            .expect("span")
            .2
            .rect
            .width
            .to_px()
    };
    let html = |mode: &str| {
        format!(
            "<style>body {{ font: 16px Inter }} div {{ width: {}px; white-space: {mode} }}</style>
             <div class=d>aaaa aaaa aaaa aaaa</div>",
            width + 1.0
        )
    };
    let count = |mode: &str| {
        let (document, pages) = render_document(&html(mode));
        lines(find(&pages[0], &document, "d").expect("div").2)
    };
    // `pre-wrap` hangs the space after the second word: two words on each line.
    assert_eq!(count("pre-wrap"), 2);
    // `break-spaces` does not: two words and a space are too wide, so the first two lines take
    // one word each, and the last two words (without a space after them) fit on the third.
    assert_eq!(count("break-spaces"), 3);
}

#[test]
fn floats_pulled_into_the_padding_do_not_narrow_shrink_to_fit_boxes() {
    // Bootstrap's `.form-switch`: the checkbox floats into the padding of `.form-check` with a
    // negative margin wider than itself. Its margin box counts as nothing (not less) in the
    // width of the flex item, so the label stays on one line (Chrome).
    let (document, pages) = render_document(
        "<style>body { font: 16px Inter } .row { display: flex }
         .check { padding-left: 40px } .check input { float: left; width: 32px; margin-left: -40px }
         .wrap { float: left; clear: left } .pull { float: left; width: 30px; margin-right: -60px }</style>
         <div class=row><div class='check a'><input type=checkbox><label>Send to board</label></div></div>
         <div class='wrap b'><div class=pull>ab</div><div>Send to board</div></div>
         <div class='wrap c'><div class=pull>ab</div><div class=pull style='margin-right: 0'>cd</div><div>Send to board</div></div>",
    );
    let count = |class: &str| lines(find(&pages[0], &document, class).expect("box").2);
    assert_eq!(count("a"), 1);
    // Block-level floats: their widths add up, and a negative sum counts as nothing. Each float
    // has a line of its own; the text takes one line beside them.
    assert_eq!(count("b"), 2);
    // Here they sum to nothing, and the text beside the second float wraps: two lines.
    assert_eq!(count("c"), 4);
}

#[test]
fn wrapping_flex_rows_are_as_narrow_as_their_widest_item() {
    // Bulma's `.field.is-grouped-multiline` of tags in a `.column` (`flex: 1 1 0`): the column's
    // automatic minimum width is the min-content width of the wrapping row, its widest item, so
    // the column keeps its share and the tags wrap (Chrome).
    let (document, pages) = render_document(
        "<style>body { margin: 0; font: 14px Inter } .cols { display: flex; width: 400px }
         .col { flex: 1 1 0 } .tags { display: flex; flex-wrap: wrap; gap: 10px }
         .tags span { flex-shrink: 0; white-space: nowrap; padding: 0 8px }</style>
         <div class=cols><div class=col>left</div><div class='col right'><div class=tags>
         <span class=a>Garden redesign</span><span>420 square metres</span><span class=c>Week 19</span></div></div></div>",
    );
    let right = find(&pages[0], &document, "right").expect("column").2;
    assert!(
        (right.rect.width.to_px() - 200.0).abs() < 0.01,
        "{}",
        right.rect.width.to_px()
    );
    let top = |class: &str| find(&pages[0], &document, class).expect("tag").1;
    assert!(top("c") > top("a"), "the tags wrap");
}

#[test]
fn unthemed_selects_keep_their_line_height() {
    // Pico CSS and Bootstrap's `.form-select` style selects with `appearance: none`: their text
    // takes the select's line height, which Chrome's theme sets to `normal` only for themed
    // ones.
    let (document, pages) = render_document(
        "<style>body { font: 14px Inter } select { font: inherit; padding: 10px; border: 1px solid;
         line-height: 30px; appearance: none } .themed { appearance: auto }</style>
         <select class=plain><option>Active</option></select>
         <select class=themed><option>Active</option></select>",
    );
    let height = |class: &str| {
        find(&pages[0], &document, class)
            .expect("select")
            .2
            .rect
            .height
            .to_px()
    };
    assert!((height("plain") - 52.0).abs() < 0.01, "{}", height("plain"));
    assert!(height("themed") < 45.0, "{}", height("themed"));
}

#[test]
fn themed_selects_keep_room_for_their_arrow_as_chrome_does() {
    // Chrome keeps 4px before the text of a themed select, and 1px and the arrow's 15px after
    // it, inside the author's padding; a list box keeps 2px on either side of its options and no
    // room for a scroll bar.
    let (document, pages) = render_document(
        "<style>body { font: 14px Inter } select { font: inherit; padding: 10px; border: 1px solid;
         appearance: none } .themed { appearance: auto } .list { appearance: auto; padding: 0 }</style>
         <select class=plain><option>Active</option></select>
         <select class=themed><option>Active</option></select>
         <select class=list size=2><option>Active</option><option>B</option></select>",
    );
    let width = |class: &str| {
        find(&pages[0], &document, class)
            .expect("select")
            .2
            .rect
            .width
            .to_px()
    };
    assert!(
        (width("themed") - width("plain") - 20.0).abs() < 0.01,
        "{}",
        width("themed")
    );
    let text = width("plain") - 22.0;
    let list = width("list") - 2.0 - 4.0;
    assert!(list <= text && list > text - 1.0, "{list} {text}");
}

#[test]
fn fieldsets_keep_the_margins_of_their_children() {
    // Pico CSS: labels with bottom margins in fieldsets without border or padding. A fieldset
    // is a formatting context of its own, so the margins of its first and last children do not
    // collapse with its own (Chrome).
    let (document, pages) = render_document(
        "<style>body { margin: 0; font: 14px/20px Inter } fieldset { margin: 0 0 12px; padding: 0;
         border: 0 } label { display: block; margin: 6px 0 }</style>
         <fieldset class=f><label>One</label><label>Two</label></fieldset><p class=after>After</p>",
    );
    let (_, top, fieldset) = find(&pages[0], &document, "f").expect("fieldset");
    // 6 + 20 + 6 + 20 + 6 pixels inside, the first and last margins included.
    assert!(
        (fieldset.rect.height.to_px() - 58.0).abs() < 0.01,
        "{}",
        fieldset.rect.height.to_px()
    );
    // The paragraph's top margin (14 pixels) collapses with the fieldset's bottom margin.
    let after = find(&pages[0], &document, "after").expect("paragraph").1;
    assert!((after - top - 72.0).abs() < 0.01, "{}", after - top);
}

#[test]
fn unthemed_checkboxes_show_their_generated_content() {
    // Pico CSS draws the thumb of a switch as the `::before` of a checkbox without its look.
    let (document, pages) = render_document(
        "<style>input { appearance: none; width: 40px; height: 20px; margin: 0; border: 0 }
         .s::before { display: block; content: ''; width: 20px; height: 100%; background: white }</style>
         <input type=checkbox class=s><input type=checkbox class=plain>",
    );
    let switch = find(&pages[0], &document, "s").expect("switch").2;
    let thumb = switch
        .children
        .iter()
        .flat_map(|child| std::iter::once(child).chain(child.children.iter()))
        .find(|fragment| (fragment.rect.width.to_px() - 20.0).abs() < 0.01)
        .expect("the thumb");
    assert!((thumb.rect.height.to_px() - 20.0).abs() < 0.01);
    let plain = find(&pages[0], &document, "plain").expect("checkbox").2;
    assert!((plain.rect.width.to_px() - 40.0).abs() < 0.01);
}

#[test]
fn shrink_to_fit_grids_give_flexible_columns_the_same_size() {
    // Tailwind's `grid-cols-2` (`repeat(2, minmax(0, 1fr))`) in a float: both columns are as
    // wide as the widest item, as the size of `1fr` under a max-content constraint is (Chrome).
    let (document, pages) = render_document(
        "<style>body { font: 14px Inter } .f { float: left }
         dl { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); column-gap: 12px; margin: 0 }
         dd { margin: 0 }</style>
         <div class=f><dl class=g><dt class=wide>Ansprechpartner</dt><dd>Lindner</dd><dt>Datum</dt><dd>21.05.</dd></dl></div>",
    );
    let wide = find(&pages[0], &document, "wide")
        .expect("term")
        .2
        .rect
        .width
        .to_px();
    let grid = find(&pages[0], &document, "g")
        .expect("grid")
        .2
        .rect
        .width
        .to_px();
    assert!((grid - (2.0 * wide + 12.0)).abs() < 0.05, "{grid} {wide}");
}

#[test]
fn block_level_push_buttons_fit_their_label_unless_stretched() {
    // Water.css: `input:not([type=checkbox], [type=radio]) { display: block }`. A submit button
    // stays as wide as its label, like `<button>`; in a stretching flex column both stretch.
    let (document, pages) = render_document(
        "<style>body { font: 14px Inter } input, button { display: block; width: auto }
         .column { display: flex; flex-direction: column; width: 300px }</style>
         <input type=submit value=Send class=a><button class=b>Send</button>
         <div class=column><input type=submit value=Send class=c><button class=d>Send</button></div>",
    );
    let width = |class: &str| {
        find(&pages[0], &document, class)
            .expect("button")
            .2
            .rect
            .width
            .to_px()
    };
    assert!(width("a") < 100.0, "{}", width("a"));
    assert!(
        (width("a") - width("b")).abs() < 0.5,
        "{} {}",
        width("a"),
        width("b")
    );
    assert!((width("c") - 300.0).abs() < 0.01, "{}", width("c"));
    assert!((width("d") - 300.0).abs() < 0.01, "{}", width("d"));
}

#[test]
fn closed_details_show_only_their_summary_as_flex_containers() {
    // Water.css: `details { display: flex; flex-direction: column }`.
    let (document, pages) = render_document(
        "<style>body { font: 14px Inter } details { display: flex; flex-direction: column }</style>
         <details class=closed><summary>Old VPN</summary><p>No longer needed.</p></details>
         <details class=open open><summary>New VPN</summary><p>Use the app.</p></details>",
    );
    let text = |class: &str, needle: &str| {
        super::contains_text(find(&pages[0], &document, class).expect("details").2, needle)
    };
    assert!(text("closed", "Old VPN"));
    assert!(!text("closed", "No longer"));
    assert!(text("open", "Use the app"));
}

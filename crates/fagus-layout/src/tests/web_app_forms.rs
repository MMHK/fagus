//! Forms, dialogs and grids of web applications as Chrome prints them.

use fagus_style::values::{FontStyle, Rgba};

use super::{find, render_document};
use crate::{Fragment, FragmentKind};

fn page(html: &str) -> String {
    format!(
        "<!DOCTYPE html><style>@page {{ size: 600px 400px; margin: 0 }} body {{ margin: 0; font: 12px/16px Inter }}</style>{html}"
    )
}

/// The text runs in a fragment tree: their text, color and font style.
fn runs(fragment: &Fragment, out: &mut Vec<(String, Rgba, FontStyle)>) {
    if let FragmentKind::Text(run) = &fragment.kind {
        out.push((run.text.clone(), run.style.color, run.style.font_style));
    }
    for child in &fragment.children {
        runs(child, out);
    }
}

#[test]
fn styles_placeholders_by_their_pseudo_element() {
    let (document, pages) = render_document(&page(
        "<style>input { font: inherit }
           .a::placeholder { color: #c00; font-style: italic }
           .b::-moz-placeholder { color: #c00 }
           .c::placeholder { color: black; opacity: .5 }</style>
         <input class=a placeholder=Optional><input class=b placeholder=Gray><input class=c placeholder=Faded>
         <input class=a placeholder=Hidden value=Value>",
    ));
    let text = |class: &str| {
        let (_, _, field) = find(&pages[0], &document, class).expect(class);
        let mut out = Vec::new();
        runs(field, &mut out);
        out.into_iter().next().expect("a text run")
    };
    let (_, color, style) = text("a");
    assert_eq!(color, Rgba::from_u8(0xcc, 0, 0, 1.0));
    assert!(matches!(style, FontStyle::Italic));
    // Chrome knows no `::-moz-placeholder`: the placeholder keeps its gray.
    assert_eq!(text("b").1, Rgba::from_u8(0x75, 0x75, 0x75, 1.0));
    // Its opacity fades its text.
    assert!((text("c").1.a - 0.5).abs() < 1e-4);
}

#[test]
fn lays_out_the_pseudo_elements_of_custom_checkboxes_as_grid_items() {
    // The check mark of a checkbox without its look, centered with `display: grid` (the
    // pattern of modern-css.dev): a 10px square in the middle of a 20px box.
    let (document, pages) = render_document(&page(
        "<style>.c { appearance: none; margin: 0; width: 20px; height: 20px; border: 0; display: grid; place-content: center }
           .c::before { content: ''; width: 10px; height: 10px; background: red }</style>
         <input type=checkbox class=c checked>",
    ));
    let (_, _, check) = find(&pages[0], &document, "c").expect("checkbox");
    let mark = check.children.first().expect("the mark");
    assert_eq!(
        (
            mark.rect.x.to_px(),
            mark.rect.y.to_px(),
            mark.rect.width.to_px(),
            mark.rect.height.to_px()
        ),
        (5.0, 5.0, 10.0, 10.0)
    );
}

#[test]
fn shrinks_stretched_grid_items_to_fixed_rows() {
    // Events in a calendar's grid of fixed rows keep to their hours, their text overflowing;
    // rows sized from the content (`1fr`) keep an item as tall as its content.
    let (document, pages) = render_document(&page(
        "<style>.g { display: grid; grid-template-columns: 100px 100px; grid-template-rows: 20px 20px }
           .f { display: grid; grid-template-rows: 1fr; height: 20px; width: 100px }</style>
         <div class=g><div class=a style='grid-row: 1 / 3'>1<br>2<br>3<br>4</div>
           <div class=b style='overflow: hidden'>1<br>2</div></div>
         <div class=f><div class=c>1<br>2</div></div>",
    ));
    let height = |class: &str| {
        find(&pages[0], &document, class)
            .expect(class)
            .2
            .rect
            .height
            .to_px()
    };
    assert_eq!(height("a"), 40.0);
    assert_eq!(height("b"), 20.0);
    assert_eq!(height("c"), 32.0);
}

#[test]
fn centers_dialogs_and_hides_popovers() {
    let (document, pages) = render_document(&page(
        "<div style='position: relative; width: 400px; height: 200px'><dialog open class=d>Saved</dialog></div>
         <div popover class=p>Hidden</div>
         <div popover class=q style='display: block'>Shown</div>",
    ));
    // An open dialog is as wide as its content and centered across its containing block, with
    // a border of 3px (`border: solid`) and a padding of 1em.
    let (x, _, dialog) = find(&pages[0], &document, "d").expect("dialog");
    let width = dialog.rect.width.to_px();
    assert!(width < 100.0, "{width}");
    assert!((x + width / 2.0 - 200.0).abs() < 0.5, "{x} {width}");
    assert!(find(&pages[0], &document, "p").is_none());
    // A popover shown by its style is as tall as its content (`height: fit-content`), centered
    // on the page between its insets.
    let (_, y, popover) = find(&pages[0], &document, "q").expect("popover");
    let height = popover.rect.height.to_px();
    assert!(height < 40.0, "{height}");
    assert!((y + height / 2.0 - 200.0).abs() < 0.5, "{y} {height}");
}

/// The text runs in a fragment tree with their tops relative to the fragment, and how many
/// boxes in it have a background.
fn option_lines(fragment: &Fragment) -> (Vec<(String, f32)>, usize) {
    fn walk(fragment: &Fragment, y: f32, out: &mut (Vec<(String, f32)>, usize)) {
        let y = y + fragment.rect.y.to_px();
        match &fragment.kind {
            FragmentKind::Text(run) => out.0.push((run.text.clone(), y)),
            FragmentKind::Box { style, .. } if style.background_color.a > 0.0 => out.1 += 1,
            _ => {}
        }
        for child in &fragment.children {
            walk(child, y, out);
        }
    }
    let mut out = (Vec::new(), 0);
    for child in &fragment.children {
        walk(child, 0.0, &mut out);
    }
    out
}

#[test]
fn scrolls_list_boxes_to_their_first_selected_option() {
    // Chrome 157 scrolls a list box to its active selection end, in a `multiple` select the
    // first option selected, where it ends below the padding box: to the bottom of the padding
    // box, the bottom padding scrolling past with the options. A select without `multiple`
    // keeps only its last option with `selected` selected.
    let options = "<option>a<option>b<option>c<option>d<option>e";
    let (document, pages) = render_document(&page(&format!(
        "<style>select {{ font: inherit; vertical-align: top }}</style>
         <select class=plain multiple size=2>{options}</select>
         <select class=first multiple size=2><option>a<option selected>b<option>c<option selected>d<option>e</select>
         <select class=below multiple size=2><option>a<option>b<option>c<option selected>d<option selected>e</select>
         <select class=padded multiple size=2 style='padding-bottom: 6px'><option>a<option>b<option>c<option selected>d<option>e</select>
         <select class=single size=3><option>a<option selected>b<option>c<option selected>d<option>e</select>"
    )));
    let lines = |class: &str| option_lines(find(&pages[0], &document, class).expect(class).2);
    let top = |class: &str| lines(class).0[0].1;
    let (plain, _) = lines("plain");
    let item = plain[1].1 - plain[0].1;
    assert!(item > 10.0, "{plain:?}");
    // `b` is shown unscrolled; `d` would scroll the list two options.
    assert!((top("first") - plain[0].1).abs() < 0.01, "{:?}", lines("first"));
    assert!(
        (top("below") - (plain[0].1 - 2.0 * item)).abs() < 0.01,
        "{:?}",
        lines("below")
    );
    assert!(
        (top("padded") - (plain[0].1 - 2.0 * item + 6.0)).abs() < 0.01,
        "{:?}",
        lines("padded")
    );
    // One option on a gray ground, `d`, scrolled to the bottom of the list.
    let (single, grounds) = lines("single");
    assert_eq!(grounds, 1, "{single:?}");
    assert!((single[0].1 - (plain[0].1 - item)).abs() < 0.01, "{single:?}");
}

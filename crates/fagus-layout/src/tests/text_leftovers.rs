//! Markers, bidi isolates and font features as Chrome sets them: the spaces of inside markers,
//! which collapse with those after them, tabs in outside markers, outside markers of their own
//! direction, the markers of `::before` list items and of inside symbols before blocks, the
//! order of isolates in right-to-left text, `text-transform: math-auto`, the font properties
//! of `::first-line`, fieldsets that `text-box-trim` leaves as they are and the features of
//! `@font-face` rules.

use std::path::Path;
use std::sync::Arc;

use super::{find, render, render_document, render_document_in, test_fonts};
use crate::fragment::{Fragment, FragmentKind};
use crate::{Images, Page};

/// The text runs on `page`: their text, left edge, top, width and font size.
fn runs(page: &Page) -> Vec<(String, f32, f32, f32, f32)> {
    fn walk(fragment: &Fragment, origin: (f32, f32), out: &mut Vec<(String, f32, f32, f32, f32)>) {
        let x = origin.0 + fragment.rect.x.to_px();
        let y = origin.1 + fragment.rect.y.to_px();
        if let FragmentKind::Text(run) = &fragment.kind {
            let width: f32 = run.glyphs.iter().map(|glyph| glyph.advance).sum();
            out.push((run.text.clone(), x, y, width, run.size));
        }
        for child in &fragment.children {
            walk(child, (x, y), out);
        }
    }
    let mut out = Vec::new();
    for fragment in &page.fragments {
        walk(fragment, (0.0, 0.0), &mut out);
    }
    out
}

/// The runs that start with `text`.
fn all<'r>(runs: &'r [(String, f32, f32, f32, f32)], text: &str) -> Vec<&'r (String, f32, f32, f32, f32)> {
    runs.iter().filter(|run| run.0.starts_with(text)).collect()
}

/// The run that starts with `text`.
fn at<'r>(runs: &'r [(String, f32, f32, f32, f32)], text: &str) -> &'r (String, f32, f32, f32, f32) {
    all(runs, text)
        .first()
        .copied()
        .unwrap_or_else(|| panic!("no run {text:?} in {runs:?}"))
}

#[test]
fn collapses_the_space_after_an_inside_marker_with_the_space_of_the_item() {
    // Chrome: the marker takes the item's `white-space`, so its last space and the one the
    // item starts with are one.
    let pages = render(
        "<style>body { margin: 0; font: 16px Inter } ol { margin: 0; padding: 0;
           list-style: inside '- ' }</style>
         <ol><li> word</li><li>word</li><li>
           word</li></ol>",
    );
    let runs = runs(&pages[0]);
    let words = all(&runs, "word");
    assert_eq!(words.len(), 3, "{runs:?}");
    assert!((words[0].1 - words[1].1).abs() < 0.01, "{runs:?}");
    assert!((words[2].1 - words[1].1).abs() < 0.01, "{runs:?}");
}

#[test]
fn sets_tabs_of_outside_markers_to_tab_stops() {
    // An outside marker keeps its tabs (`white-space: pre`), which reach the stops of its box.
    let pages = render(
        "<style>body { margin: 0 0 0 100px; font: 16px Inter } ol { margin: 0; padding: 0 }
           .tab { list-style-type: 'A\\9 B' } .space { list-style-type: 'A B' }</style>
         <ol><li class=tab>x</li><li class=space>y</li></ol>",
    );
    let runs = runs(&pages[0]);
    // Eight spaces from the start of the marker, past the `A`: much wider than one space.
    let (tab, space) = (at(&runs, "A\tB"), at(&runs, "A B"));
    assert!(tab.3 - space.3 > 15.0, "{runs:?}");
    // The marker still ends where the content starts.
    assert!((tab.1 + tab.3 - at(&runs, "x").1).abs() < 0.01, "{runs:?}");
}

#[test]
fn orders_an_isolate_apart_from_the_text_after_it() {
    // Chrome: the isolate's PDI stands between the two left-to-right runs at the level of the
    // right-to-left paragraph (UAX #9, rule L2), so the isolate is rightmost.
    let pages = render(
        "<style>body { margin: 0; font: 16px Inter }</style>
         <div dir=rtl><bdi>ab</bdi>cd</div>
         <div dir=rtl><span style='unicode-bidi: isolate'>ef</span><b>gh</b></div>
         <div dir=rtl><span>ij</span><b>kl</b></div>",
    );
    let runs = runs(&pages[0]);
    assert!(at(&runs, "cd").1 < at(&runs, "ab").1, "{runs:?}");
    assert!(at(&runs, "gh").1 < at(&runs, "ef").1, "{runs:?}");
    // Without an isolate, the left-to-right text stays one run.
    assert!(at(&runs, "ij").1 < at(&runs, "kl").1, "{runs:?}");
}

#[test]
fn places_an_outside_marker_by_the_direction_of_its_item() {
    // A right-to-left marker of a left-to-right item stands left of the content, as in Chrome.
    let pages = render(
        "<style>body { margin: 0; font: 16px Inter } ol { margin: 0; padding: 0 0 0 100px }
           .rtl::marker { direction: rtl }</style>
         <ol><li class=rtl>text</li></ol><ol dir=rtl><li>text</li></ol>",
    );
    let runs = runs(&pages[0]);
    let texts = all(&runs, "text");
    assert_eq!(texts.len(), 2, "{runs:?}");
    // The runs of the first marker, ". " and "1" right to left, on the line of the first text.
    let marker: Vec<_> = runs
        .iter()
        .filter(|run| (run.2 - texts[0].2).abs() < 0.01 && !run.0.starts_with("text"))
        .collect();
    assert!(!marker.is_empty(), "{runs:?}");
    let right = marker.iter().map(|run| run.1 + run.3).fold(f32::MIN, f32::max);
    // Its right edge meets the content's left edge.
    assert!((right - texts[0].1).abs() < 0.01, "{runs:?}");
}

#[test]
fn gives_a_before_list_item_a_marker() {
    // Chrome: `::before` with `display: list-item` counts as a list item and has a marker.
    let pages = render(
        "<style>body { margin: 0; font: 16px Inter } ol { margin: 0; padding: 0 0 0 40px }
           li { display: block } li::before { content: 'b'; display: list-item;
           list-style-position: inside }</style>
         <ol><li>one</li><li>two</li></ol>",
    );
    let runs = runs(&pages[0]);
    let (one, two) = (at(&runs, "1. "), at(&runs, "2. "));
    assert!(one.2 < two.2, "{runs:?}");
    assert!(one.1 < at(&runs, "b").1, "{runs:?}");
}

#[test]
fn keeps_an_inside_symbol_marker_before_a_block_on_a_line_of_its_own() {
    // Chrome: an inside marker is content of the item, before the block in a line of its own;
    // an outside marker goes to the first line of the block.
    let (document, pages) = render_document(
        "<style>body { margin: 0; font: 16px/20px Inter } ul { margin: 0 }
           p { margin: 0 }</style>
         <ul style='list-style-position: inside'><li><p class=a>inside</p></li></ul>
         <ul><li><p class=b>outside</p></li></ul>",
    );
    let (_, a, _) = find(&pages[0], &document, "a").expect("a");
    let (_, b, _) = find(&pages[0], &document, "b").expect("b");
    // From the top of the page's content, where the first marker's line is.
    let runs = runs(&pages[0]);
    let top = at(&runs, "\u{2022}").2;
    assert!((a - top - 20.0).abs() < 0.01, "{a} {b} {runs:?}");
    assert!((b - top - 40.0).abs() < 0.01, "{a} {b} {runs:?}");
}

#[test]
fn sets_a_single_letter_in_its_mathematical_italic_form_for_math_auto() {
    let pages = render(
        "<style>body { margin: 0; font: 16px Inter } span { text-transform: math-auto }</style>
         <p><span>A</span> <span>xy</span></p>",
    );
    let runs = runs(&pages[0]);
    assert!(runs.iter().any(|run| run.0 == "\u{1D434}"), "{runs:?}");
    // Text of more letters stays as it is.
    assert!(runs.iter().any(|run| run.0.contains("xy")), "{runs:?}");
}

#[test]
fn takes_font_synthesis_and_kerning_from_the_first_line() {
    // Inter has no small capitals: `font-synthesis-small-caps: none` of `::first-line` keeps
    // them from being made up, as in Chrome.
    let pages = render(
        "<style>body { margin: 0; font: 16px Inter } p { margin: 0 }
           .none::first-line { font-variant: small-caps; font-synthesis-small-caps: none }
           .made::first-line { font-variant: small-caps }</style>
         <p class=none>Filler<br>Again</p><p class=made>Filler<br>Again</p>",
    );
    let runs = runs(&pages[0]);
    let top = runs.iter().map(|run| run.2).fold(f32::MAX, f32::min);
    let none = runs.iter().filter(|run| run.2 < top + 10.0).collect::<Vec<_>>();
    assert!(none.iter().all(|run| (run.4 - 16.0).abs() < 0.01), "{runs:?}");
    // Made up, the lower case letters are capitals set smaller.
    let made = runs
        .iter()
        .filter(|run| run.2 > top + 30.0 && run.2 < top + 50.0)
        .collect::<Vec<_>>();
    assert!(made.iter().any(|run| run.4 < 15.0), "{runs:?}");
}

#[test]
fn leaves_the_lines_of_a_fieldset_untrimmed() {
    // Chrome does not apply `text-box-trim` to the content of a fieldset.
    let (document, pages) = render_document(
        "<style>body { margin: 0; font: 20px/40px Inter }
           fieldset, div { text-box-trim: trim-both; text-box-edge: text; border: 0; margin: 0;
           padding: 0; min-inline-size: 0 }</style>
         <fieldset class=a>x</fieldset><div class=b>x</div>",
    );
    let (_, _, fieldset) = find(&pages[0], &document, "a").expect("fieldset");
    let (_, _, div) = find(&pages[0], &document, "b").expect("div");
    assert!((fieldset.rect.height.to_px() - 40.0).abs() < 0.01);
    assert!(div.rect.height.to_px() < 30.0);
}

#[test]
fn sets_text_with_the_features_of_its_font_face_rule() {
    // `font-feature-settings` of an `@font-face` rule comes before the style's own features,
    // which override it (CSS Fonts 4, §7.2): Inter's slashed zero.
    fn glyphs(fragment: &Fragment, out: &mut Vec<u32>) {
        if let FragmentKind::Text(run) = &fragment.kind {
            out.extend(run.glyphs.iter().map(|glyph| glyph.id));
        }
        fragment.children.iter().for_each(|child| glyphs(child, out));
    }
    let mut fonts = test_fonts();
    for (family, features) in [("Plain", vec![]), ("Zeroed", vec![(*b"zero", 1)])] {
        let rule = fagus_text::Descriptors {
            family: family.to_owned(),
            features,
            ..fagus_text::Descriptors::default()
        };
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fonts/Inter-Regular.ttf");
        let data = Arc::new(std::fs::read(path).expect("font"));
        fonts.add_document_face(Arc::new(
            fagus_text::Face::from_rule(data, 0, &rule).expect("face"),
        ));
    }
    let (_, pages) = render_document_in(
        "<style>body { margin: 0; font-size: 16px } p { margin: 0 }</style>
         <p style='font-family: Plain'>0</p><p style='font-family: Zeroed'>0</p>
         <p style='font-family: Zeroed; font-feature-settings: \"zero\" 0'>0</p>",
        &fonts,
        |_| Images::default(),
    );
    let mut ids = Vec::new();
    for fragment in &pages[0].fragments {
        glyphs(fragment, &mut ids);
    }
    assert_eq!(ids.len(), 3, "{ids:?}");
    assert_ne!(ids[0], ids[1]);
    assert_eq!(ids[0], ids[2]);
}

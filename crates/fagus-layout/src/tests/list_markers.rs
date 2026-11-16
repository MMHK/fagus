//! Outside list markers whose first line lies in a formatting context inside the list item, beside
//! floats of the item's own content or after blocks without lines, as Chrome places them: beside
//! the item's border box, on the first line box. The numbers are Chrome's.

use std::sync::Arc;

use super::render_document;
use crate::{Fragment, FragmentKind, Image, ImageFormat, Images, Page};

/// The runs of text on `page` with the left and top of their boxes on the page.
fn runs(page: &Page) -> Vec<(String, f32, f32)> {
    fn walk(fragment: &Fragment, origin: (f32, f32), out: &mut Vec<(String, f32, f32)>) {
        let x = origin.0 + fragment.rect.x.to_px();
        let y = origin.1 + fragment.rect.y.to_px();
        if let FragmentKind::Text(run) = &fragment.kind {
            out.push((run.text.clone(), x, y));
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

/// Where the run that starts with `text` is.
fn at(runs: &[(String, f32, f32)], text: &str) -> (f32, f32) {
    match runs.iter().find(|(run, ..)| run.starts_with(text)) {
        Some((_, x, y)) => (*x, *y),
        None => panic!("no run {text:?} in {runs:?}"),
    }
}

#[test]
fn markers_in_formatting_contexts_inside_the_item_stand_beside_it() {
    let (_, pages) = render_document(
        "<style>body { margin: 0; font: 16px Inter } ol { margin: 0; padding: 0 40px }
         li { border: 3px solid; padding-left: 7px }</style>
         <ol><li>A plain</li>
         <li><div style='display: flow-root; padding-left: 9px'>B root</div></li>
         <li><div style='display: flex; padding-left: 9px'>C flex</div></li>
         <li><table style='border-spacing: 0'><tr><td style='padding: 0 0 0 9px'>D table</td></tr></table></li>
         <li style='column-count: 2'>E columns</li>
         <li style='direction: rtl'>F rtl</li>
         <li style='direction: rtl'><div style='display: flow-root; padding-right: 9px'>G rtl root</div></li></ol>",
    );
    let runs = runs(&pages[0]);
    let plain = at(&runs, "1.").0;
    for marker in ["2.", "3.", "4.", "5."] {
        assert!((at(&runs, marker).0 - plain).abs() < 0.01, "{marker}: {runs:?}");
    }
    // Right to left, mirrored at the item's right edge, also from a formatting context inside.
    assert!((at(&runs, "7").0 - at(&runs, "6").0).abs() < 0.01, "{runs:?}");
    // Each on the line of its item's text.
    assert!((at(&runs, "2.").1 - at(&runs, "B root").1).abs() < 0.01);
    assert!((at(&runs, "3.").1 - at(&runs, "C flex").1).abs() < 0.01);
}

#[test]
fn markers_stand_beside_floats_before_the_item_but_not_its_own() {
    let (_, pages) = render_document(
        "<style>body { margin: 0; font: 16px Inter } ol { margin: 0; padding: 0 40px }
         .f { float: left; width: 30px; height: 30px }</style>
         <ol><li>Plain</li><li><div class=f></div>A own float</li></ol><div style='clear: both'></div>
         <div class=f style='width: 100px'></div><ol><li>B outer float</li></ol>",
    );
    let runs = runs(&pages[0]);
    let (marker, text) = (at(&runs, "1.").0, at(&runs, "Plain").0);
    // Chrome: the marker stands beside the item, the text beside the float.
    assert!((at(&runs, "2.").0 - marker).abs() < 0.01, "{runs:?}");
    assert!((at(&runs, "A own").0 - text - 30.0).abs() < 0.01, "{runs:?}");
    // Beside a float placed before the item, the marker moves along with the line, 60 pixels
    // past the padding.
    let outer = runs
        .iter()
        .filter(|(run, ..)| run.starts_with("1."))
        .nth(1)
        .map(|(_, x, _)| *x);
    assert!(
        outer.is_some_and(|x| (x - (marker + 60.0)).abs() < 0.01),
        "{runs:?}"
    );
}

#[test]
fn markers_pass_blocks_without_lines() {
    let (_, pages) = render_document(
        "<style>body { margin: 0; font: 16px Inter } ol { margin: 0; padding: 0 40px }</style>
         <ol><li><div style='height: 20px'></div>A after empty</li>
         <li><img style='display: block; width: 20px; height: 20px'>B after image</li>
         <li><div><div style='height: 10px; border-top: 1px solid'></div>C nested</div></li>
         <li><div style='height: 30px'></div></li><li>E</li></ol>",
    );
    let runs = runs(&pages[0]);
    for (marker, text) in [("1.", "A after"), ("2.", "B after"), ("3.", "C nested")] {
        assert!(
            (at(&runs, marker).1 - at(&runs, text).1).abs() < 0.01,
            "{marker}: {runs:?}"
        );
    }
    // Without a line, the marker stays at the top of the item, which keeps its height.
    let (empty, next) = (at(&runs, "4.").1, at(&runs, "5.").1);
    assert!((next - empty - 30.0).abs() < 0.01, "{runs:?}");
}

#[test]
fn markers_before_tables_stand_on_the_table_baseline() {
    // A table's baseline is that of its first row: without cells aligned on the baseline (`td`
    // is in the middle), the bottom of the cells' content, below their first line. A caption
    // does not take the marker. Chrome rounds the font's ascent to 16px, so its marker stands
    // 4px below the line's baseline, Fagus' 4.2px.
    let (_, pages) = render_document(
        "<style>body { margin: 0; font: 16px/20px Inter } ol { margin: 0 } li { margin-bottom: 10px }
         td { padding: 4px }</style>
         <ol><li>Plain</li>
         <li><table><tr><td>One</td></tr></table></li>
         <li><table><tr><td>Two<br>lines</td></tr></table></li>
         <li><table><caption>Caption</caption><tr><td>Three</td></tr></table></li>
         <li><table><tr><td style='vertical-align: baseline'>Four</td><td style='height: 50px'>x</td></tr></table></li></ol>",
    );
    let runs = runs(&pages[0]);
    let below = |marker: &str, text: &str| at(&runs, marker).1 - at(&runs, text).1;
    assert!((below("2.", "One") - 4.2).abs() < 0.25, "{runs:?}");
    assert!((below("3.", "lines") - 4.2).abs() < 0.25, "{runs:?}");
    assert!((below("4.", "Three") - 4.2).abs() < 0.25, "{runs:?}");
    assert!((at(&runs, "4.").0 - at(&runs, "1.").0).abs() < 0.01, "{runs:?}");
    // A cell on the baseline gives the row its baseline.
    assert!(below("5.", "Four").abs() < 0.01, "{runs:?}");
}

#[test]
fn markers_go_on_the_first_line_below_a_float_that_starts_the_item() {
    // The float that starts the block in the item takes no marker: the marker goes with the
    // first line, which a word too wide for the room beside the float moves below it (WPT
    // css-inline firefox-bug-1901624-crash with `hyphens: manual`; Chrome 157).
    let (_, pages) = render_document(
        "<style>@page { margin: 0 } body { margin: 0; font: 20px Inter } ol { margin: 0; width: 300px }
         .f { float: right; width: 40px; height: 40px }</style>
         <ol><li><div><div class=f></div>add_definitionsd.GCC_COVERAGE_COMPILE</div></li></ol>",
    );
    let runs = runs(&pages[0]);
    assert_eq!(at(&runs, "1.").1, at(&runs, "add_").1);
    assert_eq!(at(&runs, "add_").1, 40.0);
}

#[test]
fn draws_the_image_of_a_markers_content_without_a_gap() {
    fn images(fragment: &Fragment, x: f32, out: &mut Vec<(f32, f32)>) {
        let x = x + fragment.rect.x.to_px();
        if matches!(fragment.kind, FragmentKind::Image(_)) {
            out.push((x, fragment.rect.width.to_px()));
        }
        for child in &fragment.children {
            images(child, x, out);
        }
    }
    let (_, pages) = super::render_document_with(
        "<style>@page { size: 400px 300px; margin: 0 } body { margin: 0; font: 16px/20px Inter }
           ul { margin: 0; padding-left: 40px } li::marker { content: url(mark.svg) }
         </style><ul><li>item</li></ul>",
        |_| {
            let mut images = Images::default();
            images.insert(
                "mark.svg",
                Arc::new(Image {
                    data: Arc::new(Vec::new()),
                    format: ImageFormat::Svg,
                    width: 32.0,
                    height: 16.0,
                    natural: (true, true),
                    ratio: Some(2.0),
                    inline: false,
                    orientation: 1,
                    filters: Arc::from([]),
                    pad: 0,
                }),
            );
            images
        },
    );
    let mut found = Vec::new();
    for fragment in &pages[0].fragments {
        images(fragment, 0.0, &mut found);
    }
    // Outside, it ends where the text starts, as in Chrome (WPT `css-pseudo/marker-content-012`).
    assert_eq!(found, [(8.0, 32.0)], "{:?}", runs(&pages[0]));
}

#[test]
fn inside_symbol_markers_stand_on_a_line_of_their_own_before_blocks() {
    // An inside marker is inline content of the item: before a block child it makes a line of
    // its own above the block, as an inside number does, while an outside one stands beside
    // the block's first line (WPT css-multicol multicol-span-all-list-item-001; Chrome 157).
    let (_, pages) = render_document(
        "<style>@page { margin: 0 } body { margin: 0; font: 16px/20px Inter } ul { margin: 0; padding: 0 40px }
         h3 { margin: 0; font-size: 16px }</style>
         <ul><li style='list-style-position: inside'><h3>Inside</h3></li>
         <li><h3>Outside</h3></li>
         <li style='list-style-position: inside; column-count: 1'><h3 style='column-span: all'>Spanner</h3></li></ul>",
    );
    let runs = runs(&pages[0]);
    let markers: Vec<_> = runs
        .iter()
        .filter(|(text, ..)| text == "\u{2022}")
        .map(|(_, x, y)| (*x, *y))
        .collect();
    assert_eq!(markers.len(), 3, "{runs:?}");
    let (inside, outside, spanner) = (at(&runs, "Inside"), at(&runs, "Outside"), at(&runs, "Spanner"));
    // Inside, the marker is a line above the block, which starts where the outside one does.
    assert!((markers[0].1 + 20.0 - inside.1).abs() < 0.01, "{runs:?}");
    // (The bullet's box starts a pixel before its line, as in browsers.)
    assert!(
        (inside.0 - outside.0).abs() < 0.01 && markers[0].0 >= inside.0 - 1.0,
        "{runs:?}"
    );
    // Outside, it is beside the block's line.
    assert!(
        (markers[1].1 - outside.1).abs() < 0.01 && markers[1].0 < outside.0,
        "{runs:?}"
    );
    // Before a spanner too.
    assert!((markers[2].1 + 20.0 - spanner.1).abs() < 0.01, "{runs:?}");
    assert!((spanner.0 - outside.0).abs() < 0.01, "{runs:?}");
}

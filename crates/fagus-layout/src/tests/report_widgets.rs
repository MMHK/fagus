//! Charts and gauges of reports and dashboards built from boxes, as Chrome prints them: bars of
//! a flex row whose heights are percentages of the row, and progress bars without their native
//! look (Bulma's `.progress`). The numbers are Chrome's.

use super::{find, render_document};
use crate::fragment::Fragment;

/// The fragments of the boxes with the given classes: x, y and the fragment.
fn boxes<'p>(
    document: &fagus_dom::Document,
    page: &'p crate::Page,
    classes: &[&str],
) -> Vec<(f32, f32, &'p Fragment)> {
    classes
        .iter()
        .map(|class| find(page, document, class).expect(class))
        .collect()
}

#[test]
fn resolves_percentage_heights_of_flex_items_against_a_definite_row() {
    // A bar chart: columns as tall as the row, stacked bars a share of their column.
    let (document, pages) = render_document(
        "<style>@page { size: 400px 400px; margin: 0 } body { margin: 0 }
         .bars { display: flex; align-items: flex-end; gap: 4px; height: 150px }
         .col { flex: 1; display: flex; flex-direction: column; justify-content: flex-end; height: 100%; position: relative }
         i { display: block }
         em { position: absolute; left: 0; right: 0; height: 4px; bottom: calc(50% + 2px) }</style>
         <div class=bars><div class=col><em class=label></em><i class=a style='height: 30%'></i><i class=b style='height: 20%'></i></div>
         <div class=short style='height: 80%; width: 10px'></div><div class=auto style='width: 10px'>x</div></div>
         <div style='display: flex; min-height: 100px'><div class=min style='height: 50%; width: 10px'></div></div>",
    );
    let found = boxes(&document, &pages[0], &["col", "a", "b", "label", "short", "min"]);
    assert_eq!(found[0].2.rect.height.to_px(), 150.0);
    assert_eq!((found[1].1, found[1].2.rect.height.to_px()), (75.0, 45.0));
    assert_eq!((found[2].1, found[2].2.rect.height.to_px()), (120.0, 30.0));
    // Percentages of an absolutely positioned label refer to the column.
    assert_eq!(found[3].1, 150.0 - 75.0 - 2.0 - 4.0);
    // Aligned at the end of the row.
    assert_eq!((found[4].1, found[4].2.rect.height.to_px()), (30.0, 120.0));
    // A minimum height does not make the row's height definite: the percentage is `auto`.
    assert_eq!(found[5].2.rect.height.to_px(), 0.0);
}

#[test]
fn builds_progress_bars_without_their_native_look_from_their_parts() {
    let (document, pages) = render_document(
        "<style>@page { size: 400px 400px; margin: 0 } body { margin: 0; font: 16px Inter }
         progress { appearance: none; display: block; margin-bottom: 4px }
         .b { width: 300px; height: 12px; padding: 2px; border: 1px solid }
         .b::-webkit-progress-bar { background: #ddd; padding: 1px }
         .b::-webkit-progress-value { background: #36c; width: 10px }</style>
         <progress class=a value=30 max=100>30 %</progress>
         <progress class=b value=0.7>70 %</progress>
         <progress class=c>busy</progress>
         <progress class=d value=150 max=100></progress>",
    );
    let found = boxes(&document, &pages[0], &["a", "b", "c", "d"]);
    // The UA's progress is 10em by 1em; the track fills it, the value is a share of the track.
    let bar = |fragment: &Fragment| fragment.children[0].clone();
    let a = found[0].2;
    assert_eq!((a.rect.width.to_px(), a.rect.height.to_px()), (160.0, 16.0));
    let track = bar(a);
    assert_eq!(
        (track.rect.width.to_px(), track.rect.height.to_px()),
        (160.0, 16.0)
    );
    assert_eq!(track.children[0].rect.width.to_px(), 48.0);
    // The fallback text does not show.
    assert!(a.children.len() == 1 && track.children[0].children.is_empty());
    // The track fills the content box (300px less the border and padding, `border-box` in the
    // UA's style); the value's width is set inline, over the author's.
    let track = bar(found[1].2);
    assert_eq!((track.rect.x.to_px(), track.rect.width.to_px()), (3.0, 294.0));
    let value = &track.children[0];
    assert_eq!(value.rect.x.to_px(), 1.0);
    assert!(
        (value.rect.width.to_px() - 0.7 * 292.0).abs() < 0.02,
        "{:?}",
        value.rect
    );
    // Indeterminate: no value; above the maximum: full.
    assert_eq!(bar(found[2].2).children[0].rect.width.to_px(), 0.0);
    assert_eq!(bar(found[3].2).children[0].rect.width.to_px(), 160.0);
}

#[test]
fn builds_meters_from_their_parts() {
    let (document, pages) = render_document(
        "<style>@page { size: 400px 400px; margin: 0 } body { margin: 0; font: 16px Inter }
         meter { display: block; margin-bottom: 4px }
         .b { width: 200px; height: 30px; appearance: none }
         .c { width: 200px; height: 30px }
         .c::-webkit-meter-bar { height: 10px }
         .c::-webkit-meter-optimum-value { width: 25% !important }</style>
         <meter class=a value=0.6>60 %</meter>
         <meter class=b value=0.5></meter>
         <meter class=c value=0.9></meter>",
    );
    let found = boxes(&document, &pages[0], &["a", "b", "c"]);
    let bar = |fragment: &Fragment| fragment.children[0].clone();
    // The UA's meter is 5em by 1em; its track is the middle half of it, as wide, the value a
    // share of the track inside its thin border; the fallback text does not show.
    let a = found[0].2;
    assert_eq!((a.rect.width.to_px(), a.rect.height.to_px()), (80.0, 16.0));
    assert_eq!(a.children.len(), 1);
    let track = bar(a);
    assert_eq!(
        (
            track.rect.y.to_px(),
            track.rect.width.to_px(),
            track.rect.height.to_px()
        ),
        (4.0, 80.0, 8.0)
    );
    let value = &track.children[0];
    assert_eq!((value.rect.x.to_px(), value.rect.height.to_px()), (1.0, 6.0));
    assert!(
        (value.rect.width.to_px() - 0.6 * 78.0).abs() < 0.02,
        "{:?}",
        value.rect
    );
    let track = bar(found[1].2);
    assert_eq!((track.rect.y.to_px(), track.rect.height.to_px()), (7.5, 15.0));
    assert_eq!(track.children[0].rect.width.to_px(), 100.0);
    // A track of a fixed height is centered; `!important` beats the width the element sets.
    let track = bar(found[2].2);
    assert_eq!((track.rect.y.to_px(), track.rect.height.to_px()), (10.0, 10.0));
    assert!((track.children[0].rect.width.to_px() - 0.25 * 198.0).abs() < 0.02);
}

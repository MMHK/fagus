//! Boxes with an aspect ratio in flex and grid containers, percentage heights in boxes sized
//! by their content, `align-content` of lines that overflow and wrap-reverse lines, as Chrome
//! lays them out (the cases of the Web Platform Tests in `css/css-sizing` and `css/css-flexbox`
//! that Chrome passes).

use super::{find, render_document};

/// The positions and sizes of the boxes with the given classes, in CSS px, in a document that
/// starts with `doctype`.
fn boxes_in(doctype: &str, html: &str, classes: &[&str]) -> Vec<(f32, f32, f32, f32)> {
    let (document, pages) = render_document(&format!(
        "{doctype}<style>@page {{ margin: 0 }} body {{ margin: 0; font: 12px/16px Inter }}</style>{html}"
    ));
    classes
        .iter()
        .map(|class| {
            let (x, y, fragment) = find(&pages[0], &document, class).expect(class);
            (x, y, fragment.rect.width.to_px(), fragment.rect.height.to_px())
        })
        .collect()
}

fn boxes(html: &str, classes: &[&str]) -> Vec<(f32, f32, f32, f32)> {
    boxes_in("<!DOCTYPE html>", html, classes)
}

fn sizes(html: &str, classes: &[&str]) -> Vec<(f32, f32)> {
    boxes(html, classes)
        .into_iter()
        .map(|(_, _, width, height)| (width, height))
        .collect()
}

#[test]
fn normal_alignment_keeps_the_ratio_of_grid_items() {
    // `normal` does not stretch a grid item's height that follows its width through the
    // ratio, nor its width where its height is its own or stretched explicitly
    // (`grid-aspect-ratio-001`, `-007`, `-010`, `-028`, `-030`, `-038` in WPT).
    let html =
        "<div style='display: grid'><div class=auto-width style='height: 100px; aspect-ratio: 1'></div></div>
                <div style='display: grid; grid-template-rows: 200px'>
                  <div class=fixed-width style='width: 100px; aspect-ratio: 1'></div></div>
                <div style='display: grid; grid-template: 100px / 200px'>
                  <div class=stretched style='aspect-ratio: 1; align-self: stretch'></div></div>
                <div style='display: grid; grid-template: 200px / 100px'>
                  <div class=justified style='aspect-ratio: 1; justify-self: stretch'></div></div>
                <div style='display: grid; grid-template: 50px / 100px'>
                  <div class=overflowing style='aspect-ratio: 1'></div></div>";
    assert_eq!(
        sizes(
            html,
            &[
                "auto-width",
                "fixed-width",
                "stretched",
                "justified",
                "overflowing"
            ]
        ),
        [
            (100.0, 100.0),
            (100.0, 100.0),
            (100.0, 100.0),
            (100.0, 100.0),
            (100.0, 100.0)
        ]
    );
}

#[test]
fn flex_items_stretched_to_a_definite_height_take_their_width_from_it() {
    // Through an item's ratio and through the percentage heights inside it, also where the
    // container is sized by its items (`aspect-ratio-intrinsic-size-*`, `flex-aspect-ratio-045`).
    let html = "<div class=ratio style='display: inline-flex; height: 100px'>
                  <div style='aspect-ratio: 1'></div></div>
                <div class=framed style='display: inline-flex; height: 100px'>
                  <div style='padding: 3px; border: 7px solid; margin: 10px; aspect-ratio: 1'></div></div>
                <div class=inside style='display: inline-flex; height: 100px'>
                  <div><canvas width=10 height=10 style='height: 100%; display: block'></canvas></div></div>
                <div style='display: flex; width: 0; height: 100px'>
                  <div class=narrow style='aspect-ratio: 1'></div></div>";
    assert_eq!(
        sizes(html, &["ratio", "framed", "inside", "narrow"]),
        [(100.0, 100.0), (100.0, 100.0), (100.0, 100.0), (100.0, 100.0)]
    );
}

#[test]
fn content_based_flex_bases_follow_the_ratio() {
    // `flex-basis: content` of a replaced item stretched to the line is its width through its
    // ratio, whatever its `width`; in a column it is the height its content gives, whatever its
    // `height` (`flexbox-flex-basis-content-*`, `flex-aspect-ratio-022`).
    let html = "<div style='display: flex; height: 54px'>
                  <canvas class=canvas width=20 height=150 style='flex: 0 0 content; min-width: 0;
                                                                  width: 8px; border: 2px solid'></canvas></div>
                <div style='display: flex; flex-direction: column'>
                  <div class=column style='flex-basis: content; width: 100px; aspect-ratio: 1;
                                           height: 20px; min-height: 0'></div></div>";
    let sizes = sizes(html, &["canvas", "column"]);
    assert_eq!(sizes[0].1, 54.0);
    assert!(
        (sizes[0].0 - (50.0 * 20.0 / 150.0 + 4.0)).abs() < 0.1,
        "{sizes:?}"
    );
    assert_eq!(sizes[1], (100.0, 100.0));
}

#[test]
fn a_flex_container_with_a_ratio_is_no_shorter_than_its_items() {
    // The automatic minimum size of a box with a ratio, within its maximum
    // (`flex-aspect-ratio-040`, `-043`); items stretched through their own ratio do not count
    // (`flex-aspect-ratio-cross-size-002`).
    let html = "<div class=grows style='display: flex; width: 100px; aspect-ratio: 2'>
                  <div style='width: 100%; height: 100px'></div></div>
                <div class=bounded style='display: flex; width: 100px; aspect-ratio: 2; max-height: 70px'>
                  <div style='flex: 1; height: 200px'></div></div>
                <div style='width: 200px'><div class=nested style='display: flex; aspect-ratio: 4'>
                  <div style='display: flex; aspect-ratio: 1'><div>
                    <div style='height: 100%; aspect-ratio: 2'></div></div></div></div></div>";
    assert_eq!(
        sizes(html, &["grows", "bounded", "nested"]),
        [(100.0, 100.0), (100.0, 70.0), (200.0, 50.0)]
    );
}

#[test]
fn percentage_heights_inside_boxes_sized_by_their_content_refer_to_definite_heights() {
    // A sizing keyword's width (`intrinsic-percent-replaced-012`), a flex item of a percentage
    // height (`-004`, `-025`) and, in quirks mode, boxes of an automatic height that pass their
    // containing block's on (`-006`).
    let html = "<div style='width: 25px; height: 50px'>
                  <div class=keyword style='box-sizing: border-box; width: max-content; height: 100%;
                                            padding-top: 100%'>
                    <canvas width=40 height=10 style='height: 100%; display: block'></canvas></div></div>
                <div style='display: flex; height: 200px'>
                  <div class=item style='height: 50%'><canvas width=10 height=10
                    style='height: 100%; display: block'></canvas></div></div>";
    let sizes = sizes(html, &["keyword", "item"]);
    assert_eq!(sizes, [(100.0, 50.0), (100.0, 100.0)]);
    let quirky = "<div style='height: 100px'><div class=float style='float: left'><div>
                    <canvas width=10 height=10 style='height: 100%'></canvas></div></div></div>
                  <div style='display: flex; height: 200px'>
                    <div class=item style='height: 50%'><div><canvas width=10 height=10
                      style='height: 100%; display: block'></canvas></div></div></div>";
    let found = boxes_in("", quirky, &["float", "item"]);
    assert_eq!((found[0].2, found[0].3), (100.0, 100.0));
    assert_eq!((found[1].2, found[1].3), (100.0, 100.0));
}

#[test]
fn overflowing_lines_follow_align_content() {
    // Lines taller than the container move by `align-content` past its start, unless it is
    // `safe` or distributes them; `wrap-reverse` turns `flex-start` over but not `start`.
    let lines = |style: &str| {
        let html = format!(
            "<div style='display: flex; flex-wrap: wrap; width: 90px; height: 60px; {style}'>
               <div class=first style='width: 60px; height: 20px'></div>
               <div style='width: 60px; height: 20px'></div><div style='width: 60px; height: 20px'></div>
               <div class=last style='width: 60px; height: 20px'></div></div>"
        );
        let found = boxes(&html, &["first", "last"]);
        (found[0].1, found[1].1)
    };
    assert_eq!(lines("align-content: center"), (-10.0, 50.0));
    assert_eq!(lines("align-content: flex-end"), (-20.0, 40.0));
    assert_eq!(lines("align-content: end"), (-20.0, 40.0));
    assert_eq!(lines("align-content: safe center"), (0.0, 60.0));
    assert_eq!(lines("align-content: space-around"), (0.0, 60.0));
    assert_eq!(lines("align-content: space-between"), (0.0, 60.0));
    // Reversed: the first line at the bottom.
    assert_eq!(
        lines("flex-wrap: wrap-reverse; align-content: flex-start"),
        (40.0, -20.0)
    );
    assert_eq!(
        lines("flex-wrap: wrap-reverse; align-content: start"),
        (60.0, 0.0)
    );
    assert_eq!(
        lines("flex-wrap: wrap-reverse; align-content: space-around"),
        (60.0, 0.0)
    );
    // Lines shorter than the container: `flex-start` reversed is at the bottom, `end` too.
    let short = |style: &str| {
        let html = format!(
            "<div style='display: flex; flex-wrap: wrap-reverse; width: 90px; height: 60px; {style}'>
               <div class=first style='width: 60px; height: 10px'></div>
               <div class=last style='width: 60px; height: 10px'></div></div>"
        );
        let found = boxes(&html, &["first", "last"]);
        (found[0].1, found[1].1)
    };
    assert_eq!(short("align-content: flex-start"), (50.0, 40.0));
    assert_eq!(short("align-content: end"), (50.0, 40.0));
    assert_eq!(short("align-content: start"), (10.0, 0.0));
}

#[test]
fn column_items_align_baselines_and_flex_lines_as_chrome() {
    // A column cannot share baselines: `baseline` is `flex-start`, `last baseline` `flex-end`,
    // and `wrap-reverse` turns those over (`align-baseline`).
    let column = |wrap: &str, align: &str| {
        let html = format!(
            "<div style='display: flex; flex-direction: column; width: 120px; flex-wrap: {wrap}'>
               <div class=item style='width: 40px; height: 10px; align-self: {align}'></div></div>"
        );
        boxes(&html, &["item"])[0].0
    };
    assert_eq!(column("nowrap", "baseline"), 0.0);
    assert_eq!(column("nowrap", "last baseline"), 80.0);
    assert_eq!(column("wrap-reverse", "baseline"), 80.0);
    assert_eq!(column("wrap-reverse", "flex-start"), 80.0);
    assert_eq!(column("wrap-reverse", "flex-end"), 0.0);
    assert_eq!(column("wrap-reverse", "start"), 0.0);
    assert_eq!(column("wrap-reverse", "last baseline"), 0.0);
}

#[test]
fn wrap_reverse_rows_keep_baselines_where_they_are() {
    // Items aligned by their first baselines line up at the top of a reversed line, by their
    // last ones at its bottom (`flexbox-align-self-baseline-horiz-001b`).
    let row = |align: &str| {
        let html = format!(
            "<div style='display: flex; flex-wrap: wrap-reverse; align-items: {align}'>
               <div class=small>x</div><div class=big style='font-size: 24px; line-height: 30px;
                                                            height: 100px'>x</div></div>"
        );
        let found = boxes(&html, &["small", "big"]);
        (found[0].1, found[1].1)
    };
    // First baselines: the small item's 16px line centred on the baseline of the big one's
    // 30px line.
    let (small, big) = row("baseline");
    assert_eq!(big, 0.0);
    assert!(small > 0.0 && small < 15.0, "{small}");
    // Last baselines: both at the bottom of the 100px line, the big item's text at its top.
    let (small, big) = row("last baseline");
    assert_eq!(big, 0.0);
    assert!(small > 0.0 && small < 15.0, "{small}");
}

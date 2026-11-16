//! Aspect ratios of boxes that are not replaced, flex items in columns whose height is their
//! content's, intrinsic sizes of flex rows, `justify-content` where items overflow, and the
//! sizing keywords of replaced elements, as Chrome lays them out (the cases of the Web
//! Platform Tests in `css/css-sizing` and `css/css-flexbox` that Chrome passes).

use super::{find, render_document};

/// The positions and sizes of the boxes with the given classes, in CSS px.
fn boxes(html: &str, classes: &[&str]) -> Vec<(f32, f32, f32, f32)> {
    let (document, pages) = render_document(&format!(
        "<!DOCTYPE html><style>@page {{ margin: 0 }} body {{ margin: 0; font: 12px/16px Inter }}</style>{html}"
    ));
    classes
        .iter()
        .map(|class| {
            let (x, y, fragment) = find(&pages[0], &document, class).expect(class);
            (x, y, fragment.rect.width.to_px(), fragment.rect.height.to_px())
        })
        .collect()
}

fn sizes(html: &str, classes: &[&str]) -> Vec<(f32, f32)> {
    boxes(html, classes)
        .into_iter()
        .map(|(_, _, width, height)| (width, height))
        .collect()
}

#[test]
fn boxes_with_a_ratio_and_a_definite_height_are_as_wide_in_intrinsic_sizes() {
    let html = "<div class=min style='width: min-content; height: 100px'>
                  <div style='height: 100px; aspect-ratio: 1/1'></div></div>
                <div class=max style='width: max-content; height: 25px'>
                  <div style='height: 100px; max-height: 25px; aspect-ratio: 4/1;
                              box-sizing: border-box; padding-top: 15px'></div></div>
                <div class=chain style='width: min-content; height: 100px'>
                  <div style='height: 100%'><div style='height: 100%; aspect-ratio: 1/1'></div></div></div>
                <div class=floor style='height: 25px; width: min-content; aspect-ratio: 1/1'>
                  <div style='width: 100px'></div></div>";
    assert_eq!(
        sizes(html, &["min", "max", "chain", "floor"]),
        [(100.0, 100.0), (100.0, 25.0), (100.0, 100.0), (100.0, 25.0)]
    );
}

#[test]
fn blocks_take_their_width_from_their_height_through_the_ratio() {
    let html = "<div class=border-box style='height: 50px; aspect-ratio: 2/1; box-sizing: border-box;
                                             padding-top: 25px'></div>
                <div class=content-box style='height: 50px; aspect-ratio: auto 4/1; box-sizing: border-box;
                                              padding-top: 25px'></div>
                <div class=bounded style='height: 300px; max-height: 25px; aspect-ratio: 4/1'></div>
                <div class=content style='height: 100px; aspect-ratio: 1/2'><div style='width: 100px'></div></div>
                <div style='height: 100px'>
                  <div class=transferred style='aspect-ratio: 1/1; max-height: 100%'>x</div></div>";
    assert_eq!(
        sizes(
            html,
            &["border-box", "content-box", "bounded", "content", "transferred"]
        ),
        [
            (100.0, 50.0),
            (100.0, 50.0),
            (100.0, 25.0),
            (100.0, 100.0),
            (100.0, 100.0)
        ]
    );
}

#[test]
fn absolutely_positioned_boxes_with_a_ratio() {
    let html = "<div style='position: relative; width: 100px; height: 500px'>
                  <div class=insets style='position: absolute; inset: 0; aspect-ratio: 1/1'></div></div>
                <div style='position: relative; width: 200px'>
                  <div class=bounded style='position: absolute; inset: 0; max-height: 100px; aspect-ratio: 1/1'></div></div>
                <div style='position: relative'>
                  <div class=minimum style='position: absolute; width: 100px; min-height: 0; aspect-ratio: 1/1'>
                    <div style='height: 200px'></div></div></div>
                <div style='position: relative; width: 100px; height: 200px'>
                  <canvas class=stretch width=10 height=10
                          style='position: absolute; width: 100%; height: stretch; bottom: 100px'></canvas></div>";
    assert_eq!(
        sizes(html, &["insets", "bounded", "minimum", "stretch"]),
        [(100.0, 100.0), (100.0, 100.0), (100.0, 100.0), (100.0, 100.0)]
    );
}

#[test]
fn margins_collapse_through_heights_that_behave_as_auto() {
    for height in ["100%", "min-content", "max-content", "fit-content", "stretch"] {
        let html = format!(
            "<div class=outer style='height: {height}'><div style='height: 100px; margin-bottom: 100px'></div></div>
             <div class=after style='height: 10px'></div>"
        );
        let found = boxes(&html, &["outer", "after"]);
        assert_eq!((found[0].3, found[1].1), (100.0, 200.0), "height: {height}");
    }
}

#[test]
fn items_of_columns_whose_height_is_their_content_take_their_flex_basis() {
    let html = "<div style='display: flex; flex-direction: column; width: 100px'>
                  <div class=basis style='flex: 0 0 60px; height: 0'><div class=percent style='height: 100%'></div></div>
                  <div class=bounded style='flex: 1 0 30px; max-height: 20px'></div>
                  <div class=indefinite style='flex: 1 1 0%; height: 500px'><div style='height: 40px'></div>
                    <div class=unresolved style='height: 100%'></div></div>
                </div>";
    let found = sizes(html, &["basis", "percent", "bounded", "indefinite", "unresolved"]);
    assert_eq!(
        found.iter().map(|size| size.1).collect::<Vec<_>>(),
        [60.0, 60.0, 20.0, 40.0, 0.0]
    );
}

#[test]
fn column_items_shrink_to_their_content_size_suggestion() {
    let html = "<div style='display: flex; flex-direction: column; width: 100px; height: 100px'>
                  <div class=a style='height: 50px'>1</div><div class=b style='height: 50px'>2</div>
                  <div class=c style='height: 50px'>3</div><div class=d style='height: 50px'>4</div>
                </div>
                <div style='display: flex; flex-direction: column; width: 100px; height: 200px'>
                  <div class=keyword style='max-height: min-content; flex-basis: 200px'><div style='height: 100px'></div></div>
                </div>";
    let found = sizes(html, &["a", "b", "c", "d", "keyword"]);
    assert_eq!(
        found.iter().map(|size| size.1).collect::<Vec<_>>(),
        [25.0, 25.0, 25.0, 25.0, 100.0]
    );
}

#[test]
fn flex_rows_hold_items_that_cannot_shrink_below_their_basis() {
    let html = "<div class=row style='display: flex; height: 10px; float: left'><div style='flex: 1 0 100px'></div></div>
                <div style='clear: both; width: 0'>
                  <div class=narrow style='display: flex; height: 10px; float: left'><div style='flex: 1 0 100px'></div></div></div>";
    assert_eq!(sizes(html, &["row", "narrow"]), [(100.0, 10.0), (100.0, 10.0)]);
}

#[test]
fn justify_content_aligns_items_that_overflow_the_line() {
    let html = "<div style='display: flex; width: 100px; justify-content: center'><div class=center style='flex: none; width: 200px; height: 10px'></div></div>
                <div style='display: flex; width: 100px; justify-content: space-between; flex-direction: row-reverse'>
                  <div class=reverse style='flex: none; width: 300px; height: 10px'></div></div>
                <div style='display: flex; width: 100px; justify-content: space-around'><div class=around style='flex: none; width: 200px; height: 10px'></div></div>
                <div style='display: flex; flex-direction: column-reverse; justify-content: space-between; height: 50px'>
                  <div class=column style='flex: none; height: 300px'></div></div>";
    let found = boxes(html, &["center", "reverse", "around", "column"]);
    assert_eq!(found[0].0, -50.0);
    assert_eq!(found[1].0, -200.0);
    assert_eq!(found[2].0, 0.0);
    // The column ends at the container's bottom (30px down: 3 rows of 10px before it).
    assert_eq!(found[3].1, 30.0 + 50.0 - 300.0);
}

#[test]
fn row_gap_percentages_refer_to_a_definite_height() {
    let html = "<div style='display: flex; flex-wrap: wrap; height: 100px; gap: 50% 0'>
                  <div class=first style='width: 100%'></div><div class=second style='width: 100%'></div></div>
                <div style='display: flex; flex-wrap: wrap; gap: 50% 0'>
                  <div class=third style='width: 100%; height: 10px'></div><div class=fourth style='width: 100%; height: 10px'></div></div>";
    let found = boxes(html, &["first", "second", "third", "fourth"]);
    assert_eq!(found[1].1 - (found[0].1 + found[0].3), 50.0);
    assert_eq!(found[3].1 - (found[2].1 + found[2].3), 0.0);
}

#[test]
fn infinite_flex_factors_take_all_the_free_space() {
    let html = "<div style='display: flex; width: 100px; height: 10px'>
                  <div class=infinite style='flex: calc(infinity) 0 0px'></div><div class=one style='flex: 1 0 0px'></div></div>";
    assert_eq!(sizes(html, &["infinite", "one"]), [(100.0, 10.0), (0.0, 10.0)]);
}

#[test]
fn grid_min_content_takes_the_minimum_contributions_of_scroll_containers() {
    let html = "<div class=grid style='display: grid; width: min-content; grid-auto-flow: column; height: 100px'>
                  <div style='height: 100px; aspect-ratio: 2/1; overflow: auto'></div>
                  <div style='height: 100px; aspect-ratio: 1/2; min-width: 100px; overflow: auto'></div></div>";
    assert_eq!(sizes(html, &["grid"]), [(100.0, 100.0)]);
}

#[test]
fn content_keywords_bound_replaced_elements_by_their_automatic_sizes() {
    let html = "<canvas class=max-height width=10 height=10 style='width: 100px; height: 1000px; max-height: min-content'></canvas>
                <canvas class=min-height width=10 height=10 style='width: 100px; height: 10px; min-height: min-content'></canvas>
                <canvas class=max-width width=10 height=10 style='width: 1000px; height: 100px; max-width: min-content'></canvas>
                <div class=contained style='contain: size; contain-intrinsic-inline-size: 30px;
                                            contain-intrinsic-block-size: 20px; display: inline-block'></div>";
    assert_eq!(
        sizes(html, &["max-height", "min-height", "max-width", "contained"]),
        [(100.0, 100.0), (100.0, 100.0), (100.0, 100.0), (30.0, 20.0)]
    );
}

#[test]
fn replaced_flex_items_have_minimums_through_their_ratio() {
    use crate::{Image, ImageFormat, Images};
    use std::sync::Arc;
    // A 16 × 16 image shrinks in a column to the height its width gives, and in a row to the
    // width its minimum height gives, also below its flex basis (Chrome 157).
    let html = "<!DOCTYPE html><style>body { margin: 0; font: 12px/16px Inter }</style>
        <div style='display: flex; flex-direction: column; height: 1px; float: left'>
          <img class=column style='height: 100px; width: 30px' src=dot.png></div>
        <div style='display: flex; width: 0px; clear: both'>
          <img class=row style='width: 100px; min-height: 30px' src=dot.png></div>
        <div style='display: flex; width: 0px'>
          <img class=basis style='flex-basis: 10px; min-height: 30px' src=dot.png></div>";
    let (document, pages) = super::render_document_with(html, |_| {
        let mut images = Images::default();
        images.insert(
            "dot.png",
            Arc::new(Image {
                data: Arc::new(Vec::new()),
                format: ImageFormat::Png,
                width: 16.0,
                height: 16.0,
                natural: (true, true),
                ratio: Some(1.0),
                inline: false,
                orientation: 1,
                filters: Arc::from([]),
                pad: 0,
            }),
        );
        images
    });
    for class in ["column", "row", "basis"] {
        let (_, _, fragment) = find(&pages[0], &document, class).expect(class);
        assert_eq!(
            (fragment.rect.width.to_px(), fragment.rect.height.to_px()),
            (30.0, 30.0),
            "{class}"
        );
    }
}

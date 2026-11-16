//! Grid and flex items whose sizes or positions depend on their tracks, alignment and
//! intrinsic sizes, as Chrome lays them out (the cases of the Web Platform Tests in
//! `css/css-grid`, `css/css-align` and `css/css-sizing` that Chrome passes).

use std::sync::Arc;

use super::{find, render_document_with};
use crate::{Image, ImageFormat, Images};

/// The positions and sizes of the boxes with the given classes, in CSS px.
fn boxes(html: &str, classes: &[&str]) -> Vec<(f32, f32, f32, f32)> {
    boxes_with(html, classes, Images::default())
}

/// Like [`boxes`], with `images`.
fn boxes_with(html: &str, classes: &[&str], images: Images) -> Vec<(f32, f32, f32, f32)> {
    let (document, pages) = render_document_with(
        &format!(
            "<!DOCTYPE html><style>@page {{ margin: 0 }} body {{ margin: 0; font: 12px/16px Inter }}</style>{html}"
        ),
        |_| images,
    );
    classes
        .iter()
        .map(|class| {
            let (x, y, fragment) = find(&pages[0], &document, class).expect(class);
            (x, y, fragment.rect.width.to_px(), fragment.rect.height.to_px())
        })
        .collect()
}

#[test]
fn percentage_minimum_heights_contribute_nothing_to_auto_rows() {
    // An item whose height is automatic or a percentage and whose `min-height` is a length
    // contributes that minimum, percentages of nothing, to an `auto` row: a grid 40px tall
    // gives the row the 25px left, which the item's percentages then refer to
    // (`grid-item-percentage-sizes-*` in WPT).
    let html = "<div style='display: grid; height: 40px; width: 40px; grid: 5px auto 10px / 5px auto 10px; place-items: start'>
          <div class=both style='grid-area: 2 / 2; height: 100%; min-height: 100%'><div style='height: 60px'></div></div></div>
        <div style='display: grid; height: 40px; width: 40px; grid: 5px auto 10px / 5px auto 10px; place-items: start'>
          <div class=fixed style='grid-area: 2 / 2; height: 100%; min-height: 10px'><div style='height: 60px'></div></div></div>
        <div style='display: grid; height: 40px; width: 40px; grid: 5px auto 10px / 5px auto 10px; place-items: start'>
          <div class=auto style='grid-area: 2 / 2; min-height: 100%'><div style='height: 60px'></div></div></div>";
    let found = boxes(html, &["both", "fixed", "auto"]);
    assert_eq!(found[0].3, 25.0);
    assert_eq!(found[1].3, 25.0);
    // An automatic height is its content's, overflowing the row.
    assert_eq!(found[2].3, 60.0);
}

#[test]
fn vertical_grid_items_take_their_width_from_the_ratio() {
    // A vertical item's width is its block size: with a height of its own and a ratio, `normal`
    // does not stretch it across its column (`grid-aspect-ratio-011`, `-012` in WPT).
    let html = "<div style='display: grid; grid-template-columns: 200px'>
          <div class=fixed style='height: 100px; aspect-ratio: 1; writing-mode: vertical-lr'></div></div>
        <div style='display: grid; grid-template-columns: 200px; grid-template-rows: 200px'>
          <div class=percent style='height: 50%; aspect-ratio: 2; writing-mode: vertical-rl'></div></div>";
    let found = boxes(html, &["fixed", "percent"]);
    assert_eq!((found[0].2, found[0].3), (100.0, 100.0));
    assert_eq!((found[1].2, found[1].3), (200.0, 100.0));
}

#[test]
fn wrapping_columns_are_as_wide_as_their_flexed_ratio_items() {
    // An item whose width follows its height through a ratio widens its line by the height it
    // flexes to (`flex-aspect-ratio-013`, `-014` in WPT).
    let html = "<div class=column style='display: inline-flex; flex-direction: column; flex-wrap: wrap; height: 100px'>
          <div class=item style='aspect-ratio: 1; min-height: 0; height: 50px; flex: 1'></div></div>
        <div><div class=basis style='display: inline-flex; flex-direction: column; flex-wrap: wrap; height: 100px'>
          <div style='aspect-ratio: 1; min-height: 0; flex: 1 1 50px'></div></div></div>";
    let found = boxes(html, &["column", "item", "basis"]);
    assert_eq!((found[0].2, found[0].3), (100.0, 100.0));
    assert_eq!((found[1].2, found[1].3), (100.0, 100.0));
    assert_eq!(found[2].2, 100.0);
}

#[test]
fn percentage_wide_images_without_a_size_count_as_the_default_width() {
    // An SVG with a ratio only is as wide as its container allows; with a percentage width it
    // counts as 300px wide in intrinsic sizes, as in Chrome, and without one as nothing
    // (`svg-intrinsic-size-006` in WPT).
    // An SVG image with a `viewBox` alone: a ratio, no natural size.
    let mut images = Images::default();
    images.insert(
        "ratio.svg",
        Arc::new(Image {
            data: Arc::new(Vec::new()),
            format: ImageFormat::Svg,
            width: 4.0,
            height: 1.0,
            natural: (false, false),
            ratio: Some(4.0),
            inline: false,
            orientation: 1,
            filters: Arc::from([]),
            pad: 0,
        }),
    );
    let html = "<div class=percent style='width: max-content; line-height: 0'><img src=ratio.svg style='width: 10%'></div>
        <div class=auto style='width: max-content; line-height: 0'><img src=ratio.svg></div>
        <table class=table style='border-spacing: 0'><tr><td style='padding: 0; line-height: 0'><img src=ratio.svg style='width: 100%'></td></tr></table>";
    let found = boxes_with(html, &["percent", "auto", "table"], images);
    assert_eq!(found[0].2, 300.0);
    assert_eq!(found[1].2, 0.0);
    assert_eq!(found[2].2, 300.0);
}

#[test]
fn stretched_fixed_tables_keep_their_fixed_layout() {
    // `width: stretch` is not automatic: the table lays out fixed, as wide as its container
    // however wide its content (`css-sizing/stretch/fixed-table-1` in WPT).
    let html = "<div style='width: 100px'><div class=table style='display: table; table-layout: fixed; width: stretch'>
          <div style='display: table-cell; width: 100px'><div style='width: 200px; height: 10px'></div></div></div></div>";
    assert_eq!(boxes(html, &["table"])[0].2, 100.0);
}

#[test]
fn stretched_flex_items_transfer_their_height_through_the_ratio() {
    // `height: stretch` is a definite height whatever the alignment: a replaced item's automatic
    // minimum width follows it through its ratio (`flexbox-auto-minimum-002` in WPT).
    let html = "<div style='display: flex; height: 200px; width: 200px'>
          <canvas class=item width=100 height=100 style='height: stretch; align-self: start; flex-basis: 0'></canvas></div>";
    let found = boxes(html, &["item"]);
    assert_eq!((found[0].2, found[0].3), (200.0, 200.0));
}

#[test]
fn logical_intrinsic_sizes_follow_the_writing_mode() {
    // `contain-intrinsic-inline-size` is the height of a vertical box (`contain-intrinsic-size-
    // logical-002` in WPT), and the physical sizes stay physical (`contain-intrinsic-size-008`).
    let html = "<div><div class=logical style='contain: size; contain-intrinsic-inline-size: 50px; contain-intrinsic-block-size: 100px; writing-mode: vertical-lr'></div></div>
        <div style='width: max-content'><div class=physical style='contain: size; contain-intrinsic-size: 55px 66px; writing-mode: vertical-rl'>Lorem</div></div>";
    let found = boxes(html, &["logical", "physical"]);
    assert_eq!((found[0].2, found[0].3), (100.0, 50.0));
    assert_eq!((found[1].2, found[1].3), (55.0, 66.0));
}

#[test]
fn stretch_bounds_fill_the_room_beside_floats() {
    // A box that avoids floats with `max-width: stretch` fits in the room beside a float rather
    // than moving below it (`css-sizing/stretch/bfc-next-to-float-2` in WPT).
    let html = "<div style='width: 200px'><div style='float: left; width: 100px; height: 75px'></div>
          <div class=min style='display: flow-root; height: 25px; width: 0; min-width: stretch'></div>
          <div class=max style='display: flow-root; height: 25px; width: 1000px; max-width: stretch'></div></div>";
    let found = boxes(html, &["min", "max"]);
    assert_eq!(found[0], (100.0, 0.0, 100.0, 25.0));
    assert_eq!(found[1], (100.0, 25.0, 100.0, 25.0));
}

#[test]
fn safe_alignment_starts_overflowing_static_positions() {
    // An absolutely positioned child of a flex container that is its containing block, at its
    // static position and larger than the container, starts at the cross start under `safe
    // center` (`flex-abspos-staticpos-align-self-safe-001` in WPT); without `safe` it is
    // centered.
    let html = "<div style='display: flex; width: 50px; height: 50px; position: relative'>
          <div class=safe style='position: absolute; width: 65px; height: 65px; align-self: safe center'></div></div>
        <div style='display: flex; flex-direction: column; width: 50px; height: 50px; position: relative'>
          <div class=column style='position: absolute; width: 65px; height: 65px; align-self: safe center'></div></div>
        <div style='display: flex; width: 50px; height: 50px; position: relative'>
          <div class=unsafe style='position: absolute; width: 65px; height: 65px; align-self: center'></div></div>";
    let found = boxes(html, &["safe", "column", "unsafe"]);
    assert_eq!((found[0].0, found[0].1), (0.0, 0.0));
    assert_eq!((found[1].0, found[1].1), (0.0, 50.0));
    assert_eq!((found[2].0, found[2].1), (0.0, 92.5));
}

#[test]
fn row_subgrids_place_their_items_in_the_rows_they_span_for_their_widths() {
    // A subgrid in rows that flows its items by columns has as many rows as it spans also where
    // its parent finds its widths: two items stack in one column (`subgrid/standalone-axis-size-
    // 007` in WPT).
    let html = "<div class=grid style='display: inline-grid'>
          <div class=subgrid style='display: grid; grid-row: span 2; grid-auto-flow: column; grid-template-rows: subgrid'>
            <div style='width: 100px; height: 50px'></div><div class=second style='width: 100px; height: 50px'></div></div></div>";
    let found = boxes(html, &["grid", "subgrid", "second"]);
    assert_eq!((found[0].2, found[0].3), (100.0, 100.0));
    assert_eq!((found[1].2, found[1].3), (100.0, 100.0));
    assert_eq!((found[2].0, found[2].1), (0.0, 50.0));
}

#[test]
fn percentages_in_column_flex_items_follow_definite_flexed_heights() {
    // A content-based flex basis in a column whose height is its content's leaves the item's
    // height indefinite, whatever its `height`: percentages inside are `auto`
    // (`percentage-heights-016` in WPT). A column stretched to a definite height gives its
    // items definite heights, which percentages inside refer to (`percentage-heights-019`).
    let html = "<div style='display: flex; flex-direction: column; width: 100px'>
          <div style='flex: 1 1 content; height: 100px; min-height: 0'>
            <div style='height: 100px'></div><div class=content-basis style='height: 100%'></div></div></div>
        <div style='display: flex'><div style='display: flex; flex-direction: column; width: 100px'>
          <div style='overflow: hidden; min-height: 100px'><div class=stretched style='height: 100%'></div></div></div></div>";
    let found = boxes(html, &["content-basis", "stretched"]);
    assert_eq!(found[0].3, 0.0);
    assert_eq!(found[1].3, 100.0);
}

#[test]
fn baseline_justified_items_fall_back_by_their_baseline_group() {
    // Items aligned by their baselines across a column fall in the group at its start or at its
    // end by the way their lines stack: in a right-to-left grid a `vertical-rl` item starts on
    // the right and a `vertical-lr` one on the left, at the end (`grid-justify-baseline-001`).
    let html = "<div style='display: grid; width: 100px; direction: rtl; justify-items: baseline'>
          <div class=rl style='writing-mode: vertical-rl; width: 20px; height: 30px'></div>
          <div class=lr style='writing-mode: vertical-lr; width: 20px; height: 30px'></div></div>";
    let found = boxes(html, &["rl", "lr"]);
    assert_eq!(found[0].0, 80.0);
    assert_eq!(found[1].0, 0.0);
}

#[test]
fn vertical_items_align_by_synthesized_baselines_in_horizontal_grids() {
    // A vertical item in a horizontal grid aligned by its baseline along the column takes part
    // with the baseline at the bottom of its border box (`grid-self-baseline-horiz-*` in WPT).
    let html = "<div style='display: grid; grid-auto-flow: column; align-items: baseline'>
          <div class=short style='writing-mode: vertical-lr; width: 10px; height: 10px'></div>
          <div class=tall style='writing-mode: vertical-rl; width: 10px; height: 30px'></div>
          <div class=flat style='width: 10px; height: 20px'></div></div>";
    let found = boxes(html, &["short", "tall", "flat"]);
    assert_eq!((found[0].1, found[1].1, found[2].1), (20.0, 0.0, 10.0));
}

#[test]
fn column_items_shrink_into_a_maximum_height() {
    // Items of a column whose height is its content's shrink into its `max-height`; their flex
    // basis was their content's, so percentages inside stay `auto` (`percentage-heights-015`).
    let html =
        "<div class=column style='display: flex; flex-direction: column; width: 100px; max-height: 200px'>
          <div class=item style='min-height: 0; overflow: hidden'>
            <div class=child style='height: 50%'><div style='height: 250px'></div></div></div></div>";
    let found = boxes(html, &["column", "item", "child"]);
    assert_eq!(found[0].3, 200.0);
    assert_eq!(found[1].3, 200.0);
    assert_eq!(found[2].3, 250.0);
}

#[test]
fn horizontal_items_align_by_synthesized_baselines_in_vertical_grids() {
    // In a `vertical-rl` grid of sideways text the alphabetic baseline synthesized for a
    // horizontal item is its left edge: the items' left edges line up (`grid-self-baseline-003`
    // in WPT).
    let html = "<div style='display: grid; writing-mode: vertical-rl; text-orientation: sideways; width: 40px; height: 40px;
            grid-template-columns: 20px 20px; align-items: baseline; justify-items: start'>
          <div class=narrow style='writing-mode: horizontal-tb; width: 10px; height: 10px'></div>
          <div class=wide style='writing-mode: horizontal-tb; width: 30px; height: 10px'></div></div>";
    let found = boxes(html, &["narrow", "wide"]);
    assert_eq!((found[0].0, found[1].0), (10.0, 10.0));
}

#[test]
fn items_of_the_opposite_block_flow_align_at_the_end() {
    // In a `vertical-lr` grid a `vertical-rl` item aligned by its baseline is in the group of
    // the last baselines, at the end of its row (`grid-self-baseline-002` in WPT).
    let html = "<div style='display: grid; writing-mode: vertical-lr; width: 100px; height: 40px;
            grid-template-columns: 20px 20px; align-items: baseline'>
          <div class=same style='width: 20px'></div>
          <div class=opposite style='writing-mode: vertical-rl; width: 20px'></div></div>";
    let found = boxes(html, &["same", "opposite"]);
    assert_eq!((found[0].0, found[1].0), (0.0, 80.0));
}

#[test]
fn baseline_justified_items_of_vertical_grids_line_up_their_synthesized_baselines() {
    // Vertical items aligned by their baselines across a column of a `vertical-lr` grid of
    // sideways text line up their bottoms, the baselines synthesized from their border boxes
    // (`grid-self-baseline-vertical-lr-004` in WPT).
    let html = "<div style='display: grid; writing-mode: vertical-lr; text-orientation: sideways; width: 40px; height: 40px;
            grid-template: 20px 20px / 40px; justify-items: baseline; align-items: start'>
          <div class=short style='width: 10px; height: 10px'></div>
          <div class=tall style='width: 10px; height: 30px'></div></div>";
    let found = boxes(html, &["short", "tall"]);
    assert_eq!((found[0].1, found[1].1), (20.0, 0.0));
}

#[test]
fn safe_alignment_starts_children_overflowing_their_containing_block_at_its_start() {
    // A child of a grid or flex container that is not its containing block is aligned in the
    // container's content box; under `safe` center or end alignment, it does not start before
    // its containing block, as in Chrome (`grid-abspos-staticpos-align-self-safe-outer-cb-001`
    // in WPT). In a grid area of a grid that contains it, Chrome ignores `safe`.
    let html = "<div style='position: relative; padding: 40px'>
          <div style='display: grid; width: 20px; height: 20px'>
            <div class=fits style='position: absolute; width: 30px; height: 30px; align-self: safe end; justify-self: safe end'></div></div></div>
        <div style='position: relative; padding: 40px'>
          <div style='display: grid; width: 20px; height: 20px'>
            <div class=wide style='position: absolute; width: 120px; height: 30px; align-self: safe end; justify-self: safe end'></div></div></div>
        <div style='position: relative; padding: 40px'>
          <div style='display: flex; width: 20px; height: 20px'>
            <div class=tall style='position: absolute; width: 30px; height: 120px; align-self: safe end'></div></div></div>
        <div style='display: grid; grid: 20px / 20px; width: 50px; height: 50px; position: relative'>
          <div class=area style='position: absolute; grid-area: 1 / 1 / 2 / 2; width: 30px; height: 30px; align-self: safe end; justify-self: safe end'></div></div>";
    let found = boxes(html, &["fits", "wide", "tall", "area"]);
    assert_eq!((found[0].0, found[0].1), (30.0, 30.0));
    assert_eq!((found[1].0, found[1].1), (0.0, 130.0));
    assert_eq!((found[2].0, found[2].1), (40.0, 200.0));
    assert_eq!((found[3].0, found[3].1), (-10.0, 290.0));
}

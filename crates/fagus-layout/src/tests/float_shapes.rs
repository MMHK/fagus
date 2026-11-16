//! Lines beside floats with `shape-outside`, boxes sized by the percentage heights of replaced
//! elements inside, and elements replaced by their `content`, as Chrome 157 prints them (WPT
//! `css-shapes`, `css-sizing/intrinsic-percent-replaced-*`, `css-content/element-replacement*`).
//! The numbers are Chrome's.

use std::sync::Arc;

use super::{find, render_document, render_document_with};
use crate::{FragmentKind, Image, ImageFormat, Images};

/// The left, top, width and height of the boxes with the given classes on the first page.
fn boxes_with(html: &str, classes: &[&str], images: Images) -> Vec<[f32; 4]> {
    let (document, pages) = render_document_with(
        &format!(
            "<!DOCTYPE html><style>@page {{ size: 600px 600px; margin: 0 }} body {{ margin: 0; font: 12px/16px Inter }}</style>{html}"
        ),
        |_| images,
    );
    classes
        .iter()
        .map(|class| {
            let (x, y, fragment) = find(&pages[0], &document, class).expect(class);
            [x, y, fragment.rect.width.to_px(), fragment.rect.height.to_px()]
        })
        .collect()
}

fn boxes(html: &str, classes: &[&str]) -> Vec<[f32; 4]> {
    boxes_with(html, classes, Images::default())
}

/// The left and top of the box with class `class`.
fn place(html: &str, class: &str) -> (f32, f32) {
    let [x, y, ..] = boxes(html, &[class])[0];
    (x, y)
}

#[test]
fn steps_lines_down_along_shapes_until_they_fit() {
    // A box too wide beside the circle at the top goes down 1px at a time, to where the circle
    // leaves it room (50px down), not below the float (WPT `float-retry-push-circle`).
    let html = "<div style='width: 300px; height: 100px; line-height: 1'>
        <div style='float: left; width: 100px; height: 100px; shape-outside: circle(70.710678px at 0px 0px)'></div>
        <span class=b style='display: inline-block; width: 250px; height: 20px'></span></div>";
    assert_eq!(place(html, "b"), (50.0, 50.0));
}

#[test]
fn lays_lines_out_again_in_the_band_they_come_to() {
    // Each line is as far from the rounded float as its own height needs (WPT
    // `shape-outside-margin-box-border-radius-002`).
    let found = boxes(
        "<div style='width: 200px; line-height: 0'>
           <div style='float: left; shape-outside: margin-box; border-radius: 50%; width: 20px; height: 20px;
             padding: 20px; border: 20px solid; margin: 10px'></div>
           <div class=a style='display: inline-block; width: 60px; height: 24px'></div>
           <div class=b style='display: inline-block; width: 60px; height: 36px'></div>
           <div class=c style='display: inline-block; width: 60px; height: 36px'></div>
           <div class=d style='display: inline-block; width: 60px; height: 24px'></div></div>",
        &["a", "b", "c", "d"],
    );
    let places: Vec<(f32, f32)> = found.iter().map(|found| (found[0], found[1])).collect();
    assert_eq!(
        places,
        [(108.0, 0.0), (120.0, 24.0), (120.0, 60.0), (108.0, 96.0)]
    );
    // A line that the taller box makes too narrow for both keeps the first box alone, beside
    // the triangle as far as that box reaches (WPT `shape-outside-infinite-crash`).
    let found = boxes(
        "<div style='width: 200px; line-height: 0'>
           <div style='float: left; width: 100px; height: 100px; shape-outside: polygon(0 0, 100% 100%, 0 100%)'></div>
           <span class=a style='display: inline-block; width: 90px; height: 20px'></span><span class=b
             style='display: inline-block; width: 90px; height: 22px'></span></div>",
        &["a", "b"],
    );
    assert_eq!((found[0][0], found[0][1]), (22.0, 0.0));
    assert_eq!((found[1][0], found[1][1]), (42.0, 20.0));
}

#[test]
fn wraps_lines_around_paths_and_gradients() {
    // `path()` is drawn into pixels whose rows the lines go beside (WPT `shape-outside-path-*`).
    let html = |shape: &str| {
        format!(
            "<div style='width: 200px; line-height: 0'>
               <div style='float: left; width: 200px; height: 200px; shape-outside: {shape}'></div>
               <span class=b style='display: inline-block; width: 50px; height: 20px'></span></div>"
        )
    };
    assert_eq!(
        place(&html("path(\"M 0 0 L 100 0 L 100 200 L 0 200 Z\")"), "b"),
        (100.0, 0.0)
    );
    // A circle of Bézier curves from 55px down leaves the top free.
    let circle = "path(\"M 45 55 C 69.8528 55 90 75.1472 90 100 C 90 124.8528 69.8528 145 45 145 \
                  C 20.1472 145 0 124.8528 0 100 C 0 75.1472 20.1472 55 45 55 Z\")";
    assert_eq!(place(&html(circle), "b"), (0.0, 0.0));
    // A gradient is drawn in the content box: lines go beside its opaque left half (WPT
    // `shape-outside-linear-gradient-*`).
    let gradient = "linear-gradient(to right, black 50%, transparent 50%)";
    assert_eq!(place(&html(gradient), "b"), (100.0, 0.0));
    // In a vertical writing mode the shape turns with the float.
    let found = boxes(
        "<div style='writing-mode: vertical-rl; width: 200px; height: 200px; line-height: 0'>
           <div style='float: left; width: 200px; height: 200px; shape-outside: linear-gradient(to bottom, black 50%, transparent 50%)'></div>
           <span class=b style='display: inline-block; width: 20px; height: 50px'></span></div>",
        &["b"],
    );
    assert_eq!((found[0][0], found[0][1]), (180.0, 100.0));
}

#[test]
fn sizes_boxes_by_the_percentage_heights_of_replaced_content() {
    // The canvas takes the box's height, which is definite, and its width from its ratio: the
    // box shrinks to that (WPT `intrinsic-percent-replaced-*`, `abs-pos-with-replaced-child`).
    let found = boxes(
        "<div style='height: 200px'><div class=a style='float: left; height: 50%'>
           <canvas style='display: block; height: 100%' width=1 height=1></canvas></div></div>
         <div style='position: relative; height: 200px'><div class=b style='position: absolute; top: 0; bottom: 100px'>
           <canvas style='display: block; height: 100%' width=1 height=1></canvas></div></div>
         <div style='width: 200px'><div class=c style='writing-mode: vertical-rl; width: 50%'>
           <canvas style='display: block; width: 100%' width=1 height=1></canvas></div></div>",
        &["a", "b", "c"],
    );
    assert_eq!(found[0][2..], [100.0, 100.0]);
    assert_eq!(found[1][2..], [100.0, 100.0]);
    assert_eq!(found[2][2..], [100.0, 100.0]);
}

/// A PNG image of `width` × `height` pixels.
fn image(width: f32, height: f32) -> Arc<Image> {
    Arc::new(Image {
        data: Arc::new(Vec::new()),
        format: ImageFormat::Png,
        width,
        height,
        natural: (true, true),
        ratio: Some(width / height),
        inline: false,
        orientation: 1,
        filters: Arc::from([]),
        pad: 0,
    })
}

/// Whether a fragment holds text.
fn has_text(fragment: &crate::Fragment) -> bool {
    matches!(fragment.kind, FragmentKind::Text(_)) || fragment.children.iter().any(has_text)
}

#[test]
fn replaces_elements_whose_content_is_an_image() {
    // The element shows the image as a replaced element would, sized by its own `width` and
    // `height`, and its text not at all; an `<img>` shows it in place of its source (WPT
    // `css-content/element-replacement*`). The alternative text after `/` is not shown.
    let mut images = Images::default();
    images.insert("rect.png", image(100.0, 100.0));
    images.insert("other.png", image(10.0, 10.0));
    let found = boxes_with(
        "<p class=a style='margin: 0; content: url(rect.png)'>This text should not be visible</p>
         <div class=b style='width: 50px; height: 30px; content: url(rect.png) / \"alt\"'>text</div>
         <img class=c src=other.png alt=x style='content: url(rect.png)'>",
        &["a", "b", "c"],
        images,
    );
    assert_eq!(found[0], [0.0, 0.0, 100.0, 100.0]);
    assert_eq!(found[1], [0.0, 100.0, 50.0, 30.0]);
    assert_eq!(found[2][2..], [100.0, 100.0]);
    let (_, pages) = render_document_with(
        "<!DOCTYPE html><p style='content: url(rect.png)'>This text should not be visible</p>",
        |_| {
            let mut images = Images::default();
            images.insert("rect.png", image(100.0, 100.0));
            images
        },
    );
    assert!(!pages[0].fragments.iter().any(has_text));
    // Without the image loaded, the element shows its content.
    let (_, pages) = render_document("<!DOCTYPE html><p style='content: url(missing.png)'>Shown</p>");
    assert!(pages[0].fragments.iter().any(has_text));
}

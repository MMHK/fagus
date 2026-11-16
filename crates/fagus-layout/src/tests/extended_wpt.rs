//! Layout that the Web Platform Tests of flexbox, grid, positioning, alignment, tables and
//! images measure, as Chrome 157 prints it: percentages in absolutely positioned flex
//! containers, subgrids with more named areas than tracks, relatively positioned table rows,
//! `<object>`, `<embed>` and `<video>`, `align-content` on blocks, self-alignment of absolutely
//! positioned boxes and baseline alignment in grids. The numbers are Chrome's.

use std::sync::Arc;

use super::{find, render_document, render_document_with};
use crate::{FragmentKind, Image, ImageFormat, Images};

/// The left, top, width and height of the boxes with the given classes on the first page, from
/// the top left corner of the page area.
fn boxes(html: &str, classes: &[&str]) -> Vec<[f32; 4]> {
    let (document, pages) = render_document(&format!(
        "<!DOCTYPE html><style>body {{ margin: 0; font: 12px/16px Inter }}</style><div class=origin></div>{html}"
    ));
    let (left, top, _) = find(&pages[0], &document, "origin").expect("origin");
    classes
        .iter()
        .map(|class| {
            let (x, y, fragment) = find(&pages[0], &document, class).expect(class);
            [
                x - left,
                y - top,
                fragment.rect.width.to_px(),
                fragment.rect.height.to_px(),
            ]
        })
        .collect()
}

/// An image of `width` × `height` pixels in `format`.
fn image(format: ImageFormat, width: f32, height: f32) -> Arc<Image> {
    Arc::new(Image {
        data: Arc::new(Vec::new()),
        format,
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

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.1
}

#[test]
fn resolves_percentages_in_flex_containers_between_insets() {
    // A column flex container held between the insets of its containing block has a definite
    // height: 25% of it (45px) and a basis of 100% (180px) resolve, and both shrink into it in
    // proportion (WPT `css-flexbox/percentage-heights-002`).
    let found = boxes(
        "<div style='position: relative; width: 300px; height: 200px'>
           <div style='position: absolute; inset: 10px; display: flex; flex-direction: column'>
             <div class=top style='height: 25%'>a</div><div class=rest style='flex-basis: 100%'>b</div>
           </div>
         </div>
         <div style='position: relative; width: 300px; height: 100px'>
           <div style='position: absolute; top: 0; bottom: 0; display: flex'>
             <div class=item style='width: 50px; height: 50%'></div>
           </div>
         </div>",
        &["top", "rest", "item"],
    );
    assert!(near(found[0][3], 36.0), "{found:?}");
    assert!(near(found[1][1], 46.0) && near(found[1][3], 144.0), "{found:?}");
    assert!(near(found[2][3], 50.0), "{found:?}");
}

#[test]
fn keeps_subgrid_areas_within_its_tracks() {
    // Areas of a subgrid that name more rows than it spans do not add rows to it, which once
    // read past its tracks (WPT `css-grid/subgrid/line-names-013`).
    let found = boxes(
        "<div style='display: grid; grid-template: 100px 100px / 100px; height: 100px; width: 100px'>
           <div style='display: grid; grid-template-areas: \"item item\" \"item item\";
                       grid-template-rows: subgrid'>
             <div class=item style='grid-area: item'></div>
           </div>
         </div>",
        &["item"],
    );
    assert!(near(found[0][3], 100.0) && near(found[0][2], 100.0), "{found:?}");
}

#[test]
fn moves_the_cells_of_relatively_positioned_rows() {
    // The offsets of a relatively positioned row and its row group move its cells (see
    // `fagus_paint` for the order they paint in).
    let (document, pages) = render_document(
        "<!DOCTYPE html><style>body { margin: 0; font: 12px/16px Inter } td { padding: 0 }
           table { border-spacing: 0 }</style>
         <div class=origin style='position: relative'>
           <div style='position: absolute; left: 100px; width: 30px; height: 30px; background: red'></div>
           <table><tbody style='position: relative; left: 100px; background: green'>
             <tr style='position: relative; top: 5px'><td class=cell><div style='width: 30px; height: 30px'></div></td></tr>
           </tbody></table>
           <table><tr style='position: relative; top: 100%'><td class=still><div style='height: 30px'></div></td></tr></table>
         </div>",
    );
    let (x, y, _) = find(&pages[0], &document, "cell").expect("cell");
    let (left, top, _) = find(&pages[0], &document, "origin").expect("origin");
    assert!(near(x - left, 100.0) && near(y - top, 5.0), "{x} {y}");
    // A percentage of the height does not resolve (WPT `css-position/position-relative-008`).
    let (_, still, _) = find(&pages[0], &document, "still").expect("still");
    assert!(near(still - top, 30.0), "{still}");
}

#[test]
fn shows_images_of_objects_embeds_and_video_posters() {
    use fagus_style::values::ObjectFit;
    let html = "<!DOCTYPE html><style>body { margin: 0; font: 12px/16px Inter }
          object, embed, video { display: block; object-fit: none }</style>
        <object class=svg data=pic.svg style='width: 96px; height: 64px'></object>
        <embed class=png src=pic.png style='width: 96px; height: 64px'>
        <embed class=sized src=pic.png width=50>
        <video class=poster poster=pic.png style='object-fit: contain'></video>
        <video class=empty></video>
        <video class=narrow width=100 src=movie.mp4></video>";
    let (document, pages) = render_document_with(html, |_| {
        let mut images = Images::default();
        images.insert("pic.svg", image(ImageFormat::Svg, 16.0, 8.0));
        images.insert("pic.png", image(ImageFormat::Png, 16.0, 8.0));
        images
    });
    let found = |class: &str| {
        let (_, _, fragment) = find(&pages[0], &document, class).expect(class);
        let FragmentKind::Box { style, .. } = &fragment.kind else {
            panic!("{class} is no box");
        };
        let shows_image = fragment
            .children
            .iter()
            .any(|child| matches!(child.kind, FragmentKind::Image(_)));
        (
            fragment.rect.width.to_px(),
            fragment.rect.height.to_px(),
            style.object_fit,
            shows_image,
        )
    };
    // An SVG document in an `<object>` fills its box: `object-fit` does not apply to it; it
    // does to a PNG in an `<embed>`.
    assert_eq!(found("svg"), (96.0, 64.0, ObjectFit::Fill, true));
    assert_eq!(found("png"), (96.0, 64.0, ObjectFit::None, true));
    // `width` gives an `<embed>` its width, its height following the image's ratio.
    assert_eq!(found("sized"), (50.0, 25.0, ObjectFit::None, true));
    // A video shows its poster, at the poster's size; without one it is 300 × 150, and a
    // `width` leaves it 150 pixels tall.
    assert_eq!(found("poster"), (16.0, 8.0, ObjectFit::Contain, true));
    assert_eq!(found("empty"), (300.0, 150.0, ObjectFit::None, true));
    // A video to play (which Fagus does not decode) is black all over, as Chrome prints one
    // that does not load.
    assert_eq!(found("narrow"), (100.0, 150.0, ObjectFit::Fill, true));
}

#[test]
fn makes_blocks_with_content_alignment_formatting_contexts() {
    // A block with an `align-content` other than `normal` holds its floats and the margins of
    // its children, and stays beside a float before it (WPT `css-align/blocks/align-content-block-001`).
    let found = boxes(
        "<div style='float: left; width: 30px; height: 300px'></div>
         <div class=aligned style='align-content: start'>
           <div style='margin-top: 12px'><div style='float: left; width: 10px; height: 40px'></div>x</div>
         </div>
         <div class=centered style='align-content: center; height: 80px'><div class=inner>y</div></div>",
        &["aligned", "centered", "inner"],
    );
    assert!(near(found[0][0], 30.0) && near(found[0][3], 52.0), "{found:?}");
    assert!(near(found[1][1], 52.0), "{found:?}");
    assert!(near(found[2][1], 52.0 + 32.0), "{found:?}");
}

#[test]
fn aligns_absolutely_positioned_boxes_between_their_insets() {
    let found = boxes(
        "<div style='position: relative; width: 200px; height: 100px'>
           <div class=center style='position: absolute; inset: 10px; place-self: center; width: 40px; height: 20px'></div>
           <div class=end style='position: absolute; inset: 10px; justify-self: end; align-self: end; width: 40px; height: 20px'></div>
           <div class=shrunk style='position: absolute; inset: 0; justify-self: start'>
             <span style='display: inline-block; width: 30px'></span></div>
           <div class=wide style='position: absolute; inset: 10px; justify-self: end; width: 250px; height: 10px'></div>
           <div class=fits style='position: absolute; left: 10px; right: 10px; justify-self: end; width: 190px; height: 10px'></div>
         </div>
         <div style='position: relative; height: 50px'>
           <div style='height: 30px'></div>
           <div class=static style='position: absolute; align-self: end; width: 10px; height: 10px'></div>
         </div>",
        &["center", "end", "shrunk", "wide", "fits", "static"],
    );
    assert_eq!(found[0], [80.0, 40.0, 40.0, 20.0]);
    assert_eq!(found[1], [150.0, 70.0, 40.0, 20.0]);
    // An aligned box shrinks to fit instead of stretching between its insets.
    assert!(near(found[2][2], 30.0), "{found:?}");
    // One too wide for its containing block starts at it; one too wide for the room between its
    // insets moves into its containing block.
    assert!(near(found[3][0], 0.0), "{found:?}");
    assert!(near(found[4][0], 0.0), "{found:?}");
    // At its static position, `end` ends the box there.
    assert!(near(found[5][1], 100.0 + 30.0 - 10.0), "{found:?}");
}

#[test]
fn aligns_grid_items_by_their_first_baselines() {
    let found = boxes(
        "<div style='display: grid; grid-template-columns: repeat(3, auto); align-items: baseline'>
           <div class=big style='font-size: 30px; line-height: 36px'>Big</div>
           <div class=small>small</div>
           <div class=padded style='padding-top: 20px'>padded</div>
         </div>
         <div class=after>after</div>",
        &["big", "small", "padded", "after"],
    );
    // The baselines meet at that of the padded item, the lowest: the small item moves down by
    // its padding, the big one by 3px, and the row grows to hold the big one so (Chrome 157).
    assert!(near(found[2][1], 0.0) && near(found[1][1], 20.0), "{found:?}");
    assert!(near(found[0][1], 3.0), "{found:?}");
    assert!(near(found[3][1], 39.0), "{found:?}");
}

#[test]
fn replaces_elements_by_the_image_of_their_content() {
    let html = "<!DOCTYPE html><style>body { margin: 0; font: 12px/16px Inter }
          .image { content: url(pic.png) } .two { content: url(pic.png) 'text' }</style>
        <div class=image>Text</div><span class=image></span><div class=two>Kept</div><div class=missing style='content: url(none.png)'>Also</div>";
    let (document, pages) = render_document_with(html, |_| {
        let mut images = Images::default();
        images.insert("pic.png", image(ImageFormat::Png, 16.0, 8.0));
        images
    });
    let shows_image = |fragment: &crate::Fragment| {
        fragment
            .children
            .iter()
            .any(|child| matches!(child.kind, FragmentKind::Image(_)))
    };
    // An element whose `content` is an image alone is that image (Chrome 157), its children
    // left out.
    let (_, _, image_box) = find(&pages[0], &document, "image").expect("image");
    assert!(shows_image(image_box));
    assert_eq!(
        (image_box.rect.width.to_px(), image_box.rect.height.to_px()),
        (16.0, 8.0)
    );
    // Other content, or an image that does not load, leaves the element as it is.
    let (_, _, two) = find(&pages[0], &document, "two").expect("two");
    assert!(!shows_image(two));
    let (_, _, missing) = find(&pages[0], &document, "missing").expect("missing");
    assert!(!shows_image(missing));
}

//! Images that do not load, as Chrome renders them (HTML, §15.4.1): their text after a broken
//! image icon, or a framed box of their size. The numbers are Chrome's.

use super::render_document;
use crate::{Fragment, FragmentKind, Page};

/// What a page draws: text runs with the left and top of their boxes, and the boxes (left, top,
/// width, height) of images and of boxes with a silver border.
#[derive(Default)]
struct Drawn {
    runs: Vec<(String, f32, f32)>,
    images: Vec<[f32; 4]>,
    frames: Vec<[f32; 4]>,
}

fn drawn(page: &Page) -> Drawn {
    fn walk(fragment: &Fragment, origin: (f32, f32), out: &mut Drawn) {
        let x = origin.0 + fragment.rect.x.to_px();
        let y = origin.1 + fragment.rect.y.to_px();
        let rect = [x, y, fragment.rect.width.to_px(), fragment.rect.height.to_px()];
        match &fragment.kind {
            FragmentKind::Text(run) => out.runs.push((run.text.clone(), x, y)),
            FragmentKind::Image(_) => out.images.push(rect),
            FragmentKind::Box { style, .. }
                if near(style.border_width.top, 1.0)
                    && style
                        .border_color
                        .top
                        .is_some_and(|color| near(color.r, 192.0 / 255.0)) =>
            {
                out.frames.push(rect);
            }
            _ => {}
        }
        for child in &fragment.children {
            walk(child, (x, y), out);
        }
    }
    let mut out = Drawn::default();
    for fragment in &page.fragments {
        walk(fragment, (0.0, 0.0), &mut out);
    }
    out
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

fn at(runs: &[(String, f32, f32)], text: &str) -> (f32, f32) {
    match runs.iter().find(|(run, ..)| run.starts_with(text)) {
        Some((_, x, y)) => (*x, *y),
        None => panic!("no run {text:?} in {runs:?}"),
    }
}

#[test]
fn shows_the_alt_text_after_a_broken_image_icon() {
    let (_, pages) = render_document(
        "<!doctype html><style>body { margin: 0; font: 16px Inter } p { margin: 0 0 10px }</style>
         <p>A <img src=missing.png alt=Logo width=100 height=50> after</p>
         <p>B <img src=missing.png title=Titled> after</p>
         <p>C <img src=missing.png alt=''> after</p>
         <p>D <input type=image src=missing.png></p>",
    );
    let Drawn { runs, images, frames } = drawn(&pages[0]);
    // With its text, an image takes no size of its own: the icon, 16 pixels square, at the top
    // of the line, then the text.
    let (logo, after) = (at(&runs, "Logo"), at(&runs, " after"));
    assert!(
        images
            .iter()
            .any(|[x, y, w, h]| near(x + w, logo.0) && near(*y, logo.1) && near(*w, 16.0) && near(*h, 16.0))
    );
    assert!(after.0 > logo.0 && after.0 < logo.0 + 50.0, "{runs:?}");
    // The title stands in for the alt text.
    at(&runs, "Titled");
    // An empty alt text shows nothing; an input shows "Submit".
    assert_eq!(images.len(), 3, "{images:?}");
    at(&runs, "Submit");
    assert!(frames.is_empty(), "{frames:?}");
}

#[test]
fn keeps_the_size_of_an_image_without_alt_text_in_a_frame() {
    let (_, pages) = render_document(
        "<!doctype html><style>body { margin: 0; font: 16px Inter } p { margin: 0 0 10px }</style>
         <p>A <img src=missing.png width=100 height=50> after</p>
         <p>B <img src=missing.png alt='' style='width: 60px; height: 30px; padding: 4px'> after</p>
         <p>C <img src=missing.png width=12 height=12> after</p>",
    );
    let Drawn { runs, images, frames } = drawn(&pages[0]);
    // A box of the image's size with a silver border, the icon inside its border and padding,
    // on the baseline with the bottom of its content box.
    assert_eq!(frames.len(), 2, "{frames:?}");
    let [x, y, w, h] = frames[0];
    assert_eq!((w, h), (100.0, 50.0));
    assert!(
        images
            .iter()
            .any(|image| near(image[0], x + 2.0) && near(image[1], y + 2.0))
    );
    let after = at(&runs, " after");
    assert!((after.0 - (x + 100.0)).abs() < 5.0, "{runs:?}");
    assert_eq!((frames[1][2], frames[1][3]), (60.0, 30.0));
    // Less than 18 pixels: neither frame nor icon.
    assert_eq!(images.len(), 2, "{images:?}");
}

#[test]
fn inline_blocks_that_clip_sit_on_their_last_line() {
    let (_, pages) = render_document(
        "<!doctype html><style>body { margin: 0; font: 16px Inter }
         span { display: inline-block; padding: 5px; border: 1px solid }</style>
         <p>A <span style='overflow: clip'>clip</span> after</p>
         <p>B <span>plain</span> after</p>
         <p>C <span style='overflow: hidden'>hidden</span> after</p>
         <p>D <span><span style='display: block; overflow: hidden; padding-bottom: 10px'>inner</span></span> after</p>",
    );
    let runs = drawn(&pages[0]).runs;
    // Only a scroll container sits on its bottom margin edge (as in Chrome).
    assert!((at(&runs, "clip").1 - at(&runs, "A").1).abs() < 0.01, "{runs:?}");
    assert!((at(&runs, "plain").1 - at(&runs, "B").1).abs() < 0.01, "{runs:?}");
    assert!(at(&runs, "hidden").1 < at(&runs, "C").1 - 1.0, "{runs:?}");
    // A scroll container inside one gives it the bottom of its border box as its baseline,
    // not its lines.
    assert!(at(&runs, "inner").1 < at(&runs, "D").1 - 9.0, "{runs:?}");
}

#[test]
fn underlines_the_text_of_images_in_links() {
    fn decorated(fragment: &Fragment, out: &mut Vec<(String, usize)>) {
        if let FragmentKind::Text(run) = &fragment.kind {
            out.push((run.text.clone(), run.style.decorations.len()));
        }
        fragment.children.iter().for_each(|child| decorated(child, out));
    }
    let (_, pages) = render_document(
        "<!doctype html><style>body { margin: 0; font: 16px Inter }</style>
         <p><a href=x><img src=missing.png alt=Linked></a></p>",
    );
    let mut runs = Vec::new();
    decorated(&pages[0].fragments[0], &mut runs);
    // The link's underline reaches the text that stands in for the image.
    assert!(
        runs.iter().any(|(text, count)| text == "Linked" && *count == 1),
        "{runs:?}"
    );
}

#[test]
fn keeps_the_height_of_the_ratio_of_an_image_without_text() {
    let (document, pages) = render_document(
        "<!doctype html><style>body { margin: 0; font: 16px/20px Inter } div { margin: 0 0 10px }</style>
         <div class=ratio><img src=missing.png style='width: 100px; aspect-ratio: 2'> A</div>
         <div class=sized><img src=missing.png style='width: 100px; height: 50px'> B</div>
         <div class=none><img style='width: 100px; aspect-ratio: 2'> C</div>",
    );
    let height = |class: &str| {
        super::find(&pages[0], &document, class)
            .expect(class)
            .2
            .rect
            .height
            .to_px()
    };
    // Sized by its ratio alone, the image is 100 by 50 pixels; the frame inside it is as high
    // as the icon in it, and the line's baseline lies under the frame, inside the image. Sized
    // by its height, the frame fills it and the baseline is at its bottom.
    let Drawn { frames, images, .. } = drawn(&pages[0]);
    assert_eq!((frames[0][2], frames[0][3]), (100.0, 20.0), "{frames:?}");
    assert_eq!((frames[1][2], frames[1][3]), (100.0, 50.0), "{frames:?}");
    assert_eq!(height("ratio"), 50.0);
    assert_eq!(height("sized"), 54.0);
    // Without a source or text, an image does not fail: an empty box of its size.
    assert_eq!(frames.len(), 2, "{frames:?}");
    assert_eq!(images.len(), 2, "{images:?}");
    assert_eq!(height("none"), 54.0);
}

#[test]
fn takes_the_ratio_of_the_size_attributes_where_a_style_sets_height_auto() {
    let (document, pages) = render_document(
        "<!doctype html><style>body { margin: 0; font: 16px/20px Inter } div { margin: 0 0 10px }</style>
         <div class=auto><img src=missing.png width=100 height=50 alt='' style='height: auto'> A</div>
         <div class=sized><img src=missing.png width=100 height=50> B</div>",
    );
    let height = |class: &str| {
        super::find(&pages[0], &document, class)
            .expect(class)
            .2
            .rect
            .height
            .to_px()
    };
    // `height: auto` overrides the attribute (a presentational hint), which leaves the width
    // and the ratio of the attributes: an image sized by its ratio, with its baseline under the
    // frame inside it, as in Chrome; with both attributes, the baseline is at its bottom.
    let Drawn { frames, .. } = drawn(&pages[0]);
    assert_eq!((frames[0][2], frames[0][3]), (100.0, 20.0), "{frames:?}");
    assert_eq!((frames[1][2], frames[1][3]), (100.0, 50.0), "{frames:?}");
    assert_eq!(height("auto"), 50.0);
    assert_eq!(height("sized"), 54.0);
}

#[test]
fn frames_images_without_a_source_that_have_a_size() {
    let (_, pages) = render_document(
        "<!doctype html><style>body { margin: 0; font: 16px Inter } p { margin: 0 }</style>
         <p>A <img style='width: 100px; height: 50px; margin-bottom: 6px'> after</p>
         <p>B <img style='width: 100px'> <img> after</p>",
    );
    let Drawn { runs, images, frames } = drawn(&pages[0]);
    // A box of its size with a silver border and no icon (Chrome 157); without a size, nothing.
    assert_eq!(frames.len(), 1, "{frames:?}");
    assert!(images.is_empty(), "{images:?}");
    let [_, y, w, h] = frames[0];
    assert_eq!((w, h), (100.0, 50.0));
    // It sits on the baseline with the bottom of its margin box, as a replaced element does:
    // the text after it ends 6 pixels below it (its top an ascent of about 15 pixels above).
    let after = at(&runs, " after");
    assert!(after.1 > y + h - 12.0 && after.1 < y + h, "{runs:?} {frames:?}");
    let b = at(&runs, "B");
    assert!(b.1 >= y + h + 6.0, "{runs:?} {frames:?}");
}

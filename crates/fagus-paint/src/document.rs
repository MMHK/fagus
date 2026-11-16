//! SVG images with HTML in them (see [`fagus_layout::SvgDocument`]): each is drawn as what its
//! own page draws, scaled into the box of the image as the image would be, so that its HTML is
//! drawn as text and boxes, in order with the SVG's shapes, as Chrome draws it.
//!
//! The documents of frames (see [`fagus_layout::FrameDocument`]) are drawn the same way, in
//! place of the blank images that stand for them, unscaled at the corner of the frame's content
//! box and clipped to it.

use std::sync::Arc;

use fagus_layout::{FrameDocument, Image, Images, SvgDocument};
use fagus_text::FontDb;
use rustc_hash::FxHashMap;

use crate::{FilteredPaint, Item, MaskPaint, RectF};

/// What the pages of SVG documents draw, by the address of the document.
type Drawn = FxHashMap<usize, Arc<[Item]>>;

/// Replaces the images in `items` that are SVG documents of their own in `images` (in masks
/// and filtered groups too) by what their pages draw, placed into the images' boxes.
pub(crate) fn expand(items: &mut Vec<Item>, fonts: &FontDb, images: &Images) {
    if !images.has_documents() || !items.iter().any(|item| has_document(item, images)) {
        return;
    }
    let mut drawn = Drawn::default();
    *items = expanded(std::mem::take(items), fonts, images, &mut drawn);
}

/// Whether `item` draws an SVG document, itself or in what it masks with or filters.
fn has_document(item: &Item, images: &Images) -> bool {
    match item {
        Item::Image { image, .. } => {
            document_of(image, images).is_some() || images.frame_document(&image.data).is_some()
        }
        Item::PushMask(mask) => mask.items.iter().any(|item| has_document(item, images)),
        Item::Filtered(filtered) => filtered.items.iter().any(|item| has_document(item, images)),
        _ => false,
    }
}

/// The SVG document of an image, if it is drawn as one: an SVG image with HTML in it whose
/// pixels go through no filter.
fn document_of<'a>(image: &Image, images: &'a Images) -> Option<&'a Arc<SvgDocument>> {
    if image.format != fagus_layout::ImageFormat::Svg || !image.filters.is_empty() {
        return None;
    }
    images.document(&image.data)
}

fn expanded(items: Vec<Item>, fonts: &FontDb, images: &Images, drawn: &mut Drawn) -> Vec<Item> {
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        match item {
            Item::Image { rect, image } => {
                if let Some(document) = document_of(&image, images) {
                    place(document, rect, fonts, drawn, &mut out);
                } else if let Some(frame) = images.frame_document(&image.data) {
                    place_frame(frame, rect, fonts, drawn, &mut out);
                } else {
                    out.push(Item::Image { rect, image });
                }
            }
            Item::PushMask(mask) if mask.items.iter().any(|item| has_document(item, images)) => {
                out.push(Item::PushMask(Arc::new(MaskPaint {
                    items: expanded(mask.items.clone(), fonts, images, drawn),
                    luminance: mask.luminance,
                })));
            }
            Item::Filtered(filtered) if filtered.items.iter().any(|item| has_document(item, images)) => {
                out.push(Item::Filtered(Arc::new(FilteredPaint {
                    items: expanded(filtered.items.clone(), fonts, images, drawn),
                    bounds: filtered.bounds,
                    filters: filtered.filters.clone(),
                })));
            }
            item => out.push(item),
        }
    }
    out
}

/// Writes what the page of `document` draws, scaled into `rect` as the SVG fills a box of its
/// size (see [`fagus_layout::SvgFit`]) and clipped to it.
fn place(document: &Arc<SvgDocument>, rect: RectF, fonts: &FontDb, drawn: &mut Drawn, out: &mut Vec<Item>) {
    let (width, height) = document.size;
    if width <= 0.0 || height <= 0.0 || rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }
    let page = drawn.entry(Arc::as_ptr(document) as usize).or_insert_with(|| {
        let pages = crate::paint(
            std::slice::from_ref(&document.page),
            fonts,
            &document.images,
            false,
        );
        pages
            .into_iter()
            .next()
            .map_or_else(|| Arc::from([]), |page| Arc::from(page.items))
    });
    let (x, y, drawn_width, drawn_height) = document.fit.place(document.size, rect.width, rect.height);
    out.push(Item::PushClip(rect, None));
    out.push(Item::PushTransform([
        drawn_width / width,
        0.0,
        0.0,
        drawn_height / height,
        rect.x + x,
        rect.y + y,
    ]));
    out.extend(page.iter().cloned());
    out.push(Item::Pop);
    out.push(Item::Pop);
}

/// Writes what the page of the document of a frame draws at the corner of `rect`, the frame's
/// content box, clipped to it.
fn place_frame(
    frame: &Arc<FrameDocument>,
    rect: RectF,
    fonts: &FontDb,
    drawn: &mut Drawn,
    out: &mut Vec<Item>,
) {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }
    let page = drawn
        .entry(Arc::as_ptr(frame) as usize)
        .or_insert_with(|| Arc::from(crate::paint_frame(&frame.page, fonts, &frame.images)));
    if page.is_empty() {
        return;
    }
    out.push(Item::PushClip(rect, None));
    out.push(Item::PushTransform([1.0, 0.0, 0.0, 1.0, rect.x, rect.y]));
    out.extend(page.iter().cloned());
    out.push(Item::Pop);
    out.push(Item::Pop);
}

#[cfg(test)]
mod tests {
    use fagus_layout::units::{Au, Rect};

    use super::*;

    #[test]
    fn draws_svg_documents_in_place_of_their_images() {
        let data = Arc::new(b"<svg xmlns='http://www.w3.org/2000/svg'/>".to_vec());
        let image = Arc::new(Image {
            data: Arc::clone(&data),
            format: fagus_layout::ImageFormat::Svg,
            width: 100.0,
            height: 50.0,
            natural: (true, true),
            ratio: Some(2.0),
            inline: false,
            orientation: 1,
            filters: Arc::from([]),
            pad: 0,
        });
        let (width, height) = (Au::from_px(100.0), Au::from_px(50.0));
        let page = fagus_layout::Page {
            width,
            height,
            fragments: Vec::new(),
            margin: [Au::ZERO; 4],
            style: Arc::new(fagus_style::ComputedStyle::default()),
            canvas: None,
            bleed: 0.0,
            marks: fagus_layout::PageMarks::default(),
            orientation: fagus_layout::PageOrientation::Upright,
            scale: 1.0,
            area: Rect::new(Au::ZERO, Au::ZERO, width, height),
            shift: 0.0,
            carried: Vec::new(),
        };
        let mut images = Images::default();
        images.insert_document(
            &data,
            Arc::new(SvgDocument {
                page,
                images: Images::default(),
                size: (100.0, 50.0),
                fit: fagus_layout::SvgFit::Uniform {
                    cover: false,
                    x: 0.5,
                    y: 0.5,
                },
            }),
        );
        let rect = RectF {
            x: 10.0,
            y: 20.0,
            width: 200.0,
            height: 200.0,
        };
        let mut items = vec![Item::Image {
            rect,
            image: Arc::clone(&image),
        }];
        expand(&mut items, &FontDb::new(), &images);
        // Twice the size, centered down the square box: 50 pixels down.
        assert_eq!(items[0], Item::PushClip(rect, None));
        assert_eq!(items[1], Item::PushTransform([2.0, 0.0, 0.0, 2.0, 10.0, 70.0]));
        assert_eq!(items.last(), Some(&Item::Pop));
        assert!(!items.iter().any(|item| matches!(item, Item::Image { .. })));
        // An image of other data stays an image.
        let other = Arc::new(Image {
            data: Arc::new(data.to_vec()),
            ..(*image).clone()
        });
        let mut items = vec![Item::Image { rect, image: other }];
        expand(&mut items, &FontDb::new(), &images);
        assert!(matches!(items[..], [Item::Image { .. }]));
    }
}

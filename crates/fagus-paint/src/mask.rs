//! `mask` (CSS Masking 1, §7): image layers, painted as background layers are, of which the
//! alpha (or the luminance) masks what a box draws.

use fagus_layout::Images;
use fagus_style::ComputedStyle;
use fagus_style::values::{
    BackgroundRepeat, BackgroundSize, BoxArea, Image, LengthPercentageAuto, MaskComposite, MaskLayers,
    MaskMode, TransformBox,
};

use crate::background::{LayerGeometry, layer};
use crate::{Item, MaskPaint, Radii, RectF};

/// Where the layers of a mask are sized, positioned and clipped.
#[derive(Clone, Copy)]
pub(crate) enum Frame {
    /// A CSS box with `border_box` and `radii`, whose boxes the layers name.
    Box(RectF, Option<Radii>),
    /// An SVG element (see [`SvgBoxes`]), masked over `region`, which all it draws lies in.
    Svg(SvgBoxes, RectF),
}

/// The boxes of an SVG element that the layers of its mask refer to (CSS Masking 1, §6.1): its
/// object bounding box (`fill-box`), its stroke bounding box (`stroke-box`) and its nearest
/// viewport (`view-box`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SvgBoxes {
    pub fill: RectF,
    pub stroke: RectF,
    pub view: RectF,
}

impl SvgBoxes {
    fn of(&self, reference: TransformBox) -> RectF {
        match reference {
            TransformBox::ContentBox | TransformBox::FillBox => self.fill,
            TransformBox::BorderBox | TransformBox::StrokeBox => self.stroke,
            TransformBox::ViewBox => self.view,
        }
    }
}

/// Pushes the mask of a box with `border_box` and `radii`; returns how many masks it pushed
/// (none or one), each to be popped. The layers are combined as alpha, each with the layers
/// below it, as Chrome prints them: a layer of luminance turned into alpha first (see
/// [`luminance_alpha`], [`within`] and [`composited`]).
pub(crate) fn push(
    style: &ComputedStyle,
    mask: &MaskLayers,
    (border_box, radii): (RectF, Option<Radii>),
    images: &Images,
    items: &mut Vec<Item>,
) -> usize {
    match paint(style, mask, Frame::Box(border_box, radii), images) {
        Some(paint) => {
            items.push(Item::PushMask(std::sync::Arc::new(paint)));
            1
        }
        None => 0,
    }
}

/// What the layers of `mask` draw in `frame`, as a mask; `None` where no layer shows an image.
pub(crate) fn paint(
    style: &ComputedStyle,
    mask: &MaskLayers,
    frame: Frame,
    images: &Images,
) -> Option<MaskPaint> {
    // The area that layers of luminance and composited layers cover.
    let border_box = match frame {
        Frame::Box(border_box, _) => border_box,
        Frame::Svg(_, region) => region,
    };
    let pick = |list: usize, index: usize| index % list.max(1);
    // Where the layer `index` is drawn (the border box it takes), and how.
    let cycle = |index: usize| {
        let size =
            mask.sizes
                .get(pick(mask.sizes.len(), index))
                .cloned()
                .unwrap_or(BackgroundSize::Explicit(
                    LengthPercentageAuto::Auto,
                    LengthPercentageAuto::Auto,
                ));
        let repeat = mask
            .repeats
            .get(pick(mask.repeats.len(), index))
            .copied()
            .unwrap_or((BackgroundRepeat::Repeat, BackgroundRepeat::Repeat));
        let position = (
            mask.position_x.get(pick(mask.position_x.len(), index)).cloned(),
            mask.position_y.get(pick(mask.position_y.len(), index)).cloned(),
        );
        match frame {
            Frame::Box(border_box, radii) => (
                (border_box, radii),
                LayerGeometry {
                    whole: None,
                    exact: None,
                    fixed: None,
                    origin: mask
                        .origins
                        .get(pick(mask.origins.len(), index))
                        .copied()
                        .unwrap_or(BoxArea::BorderBox),
                    clip: mask
                        .clips
                        .get(pick(mask.clips.len(), index))
                        .copied()
                        .unwrap_or(Some(BoxArea::BorderBox)),
                    size,
                    repeat,
                    position,
                },
            ),
            // The boxes of an SVG element are its border box, in turn: the clip box (the
            // region without a clip), in which the layer is sized and positioned in its origin
            // box.
            Frame::Svg(boxes, region) => {
                let origin = mask
                    .svg_origins
                    .get(pick(mask.svg_origins.len(), index))
                    .copied()
                    .unwrap_or(TransformBox::StrokeBox);
                let clip = mask
                    .svg_clips
                    .get(pick(mask.svg_clips.len(), index))
                    .copied()
                    .unwrap_or(Some(TransformBox::StrokeBox));
                (
                    (clip.map_or(region, |clip| boxes.of(clip)), None),
                    LayerGeometry {
                        whole: Some(boxes.of(origin)),
                        exact: None,
                        fixed: None,
                        origin: BoxArea::BorderBox,
                        clip: clip.map(|_| BoxArea::BorderBox),
                        size,
                        repeat,
                        position,
                    },
                )
            }
        }
    };
    let in_luminance =
        |index: usize| mask.modes.get(index % mask.modes.len().max(1)).copied() == Some(MaskMode::Luminance);
    let composite = |index: usize| {
        mask.composites
            .get(index % mask.composites.len().max(1))
            .copied()
            .unwrap_or_default()
    };
    // Images that could not be loaded mask nothing, as in Chrome: without any image, there is
    // no mask.
    let shows = |image: &Image| match image {
        Image::Url(url, _) => {
            (images.get(url).is_some() && !images.is_blank(url)) || crate::reference::is_mask(url, images)
        }
        Image::Gradient(_) => true,
        Image::None => false,
    };
    if !mask.images.iter().any(shows) {
        // Under a backdrop filter, a mask of images that could not be loaded masks all (Chrome
        // 157, measured).
        if style.backdrop_filter && mask.images.iter().any(|image| matches!(image, Image::Url(..))) {
            return Some(MaskPaint {
                items: Vec::new(),
                luminance: false,
            });
        }
        return None;
    }
    // From the bottom layer up, the alpha of the layers so far.
    let mut below: Option<Vec<Item>> = None;
    for index in (0..mask.images.len()).rev() {
        // A layer that shows nothing still masks, as a transparent image.
        let mut drawn = Vec::new();
        let reference =
            matches!(&mask.images[index], Image::Url(url, _) if crate::reference::is_mask(url, images));
        if reference && let Image::Url(url, _) = &mask.images[index] {
            // An SVG `<mask>` covers the border box with its alpha, its luminance or alpha as
            // `mask-mode` says (its own `mask-type` for `match-source`).
            let mode = mask.modes.get(index % mask.modes.len().max(1)).copied();
            let luminance = match mode {
                Some(MaskMode::Luminance) => Some(true),
                Some(MaskMode::Alpha) => Some(false),
                Some(MaskMode::MatchSource) | None => None,
            };
            // Over the object bounding box of an SVG element, reaching over its region.
            let (over, reach) = match frame {
                Frame::Box(border_box, _) => (border_box, shadow_reach(style)),
                Frame::Svg(boxes, region) => (
                    boxes.fill,
                    (boxes.fill.x - region.x)
                        .max(boxes.fill.y - region.y)
                        .max(region.x + region.width - boxes.fill.x - boxes.fill.width)
                        .max(region.y + region.height - boxes.fill.y - boxes.fill.height)
                        .max(0.0),
                ),
            };
            drawn.extend(crate::reference::mask(url, over, images, luminance, reach));
        } else {
            let (placed, geometry) = cycle(index);
            layer(
                (style, placed.0, placed.1),
                &mask.images[index],
                &geometry,
                images,
                &mut drawn,
            );
        }
        let luminance = in_luminance(index) && !reference;
        // A single layer of luminance is a mask of luminance.
        if luminance && mask.images.len() == 1 {
            return Some(MaskPaint {
                items: drawn,
                luminance: true,
            });
        }
        let top = if luminance {
            luminance_alpha(drawn, border_box)
        } else {
            drawn
        };
        below = Some(match below.take() {
            None => top,
            Some(below) => match composite(index) {
                MaskComposite::Add | MaskComposite::Exclude => [below, top].concat(),
                // A layer of luminance combines exactly, one of alpha as Skia's PDF output
                // draws it (see [`composited`]).
                MaskComposite::Intersect if luminance => within(&below, top, false, border_box),
                MaskComposite::Subtract if luminance => within(&below, top, true, border_box),
                // An image is drawn over its rectangle: what lies below goes there, and the
                // image within (or outside) it (Chrome 157, measured).
                composite @ (MaskComposite::Intersect | MaskComposite::Subtract)
                    if matches!(mask.images[index], Image::Url(..)) =>
                {
                    let shape = if reference {
                        Some(border_box)
                    } else {
                        drawn_area(&top)
                    };
                    match shape {
                        Some(shape) => shaped(
                            &below,
                            top,
                            composite == MaskComposite::Subtract,
                            shape,
                            border_box,
                        ),
                        None => below,
                    }
                }
                MaskComposite::Intersect => composited(&below, top, false, border_box),
                MaskComposite::Subtract => composited(&below, top, true, border_box),
            },
        });
    }
    Some(MaskPaint {
        items: below.unwrap_or_default(),
        luminance: false,
    })
}

/// How far the outer shadows of a box reach out of its border box.
fn shadow_reach(style: &ComputedStyle) -> f32 {
    style
        .box_shadow
        .iter()
        .filter(|shadow| !shadow.inset)
        .map(|shadow| shadow.x.abs().max(shadow.y.abs()) + shadow.blur.max(0.0) + shadow.spread.max(0.0))
        .fold(0.0, f32::max)
}

/// A layer of luminance (what `items` draw) as alpha over `area`: black, masked by the
/// luminance. Chrome combines layers of luminance with those below as layers of alpha: `add`
/// draws one over the other (measured).
fn luminance_alpha(items: Vec<Item>, area: RectF) -> Vec<Item> {
    vec![
        Item::PushMask(std::sync::Arc::new(MaskPaint {
            items,
            luminance: true,
        })),
        Item::Rect {
            rect: area,
            color: fagus_style::values::Rgba::BLACK,
            radii: None,
        },
        Item::Pop,
    ]
}

/// The alpha of a layer within the layers `below` it (`intersect`, `a·b`), or outside them
/// (`subtract`, `a·(1 − b)`), as Chrome prints a layer of luminance (measured).
fn within(below: &[Item], layer: Vec<Item>, out: bool, area: RectF) -> Vec<Item> {
    let mut items = Vec::with_capacity(layer.len() + 2);
    items.push(alpha_of(below, out, area));
    items.extend(layer);
    items.push(Item::Pop);
    items
}

/// The alpha of a layer that intersects with the layers `below` it or subtracts them (`out`),
/// as Chrome prints it: Skia's PDF output draws what lies below where the layer is not, then
/// the layer within what lies below (or outside it): `a·b + b·(1 − a)·(1 − a·b)` for
/// `intersect`, `a·(1 − b) + b·(1 − a)·(1 − a·(1 − b))` for `subtract` (measured; `exclude` is
/// drawn as `add`).
fn composited(below: &[Item], layer: Vec<Item>, out: bool, area: RectF) -> Vec<Item> {
    let mut items = Vec::with_capacity(below.len() + layer.len() + 4);
    items.push(alpha_of(&layer, true, area));
    items.extend(below.iter().cloned());
    items.push(Item::Pop);
    items.push(alpha_of(below, out, area));
    items.extend(layer);
    items.push(Item::Pop);
    items
}

/// The rectangle that the images (or tiles) of a layer's `items` cover, within its clips.
fn drawn_area(items: &[Item]) -> Option<RectF> {
    let mut area: Option<RectF> = None;
    let mut clips: Vec<RectF> = Vec::new();
    let mut add = |rect: RectF, clips: &[RectF]| {
        let mut rect = rect;
        for clip in clips {
            rect = intersect(rect, *clip);
        }
        if rect.width <= 0.0 || rect.height <= 0.0 {
            return;
        }
        area = Some(match area {
            None => rect,
            Some(known) => {
                let (left, top) = (known.x.min(rect.x), known.y.min(rect.y));
                let right = (known.x + known.width).max(rect.x + rect.width);
                let bottom = (known.y + known.height).max(rect.y + rect.height);
                RectF {
                    x: left,
                    y: top,
                    width: right - left,
                    height: bottom - top,
                }
            }
        });
    };
    for item in items {
        match item {
            Item::PushClip(rect, _) => clips.push(*rect),
            Item::Pop => {
                clips.pop();
            }
            Item::Image { rect, .. } | Item::Gradient { rect, .. } | Item::Rect { rect, .. } => {
                add(*rect, &clips);
            }
            Item::Tiles { area, .. } => add(*area, &clips),
            _ => {}
        }
    }
    area
}

/// The part of `a` within `b` (empty where they do not meet).
fn intersect(a: RectF, b: RectF) -> RectF {
    let (left, top) = (a.x.max(b.x), a.y.max(b.y));
    let right = (a.x + a.width).min(b.x + b.width);
    let bottom = (a.y + a.height).min(b.y + b.height);
    RectF {
        x: left,
        y: top,
        width: (right - left).max(0.0),
        height: (bottom - top).max(0.0),
    }
}

/// The alpha of an image layer that intersects with the layers `below` it or subtracts them
/// (`out`), as Chrome prints it: what lies below outside the image's rectangle `shape`, then the
/// image within (or outside) what lies below: `b·(1 − s) + a·b` for `intersect`,
/// `b·(1 − s) + a·(1 − b)` for `subtract`.
fn shaped(below: &[Item], layer: Vec<Item>, out: bool, shape: RectF, area: RectF) -> Vec<Item> {
    let outline = Item::Rect {
        rect: shape,
        color: fagus_style::values::Rgba::BLACK,
        radii: None,
    };
    let mut items = Vec::with_capacity(below.len() + layer.len() + 4);
    items.push(alpha_of(&[outline], true, area));
    items.extend(below.iter().cloned());
    items.push(Item::Pop);
    items.push(alpha_of(below, out, area));
    items.extend(layer);
    items.push(Item::Pop);
    items
}

/// A mask of the alpha of `items`, or of its complement (`inverted`) within `area`.
fn alpha_of(items: &[Item], inverted: bool, area: RectF) -> Item {
    use fagus_style::values::Rgba;
    let paint = if inverted {
        // White where nothing is drawn, black as far as the items' alpha reaches: luminance.
        let black = Item::Rect {
            rect: area,
            color: Rgba::BLACK,
            radii: None,
        };
        MaskPaint {
            items: vec![
                Item::Rect {
                    rect: area,
                    color: Rgba::WHITE,
                    radii: None,
                },
                alpha_of(items, false, area),
                black,
                Item::Pop,
            ],
            luminance: true,
        }
    } else {
        MaskPaint {
            items: items.to_vec(),
            luminance: false,
        }
    };
    Item::PushMask(std::sync::Arc::new(paint))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fagus_style::values::Rgba;

    #[test]
    fn composites_image_layers_over_their_rectangles() {
        let rect = |x: f32, width: f32| RectF {
            x,
            y: 0.0,
            width,
            height: 10.0,
        };
        let image = |area: RectF| Item::Rect {
            rect: area,
            color: Rgba::BLACK,
            radii: None,
        };
        // Tiles under a clip cover the clip at most; images add up to their bounds.
        let tiles = [
            Item::PushClip(rect(2.0, 4.0), None),
            image(rect(0.0, 5.0)),
            image(rect(5.0, 5.0)),
            Item::Pop,
        ];
        assert_eq!(drawn_area(&tiles), Some(rect(2.0, 4.0)));
        assert_eq!(
            drawn_area(&[image(rect(0.0, 2.0)), image(rect(8.0, 2.0))]),
            Some(rect(0.0, 10.0))
        );
        assert_eq!(drawn_area(&[]), None);
        let below = image(rect(0.0, 10.0));
        let items = shaped(
            std::slice::from_ref(&below),
            vec![image(rect(0.0, 5.0))],
            true,
            rect(0.0, 5.0),
            rect(0.0, 10.0),
        );
        // What lies below, outside the image's rectangle; then the image outside what lies below.
        assert_eq!(items.len(), 6);
        assert!(matches!(&items[0], Item::PushMask(mask) if mask.luminance) && items[1] == below);
        assert!(matches!(&items[3], Item::PushMask(mask) if mask.luminance));
    }

    #[test]
    fn takes_svg_images_that_draw_nothing_for_no_mask() {
        let svg = fagus_layout::Image {
            data: std::sync::Arc::new(b"<svg xmlns='http://www.w3.org/2000/svg'/>".to_vec()),
            format: fagus_layout::ImageFormat::Svg,
            width: 10.0,
            height: 10.0,
            natural: (true, true),
            ratio: Some(1.0),
            inline: false,
            orientation: 1,
            filters: std::sync::Arc::from([]),
            pad: 0,
        };
        let mut images = Images::default();
        images.insert("masks.svg#m", std::sync::Arc::new(svg));
        let mask = MaskLayers {
            images: std::sync::Arc::from([Image::Url(std::sync::Arc::from("masks.svg#m"), 1.0)]),
            ..MaskLayers::default()
        };
        let border_box = RectF {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        };
        let style = ComputedStyle::default();
        let mut items = Vec::new();
        assert_eq!(push(&style, &mask, (border_box, None), &images, &mut items), 1);
        images.insert_blank("masks.svg#m");
        items.clear();
        assert_eq!(push(&style, &mask, (border_box, None), &images, &mut items), 0);
        assert_eq!(items, []);
    }

    #[test]
    fn masks_all_with_images_not_loaded_under_a_backdrop_filter() {
        let mask = MaskLayers {
            images: std::sync::Arc::from([Image::Url(std::sync::Arc::from("missing.png"), 1.0)]),
            ..MaskLayers::default()
        };
        let border_box = RectF {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        };
        let images = Images::default();
        let mut items = Vec::new();
        // An image that is not there masks nothing...
        assert_eq!(
            push(
                &ComputedStyle::default(),
                &mask,
                (border_box, None),
                &images,
                &mut items
            ),
            0
        );
        // ...but under a backdrop filter, all.
        let filtered = ComputedStyle {
            backdrop_filter: true,
            ..ComputedStyle::default()
        };
        assert_eq!(push(&filtered, &mask, (border_box, None), &images, &mut items), 1);
        assert!(matches!(&items[0], Item::PushMask(mask) if mask.items.is_empty() && !mask.luminance));
    }

    #[test]
    fn composites_layers_as_chrome_prints_them() {
        let area = RectF {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        };
        let layer = |color: Rgba| Item::Rect {
            rect: area,
            color,
            radii: None,
        };
        let (below, top) = (layer(Rgba::BLACK), layer(Rgba::WHITE));
        let alpha = |item: &Item, inverted: bool| match item {
            Item::PushMask(mask) => mask.luminance == inverted,
            _ => false,
        };
        // What lies below, where the layer is not; then the layer within (or outside) it.
        for out in [false, true] {
            let items = composited(std::slice::from_ref(&below), vec![top.clone()], out, area);
            assert_eq!(items.len(), 6);
            assert!(alpha(&items[0], true) && items[1] == below && items[2] == Item::Pop);
            assert!(alpha(&items[3], out) && items[4] == top && items[5] == Item::Pop);
        }
        // A layer of luminance: black under its luminance, within (or outside) the alpha below.
        let luminance = luminance_alpha(vec![top.clone()], area);
        assert!(alpha(&luminance[0], true) && luminance[2] == Item::Pop);
        assert!(matches!(luminance[1], Item::Rect { color, .. } if color == Rgba::BLACK));
        for out in [false, true] {
            let items = within(std::slice::from_ref(&below), luminance.clone(), out, area);
            assert_eq!(items.len(), 5);
            assert!(alpha(&items[0], out) && items[1..4] == luminance[..] && items[4] == Item::Pop);
        }
    }
}

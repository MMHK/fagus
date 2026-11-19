//! Backgrounds: the color and the image layers of a box (CSS Backgrounds 3), with gradients
//! resolved to geometry and color stops (CSS Images 3 and 4).

use std::cell::Cell;
use std::sync::Arc;

use fagus_style::ComputedStyle;
use fagus_style::values::{
    BackgroundRepeat, BackgroundSize, BlendMode, BoxArea, Gradient, GradientKind, GradientStop, Image,
    LengthPercentageAuto, LinearDirection, RadialShape, RadialSize, Rgba, StopPosition,
};

use crate::color::{default_interpolation, mix};
use crate::{GradientPaint, Item, MaskPaint, Radii, RectF};
use fagus_layout::Images;

/// More tiles than this are drawn as one pattern, not one by one.
const MAX_TILES: usize = 4096;

/// How many tiles a document draws one by one at most, all its backgrounds together; the
/// layers past them are drawn as patterns. A fixed box with a background of thousands of tiles
/// on each of thousands of pages took gigabytes.
const MAX_DOCUMENT_TILES: usize = 1_000_000;

thread_local! {
    /// How many more tiles the document being painted on this thread draws one by one.
    static TILES_LEFT: Cell<usize> = const { Cell::new(MAX_DOCUMENT_TILES) };
}

/// Runs `paint` (which paints a document) with all of [`MAX_DOCUMENT_TILES`] left to it, and
/// leaves what was left to a document painted around it as it was.
pub(crate) fn with_tile_budget<T>(paint: impl FnOnce() -> T) -> T {
    let outer = TILES_LEFT.with(|left| left.replace(MAX_DOCUMENT_TILES));
    let painted = paint();
    TILES_LEFT.with(|left| left.set(outer));
    painted
}

/// Whether `count` tiles are drawn one by one, which takes them from what the document has left.
fn one_by_one(count: usize) -> bool {
    count <= MAX_TILES
        && TILES_LEFT.with(|left| {
            let fits = count <= left.get();
            if fits {
                left.set(left.get() - count);
            }
            fits
        })
}

/// The farthest a color stop of a gradient lies from its start, in pixels.
const MAX_STOP: f32 = 1e9;

/// Where a background is drawn.
#[derive(Clone, Copy)]
pub(crate) struct Placement {
    /// The border box it is drawn in and its corner radii.
    pub(crate) border_box: RectF,
    pub(crate) radii: Option<Radii>,
    /// The border box its layers are sized and positioned in, if not that one: of a box
    /// broken across pages, the box drawn whole, of which this one is a slice.
    pub(crate) whole: Option<RectF>,
    /// The box its layers are sized and positioned in (`whole` or the border box) as laid out,
    /// before Chrome snapped it to whole pixels, where it did: images with natural sizes are
    /// sized and layers positioned in a positioning area as large as this box gives, at the
    /// corner of the snapped one (Blink's unsnapped positioning area).
    pub(crate) exact: Option<RectF>,
    /// Under an opaque border, which hides it, the background ends at the border's inner edge,
    /// so that the antialiased edges of the two do not let it show through, as Chrome avoids
    /// it bleeding.
    pub(crate) opaque_border: bool,
    /// Whether it covers all of the border box, whatever `background-clip` says: the page
    /// box's and the canvas's do.
    pub(crate) covers: bool,
    /// The area its fixed layers (`background-attachment: fixed`) are sized and positioned in,
    /// if not Chrome's window ([`FIXED_VIEWPORT`]): the canvas's (see `canvas_background`).
    pub(crate) viewport: Option<RectF>,
    /// The border box its local layers (`background-attachment: local`) are sized and
    /// positioned in, if not the box's own: that of a scroll container's scrollable overflow
    /// area (see [`scrollable_border_box`]).
    pub(crate) scrolled: Option<RectF>,
}

impl Placement {
    /// The background of a box with `border_box` and `radii`.
    pub(crate) fn of(border_box: RectF, radii: Option<Radii>) -> Self {
        Self {
            border_box,
            radii,
            whole: None,
            exact: None,
            opaque_border: false,
            covers: false,
            viewport: None,
            scrolled: None,
        }
    }
}

/// The border box of the scrollable overflow area of the box of `style` with the fragment
/// `fragment` and `border_box` (CSS Overflow 3, §2.2): its padding box reaching as far right and
/// down as its content does, and its end padding past that. `None` unless the box is a scroll
/// container with local background layers, and in other writing modes than horizontal
/// left-to-right, whose content overflows to other sides. The local layers of a scroll container
/// are sized and positioned in it (scrolled to its start when printed), as Chrome does; those of
/// another box in its padding box, as scrolling ones (CSS Backgrounds 3, §3.5).
pub(crate) fn scrollable_border_box(
    style: &ComputedStyle,
    fragment: &fagus_layout::Fragment,
    border_box: RectF,
) -> Option<RectF> {
    use fagus_style::values::Overflow;
    // `auto` and `scroll` compute to `hidden` here: printed, they do not scroll either.
    if !style.background_local.iter().any(|local| *local)
        || !(style.overflow_x == Overflow::Hidden || style.overflow_y == Overflow::Hidden)
        || style.is_vertical()
        || style.is_rtl()
    {
        return None;
    }
    let (mut right, mut bottom) = (0.0_f32, 0.0_f32);
    content_extent(fragment, (0.0, 0.0), &mut right, &mut bottom);
    let border = &style.border_width;
    let basis = border_box.width;
    let right = right + style.padding.right.resolve(basis) + border.right;
    let bottom = bottom + style.padding.bottom.resolve(basis) + border.bottom;
    Some(RectF {
        width: border_box.width.max(right),
        height: border_box.height.max(bottom),
        ..border_box
    })
}

/// Extends `right` and `bottom` to the boxes, text and images inside `fragment` (at `origin`
/// in the box being measured), into boxes that do not clip their own overflow.
fn content_extent(fragment: &fagus_layout::Fragment, origin: (f32, f32), right: &mut f32, bottom: &mut f32) {
    use fagus_layout::FragmentKind;
    for child in &fragment.children {
        let x = origin.0 + child.rect.x.to_px();
        let y = origin.1 + child.rect.y.to_px();
        match &child.kind {
            FragmentKind::Placeholder(_) => continue,
            // A line box is as wide as its container; what it holds may reach farther.
            FragmentKind::Line { .. } => {}
            FragmentKind::Box { style, .. }
                if style.overflow_x != fagus_style::values::Overflow::Visible
                    || style.overflow_y != fagus_style::values::Overflow::Visible =>
            {
                *right = right.max(x + child.rect.width.to_px());
                *bottom = bottom.max(y + child.rect.height.to_px());
                continue;
            }
            _ => {
                *right = right.max(x + child.rect.width.to_px());
                *bottom = bottom.max(y + child.rect.height.to_px());
            }
        }
        content_extent(child, (x, y), right, bottom);
    }
}

/// Paints the background of a box.
pub(crate) fn paint(style: &ComputedStyle, placement: Placement, images: &Images, items: &mut Vec<Item>) {
    paint_clipped(style, placement, images, ClipMasks::default(), items);
}

/// The mask of a background clipped to the border area (`background-clip: border-area`): the
/// borders of the box of `style` with the border box `rect`, `radii` and `sides`, opaque
/// whatever their color, where their styles and widths paint them (Blink's
/// `PaintFillLayerBorderAreaFillBox`, which paints them black into a mask). Empty where the box
/// has no border: the layers show nothing.
pub(crate) fn border_area(
    style: &ComputedStyle,
    rect: RectF,
    radii: Option<Radii>,
    sides: fagus_layout::VisibleSides,
) -> Arc<MaskPaint> {
    let mut black = style.clone();
    let opaque = Some(Rgba::BLACK);
    black.border_color = fagus_style::values::Sides {
        top: opaque,
        right: opaque,
        bottom: opaque,
        left: opaque,
    };
    let mut painted = Vec::new();
    crate::paint_borders(&black, rect, radii, sides, &mut painted);
    Arc::new(MaskPaint {
        items: painted,
        luminance: false,
    })
}

/// What the layers of a background with `background-clip: text` or `border-area` show through.
#[derive(Clone, Copy, Default)]
pub(crate) struct ClipMasks<'a> {
    /// The glyphs of the box's text.
    pub(crate) text: Option<&'a Arc<MaskPaint>>,
    /// Where the box's borders paint (see [`border_area`]).
    pub(crate) border_area: Option<&'a Arc<MaskPaint>>,
}

/// Paints the background of a box whose layers with `background-clip: text` show through
/// the text mask of `masks`, the glyphs of its text, and those with `border-area` through its
/// border-area mask (and both through either where they are both); the others are clipped to
/// their boxes alone, and the color as the bottom layer (Chrome 157, measured).
pub(crate) fn paint_clipped(
    style: &ComputedStyle,
    placement: Placement,
    images: &Images,
    masks: ClipMasks<'_>,
    items: &mut Vec<Item>,
) {
    let Placement {
        border_box,
        radii,
        whole,
        exact,
        opaque_border,
        covers,
        viewport,
        scrolled,
    } = placement;
    let layers = style.background_image.len().max(1);
    let to_border_area = |index: usize| {
        !covers
            && masks.border_area.is_some()
            && cycle(&style.background_clip_border_area, index).unwrap_or(false)
    };
    // The local layers of a scroll container scroll with its content, inside its padding box,
    // with the color where it is their bottom layer: they do not reach under the border
    // (Chrome 157, measured).
    let local = |index: usize| scrolled.is_some() && cycle(&style.background_local, index).unwrap_or(false);
    let clip_of = |index: usize| match cycle(&style.background_clip, index).unwrap_or(BoxArea::BorderBox) {
        _ if covers || to_border_area(index) => BoxArea::BorderBox,
        BoxArea::BorderBox if opaque_border || local(index) => BoxArea::PaddingBox,
        clip => clip,
    };
    // Layers with a blend mode blend with the layers and the color below them alone: the
    // background is an isolated group (Compositing 1, §6.2).
    let blend_of = |index: usize| cycle(&style.background_blend_mode, index).unwrap_or_default();
    let blends = (0..style.background_image.len()).any(|index| blend_of(index) != BlendMode::Normal);
    let start = items.len();
    if blends {
        items.push(Item::PushIsolated);
    }

    // A layer clipped to the text and to the border area shows through both.
    let mut both: Option<Arc<MaskPaint>> = None;
    let mut mask_of = |index: usize| -> Option<Arc<MaskPaint>> {
        let text = masks
            .text
            .filter(|_| cycle(&style.background_clip_text, index).unwrap_or(false));
        let border = masks.border_area.filter(|_| to_border_area(index));
        match (text, border) {
            (Some(text), Some(border)) => Some(Arc::clone(both.get_or_insert_with(|| {
                Arc::new(MaskPaint {
                    items: border.items.iter().chain(&text.items).cloned().collect(),
                    luminance: false,
                })
            }))),
            (mask, None) | (None, mask) => mask.cloned(),
        }
    };

    // The color is clipped like the bottom layer. Under an opaque border with rounded corners,
    // Chrome ends it halfway into the border, where the border's antialiased inner edge lies
    // on it (Blink's `kBackgroundBleedShrinkBackground`), not at that edge, which would show
    // a seam between the two.
    if !style.background_color.is_transparent() {
        let under_border = opaque_border
            && radii.is_some()
            && clip_of(layers - 1) == BoxArea::PaddingBox
            && cycle(&style.background_clip, layers - 1).unwrap_or(BoxArea::BorderBox) == BoxArea::BorderBox;
        let (rect, rect_radii) = if under_border {
            halfway_into_border(style, border_box, radii)
        } else {
            area(style, border_box, radii, clip_of(layers - 1))
        };
        let mask = mask_of(layers - 1);
        if let Some(mask) = &mask {
            items.push(Item::PushMask(Arc::clone(mask)));
        }
        items.push(Item::Rect {
            rect,
            color: style.background_color,
            radii: rect_radii,
        });
        if mask.is_some() {
            items.push(Item::Pop);
        }
    }

    for index in (0..style.background_image.len()).rev() {
        let geometry = LayerGeometry {
            whole: scrolled.filter(|_| local(index)).or(whole),
            exact: exact.filter(|_| !local(index)),
            origin: cycle(&style.background_origin, index).unwrap_or(BoxArea::PaddingBox),
            fixed: cycle(&style.background_fixed, index)
                .unwrap_or(false)
                .then(|| viewport.unwrap_or(FIXED_VIEWPORT)),
            clip: Some(clip_of(index)),
            size: cycle(&style.background_size, index).unwrap_or(BackgroundSize::Explicit(
                LengthPercentageAuto::Auto,
                LengthPercentageAuto::Auto,
            )),
            repeat: cycle(&style.background_repeat, index)
                .unwrap_or((BackgroundRepeat::Repeat, BackgroundRepeat::Repeat)),
            position: (
                cycle(&style.background_position_x, index),
                cycle(&style.background_position_y, index),
            ),
        };
        let blend = blend_of(index);
        let at = items.len();
        if blend != BlendMode::Normal {
            items.push(Item::PushBlend(blend));
        }
        let mask = mask_of(index);
        if let Some(mask) = &mask {
            items.push(Item::PushMask(Arc::clone(mask)));
        }
        if !layer(
            (style, border_box, radii),
            &style.background_image[index],
            &geometry,
            images,
            items,
        ) {
            items.truncate(at);
            continue;
        }
        if mask.is_some() {
            items.push(Item::Pop);
        }
        if blend != BlendMode::Normal {
            items.push(Item::Pop);
        }
    }
    if blends {
        items.push(Item::Pop);
    }
    if let Some(text) = masks.text {
        text_mask_edges(text, border_box, items, start);
    }
}

/// The device pixels of Chrome's PDFs: 300 per inch, 3.125 per CSS pixel.
const DEVICE_PIXELS: f32 = 300.0 / 96.0;

/// Chrome draws a background clipped to the text (Skia's PDF device, which clips a layer to a
/// mask by drawing it twice) once more, before it, through the inverse of a group as large as
/// the border box rounded out to device pixels: this shows nothing but a faint line along the
/// edges of that group, where viewers such as Poppler and pdf.js blend its antialiased edge,
/// about a quarter of the background's color. Fagus draws the same: the items that paint the
/// background from `start` on, their text masks taken away, through a luminosity mask that is
/// black inside the rounded box and white around it.
fn text_mask_edges(text: &Arc<MaskPaint>, border_box: RectF, items: &mut Vec<Item>, start: usize) {
    if !items[start..]
        .iter()
        .any(|item| matches!(item, Item::PushMask(mask) if Arc::ptr_eq(mask, text)))
        || border_box.width <= 0.0
        || border_box.height <= 0.0
    {
        return;
    }
    let device = |value: f32, round: fn(f32) -> f32| round(value * DEVICE_PIXELS) / DEVICE_PIXELS;
    let left = device(border_box.x, f32::floor);
    let top = device(border_box.y, f32::floor);
    let rounded = RectF {
        x: left,
        y: top,
        width: device(border_box.x + border_box.width, f32::ceil) - left,
        height: device(border_box.y + border_box.height, f32::ceil) - top,
    };
    let margin = 2.0;
    let around = RectF {
        x: rounded.x - margin,
        y: rounded.y - margin,
        width: rounded.width + 2.0 * margin,
        height: rounded.height + 2.0 * margin,
    };
    let edges = Arc::new(MaskPaint {
        items: vec![
            Item::Rect {
                rect: around,
                color: Rgba::WHITE,
                radii: None,
            },
            Item::Rect {
                rect: rounded,
                color: Rgba::BLACK,
                radii: None,
            },
        ],
        luminance: true,
    });
    let mut again = Vec::with_capacity(items.len() - start + 2);
    again.push(Item::PushMask(edges));
    again.extend(items[start..].iter().map(|item| match item {
        // An isolated group in its place keeps the pushes and pops balanced.
        Item::PushMask(mask) if Arc::ptr_eq(mask, text) => Item::PushIsolated,
        item => item.clone(),
    }));
    again.push(Item::Pop);
    items.splice(start..start, again);
}

/// The area a fixed background layer (`background-attachment: fixed`) is sized and positioned
/// in when Chrome prints: the window it loaded the document in, 756 × 469 CSS pixels, at the top
/// left of every page, margins included (Chrome 157, measured: `center / 50% 50%` draws an
/// image 378 × 234.5 pixels large at (189, 117.25) on each page).
const FIXED_VIEWPORT: RectF = RectF {
    x: 0.0,
    y: 0.0,
    width: 756.0,
    height: 469.0,
};

/// Where and how an image layer is painted: the properties of a background layer (CSS
/// Backgrounds 3, §3), which mask layers share.
pub(crate) struct LayerGeometry {
    /// The border box the layer is sized and positioned in, if not the box's own.
    pub(crate) whole: Option<RectF>,
    /// That border box before it was snapped to whole pixels (see [`Placement::exact`]).
    pub(crate) exact: Option<RectF>,
    pub(crate) origin: BoxArea,
    /// The viewport the layer is sized and positioned in where it is fixed
    /// (`background-attachment: fixed`).
    pub(crate) fixed: Option<RectF>,
    /// `None` where the layer is not clipped (`mask-clip: no-clip`).
    pub(crate) clip: Option<BoxArea>,
    pub(crate) size: BackgroundSize,
    pub(crate) repeat: (BackgroundRepeat, BackgroundRepeat),
    pub(crate) position: (
        Option<fagus_style::values::LengthPercentage>,
        Option<fagus_style::values::LengthPercentage>,
    ),
}

/// Paints the tiles of an image layer of the box of `style` with `border_box` and `radii`;
/// `false` if it shows nothing.
pub(crate) fn layer(
    (style, border_box, radii): (&ComputedStyle, RectF, Option<Radii>),
    image: &Image,
    geometry: &LayerGeometry,
    images: &Images,
    items: &mut Vec<Item>,
) -> bool {
    let layer = match image {
        Image::Gradient(gradient) => Layer::Gradient(gradient),
        Image::Url(url, resolution) => match images.get(url) {
            Some(image) => Layer::Image(fagus_layout::Image::zoomed(
                &fagus_layout::Image::styled(&fagus_layout::Image::at_resolution(image, *resolution), style),
                style.zoom,
            )),
            None => return false,
        },
        Image::None => return false,
    };
    // Without a clip, the tiles cover the border box and reach out of it.
    let (clip, clip_radii) = area(
        style,
        border_box,
        radii,
        geometry.clip.unwrap_or(BoxArea::BorderBox),
    );
    let (positioning, _) = if let Some(viewport) = geometry.fixed {
        (viewport, None)
    } else {
        area(style, geometry.whole.unwrap_or(border_box), None, geometry.origin)
    };
    // The positioning area as large as the box as laid out gives it, at the corner of the
    // snapped one: Chrome sizes images with natural sizes in it (others and `contain` and
    // `cover` in the snapped one) and positions layers in it, all but spaced ones (Blink's
    // `BackgroundImageGeometry`).
    let exact = geometry
        .exact
        .filter(|_| geometry.fixed.is_none())
        .map(|exact| area(style, exact, None, geometry.origin).0);
    let unsnapped = exact.map_or(positioning, |exact| RectF {
        width: exact.width,
        height: exact.height,
        ..positioning
    });
    let (size, repeat) = (&geometry.size, geometry.repeat);
    let mut tile = match &layer {
        Layer::Gradient(_) => tile_size(size, positioning),
        Layer::Image(image)
            if matches!(size, BackgroundSize::Explicit(..)) && (image.natural.0 || image.natural.1) =>
        {
            image_tile_size(size, unsnapped, image)
        }
        Layer::Image(image) => image_tile_size(size, positioning, image),
    };
    // `round` scales the tile so that a whole number fits; where it rounds one axis only and
    // the size of the other is `auto`, that one scales with it, keeping the ratio (CSS
    // Backgrounds 3, §3.4).
    let natural = tile;
    if repeat.0 == BackgroundRepeat::Round && tile.0 > 0.0 {
        tile.0 = positioning.width / (positioning.width / tile.0).round().max(1.0);
    }
    if repeat.1 == BackgroundRepeat::Round && tile.1 > 0.0 {
        tile.1 = positioning.height / (positioning.height / tile.1).round().max(1.0);
    }
    if let BackgroundSize::Explicit(width, height) = size {
        let rounds = |axis: BackgroundRepeat| axis == BackgroundRepeat::Round;
        if rounds(repeat.0) && !rounds(repeat.1) && *height == LengthPercentageAuto::Auto && natural.0 > 0.0 {
            tile.1 *= tile.0 / natural.0;
        } else if rounds(repeat.1)
            && !rounds(repeat.0)
            && *width == LengthPercentageAuto::Auto
            && natural.1 > 0.0
        {
            tile.0 *= tile.1 / natural.1;
        }
    }
    // Chrome's tile sizes are in layout units (1/64 pixel, cut toward zero): a 13.3 pixels wide
    // image repeats every 13.296875 pixels.
    if exact.is_some() && matches!(layer, Layer::Image(_)) {
        tile = ((tile.0 * 64.0).trunc() / 64.0, (tile.1 * 64.0).trunc() / 64.0);
    }
    if tile.0 <= 0.0 || tile.1 <= 0.0 {
        return false;
    }
    // Chrome draws an SVG image into tiles of whole pixels: one less than half a pixel wide or
    // tall rounds to nothing (raster images and gradients still show; measured, Chrome 157).
    if matches!(&layer, Layer::Image(image) if image.format == fagus_layout::ImageFormat::Svg)
        && (tile.0 < 0.5 || tile.1 < 0.5)
    {
        return false;
    }
    let (position_x, position_y) = &geometry.position;
    // A layer that repeats is placed in the unsnapped area. One that does not, too, and where
    // it starts inside the area, it is moved to whole pixels and an image is drawn as many
    // pixels large as it rounds to, as Chrome snaps its destination rectangle (`SetNoRepeatX`).
    // Spaced and rounded ones fill the snapped area.
    let snaps = matches!(layer, Layer::Image(_));
    let place = |position: Option<&fagus_style::values::LengthPercentage>,
                 repeat: BackgroundRepeat,
                 (start, length, tile): (f32, f32, f32),
                 exact: Option<(f32, f32)>| {
        let resolve = |length: f32| position.map_or(0.0, |position| position.resolve(length - tile));
        // In layout units (1/64 pixel, cut toward zero) as Chrome resolves them, which decides
        // which way an offset of half a pixel rounds.
        let units = |value: f32| (value * 64.0).trunc() / 64.0;
        let exact_resolve =
            |length: f32| position.map_or(0.0, |position| units(position.resolve(length - units(tile))));
        match (repeat, exact) {
            (BackgroundRepeat::Repeat, Some((_, exact_length))) => {
                (start + exact_resolve(exact_length), tile)
            }
            (BackgroundRepeat::NoRepeat, Some((exact_start, exact_length))) => {
                let offset = exact_resolve(exact_length);
                if offset >= 0.0 {
                    let at = (exact_start + offset + 0.5).floor();
                    (
                        at,
                        if snaps {
                            (tile + 0.5).floor().max(1.0)
                        } else {
                            tile
                        },
                    )
                } else {
                    (start + offset, tile)
                }
            }
            _ => (start + resolve(length), tile),
        }
    };
    let (x, width) = place(
        position_x.as_ref(),
        repeat.0,
        (positioning.x, positioning.width, tile.0),
        exact.map(|exact| (exact.x, exact.width)),
    );
    let (y, height) = place(
        position_y.as_ref(),
        repeat.1,
        (positioning.y, positioning.height, tile.1),
        exact.map(|exact| (exact.y, exact.height)),
    );
    let tile = (width, height);
    let paint = match layer {
        Layer::Gradient(gradient) => match gradient_paint(gradient, tile.0, tile.1, style.color) {
            Some(paint) => Tile::Gradient(Arc::new(paint)),
            None => return false,
        },
        Layer::Image(image) => Tile::Image(image),
    };
    // Gradient tiles smaller than a pixel both ways, repeated, blend into their average color,
    // as Chrome prints them (rasterized).
    if let Tile::Gradient(gradient) = &paint
        && tile.0 < 1.0
        && tile.1 < 1.0
        && repeat.0 != BackgroundRepeat::NoRepeat
        && repeat.1 != BackgroundRepeat::NoRepeat
    {
        let color = match &**gradient {
            GradientPaint::Solid(color) => *color,
            GradientPaint::Linear { stops, .. }
            | GradientPaint::Radial { stops, .. }
            | GradientPaint::Conic { stops, .. } => average(stops),
        };
        items.push(Item::Rect {
            rect: clip,
            color,
            radii: clip_radii,
        });
        return true;
    }
    let xs = positions(
        x,
        tile.0,
        repeat.0,
        clip.x,
        clip.width,
        positioning.x,
        positioning.width,
    );
    let ys = positions(
        y,
        tile.1,
        repeat.1,
        clip.y,
        clip.height,
        positioning.y,
        positioning.height,
    );
    if geometry.clip.is_some() {
        items.push(Item::PushClip(clip, clip_radii));
    }
    if !one_by_one(xs.len() * ys.len()) {
        // As one pattern: over the painting area, where the layer repeats along an axis, or
        // its tiles span.
        let axis = |positions: &[f32], size: f32, repeat: BackgroundRepeat, (start, length): (f32, f32)| {
            let (first, last) = (positions[0], positions[positions.len() - 1]);
            let (from, to) = match (positions.len(), repeat) {
                (1, _) | (_, BackgroundRepeat::Space) => (first, last + size),
                _ => (start, start + length),
            };
            let step = positions.get(1).map_or(size, |second| second - first);
            (first, step, from.max(start), to.min(start + length))
        };
        let (origin_x, step_x, left, right) = axis(&xs, tile.0, repeat.0, (clip.x, clip.width));
        let (origin_y, step_y, top, bottom) = axis(&ys, tile.1, repeat.1, (clip.y, clip.height));
        let rect = RectF {
            x: 0.0,
            y: 0.0,
            width: tile.0,
            height: tile.1,
        };
        items.push(Item::Tiles {
            area: RectF {
                x: left,
                y: top,
                width: (right - left).max(0.0),
                height: (bottom - top).max(0.0),
            },
            origin: (origin_x, origin_y),
            step: (step_x, step_y),
            tile: Arc::new(match paint {
                Tile::Gradient(gradient) => Item::Gradient { rect, gradient },
                Tile::Image(image) => Item::Image { rect, image },
            }),
        });
        if geometry.clip.is_some() {
            items.push(Item::Pop);
        }
        return true;
    }
    // Gradient tiles that touch and look alike where they meet are drawn as one: those of a
    // gradient that changes along one axis alone, in rows or columns across it, and those of
    // one color all as one. Drawn one by one, the antialiased edges of the tiles would show
    // as lines between them, where Chrome, which paints the layer as one image, shows none.
    let (along_x, along_y) = match &paint {
        Tile::Gradient(gradient) => uniform_axes(gradient),
        Tile::Image(_) => (false, false),
    };
    let runs_x = if along_x {
        runs(&xs, tile.0)
    } else {
        xs.iter().map(|&x| (x, tile.0)).collect()
    };
    let runs_y = if along_y {
        runs(&ys, tile.1)
    } else {
        ys.iter().map(|&y| (y, tile.1)).collect()
    };
    for &(tile_y, height) in &runs_y {
        for &(tile_x, width) in &runs_x {
            let rect = RectF {
                x: tile_x,
                y: tile_y,
                width,
                height,
            };
            items.push(match &paint {
                Tile::Gradient(gradient) => Item::Gradient {
                    rect,
                    gradient: Arc::clone(gradient),
                },
                Tile::Image(image) => Item::Image {
                    rect,
                    image: Arc::clone(image),
                },
            });
        }
    }
    if geometry.clip.is_some() {
        items.push(Item::Pop);
    }
    true
}

/// What a background layer shows.
enum Layer<'a> {
    Gradient(&'a Gradient),
    Image(Arc<fagus_layout::Image>),
}

/// What each tile of a layer shows.
enum Tile {
    Gradient(Arc<GradientPaint>),
    Image(Arc<fagus_layout::Image>),
}

/// The size of one tile of an image (CSS Backgrounds 3, §3.9, with the default sizing algorithm
/// of CSS Images 3, §5.3): sizes the image lacks come from its natural ratio or else from the
/// background positioning area, as for an SVG without `width` and `height`.
fn image_tile_size(size: &BackgroundSize, area: RectF, image: &fagus_layout::Image) -> (f32, f32) {
    let natural_width = image.natural.0.then_some(image.width);
    let natural_height = image.natural.1.then_some(image.height);
    let ratio = image.ratio.filter(|ratio| ratio.is_finite() && *ratio > 0.0);
    // The largest (`contain`) or smallest (`cover`) size of the ratio that fits the area.
    let fit = |cover: bool| match ratio {
        Some(ratio) if area.height > 0.0 => {
            let width = if cover {
                area.width.max(area.height * ratio)
            } else {
                area.width.min(area.height * ratio)
            };
            (width, width / ratio)
        }
        _ => (area.width, area.height),
    };
    match size {
        BackgroundSize::Explicit(explicit_width, explicit_height) => {
            match (explicit_width.length(), explicit_height.length()) {
                (None, None) => match (natural_width, natural_height, ratio) {
                    (Some(width), Some(height), _) => (width, height),
                    (Some(width), None, Some(ratio)) => (width, width / ratio),
                    (None, Some(height), Some(ratio)) => (height * ratio, height),
                    (Some(width), None, None) => (width, area.height),
                    (None, Some(height), None) => (area.width, height),
                    (None, None, _) => fit(false),
                },
                (Some(w), None) => {
                    let w = w.resolve(area.width);
                    (
                        w,
                        ratio.map_or(natural_height.unwrap_or(area.height), |ratio| w / ratio),
                    )
                }
                (None, Some(h)) => {
                    let h = h.resolve(area.height);
                    (
                        ratio.map_or(natural_width.unwrap_or(area.width), |ratio| h * ratio),
                        h,
                    )
                }
                (Some(w), Some(h)) => (w.resolve(area.width), h.resolve(area.height)),
            }
        }
        BackgroundSize::Contain => fit(false),
        BackgroundSize::Cover => fit(true),
    }
}

/// The value of a background list for layer `index`: lists repeat to match the layer count.
fn cycle<T: Clone>(list: &[T], index: usize) -> Option<T> {
    (!list.is_empty()).then(|| list[index % list.len()].clone())
}

/// A box of the box model and its corner radii (CSS Backgrounds 3, §5.2: inner radii shrink by
/// the border and padding widths).
pub(crate) fn area(
    style: &ComputedStyle,
    border_box: RectF,
    radii: Option<Radii>,
    area: BoxArea,
) -> (RectF, Option<Radii>) {
    let border = &style.border_width;
    let mut inset = match area {
        BoxArea::BorderBox => return (border_box, radii),
        BoxArea::MarginBox => {
            // Its margins outside, of the box's own width as padding (see below); `auto` ones
            // are none. Rounded corners grow with them.
            let basis = border_box.width;
            let margin = |side: &LengthPercentageAuto| match side {
                LengthPercentageAuto::Length(length) => length.resolve(basis),
                LengthPercentageAuto::Auto => 0.0,
            };
            let [top, right, bottom, left] = [
                margin(&style.margin.top),
                margin(&style.margin.right),
                margin(&style.margin.bottom),
                margin(&style.margin.left),
            ];
            let rect = RectF {
                x: border_box.x - left,
                y: border_box.y - top,
                width: (border_box.width + left + right).max(0.0),
                height: (border_box.height + top + bottom).max(0.0),
            };
            let radii = radii.map(|[top_left, top_right, bottom_right, bottom_left]| {
                // A radius less than the margin grows by less (CSS Backgrounds 3, §7.3, as
                // for spread shadows): `r + m·(1 + (r/m − 1)³)`.
                let spread = |radius: f32, margin: f32| {
                    if margin > 0.0 && radius < margin {
                        radius + margin * (1.0 + (radius / margin - 1.0).powi(3))
                    } else {
                        radius + margin.max(0.0)
                    }
                };
                let grow = |(rx, ry): (f32, f32), x: f32, y: f32| {
                    if rx > 0.0 && ry > 0.0 {
                        (spread(rx, x), spread(ry, y))
                    } else {
                        (rx, ry)
                    }
                };
                [
                    grow(top_left, left, top),
                    grow(top_right, right, top),
                    grow(bottom_right, right, bottom),
                    grow(bottom_left, left, bottom),
                ]
            });
            return (rect, radii);
        }
        BoxArea::PaddingBox | BoxArea::ContentBox => [border.top, border.right, border.bottom, border.left],
    };
    if area == BoxArea::ContentBox {
        // Percentages of padding refer to the containing block's width, which painting does not
        // know; the box's own width is close for most boxes.
        let basis = border_box.width;
        inset[0] += style.padding.top.resolve(basis);
        inset[1] += style.padding.right.resolve(basis);
        inset[2] += style.padding.bottom.resolve(basis);
        inset[3] += style.padding.left.resolve(basis);
    }
    let rect = RectF {
        x: border_box.x + inset[3],
        y: border_box.y + inset[0],
        width: (border_box.width - inset[1] - inset[3]).max(0.0),
        height: (border_box.height - inset[0] - inset[2]).max(0.0),
    };
    let radii = radii.map(|[top_left, top_right, bottom_right, bottom_left]| {
        let shrink = |(rx, ry): (f32, f32), x: f32, y: f32| ((rx - x).max(0.0), (ry - y).max(0.0));
        [
            shrink(top_left, inset[3], inset[0]),
            shrink(top_right, inset[1], inset[0]),
            shrink(bottom_right, inset[1], inset[2]),
            shrink(bottom_left, inset[3], inset[2]),
        ]
    });
    (rect, radii)
}

/// The border box of a box inset by half its border widths, with its corner radii shrunk as
/// much.
fn halfway_into_border(
    style: &ComputedStyle,
    border_box: RectF,
    radii: Option<Radii>,
) -> (RectF, Option<Radii>) {
    let border = &style.border_width;
    let [top, right, bottom, left] =
        [border.top, border.right, border.bottom, border.left].map(|width| width / 2.0);
    let rect = RectF {
        x: border_box.x + left,
        y: border_box.y + top,
        width: (border_box.width - left - right).max(0.0),
        height: (border_box.height - top - bottom).max(0.0),
    };
    let radii = radii.map(|[top_left, top_right, bottom_right, bottom_left]| {
        let shrink = |(rx, ry): (f32, f32), x: f32, y: f32| ((rx - x).max(0.0), (ry - y).max(0.0));
        [
            shrink(top_left, left, top),
            shrink(top_right, right, top),
            shrink(bottom_right, right, bottom),
            shrink(bottom_left, left, bottom),
        ]
    });
    (rect, radii)
}

/// The size of one tile of an image without intrinsic size (a gradient).
fn tile_size(size: &BackgroundSize, area: RectF) -> (f32, f32) {
    match size {
        BackgroundSize::Explicit(width, height) => (
            width
                .length()
                .map_or(area.width, |width| width.resolve(area.width)),
            height
                .length()
                .map_or(area.height, |height| height.resolve(area.height)),
        ),
        BackgroundSize::Cover | BackgroundSize::Contain => (area.width, area.height),
    }
}

/// Along which axes (x, y) a gradient tile looks the same everywhere: a linear gradient
/// across the other one, or one of a single color.
fn uniform_axes(gradient: &GradientPaint) -> (bool, bool) {
    let one_color = |stops: &[(f32, Rgba)]| stops.windows(2).all(|pair| pair[0].1 == pair[1].1);
    match gradient {
        GradientPaint::Solid(_) => (true, true),
        GradientPaint::Linear { stops, .. }
        | GradientPaint::Radial { stops, .. }
        | GradientPaint::Conic { stops, .. }
            if one_color(stops) =>
        {
            (true, true)
        }
        GradientPaint::Linear { start, end, .. } => {
            let (dx, dy) = ((end.0 - start.0).abs(), (end.1 - start.1).abs());
            let tolerance = 1e-5 * (dx + dy);
            (dx <= tolerance, dy <= tolerance)
        }
        GradientPaint::Radial { .. } | GradientPaint::Conic { .. } => (false, false),
    }
}

/// The tiles of `size` at `positions` along one axis, those that touch joined: their starts and
/// lengths.
fn runs(positions: &[f32], size: f32) -> Vec<(f32, f32)> {
    let mut runs: Vec<(f32, f32)> = Vec::new();
    for &position in positions {
        match runs.last_mut() {
            Some((start, length)) if (*start + *length - position).abs() < 1e-3 => *length += size,
            _ => runs.push((position, size)),
        }
    }
    runs
}

/// The start positions of the tiles along one axis.
fn positions(
    position: f32,
    tile: f32,
    repeat: BackgroundRepeat,
    clip_start: f32,
    clip_length: f32,
    area_start: f32,
    area_length: f32,
) -> Vec<f32> {
    let clip_end = clip_start + clip_length;
    match repeat {
        BackgroundRepeat::NoRepeat => vec![position],
        BackgroundRepeat::Space => {
            let count = (area_length / tile).floor();
            if count < 2.0 {
                return vec![position];
            }
            // Spaced out in the positioning area, and on at that pace over the painting area
            // around it (a border box that reaches past a padding box), as Chrome tiles them.
            let period = tile + (area_length - count * tile) / (count - 1.0);
            let first = area_start - ((area_start - clip_start) / period).ceil().max(0.0) * period;
            let count = ((clip_end - first) / period).ceil().max(0.0) as usize;
            (0..count.min(MAX_TILES + 1))
                .map(|index| first + index as f32 * period)
                .collect()
        }
        BackgroundRepeat::Repeat | BackgroundRepeat::Round => {
            let first = position - ((position - clip_start) / tile).ceil() * tile;
            let count = ((clip_end - first) / tile).ceil().max(0.0) as usize;
            (0..count.min(MAX_TILES + 1))
                .map(|index| first + index as f32 * tile)
                .collect()
        }
    }
}

/// A color stop or a hint with its position on the gradient line.
#[derive(Clone, Copy, Debug)]
enum Stop {
    Color(f32, Rgba),
    Hint(f32),
}

/// Resolves the positions of color stops and hints (CSS Images 3, §3.5.3). `length` is the
/// length of the gradient line (360 for conic gradients, whose positions are in degrees).
fn resolve_stops(stops: &[GradientStop], length: f32, current: Rgba) -> Vec<Stop> {
    // Positions beyond any float (`calc(1e39%)`) are as far as can be, as in Chrome.
    let position = |position: &StopPosition| {
        let at = match position {
            StopPosition::Length(length_percentage) => length_percentage.resolve(length),
            StopPosition::Angle(degrees) => *degrees,
        };
        if at.is_nan() {
            0.0
        } else {
            at.clamp(-MAX_STOP, MAX_STOP)
        }
    };
    let count = stops
        .iter()
        .filter(|stop| matches!(stop, GradientStop::Color { .. }))
        .count();
    let mut resolved: Vec<(bool, Option<f32>, Rgba)> = Vec::with_capacity(stops.len());
    // The colors the gradient goes on from at stops, where they differ from the stops' own.
    let mut afters: Vec<Option<Rgba>> = Vec::with_capacity(stops.len());
    let mut color_index = 0;
    for stop in stops {
        match stop {
            GradientStop::Color {
                color,
                position: stop_position,
                after,
                ..
            } => {
                let mut at = stop_position.as_ref().map(position);
                if at.is_none() && color_index == 0 {
                    at = Some(0.0);
                }
                if at.is_none() && color_index + 1 == count {
                    at = Some(length);
                }
                resolved.push((true, at, color.unwrap_or(current)));
                afters.push(*after);
                color_index += 1;
            }
            GradientStop::Hint(hint) => {
                resolved.push((false, Some(position(hint)), Rgba::TRANSPARENT));
                afters.push(None);
            }
        }
    }
    // Positions never go back.
    let mut highest = f32::NEG_INFINITY;
    for entry in &mut resolved {
        if let Some(at) = &mut entry.1 {
            *at = at.max(highest);
            highest = *at;
        }
    }
    // Stops without a position are spread evenly between their neighbors.
    let mut index = 0;
    while index < resolved.len() {
        if resolved[index].1.is_some() {
            index += 1;
            continue;
        }
        let start = index;
        while index < resolved.len() && resolved[index].1.is_none() {
            index += 1;
        }
        let before = resolved[start - 1].1.unwrap_or(0.0);
        let after = resolved.get(index).and_then(|entry| entry.1).unwrap_or(before);
        let steps = (index - start + 1) as f32;
        for (offset, entry) in resolved[start..index].iter_mut().enumerate() {
            entry.1 = Some(before + (after - before) * (offset + 1) as f32 / steps);
        }
    }
    let mut result = Vec::with_capacity(resolved.len());
    for ((is_color, at, color), after) in resolved.into_iter().zip(afters) {
        let at = at.unwrap_or(0.0);
        if is_color {
            result.push(Stop::Color(at, color));
            if let Some(after) = after {
                result.push(Stop::Color(at, after));
            }
        } else {
            result.push(Stop::Hint(at));
        }
    }
    result
}

/// Turns stops into the color stops of a PDF shading, which interpolates linearly in sRGB without
/// premultiplied alpha, as Chrome prints gradients: its PDF backend interpolates the stops Blink
/// gives it in sRGB whatever color space the gradient names (Chrome 157, measured), and Blink
/// puts nine stops in place of each color hint, mixed in that space (its
/// `ReplaceColorHintsWithColorStops`). Segments with a different alpha at their ends are
/// sampled (`samples` steps), mixed with premultiplied alpha.
///
/// A gradient with a translucent stop Chrome draws as an image, interpolated in the gradient's
/// own space (Chrome 157, measured: `in hsl` from red to a transparent green passes yellow):
/// its segments are all sampled, mixed in that space.
fn shading_stops(stops: &[Stop], gradient: &Gradient, samples: usize) -> Vec<(f32, Rgba)> {
    let srgb = default_interpolation(true);
    let space = gradient.interpolation.unwrap_or(srgb);
    let hinted = with_hints_replaced(stops, space);
    let translucent = space != srgb && hinted.iter().any(|(_, color)| color.a < 1.0);
    let (mixing, samples) = if translucent {
        (space, samples.max(polar_samples(space)))
    } else {
        (srgb, samples)
    };
    let mut result: Vec<(f32, Rgba)> = Vec::with_capacity(hinted.len());
    for (index, &(at, color)) in hinted.iter().enumerate() {
        if let Some(&(start, from)) = index.checked_sub(1).and_then(|previous| hinted.get(previous)) {
            let span = at - start;
            if span > 0.0 && ((from.a - color.a).abs() > f32::EPSILON || (translucent && from != color)) {
                for step in 1..samples {
                    let t = step as f32 / samples as f32;
                    result.push((start + span * t, mix(from, color, t, mixing)));
                }
            }
        }
        result.push((at, color));
    }
    with_transparent_stops_tinted(&result)
}

/// How many samples a segment mixed in `interpolation` takes at least: more where the hue
/// turns, as far as a whole turn (`longer hue`), so that the sRGB shading between the samples
/// stays close to the hues.
fn polar_samples(interpolation: fagus_style::values::Interpolation) -> usize {
    use fagus_style::values::ColorSpace;
    match interpolation.space {
        ColorSpace::Hsl | ColorSpace::Hwb | ColorSpace::Lch | ColorSpace::Oklch => 48,
        _ => 16,
    }
}

/// The stops with each fully transparent one given the color of its nearest visible neighbors,
/// split in two at a stop between two different ones. Mixed with premultiplied alpha, as CSS
/// mixes gradients, a transparent stop's own color never shows; the PDF shading mixes colors and
/// alpha apart, so a viewer's sampling of the edges of a hard stop from `transparent` (black)
/// would draw dark fringes where Chrome draws the visible color fading out.
fn with_transparent_stops_tinted(stops: &[(f32, Rgba)]) -> Vec<(f32, Rgba)> {
    let visible = |color: &Rgba| color.a > f32::EPSILON;
    let mut result: Vec<(f32, Rgba)> = Vec::with_capacity(stops.len());
    for (index, &(at, color)) in stops.iter().enumerate() {
        if visible(&color) {
            result.push((at, color));
            continue;
        }
        // Toward the start, the color of the stop before (as tinted); toward the end, the next
        // visible one's.
        let after = stops[index + 1..]
            .iter()
            .find(|stop| visible(&stop.1))
            .map(|stop| stop.1);
        let before = result.last().map(|stop| stop.1);
        let tinted = |tint: Rgba| Rgba { a: color.a, ..tint };
        match (before.or(after), after.or(before)) {
            (Some(left), Some(right)) => {
                result.push((at, tinted(left)));
                if (left.r, left.g, left.b) != (right.r, right.g, right.b) {
                    result.push((at, tinted(right)));
                }
            }
            _ => result.push((at, color)),
        }
    }
    result
}

/// The color stops of `stops` with each color hint replaced as Blink replaces it: dropped
/// halfway between its stops, a hard stop at one of them, else nine stops that follow the
/// hint's curve, mixed in `interpolation`.
fn with_hints_replaced(
    stops: &[Stop],
    interpolation: fagus_style::values::Interpolation,
) -> Vec<(f32, Rgba)> {
    let mut result: Vec<(f32, Rgba)> = Vec::with_capacity(stops.len());
    for (index, stop) in stops.iter().enumerate() {
        match *stop {
            Stop::Color(at, color) => result.push((at, color)),
            Stop::Hint(at) => {
                let (Some(&(start, from)), Some(Stop::Color(end, to))) =
                    (result.last(), stops.get(index + 1))
                else {
                    continue;
                };
                let (end, to) = (*end, *to);
                let (left, right, total) = (at - start, end - at, end - start);
                let nearly = |a: f32, b: f32| (a - b).abs() <= 1e-6 * total.abs().max(1.0);
                if nearly(left, right) {
                    continue;
                }
                if nearly(left, 0.0) {
                    result.push((at, to));
                    continue;
                }
                if nearly(right, 0.0) {
                    result.push((at, from));
                    continue;
                }
                let offsets: [f32; 9] = std::array::from_fn(|y| {
                    let y = y as f32;
                    if left > right {
                        if y < 7.0 {
                            start + left * ((7.0 + y) / 13.0)
                        } else {
                            at + right * ((y - 6.0) / 3.0)
                        }
                    } else if y < 2.0 {
                        start + left * ((y + 1.0) / 3.0)
                    } else {
                        at + right * ((y - 2.0) / 13.0)
                    }
                });
                let exponent = libm::logf(0.5_f32) / libm::logf(left / total);
                for offset in offsets {
                    let weight = libm::powf((offset - start) / total, exponent);
                    if weight.is_finite() {
                        result.push((offset, mix(from, to, weight, interpolation)));
                    }
                }
            }
        }
    }
    result
}

/// The color at `at` between the shading stops.
fn color_at(stops: &[(f32, Rgba)], at: f32) -> Rgba {
    let Some(first) = stops.first() else {
        return Rgba::TRANSPARENT;
    };
    if at <= first.0 {
        return first.1;
    }
    for pair in stops.windows(2) {
        let ((start, from), (end, to)) = (pair[0], pair[1]);
        if at <= end {
            let t = if end > start {
                (at - start) / (end - start)
            } else {
                1.0
            };
            return mix(from, to, t, default_interpolation(true));
        }
    }
    stops.last().map_or(Rgba::TRANSPARENT, |last| last.1)
}

/// Repeats the stops of one period so that they cover `0..extent`.
fn repeat_stops(stops: &[(f32, Rgba)], extent: f32, max_stops: usize) -> Option<Vec<(f32, Rgba)>> {
    let first = stops.first()?.0;
    let period = stops.last()?.0 - first;
    if period <= extent / 1000.0 {
        return None;
    }
    let start = ((0.0 - first) / period).floor() as i64;
    let end = ((extent - first) / period).ceil() as i64;
    let repeats = usize::try_from(end - start).ok()?;
    if repeats * stops.len() > max_stops {
        return None;
    }
    Some(
        (start..end)
            .flat_map(|repeat| {
                let shift = repeat as f32 * period;
                stops.iter().map(move |&(at, color)| (at + shift, color))
            })
            .collect(),
    )
}

/// The color of a repeating gradient whose stops all lie at one place (to a float's precision,
/// of the gradient's `length`): its last stop's, as Blink draws it (`NormalizeAndAddStops`).
fn coincident(gradient: &Gradient, stops: &[(f32, Rgba)], length: f32) -> Option<Rgba> {
    let (first, last) = (stops.first()?, stops.last()?);
    (gradient.repeating && length > 0.0 && (last.0 - first.0) / length < f32::EPSILON).then_some(last.1)
}

/// The average color of a gradient whose repeating period is too short to show.
fn average(stops: &[(f32, Rgba)]) -> Rgba {
    let count = stops.len().max(1) as f32;
    let sum = stops.iter().fold([0.0; 4], |sum, (_, color)| {
        [
            sum[0] + color.r,
            sum[1] + color.g,
            sum[2] + color.b,
            sum[3] + color.a,
        ]
    });
    Rgba {
        r: sum[0] / count,
        g: sum[1] / count,
        b: sum[2] / count,
        a: sum[3] / count,
    }
}

/// Normalizes positions to `0..1` between `start` and `end`.
fn normalized(stops: Vec<(f32, Rgba)>, start: f32, end: f32) -> Vec<(f32, Rgba)> {
    let span = (end - start).max(f32::EPSILON);
    stops
        .into_iter()
        .map(|(at, color)| (((at - start) / span).clamp(0.0, 1.0), color))
        .collect()
}

/// Resolves a gradient for a tile of `width` × `height` with its origin at the tile's top left.
pub(crate) fn gradient_paint(
    gradient: &Gradient,
    width: f32,
    height: f32,
    current: Rgba,
) -> Option<GradientPaint> {
    match &gradient.kind {
        GradientKind::Linear(direction) => {
            let (dx, dy) = match *direction {
                LinearDirection::Angle(degrees) => {
                    let (sin, cos) = libm::sincosf(degrees.to_radians());
                    (sin, -cos)
                }
                // Towards a corner: perpendicular to the diagonal between the two other corners.
                LinearDirection::Corner { right, bottom } => {
                    let norm = libm::hypotf(width, height).max(f32::EPSILON);
                    let sx = if right { 1.0 } else { -1.0 };
                    let sy = if bottom { 1.0 } else { -1.0 };
                    (sx * height / norm, sy * width / norm)
                }
            };
            let length = (width * dx).abs() + (height * dy).abs();
            let stops = shading_stops(&resolve_stops(&gradient.stops, length, current), gradient, 16);
            let (first, last) = (stops.first()?.0, stops.last()?.0);
            if let Some(color) = coincident(gradient, &stops, length) {
                return Some(GradientPaint::Solid(color));
            }
            if gradient.repeating && last - first <= length / 1000.0 {
                return Some(GradientPaint::Solid(average(&stops)));
            }
            let last = last.max(first + 0.001);
            let mut stops = stops;
            let (mut first, mut last) = (first, last);
            if !gradient.repeating {
                // A hard stop at an end would be lost when the shading extends its end colors.
                let (start_color, end_color) = (stops[0].1, stops[stops.len() - 1].1);
                first -= 1.0;
                last += 1.0;
                stops.insert(0, (first, start_color));
                stops.push((last, end_color));
            }
            let origin = (width / 2.0 - dx * length / 2.0, height / 2.0 - dy * length / 2.0);
            Some(GradientPaint::Linear {
                start: (origin.0 + dx * first, origin.1 + dy * first),
                end: (origin.0 + dx * last, origin.1 + dy * last),
                stops: normalized(stops, first, last),
                repeat: gradient.repeating,
            })
        }
        GradientKind::Radial { shape, position } => {
            let center = (position.0.resolve(width), position.1.resolve(height));
            let (rx, ry) = radii(shape, center, width, height);
            if rx <= 0.0 || ry <= 0.0 {
                return None;
            }
            let mut stops = shading_stops(&resolve_stops(&gradient.stops, rx, current), gradient, 16);
            if let Some(color) = coincident(gradient, &stops, rx) {
                return Some(GradientPaint::Solid(color));
            }
            // The farthest point of the tile, in units of the x radius.
            let reach = [(0.0, 0.0), (width, 0.0), (0.0, height), (width, height)]
                .into_iter()
                .map(|(x, y): (f32, f32)| libm::hypotf((x - center.0) / rx, (y - center.1) / ry) * rx)
                .fold(0.0, f32::max);
            if gradient.repeating {
                match repeat_stops(&stops, reach, 2048) {
                    Some(repeated) => stops = repeated,
                    None => return Some(GradientPaint::Solid(average(&stops))),
                }
            }
            // Radial shadings start at the center: what lies before it is cut off.
            if stops.first()?.0 < 0.0 {
                let at_center = color_at(&stops, 0.0);
                stops.retain(|stop| stop.0 > 0.0);
                stops.insert(0, (0.0, at_center));
            }
            if !gradient.repeating {
                let end_color = stops[stops.len() - 1].1;
                let end = stops[stops.len() - 1].0;
                stops.push((end + 1.0, end_color));
            }
            let end = stops.last()?.0.max(0.001);
            Some(GradientPaint::Radial {
                center,
                radius: end,
                scale_y: ry / rx,
                stops: normalized(stops, 0.0, end),
            })
        }
        GradientKind::Conic { from, position } => {
            let center = (position.0.resolve(width), position.1.resolve(height));
            let segments = gradient.stops.len().max(2);
            // PDF viewers evaluate conic gradients with a function of at most ~100 stops.
            let samples = (90 / segments).clamp(1, 16);
            let mut stops = shading_stops(&resolve_stops(&gradient.stops, 360.0, current), gradient, samples);
            if let Some(color) = coincident(gradient, &stops, 360.0) {
                return Some(GradientPaint::Solid(color));
            }
            if gradient.repeating {
                // Many repeats are drawn as an image (see `fagus_pdf`), not in their average.
                match repeat_stops(&stops, 360.0, 2048) {
                    Some(repeated) => stops = repeated,
                    None => return Some(GradientPaint::Solid(average(&stops))),
                }
            }
            // Only one turn is shown.
            let start = color_at(&stops, 0.0);
            let end = color_at(&stops, 360.0);
            stops.retain(|stop| stop.0 > 0.0 && stop.0 < 360.0);
            stops.insert(0, (0.0, start));
            stops.push((360.0, end));
            Some(GradientPaint::Conic {
                center,
                start_angle: *from,
                stops: normalized(stops, 0.0, 360.0),
            })
        }
    }
}

/// The radii of a radial gradient's ending shape (CSS Images 3, §3.2.2).
fn radii(shape: &RadialShape, center: (f32, f32), width: f32, height: f32) -> (f32, f32) {
    let (cx, cy) = center;
    let horizontal = [cx.abs(), (width - cx).abs()];
    let vertical = [cy.abs(), (height - cy).abs()];
    let closest = (horizontal[0].min(horizontal[1]), vertical[0].min(vertical[1]));
    let farthest = (horizontal[0].max(horizontal[1]), vertical[0].max(vertical[1]));
    let corners = [(0.0, 0.0), (width, 0.0), (0.0, height), (width, height)]
        .map(|(x, y): (f32, f32)| libm::hypotf(x - cx, y - cy));
    let closest_corner = corners.iter().copied().fold(f32::INFINITY, f32::min);
    let farthest_corner = corners.iter().copied().fold(0.0, f32::max);
    match shape {
        RadialShape::Circle(size) => {
            let radius = match size {
                RadialSize::ClosestSide => closest.0.min(closest.1),
                RadialSize::FarthestSide => farthest.0.max(farthest.1),
                RadialSize::ClosestCorner => closest_corner,
                RadialSize::FarthestCorner => farthest_corner,
                RadialSize::Radii(radius, _) => radius.resolve(width),
            };
            (radius, radius)
        }
        RadialShape::Ellipse(size) => match size {
            RadialSize::ClosestSide => closest,
            RadialSize::FarthestSide => farthest,
            // Same aspect ratio as for the sides, through the corner.
            RadialSize::ClosestCorner => (
                closest.0 * std::f32::consts::SQRT_2,
                closest.1 * std::f32::consts::SQRT_2,
            ),
            RadialSize::FarthestCorner => (
                farthest.0 * std::f32::consts::SQRT_2,
                farthest.1 * std::f32::consts::SQRT_2,
            ),
            RadialSize::Radii(x, y) => (x.resolve(width), y.resolve(height)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fagus_style::values::{ColorSpace, LengthPercentage};

    fn stop(color: Rgba, position: Option<f32>) -> GradientStop {
        GradientStop::Color {
            color: Some(color),
            legacy: true,
            position: position.map(|fraction| StopPosition::Length(LengthPercentage::Percent(fraction))),
            after: None,
        }
    }

    #[test]
    fn sizes_local_layers_of_a_scroll_container_in_its_scrollable_overflow() {
        use fagus_layout::units::{Au, Rect};
        use fagus_layout::{Fragment, FragmentKind, VisibleSides};
        use fagus_style::values::{Overflow, Sides};
        // A box 100 × 100 with a 5px border and 10px padding, holding a block 200 × 180.
        let style = ComputedStyle::from(fagus_style::ComputedValues {
            background_color: Rgba::from_u8(238, 238, 238, 1.0),
            background_local: Arc::from([true]),
            border_width: Sides::all(5.0),
            padding: Sides::all(LengthPercentage::Px(10.0)),
            overflow_x: Overflow::Hidden,
            overflow_y: Overflow::Hidden,
            ..fagus_style::ComputedValues::default()
        });
        let px = Au::from_px;
        let child = Fragment {
            rect: Rect::new(px(15.0), px(15.0), px(200.0), px(180.0)),
            kind: FragmentKind::Anonymous,
            children: Vec::new().into(),
        };
        let fragment = Fragment {
            rect: Rect::new(px(0.0), px(0.0), px(130.0), px(130.0)),
            kind: FragmentKind::Box {
                style: Arc::new(style.clone()),
                node: None,
                sides: VisibleSides::ALL,
            },
            children: vec![child].into(),
        };
        let border_box = RectF {
            x: 0.0,
            y: 0.0,
            width: 130.0,
            height: 130.0,
        };
        // Its padding box reaches 10px past the block: 220 × 200, borders around it.
        let scrolled = scrollable_border_box(&style, &fragment, border_box);
        assert_eq!(
            scrolled,
            Some(RectF {
                width: 230.0,
                height: 210.0,
                ..border_box
            })
        );
        // Not in a box that does not clip its overflow, nor without local layers.
        let visible = ComputedStyle::from(fagus_style::ComputedValues {
            overflow_x: Overflow::Visible,
            overflow_y: Overflow::Visible,
            ..fagus_style::ComputedValues::from(style.clone())
        });
        assert_eq!(scrollable_border_box(&visible, &fragment, border_box), None);
        let scrolling = ComputedStyle::from(fagus_style::ComputedValues {
            background_local: Arc::from([]),
            ..fagus_style::ComputedValues::from(style.clone())
        });
        assert_eq!(scrollable_border_box(&scrolling, &fragment, border_box), None);
        // The color, under a local bottom layer, stays inside the padding box.
        let mut items = Vec::new();
        let placement = Placement {
            scrolled,
            ..Placement::of(border_box, None)
        };
        paint(&style, placement, &Images::default(), &mut items);
        let color = items.iter().find_map(|item| match item {
            Item::Rect { rect, .. } => Some(*rect),
            _ => None,
        });
        assert_eq!(
            color,
            Some(RectF {
                x: 5.0,
                y: 5.0,
                width: 120.0,
                height: 120.0,
            })
        );
    }

    #[test]
    fn ends_the_color_halfway_into_an_opaque_rounded_border() {
        // A checkbox of Obsidian's: 1px border and background of one color, rounded by 4px.
        let purple = Rgba::from_u8(139, 108, 239, 1.0);
        let style = ComputedStyle::from(fagus_style::ComputedValues {
            background_color: purple,
            border_width: fagus_style::values::Sides::all(1.0),
            ..fagus_style::ComputedValues::default()
        });
        let border_box = RectF {
            x: 10.0,
            y: 10.0,
            width: 15.0,
            height: 15.0,
        };
        let color_rect = |radii: Option<Radii>, opaque_border: bool| {
            let mut items = Vec::new();
            let placement = Placement {
                opaque_border,
                ..Placement::of(border_box, radii)
            };
            paint(&style, placement, &Images::default(), &mut items);
            items.into_iter().find_map(|item| match item {
                Item::Rect { rect, radii, .. } => Some((rect, radii)),
                _ => None,
            })
        };
        let half = RectF {
            x: 10.5,
            y: 10.5,
            width: 14.0,
            height: 14.0,
        };
        assert_eq!(
            color_rect(Some([(4.0, 4.0); 4]), true),
            Some((half, Some([(3.5, 3.5); 4])))
        );
        // Square corners keep it at the inner edge; without an opaque border it fills the box.
        let inner = RectF {
            x: 11.0,
            y: 11.0,
            width: 13.0,
            height: 13.0,
        };
        assert_eq!(color_rect(None, true), Some((inner, None)));
        assert_eq!(
            color_rect(Some([(4.0, 4.0); 4]), false),
            Some((border_box, Some([(4.0, 4.0); 4])))
        );
    }

    #[test]
    fn fixes_up_stop_positions() {
        let red = Rgba::from_u8(255, 0, 0, 1.0);
        let stops = resolve_stops(
            &[
                stop(red, None),
                stop(red, Some(0.5)),
                stop(red, Some(0.2)),
                stop(red, None),
                stop(red, None),
            ],
            100.0,
            Rgba::BLACK,
        );
        let positions: Vec<f32> = stops
            .iter()
            .map(|stop| match stop {
                Stop::Color(at, _) | Stop::Hint(at) => *at,
            })
            .collect();
        assert_eq!(positions, [0.0, 50.0, 50.0, 75.0, 100.0]);
    }

    #[test]
    fn gives_transparent_stops_the_color_of_their_visible_neighbors() {
        // `repeating-linear-gradient(transparent 0 6px, #eef1f6 6px 7px)`: the transparent part
        // takes the stripe's color, so no black shows at the hard stop's edge.
        let stripe = Rgba::from_u8(238, 241, 246, 1.0);
        let red = Rgba::from_u8(255, 0, 0, 1.0);
        let gradient = |stops: Vec<GradientStop>| Gradient {
            kind: GradientKind::Linear(LinearDirection::Angle(90.0)),
            repeating: true,
            stops,
            interpolation: None,
        };
        let striped = gradient(vec![
            stop(Rgba::TRANSPARENT, Some(0.0)),
            stop(Rgba::TRANSPARENT, Some(0.6)),
            stop(stripe, Some(0.6)),
            stop(stripe, Some(0.7)),
        ]);
        let stops = shading_stops(&resolve_stops(&striped.stops, 10.0, Rgba::BLACK), &striped, 16);
        let clear = Rgba { a: 0.0, ..stripe };
        assert_eq!(stops, [(0.0, clear), (6.0, clear), (6.0, stripe), (7.0, stripe)]);
        // Between two colors, a transparent stop is split: each side fades from its own color.
        let between = gradient(vec![
            stop(red, Some(0.0)),
            stop(Rgba::TRANSPARENT, Some(0.5)),
            stop(Rgba::TRANSPARENT, Some(0.5)),
            stop(stripe, Some(1.0)),
        ]);
        let stops = shading_stops(&resolve_stops(&between.stops, 10.0, Rgba::BLACK), &between, 2);
        let colors: Vec<(f32, [u8; 4])> = stops
            .iter()
            .map(|(at, color)| {
                (
                    *at,
                    [color.r, color.g, color.b, color.a].map(|c| (c * 255.0).round() as u8),
                )
            })
            .collect();
        assert_eq!(
            colors,
            [
                (0.0, [255, 0, 0, 255]),
                (2.5, [255, 0, 0, 128]),
                (5.0, [255, 0, 0, 0]),
                (5.0, [238, 241, 246, 0]),
                (5.0, [238, 241, 246, 0]),
                (7.5, [238, 241, 246, 128]),
                (10.0, [238, 241, 246, 255]),
            ]
        );
    }

    #[test]
    fn mixes_translucent_gradients_in_their_own_space() {
        // `in hsl` from red to a transparent green passes yellow (Chrome draws such a gradient
        // as an image); an opaque one is a shading in sRGB.
        let hsl = Some(fagus_style::values::Interpolation {
            space: ColorSpace::Hsl,
            hue: fagus_style::values::HueInterpolation::Shorter,
        });
        let gradient = |to: Rgba| Gradient {
            kind: GradientKind::Linear(LinearDirection::Angle(90.0)),
            repeating: false,
            stops: vec![stop(Rgba::from_u8(255, 0, 0, 1.0), None), stop(to, None)],
            interpolation: hsl,
        };
        let translucent = gradient(Rgba::from_u8(0, 255, 0, 0.0));
        let stops = shading_stops(
            &resolve_stops(&translucent.stops, 100.0, Rgba::BLACK),
            &translucent,
            16,
        );
        let middle = color_at(&stops, 50.0);
        assert!(
            middle.r > 0.9 && middle.g > 0.9 && middle.b < 0.1,
            "yellow: {middle:?}"
        );
        let opaque = gradient(Rgba::from_u8(0, 0, 255, 1.0));
        let stops = shading_stops(&resolve_stops(&opaque.stops, 100.0, Rgba::BLACK), &opaque, 16);
        assert_eq!(stops.len(), 2, "{stops:?}");
    }

    #[test]
    fn paints_repeating_gradients_of_coincident_stops_in_their_last_color() {
        let gradient = Gradient {
            kind: GradientKind::Linear(LinearDirection::Angle(180.0)),
            repeating: true,
            stops: vec![
                stop(Rgba::from_u8(255, 165, 0, 1.0), Some(0.5)),
                stop(Rgba::from_u8(0, 0, 255, 1.0), Some(0.5)),
            ],
            interpolation: None,
        };
        let paint = gradient_paint(&gradient, 100.0, 100.0, Rgba::BLACK);
        assert!(
            matches!(paint, Some(GradientPaint::Solid(color)) if color == Rgba::from_u8(0, 0, 255, 1.0)),
            "{paint:?}"
        );
    }

    #[test]
    fn interpolates_between_stops_in_srgb_and_follows_hints_in_the_gradients_space() {
        let (blue, yellow) = (Rgba::from_u8(0, 0, 255, 1.0), Rgba::from_u8(255, 255, 0, 1.0));
        let hsl = Some(fagus_style::values::Interpolation {
            space: ColorSpace::Hsl,
            hue: fagus_style::values::HueInterpolation::Shorter,
        });
        let gradient = |stops: Vec<GradientStop>| Gradient {
            kind: GradientKind::Linear(LinearDirection::Angle(90.0)),
            repeating: false,
            stops,
            interpolation: hsl,
        };
        // `in hsl` between two stops: the two stops alone, which the shading mixes in sRGB.
        let plain = gradient(vec![stop(blue, None), stop(yellow, None)]);
        let stops = shading_stops(&resolve_stops(&plain.stops, 100.0, Rgba::BLACK), &plain, 16);
        assert_eq!(stops, [(0.0, blue), (100.0, yellow)]);
        // A hint at 20%: nine stops in its place, mixed in HSL (through green), as Blink's.
        let hinted = gradient(vec![
            stop(blue, None),
            GradientStop::Hint(StopPosition::Length(LengthPercentage::Percent(0.2))),
            stop(yellow, None),
        ]);
        let stops = shading_stops(&resolve_stops(&hinted.stops, 100.0, Rgba::BLACK), &hinted, 16);
        assert_eq!(stops.len(), 11);
        assert!((stops[1].0 - 20.0 / 3.0).abs() < 1e-3 && (stops[3].0 - 20.0).abs() < 1e-3);
        let (at, middle) = stops[3];
        assert!((at - 20.0).abs() < 1e-3);
        assert!(middle.g > 0.9 && middle.r < 0.1, "{middle:?}");
        // A hint halfway is no hint.
        let halfway = gradient(vec![
            stop(blue, None),
            GradientStop::Hint(StopPosition::Length(LengthPercentage::Percent(0.5))),
            stop(yellow, None),
        ]);
        let stops = shading_stops(&resolve_stops(&halfway.stops, 100.0, Rgba::BLACK), &halfway, 16);
        assert_eq!(stops.len(), 2);
    }

    #[test]
    fn spaces_tiles_over_the_painting_area_at_their_pace() {
        // Two 30-pixel tiles fit the 65 pixels of a padding box 35 pixels into the border box:
        // 35 apart, and on so into the border.
        let xs = positions(0.0, 30.0, BackgroundRepeat::Space, 0.0, 135.0, 35.0, 65.0);
        assert_eq!(xs, [0.0, 35.0, 70.0, 105.0]);
        // Clipped to the positioning area, just the ones that fit.
        let xs = positions(0.0, 30.0, BackgroundRepeat::Space, 35.0, 65.0, 35.0, 65.0);
        assert_eq!(xs, [35.0, 70.0]);
    }

    #[test]
    fn rounds_tiles_keeping_their_ratio_and_blends_tiny_ones() {
        let style = ComputedStyle::default();
        let red = Rgba::from_u8(255, 0, 0, 1.0);
        let image = Image::Gradient(Arc::new(Gradient {
            kind: GradientKind::Linear(LinearDirection::Angle(180.0)),
            repeating: false,
            stops: vec![stop(red, None), stop(red, None)],
            interpolation: None,
        }));
        // A diagonal one, whose tiles are drawn one by one.
        let diagonal = Image::Gradient(Arc::new(Gradient {
            kind: GradientKind::Linear(LinearDirection::Angle(45.0)),
            repeating: false,
            stops: vec![stop(red, None), stop(Rgba::from_u8(0, 0, 255, 1.0), None)],
            interpolation: None,
        }));
        let rect = RectF {
            x: 0.0,
            y: 0.0,
            width: 180.0,
            height: 180.0,
        };
        let tiles = |image: &Image, size: BackgroundSize, repeat| {
            let geometry = LayerGeometry {
                whole: None,
                exact: None,
                origin: BoxArea::BorderBox,
                fixed: None,
                clip: Some(BoxArea::BorderBox),
                size,
                repeat,
                position: (None, None),
            };
            let mut items = Vec::new();
            layer(
                (&style, rect, None),
                image,
                &geometry,
                &Images::default(),
                &mut items,
            );
            items
        };
        // 52 pixels wide, rounded to 60 (three across), and as tall as auto makes it, scaled
        // with it: 180 × 60 / 52.
        let rounded = tiles(
            &diagonal,
            BackgroundSize::Explicit(
                LengthPercentageAuto::Length(LengthPercentage::Px(52.0)),
                LengthPercentageAuto::Auto,
            ),
            (BackgroundRepeat::Round, BackgroundRepeat::Repeat),
        );
        let Some(Item::Gradient { rect: tile, .. }) = rounded.get(1) else {
            panic!("{rounded:?}");
        };
        assert!((tile.width - 60.0).abs() < 1e-3, "{tile:?}");
        assert!((tile.height - 180.0 * 60.0 / 52.0).abs() < 1e-2, "{tile:?}");
        // Tiles of a fifth of a pixel: one rectangle in their color.
        let tiny = tiles(
            &image,
            BackgroundSize::Explicit(
                LengthPercentageAuto::Length(LengthPercentage::Px(0.2)),
                LengthPercentageAuto::Length(LengthPercentage::Px(0.2)),
            ),
            (BackgroundRepeat::Repeat, BackgroundRepeat::Repeat),
        );
        assert!(
            matches!(&tiny[..], [Item::Rect { color, .. }] if *color == red),
            "{tiny:?}"
        );
        // Tiles of one color are drawn as one; those of a gradient down them, in one row
        // across the box each.
        let rect_of = |item: &Item| match item {
            Item::Gradient { rect, .. } => Some(*rect),
            _ => None,
        };
        let size = || {
            BackgroundSize::Explicit(
                LengthPercentageAuto::Length(LengthPercentage::Px(60.0)),
                LengthPercentageAuto::Length(LengthPercentage::Px(60.0)),
            )
        };
        let repeat = (BackgroundRepeat::Repeat, BackgroundRepeat::Repeat);
        let solid: Vec<RectF> = tiles(&image, size(), repeat).iter().filter_map(rect_of).collect();
        assert_eq!(solid, [rect], "{solid:?}");
        let down = Image::Gradient(Arc::new(Gradient {
            kind: GradientKind::Linear(LinearDirection::Angle(180.0)),
            repeating: false,
            stops: vec![stop(red, None), stop(Rgba::from_u8(0, 0, 255, 1.0), None)],
            interpolation: None,
        }));
        let rows: Vec<RectF> = tiles(&down, size(), repeat).iter().filter_map(rect_of).collect();
        assert_eq!(rows.len(), 3, "{rows:?}");
        assert!(
            rows.iter()
                .all(|row| (row.width - 180.0).abs() < 1e-3 && (row.height - 60.0).abs() < 1e-3),
            "{rows:?}"
        );
        let diagonal_tiles = tiles(&diagonal, size(), repeat)
            .iter()
            .filter_map(rect_of)
            .count();
        assert_eq!(diagonal_tiles, 9);
    }

    #[test]
    fn points_corner_gradients_at_the_corner() {
        let gradient = Gradient {
            kind: GradientKind::Linear(LinearDirection::Corner {
                right: true,
                bottom: true,
            }),
            repeating: false,
            stops: vec![stop(Rgba::BLACK, None), stop(Rgba::WHITE, None)],
            interpolation: None,
        };
        let Some(GradientPaint::Linear { start, end, .. }) =
            gradient_paint(&gradient, 200.0, 100.0, Rgba::BLACK)
        else {
            panic!("linear gradient");
        };
        // The line goes through the center, perpendicular to the other diagonal.
        let (mx, my) = (f32::midpoint(start.0, end.0), f32::midpoint(start.1, end.1));
        assert!((mx - 100.0).abs() < 0.01 && (my - 50.0).abs() < 0.01);
        let direction = (end.0 - start.0, end.1 - start.1);
        assert!(
            (direction.0 * 200.0 - direction.1 * 100.0).abs() < 0.5,
            "perpendicular to (200, -100)"
        );
    }

    #[test]
    fn repeats_conic_gradients_of_short_periods() {
        // A seal's rays: two colors every 12 degrees, 30 periods of 4 stops around the circle.
        let (gold, light) = (
            Rgba::from_u8(0xb0, 0x8d, 0x57, 1.0),
            Rgba::from_u8(0xc9, 0xa8, 0x6a, 1.0),
        );
        let angle = |degrees: f32| Some(StopPosition::Angle(degrees));
        let at = |color: Rgba, position: Option<StopPosition>| GradientStop::Color {
            color: Some(color),
            legacy: true,
            position,
            after: None,
        };
        let gradient = Gradient {
            kind: GradientKind::Conic {
                from: 0.0,
                position: (LengthPercentage::Percent(0.5), LengthPercentage::Percent(0.5)),
            },
            repeating: true,
            stops: vec![
                at(gold, angle(0.0)),
                at(gold, angle(6.0)),
                at(light, angle(6.0)),
                at(light, angle(12.0)),
            ],
            interpolation: None,
        };
        let Some(GradientPaint::Conic { stops, .. }) = gradient_paint(&gradient, 100.0, 100.0, Rgba::BLACK)
        else {
            panic!("a conic gradient, not its average color");
        };
        assert!(stops.len() >= 120, "{}", stops.len());
        // Gold up to 6 degrees, light up to 12, gold again after.
        assert_eq!(color_at(&stops, 3.0 / 360.0), gold);
        assert_eq!(color_at(&stops, 9.0 / 360.0), light);
        assert_eq!(color_at(&stops, 15.0 / 360.0), gold);
    }

    #[test]
    fn drops_svg_tiles_smaller_than_half_a_pixel() {
        use fagus_style::values::LengthPercentageAuto as Lpa;
        let image = |format| fagus_layout::Image {
            data: Arc::new(Vec::new()),
            format,
            width: 50.0,
            height: 50.0,
            natural: (true, true),
            ratio: Some(1.0),
            inline: false,
            orientation: 1,
            filters: Arc::from([]),
            pad: 0,
        };
        let mut images = Images::default();
        images.insert("a.svg", Arc::new(image(fagus_layout::ImageFormat::Svg)));
        images.insert("a.png", Arc::new(image(fagus_layout::ImageFormat::Png)));
        let style = ComputedStyle::default();
        let border_box = RectF {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };
        let painted = |url: &str, width: f32, height: f32| {
            let geometry = LayerGeometry {
                whole: None,
                exact: None,
                origin: BoxArea::PaddingBox,
                fixed: None,
                clip: Some(BoxArea::BorderBox),
                size: BackgroundSize::Explicit(
                    Lpa::Length(LengthPercentage::Px(width)),
                    Lpa::Length(LengthPercentage::Px(height)),
                ),
                repeat: (BackgroundRepeat::Repeat, BackgroundRepeat::Repeat),
                position: (None, None),
            };
            layer(
                (&style, border_box, None),
                &Image::Url(url.into(), 1.0),
                &geometry,
                &images,
                &mut Vec::new(),
            )
        };
        assert!(!painted("a.svg", 0.2, 10.0));
        assert!(!painted("a.svg", 0.49, 0.49));
        assert!(painted("a.svg", 0.51, 0.51));
        assert!(painted("a.png", 0.2, 0.2));
    }

    #[test]
    fn positions_layers_in_the_box_as_laid_out_and_snaps_single_images() {
        use fagus_style::values::LengthPercentageAuto as Lpa;
        let mut images = Images::default();
        images.insert(
            "a.png",
            Arc::new(fagus_layout::Image {
                data: Arc::new(Vec::new()),
                format: fagus_layout::ImageFormat::Png,
                width: 10.0,
                height: 10.0,
                natural: (true, true),
                ratio: Some(1.0),
                inline: false,
                orientation: 1,
                filters: Arc::from([]),
                pad: 0,
            }),
        );
        let style = ComputedStyle::default();
        // A box laid out 300.3 pixels wide at x = 10.4, painted into its snapped border box
        // (10 to 311); the image is 13.3 pixels large, centred.
        let exact = RectF {
            x: 10.4,
            y: 0.0,
            width: 300.3,
            height: 40.0,
        };
        let snapped = RectF {
            x: 10.0,
            y: 0.0,
            width: 301.0,
            height: 40.0,
        };
        let tiles = |repeat| {
            let center = Some(LengthPercentage::Percent(0.5));
            let geometry = LayerGeometry {
                whole: None,
                exact: Some(exact),
                origin: BoxArea::PaddingBox,
                fixed: None,
                clip: Some(BoxArea::BorderBox),
                size: BackgroundSize::Explicit(
                    Lpa::Length(LengthPercentage::Px(13.3)),
                    Lpa::Length(LengthPercentage::Px(13.3)),
                ),
                repeat: (repeat, repeat),
                position: (center.clone(), center),
            };
            let mut items = Vec::new();
            assert!(layer(
                (&style, snapped, None),
                &Image::Url("a.png".into(), 1.0),
                &geometry,
                &images,
                &mut items,
            ));
            items
                .iter()
                .filter_map(|item| match item {
                    Item::Image { rect, .. } => Some(*rect),
                    Item::Tiles { origin, step, .. } => Some(RectF {
                        x: origin.0,
                        y: origin.1,
                        width: step.0,
                        height: step.1,
                    }),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        // Repeated, the tiles are as large as 13.3 pixels in layout units, and one starts in the
        // middle of the 300.3 pixels from the snapped box's corner: at 10 + 143.5.
        let repeated = tiles(BackgroundRepeat::Repeat);
        let step = repeated[1].x - repeated[0].x;
        assert!((step - 13.296_875).abs() < 1e-3, "{repeated:?}");
        let phase = (153.5 - repeated[0].x) / step;
        assert!((phase - phase.round()).abs() < 1e-3, "{repeated:?}");
        // Alone, the image is moved to whole pixels from where the box as laid out puts it
        // (10.4 + 143.5 rounds to 154) and drawn 13 pixels large (Chrome 157, measured).
        let single = tiles(BackgroundRepeat::NoRepeat);
        assert_eq!(single.len(), 1);
        assert_eq!(
            (single[0].x, single[0].width, single[0].height),
            (154.0, 13.0, 13.0)
        );
    }

    #[test]
    fn sizes_images_without_natural_sizes_by_the_area() {
        use fagus_style::values::LengthPercentageAuto as Lpa;
        // An SVG image without `width` and `height` (its fallback size 300 × 150), with or
        // without a ratio, in an area of 256 × 768.
        let image = |natural: (bool, bool), ratio: Option<f32>| fagus_layout::Image {
            data: Arc::new(Vec::new()),
            format: fagus_layout::ImageFormat::Svg,
            width: 300.0,
            height: 150.0,
            natural,
            ratio,
            inline: false,
            orientation: 1,
            filters: Arc::from([]),
            pad: 0,
        };
        let area = RectF {
            x: 0.0,
            y: 0.0,
            width: 256.0,
            height: 768.0,
        };
        let auto = BackgroundSize::Explicit(Lpa::Auto, Lpa::Auto);
        // Neither size nor ratio: the area.
        assert_eq!(
            image_tile_size(&auto, area, &image((false, false), None)),
            (256.0, 768.0)
        );
        // A ratio alone: as `contain`.
        assert_eq!(
            image_tile_size(&auto, area, &image((false, false), Some(2.0))),
            (256.0, 128.0)
        );
        assert_eq!(
            image_tile_size(&BackgroundSize::Cover, area, &image((false, false), Some(2.0))),
            (1536.0, 768.0)
        );
        // A natural width without a ratio: the area's height.
        assert_eq!(
            image_tile_size(&auto, area, &image((true, false), None)),
            (300.0, 768.0)
        );
        // A width given and no ratio: the area's height.
        let width = BackgroundSize::Explicit(Lpa::Length(LengthPercentage::Px(40.0)), Lpa::Auto);
        assert_eq!(
            image_tile_size(&width, area, &image((false, false), None)),
            (40.0, 768.0)
        );
        // A raster image keeps its size.
        assert_eq!(
            image_tile_size(&auto, area, &image((true, true), Some(2.0))),
            (300.0, 150.0)
        );
    }
}

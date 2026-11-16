//! `border-image` (CSS Backgrounds 3, §6): an image cut into nine parts, the corners drawn into
//! the corners of the border area, the edges scaled and tiled along its sides, and the middle,
//! with `fill`, over the rest.

use std::sync::Arc;

use fagus_layout::{Images, VisibleSides};
use fagus_style::ComputedStyle;
use fagus_style::values::{BorderImageOutset, BorderImageRepeat, BorderImageWidth, Image, ImageSlice};

use crate::background::gradient_paint;
use crate::{GradientPaint, Item, RectF};

/// More tiles than this along an edge are not drawn.
const MAX_TILES: usize = 1024;

/// What a border image shows: an image, or a gradient drawn as large as the area.
enum Source {
    Image(Arc<fagus_layout::Image>),
    Gradient(Arc<GradientPaint>),
}

/// Paints the border image of a box with `border_box`, of which `sides` are visible; `false`
/// where it has none that can be drawn, and its border is drawn instead. An image that is there
/// replaces the border even where none of its parts has room to show (Chrome 157, measured:
/// `100%` slices on a box with a top border alone show nothing).
pub(crate) fn paint(
    style: &ComputedStyle,
    border_box: RectF,
    sides: VisibleSides,
    images: &Images,
    items: &mut Vec<Item>,
) -> bool {
    let Some(border) = style.border_image.as_deref() else {
        return false;
    };
    let border_width = &style.border_width;
    let widths = [
        border_width.top,
        border_width.right,
        border_width.bottom,
        border_width.left,
    ];
    // A side where the box is broken across pages or lines has no border image.
    let visible = [sides.top, sides.right, sides.bottom, sides.left];
    let outset = std::array::from_fn::<f32, 4, _>(|side| match border.outset[side] {
        _ if !visible[side] => 0.0,
        BorderImageOutset::Number(number) => number * widths[side],
        BorderImageOutset::Length(length) => length,
    });
    let area = RectF {
        x: border_box.x - outset[3],
        y: border_box.y - outset[0],
        width: border_box.width + outset[1] + outset[3],
        height: border_box.height + outset[0] + outset[2],
    };
    if area.width <= 0.0 || area.height <= 0.0 {
        return false;
    }
    let source = match &border.source {
        Image::Url(url, resolution) => match images.get(url) {
            Some(image) => Source::Image(fagus_layout::Image::styled(
                &fagus_layout::Image::at_resolution(image, *resolution),
                style,
            )),
            None => return false,
        },
        Image::Gradient(gradient) => match gradient_paint(gradient, area.width, area.height, style.color) {
            Some(paint) => Source::Gradient(Arc::new(paint)),
            None => return false,
        },
        Image::None => return false,
    };
    // The image's size; one without a natural size is as large as the area.
    let (image_width, image_height) = match &source {
        Source::Image(image) if image.natural == (true, true) => (image.width, image.height),
        _ => (area.width, area.height),
    };
    if image_width <= 0.0 || image_height <= 0.0 {
        return false;
    }
    // Where the image is cut, in from its top, right, bottom and left.
    let cut = std::array::from_fn::<f32, 4, _>(|side| {
        let along = if side % 2 == 0 { image_height } else { image_width };
        match border.slice[side] {
            ImageSlice::Number(number) => number.min(along),
            ImageSlice::Percent(fraction) => (fraction * along).min(along),
        }
    });
    // How wide the parts are drawn, shrunk together where they would overlap (§6.2).
    let mut drawn = std::array::from_fn::<f32, 4, _>(|side| {
        if !visible[side] {
            return 0.0;
        }
        match &border.width[side] {
            BorderImageWidth::Number(number) => number * widths[side],
            BorderImageWidth::Length(length) => {
                length.resolve(if side % 2 == 0 { area.height } else { area.width })
            }
            BorderImageWidth::Auto => cut[side],
        }
    });
    let factor = [
        area.width / (drawn[1] + drawn[3]),
        area.height / (drawn[0] + drawn[2]),
    ]
    .into_iter()
    .filter(|factor| factor.is_finite())
    .fold(1.0_f32, f32::min);
    for width in &mut drawn {
        *width *= factor;
    }

    // The edges of the nine parts in the image and in the area.
    let source_x = [0.0, cut[3], image_width - cut[1], image_width];
    let source_y = [0.0, cut[0], image_height - cut[2], image_height];
    let area_x = [
        area.x,
        area.x + drawn[3],
        area.x + area.width - drawn[1],
        area.x + area.width,
    ];
    let area_y = [
        area.y,
        area.y + drawn[0],
        area.y + area.height - drawn[2],
        area.y + area.height,
    ];
    // How much the edges scale the image across them: the middle scales as they do.
    let scale = |drawn: f32, cut: f32| (cut > 0.0 && drawn > 0.0).then(|| drawn / cut);
    let horizontal = scale(drawn[0], cut[0]).or_else(|| scale(drawn[2], cut[2]));
    let vertical = scale(drawn[3], cut[3]).or_else(|| scale(drawn[1], cut[1]));
    let (across, down) = border.repeat;
    for row in 0..3 {
        for column in 0..3 {
            if row == 1 && column == 1 && !border.fill {
                continue;
            }
            // Parts of hidden sides and their corners are not drawn.
            if (row == 0 && !visible[0])
                || (row == 2 && !visible[2])
                || (column == 0 && !visible[3])
                || (column == 2 && !visible[1])
            {
                continue;
            }
            let part = RectF {
                x: source_x[column],
                y: source_y[row],
                width: source_x[column + 1] - source_x[column],
                height: source_y[row + 1] - source_y[row],
            };
            let space = RectF {
                x: area_x[column],
                y: area_y[row],
                width: area_x[column + 1] - area_x[column],
                height: area_y[row + 1] - area_y[row],
            };
            if part.width <= 0.0 || part.height <= 0.0 || space.width <= 0.0 || space.height <= 0.0 {
                continue;
            }
            // The size of one tile: corners fill their space; edges scale to the border's width
            // across them and keep their ratio along them, unless stretched.
            let tile_width = match (column, row) {
                (1, 0 | 2) if across != BorderImageRepeat::Stretch => part.width * space.height / part.height,
                (1, 1) if across != BorderImageRepeat::Stretch => part.width * horizontal.unwrap_or(1.0),
                _ => space.width,
            };
            let tile_height = match (column, row) {
                (0 | 2, 1) if down != BorderImageRepeat::Stretch => part.height * space.width / part.width,
                (1, 1) if down != BorderImageRepeat::Stretch => part.height * vertical.unwrap_or(1.0),
                _ => space.height,
            };
            let corner = row != 1 && column != 1;
            let xs = tiles(
                space.x,
                space.width,
                tile_width,
                if corner {
                    BorderImageRepeat::Stretch
                } else {
                    across
                },
            );
            let ys = tiles(
                space.y,
                space.height,
                tile_height,
                if corner { BorderImageRepeat::Stretch } else { down },
            );
            if xs.is_empty() || ys.is_empty() || xs.len() * ys.len() > MAX_TILES {
                continue;
            }
            for &(x, width) in &xs {
                for &(y, height) in &ys {
                    // The tile, cut at the ends of its space.
                    let (left, top) = (x.max(space.x), y.max(space.y));
                    let right = (x + width).min(space.x + space.width);
                    let bottom = (y + height).min(space.y + space.height);
                    if right <= left || bottom <= top {
                        continue;
                    }
                    let tile = RectF {
                        x: left,
                        y: top,
                        width: right - left,
                        height: bottom - top,
                    };
                    // A raster image's part is an image of its own, scaled into the tile.
                    if let Source::Image(image) = &source
                        && image.format != fagus_layout::ImageFormat::Svg
                    {
                        let mut part_image = (**image).clone();
                        part_image.filters = Arc::from([fagus_layout::PixelFilter::Part([
                            part.x / image_width,
                            part.y / image_height,
                            (part.x + part.width) / image_width,
                            (part.y + part.height) / image_height,
                        ])]);
                        let part_image = Arc::new(part_image);
                        let whole = RectF { x, y, width, height };
                        if whole == tile {
                            items.push(Item::Image {
                                rect: tile,
                                image: part_image,
                            });
                        } else {
                            items.push(Item::PushClip(tile, None));
                            items.push(Item::Image {
                                rect: whole,
                                image: part_image,
                            });
                            items.push(Item::Pop);
                        }
                        continue;
                    }
                    items.push(Item::PushClip(tile, None));
                    // The part, scaled into the tile: the whole image, moved and scaled so.
                    let (scale_x, scale_y) = (width / part.width, height / part.height);
                    items.push(Item::PushTransform([
                        scale_x,
                        0.0,
                        0.0,
                        scale_y,
                        x - part.x * scale_x,
                        y - part.y * scale_y,
                    ]));
                    let rect = RectF {
                        x: 0.0,
                        y: 0.0,
                        width: image_width,
                        height: image_height,
                    };
                    items.push(match &source {
                        Source::Image(image) => Item::Image {
                            rect,
                            image: Arc::clone(image),
                        },
                        Source::Gradient(gradient) => Item::Gradient {
                            rect,
                            gradient: Arc::clone(gradient),
                        },
                    });
                    items.push(Item::Pop);
                    items.push(Item::Pop);
                }
            }
        }
    }
    true
}

/// The tiles of `size` along a space from `start` of `length`, as their starts and sizes, as
/// `border-image-repeat` lays them out.
fn tiles(start: f32, length: f32, size: f32, repeat: BorderImageRepeat) -> Vec<(f32, f32)> {
    if size <= 0.0 || length <= 0.0 {
        return Vec::new();
    }
    match repeat {
        BorderImageRepeat::Stretch => vec![(start, length)],
        // Whole tiles, as many as fit best, scaled to fill the space.
        BorderImageRepeat::Round => {
            let count = (length / size).round().max(1.0);
            let size = length / count;
            (0..count as usize)
                .map(|index| (start + index as f32 * size, size))
                .collect()
        }
        // Tiles centered in the space, cut at its ends.
        BorderImageRepeat::Repeat => {
            let first = start + (length - size) / 2.0;
            let before = ((first - start) / size).ceil().max(0.0) as usize;
            let count = before * 2 + 1;
            if count > MAX_TILES {
                return Vec::new();
            }
            (0..count)
                .map(|index| (first + (index as f32 - before as f32) * size, size))
                .collect()
        }
        // Whole tiles, with the room left between them and at the ends.
        BorderImageRepeat::Space => {
            let count = (length / size).floor();
            if count < 1.0 {
                return Vec::new();
            }
            let gap = (length - count * size) / (count + 1.0);
            (0..count as usize)
                .map(|index| (start + gap + index as f32 * (size + gap), size))
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lays_out_tiles_as_border_image_repeat_says() {
        // Stretched: one tile over the space.
        assert_eq!(
            tiles(10.0, 100.0, 30.0, BorderImageRepeat::Stretch),
            [(10.0, 100.0)]
        );
        // Rounded: whole tiles, scaled to fill it.
        assert_eq!(
            tiles(0.0, 100.0, 30.0, BorderImageRepeat::Round),
            [
                (0.0, 100.0 / 3.0),
                (100.0 / 3.0, 100.0 / 3.0),
                (200.0 / 3.0, 100.0 / 3.0)
            ]
        );
        // Repeated: centered, cut at the ends.
        assert_eq!(
            tiles(0.0, 100.0, 40.0, BorderImageRepeat::Repeat),
            [(-10.0, 40.0), (30.0, 40.0), (70.0, 40.0)]
        );
        // Spaced: whole tiles with the room left between and around them.
        let spaced: Vec<(f32, f32)> = tiles(0.0, 100.0, 40.0, BorderImageRepeat::Space)
            .into_iter()
            .map(|(start, size)| ((start * 1000.0).round() / 1000.0, size))
            .collect();
        assert_eq!(spaced, [(6.667, 40.0), (53.333, 40.0)]);
        assert_eq!(
            tiles(0.0, 30.0, 40.0, BorderImageRepeat::Space),
            [] as [(f32, f32); 0]
        );
    }

    #[test]
    fn replaces_the_border_where_no_part_has_room() {
        // `100%` slices leave the edges nothing, and a box with a top border alone has no
        // corners: nothing is drawn, and nothing of the border either (Chrome 157).
        let mut style = ComputedStyle::default();
        style.border_width.top = 10.0;
        style.border_image = Some(Arc::new(fagus_style::values::BorderImage {
            source: Image::Gradient(Arc::new(fagus_style::values::Gradient {
                kind: fagus_style::values::GradientKind::Linear(fagus_style::values::LinearDirection::Angle(
                    180.0,
                )),
                repeating: false,
                stops: vec![
                    fagus_style::values::GradientStop::Color {
                        color: Some(fagus_style::values::Rgba::BLACK),
                        legacy: true,
                        position: None,
                        after: None,
                    };
                    2
                ],
                interpolation: None,
            })),
            ..fagus_style::values::BorderImage::default()
        }));
        let rect = RectF {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
        };
        let mut items = Vec::new();
        assert!(paint(
            &style,
            rect,
            VisibleSides::ALL,
            &Images::default(),
            &mut items
        ));
        assert!(items.is_empty(), "{items:?}");
    }
}

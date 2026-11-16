//! Box shadows (CSS Backgrounds 3, §6): shapes and clips; how a blur is drawn is up to the
//! output format.

use std::sync::Arc;

use fagus_layout::VisibleSides;
use fagus_style::ComputedStyle;

use crate::{BoxShadowPaint, Item, Radii, RectF};

/// Paints the outer (`inset == false`) or inset shadows of a box with border box `border_box`.
/// The first shadow is on top, so they are painted last to first.
pub(crate) fn paint(
    style: &ComputedStyle,
    border_box: RectF,
    radii: Option<Radii>,
    inset: bool,
    items: &mut Vec<Item>,
) {
    paint_slice(style, border_box, radii, VisibleSides::ALL, inset, items);
}

/// Paints the shadows of a fragment of a box broken across columns or pages whose sides
/// `sides` are drawn (`box-decoration-break: slice`): each shadow goes on past the sides where
/// the box is cut, as if the box went on there, and is cut off at them (Blink's `ClipToSides`
/// and `AdjustRectForSideClipping`, in any writing mode; Chrome 157).
pub(crate) fn paint_slice(
    style: &ComputedStyle,
    border_box: RectF,
    radii: Option<Radii>,
    sides: VisibleSides,
    inset: bool,
    items: &mut Vec<Item>,
) {
    // The corners of a native frame are rounded by Chrome's theme, not by `border-radius`: its
    // shadows keep square corners (Chrome 157, measured on a button).
    let radii = if style.native_frame.is_some() { None } else { radii };
    for shadow in style
        .box_shadow
        .iter()
        .rev()
        .filter(|shadow| shadow.inset == inset)
    {
        let color = shadow.color.unwrap_or(style.color);
        if color.is_transparent() {
            continue;
        }
        // How far the box seems to go on past each side where it is cut.
        let past = [
            (!sides.top).then(|| shadow.y.max(0.0) + shadow.blur),
            (!sides.right).then(|| shadow.blur - shadow.x.min(0.0)),
            (!sides.bottom).then(|| shadow.blur - shadow.y.min(0.0)),
            (!sides.left).then(|| shadow.x.max(0.0) + shadow.blur),
        ];
        let paint = if inset {
            let border = &style.border_width;
            let [top, right, bottom, left] = [
                (sides.top, border.top),
                (sides.right, border.right),
                (sides.bottom, border.bottom),
                (sides.left, border.left),
            ]
            .map(|(shown, width)| if shown { width } else { 0.0 });
            let area = RectF {
                x: border_box.x + left,
                y: border_box.y + top,
                width: (border_box.width - left - right).max(0.0),
                height: (border_box.height - top - bottom).max(0.0),
            };
            let area_radii = radii.map(|radii| {
                let insets = [(left, top), (right, top), (right, bottom), (left, bottom)];
                let mut inner = radii;
                for (radius, (x, y)) in inner.iter_mut().zip(insets) {
                    *radius = ((radius.0 - x).max(0.0), (radius.1 - y).max(0.0));
                }
                inner
            });
            BoxShadowPaint {
                area,
                area_radii,
                shape: spread(moved(extended(area, past), shadow.x, shadow.y), -shadow.spread),
                shape_radii: area_radii.map(|radii| spread_radii(radii, -shadow.spread, area)),
                hole: None,
                color,
                blur: shadow.blur,
                inset: true,
                outlines: None,
            }
        } else {
            BoxShadowPaint {
                area: border_box,
                area_radii: radii,
                shape: spread(
                    moved(extended(border_box, past), shadow.x, shadow.y),
                    shadow.spread,
                ),
                shape_radii: radii.map(|radii| spread_radii(radii, shadow.spread, border_box)),
                hole: None,
                color,
                blur: shadow.blur,
                inset: false,
                outlines: None,
            }
        };
        if paint.shape.width <= 0.0 && paint.shape.height <= 0.0 && !inset {
            continue;
        }
        // An outer shadow is cut off where the box is.
        let cut = !inset && past.iter().any(Option::is_some);
        if cut {
            const FAR: f32 = 1.0e5;
            let [top, right, bottom, left] = past.map(|past| if past.is_some() { 0.0 } else { FAR });
            items.push(Item::PushClip(
                RectF {
                    x: border_box.x - left,
                    y: border_box.y - top,
                    width: border_box.width + left + right,
                    height: border_box.height + top + bottom,
                },
                None,
            ));
        }
        items.push(Item::BoxShadow(Arc::new(paint)));
        if cut {
            items.push(Item::Pop);
        }
    }
}

/// `rect` reaching `past` (top, right, bottom, left) further where that is given.
fn extended(rect: RectF, past: [Option<f32>; 4]) -> RectF {
    let [top, right, bottom, left] = past.map(|past| past.unwrap_or(0.0));
    RectF {
        x: rect.x - left,
        y: rect.y - top,
        width: rect.width + left + right,
        height: rect.height + top + bottom,
    }
}

fn moved(rect: RectF, x: f32, y: f32) -> RectF {
    RectF {
        x: rect.x + x,
        y: rect.y + y,
        ..rect
    }
}

fn spread(rect: RectF, distance: f32) -> RectF {
    RectF {
        x: rect.x - distance,
        y: rect.y - distance,
        width: (rect.width + 2.0 * distance).max(0.0),
        height: (rect.height + 2.0 * distance).max(0.0),
    }
}

/// Corner radii of a shadow of a box `size` large, spread by `distance` (CSS Backgrounds 3,
/// §6.1.1, "outset-adjusted border radius"): they grow with the spread, but a corner sharper
/// than the spread, and small for the box, stays close to sharp. A shrinking spread takes the
/// distance off them. The algorithm of the specification, as Chrome follows it (Blink's
/// `ComputeOutsetAdjustedBorderRadius`).
fn spread_radii(radii: Radii, distance: f32, size: RectF) -> Radii {
    if distance < 0.0 {
        return radii.map(|(x, y)| ((x + distance).max(0.0), (y + distance).max(0.0)));
    }
    if distance == 0.0 || size.width <= 0.0 || size.height <= 0.0 {
        return radii;
    }
    radii.map(|(x, y)| {
        if x <= 0.0 && y <= 0.0 {
            return (x, y);
        }
        let coverage = 2.0 * (x / size.width).min(y / size.height);
        let adjusted = |radius: f32| {
            if radius > distance || coverage > 1.0 {
                return radius + distance;
            }
            let ratio = radius / distance;
            radius + distance * (1.0 - (1.0 - ratio).powi(3) * (1.0 - coverage.powi(3)))
        };
        (adjusted(x), adjusted(y))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cuts_shadows_off_at_the_sides_where_a_box_breaks() {
        use fagus_style::values::BoxShadow;
        let style = ComputedStyle {
            box_shadow: Arc::from([BoxShadow {
                color: Some(fagus_style::values::Rgba::BLACK),
                x: 4.0,
                y: 0.0,
                blur: 6.0,
                spread: 10.0,
                inset: false,
            }]),
            ..ComputedStyle::default()
        };
        let rect = RectF {
            x: 100.0,
            y: 20.0,
            width: 50.0,
            height: 100.0,
        };
        // The first slice of a `vertical-rl` box (or an inline box broken across lines in a
        // right-to-left line) is cut on its left: its shadow goes on past that side by the
        // offset and the blur, and is cut off there (Blink's `ClipToSides`, Chrome 157, WPT
        // css-break box-shadow-003).
        let sides = VisibleSides {
            left: false,
            ..VisibleSides::ALL
        };
        let mut items = Vec::new();
        paint_slice(&style, rect, None, sides, false, &mut items);
        let [Item::PushClip(clip, None), Item::BoxShadow(shadow), Item::Pop] = &items[..] else {
            panic!("a cut shadow: {items:?}");
        };
        assert_eq!(clip.x, 100.0);
        assert!(clip.x + clip.width > 1000.0 && clip.y < -1000.0);
        assert_eq!(shadow.shape.x, 100.0 - 10.0 + 4.0 - 10.0);
        assert_eq!(shadow.shape.width, 50.0 + 10.0 + 20.0);
        // Whole, it is not cut.
        let mut items = Vec::new();
        paint_slice(&style, rect, None, VisibleSides::ALL, false, &mut items);
        assert!(matches!(&items[..], [Item::BoxShadow(_)]));
    }

    #[test]
    fn spreads_corners_by_how_much_of_the_box_they_cover() {
        let rect = |width, height| RectF {
            x: 0.0,
            y: 0.0,
            width,
            height,
        };
        // A circle of 2 pixels spread by 149 stays a circle: its corners cover the box.
        let [(x, y), ..] = spread_radii([(1.0, 1.0); 4], 149.0, rect(2.0, 2.0));
        assert!((x - 150.0).abs() < 1e-3 && (y - 150.0).abs() < 1e-3, "{x} {y}");
        // A small corner of a large box stays close to sharp.
        let [(x, _), ..] = spread_radii([(1.0, 1.0); 4], 50.0, rect(200.0, 200.0));
        assert!(x > 1.0 && x < 4.0, "{x}");
        // Larger than the spread: the spread adds to it; shrinking takes it off.
        assert_eq!(
            spread_radii([(20.0, 20.0); 4], 10.0, rect(200.0, 200.0))[0],
            (30.0, 30.0)
        );
        assert_eq!(
            spread_radii([(20.0, 5.0); 4], -10.0, rect(200.0, 200.0))[0],
            (10.0, 0.0)
        );
        // Square corners stay square.
        assert_eq!(
            spread_radii([(0.0, 0.0); 4], 10.0, rect(200.0, 200.0))[0],
            (0.0, 0.0)
        );
    }
}

//! Scroll bars of scroll containers, drawn as Chrome on Linux prints them (Blink's
//! `ScrollbarThemeFluent` and the `NativeThemeFluent` of Chromium's `ui/native_theme`, without
//! its arrow icon font): a track with an arrow button at either end and, where the content
//! overflows, a thumb with round ends at the start of the track, the content being printed
//! scrolled to its start. Layout gave the box their room (see `fagus_layout::scrollbars`).

use fagus_layout::Fragment;
use fagus_style::ComputedStyle;
use fagus_style::values::Rgba;

use crate::{ClipGeometry, Item, RectF};

/// The length of an arrow button along its scroll bar (`kScrollbarButtonSideLength`).
const BUTTON: f32 = 18.0;
/// The side of the square an arrow is drawn in, in a button of [`BUTTON`] (`GetArrowRect`).
const ARROW: f32 = 9.0;
/// The thickness and least length of a thumb (`GetVerticalScrollbarThumbSize`).
const THUMB: (f32, f32) = (9.0, 17.0);
/// The thickness of a scroll bar of `scrollbar-width: auto`.
const THICKNESS: f32 = 15.0;
/// The color of the track, the buttons and the corner (`kColorWebNativeControlScrollbarTrack`).
const TRACK: Rgba = gray(0xFC);
/// The color of the arrows and the thumb.
const THUMB_COLOR: Rgba = gray(0x8B);

const fn gray(level: u8) -> Rgba {
    let value = level as f32 / 255.0;
    Rgba {
        r: value,
        g: value,
        b: value,
        a: 1.0,
    }
}

/// The parts of the padding box `padding` (snapped) of a scroll container of `style` that its
/// scroll bars cover: the vertical bar, the horizontal one, and the corner between them (or
/// below a vertical bar beside a resize handle).
pub(crate) struct Areas {
    pub(crate) vertical: Option<RectF>,
    pub(crate) horizontal: Option<RectF>,
    pub(crate) corner: Option<RectF>,
}

pub(crate) fn areas(style: &ComputedStyle, padding: RectF, bottom_visible: bool) -> Areas {
    let bars = style.scrollbars;
    let thickness = bars.thickness;
    if !bars.any() {
        return Areas {
            vertical: None,
            horizontal: None,
            corner: None,
        };
    }
    let horizontal = bars.horizontal && bottom_visible;
    let left = style.is_rtl();
    // A text area's resize handle takes the corner below a vertical bar too.
    let cornered = horizontal || bars.vertical && style.resize && bottom_visible;
    let bar_x = if left {
        padding.x
    } else {
        padding.x + padding.width - thickness
    };
    let vertical = bars.vertical.then(|| RectF {
        x: bar_x,
        y: padding.y,
        width: thickness,
        height: (padding.height - if cornered { thickness } else { 0.0 }).max(0.0),
    });
    let horizontal_bar = horizontal.then(|| RectF {
        x: if bars.vertical && left {
            padding.x + thickness
        } else {
            padding.x
        },
        y: padding.y + padding.height - thickness,
        width: (padding.width - if bars.vertical { thickness } else { 0.0 }).max(0.0),
        height: thickness,
    });
    let corner = (bars.vertical && cornered).then_some(RectF {
        x: bar_x,
        y: padding.y + padding.height - thickness,
        width: thickness,
        height: thickness,
    });
    Areas {
        vertical,
        horizontal: horizontal_bar,
        corner,
    }
}

/// Paints the scroll bars of the fragment of a scroll container of `style` whose padding box
/// (snapped) is `padding`, at `(x, y)` its border box.
pub(crate) fn paint(
    style: &ComputedStyle,
    fragment: &Fragment,
    padding: RectF,
    bottom_visible: bool,
    items: &mut Vec<Item>,
) {
    let areas = areas(style, padding, bottom_visible);
    let proportion = style.scrollbars.thickness / THICKNESS;
    let (thumb_color, track_color) = match style.scrollbar_color {
        Some((thumb, track)) => (thumb.unwrap_or(style.color), track.unwrap_or(style.color)),
        None => (THUMB_COLOR, TRACK),
    };
    // How much of the content shows, and how large it is, as Chrome counts them for the thumb,
    // in whole pixels.
    let size = fagus_layout::scrollbars::scroll_size(style, fragment);
    let colors = (thumb_color, track_color);
    if let Some(bar) = areas.vertical {
        let sizes = (size.client.1.round(), size.total.1.round());
        paint_bar(bar, true, sizes, false, proportion, colors, items);
    }
    if let Some(bar) = areas.horizontal {
        // Right to left, the content is scrolled to its right end, and so is the thumb.
        let sizes = (size.client.0.round(), size.total.0.round());
        paint_bar(bar, false, sizes, style.is_rtl(), proportion, colors, items);
    }
    if let Some(corner) = areas.corner {
        items.push(Item::Rect {
            rect: corner,
            color: track_color,
            radii: None,
        });
        // The resize handle of a box that may be resized lies in the corner, over the
        // padding box's corner, which the bars cover.
        if style.resize {
            let left = style.is_rtl();
            items.push(Item::Image {
                rect: RectF {
                    x: if left {
                        corner.x
                    } else {
                        corner.x + corner.width - 9.0
                    },
                    y: corner.y + corner.height - 9.0,
                    width: 9.0,
                    height: 9.0,
                },
                image: fagus_layout::scrollbars::resize_handle(left),
            });
        }
    }
}

/// Paints one scroll bar filling `bar`: its track and buttons, its arrows and, where `visible`
/// is less than `total`, its thumb (`ScrollbarTheme::ThumbLength`, `ScrollbarThemeFluent`), at
/// the start of the track or at its end (`at_end`).
fn paint_bar(
    bar: RectF,
    vertical: bool,
    (visible, total): (f32, f32),
    at_end: bool,
    proportion: f32,
    (thumb_color, track_color): (Rgba, Rgba),
    items: &mut Vec<Item>,
) {
    if bar.width <= 0.0 || bar.height <= 0.0 {
        return;
    }
    // The buttons and the track are filled alike.
    items.push(Item::Rect {
        rect: bar,
        color: track_color,
        radii: None,
    });
    let (length, thickness) = if vertical {
        (bar.height, bar.width)
    } else {
        (bar.width, bar.height)
    };
    // Buttons take half of a bar too short for both and a track (`ButtonSize`).
    let desired = (BUTTON * proportion).round();
    let button = if length < 2.0 * desired {
        (length / 2.0).floor()
    } else {
        desired
    };
    let along = |start: f32, size: f32| {
        if vertical {
            RectF {
                x: bar.x,
                y: bar.y + start,
                width: thickness,
                height: size,
            }
        } else {
            RectF {
                x: bar.x + start,
                y: bar.y,
                width: size,
                height: thickness,
            }
        }
    };
    if button > 0.0 {
        paint_arrow(along(0.0, button), vertical, false, thumb_color, items);
        paint_arrow(along(length - button, button), vertical, true, thumb_color, items);
    }
    let track = if length > 2.0 * button {
        length - 2.0 * button
    } else {
        0.0
    };
    if total <= visible || total <= 0.0 || track <= 0.0 {
        return;
    }
    let least = (THUMB.1 * proportion).trunc();
    let thumb_length = (visible / total * track).round().max(least).min(track);
    // The thumb's thickness differs from the bar's by an even number of pixels, so that it
    // lies in its middle (`ThumbThickness`).
    let thumb_thickness = {
        let own = (THUMB.0 * proportion).round();
        own - ((thickness - own) % 2.0)
    };
    let inset = (thickness - thumb_thickness) / 2.0;
    let start = if at_end {
        length - button - thumb_length
    } else {
        button
    };
    let rect = if vertical {
        RectF {
            x: bar.x + inset,
            y: bar.y + start,
            width: thumb_thickness,
            height: thumb_length,
        }
    } else {
        RectF {
            x: bar.x + start,
            y: bar.y + inset,
            width: thumb_length,
            height: thumb_thickness,
        }
    };
    let radius = thumb_thickness.min(thumb_length) / 2.0;
    items.push(Item::Rect {
        rect,
        color: thumb_color,
        radii: Some([(radius, radius); 4]),
    });
}

/// Paints the triangle of an arrow button `button` of a vertical or horizontal bar, pointing to
/// its end (`forward`) or its start, as `NativeThemeFluent::GetArrowRect` places it and
/// `NativeThemeBase::PathForArrow` draws it.
fn paint_arrow(button: RectF, vertical: bool, forward: bool, color: Rgba, items: &mut Vec<Item>) {
    let points = arrow(button, vertical, forward);
    items.push(Item::PushClipShape(ClipGeometry::Polygon {
        points: points.to_vec(),
        even_odd: false,
    }));
    let (left, right) = points.iter().fold((f32::MAX, f32::MIN), |(left, right), point| {
        (left.min(point.0), right.max(point.0))
    });
    let (top, bottom) = points.iter().fold((f32::MAX, f32::MIN), |(top, bottom), point| {
        (top.min(point.1), bottom.max(point.1))
    });
    items.push(Item::Rect {
        rect: RectF {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        },
        color,
        radii: None,
    });
    items.push(Item::Pop);
}

/// The corners of the triangle of an arrow (see [`paint_arrow`]).
#[allow(clippy::cast_possible_truncation, reason = "sides of a few pixels")]
fn arrow(button: RectF, vertical: bool, forward: bool) -> [(f32, f32); 3] {
    let (small, large) = if button.width < button.height {
        (button.width, button.height)
    } else {
        (button.height, button.width)
    };
    let scale = large / BUTTON;
    let mut side = (ARROW * scale).ceil() as i32;
    // An odd difference to the smaller side keeps the arrow off whole pixels: Chrome adds a
    // pixel (or takes one away, as the remainder keeps the sign).
    side += (small as i32 - side) % 2;
    let side = side as f32;
    let (width, height) = (button.width.min(side), button.height.min(side));
    let offset = if forward { scale.round() } else { -scale.round() };
    let mut rect = RectF {
        x: (button.x + (button.width - width) / 2.0).floor(),
        y: (button.y + (button.height - height) / 2.0).floor(),
        width,
        height,
    };
    if vertical {
        rect.y += offset;
    } else {
        rect.x += offset;
    }
    let half = |value: f32| ((value.round() as i32) / 2 + 1) as f32;
    let center = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
    if vertical {
        // Pointing up, mirrored for down.
        let height = half(rect.height);
        let base = rect.y + rect.height - ((height as i32) / 2) as f32 + 1.0;
        let points = [
            (rect.x, base),
            (rect.x + rect.width, base),
            (rect.x + rect.width / 2.0, base - height),
        ];
        if forward {
            points.map(|(x, y)| (x, 2.0 * center.1 - y))
        } else {
            points
        }
    } else {
        // Pointing right, mirrored for left.
        let width = half(rect.width);
        let base = rect.x + ((width as i32) / 2) as f32;
        let points = [
            (base, rect.y),
            (base, rect.y + rect.height),
            (base + width, rect.y + rect.height / 2.0),
        ];
        if forward {
            points
        } else {
            points.map(|(x, y)| (2.0 * center.0 - x, y))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, width: f32, height: f32) -> RectF {
        RectF { x, y, width, height }
    }

    #[test]
    fn places_arrows_as_chrome_on_linux() {
        // Measured in Chrome 157's PDF: a vertical bar at x 287, 15px wide, buttons 18px long;
        // a horizontal one at y 63; and `scrollbar-width: thin`, 10px with 12px buttons.
        assert_eq!(
            arrow(rect(287.0, 2.0, 15.0, 18.0), true, false),
            [(290.0, 13.0), (299.0, 13.0), (294.5, 8.0)]
        );
        assert_eq!(
            arrow(rect(287.0, 45.0, 15.0, 18.0), true, true),
            [(290.0, 51.0), (299.0, 51.0), (294.5, 56.0)]
        );
        assert_eq!(
            arrow(rect(2.0, 63.0, 18.0, 15.0), false, false),
            [(12.0, 66.0), (12.0, 75.0), (7.0, 70.5)]
        );
        assert_eq!(
            arrow(rect(269.0, 63.0, 18.0, 15.0), false, true),
            [(276.0, 66.0), (276.0, 75.0), (281.0, 70.5)]
        );
        assert_eq!(
            arrow(rect(2.0, 22.0, 12.0, 10.0), false, false),
            [(8.0, 24.0), (8.0, 30.0), (4.0, 27.0)]
        );
        assert_eq!(
            arrow(rect(290.0, 22.0, 12.0, 10.0), false, true),
            [(296.0, 24.0), (296.0, 30.0), (300.0, 27.0)]
        );
    }

    #[test]
    fn sizes_the_thumb_as_chrome_on_linux() {
        // A bar 300px long with 600px of content in 300px: the thumb is half the track of 264px
        // long, 9px thick, 3px in (Chrome 157, measured).
        let mut items = Vec::new();
        paint_bar(
            rect(2.0, 22.0, 300.0, 15.0),
            false,
            (300.0, 600.0),
            false,
            1.0,
            (THUMB_COLOR, TRACK),
            &mut items,
        );
        assert!(items.contains(&Item::Rect {
            rect: rect(20.0, 25.0, 132.0, 9.0),
            color: THUMB_COLOR,
            radii: Some([(4.5, 4.5); 4]),
        }));
        // A short track gets the least thumb, 17px.
        let mut items = Vec::new();
        paint_bar(
            rect(287.0, 2.0, 15.0, 56.0),
            true,
            (56.0, 200.0),
            false,
            1.0,
            (THUMB_COLOR, TRACK),
            &mut items,
        );
        assert!(items.contains(&Item::Rect {
            rect: rect(290.0, 20.0, 9.0, 17.0),
            color: THUMB_COLOR,
            radii: Some([(4.5, 4.5); 4]),
        }));
    }
}

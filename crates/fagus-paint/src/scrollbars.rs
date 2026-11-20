//! Scroll bars of scroll containers, drawn as Chrome on Linux prints them (Blink's
//! `ScrollbarThemeFluent` and the `NativeThemeFluent` of Chromium's `ui/native_theme`, without
//! its arrow icon font): a track with an arrow button at either end and, where the content
//! overflows, a thumb with round ends at the start of the track, the content being printed
//! scrolled to its start. Layout gave the box their room (see `fagus_layout::scrollbars`), and
//! the room of the gutters of `scrollbar-gutter: stable`, which stay empty where the box has no
//! bar. A scroll container that may be resized (`resize`) has a resize handle in its corner.

use fagus_layout::Fragment;
use fagus_style::ComputedStyle;
use fagus_style::values::{Overflow, Rgba};

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

/// The resize handle's color: black and white at 60 % (`SkColorSetARGB(153, …)`), drawn as
/// hairlines, which Chrome's PDF output draws for its strokes of 1px (0.75pt) at three quarters
/// of their alpha (see `fagus_layout`'s native frames of text fields).
const HANDLE_ALPHA: f32 = 153.0 / 255.0 * 0.75;

/// The parts of the padding box `padding` (snapped) of a scroll container of `style` that its
/// scroll bars cover: the vertical bar, the horizontal one, and the corner between them, or at
/// the end of a bar beside a resize handle (Blink's `ScrollCornerRect`).
pub(crate) struct Areas {
    pub(crate) vertical: Option<RectF>,
    pub(crate) horizontal: Option<RectF>,
    pub(crate) corner: Option<RectF>,
}

/// Whether a box of `style` may be resized, which Chrome shows with a resize handle: a scroll
/// container with `resize` other than `none` (Blink's `CanResize`).
pub(crate) fn resizes(style: &ComputedStyle) -> bool {
    style.resize && (style.overflow_x == Overflow::Hidden || style.overflow_y == Overflow::Hidden)
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
    // A corner where both bars meet, and at the end of either beside a resize handle, which
    // the handle lies in (on the last page of a box broken across pages).
    let resizer = resizes(style) && bottom_visible;
    let cornered = (bars.vertical && horizontal) || (resizer && (bars.vertical || horizontal));
    let corner_x = if left {
        padding.x
    } else {
        padding.x + padding.width - thickness
    };
    let vertical = bars.vertical.then(|| RectF {
        x: corner_x,
        y: padding.y,
        width: thickness,
        height: (padding.height - if cornered { thickness } else { 0.0 }).max(0.0),
    });
    let horizontal_bar = horizontal.then(|| RectF {
        x: if cornered && left {
            padding.x + thickness
        } else {
            padding.x
        },
        y: padding.y + padding.height - thickness,
        width: (padding.width - if cornered { thickness } else { 0.0 }).max(0.0),
        height: thickness,
    });
    let corner = cornered.then_some(RectF {
        x: corner_x,
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

/// Paints the resize handle of a box of `style` that may be resized and has no scroll bars,
/// over its content as Chrome paints it (an overlay control): in the corner of its padding box
/// `padding` (snapped) as large as a scroll bar of `scrollbar-width: auto` is thick, whatever the
/// box's `scrollbar-width` (Blink's `CornerRect`).
pub(crate) fn paint_lone_resizer(style: &ComputedStyle, padding: RectF, items: &mut Vec<Item>) {
    let corner = RectF {
        x: if style.is_rtl() {
            padding.x
        } else {
            padding.x + padding.width - THICKNESS
        },
        y: padding.y + padding.height - THICKNESS,
        width: THICKNESS,
        height: THICKNESS,
    };
    paint_resizer(corner, style.is_rtl(), items);
}

/// Paints a resize handle in `corner`, on its left in right-to-left boxes: two diagonal lines 7
/// and 3 pixels long, 2 pixels from the corner's edges, black, with white ones a pixel below
/// them (`ScrollableAreaPainter::DrawPlatformResizerImage` with `TextAreaResizerFixedSize`).
#[allow(clippy::cast_possible_truncation, reason = "a corner of a few pixels")]
fn paint_resizer(corner: RectF, on_left: bool, items: &mut Vec<Item>) {
    let (width, height) = (corner.width as i32, corner.height as i32);
    if width <= 0 || height <= 0 {
        return;
    }
    // Spacing and sizes, capped to fit the corner.
    let spacing_x = 2.min(width / 4).max(1);
    let spacing_y = 2.min(height / 4).max(1);
    let size_x = 7.min(width - spacing_x).max(1);
    let size_y = 7.min(height - spacing_y).max(1);
    let spacing = spacing_x.min(spacing_y) as f32;
    let large = size_x.min(size_y);
    let small = (large / 2).max(1) as f32;
    let large = large as f32;
    let (right, bottom) = (corner.x + corner.width, corner.y + corner.height);
    let x = |inset: f32| {
        if on_left { corner.x + inset } else { right - inset }
    };
    let points = [
        (x(spacing), bottom - (spacing + large)),
        (x(spacing + large), bottom - spacing),
        (x(spacing), bottom - (spacing + small)),
        (x(spacing + small), bottom - spacing),
    ];
    let offset = if on_left { -1.0 } else { 1.0 };
    let dark = Rgba {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: HANDLE_ALPHA,
    };
    let light = Rgba {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: HANDLE_ALPHA,
    };
    let line = |from: (f32, f32), to: (f32, f32), color: Rgba| Item::DashedLine {
        from,
        to,
        thickness: 0.0,
        dash: 0.0,
        dotted: false,
        color,
    };
    items.push(line(points[0], points[1], dark));
    items.push(line(points[2], points[3], dark));
    // A pixel below, for contrast on a dark ground.
    items.push(line(
        (points[0].0, points[0].1 + 1.0),
        (points[1].0 + offset, points[1].1),
        light,
    ));
    items.push(line(
        (points[2].0, points[2].1 + 1.0),
        (points[3].0 + offset, points[3].1),
        light,
    ));
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
        if resizes(style) && bottom_visible {
            paint_resizer(corner, style.is_rtl(), items);
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

    /// The items of the first page of `html`, painted.
    fn painted(html: &str) -> Vec<Item> {
        let document = fagus_dom::Document::parse_html(html);
        let user_agent = fagus_css::Stylesheet::parse(
            fagus_style::USER_AGENT_CSS,
            fagus_css::Origin::UserAgent,
            "ua.css",
        );
        let rules = fagus_css::RuleSet::new(&[&user_agent], &fagus_css::MediaEnvironment::default());
        let styled = fagus_style::compute_styles(&document, &rules, (794.0, 1123.0));
        let mut fonts = fagus_text::FontDb::new();
        fonts.add_default_fonts();
        let images = fagus_layout::Images::default();
        let pages = fagus_layout::layout(&document, &styled, &rules, &fonts, &images);
        crate::paint(&pages, &fonts, &images, false).remove(0).items
    }

    /// A line of a resize handle: where it starts and ends, and its alpha.
    type Line = ((f32, f32), (f32, f32), f32);

    /// The lines of resize handles among `items`.
    fn handle_lines(items: &[Item]) -> Vec<Line> {
        items
            .iter()
            .filter_map(|item| match item {
                Item::DashedLine {
                    from,
                    to,
                    thickness,
                    dash,
                    color,
                    ..
                } if *thickness == 0.0 && *dash == 0.0 => Some((*from, *to, color.a)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn draws_resize_handles_as_chrome_on_linux() {
        // A corner of 15 pixels: lines of 7 and 3 pixels 2 in from its right and bottom edges,
        // black, and white ones a pixel below, hairlines at 45 % (Chromium 157's
        // `DrawPlatformResizerImage`, 60 % strokes of 1px that Skia's PDF output draws so).
        let mut items = Vec::new();
        paint_resizer(rect(100.0, 50.0, 15.0, 15.0), false, &mut items);
        let alpha = 0.45;
        let lines = handle_lines(&items);
        assert_eq!(lines.len(), 4);
        for (line, expected) in lines.iter().zip([
            ((113.0, 56.0), (106.0, 63.0)),
            ((113.0, 60.0), (110.0, 63.0)),
            ((113.0, 57.0), (107.0, 63.0)),
            ((113.0, 61.0), (111.0, 63.0)),
        ]) {
            assert_eq!((line.0, line.1), expected);
            assert!((line.2 - alpha).abs() < 1e-6);
        }
        // Mirrored at the left.
        let mut items = Vec::new();
        paint_resizer(rect(0.0, 50.0, 15.0, 15.0), true, &mut items);
        let lines = handle_lines(&items);
        assert_eq!((lines[0].0, lines[0].1), ((2.0, 56.0), (9.0, 63.0)));
        assert_eq!((lines[2].0, lines[2].1), ((2.0, 57.0), (8.0, 63.0)));
    }

    #[test]
    fn gives_resizable_boxes_a_corner_beside_one_scroll_bar() {
        // A horizontal bar alone ends before the corner of a box that may be resized, which
        // lies at its end (on the left right to left), as Blink's `RectForHorizontalScrollbar`.
        let style = |rtl: bool| {
            ComputedStyle::from(fagus_style::ComputedValues {
                resize: true,
                overflow_x: Overflow::Hidden,
                overflow_y: Overflow::Hidden,
                direction: if rtl {
                    fagus_style::values::Direction::Rtl
                } else {
                    fagus_style::values::Direction::Ltr
                },
                scrollbars: fagus_style::values::Scrollbars {
                    vertical: false,
                    horizontal: true,
                    thickness: 15.0,
                    gutters: (false, false),
                },
                ..fagus_style::ComputedValues::default()
            })
        };
        let padding = rect(10.0, 10.0, 200.0, 100.0);
        let areas = areas(&style(false), padding, true);
        assert_eq!(areas.horizontal, Some(rect(10.0, 95.0, 185.0, 15.0)));
        assert_eq!(areas.corner, Some(rect(195.0, 95.0, 15.0, 15.0)));
        let areas = super::areas(&style(true), padding, true);
        assert_eq!(areas.horizontal, Some(rect(25.0, 95.0, 185.0, 15.0)));
        assert_eq!(areas.corner, Some(rect(10.0, 95.0, 15.0, 15.0)));
    }

    #[test]
    fn draws_the_resize_handles_of_scroll_containers() {
        // Of a scroll container that may be resized (a text area, with its native look or
        // without, and any other box), after its content; not of a box whose overflow is
        // visible, nor with `resize: none`.
        let count = |html: &str| handle_lines(&painted(html)).len();
        assert_eq!(
            count("<div style='overflow: hidden; resize: both; height: 50px'>x</div>"),
            4
        );
        assert_eq!(
            count("<div style='overflow: auto; resize: vertical; height: 50px'>x</div>"),
            4
        );
        assert_eq!(count("<div style='resize: both; height: 50px'>x</div>"), 0);
        assert_eq!(
            count("<div style='overflow: clip; resize: both; height: 50px'>x</div>"),
            0
        );
        assert_eq!(count("<textarea>a</textarea>"), 4);
        assert_eq!(
            count("<textarea style='appearance: none; border: 1px solid'>a</textarea>"),
            4
        );
        assert_eq!(count("<textarea style='resize: none'>a</textarea>"), 0);
        // Over the content: after the text of the box.
        let items = painted(
            "<div style='overflow: hidden; resize: both; height: 20px; width: 40px; font-size: 40px'>xxxxx</div>",
        );
        let text = items
            .iter()
            .rposition(|item| matches!(item, Item::Text { .. }))
            .expect("text");
        let handle = items
            .iter()
            .position(|item| matches!(item, Item::DashedLine { .. }))
            .expect("a handle");
        assert!(handle > text);
    }

    #[test]
    fn draws_resize_handles_over_the_layers_their_boxes_hold() {
        // Chrome paints a box's resize handle after the boxes it holds that are painted as
        // layers after it (a child with `will-change: transform`, a positioned one), clipped as
        // the box clips them, and before the layers after the last of them.
        let items = |child: &str| {
            painted(&format!(
                "<div style='overflow: auto; resize: both; width: 50px; height: 50px'>
                   <div style='{child}; width: 100%; height: 100%; background: yellow'></div></div>
                 <div style='position: relative; height: 10px; background: blue'></div>"
            ))
        };
        let colored = |items: &[Item], blue: f32| {
            items
                .iter()
                .rposition(
                    |item| matches!(item, Item::Rect { color, .. } if (color.b - blue).abs() < 0.01 && color.a > 0.0),
                )
                .expect("a background")
        };
        // (After the blue box too where what it holds comes after it.)
        for (child, before_blue) in [
            ("will-change: transform", true),
            ("position: relative", true),
            ("position: relative; z-index: 1", false),
        ] {
            let items = items(child);
            let handle = items
                .iter()
                .position(|item| matches!(item, Item::DashedLine { .. }))
                .expect("a handle");
            assert!(handle > colored(&items, 0.0), "{child}");
            assert_eq!(handle < colored(&items, 1.0), before_blue, "{child}");
            assert_eq!(handle_lines(&items).len(), 4, "{child}");
        }
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

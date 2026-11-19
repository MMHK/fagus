//! Form controls (`<input>`, `<textarea>`, `<select>`, `<button>`, `<progress>`, `<meter>`) as
//! Chrome prints them.
//!
//! A text field is a flex box holding one line of text, its value or else its placeholder, cut
//! off at its content box; a button holds its label; a `<select>` shows its selected option and
//! an arrow. Checkboxes, radio buttons, sliders, progress bars and the like are replaced boxes
//! whose marks are SVG images drawn as Chrome's native theme draws them; a meter (and a progress
//! bar without its native look) is built of boxes styled by its pseudo-elements. A control with
//! `appearance: auto` and no author background or border has Chrome's native frame: a 1px
//! `#767676` border with corners rounded by 2px, in place of the border of the user agent style.
//!
//! The marks follow `ui/native_theme/native_theme_base.cc` of Chromium:
//! Copyright 2012 The Chromium Authors. Use of this source code is governed by a BSD-style
//! license that can be found in the CHROMIUM-LICENSE file.

use std::fmt::Write as _;
use std::sync::Arc;

use fagus_dom::{ElementData, NodeId};
use fagus_style::values::{
    Appearance, BorderStyle, BoxSizing, Content, Display, DisplayInside, DisplayOutside, ItemAlignment,
    LengthPercentage, LengthPercentageAuto, LineHeight, Overflow, Position, Rgba, Size, WhiteSpace,
};
use fagus_style::{ComputedStyle, ComputedValues, PseudoKey};
use fagus_text::FontDb;

use super::{BoxId, BoxKind, BoxTree, Builder, InlineItem, LayoutBox, anonymous_style};
use crate::image::{Image, ImageFormat};
use crate::inline::{face_for, metrics, used_font_size};

mod dates;
pub(super) mod locale;

/// The gray of native borders.
const BORDER: Rgba = gray(0x76);
/// The accent of checked controls without `accent-color`.
const ACCENT: Rgba = Rgba {
    r: 0.0,
    g: 0x75 as f32 / 255.0,
    b: 1.0,
    a: 1.0,
};
/// The accent of checked controls without `accent-color` in the dark color scheme.
const DARK_ACCENT: Rgba = Rgba {
    r: 0x99 as f32 / 255.0,
    g: 0xc8 as f32 / 255.0,
    b: 1.0,
    a: 1.0,
};

/// The colors Chrome's theme paints checkboxes, radio buttons, sliders and progress bars in, in
/// the light or the dark color scheme (Chrome 157, measured).
struct Palette {
    /// The ground of a checkbox or radio button, and the color of its check.
    ground: Rgba,
    /// The layer under the ground of a disabled one, and the ground then.
    disabled_layer: Rgba,
    disabled_ground: Rgba,
    /// The border of an unchecked one, enabled and disabled.
    border: Rgba,
    disabled_border: Rgba,
    /// The accent of a disabled checked one, and of a disabled slider.
    disabled_accent: Rgba,
    disabled_slider: Rgba,
    /// The track of a slider or progress bar, enabled and disabled, and its border.
    track: Rgba,
    disabled_track: Rgba,
    track_border: Rgba,
    disabled_track_border: Rgba,
}

fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            ground: gray(0x3b),
            disabled_layer: gray(0x3b),
            disabled_ground: gray(0x3b),
            border: gray(0x85),
            disabled_border: gray(0x62),
            disabled_accent: gray(0x75),
            disabled_slider: gray(0x75),
            track: gray(0x3b),
            disabled_track: gray(0x36),
            track_border: gray(0x85),
            disabled_track_border: gray(0x62),
        }
    } else {
        Palette {
            ground: gray(0xff),
            disabled_layer: Rgba { a: 0.2, ..gray(0xa9) },
            disabled_ground: Rgba { a: 0.6, ..gray(0xff) },
            border: frame_border(false),
            disabled_border: frame_border(true),
            disabled_accent: frame_border(true),
            disabled_slider: gray(0xcb),
            track: gray(0xef),
            disabled_track: Rgba {
                a: DISABLED_ALPHA,
                ..gray(0xef)
            },
            track_border: SLIDER_BORDER,
            disabled_track_border: SLIDER_BORDER,
        }
    }
}

/// The border of progress bars and slider tracks.
const SLIDER_BORDER: Rgba = Rgba {
    a: 128.0 / 255.0,
    ..gray(0x76)
};
/// The color of placeholder text.
const PLACEHOLDER: Rgba = gray(0x75);
/// The size of a checkbox or radio button.
const CHECK_SIZE: f32 = 13.0;
/// The width Chrome keeps at the end of a `<select>` for its arrow
/// (`ClampedMenuListArrowPaddingSize`, the thickness of a scroll bar on Linux).
const ARROW_WIDTH: f32 = 15.0;
/// The width of the spin button of a number field (Chrome 157 on Linux, measured).
const SPIN_BUTTON: f32 = 15.0;
/// The width of a scroll bar, which Chrome 157 adds to that of a `<textarea>` (measured), at
/// any zoom: scroll bars are not zoomed.
const SCROLLBAR: f32 = 16.0;

const fn gray(level: u8) -> Rgba {
    let value = level as f32 / 255.0;
    Rgba {
        r: value,
        g: value,
        b: value,
        a: 1.0,
    }
}

/// What a form control element is, for the boxes it gets.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Control {
    /// A one-line text field (`text`, `search`, `email`, `password`, `number`, dates, …).
    Field,
    /// A checkbox or a radio button.
    Check {
        radio: bool,
    },
    /// `input type=submit`, `reset` or `button`: a button with a label.
    Button,
    /// `input type=image`: an image like `<img>`.
    Image,
    /// `input type=range`.
    Range,
    /// `input type=color`.
    Color,
    /// `input type=file`: a button and the text that no file is chosen.
    File,
    TextArea,
    Select,
    Progress,
    Meter,
}

impl Control {
    /// The control an element is, if it is one; `<button>` is a block container of its own.
    pub(super) fn of(data: &ElementData) -> Option<Self> {
        if data.is("textarea") {
            return Some(Self::TextArea);
        }
        if data.is("select") {
            return Some(Self::Select);
        }
        if data.is("progress") {
            return Some(Self::Progress);
        }
        if data.is("meter") {
            return Some(Self::Meter);
        }
        if !data.is("input") {
            return None;
        }
        Some(match input_type(data).as_str() {
            "checkbox" => Self::Check { radio: false },
            "radio" => Self::Check { radio: true },
            "submit" | "reset" | "button" => Self::Button,
            "image" => Self::Image,
            "range" => Self::Range,
            "color" => Self::Color,
            "file" => Self::File,
            _ => Self::Field,
        })
    }
}

/// The `type` of an `<input>`, in lowercase; `text` if it has none.
fn input_type(data: &ElementData) -> String {
    data.attribute("type")
        .map_or_else(|| "text".to_owned(), |kind| kind.trim().to_ascii_lowercase())
}

/// Whether a control has its native look: `appearance` is not `none`, and author style leaves
/// its background and border alone (Chrome's `LayoutTheme::IsControlStyled`).
fn native(style: &ComputedStyle) -> bool {
    style.appearance == Appearance::Auto && !style.author_frame
}

/// Which frame Chrome's theme draws around a control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Frame {
    /// A text field, a text area or a `<select>` showing one option, on a white ground
    /// (`PaintTextField`).
    Field,
    /// A `<select>` showing a list of options: on a white ground, stroked like a button.
    ListBox,
    /// A button, on a light gray ground (`PaintButton`).
    Button,
}

/// The width of the stroke of a native frame at an effective zoom (`AdjustBorderWidthByZoom`
/// of 1px, measured in Chrome 157): whole pixels, at least one.
fn frame_width(zoom: f32) -> f32 {
    (zoom + 1e-4).floor().max(1.0)
}

/// The alpha of the native border and accent of disabled controls (`0x4d`).
const DISABLED_ALPHA: f32 = 77.0 / 255.0;

/// The ground of a native frame, as one color: where the control is disabled, the gray layer
/// Chrome lightens it with (`#a9a9a9` at 20%) below its ground (white at 60% in a field,
/// `#efefef` at 30% on a button), composed into one.
fn frame_ground(frame: Frame, disabled: bool) -> Rgba {
    let (ground, alpha) = match (frame, disabled) {
        (Frame::Field | Frame::ListBox, false) => return gray(0xff),
        (Frame::Button, false) => return gray(0xef),
        (Frame::Field | Frame::ListBox, true) => (255.0, 0.6),
        (Frame::Button, true) => (239.0, DISABLED_ALPHA),
    };
    let lighten = 0.2;
    let a = lighten + alpha - lighten * alpha;
    let value = (169.0 * lighten * (1.0 - alpha) + ground * alpha) / a / 255.0;
    Rgba {
        r: value,
        g: value,
        b: value,
        a,
    }
}

/// The color of a native border.
fn frame_border(disabled: bool) -> Rgba {
    if disabled {
        Rgba {
            a: DISABLED_ALPHA,
            ..BORDER
        }
    } else {
        BORDER
    }
}

/// Gives a style Chrome's native frame: a stroke 1px wide (whole pixels of the zoom) with
/// corners rounded by 2px (zoomed) along the middle of its border, on the ground of the frame,
/// with the rest of a wider border added to the padding so that the box keeps its size. Like
/// everything Chrome's theme paints, it is drawn on whole pixels.
///
/// Chrome prints the 1px stroke of a field as a hairline at three quarters of its alpha (Skia
/// draws a stroke thinner than a device unit, 0.75pt here, so), that of a button as a stroke.
fn native_frame(style: &mut ComputedValues, frame: Frame, disabled: bool) {
    let zoom = style.zoom;
    let stroke = frame_width(zoom);
    let hairline = frame == Frame::Field && stroke * 0.75 < 1.0;
    // The dark color scheme's frames (Chrome 157, measured): a button's border has its ground's
    // color, a field's is lighter than its ground, less so where it is disabled.
    let dark = match (style.dark, disabled, frame) {
        (true, false, Frame::Button) => Some((gray(0x6b), gray(0x6b))),
        (true, false, Frame::Field | Frame::ListBox) => Some((gray(0x3b), gray(0x85))),
        // A disabled field keeps its dark ground, its border darker.
        (true, true, Frame::Field | Frame::ListBox) => Some((gray(0x3b), gray(0x62))),
        _ => None,
    };
    let border = dark.map_or_else(|| frame_border(disabled), |(_, border)| border);
    let color = if hairline {
        let color = border;
        Rgba {
            a: color.a * 0.75,
            ..color
        }
    } else {
        border
    };
    let sides = [
        (
            &mut style.border_width.top,
            &mut style.border_style.top,
            &mut style.border_color.top,
            &mut style.padding.top,
        ),
        (
            &mut style.border_width.right,
            &mut style.border_style.right,
            &mut style.border_color.right,
            &mut style.padding.right,
        ),
        (
            &mut style.border_width.bottom,
            &mut style.border_style.bottom,
            &mut style.border_color.bottom,
            &mut style.padding.bottom,
        ),
        (
            &mut style.border_width.left,
            &mut style.border_style.left,
            &mut style.border_color.left,
            &mut style.padding.left,
        ),
    ];
    for (width, border_style, border_color, padding) in sides {
        if *width <= 0.0 {
            continue;
        }
        add_px(padding, *width - stroke);
        *width = stroke;
        *border_style = BorderStyle::Solid;
        *border_color = Some(color);
    }
    style.native_frame = Some(if hairline { 0.0 } else { stroke });
    // A field keeps a transparent ground (`PaintTextField` draws none).
    if frame == Frame::Button || style.background_color.a > 0.0 {
        style.background_color = dark.map_or_else(|| frame_ground(frame, disabled), |(ground, _)| ground);
    }
    let corners = &mut style.border_radius;
    let zero = |corner: &(LengthPercentage, LengthPercentage)| corner.0.fixed() == Some(0.0);
    if zero(&corners.top_left)
        && zero(&corners.top_right)
        && zero(&corners.bottom_right)
        && zero(&corners.bottom_left)
    {
        // A radius of 2px (zoomed) along the middle of the border.
        let radius = 2.0 * zoom + stroke / 2.0;
        let radius = (LengthPercentage::Px(radius), LengthPercentage::Px(radius));
        corners.top_left = radius.clone();
        corners.top_right = radius.clone();
        corners.bottom_right = radius.clone();
        corners.bottom_left = radius;
    }
}

/// Adds `px` to a length, if it is one.
fn add_px(length: &mut LengthPercentage, px: f32) {
    if let LengthPercentage::Px(value) = length {
        *value += px;
    }
}

/// The style of a `<button>`, with the native frame where it has its native look.
pub(super) fn button_style(style: &Arc<ComputedStyle>, disabled: bool) -> Arc<ComputedStyle> {
    let mut style = (**style).clone();
    if native(&style) {
        native_frame(&mut style, Frame::Button, disabled);
    }
    // Chrome centers the content of a button in its height.
    if style.align_content == fagus_style::values::ContentAlignment::Normal {
        style.align_content = fagus_style::values::ContentAlignment::Center;
    }
    Arc::new(style)
}

/// The width and height of the content box of a box with `style`, in pixels, where its style
/// gives them in pixels.
fn fixed_size(style: &ComputedStyle) -> (Option<f32>, Option<f32>) {
    let frame = |a: &LengthPercentage, b: &LengthPercentage, border: f32| {
        a.fixed().unwrap_or(0.0) + b.fixed().unwrap_or(0.0) + border
    };
    let border_box = style.box_sizing == BoxSizing::BorderBox;
    let content = |size: &Size, frame: f32| match size {
        Size::Length(length) => length
            .fixed()
            .map(|size| if border_box { (size - frame).max(0.0) } else { size }),
        _ => None,
    };
    (
        content(
            &style.width,
            frame(
                &style.padding.left,
                &style.padding.right,
                style.border_width.left + style.border_width.right,
            ),
        ),
        content(
            &style.height,
            frame(
                &style.padding.top,
                &style.padding.bottom,
                style.border_width.top + style.border_width.bottom,
            ),
        ),
    )
}

/// Sets an `auto` width or height of a style to a content size, as a border-box size if the
/// box is sized so.
fn set_content_size(style: &mut ComputedValues, width: Option<f32>, height: Option<f32>) {
    let border_box = style.box_sizing == BoxSizing::BorderBox;
    if let Some(width) = width
        && style.width == Size::Auto
    {
        let frame = if border_box {
            style.padding.left.fixed().unwrap_or(0.0)
                + style.padding.right.fixed().unwrap_or(0.0)
                + style.border_width.left
                + style.border_width.right
        } else {
            0.0
        };
        style.width = Size::Length(LengthPercentage::Px(width + frame));
    }
    if let Some(height) = height
        && style.height == Size::Auto
    {
        let frame = if border_box {
            style.padding.top.fixed().unwrap_or(0.0)
                + style.padding.bottom.fixed().unwrap_or(0.0)
                + style.border_width.top
                + style.border_width.bottom
        } else {
            0.0
        };
        style.height = Size::Length(LengthPercentage::Px(height + frame));
    }
}

/// Font families whose average character width Chrome does not trust: it takes the width of a
/// `0` instead (`kFontFamiliesWithInvalidCharWidth` in `layout_text_control.cc`).
const INVALID_CHAR_WIDTH: [&str; 34] = [
    "American Typewriter",
    "Arial Hebrew",
    "Chalkboard",
    "Cochin",
    "Corsiva Hebrew",
    "Courier",
    "Euphemia UCAS",
    "Geneva",
    "Gill Sans",
    "Hei",
    "Helvetica",
    "Hoefler Text",
    "InaiMathi",
    "Kai",
    "Lucida Grande",
    "Marker Felt",
    "Monaco",
    "Mshtakan",
    "New Peninim MT",
    "Osaka",
    "Raanana",
    "STHeiti",
    "Symbol",
    "Times",
    "Apple Braille",
    "Apple LiGothic",
    "Apple LiSung",
    "Apple Symbols",
    "AppleGothic",
    "AppleMyungjo",
    "#GungSeo",
    "#HeadLineA",
    "#PCMyungjo",
    "#PilGi",
];

/// What sizes text controls in a style's font, in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct CharWidths {
    /// The width of an average character, as Chrome on Linux takes it: the OS/2 table's (else
    /// an `x`'s), not less than that rounded; of a `0` for fonts without a trusted average.
    pub average: f32,
    /// The widest character, as Chrome on Linux takes it: the width of the box around all
    /// glyphs (the `head` table's), rounded; 0 for fonts without a trusted average.
    pub max: f32,
}

/// The character widths of a style's first available font (`GetAvgCharWidth` in Chrome).
pub(super) fn char_widths(fonts: Option<&FontDb>, style: &ComputedStyle) -> CharWidths {
    let fallback = CharWidths {
        average: (style.font_size / 2.0).round(),
        max: 0.0,
    };
    let Some(fonts) = fonts else {
        return fallback;
    };
    let Some(face_id) = face_for(fonts, style) else {
        return fallback;
    };
    let size = used_font_size(fonts, style);
    let face = fonts.face(face_id);
    let scale = face.scale(size);
    let advance = |character: char| {
        face.has_glyph(character)
            .then(|| fonts.shape(face_id, &character.to_string()).advance * scale)
    };
    let zero = advance('0');
    // Skia's average width from the OS/2 table, else Blink's from the "x" (`PlatformInit`).
    let average = if face.metrics.average_width > 0.0 {
        face.metrics.average_width * scale
    } else {
        advance('x').unwrap_or_else(|| face.x_height_at(size))
    };
    let average = average.max(average.round());
    let family = style.font_family.first().map_or("", String::as_str);
    let trusted = !family.is_empty()
        && !INVALID_CHAR_WIDTH
            .iter()
            .any(|invalid| invalid.eq_ignore_ascii_case(family))
        && !zero.is_some_and(|zero| average > zero * 1.7);
    if trusted {
        CharWidths {
            average,
            max: (face.metrics.bounding_width * scale).round(),
        }
    } else {
        CharWidths {
            average: zero.unwrap_or(fallback.average),
            max: 0.0,
        }
    }
}

/// The rounded ascent and descent of a style's first available font, in pixels.
fn font_height(fonts: Option<&FontDb>, style: &ComputedStyle) -> Option<f32> {
    let fonts = fonts?;
    let face = fonts.face(face_for(fonts, style)?);
    let (ascent, descent) = face.rounded_ascent_descent(used_font_size(fonts, style));
    Some(ascent + descent)
}

/// The content width of a text field `size` characters wide (`TextFieldIntrinsicInlineSize`).
pub(super) fn field_width(widths: CharWidths, size: u32) -> f32 {
    let mut width = widths.average * size as f32;
    if widths.max > widths.average {
        width += widths.max - widths.average;
    }
    width.ceil()
}

/// The width of a text in a style's first available font, in pixels, shaped in the style's
/// language.
fn text_width(fonts: Option<&FontDb>, style: &ComputedStyle, text: &str) -> f32 {
    let Some(fonts) = fonts else {
        return text.chars().count() as f32 * style.font_size / 2.0;
    };
    let Some(face) = face_for(fonts, style) else {
        return 0.0;
    };
    // In the language of the control, as its text is shaped.
    let flow = fagus_text::Flow::LeftToRight;
    let shaped = fonts.shape_in_language(face, text, &[], &[], flow, style.lang.as_deref());
    shaped.advance * fonts.face(face).scale(used_font_size(fonts, style))
}

/// The `line-height` of the line inside a one-line text field of style `own`, as Blink gives
/// its inner editor (`CreateInnerEditorStyle`): `normal` where the field's line height, cut to
/// whole pixels, is no more than its font size, or where its height is a percentage or a
/// length larger than that line height; the field's own otherwise.
fn editor_line_height(fonts: Option<&FontDb>, own: &ComputedStyle) -> LineHeight {
    let line = match own.line_height {
        // A length counts as it is, unrounded.
        LineHeight::Px(px) => px,
        _ => line_height(fonts, own),
    }
    .trunc();
    let height = if own.is_vertical() {
        &own.width
    } else {
        &own.height
    };
    let normal = own.font_size >= line
        || match height {
            Size::Length(length) => length.fixed().is_none_or(|height| height > line),
            _ => false,
        };
    if normal {
        LineHeight::Normal
    } else {
        own.line_height
    }
}

/// The `line-height` of a style in pixels, `normal` from its font.
fn line_height(fonts: Option<&FontDb>, style: &ComputedStyle) -> f32 {
    match fonts {
        Some(fonts) => metrics(fonts, style).line_height,
        None => style.line_height_px(style.font_size * 1.2),
    }
}

/// A number attribute of an element, if it is a positive integer.
fn positive(data: &ElementData, name: &str) -> Option<u32> {
    data.attribute(name)
        .and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|value| *value > 0)
}

/// How many characters a number field with a minimum and a maximum is wide: those of the longer
/// of them, with as many decimals as its step has.
fn number_size(data: &ElementData) -> Option<u32> {
    let (min, max) = (number(data, "min")?, number(data, "max")?);
    if min > max {
        return None;
    }
    let integer = |value: f32| format!("{}", value.trunc() as i64).len();
    let decimals = data
        .attribute("step")
        .and_then(|step| step.trim().split_once('.').map(|(_, decimals)| decimals.len()))
        .unwrap_or(0);
    let length = integer(min).max(integer(max)) + if decimals > 0 { decimals + 1 } else { 0 };
    u32::try_from(length).ok()
}

/// A number attribute of an element, if it is a finite number.
fn number(data: &ElementData, name: &str) -> Option<f32> {
    data.attribute(name)
        .and_then(|value| value.trim().parse::<f32>().ok())
        .filter(|value| value.is_finite())
}

/// Text with its white space collapsed and trimmed, as an option shows its label.
fn collapse(text: &str) -> String {
    text.split_ascii_whitespace().collect::<Vec<_>>().join(" ")
}

/// An SVG image of `width` × `height` CSS pixels with `body` as its content.
fn svg(width: f32, height: f32, body: &str) -> Arc<Image> {
    let data = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">{body}</svg>"#
    );
    Arc::new(Image {
        data: Arc::new(data.into_bytes()),
        format: ImageFormat::Svg,
        width,
        height,
        natural: (true, true),
        ratio: (width > 0.0 && height > 0.0).then(|| width / height),
        inline: false,
        orientation: 1,
        filters: Arc::from([]),
        pad: 0,
    })
}

/// A color in SVG.
fn paint(color: Rgba) -> String {
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "rgb({},{},{})\" fill-opacity=\"{}",
        channel(color.r),
        channel(color.g),
        channel(color.b),
        color.a
    )
}

/// A color as an SVG stroke.
fn stroke(color: Rgba) -> String {
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "rgb({},{},{})\" stroke-opacity=\"{}",
        channel(color.r),
        channel(color.g),
        channel(color.b),
        color.a
    )
}

/// The mark of a checkbox or radio button in a box of `width` × `height`: the largest square
/// that fits, centered (`PaintCheckbox`, `PaintRadio`). A disabled one is gray, whatever its
/// accent, on a lightened ground.
pub(super) fn check_mark(
    width: f32,
    height: f32,
    radio: bool,
    checked: bool,
    accent: Rgba,
    (disabled, dark): (bool, bool),
) -> String {
    let palette = palette(dark);
    let side = width.min(height);
    let (x, y) = ((width - side) / 2.0, (height - side) / 2.0);
    let accent = if disabled { palette.disabled_accent } else { accent };
    let border = match (checked, disabled) {
        (true, _) => accent,
        (false, true) => palette.disabled_border,
        (false, false) => palette.border,
    };
    let mut out = String::new();
    if side <= 2.0 {
        let _ = write!(
            out,
            r#"<rect x="{x}" y="{y}" width="{side}" height="{side}" fill="{}"/>"#,
            paint(border)
        );
        return out;
    }
    let radius = if radio { side / 2.0 } else { 2.0 };
    let rect = |out: &mut String, inset: f32, radius: f32, attributes: &str| {
        let size = side - 2.0 * inset;
        let radius = radius.min(size / 2.0);
        let _ = write!(
            out,
            r#"<rect x="{}" y="{}" width="{size}" height="{size}" rx="{radius}" {attributes}/>"#,
            x + inset,
            y + inset
        );
    };
    // The ground, lightened where the control is disabled; the check is drawn in its color.
    let ground = if disabled {
        rect(
            &mut out,
            0.2,
            radius,
            &format!(r#"fill="{}""#, paint(palette.disabled_layer)),
        );
        palette.disabled_ground
    } else {
        palette.ground
    };
    rect(&mut out, 0.2, radius, &format!(r#"fill="{}""#, paint(ground)));
    if !radio && checked {
        rect(&mut out, 0.0, radius, &format!(r#"fill="{}""#, paint(accent)));
        let _ = write!(
            out,
            r#"<path d="M{} {} l{} {} L{} {}" fill="none" stroke="{}" stroke-width="{}"/>"#,
            x + side * 0.2,
            y + side / 2.0,
            side * 0.2,
            side * 0.2,
            x + side - side * 0.2,
            y + side * 0.2,
            stroke(ground),
            side * 0.16
        );
    } else {
        rect(
            &mut out,
            0.5,
            radius,
            &format!(r#"fill="none" stroke="{}" stroke-width="1""#, stroke(border)),
        );
    }
    if radio && checked {
        rect(
            &mut out,
            side * 0.2,
            radius,
            &format!(r#"fill="{}""#, paint(accent)),
        );
    }
    out
}

/// A progress bar or slider track of `width` × `height` filled to `fraction` (`PaintProgressBar`).
fn progress_bar(width: f32, height: f32, fraction: Option<f32>, accent: Rgba, dark: bool) -> String {
    let palette = palette(dark);
    let thickness = height * 8.0 / 16.0;
    let top = height / 2.0 - thickness / 2.0;
    let radius = (thickness / 2.0).min(width / 2.0);
    let mut out = format!(
        r#"<defs><clipPath id="t"><rect y="{top}" width="{width}" height="{thickness}" rx="{radius}"/></clipPath></defs><rect y="{top}" width="{width}" height="{thickness}" rx="{radius}" fill="{}"/>"#,
        paint(palette.track)
    );
    if let Some(fraction) = fraction {
        let mut value = (width * fraction.clamp(0.0, 1.0)).round();
        if value > 0.0 {
            value = value.max(2.0);
            let _ = write!(
                out,
                r#"<rect y="{top}" width="{value}" height="{thickness}" fill="{}" clip-path="url(#t)"/>"#,
                paint(accent)
            );
        }
    } else {
        // An indeterminate bar shows the first frame of its animation: a rounded fifth of the
        // bar at its start.
        let value = (width / 5.0).floor();
        let _ = write!(
            out,
            r#"<rect y="{top}" width="{value}" height="{thickness}" rx="{}" fill="{}"/>"#,
            radius.min(value / 2.0),
            paint(accent)
        );
    }
    let _ = write!(
        out,
        r#"<rect x="0.5" y="{}" width="{}" height="{}" rx="{}" fill="none" stroke="{}" stroke-width="1"/>"#,
        top + 0.5,
        (width - 1.0).max(0.0),
        (thickness - 1.0).max(0.0),
        (radius - 0.5).max(0.0),
        stroke(palette.track_border)
    );
    out
}

/// A slider of `width` × `height` with its thumb at `fraction` (`PaintSliderTrack`,
/// `PaintSliderThumb`), without its track unless `track`; a disabled one is gray. The track
/// takes the whole box, the thumb moves in `content` (x, y, width, height), its content box.
fn slider(
    width: f32,
    height: f32,
    content: [f32; 4],
    fraction: f32,
    accent: Rgba,
    (disabled, dark): (bool, bool),
    track: bool,
) -> String {
    const THUMB: f32 = 16.0;
    let palette = palette(dark);
    let [content_x, content_y, content_width, content_height] = content;
    let thumb_x = content_x + ((content_width - THUMB).max(0.0) * fraction.clamp(0.0, 1.0)).round();
    let top = (height / 2.0 - 4.0).max(0.0);
    let thickness = 8.0_f32.min(height);
    let radius = thickness / 2.0;
    let track_width = (width - 2.0).max(0.0);
    let value = thumb_x + THUMB / 2.0;
    let center = content_y + content_height / 2.0;
    let accent = if disabled { palette.disabled_slider } else { accent };
    let thumb = format!(
        r#"<circle cx="{}" cy="{center}" r="{}" fill="{}"/>"#,
        thumb_x + THUMB / 2.0,
        (THUMB.min(content_height) / 2.0 - 0.5).max(0.0),
        paint(accent)
    );
    if !track {
        return thumb;
    }
    format!(
        concat!(
            r#"<defs><clipPath id="v"><rect width="{value}" height="{height}"/></clipPath></defs>"#,
            r#"<rect x="1" y="{top}" width="{track_width}" height="{thickness}" rx="{radius}" fill="{track}"/>"#,
            r#"<rect x="1" y="{top}" width="{track_width}" height="{thickness}" rx="{radius}" fill="{accent}" clip-path="url(#v)"/>"#,
            r#"<rect x="1.5" y="{inner_top}" width="{inner_width}" height="{inner_height}" rx="{inner_radius}" fill="none" stroke="{border}" stroke-width="1"/>"#,
            "{thumb}",
        ),
        value = value,
        height = height,
        top = top,
        track_width = track_width,
        thickness = thickness,
        radius = radius,
        track = paint(if disabled {
            palette.disabled_track
        } else {
            palette.track
        }),
        accent = paint(accent),
        inner_top = top + 0.5,
        inner_width = (track_width - 1.0).max(0.0),
        inner_height = (thickness - 1.0).max(0.0),
        inner_radius = (radius - 0.5).max(0.0),
        border = stroke(if disabled {
            palette.disabled_track_border
        } else {
            palette.track_border
        }),
        thumb = thumb,
    )
}

/// A track or thumb of a slider without its native look as rules style it
/// (`::-webkit-slider-runnable-track`, `::-webkit-slider-thumb`): a rounded box of a color.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SliderPart {
    /// Its width (a thumb's; a track spans the slider) and height, where they are lengths.
    width: Option<f32>,
    height: Option<f32>,
    color: Rgba,
    /// The radius of its corners.
    radius: f32,
    /// Its top margin: a thumb's, from the top of the track.
    margin_top: f32,
}

impl SliderPart {
    /// The part that `style` gives, where it has no native look (a thumb with it keeps
    /// Chrome's).
    fn of(style: &ComputedStyle, thumb: bool) -> Option<Self> {
        if thumb && style.appearance == Appearance::Auto {
            return None;
        }
        let length = |size: &Size| match size {
            Size::Length(length) => length.fixed(),
            _ => None,
        };
        Some(Self {
            width: length(&style.width),
            height: length(&style.height),
            color: style.background_color,
            radius: style.border_radius.top_left.0.fixed().unwrap_or(0.0),
            margin_top: style.margin.top.resolve_or(0.0, 0.0),
        })
    }
}

/// The track and thumb of a slider without its native look, where rules style them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct SliderParts {
    track: Option<SliderPart>,
    thumb: Option<SliderPart>,
}

/// A slider without its native look whose track or thumb rules style, as Chrome lays them out
/// in its content box (`content`: x, y, width, height): the track across it, centered in its
/// height (as tall as the thumb without a height of its own), the thumb in the track from its
/// top margin, moved along it by `fraction` of what the track is wider. A thumb with its native
/// look is a circle of the accent color in the middle of the track.
fn styled_slider(content: [f32; 4], fraction: f32, accent: Rgba, parts: SliderParts) -> String {
    const THUMB: f32 = 16.0;
    let [x, y, width, height] = content;
    let rect = |(left, top, w, h): (f32, f32, f32, f32), radius: f32, color: Rgba| {
        let radius = radius.min(w / 2.0).min(h / 2.0).max(0.0);
        format!(
            r#"<rect x="{left}" y="{top}" width="{}" height="{}" rx="{radius}" fill="{}"/>"#,
            w.max(0.0),
            h.max(0.0),
            paint(color)
        )
    };
    let thumb_size = parts.thumb.map_or((THUMB, THUMB), |thumb| {
        (thumb.width.unwrap_or(THUMB), thumb.height.unwrap_or(0.0))
    });
    let track_height = parts.track.and_then(|track| track.height).unwrap_or(thumb_size.1);
    let track_top = y + (height - track_height) / 2.0;
    let mut body = String::new();
    if let Some(track) = parts.track {
        body.push_str(&rect(
            (x, track_top, width, track_height),
            track.radius,
            track.color,
        ));
    }
    let thumb_left = x + (width - thumb_size.0).max(0.0) * fraction.clamp(0.0, 1.0);
    match parts.thumb {
        Some(thumb) => body.push_str(&rect(
            (
                thumb_left,
                track_top + thumb.margin_top,
                thumb_size.0,
                thumb_size.1,
            ),
            thumb.radius,
            thumb.color,
        )),
        None => {
            let _ = write!(
                body,
                r#"<circle cx="{}" cy="{}" r="{}" fill="{}"/>"#,
                thumb_left + THUMB / 2.0,
                track_top + track_height / 2.0,
                THUMB / 2.0 - 0.5,
                paint(accent)
            );
        }
    }
    body
}

/// The room Chrome keeps at the end of a `<select>` for its arrow at an effective zoom
/// (`ClampedMenuListArrowPaddingSize`): [`ARROW_WIDTH`] zoomed, but not below it.
fn arrow_box(zoom: f32) -> f32 {
    ARROW_WIDTH * zoom.max(1.0)
}

/// The padding Chrome keeps at the end of a `<select>` besides the room for its arrow: a pixel
/// zoomed, which with the room is cut to whole pixels (`PopupInternalPaddingEnd`).
fn arrow_padding(zoom: f32) -> f32 {
    (zoom + arrow_box(zoom) + 1e-4).floor() - arrow_box(zoom)
}

/// The arrow of a `<select>` at an effective zoom (`PaintMenuList`): a "v" twice as wide as it
/// is high, 8px wide zoomed, stroked 2px wide, in a box as wide as the room for the arrow, which
/// lies after [`arrow_padding`] at the end of the content box. The arrow lies where Chrome puts
/// it, in the middle of that room and the padding, its left edge cut to whole pixels.
fn select_arrow(color: Rgba, zoom: f32) -> Arc<Image> {
    let room = arrow_box(zoom);
    let size = (8.0 * room / ARROW_WIDTH + 1e-4).floor();
    let height = (size / 2.0).floor();
    let start = (zoom + room + 1e-4).floor() - (f32::midpoint(room, size) - 1e-4).ceil();
    svg(
        room,
        height + 3.0,
        &format!(
            r#"<path d="M{start} 1 L{} {} L{} 1" fill="none" stroke="{}" stroke-width="2"/>"#,
            start + size / 2.0,
            1.0 + height,
            start + size,
            stroke(color)
        ),
    )
}

/// The resize handle at the bottom right of a `<textarea>`, or mirrored at its bottom left
/// (`mirrored`, right to left): two diagonal strokes, black with white beside them
/// (`ScrollbarTheme::PaintResizer`).
fn resizer(mirrored: bool) -> Arc<Image> {
    const STROKES: &str = concat!(
        r#"<path d="M7 0 L0 7 M7 4 L4 7" fill="none" stroke="rgb(0,0,0)" stroke-opacity="0.6" stroke-width="1"/>"#,
        r#"<path d="M7 1 L1 7 M7 5 L5 7" fill="none" stroke="rgb(255,255,255)" stroke-opacity="0.6" stroke-width="1"/>"#,
    );
    if mirrored {
        svg(
            9.0,
            9.0,
            &format!(r#"<g transform="matrix(-1 0 0 1 9 0)">{STROKES}</g>"#),
        )
    } else {
        svg(9.0, 9.0, STROKES)
    }
}

/// A mark Chrome's theme draws in the content box of a control, at whatever size the box gets.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Mark {
    /// A checkbox or radio button.
    Check {
        radio: bool,
        checked: bool,
        accent: Rgba,
        disabled: bool,
        /// In the dark color scheme.
        dark: bool,
    },
    /// A slider with its thumb at `fraction`, without its track unless `track`; without its
    /// native look, with the track and thumb that rules give it (`parts`).
    Slider {
        fraction: f32,
        accent: Rgba,
        disabled: bool,
        track: bool,
        dark: bool,
        parts: SliderParts,
    },
    /// A progress bar filled to `fraction`, indeterminate without one.
    Progress {
        fraction: Option<f32>,
        accent: Rgba,
        dark: bool,
    },
}

/// The mark of a checkbox or radio button of `style` drawn in a content box of `width` ×
/// `height`, checked or not, as Chrome's theme draws it for a control with its native look: what
/// the control shows in the other state, for the field of an interactive form.
pub fn control_mark(
    style: &ComputedStyle,
    radio: bool,
    checked: bool,
    disabled: bool,
    (width, height): (f32, f32),
) -> Arc<Image> {
    let accent = style
        .accent_color
        .unwrap_or(if style.dark { DARK_ACCENT } else { ACCENT });
    Mark::Check {
        radio,
        checked,
        accent,
        disabled,
        dark: style.dark,
    }
    .image(width, height, [0.0, 0.0, width, height], style.zoom)
}

impl Mark {
    /// Whether the mark is drawn across the border box, not the content box: Chrome's theme
    /// paints a slider over its padding and border.
    pub(crate) fn covers_border_box(&self) -> bool {
        matches!(self, Self::Slider { track: true, .. })
    }

    /// The image of the mark in a box of `width` × `height` around a content box at `content`
    /// (x, y, width, height).
    /// With an effective zoom of `zoom`, the mark is drawn at the size divided by the zoom and
    /// scaled up, as Chrome's theme scales what it paints.
    pub(crate) fn image(&self, width: f32, height: f32, content: [f32; 4], zoom: f32) -> Arc<Image> {
        if (zoom - 1.0).abs() > 1e-6 && zoom > 0.0 {
            let image = self.image(
                width / zoom,
                height / zoom,
                content.map(|value| value / zoom),
                1.0,
            );
            return Image::zoomed(&image, zoom);
        }
        let body = match *self {
            Self::Check {
                radio,
                checked,
                accent,
                disabled,
                dark,
            } => check_mark(width, height, radio, checked, accent, (disabled, dark)),
            Self::Slider {
                fraction,
                accent,
                disabled,
                track,
                dark,
                parts,
            } => {
                if parts.track.is_some() || parts.thumb.is_some() {
                    styled_slider(content, fraction, accent, parts)
                } else {
                    slider(width, height, content, fraction, accent, (disabled, dark), track)
                }
            }
            Self::Progress {
                fraction,
                accent,
                dark,
            } => progress_bar(width, height, fraction, accent, dark),
        };
        let mut image = svg(width, height, &body);
        // Stretched, a control keeps the size it has in the other axis: its mark has no ratio.
        if let Some(image) = Arc::get_mut(&mut image) {
            image.ratio = None;
        }
        image
    }
}

impl Builder<'_> {
    /// The box of a form control.
    pub(super) fn control(
        &self,
        tree: &mut BoxTree,
        element: NodeId,
        control: &Control,
        style: &Arc<ComputedStyle>,
    ) -> BoxId {
        let Some(data) = self.document.element(element) else {
            return self.replaced(tree, element, style);
        };
        let mut own = ComputedStyle::from(ComputedValues {
            display: style.display.blockify(),
            ..ComputedValues::clone(style)
        });
        let native = native(style);
        let dark = style.dark;
        let accent = style
            .accent_color
            .unwrap_or(if dark { DARK_ACCENT } else { ACCENT });
        let disabled = data.attribute("disabled").is_some();
        match control {
            Control::Image => self.replaced(tree, element, style),
            Control::Check { radio } => {
                if style.appearance == Appearance::None {
                    // Without its look a checkbox is an empty box, whose `::before` and
                    // `::after` Chrome draws (the thumb of Pico CSS's switches), as items where
                    // it is a grid or flex container (a check mark centered with `display:
                    // grid; place-content: center`).
                    let generates = [PseudoKey::Before, PseudoKey::After].into_iter().any(|key| {
                        self.styled.pseudo(element, key).is_some_and(|pseudo| {
                            pseudo.display != Display::None && matches!(pseudo.content, Content::Items(_))
                        })
                    });
                    if generates {
                        return self.block_level_box(tree, element, Arc::new(own));
                    }
                    return self.replaced_box(tree, element, own, None);
                }
                // Chrome's theme drops the padding and border of a checkbox or radio button
                // (`AdjustCheckboxStyle`) and paints it in place of its background: its mark
                // takes the whole box.
                own.padding = fagus_style::values::Sides::all(LengthPercentage::ZERO);
                own.border_width = fagus_style::values::Sides::all(0.0);
                own.border_style = fagus_style::values::Sides::all(BorderStyle::None);
                own.border_image = None;
                own.background_color = Rgba::TRANSPARENT;
                own.background_image = Arc::from([]);
                let (width, height) = fixed_size(&own);
                let check_size = CHECK_SIZE * own.zoom;
                let (width, height) = (width.unwrap_or(check_size), height.unwrap_or(check_size));
                let checked = fagus_css::checked(self.document, element);
                let mark = Mark::Check {
                    radio: *radio,
                    checked,
                    accent,
                    disabled,
                    dark,
                };
                // 13 × 13 is its natural size, without a ratio: stretched in a flex column or
                // grid, it draws its mark in the middle of the box it gets.
                let zoom = own.zoom;
                let id = self.replaced_box(
                    tree,
                    element,
                    own,
                    Some(mark.image(width, height, [0.0, 0.0, width, height], zoom)),
                );
                tree.marks.insert(id, mark);
                tree.control_widths.insert(id, check_size);
                id
            }
            Control::Range => {
                let (width, height) = fixed_size(style);
                let (width, height) = (
                    width.unwrap_or(129.0 * style.zoom),
                    height.unwrap_or(16.0 * style.zoom),
                );
                let min = number(data, "min").unwrap_or(0.0);
                let max = number(data, "max").unwrap_or(100.0).max(min);
                let value = number(data, "value")
                    .unwrap_or(f32::midpoint(min, max))
                    .clamp(min, max);
                let fraction = if max > min {
                    (value - min) / (max - min)
                } else {
                    0.0
                };
                // Without its native look a slider keeps its size and its thumb, which has a
                // look of its own.
                let track = style.appearance == Appearance::Auto;
                // Its theme paints it in place of its background and border, which keep their
                // room.
                if track {
                    own.background_color = Rgba::TRANSPARENT;
                    own.background_image = Arc::from([]);
                    own.border_color = fagus_style::values::Sides::all(Some(Rgba::TRANSPARENT));
                    own.border_image = None;
                }
                // 129 × 16 is its natural size, without a ratio: Chrome stretches it in a flex
                // column or grid, its track across, and draws it at the size it gets.
                let parts = if track {
                    SliderParts::default()
                } else {
                    let part = |key, thumb| {
                        self.styled
                            .pseudo(element, key)
                            .and_then(|style| SliderPart::of(style, thumb))
                    };
                    SliderParts {
                        track: part(PseudoKey::SliderTrack, false),
                        thumb: part(PseudoKey::SliderThumb, true),
                    }
                };
                let mark = Mark::Slider {
                    fraction,
                    accent,
                    disabled,
                    track,
                    dark,
                    parts,
                };
                let zoom = own.zoom;
                let id = self.replaced_box(
                    tree,
                    element,
                    own,
                    Some(mark.image(width, height, [0.0, 0.0, width, height], zoom)),
                );
                tree.marks.insert(id, mark);
                id
            }
            Control::Meter => self.styled_meter(tree, element, own),
            Control::Progress => {
                let (width, height) = fixed_size(style);
                let em = style.font_size;
                let (width, height) = (width.unwrap_or(10.0 * em), height.unwrap_or(em));
                let native = style.appearance == Appearance::Auto && !style.author_frame;
                if !native {
                    return self.styled_progress(tree, element, data, own);
                }
                let fraction = fagus_style::progress_fraction(data);
                let mark = Mark::Progress {
                    fraction,
                    accent,
                    dark,
                };
                let image = mark.image(width, height, [0.0, 0.0, width, height], style.zoom);
                let id = self.replaced_box(tree, element, own, Some(image));
                tree.marks.insert(id, mark);
                id
            }
            Control::Color => {
                if native {
                    native_frame(&mut own, Frame::Button, disabled);
                }
                let zoom = own.zoom;
                let (width, height) = fixed_size(&own);
                let (width, height) = (width.unwrap_or(44.0 * zoom), height.unwrap_or(23.0 * zoom));
                let value = data
                    .attribute("value")
                    .filter(|value| value.len() == 7 && value.starts_with('#'))
                    .unwrap_or("#000000");
                let swatch = Image::zoomed(
                    &svg(
                        width / zoom,
                        height / zoom,
                        &format!(
                            r#"<rect x="2.5" y="4.5" width="{}" height="{}" fill="{value}" stroke="rgb(119,119,119)" stroke-width="1"/>"#,
                            (width / zoom - 5.0).max(0.0),
                            (height / zoom - 9.0).max(0.0)
                        ),
                    ),
                    zoom,
                );
                // The baseline is the bottom of the swatch, 4px above the content box.
                let raise = own.border_width.bottom + own.padding.bottom.fixed().unwrap_or(0.0) + 4.0 * zoom;
                let id = self.replaced_box(tree, element, own, Some(swatch));
                tree.border_box_baselines.insert(id, raise);
                id
            }
            Control::Field => self.field(tree, element, data, own, native),
            Control::Button => {
                if native {
                    native_frame(&mut own, Frame::Button, disabled);
                }
                // Chrome's labels are in the language of its user interface; Fagus takes the
                // language of the control.
                let ui = self.ui_locale(style);
                let label = data.attribute("value").map_or_else(
                    || match input_type(data).as_str() {
                        "submit" => ui.submit.to_owned(),
                        "reset" => ui.reset.to_owned(),
                        _ => String::new(),
                    },
                    str::to_owned,
                );
                let text_style = Arc::new(own.inherit());
                let items = text_items(label, &text_style);
                let id = tree.push(LayoutBox {
                    style: Arc::new(own),
                    node: Some(element),
                    generated: false,
                    kind: BoxKind::Inline { items },
                });
                // A block-level push button is as wide as its label, like `<button>` (Chrome's
                // `AutoWidthShouldFitContent`): Water.css makes submit buttons `display: block`.
                tree.shrinks_to_fit.insert(id);
                id
            }
            Control::File => {
                let multiple = data.attribute("multiple").is_some();
                self.file(tree, element, own, disabled, multiple)
            }
            Control::TextArea => self.text_area(tree, element, data, own, native),
            Control::Select => self.select(tree, element, data, own, native),
        }
    }

    /// How a control with `style` writes its texts: in the language of its `lang`, else in that
    /// the engine was given, else in American English.
    pub(super) fn ui_locale(&self, style: &ComputedStyle) -> &'static locale::UiLocale {
        locale::resolve(style.lang.as_deref(), self.locale)
    }

    /// A replaced box of a control with its mark, if it has one.
    /// A `<progress>` without its native look, as Chrome builds it: a block container around
    /// its track (`::-webkit-progress-bar`) around the filled part (`::-webkit-progress-value`),
    /// as wide as the fraction of the value, none where the value is indeterminate. Its content
    /// (the fallback text) does not show.
    fn styled_progress(
        &self,
        tree: &mut BoxTree,
        element: NodeId,
        data: &ElementData,
        own: ComputedStyle,
    ) -> BoxId {
        let bar = self.control_part(element, PseudoKey::ProgressBar, &own);
        let mut value = self.control_part(element, PseudoKey::ProgressValue, &own);
        // The element sets the width of the value in its inline style, which the cascade has
        // taken in.
        if self.styled.pseudo(element, PseudoKey::ProgressValue).is_none() {
            let fraction = fagus_style::progress_fraction(data).unwrap_or(0.0);
            value.width = Size::Length(LengthPercentage::Percent(fraction));
        }
        let value = tree.push(LayoutBox {
            style: Arc::new(value),
            node: None,
            generated: true,
            kind: BoxKind::Block { children: Vec::new() },
        });
        let bar = tree.push(LayoutBox {
            style: Arc::new(bar),
            node: None,
            generated: true,
            kind: BoxKind::Block {
                children: vec![value],
            },
        });
        tree.push(LayoutBox {
            style: Arc::new(own),
            node: Some(element),
            generated: false,
            kind: BoxKind::Block { children: vec![bar] },
        })
    }

    /// Gives the text style of a control's placeholder what its `::placeholder` sets: its color
    /// and font, its spacing, case and decorations, faded by its opacity. The line keeps the control's height, as
    /// Chrome's placeholder has the line height of the field (`line-height: initial` in an
    /// `<input>`). Without the pseudo-element's style, the placeholder is gray.
    fn placeholder_style(&self, element: NodeId, text: &mut ComputedValues) {
        let Some(own) = self.styled.pseudo(element, PseudoKey::Placeholder) else {
            text.color = PLACEHOLDER;
            return;
        };
        // Its opacity fades its text, which is all it paints.
        let opacity = own.opacity;
        let fade = move |color: Rgba| Rgba {
            a: color.a * opacity,
            ..color
        };
        text.color = Rgba {
            a: own.color.a * opacity,
            ..own.color
        };
        text.text_fill_color = own.text_fill_color.map(fade);
        text.font_family = Arc::clone(&own.font_family);
        text.font_size = own.font_size;
        text.font_weight = own.font_weight;
        text.font_style = own.font_style;
        text.font_stretch = own.font_stretch;
        text.font_variant = own.font_variant;
        text.letter_spacing = own.letter_spacing;
        text.word_spacing = own.word_spacing;
        text.text_transform = own.text_transform;
        text.text_decoration_line = own.text_decoration_line;
        text.text_decoration_style = own.text_decoration_style;
        text.text_decoration_color = own.text_decoration_color;
        text.text_decoration_thickness = own.text_decoration_thickness;
        text.text_shadow = Arc::clone(&own.text_shadow);
    }

    /// The style of a part of a `<progress>` or `<meter>`: a `<div>` in its shadow tree, so a
    /// block.
    fn control_part(&self, element: NodeId, key: PseudoKey, own: &ComputedStyle) -> ComputedStyle {
        let mut style = self
            .styled
            .pseudo(element, key)
            .map_or_else(|| own.inherit(), |style| (**style).clone());
        if style.display != Display::None {
            style.display = Display::BLOCK;
        }
        style
    }

    /// A `<meter>` as Chrome builds it, with its native look or without: its track
    /// (`::-webkit-meter-bar`) around the filled part (`::-webkit-meter-optimum-value` and the
    /// like), as wide as the fraction of the value. Chrome places the track in the middle row of
    /// a grid of rows `1fr 2fr 1fr`, centered and as high as that row by default: here the
    /// meter is a flex column centering the track, whose percentage height refers to half the
    /// meter's. Its content (the fallback text) does not show.
    fn styled_meter(&self, tree: &mut BoxTree, element: NodeId, mut own: ComputedStyle) -> BoxId {
        let mut bar = self.control_part(element, PseudoKey::MeterBar, &own);
        let value = self.control_part(element, PseudoKey::MeterValue, &own);
        let half = |size: &Size| match size {
            Size::Length(LengthPercentage::Percent(percent)) => {
                Size::Length(LengthPercentage::Percent(percent / 2.0))
            }
            other => other.clone(),
        };
        bar.height = half(&bar.height);
        bar.min_height = half(&bar.min_height);
        own.display = flex_display(own.display);
        own.flex_direction = fagus_style::values::FlexDirection::Column;
        own.justify_content = fagus_style::values::ContentAlignment::Center;
        let value = tree.push(LayoutBox {
            style: Arc::new(value),
            node: None,
            generated: true,
            kind: BoxKind::Block { children: Vec::new() },
        });
        let bar = tree.push(LayoutBox {
            style: Arc::new(bar),
            node: None,
            generated: true,
            kind: BoxKind::Block {
                children: vec![value],
            },
        });
        let id = tree.push(LayoutBox {
            style: Arc::new(own),
            node: Some(element),
            generated: false,
            kind: BoxKind::Flex { children: vec![bar] },
        });
        // Its baseline is the bottom of its border box, as of a control drawn by the theme.
        tree.border_box_baselines.insert(id, 0.0);
        id
    }

    fn replaced_box(
        &self,
        tree: &mut BoxTree,
        element: NodeId,
        style: ComputedStyle,
        image: Option<Arc<Image>>,
    ) -> BoxId {
        let drawn = image.is_some();
        let id = tree.push(LayoutBox {
            style: Arc::new(style),
            node: Some(element),
            generated: false,
            kind: BoxKind::Replaced {
                image,
                attributes: (None, None),
            },
        });
        // Chrome puts the baseline of a control drawn by its theme at the bottom of its border
        // box.
        if drawn {
            tree.border_box_baselines.insert(id, 0.0);
        }
        id
    }

    /// A one-line text field: a flex box around a box with the line of its value or
    /// placeholder, cut off at its edges and centered in its height.
    fn field(
        &self,
        tree: &mut BoxTree,
        element: NodeId,
        data: &ElementData,
        mut own: ComputedStyle,
        native: bool,
    ) -> BoxId {
        let disabled = data.attribute("disabled").is_some();
        let kind = input_type(data);
        if native {
            native_frame(&mut own, Frame::Field, disabled);
        }
        let ui = self.ui_locale(&own);
        if let Some(parts) = dates::parts(
            &kind,
            data.attribute("value"),
            data.attribute("step"),
            data.attribute("max"),
            ui,
        ) {
            let hidden = disabled || data.attribute("readonly").is_some();
            return self.date_field(tree, element, own, &kind, &parts, hidden);
        }
        let value = data
            .attribute("value")
            .map(|value| value.replace(['\r', '\n'], ""));
        let value = match kind.as_str() {
            "password" => value.map(|value| "\u{2022}".repeat(value.chars().count())),
            "number" => value
                .filter(|value| value.trim().parse::<f64>().is_ok_and(f64::is_finite))
                .map(|value| locale::localize_number(&value, ui)),
            "email" | "url" => value.map(|value| value.trim().to_owned()),
            _ => value,
        }
        .filter(|value| !value.is_empty());
        let (text, placeholder) = match value {
            Some(value) => (value, false),
            None => (
                data.attribute("placeholder")
                    .map(|text| text.replace(['\r', '\n'], ""))
                    .unwrap_or_default(),
                true,
            ),
        };
        let width = if own.field_sizing_content {
            // As wide as its text, at least as the caret (1px), and its spin button.
            text_width(self.fonts, &own, &text).max(1.0)
                + if kind == "number" {
                    SPIN_BUTTON * own.zoom
                } else {
                    0.0
                }
        } else if kind == "number" {
            // A number field ignores `size`; with a minimum and a maximum it is as wide as
            // their longest number and its spin button (`GetSizeWithDecoration`).
            match number_size(data) {
                Some(size) => field_width(char_widths(self.fonts, &own), size) + SPIN_BUTTON * own.zoom,
                None => field_width(char_widths(self.fonts, &own), 20),
            }
        } else {
            field_width(
                char_widths(self.fonts, &own),
                positive(data, "size").unwrap_or(20),
            )
        };
        own.display = flex_display(own.display);
        own.align_items = ItemAlignment::Center;
        let mut inner = field_line_style(&own);
        inner.line_height = editor_line_height(self.fonts, &own);
        let mut text_style = inner.inherit();
        if placeholder {
            self.placeholder_style(element, &mut text_style);
        }
        let line = tree.push(LayoutBox {
            style: Arc::new(inner),
            node: None,
            generated: true,
            kind: BoxKind::Inline {
                items: text_items(text, &Arc::new(text_style)),
            },
        });
        let id = tree.push(LayoutBox {
            style: Arc::new(own),
            node: Some(element),
            generated: false,
            kind: BoxKind::Flex { children: vec![line] },
        });
        intrinsic_width(tree, id, width);
        id
    }

    /// A date or time field: a flex box around a box with the line of its fields, each with a
    /// pixel of room on either side and a pixel above and below the line, and the picker icon
    /// (hidden where the field is disabled or read-only). Unless its width is set, the field is
    /// as wide as all its fields at their widest and the icon (`DateTimeEditElement`).
    fn date_field(
        &self,
        tree: &mut BoxTree,
        element: NodeId,
        mut own: ComputedStyle,
        kind: &str,
        parts: &[dates::Part],
        hidden: bool,
    ) -> BoxId {
        let zoom = own.zoom;
        let mut indicator = dates::indicator(kind, own.font_size);
        indicator.padding *= zoom;
        indicator.margin *= zoom;
        let width = {
            let fields: f32 = parts
                .iter()
                .map(|part| match part {
                    // Each field keeps the room of its widest text, whatever `field-sizing`.
                    dates::Part::Field { room, .. } => {
                        room.iter()
                            .map(|text| text_width(self.fonts, &own, text))
                            .fold(0.0, f32::max)
                            + 2.0 * zoom
                    }
                    dates::Part::Text(text) => text_width(self.fonts, &own, text),
                })
                .sum();
            let icon = indicator.margin + indicator.side + 2.0 * indicator.padding;
            fields.ceil() + icon
        };
        own.display = flex_display(own.display);
        own.align_items = ItemAlignment::Center;
        let mut inner = field_line_style(&own);
        inner.line_height = own.line_height;
        inner.padding.top = LengthPercentage::Px(zoom);
        inner.padding.bottom = LengthPercentage::Px(zoom);
        let text_style = Arc::new(inner.inherit());
        let mut field = text_style.inherit();
        field.padding.left = LengthPercentage::Px(zoom);
        field.padding.right = LengthPercentage::Px(zoom);
        let field = Arc::new(field);
        let mut items = Vec::new();
        for part in parts {
            match part {
                dates::Part::Field { text, .. } => {
                    items.push(InlineItem::Open {
                        style: Arc::clone(&field),
                        node: None,
                    });
                    items.push(InlineItem::Text {
                        text: text.clone(),
                        style: Arc::clone(&text_style),
                    });
                    items.push(InlineItem::Close);
                }
                dates::Part::Text(text) => items.push(InlineItem::Text {
                    text: (*text).to_owned(),
                    style: Arc::clone(&text_style),
                }),
            }
        }
        let line = tree.push(LayoutBox {
            style: Arc::new(inner),
            node: None,
            generated: true,
            kind: BoxKind::Inline {
                items: super::process_white_space(items),
            },
        });
        let mut icon = anonymous_style(&own);
        icon.display = Display::BLOCK;
        icon.flex_shrink = 0.0;
        icon.padding = fagus_style::values::Sides::all(LengthPercentage::Px(indicator.padding));
        icon.margin.left = LengthPercentageAuto::Length(LengthPercentage::Px(indicator.margin));
        if hidden {
            icon.visibility = fagus_style::values::Visibility::Hidden;
        }
        let icon = tree.push(LayoutBox {
            style: Arc::new(icon),
            node: None,
            generated: true,
            kind: BoxKind::Replaced {
                image: Some(svg(indicator.side, indicator.side, &indicator.body)),
                attributes: (None, None),
            },
        });
        let id = tree.push(LayoutBox {
            style: Arc::new(own),
            node: Some(element),
            generated: false,
            kind: BoxKind::Flex {
                children: vec![line, icon],
            },
        });
        intrinsic_width(tree, id, width);
        id
    }

    /// The style and children of a fieldset with a rendered legend: its first `<legend>` child
    /// that neither floats nor is positioned out of flow (HTML, §15.3.13), with the legend's
    /// element. The legend sits on the top border: it is placed like a glyph, shrunk to fit, in
    /// an anonymous box that reaches up over the border. The rest of the content goes into an
    /// anonymous box below it that takes the fieldset's padding, as in Chrome.
    pub(super) fn fieldset(
        &self,
        tree: &mut BoxTree,
        style: &Arc<ComputedStyle>,
        children: &[BoxId],
    ) -> Option<(Arc<ComputedStyle>, Vec<BoxId>, NodeId)> {
        let index = children.iter().position(|&child| {
            let child = tree.get(child);
            child
                .node
                .and_then(|node| self.document.element(node))
                .is_some_and(|data| data.is("legend"))
                && !child.style.is_floating()
                && !child.style.is_out_of_flow()
        })?;
        let legend_box = children[index];
        let legend = tree.get(legend_box).node?;
        let align = self
            .document
            .element(legend)
            .and_then(|data| data.attribute("align"))
            .map(str::to_ascii_lowercase);
        let mut wrapper = anonymous_style(style);
        wrapper.line_height = LineHeight::Px(0.0);
        // Nor does a strut take room around the legend's baseline: the line is the legend.
        wrapper.font_size = 0.0;
        wrapper.text_indent = LengthPercentage::ZERO;
        wrapper.text_align = match align.as_deref() {
            Some("center") => fagus_style::values::TextAlign::Center,
            Some("right") => fagus_style::values::TextAlign::Right,
            _ => fagus_style::values::TextAlign::Left,
        };
        wrapper.white_space = WhiteSpace::PRE;
        wrapper.margin.top = LengthPercentageAuto::Length(LengthPercentage::Px(-style.border_width.top));
        // A legend less tall than the top border is centered on it, and the content starts below
        // the border all the same, as in Chrome (WPT `contain-inline-size-fieldset`).
        wrapper.min_height = Size::Length(LengthPercentage::Px(style.border_width.top));
        wrapper.align_content = fagus_style::values::ContentAlignment::Center;
        // The legend gives the fieldset no baseline: its first and last ones are those of the
        // content below it, as in Chrome (`FieldsetLayoutAlgorithm`; WPT
        // `contain-size-fieldset-004`). The box keeps its style in its fragment for that.
        wrapper.layout_containment = true;
        let wrapper = tree.push(LayoutBox {
            style: Arc::new(wrapper),
            node: None,
            generated: true,
            kind: BoxKind::Inline {
                items: vec![InlineItem::Atomic(legend_box)],
            },
        });
        let mut content = anonymous_style(style);
        content.padding.top = style.padding.top.clone();
        content.padding.bottom = style.padding.bottom.clone();
        let rest: Vec<BoxId> = children
            .iter()
            .copied()
            .filter(|&child| child != legend_box)
            .collect();
        // In a multi-column fieldset the content box below the legend has the columns, as in
        // Chrome: its content goes into the flow that layout breaks into columns.
        let kind = if style.is_multicol() {
            content.column_count = style.column_count;
            content.column_width = style.column_width;
            content.column_height = style.column_height;
            content.column_wrap = style.column_wrap;
            content.column_gap.clone_from(&style.column_gap);
            content.row_gap.clone_from(&style.row_gap);
            content.column_fill = style.column_fill;
            content.column_rule_width = style.column_rule_width;
            content.column_rule_style = style.column_rule_style;
            content.column_rule_color = style.column_rule_color;
            let flow = tree.push(LayoutBox {
                style: Arc::new(anonymous_style(&content)),
                node: None,
                generated: false,
                kind: BoxKind::Block { children: rest },
            });
            BoxKind::Block { children: vec![flow] }
        } else {
            BoxKind::Block { children: rest }
        };
        let content = tree.push(LayoutBox {
            style: Arc::new(content),
            node: None,
            generated: false,
            kind,
        });
        let mut own = (**style).clone();
        // The fieldset keeps its padding at the sides; the box below the legend takes over the
        // padding above and below, which a `height` still leaves out (Chrome's
        // `FieldsetLayoutAlgorithm`).
        let vertical = fixed_px(&style.padding.top) + fixed_px(&style.padding.bottom);
        if style.box_sizing == BoxSizing::ContentBox {
            grow_size(&mut own.height, vertical);
            grow_size(&mut own.min_height, vertical);
            if let fagus_style::values::MaxSize::Length(length) = &mut own.max_height
                && let Some(px) = length.fixed()
            {
                *length = LengthPercentage::Px(px + vertical);
            }
        }
        // With size containment it is as large as if it were empty, its legend too: as high as
        // that padding, as in Chrome (WPT `contain-size-fieldset-003`).
        if style.size_containment().1 {
            own.contain_intrinsic_size.1 = Some(own.contain_intrinsic_size.1.unwrap_or(0.0) + vertical);
        }
        own.padding.top = LengthPercentage::ZERO;
        own.padding.bottom = LengthPercentage::ZERO;
        own.column_count = None;
        own.column_width = None;
        own.column_height = None;
        Some((Arc::new(own), vec![wrapper, content], legend))
    }

    /// A box of an image beside the text of a control: a `<select>`'s arrow, a date field's
    /// calendar.
    fn icon(&self, tree: &mut BoxTree, parent: &ComputedStyle, image: Arc<Image>) -> BoxId {
        let mut style = anonymous_style(parent);
        style.display = Display::BLOCK;
        style.flex_shrink = 0.0;
        tree.push(LayoutBox {
            style: Arc::new(style),
            node: None,
            generated: true,
            kind: BoxKind::Replaced {
                image: Some(image),
                attributes: (None, None),
            },
        })
    }

    /// `input type=file`: a button "Choose File" ("Choose Files" if it takes `multiple`) and the
    /// text "No file chosen", in the language of the control.
    fn file(
        &self,
        tree: &mut BoxTree,
        element: NodeId,
        mut own: ComputedStyle,
        disabled: bool,
        multiple: bool,
    ) -> BoxId {
        let ui = self.ui_locale(&own);
        let label = if multiple { ui.choose_files } else { ui.choose_file };
        let none = ui.no_file;
        let zoom = own.zoom;
        let stroke = frame_width(zoom);
        let mut button = anonymous_style(&own);
        button.display = Display::Normal {
            outside: DisplayOutside::Inline,
            inside: DisplayInside::FlowRoot,
            list_item: false,
        };
        button.box_sizing = BoxSizing::BorderBox;
        // A frame 1px wide, padding 2px and 7px within it: 3px and 8px in all, zoomed.
        button.padding.top = LengthPercentage::Px(3.0 * zoom - stroke);
        button.padding.bottom = LengthPercentage::Px(3.0 * zoom - stroke);
        button.padding.left = LengthPercentage::Px(8.0 * zoom - stroke);
        button.padding.right = LengthPercentage::Px(8.0 * zoom - stroke);
        button.margin.right = LengthPercentageAuto::Length(LengthPercentage::Px(4.0 * zoom));
        button.border_width = fagus_style::values::Sides::all(stroke);
        button.border_style = fagus_style::values::Sides::all(BorderStyle::Solid);
        button.border_color = fagus_style::values::Sides::all(Some(frame_border(disabled)));
        button.native_frame = Some(stroke);
        let radius = 2.0 * zoom + stroke / 2.0;
        let radius = (LengthPercentage::Px(radius), LengthPercentage::Px(radius));
        button.border_radius.top_left = radius.clone();
        button.border_radius.top_right = radius.clone();
        button.border_radius.bottom_right = radius.clone();
        button.border_radius.bottom_left = radius;
        button.background_color = frame_ground(Frame::Button, disabled);
        if disabled {
            button.color = Rgba {
                a: DISABLED_ALPHA,
                ..gray(0x10)
            };
        }
        button.white_space = WhiteSpace::PRE;
        let button_text = Arc::new(button.inherit());
        let button = tree.push(LayoutBox {
            style: Arc::new(ComputedStyle::from(ComputedValues {
                display: button.display.blockify(),
                ..ComputedValues::from(button)
            })),
            node: None,
            generated: true,
            kind: BoxKind::Inline {
                items: text_items(label.to_owned(), &button_text),
            },
        });
        let mut items = vec![InlineItem::Atomic(button)];
        let text = Arc::new(ComputedStyle::from(ComputedValues {
            white_space: WhiteSpace::PRE,
            ..ComputedValues::from(own.inherit())
        }));
        items.extend(text_items(none.to_owned(), &text));
        // As wide as 34 digits, or as the button and the text if they are wider
        // (`FileUploadControlIntrinsicInlineSize`); with `field-sizing: content`, as the button
        // and the text.
        if own.width == Size::Auto {
            let button_width = text_width(self.fonts, &button_text, label) + (16.0 + 4.0) * zoom;
            let content = button_width + text_width(self.fonts, &own, none);
            let width = if own.field_sizing_content {
                content
            } else {
                (34.0 * text_width(self.fonts, &own, "0")).max(content).ceil()
            };
            set_content_size(&mut own, Some(width), None);
        }
        tree.push(LayoutBox {
            style: Arc::new(ComputedStyle::from(ComputedValues {
                white_space: WhiteSpace::PRE,
                ..ComputedValues::from(own)
            })),
            node: Some(element),
            generated: false,
            kind: BoxKind::Inline { items },
        })
    }

    /// A `<textarea>`: its text, wrapped, in a box `cols` characters wide and `rows` lines high,
    /// cut off at its edges, with the resize handle in its corner.
    fn text_area(
        &self,
        tree: &mut BoxTree,
        element: NodeId,
        data: &ElementData,
        mut own: ComputedStyle,
        native: bool,
    ) -> BoxId {
        let disabled = data.attribute("disabled").is_some();
        let value = self.document.text_content(element).replace("\r\n", "\n");
        let (text, placeholder) = if value.is_empty() {
            (data.attribute("placeholder").unwrap_or_default().to_owned(), true)
        } else {
            (value, false)
        };
        if native {
            native_frame(&mut own, Frame::Field, disabled);
        }
        let cols = positive(data, "cols").unwrap_or(20);
        let rows = positive(data, "rows").unwrap_or(2);
        let line = line_height(self.fonts, &own);
        let (gutter, bar) = text_area_scrollbars(&own);
        // With `field-sizing: content`, as wide as its longest line and as high as its lines,
        // at least one, without room for scroll bars.
        let content = own.field_sizing_content;
        let width = if content {
            text.split('\n')
                .map(|line| text_width(self.fonts, &own, line))
                .fold(1.0, f32::max)
        } else {
            (char_widths(self.fonts, &own).average * cols as f32).ceil() + gutter
        };
        // A text longer than the box scrolls, but Chrome prints no scroll bar and keeps the
        // room of one only in the width the columns give (`overflow: auto`, which Fagus keeps
        // as `hidden`). Right to left, the resize handle is on the left, as in Chrome.
        let rtl = own.is_rtl();
        own.overflow_x = Overflow::Hidden;
        own.overflow_y = Overflow::Hidden;
        // Its text is in a box `rows` lines high, the inner editor, whose overflow shows: the
        // text area's own size stays auto, as that of a text field, so that it fits the columns
        // and rows in block flow and stretches in flex and grid layout, its text across.
        let mut inner = anonymous_style(&own);
        inner.display = Display::BLOCK;
        // In vertical writing modes the rows stack across, as in Chrome: the editor is that
        // wide (and the columns give the length of its lines, see `intrinsic_width`).
        if !content {
            let block = Size::Length(LengthPercentage::Px(line * rows as f32 + bar));
            if own.writing_mode.is_vertical() {
                inner.width = block;
            } else {
                inner.height = block;
            }
        }
        let mut text_style = own.inherit();
        if placeholder {
            self.placeholder_style(element, &mut text_style);
        }
        let mut items = text_items(text, &Arc::new(text_style));
        // Chrome draws the resizer of a text area that may be resized, with its native look or
        // without (Bootstrap's `.form-control`).
        if own.resize {
            if own.position == Position::Static {
                own.position = Position::Relative;
            }
            let mut handle = anonymous_style(&own);
            handle.position = Position::Absolute;
            if rtl {
                handle.inset.left = LengthPercentageAuto::Length(LengthPercentage::ZERO);
            } else {
                handle.inset.right = LengthPercentageAuto::Length(LengthPercentage::ZERO);
            }
            handle.inset.bottom = LengthPercentageAuto::Length(LengthPercentage::ZERO);
            let handle = tree.push(LayoutBox {
                style: Arc::new(handle),
                node: None,
                generated: true,
                kind: BoxKind::Replaced {
                    image: Some(resizer(rtl)),
                    attributes: (None, None),
                },
            });
            items.push(InlineItem::OutOfFlow(handle));
        }
        // A scroll container, it sits on its bottom margin edge (CSS 2.1, §10.8.1).
        let margin_bottom = own.margin.bottom.resolve_or(0.0, 0.0);
        let inner = tree.push(LayoutBox {
            style: Arc::new(inner),
            node: None,
            generated: true,
            kind: BoxKind::Inline { items },
        });
        let id = tree.push(LayoutBox {
            style: Arc::new(own),
            node: Some(element),
            generated: false,
            kind: BoxKind::Block {
                children: vec![inner],
            },
        });
        tree.border_box_baselines.insert(id, -margin_bottom);
        intrinsic_width(tree, id, width);
        id
    }

    /// A `<select>`: its selected option and the arrow, or a list of its options where it
    /// shows more than one (`multiple`, `size` above 1).
    fn select(
        &self,
        tree: &mut BoxTree,
        element: NodeId,
        data: &ElementData,
        mut own: ComputedStyle,
        native: bool,
    ) -> BoxId {
        let disabled = data.attribute("disabled").is_some();
        // An option that is not displayed, or in an optgroup that is not, shows no line, as in
        // Chrome (WPT `select-4-option-optgroup-display-none`).
        let hidden = |node: NodeId| {
            let mut node = Some(node);
            while let Some(current) = node.filter(|&current| current != element) {
                if self
                    .styled
                    .style(current)
                    .is_some_and(|style| style.display == Display::None)
                {
                    return true;
                }
                node = self.document.parent(current);
            }
            false
        };
        let options: Vec<(String, bool)> = self
            .document
            .descendants(element)
            .filter_map(|node| {
                let option = self.document.element(node)?;
                (option.is("option") && !hidden(node)).then(|| {
                    let label = option
                        .attribute("label")
                        .map_or_else(|| collapse(&self.document.text_content(node)), str::to_owned);
                    (label, option.attribute("selected").is_some())
                })
            })
            .collect();
        let multiple = data.attribute("multiple").is_some();
        let size = positive(data, "size").unwrap_or(if multiple { 4 } else { 1 });
        let options = if multiple {
            options
        } else {
            only_last_selected(options)
        };
        if native {
            let frame = if size > 1 { Frame::ListBox } else { Frame::Field };
            native_frame(&mut own, frame, disabled);
        }
        // Size containment in the width makes it as wide as a select without options, as in
        // Chrome (WPT `contain-size-select-elem-001`).
        let mut text_width = if own.size_containment().0 {
            0.0
        } else {
            options
                .iter()
                .map(|(label, _)| text_width(self.fonts, &own, label))
                .fold(0.0, f32::max)
        };
        if size > 1 {
            // With `field-sizing: content`, a line for each option.
            let size = if own.field_sizing_content {
                u32::try_from(options.len()).unwrap_or(u32::MAX).max(1)
            } else {
                size
            };
            return self.list_box(tree, element, own, &options, size, text_width);
        }
        let label = if multiple {
            // `multiple` with `size=1`: Chrome 157 shows a menu list with the selected option,
            // or else how many are selected, and keeps room for the latter.
            let selected: Vec<&String> = options
                .iter()
                .filter_map(|(label, selected)| selected.then_some(label))
                .collect();
            let count = self
                .ui_locale(&own)
                .selected
                .replace("{n}", &selected.len().to_string());
            text_width = text_width.max(self::text_width(self.fonts, &own, &count));
            match selected.as_slice() {
                [one] => (*one).clone(),
                _ => count,
            }
        } else {
            let selected = options.iter().rposition(|(_, selected)| *selected).unwrap_or(0);
            options
                .get(selected)
                .map(|(label, _)| label.clone())
                .unwrap_or_default()
        };
        // With `field-sizing: content`, as wide as the selected option.
        let text_width = if own.field_sizing_content {
            self::text_width(self.fonts, &own, &label)
        } else {
            text_width.ceil()
        };
        if own.appearance == Appearance::Auto {
            // The padding Chrome keeps inside a menu list (`PopupInternalPadding*`): 4px at its
            // start, 1px at its end, where the arrow is.
            let own = own.values_mut();
            let (start, end) = if own.is_rtl() {
                (&mut own.padding.right, &mut own.padding.left)
            } else {
                (&mut own.padding.left, &mut own.padding.right)
            };
            // Zoomed and cut to whole pixels.
            let zoom = own.zoom;
            add_px(start, (4.0 * zoom + 1e-4).floor());
            add_px(end, arrow_padding(zoom));
            add_px(&mut own.padding.top, (zoom + 1e-4).floor());
            add_px(&mut own.padding.bottom, (zoom + 1e-4).floor());
        }
        let arrow = own.appearance == Appearance::Auto;
        let width = text_width + if arrow { arrow_box(own.zoom) } else { 0.0 };
        // As high as the font's rounded ascent and descent, without its line gap
        // (`MenuListIntrinsicBlockSize`).
        let height = arrow.then(|| font_height(self.fonts, &own)).flatten();
        set_content_size(&mut own, None, height);
        own.display = flex_display(own.display);
        own.align_items = ItemAlignment::Center;
        // Chrome's theme sets a menu list's line height to `normal`; without the theme
        // (`appearance: none`, as Pico CSS styles selects) the text keeps the select's line
        // height (`LayoutTheme::AdjustMenuListStyle`).
        if arrow {
            own.line_height = LineHeight::Normal;
        }
        let mut inner = anonymous_style(&own);
        inner.white_space = WhiteSpace::PRE;
        inner.overflow_x = Overflow::Clip;
        inner.overflow_y = Overflow::Clip;
        inner.flex_grow = 1.0;
        inner.flex_basis = Size::Length(LengthPercentage::ZERO);
        inner.min_width = Size::Length(LengthPercentage::ZERO);
        // Auto margins center the text as Chrome does, but only where it fits: taller text
        // starts at the top (Blink's `MenuListSelectType::AdjustInnerStyle`).
        inner.margin.top = LengthPercentageAuto::Auto;
        inner.margin.bottom = LengthPercentageAuto::Auto;
        let text_style = Arc::new(inner.inherit());
        let line = tree.push(LayoutBox {
            style: Arc::new(inner),
            node: None,
            generated: true,
            kind: BoxKind::Inline {
                items: text_items(label, &text_style),
            },
        });
        let mut children = vec![line];
        if arrow {
            let icon = self.icon(tree, &own, select_arrow(own.color, own.zoom));
            // Chrome 157 draws the arrow at the end of the padding box, 1px (zoomed) in: the
            // padding the author gives the end lies between the text and the arrow.
            let rtl = own.is_rtl();
            let internal = arrow_padding(own.zoom);
            let end = if rtl {
                &own.padding.left
            } else {
                &own.padding.right
            };
            if let Some(gap) = end.fixed().map(|end| end - internal).filter(|gap| *gap > 0.0) {
                let style = Arc::make_mut(&mut tree.box_mut(icon).style).values_mut();
                let (before, after) = if rtl {
                    (&mut style.margin.right, &mut style.margin.left)
                } else {
                    (&mut style.margin.left, &mut style.margin.right)
                };
                *before = LengthPercentageAuto::Length(LengthPercentage::Px(gap));
                *after = LengthPercentageAuto::Length(LengthPercentage::Px(-gap));
            }
            children.push(icon);
        }
        let id = tree.push(LayoutBox {
            style: Arc::new(own),
            node: Some(element),
            generated: false,
            kind: BoxKind::Flex { children },
        });
        intrinsic_width(tree, id, width);
        id
    }

    /// A `<select>` that shows `size` options at once: a box with a line for each option, the
    /// selected ones on a gray ground. Chrome prints no scroll bar on it, nor keeps room for one.
    fn list_box(
        &self,
        tree: &mut BoxTree,
        element: NodeId,
        mut own: ComputedStyle,
        options: &[(String, bool)],
        size: u32,
        text_width: f32,
    ) -> BoxId {
        // An option is a line high, at least 1.2em, with a pixel of padding below.
        let item_height = line_height(
            self.fonts,
            &ComputedStyle::from(ComputedValues {
                line_height: LineHeight::Normal,
                ..ComputedValues::from(own.clone())
            }),
        )
        .max(1.2 * own.font_size)
            + own.zoom;
        if own.vertical_align == fagus_style::values::VerticalAlign::Baseline {
            own.vertical_align = fagus_style::values::VerticalAlign::TextBottom;
        }
        // In vertical writing modes the options stack across, as in Chrome.
        let vertical = own.writing_mode.is_vertical();
        let block = Some(item_height * size as f32);
        if vertical {
            set_content_size(&mut own, block, None);
        } else {
            set_content_size(&mut own, None, block);
        }
        own.overflow_x = Overflow::Clip;
        own.overflow_y = Overflow::Clip;
        own.line_height = LineHeight::Normal;
        // Chrome scrolls the first selected option into view (the active selection end, which
        // the first option selected while parsing sets): where it is below the scrollport, the
        // padding box, to its bottom edge. The end padding scrolls past with the options.
        let visible = if vertical {
            fixed_size(&own).0
        } else {
            fixed_size(&own).1
        }
        .unwrap_or(item_height * size as f32);
        let end_padding =
            match own.writing_mode {
                fagus_style::values::WritingMode::HorizontalTb => &own.padding.bottom,
                fagus_style::values::WritingMode::VerticalRl
                | fagus_style::values::WritingMode::SidewaysRl => &own.padding.left,
                fagus_style::values::WritingMode::VerticalLr
                | fagus_style::values::WritingMode::SidewaysLr => &own.padding.right,
            }
            .fixed()
            .unwrap_or(0.0);
        let scroll = list_scroll(options, item_height, visible, end_padding);
        let mut children = Vec::new();
        for (index, (label, selected)) in options.iter().enumerate() {
            let mut option = anonymous_style(&own);
            if index == 0 && scroll > 0.0 {
                // On the block-start side of the list.
                let margin = LengthPercentageAuto::Length(LengthPercentage::Px(-scroll));
                match own.writing_mode {
                    fagus_style::values::WritingMode::HorizontalTb => option.margin.top = margin,
                    fagus_style::values::WritingMode::VerticalRl
                    | fagus_style::values::WritingMode::SidewaysRl => option.margin.right = margin,
                    fagus_style::values::WritingMode::VerticalLr
                    | fagus_style::values::WritingMode::SidewaysLr => option.margin.left = margin,
                }
            }
            option.white_space = WhiteSpace::PRE;
            // Padding at the ends of its line and after it, on the sides they are on in the
            // list's writing mode.
            let (inline, after) = (
                LengthPercentage::Px(2.0 * own.zoom),
                LengthPercentage::Px(own.zoom),
            );
            let least = Size::Length(LengthPercentage::Px(1.2 * own.font_size));
            match own.writing_mode {
                fagus_style::values::WritingMode::HorizontalTb => {
                    option.padding.left = inline.clone();
                    option.padding.right = inline;
                    option.padding.bottom = after;
                    option.min_height = least;
                }
                mode => {
                    option.padding.top = inline.clone();
                    option.padding.bottom = inline;
                    if matches!(
                        mode,
                        fagus_style::values::WritingMode::VerticalRl
                            | fagus_style::values::WritingMode::SidewaysRl
                    ) {
                        option.padding.left = after;
                    } else {
                        option.padding.right = after;
                    }
                    option.min_width = least;
                }
            }
            if *selected {
                // Lighter than the list's ground in the dark color scheme (Chrome 157, measured).
                option.background_color = if own.dark { gray(0x54) } else { gray(0xce) };
            }
            let text = Arc::new(option.inherit());
            children.push(tree.push(LayoutBox {
                style: Arc::new(option),
                node: None,
                generated: true,
                kind: BoxKind::Inline {
                    items: text_items(label.clone(), &text),
                },
            }));
        }
        let id = tree.push(LayoutBox {
            style: Arc::new(own),
            node: Some(element),
            generated: false,
            kind: BoxKind::Block { children },
        });
        let zoom = tree.get(id).style.zoom;
        intrinsic_width(tree, id, text_width + 4.0 * zoom);
        id
    }
}

/// The pixels of a length that does not depend on a percentage, else zero.
fn fixed_px(length: &LengthPercentage) -> f32 {
    length.fixed().unwrap_or(0.0)
}

/// Adds `extra` pixels to a size that is a length of its own (not a percentage or a keyword).
fn grow_size(size: &mut Size, extra: f32) {
    if let Size::Length(length) = size
        && let Some(px) = length.fixed()
    {
        *length = LengthPercentage::Px(px + extra);
    }
}

/// The options of a `<select>` without `multiple`, of which only the last with `selected` is
/// selected (HTML §4.10.7: selecting an option deselects the others).
fn only_last_selected(mut options: Vec<(String, bool)>) -> Vec<(String, bool)> {
    if let Some(last) = options.iter().rposition(|(_, selected)| *selected) {
        for (index, (_, selected)) in options.iter_mut().enumerate() {
            *selected = index == last;
        }
    }
    options
}

/// How far a list box of options `item` high is scrolled to show its first selected option,
/// `visible` being its content height and `end_padding` its padding after the options: so far
/// that the option ends at the end of the padding box, where it would end below it.
fn list_scroll(options: &[(String, bool)], item: f32, visible: f32, end_padding: f32) -> f32 {
    options
        .iter()
        .position(|(_, selected)| *selected)
        .map_or(0.0, |first| {
            (item * (first + 1) as f32 - visible - end_padding).max(0.0)
        })
}

/// Makes `width` the content width of a field (a text or date field, a select) whose `width`
/// is auto (in vertical writing modes the length of its lines, its height): its min-content and
/// max-content width, which it fits in block flow, while it stretches in flex and grid layout as
/// Chrome stretches it (`AutoWidthShouldFitContent`).
fn intrinsic_width(tree: &mut BoxTree, id: BoxId, width: f32) {
    tree.control_widths.insert(id, width);
    tree.shrinks_to_fit.insert(id);
}

/// The room a `<textarea>` keeps for scroll bars beside and below its text
/// (`TextAreaIntrinsicInlineSize`, `TextAreaIntrinsicBlockSize`): beside it unless
/// `overflow-y` is `hidden` or `clip` (`visible` is `auto` in a text area), below it where
/// `overflow-x` is `scroll`.
fn text_area_scrollbars(style: &ComputedStyle) -> (f32, f32) {
    use fagus_style::values::Scrollbar;
    let beside = match style.overflow_y {
        Overflow::Visible => true,
        Overflow::Clip => false,
        Overflow::Hidden => style.scrollbar_y != Scrollbar::None,
    };
    let below = style.overflow_x == Overflow::Hidden && style.scrollbar_x == Scrollbar::Scroll;
    (
        if beside { SCROLLBAR } else { 0.0 },
        if below { SCROLLBAR } else { 0.0 },
    )
}

/// The style of the box with the line of a text field: one line, cut off at its edges, taking
/// the room of the field.
fn field_line_style(own: &ComputedStyle) -> ComputedStyle {
    let mut inner = anonymous_style(own);
    inner.white_space = WhiteSpace::PRE;
    inner.overflow_x = Overflow::Clip;
    inner.overflow_y = Overflow::Clip;
    inner.flex_grow = 1.0;
    inner.flex_basis = Size::Length(LengthPercentage::ZERO);
    inner.min_width = Size::Length(LengthPercentage::ZERO);
    inner
}

/// The inline items of a control's text: the text, or a line break that gives an empty control
/// the height and baseline of a line.
fn text_items(text: String, style: &Arc<ComputedStyle>) -> Vec<InlineItem> {
    if text.is_empty() {
        vec![InlineItem::LineBreak {
            style: Arc::clone(style),
        }]
    } else {
        super::process_white_space(vec![InlineItem::Text {
            text,
            style: Arc::clone(style),
        }])
    }
}

/// The display of a control laid out as a flex box, inline or block as it is.
fn flex_display(display: Display) -> Display {
    match display {
        Display::Normal { outside, .. } => Display::Normal {
            outside,
            inside: DisplayInside::Flex,
            list_item: false,
        },
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_width_follows_chrome() {
        // Liberation Sans (Arial) at 13.33px on Linux: 7.73px a character (8 rounded), the box
        // around its glyphs 24.61px (25).
        let arial = CharWidths {
            average: 8.0,
            max: 25.0,
        };
        assert!((field_width(arial, 20) - 177.0).abs() < 1e-3);
        assert!((field_width(arial, 1) - 25.0).abs() < 1e-3);
        // DejaVu Sans Mono: 8.03px a character, 17.02px (17) for its glyphs.
        let mono = CharWidths {
            average: 8.027_344,
            max: 17.0,
        };
        assert!((field_width(mono, 20) - 170.0).abs() < 1e-3);
    }

    #[test]
    fn checked_checkbox_has_accent_and_check() {
        let mark = check_mark(13.0, 13.0, false, true, ACCENT, (false, false));
        assert!(mark.contains("rgb(0,117,255)"));
        assert!(mark.contains(r#"stroke="rgb(255,255,255)""#));
        let disabled = check_mark(13.0, 13.0, false, true, ACCENT, (true, false));
        assert!(!disabled.contains("rgb(0,117,255)"));
        assert!(disabled.contains("rgb(169,169,169)"));
        let unchecked = check_mark(13.0, 13.0, true, false, ACCENT, (false, false));
        assert!(unchecked.contains("rgb(118,118,118)"));
        assert!(!unchecked.contains("rgb(0,117,255)"));
    }

    #[test]
    fn dark_controls_take_the_dark_theme() {
        // A dark ground with a lighter border; checked, the dark accent with a dark check.
        let unchecked = check_mark(13.0, 13.0, false, false, DARK_ACCENT, (false, true));
        assert!(unchecked.contains("rgb(59,59,59)") && unchecked.contains("rgb(133,133,133)"));
        let checked = check_mark(13.0, 13.0, false, true, DARK_ACCENT, (false, true));
        assert!(checked.contains("rgb(153,200,255)") && checked.contains(r#"stroke="rgb(59,59,59)""#));
        let disabled = check_mark(13.0, 13.0, true, false, DARK_ACCENT, (true, true));
        assert!(disabled.contains("rgb(98,98,98)") && !disabled.contains("rgb(169,169,169)"));
        let bar = progress_bar(160.0, 16.0, Some(0.4), DARK_ACCENT, true);
        assert!(bar.contains(r#"fill="rgb(59,59,59)""#) && bar.contains("rgb(133,133,133)"));
        let track = slider(
            129.0,
            16.0,
            [0.0, 0.0, 129.0, 16.0],
            0.5,
            DARK_ACCENT,
            (true, true),
            true,
        );
        assert!(track.contains("rgb(54,54,54)") && track.contains("rgb(117,117,117)"));
    }

    #[test]
    fn draws_the_styled_track_and_thumb_of_a_slider() {
        // Bootstrap's `.form-range`: a track 8px high with round ends across a box 24px high,
        // a thumb of 16px whose top margin of -4px centers it on the track, at a quarter.
        let part = |width, height, radius, margin_top| SliderPart {
            width,
            height: Some(height),
            color: Rgba::from_u8(0x0d, 0x6e, 0xfd, 1.0),
            radius,
            margin_top,
        };
        let parts = SliderParts {
            track: Some(part(None, 8.0, 16.0, 0.0)),
            thumb: Some(part(Some(16.0), 16.0, 16.0, -4.0)),
        };
        let body = styled_slider([0.0, 0.0, 216.0, 24.0], 0.25, ACCENT, parts);
        assert!(
            body.contains(r#"<rect x="0" y="8" width="216" height="8" rx="4""#),
            "{body}"
        );
        assert!(
            body.contains(r#"<rect x="50" y="4" width="16" height="16" rx="8""#),
            "{body}"
        );
        // A thumb with its native look is a circle in the middle of the track.
        let native = styled_slider(
            [0.0, 0.0, 216.0, 24.0],
            0.0,
            ACCENT,
            SliderParts { thumb: None, ..parts },
        );
        assert!(native.contains(r#"<circle cx="8" cy="12""#), "{native}");
    }

    #[test]
    fn moves_the_slider_thumb_in_the_content_box_and_the_track_across_the_box() {
        // A slider 149px wide with 10px of padding at either side: the track takes the whole
        // box, the thumb (16px) moves from the start of the content box to its end.
        let at = |fraction: f32| {
            slider(
                149.0,
                16.0,
                [10.0, 0.0, 129.0, 16.0],
                fraction,
                ACCENT,
                (false, false),
                true,
            )
        };
        assert!(at(0.0).contains(r#"<circle cx="18""#));
        assert!(at(1.0).contains(r#"<circle cx="131""#));
        assert!(at(0.5).contains(r#"width="147""#));
        assert!(
            Mark::Slider {
                fraction: 0.5,
                accent: ACCENT,
                disabled: false,
                track: true,
                dark: false,
                parts: SliderParts::default(),
            }
            .covers_border_box()
        );
        let mark = Mark::Progress {
            fraction: Some(0.5),
            accent: ACCENT,
            dark: false,
        };
        assert!(!mark.covers_border_box());
        // Drawn at the size the box gets, without a ratio that would tie its height to it.
        let image = mark.image(300.0, 16.0, [0.0, 0.0, 300.0, 16.0], 1.0);
        assert_eq!((image.width, image.height, image.ratio), (300.0, 16.0, None));
    }

    #[test]
    fn native_frame_keeps_the_size() {
        let mut style = ComputedStyle::from(ComputedValues {
            border_width: fagus_style::values::Sides::all(2.0),
            border_style: fagus_style::values::Sides::all(BorderStyle::Inset),
            padding: fagus_style::values::Sides::all(LengthPercentage::Px(1.0)),
            ..ComputedValues::default()
        });
        native_frame(&mut style, Frame::Field, false);
        assert_eq!(style.border_width.top, 1.0);
        assert_eq!(style.padding.left, LengthPercentage::Px(2.0));
        assert_eq!(style.border_style.left, BorderStyle::Solid);
    }

    #[test]
    fn strokes_native_frames_as_chrome_prints_them() {
        let framed = |frame, zoom: f32| {
            let mut style = ComputedStyle::from(ComputedValues {
                border_width: fagus_style::values::Sides::all((2.0 * zoom).floor().max(1.0)),
                border_style: fagus_style::values::Sides::all(BorderStyle::Inset),
                padding: fagus_style::values::Sides::all(LengthPercentage::Px(zoom)),
                zoom,
                ..ComputedValues::default()
            });
            native_frame(&mut style, frame, false);
            style
        };
        // A field's 1px stroke is a hairline at three quarters of its alpha, a button's a stroke.
        let field = framed(Frame::Field, 1.0);
        assert_eq!(field.native_frame, Some(0.0));
        assert!((field.border_color.top.expect("a color").a - 0.75).abs() < 1e-6);
        let button = framed(Frame::Button, 1.0);
        assert_eq!(button.native_frame, Some(1.0));
        assert_eq!(button.border_color.top.expect("a color").a, 1.0);
        assert_eq!(framed(Frame::ListBox, 1.0).native_frame, Some(1.0));
        // Zoomed, the stroke is as wide as whole pixels of the zoom, rounded by 2px zoomed along
        // its middle: a field's too, which is no hairline then.
        let zoomed = framed(Frame::Field, 2.0);
        assert_eq!(zoomed.native_frame, Some(2.0));
        assert_eq!(zoomed.border_width.top, 2.0);
        assert_eq!(zoomed.padding.left, LengthPercentage::Px(4.0));
        assert_eq!(zoomed.border_radius.top_left.0, LengthPercentage::Px(5.0));
        assert_eq!(framed(Frame::Field, 1.5).native_frame, Some(0.0));
        assert_eq!(framed(Frame::Button, 2.5).native_frame, Some(2.0));
    }

    #[test]
    fn places_the_select_arrow_as_chrome_at_any_zoom() {
        // The arrow's left edge in its box, its width and its height, from its path.
        let arrow = |zoom: f32| {
            let image = select_arrow(gray(0), zoom);
            let svg = String::from_utf8(image.data.to_vec()).expect("text");
            let path = svg.split("d=\"M").nth(1).expect("a path");
            let numbers: Vec<f32> = path
                .split(['"', ' ', 'L'])
                .take_while(|part| !part.contains('='))
                .filter_map(|part| part.parse().ok())
                .collect();
            (
                image.width,
                numbers[0],
                numbers[4] - numbers[0],
                numbers[3] - numbers[1],
            )
        };
        // Measured in Chrome 157 on Linux: 8px wide in a room of 15px, 4px in; zoomed, 16px in
        // 30px, 9px in; the stroke stays 2px wide.
        assert_eq!(arrow(1.0), (15.0, 4.0, 8.0, 4.0));
        assert_eq!(arrow(2.0), (30.0, 9.0, 16.0, 8.0));
        assert_eq!(arrow(3.0), (45.0, 13.0, 24.0, 12.0));
        assert_eq!(arrow_padding(1.0), 1.0);
        assert_eq!(arrow_padding(1.5), 1.5);
    }
}

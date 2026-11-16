//! What an image that does not load shows instead (HTML, §15.4.1), as Chrome renders an `<img>`
//! or `<input type=image>` whose image is missing (Blink's `HTMLImageFallbackHelper`).
//!
//! The element becomes a box of its own, laid out like an inline-block: its text (the `alt`
//! attribute, else its `title`; for `input`, also its `value`, else "Submit") after a broken
//! image icon 16 pixels square that floats at its start. An element with a size (both `width`
//! and `height`) and no `alt` text, or with any text in quirks mode, keeps that size like the
//! image would: a box with a silver border and padding of one pixel, the icon and the text in
//! it, both left out where it is less than 18 pixels wide or high. An image whose `alt` is empty
//! represents nothing and shows nothing.
//!
//! The behavior follows Chromium's `html_image_fallback_helper.cc`:
//! Copyright 2014 The Chromium Authors. Use of this source code is governed by a BSD-style
//! license that can be found in the CHROMIUM-LICENSE file.
//! The icon is drawn with shapes of its own that resemble Chrome's: a page with a folded corner,
//! a sky with a cloud and a green hill, torn across its lower right corner.

use std::sync::Arc;

use fagus_dom::{ElementData, NodeId};
use fagus_style::ComputedStyle;
use fagus_style::values::{
    BorderStyle, BoxSizing, Direction, Display, DisplayInside, DisplayOutside, Float, LengthPercentage,
    LengthPercentageAuto, Overflow, Rgba, Sides, Size,
};

use super::forms::Control;
use super::{BoxId, BoxKind, BoxTree, Builder, InlineItem, LayoutBox, anonymous_style, process_white_space};
use crate::image::{Image, ImageFormat};

/// The side of the broken image icon, in CSS pixels.
const ICON: f32 = 16.0;

impl Builder<'_> {
    /// The box of an `<img>` or `<input type=image>` whose image does not load, atomic inline
    /// unless the element is block-level; `None` for other elements and images that load.
    pub(super) fn image_fallback(
        &self,
        tree: &mut BoxTree,
        element: NodeId,
        data: &ElementData,
        style: &Arc<ComputedStyle>,
    ) -> Option<BoxId> {
        let input = Control::of(data) == Some(Control::Image);
        if !(input || data.is("img") && !data.is_svg("svg")) {
            return None;
        }
        let src = self.images.source(element, data).map_or("", |(src, _)| src);
        if !src.is_empty() && self.images.get(src).is_some() || self.content_image(element).is_some() {
            return None;
        }
        let alt_attribute = data.attribute("alt");
        let text: Option<String> = if input {
            Some(
                alt_attribute
                    .or_else(|| data.attribute("title"))
                    .or_else(|| data.attribute("value"))
                    .map_or_else(|| self.ui_locale(style).submit.to_owned(), str::to_owned),
            )
        } else {
            alt_attribute
                .or_else(|| data.attribute("title"))
                .map(str::to_owned)
        };
        // Without a source and without text, an image does not fail to load: it is an empty
        // replaced box, as in Chrome, unless it has a size, which shows the box's border alone
        // (Chrome 157, measured).
        let no_source = !input
            && data.attribute("src").is_none()
            && data.attribute("srcset").is_none()
            && text.as_deref().is_none_or(str::is_empty);
        let quirks = self.document.quirks_mode();
        let has_size =
            |style: &ComputedStyle| !matches!(style.width, Size::Auto) && !matches!(style.height, Size::Auto);
        // `width` and `height` are presentational hints of the style (with the ratio they give).
        let mut host = (**style).clone();
        // In quirks mode a size given on one side only is given on both.
        if quirks {
            match (&host.width, &host.height) {
                (Size::Auto, Size::Auto) => {}
                (width, Size::Auto) => host.height = width.clone(),
                (Size::Auto, height) => host.width = height.clone(),
                _ => {}
            }
        }
        let sized_by_ratio = host.aspect_ratio.is_some()
            && !(matches!(host.width, Size::Auto) && matches!(host.height, Size::Auto));
        let replaced =
            (has_size(&host) || sized_by_ratio) && (quirks || alt_attribute.is_none_or(str::is_empty));
        // (Chrome frames one sized by its ratio alone too, all over its box, which the frame
        // below, as high as its content, would not match: it stays an empty box.)
        if no_source && (!replaced || matches!(host.height, Size::Auto)) {
            return None;
        }
        let inline = !host.display.is_block_level();
        // A box sized by its ratio alone keeps the height of the ratio, while the box inside it
        // takes the height of its content, which the baseline lies under (that of a scroll
        // container), as in Chrome.
        let ratio_only = replaced && matches!(host.height, Size::Auto);
        // An image that represents nothing is an empty box.
        let nothing = !replaced
            && match text.as_deref() {
                Some(text) => text.is_empty(),
                None => src.is_empty(),
            };
        if !replaced {
            // An inline image (not an inline-block) leaves its size.
            if host.display == Display::INLINE {
                host.width = Size::Auto;
                host.height = Size::Auto;
                host.aspect_ratio = None;
            }
        }
        host.display = if inline {
            Display::Normal {
                outside: DisplayOutside::Inline,
                inside: DisplayInside::FlowRoot,
                list_item: false,
            }
        } else {
            host.display.blockify()
        };
        let small = replaced && smaller_than_icon(&host);
        let icon = (!small && !nothing && !no_source).then(|| self.broken_image_icon(tree, &host));
        let mut text_style = host.inherit();
        // The text of an image shown as text is underlined in a link: the decorations of the
        // element's ancestors reach it, as they do not reach a box that keeps the image's size.
        if !replaced {
            text_style.decorations = Arc::clone(&host.decorations);
        }
        let text_style = Arc::new(text_style);
        let mut items: Vec<InlineItem> = icon.into_iter().map(InlineItem::Float).collect();
        if let Some(text) = text.filter(|text| !text.is_empty()) {
            items.push(InlineItem::Text {
                text,
                style: Arc::clone(&text_style),
            });
        }
        let items = process_white_space(items);
        let without_lines = !items.iter().any(|item| matches!(item, InlineItem::Text { .. }));
        let kind = if replaced {
            // A box with a style of its own, like a pseudo-element (Chrome's is in a shadow
            // tree).
            let container = tree.push(LayoutBox {
                style: Arc::new(container_style(&host, !small)),
                node: None,
                generated: true,
                kind: BoxKind::Inline { items },
            });
            BoxKind::Block {
                children: vec![container],
            }
        } else {
            BoxKind::Inline { items }
        };
        // Without lines (only the icon, or an empty box) the baseline is the bottom of the
        // content box; that of a box that keeps the image's size is the bottom of its margin
        // box, as of a replaced element (Chrome 157, measured).
        let raise = if replaced {
            -host.margin.bottom.resolve_or(0.0, 0.0)
        } else {
            host.border_width.bottom + host.padding.bottom.fixed().unwrap_or(0.0)
        };
        let id = tree.push(LayoutBox {
            style: Arc::new(host),
            node: Some(element),
            generated: false,
            kind,
        });
        if (replaced && !ratio_only) || (!replaced && without_lines) {
            tree.border_box_baselines.insert(id, raise);
        }
        Some(id)
    }

    /// The broken image icon in a box with `host`'s style: a float at the start of its line.
    fn broken_image_icon(&self, tree: &mut BoxTree, host: &ComputedStyle) -> BoxId {
        let mut style = anonymous_style(host);
        style.float = if host.direction == Direction::Rtl {
            Float::Right
        } else {
            Float::Left
        };
        style.width = Size::Length(LengthPercentage::Px(ICON));
        style.height = Size::Length(LengthPercentage::Px(ICON));
        style.margin = Sides::all(LengthPercentageAuto::Length(LengthPercentage::Px(0.0)));
        tree.push(LayoutBox {
            style: Arc::new(style),
            node: None,
            generated: false,
            kind: BoxKind::Replaced {
                image: Some(icon_image()),
                attributes: (Some(ICON), Some(ICON)),
            },
        })
    }
}

/// Whether a fixed width or height of an image keeps the icon out: less than its 16 pixels and
/// the 2 of the border and padding before it.
fn smaller_than_icon(style: &ComputedStyle) -> bool {
    let fixed = |size: &Size| match size {
        Size::Length(LengthPercentage::Px(px)) => Some(*px),
        _ => None,
    };
    [fixed(&style.width), fixed(&style.height)]
        .into_iter()
        .flatten()
        .any(|px| px < ICON + 2.0)
}

/// The style of the box inside an image that keeps its size: a formatting context of that size,
/// cut off at its edges, with a silver border and padding of a pixel if `framed`.
fn container_style(host: &ComputedStyle, framed: bool) -> ComputedStyle {
    let mut style = anonymous_style(host);
    style.display = Display::Normal {
        outside: DisplayOutside::Block,
        inside: DisplayInside::FlowRoot,
        list_item: false,
    };
    style.overflow_x = Overflow::Hidden;
    style.overflow_y = Overflow::Hidden;
    style.width = host.width.clone();
    style.height = host.height.clone();
    style.max_width = host.max_width.clone();
    style.max_height = host.max_height.clone();
    if framed {
        let silver = Rgba {
            r: 192.0 / 255.0,
            g: 192.0 / 255.0,
            b: 192.0 / 255.0,
            a: 1.0,
        };
        style.border_width = Sides::all(1.0);
        style.border_style = Sides::all(BorderStyle::Solid);
        style.border_color = Sides::all(Some(silver));
        style.padding = Sides::all(LengthPercentage::Px(1.0));
        style.box_sizing = BoxSizing::BorderBox;
    }
    style
}

/// The broken image icon, 16 pixels square: a page 14 units wide and 16 high, stretched to the
/// square as Chrome draws its icon, its upper right corner folded, a pale blue sky with a white
/// cloud over a green hill, torn from the bottom to the right edge.
fn icon_image() -> Arc<Image> {
    let data = concat!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 14 16" preserveAspectRatio="none">"#,
        r#"<defs><linearGradient id="sky" x1="0" y1="0" x2="0" y2="1">"#,
        r#"<stop offset="0" stop-color="rgb(241,246,254)"/><stop offset="1" stop-color="rgb(190,208,240)"/></linearGradient>"#,
        r#"<linearGradient id="edge" x1="0" y1="0" x2="0" y2="1">"#,
        r#"<stop offset="0" stop-color="rgb(163,163,163)"/><stop offset="1" stop-color="rgb(131,131,132)"/></linearGradient>"#,
        r#"<clipPath id="page"><path d="M0 0H10L14 4V8L6 16H0Z M14 11V16H9Z"/></clipPath></defs>"#,
        r#"<g clip-path="url(#page)"><rect width="14" height="16" fill="url(#sky)"/>"#,
        r#"<path d="M2 5.2C2 4.4 2.7 4 3.3 4.1C3.6 3.3 4.2 2.9 4.9 3C5.6 3.1 6 3.7 6.1 4.3C6.7 4.4 7 4.9 6.9 5.4"#,
        r#"C6.9 5.8 6.6 6 6.2 6H2.7C2.3 6 2 5.7 2 5.2Z" fill="rgb(255,255,255)"/>"#,
        r#"<path d="M0.5 16C1.4 12.4 3.6 9.6 6.8 9.6C9.6 9.6 11.6 11.6 13.5 14.6V16Z" fill="rgb(88,174,57)"/>"#,
        r#"<path d="M0.5 16V0.5H10L13.5 4V8.5 M13.5 11.5V15.5H9.5 M5.5 15.5H0.5" fill="none" stroke="url(#edge)"/></g>"#,
        r#"<path d="M9.5 0.5V4.5H13.5Z" fill="rgb(255,255,255)" stroke="rgb(150,150,150)" stroke-linejoin="bevel"/>"#,
        r#"</svg>"#
    );
    Arc::new(Image {
        data: Arc::new(data.as_bytes().to_vec()),
        format: ImageFormat::Svg,
        width: ICON,
        height: ICON,
        natural: (true, true),
        ratio: Some(1.0),
        inline: false,
        orientation: 1,
        filters: Arc::from([]),
        pad: 0,
    })
}

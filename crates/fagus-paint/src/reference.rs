//! SVG elements that `clip-path`, `mask` and `filter` name by URL (`url(#id)`), applied to a
//! box: the user space of a `<clipPath>` or `<mask>` starts at the top left corner of the
//! border box, and its bounding box is the border box, as in Chrome.

use std::borrow::Cow;
use std::fmt::Write as _;
use std::sync::Arc;

use fagus_layout::{Image, ImageFormat, Images, PathSegment, SvgReference};
use fagus_style::values::{Filter, Rgba};

use crate::{ClipGeometry, Item, MaskPaint, PixelFilterStep, RectF};

/// The id that a URL of the document names, without its `#`.
fn fragment(url: &str) -> Option<&str> {
    url.strip_prefix('#')
}

/// How the `<clipPath>` at `url` clips a box with `border_box`: `None` for an element that is
/// not found, which clips nothing.
pub(crate) fn clip(url: &str, border_box: RectF, images: &Images, zoom: f32) -> Option<Item> {
    match &**images.reference(url)? {
        SvgReference::Clip(clip) => {
            // User units are CSS pixels, zoomed with the element (Chrome 157, measured).
            let (scale_x, scale_y) = if clip.bounding_box {
                (border_box.width, border_box.height)
            } else if zoom > 0.0 {
                (zoom, zoom)
            } else {
                (1.0, 1.0)
            };
            let point = |x: f32, y: f32| (border_box.x + x * scale_x, border_box.y + y * scale_y);
            let segments = clip
                .segments
                .iter()
                .map(|segment| match *segment {
                    PathSegment::Move(x, y) => {
                        let (x, y) = point(x, y);
                        PathSegment::Move(x, y)
                    }
                    PathSegment::Line(x, y) => {
                        let (x, y) = point(x, y);
                        PathSegment::Line(x, y)
                    }
                    PathSegment::Cubic([x1, y1, x2, y2, x, y]) => {
                        let (x1, y1) = point(x1, y1);
                        let (x2, y2) = point(x2, y2);
                        let (x, y) = point(x, y);
                        PathSegment::Cubic([x1, y1, x2, y2, x, y])
                    }
                    PathSegment::Close => PathSegment::Close,
                })
                .collect();
            Some(Item::PushClipShape(ClipGeometry::Path {
                segments,
                even_odd: clip.even_odd,
            }))
        }
        SvgReference::Markup { markup, clip: true } => {
            let image = coverage(markup, fragment(url)?, true, border_box, None, (0.0, zoom))?;
            Some(Item::PushMask(Arc::new(MaskPaint {
                items: vec![image],
                luminance: false,
            })))
        }
        _ => None,
    }
}

/// What the `<mask>` at `url` draws over a box with `border_box`, as the alpha of a mask
/// layer; `None` for an element that is not found. `luminance` masks by the luminance (or the
/// alpha) of what it draws whatever its `mask-type` says.
/// `reach` is how far the box draws outside its border box (its shadows), where the mask
/// applies too.
pub(crate) fn mask(
    url: &str,
    border_box: RectF,
    images: &Images,
    luminance: Option<bool>,
    reach: f32,
) -> Option<Item> {
    match &**images.reference(url)? {
        SvgReference::Markup { markup, clip: false } => {
            let id = url.rsplit_once('#').map_or(url, |(_, id)| id);
            coverage(markup, id, false, border_box, luminance, (reach, 1.0))
        }
        _ => None,
    }
}

/// Whether `url` names a `<mask>` that masks a box.
pub(crate) fn is_mask(url: &str, images: &Images) -> bool {
    images
        .reference(url)
        .is_some_and(|reference| matches!(&**reference, SvgReference::Markup { clip: false, .. }))
}

/// An SVG image over `border_box` of a white rectangle that the element `id` of `markup` clips
/// or masks. The element is one of the document, so its text takes the document's fonts, as
/// that of an inline SVG does.
fn coverage(
    markup: &str,
    id: &str,
    clip: bool,
    border_box: RectF,
    luminance: Option<bool>,
    (reach, zoom): (f32, f32),
) -> Option<Item> {
    let (width, height) = (border_box.width, border_box.height);
    if !(width > 0.0 && height > 0.0 && width.is_finite() && height.is_finite()) {
        return None;
    }
    let reach = if reach.is_finite() { reach.max(0.0) } else { 0.0 };
    let markup = match luminance {
        Some(luminance) => Cow::Owned(with_mask_type(markup, luminance)),
        None => Cow::Borrowed(markup),
    };
    let document = svg_coverage(&markup, id, clip, (width, height), (reach, zoom));
    let (outer_width, outer_height) = (width + 2.0 * reach, height + 2.0 * reach);
    Some(Item::Image {
        rect: RectF {
            x: border_box.x - reach,
            y: border_box.y - reach,
            width: outer_width,
            height: outer_height,
        },
        image: Arc::new(Image {
            data: Arc::new(document.into_bytes()),
            format: ImageFormat::Svg,
            width: outer_width,
            height: outer_height,
            natural: (true, true),
            ratio: Some(outer_width / outer_height),
            inline: true,
            orientation: 1,
            filters: Arc::from([]),
            pad: 0,
        }),
    })
}

/// `markup`, which starts with a `<mask>` element as Fagus writes it (attributes in double
/// quotes), with the `mask-type` of that element set to `luminance` or `alpha`, over what its
/// attribute and style say.
fn with_mask_type(markup: &str, luminance: bool) -> String {
    let value = if luminance { "luminance" } else { "alpha" };
    let Some(end) = markup.find('>') else {
        return markup.to_owned();
    };
    let mut tag = markup[..end].trim_end_matches('/').to_owned();
    let closed = if markup[..end].ends_with('/') { "/" } else { "" };
    if let Some(at) = tag.find(" mask-type=\"")
        && let Some(length) = tag[at + 12..].find('"')
    {
        tag.replace_range(at..at + 13 + length, "");
    }
    // A declaration at the end of its style wins over those before it.
    match tag
        .find(" style=\"")
        .and_then(|at| Some(at + 8 + tag[at + 8..].find('"')?))
    {
        Some(close) => tag.insert_str(close, &format!(";mask-type:{value}")),
        None => {
            let _ = write!(tag, " mask-type=\"{value}\"");
        }
    }
    format!("{tag}{closed}{}", &markup[end..])
}

/// An SVG document `width` × `height` pixels large that draws a white rectangle over all of it,
/// clipped (`clip`) or masked by the element with the id `id` in `markup`; with a `reach`, it is
/// larger by that much on every side, which the rectangle's stroke covers (so that its bounding
/// box, which the element's units may refer to, stays the box).
/// The document [`coverage`] draws: the box `width` × `height` (and `reach` around it) in user
/// units that are CSS pixels zoomed by `zoom`.
fn svg_coverage(
    markup: &str,
    id: &str,
    clip: bool,
    (width, height): (f32, f32),
    (reach, zoom): (f32, f32),
) -> String {
    let mut escaped = String::with_capacity(id.len());
    for character in id.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '"' => escaped.push_str("&quot;"),
            _ => escaped.push(character),
        }
    }
    let property = if clip { "clip-path" } else { "mask" };
    let (outer_width, outer_height) = (width + 2.0 * reach, height + 2.0 * reach);
    let zoom = if zoom > 0.0 && zoom.is_finite() { zoom } else { 1.0 };
    let user = |value: f32| value / zoom;
    let (start, view_width, view_height) = (user(0.0 - reach) + 0.0, user(outer_width), user(outer_height));
    let stroke = if reach > 0.0 {
        format!(" stroke=\"#fff\" stroke-width=\"{}\"", user(2.0 * reach))
    } else {
        String::new()
    };
    let (box_width, box_height) = (user(width), user(height));
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" \
         width=\"{outer_width}\" height=\"{outer_height}\" viewBox=\"{start} {start} {view_width} {view_height}\"><defs>{markup}</defs>\
         <rect width=\"{box_width}\" height=\"{box_height}\" fill=\"#fff\"{stroke} {property}=\"url(#{escaped})\"/></svg>"
    )
}

/// `filter` with its SVG filters replaced by the functions they resolve to; one that is not
/// found filters nothing, and one that filters pixels stays a URL.
pub(crate) fn filters<'a>(filters: &'a [Filter], images: &Images) -> Cow<'a, [Filter]> {
    if !filters.iter().any(|filter| matches!(filter, Filter::Url(_))) {
        return Cow::Borrowed(filters);
    }
    let mut resolved = Vec::new();
    for filter in filters {
        match filter {
            Filter::Url(url) => match images.reference(url).map(|reference| &**reference) {
                Some(SvgReference::Filter(functions)) => resolved.extend(functions.iter().cloned()),
                Some(SvgReference::PixelFilter { .. }) => resolved.push(filter.clone()),
                _ => {}
            },
            other => resolved.push(other.clone()),
        }
    }
    Cow::Owned(resolved)
}

/// The filters (resolved by [`filters`]) that the pixels of a box in the color `current` go
/// through: SVG filters with their markup, and functions with `currentColor` resolved.
///
/// A shadow in `currentColor` taints what follows it (Filter Effects 1, §16.1, which would let
/// visited links show through), as one before `filters` does (`tainted`): a displacement map
/// after it leaves what it displaces as it is, as in Chrome.
pub(crate) fn pixel_steps(
    filters: &[Filter],
    current: Rgba,
    images: &Images,
    mut tainted: bool,
) -> Vec<PixelFilterStep> {
    filters
        .iter()
        .filter_map(|filter| match filter {
            Filter::Url(url) => match &**images.reference(url)? {
                SvgReference::PixelFilter { markup } => Some(PixelFilterStep::Svg {
                    markup: if tainted && markup.contains("feDisplacementMap") {
                        Arc::from(without_displacement(markup))
                    } else {
                        Arc::clone(markup)
                    },
                    id: Arc::from(fragment(url)?),
                }),
                _ => None,
            },
            Filter::DropShadow { x, y, blur, color } => {
                tainted |= color.is_none();
                Some(PixelFilterStep::Function(Filter::DropShadow {
                    x: *x,
                    y: *y,
                    blur: *blur,
                    color: Some(color.unwrap_or(current)),
                }))
            }
            other => Some(PixelFilterStep::Function(other.clone())),
        })
        .collect()
}

/// The markup of an SVG filter with its displacement maps (`feDisplacementMap`) made offsets of
/// nothing (`feOffset`), which pass their input through.
fn without_displacement(markup: &str) -> String {
    markup
        .replace("<feDisplacementMap", "<feOffset")
        .replace("</feDisplacementMap>", "</feOffset>")
}

#[cfg(test)]
mod tests {
    use super::*;
    use fagus_layout::SvgClip;

    #[test]
    fn displaces_nothing_after_a_shadow_in_the_current_color() {
        let mut images = Images::default();
        let markup = r#"<filter id="d"><feDisplacementMap in="SourceGraphic" in2="SourceGraphic" scale="200"></feDisplacementMap></filter>"#;
        images.insert_reference(
            "#d",
            Arc::new(SvgReference::PixelFilter {
                markup: Arc::from(markup),
            }),
        );
        let shadow = |color: Option<Rgba>| Filter::DropShadow {
            x: 0.0,
            y: 0.0,
            blur: 0.0,
            color,
        };
        let svg = |steps: &[PixelFilterStep]| match steps.last() {
            Some(PixelFilterStep::Svg { markup, .. }) => markup.to_string(),
            _ => panic!("an SVG filter"),
        };
        let url = Filter::Url(Arc::from("#d"));
        let tainted = pixel_steps(&[shadow(None), url.clone()], Rgba::BLACK, &images, false);
        assert!(svg(&tainted).starts_with("<filter id=\"d\"><feOffset in=\"SourceGraphic\""));
        // A shadow of another color, or none before it, taints nothing.
        let clean = pixel_steps(
            &[shadow(Some(Rgba::BLACK)), url.clone()],
            Rgba::BLACK,
            &images,
            false,
        );
        assert_eq!(svg(&clean), markup);
        assert_eq!(
            svg(&pixel_steps(
                std::slice::from_ref(&url),
                Rgba::BLACK,
                &images,
                false
            )),
            markup
        );
        // One before the filters taints them too.
        assert_ne!(svg(&pixel_steps(&[url], Rgba::BLACK, &images, true)), markup);
    }

    #[test]
    fn places_clip_paths_in_the_border_box() {
        let mut images = Images::default();
        let segments = vec![
            PathSegment::Move(0.0, 0.0),
            PathSegment::Line(1.0, 0.0),
            PathSegment::Line(0.5, 1.0),
            PathSegment::Close,
        ];
        images.insert_reference(
            "#t",
            Arc::new(SvgReference::Clip(SvgClip {
                segments: segments.clone(),
                even_odd: false,
                bounding_box: true,
            })),
        );
        images.insert_reference(
            "#u",
            Arc::new(SvgReference::Clip(SvgClip {
                segments,
                even_odd: true,
                bounding_box: false,
            })),
        );
        let border_box = RectF {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
        };
        let Some(Item::PushClipShape(ClipGeometry::Path {
            segments,
            even_odd: false,
        })) = clip("#t", border_box, &images, 1.0)
        else {
            panic!("a clip");
        };
        assert_eq!(segments[2], PathSegment::Line(60.0, 70.0));
        let Some(Item::PushClipShape(ClipGeometry::Path {
            segments,
            even_odd: true,
        })) = clip("#u", border_box, &images, 1.0)
        else {
            panic!("a clip");
        };
        assert_eq!(segments[2], PathSegment::Line(10.5, 21.0));
        // User units are zoomed with the element.
        let Some(Item::PushClipShape(ClipGeometry::Path { segments, .. })) =
            clip("#u", border_box, &images, 2.0)
        else {
            panic!("a clip");
        };
        assert_eq!(segments[2], PathSegment::Line(11.0, 22.0));
        // An element that is not there clips nothing.
        assert_eq!(clip("#missing", border_box, &images, 1.0), None);
    }

    #[test]
    fn draws_masks_over_the_border_box() {
        let mut images = Images::default();
        images.insert_reference(
            "#m",
            Arc::new(SvgReference::Markup {
                markup: Arc::from("<mask id=\"m\"/>"),
                clip: false,
            }),
        );
        let border_box = RectF {
            x: 5.0,
            y: 5.0,
            width: 40.0,
            height: 20.0,
        };
        let Some(Item::Image { rect, image }) = mask("#m", border_box, &images, None, 0.0) else {
            panic!("an image");
        };
        assert_eq!(rect, border_box);
        let svg = String::from_utf8_lossy(&image.data);
        assert!(svg.contains("viewBox=\"0 0 40 20\"") && svg.contains("mask=\"url(#m)\""));
        // Over the box's shadows too, the rectangle's stroke reaching out to them.
        let Some(Item::Image { rect, image }) = mask("#m", border_box, &images, None, 10.0) else {
            panic!("an image");
        };
        assert_eq!((rect.x, rect.width), (-5.0, 60.0));
        let svg = String::from_utf8_lossy(&image.data);
        assert!(svg.contains("viewBox=\"-10 -10 60 40\"") && svg.contains("stroke-width=\"20\""));
        assert!(svg.contains("<rect width=\"40\" height=\"20\""));
        assert!(is_mask("#m", &images) && !is_mask("#n", &images));
        // `mask-mode` decides over the `<mask>`'s own `mask-type`.
        assert_eq!(
            with_mask_type("<mask id=\"m\" mask-type=\"alpha\"><rect/></mask>", true),
            "<mask id=\"m\" mask-type=\"luminance\"><rect/></mask>"
        );
        assert_eq!(
            with_mask_type("<mask style=\"mask-type: luminance\"/>", false),
            "<mask style=\"mask-type: luminance;mask-type:alpha\"/>"
        );
        // Filters that are not found are left out.
        let list = [Filter::Url(Arc::from("#f")), Filter::Blur(2.0)];
        assert_eq!(filters(&list, &images)[..], [Filter::Blur(2.0)]);
    }
}

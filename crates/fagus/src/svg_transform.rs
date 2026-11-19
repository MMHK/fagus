//! The transforms of the elements of inline SVGs as CSS computes them (CSS Transforms 1, §6;
//! SVG 2, §8.5): `transform` (from a style sheet, or the `transform` presentation attribute),
//! `translate`, `rotate` and `scale`, around `transform-origin`, with `transform-origin` and
//! percentages of the box that `transform-box` names. Fagus writes them out as matrices for the
//! SVG renderer, which reads only the attributes.
//!
//! The object and stroke bounding boxes (`fill-box`, `stroke-box`) come from the SVG renderer:
//! the document is read once with the transforms of the elements that need them left out and
//! those elements marked (see [`Marks`]), then written again with their transforms.

use fagus_dom::{Document, NodeId, SvgDeclared};
use fagus_style::values::{ClipPath, ClipShape, LengthPercentage, Size, TransformBox};
use fagus_style::{ComputedStyle, StyledDocument};
use rustc_hash::FxHashMap;

/// The start of the ids that mark the elements whose bounding boxes are measured.
pub(crate) const MARK: &str = "fagus-box-";

/// How the transforms of the elements of an SVG are written.
pub(crate) enum Marks<'a> {
    /// The elements that need their bounding boxes are marked, without their transforms.
    Measure,
    /// All transforms are written, with the bounding boxes measured (by element).
    Boxes(&'a FxHashMap<NodeId, fagus_pdf::svg_boxes::Boxes>),
}

/// The box of an SVG element that its transform refers to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Reference {
    Fill,
    Stroke,
    View,
}

/// What CSS makes of the SVG element `element` (see [`SvgDeclared`]), its `style` declarations
/// aside: its transform where Fagus writes it (see [`rewritten`]), the basic shape that clips it,
/// the mask of its `mask-*` properties (with the images of `images`) and, while measuring, its
/// mark.
fn transform(
    document: &Document,
    styled: &StyledDocument,
    element: NodeId,
    marks: &Marks<'_>,
    images: Option<&fagus_layout::Images>,
) -> SvgDeclared {
    let Some(style) = styled.style(element) else {
        return SvgDeclared::default();
    };
    let transformed = rewritten(document, style, element);
    let clip = clip_shape(document, style, element);
    let transform_box = (transformed && style.has_transform())
        .then(|| reference(document, style, element, style.transform_box));
    let clip_box = clip.map(|clip| reference(document, style, element, clip.svg_reference));
    let masked = masked(document, style, element);
    if let Marks::Measure = marks
        && (masked
            || [transform_box, clip_box]
                .iter()
                .any(|reference| reference.is_some_and(|reference| reference != Reference::View)))
    {
        // Measured without its transform, in the user space that its box is in, and unclipped.
        return SvgDeclared {
            transform: Some(String::new()),
            id: Some(format!("{MARK}{}", element.index())),
            ..SvgDeclared::default()
        };
    }
    // A box that was not measured (that of an element the SVG renderer leaves out, or of a
    // definition that CSS names) leaves the element's own attributes.
    let rect = |reference: Reference| match (reference, marks) {
        (Reference::View, _) => Some(viewport(document, styled, element)),
        (_, Marks::Boxes(boxes)) => boxes.get(&element).map(|(fill, stroke)| {
            if reference == Reference::Fill {
                *fill
            } else {
                *stroke
            }
        }),
        (_, Marks::Measure) => None,
    };
    let transform = if transformed {
        match transform_box {
            None => Some(String::new()),
            Some(reference) => rect(reference).map(|[x, y, width, height]| {
                let matrix = fagus_paint::svg_matrix(style, (x, y, width, height)).unwrap_or([0.0; 6]);
                let values: Vec<String> = matrix
                    .iter()
                    .map(|value| {
                        if value.is_finite() {
                            value.to_string()
                        } else {
                            "0".to_owned()
                        }
                    })
                    .collect();
                format!("matrix({})", values.join(" "))
            }),
        }
    } else {
        None
    };
    let clip = clip.zip(clip_box).and_then(|(clip, reference)| {
        let [x, y, width, height] = rect(reference)?;
        fagus_paint::svg_clip_path(clip, (x, y, width, height))
    });
    let mask = images.filter(|_| masked).and_then(|images| {
        let rect = |reference: Reference| {
            rect(reference).map(|[x, y, width, height]| fagus_paint::RectF { x, y, width, height })
        };
        let boxes = fagus_paint::SvgBoxes {
            fill: rect(Reference::Fill)?,
            stroke: rect(Reference::Stroke)?,
            view: rect(Reference::View)?,
        };
        fagus_paint::svg_mask(
            style,
            boxes,
            images,
            &format!("{}{}", fagus_dom::SVG_MASK, element.index()),
        )
    });
    SvgDeclared {
        transform,
        clip,
        mask,
        ..SvgDeclared::default()
    }
}

/// Whether the `mask-*` properties mask the SVG element `element` (see
/// [`fagus_paint::svg_mask`]): one that masks apply to, not an outer `<svg>` (a CSS box) or a
/// `<foreignObject>` (whose HTML Fagus draws).
fn masked(document: &Document, style: &ComputedStyle, element: NodeId) -> bool {
    style.mask.as_deref().is_some_and(|mask| !mask.is_none())
        && document.element(element).is_some_and(|data| {
            data.name.ns == fagus_dom::ns!(svg)
                && fagus_style::svg_hints::TRANSFORMABLE.contains(&&*data.name.local)
                && !matches!(&*data.name.local, "foreignObject" | "clipPath")
        })
        && !outer(document, element)
}

/// The basic shape of `clip-path` that clips the SVG element `element`, if any: on an element
/// that clip paths apply to, not an outer `<svg>`, which is a CSS box.
fn clip_shape<'s>(document: &Document, style: &'s ComputedStyle, element: NodeId) -> Option<&'s ClipPath> {
    let clip = style
        .clip_path
        .as_deref()
        .filter(|clip| !matches!(clip.shape, ClipShape::Url(_)))?;
    let data = document.element(element)?;
    (data.name.ns == fagus_dom::ns!(svg)
        && fagus_style::svg_hints::TRANSFORMABLE.contains(&&*data.name.local)
        && !outer(document, element))
    .then_some(clip)
}

/// Whether the SVG element `element` needs its bounding box measured for its transform or the
/// basic shape that clips it.
pub(crate) fn measured(document: &Document, styled: &StyledDocument, element: NodeId) -> bool {
    styled.style(element).is_some_and(|style| {
        masked(document, style, element)
            || (style.has_transform()
                && rewritten(document, style, element)
                && reference(document, style, element, style.transform_box) != Reference::View)
            || clip_shape(document, style, element).is_some_and(|clip| {
                reference(document, style, element, clip.svg_reference) != Reference::View
            })
    })
}

/// Whether Fagus writes the transform of the SVG element `element`: an element that a
/// transform applies to (not an outer `<svg>`, which is a CSS box), with a transform or the
/// attributes or declarations of one.
fn rewritten(document: &Document, style: &ComputedStyle, element: NodeId) -> bool {
    let Some(data) = document.element(element) else {
        return false;
    };
    if data.name.ns != fagus_dom::ns!(svg)
        || !fagus_style::svg_hints::TRANSFORMABLE.contains(&&*data.name.local)
        || outer(document, element)
    {
        return false;
    }
    style.has_transform()
        || data.attribute("transform").is_some()
        || data.attribute("transform-origin").is_some()
        || data.attribute("style").is_some_and(|style| {
            ["transform", "translate", "rotate", "scale"]
                .iter()
                .any(|name| style.to_ascii_lowercase().contains(name))
        })
}

/// Whether `element` is an outer `<svg>`: one in HTML (or right in a `<foreignObject>`).
fn outer(document: &Document, element: NodeId) -> bool {
    document
        .parent_element(element)
        .and_then(|parent| document.element(parent))
        .is_none_or(|parent| parent.name.ns != fagus_dom::ns!(svg) || parent.is_svg("foreignObject"))
}

/// The box of `element` that `used` names (for its transform or clip path): `content-box` and
/// `border-box` are `fill-box` and `stroke-box` for SVG elements, and a non-scaling stroke has
/// the object bounding box as its stroke box, as in Chrome (`ComputedStyle::UsedTransformBox`).
fn reference(document: &Document, style: &ComputedStyle, element: NodeId, used: TransformBox) -> Reference {
    match used {
        TransformBox::ContentBox | TransformBox::FillBox => Reference::Fill,
        TransformBox::BorderBox | TransformBox::StrokeBox => {
            let non_scaling = document.element(element).is_some_and(|data| {
                data.attribute("vector-effect")
                    .is_some_and(|value| value.trim() == "non-scaling-stroke")
                    || data
                        .attribute("style")
                        .is_some_and(|style| style.contains("non-scaling-stroke"))
            }) || style
                .svg_paint
                .iter()
                .any(|(name, value)| *name == "vector-effect" && value == "non-scaling-stroke");
            if non_scaling {
                Reference::Fill
            } else {
                Reference::Stroke
            }
        }
        TransformBox::ViewBox => Reference::View,
    }
}

/// The nearest SVG viewport of `element` (`view-box`): at the origin, as large as the `viewBox`
/// of the `<svg>` that establishes it or, without one, as the `<svg>` itself, whose sizes in
/// percentages (or left out) are taken as those of the viewport around it.
fn viewport(document: &Document, styled: &StyledDocument, element: NodeId) -> [f32; 4] {
    let (mut width, mut height) = (None, None);
    for ancestor in std::iter::successors(document.parent_element(element), |&node| {
        document.parent_element(node)
    }) {
        let Some(data) = document.element(ancestor).filter(|data| data.is_svg("svg")) else {
            continue;
        };
        if width.is_none()
            && height.is_none()
            && let Some([_, _, view_width, view_height]) = data.attribute("viewBox").and_then(view_box)
        {
            return [0.0, 0.0, view_width, view_height];
        }
        let outer = outer(document, ancestor);
        let style = styled.style(ancestor).filter(|_| outer);
        let size = |size: Option<&Size>, attribute: &str| match size {
            Some(Size::Length(LengthPercentage::Px(pixels))) => Some(*pixels),
            _ => data.attribute(attribute).and_then(absolute_length),
        };
        width = width.or_else(|| size(style.map(|style| &style.width), "width"));
        height = height.or_else(|| size(style.map(|style| &style.height), "height"));
        if outer || (width.is_some() && height.is_some()) {
            break;
        }
    }
    [0.0, 0.0, width.unwrap_or(300.0), height.unwrap_or(150.0)]
}

/// The width and height of the nearest SVG viewport of `element` (see [`viewport`]).
pub(crate) fn viewport_size(document: &Document, styled: &StyledDocument, element: NodeId) -> (f32, f32) {
    let [_, _, width, height] = viewport(document, styled, element);
    (width, height)
}

/// The four numbers of a `viewBox`, if it is one with a positive size.
fn view_box(value: &str) -> Option<[f32; 4]> {
    let numbers: Vec<f32> = value
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|part| !part.is_empty())
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    match numbers[..] {
        [x, y, width, height] if width > 0.0 && height > 0.0 => Some([x, y, width, height]),
        _ => None,
    }
}

/// A length in pixels (a number, or one with `px`); `None` for others, such as percentages.
fn absolute_length(value: &str) -> Option<f32> {
    let value = value.trim();
    value
        .strip_suffix("px")
        .unwrap_or(value)
        .trim()
        .parse()
        .ok()
        .filter(|length: &f32| length.is_finite() && *length >= 0.0)
}

/// What CSS declares for an SVG element: `declarations` for its `style` attribute, and its
/// transform, clip and mask (see [`transform`]), the mask with the images of `images` (none
/// without them).
pub(crate) fn with_transforms<'a>(
    document: &'a Document,
    styled: &'a StyledDocument,
    marks: &'a Marks<'a>,
    declarations: fn(&Document, &StyledDocument, NodeId) -> String,
    images: Option<&'a fagus_layout::Images>,
) -> impl Fn(NodeId) -> SvgDeclared + 'a {
    move |element| {
        let mut style = declarations(document, styled, element);
        // A filter of another document, resolved.
        if let Some(own) = styled.style(element)
            && own.filter.iter().any(
                |filter| matches!(filter, fagus_style::values::Filter::Url(url) if !url.starts_with('#')),
            )
            && !outer(document, element)
            && let Some(filter) = svg_filter(&own.filter, own.color, own.zoom, images)
        {
            if !style.is_empty() {
                style.push_str("; ");
            }
            style.push_str("filter: ");
            style.push_str(&filter);
        }
        SvgDeclared {
            style,
            attributes: crate::svg_geometry::attributes(document, styled, element),
            ..transform(document, styled, element, marks, images)
        }
    }
}

/// The filter functions `filters` as CSS text for the SVG renderer, which reads colors of CSS
/// Color 3 and lengths in user units (CSS pixels divided by the element's `zoom`), `current`
/// standing for `currentColor`; `None` for no filter or one it cannot read. A filter of another
/// document (`url(filters.svg#f)`) is written as the functions it resolves to in `images`.
pub(crate) fn svg_filter(
    filters: &[fagus_style::values::Filter],
    current: fagus_style::values::Rgba,
    zoom: f32,
    images: Option<&fagus_layout::Images>,
) -> Option<String> {
    use fagus_style::values::Filter;
    if filters.is_empty() {
        return None;
    }
    let zoom = if zoom.is_finite() && zoom > 0.0 { zoom } else { 1.0 };
    let length = |value: f32| format!("{}px", value / zoom);
    let mut functions = Vec::with_capacity(filters.len());
    for filter in filters {
        functions.push(match filter {
            Filter::Blur(deviation) => format!("blur({})", length(*deviation)),
            Filter::Brightness(amount) => format!("brightness({amount})"),
            Filter::Contrast(amount) => format!("contrast({amount})"),
            Filter::Grayscale(amount) => format!("grayscale({amount})"),
            Filter::HueRotate(degrees) => format!("hue-rotate({degrees}deg)"),
            Filter::Invert(amount) => format!("invert({amount})"),
            Filter::Opacity(amount) => format!("opacity({amount})"),
            Filter::Saturate(amount) => format!("saturate({amount})"),
            Filter::Sepia(amount) => format!("sepia({amount})"),
            // The SVG renderer reads the color of a shadow in hexadecimal only, and takes it
            // for one in linear light (resvg 0.47 turns the shadow into sRGB in a filter of
            // sRGB): the color is given in linear light.
            Filter::DropShadow { x, y, blur, color } => {
                let color = color.unwrap_or(current);
                let linear = |value: f32| {
                    let value = value.clamp(0.0, 1.0);
                    let light = if value <= 0.040_45 {
                        value / 12.92
                    } else {
                        libm::powf((value + 0.055) / 1.055, 2.4)
                    };
                    (light * 255.0).round() as u8
                };
                format!(
                    "drop-shadow({} {} {} #{:02x}{:02x}{:02x}{:02x})",
                    length(*x),
                    length(*y),
                    length(*blur),
                    linear(color.r),
                    linear(color.g),
                    linear(color.b),
                    (color.a.clamp(0.0, 1.0) * 255.0).round() as u8
                )
            }
            // An element of the SVG; others are not found by the SVG renderer.
            Filter::Url(url) if url.starts_with('#') && !url.contains([')', '"', '\\']) => {
                format!("url({url})")
            }
            Filter::Url(url) => match images
                .and_then(|images| images.reference(url))
                .map(|reference| &**reference)
            {
                Some(fagus_layout::SvgReference::Filter(functions)) => {
                    svg_filter(functions, current, zoom, None)?
                }
                _ => return None,
            },
            Filter::ColorMatrix { .. } => return None,
        });
    }
    Some(functions.join(" "))
}

/// The bounding boxes of the elements of the inline `<svg>` element `svg` that [`Marks::Measure`]
/// marked in its document `markup`, by element.
pub(crate) fn boxes(
    document: &Document,
    svg: NodeId,
    markup: &str,
    fonts: &fagus_text::FontDb,
) -> FxHashMap<NodeId, fagus_pdf::svg_boxes::Boxes> {
    let by_index: FxHashMap<usize, NodeId> = document
        .descendants(svg)
        .map(|node| (node.index(), node))
        .collect();
    fagus_pdf::svg_boxes::bounding_boxes(markup.as_bytes(), fonts, MARK)
        .into_iter()
        .filter_map(|(id, boxes)| {
            let index: usize = id.strip_prefix(MARK)?.parse().ok()?;
            Some((*by_index.get(&index)?, boxes))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fagus_css::{MediaEnvironment, Origin, RuleSet, Stylesheet};

    /// The SVG document of the first inline `<svg>` of `html`, its transforms written as
    /// [`crate::Engine`] writes them, with `css` for its elements.
    fn markup(html: &str, css: &str) -> String {
        let document = Document::parse_html(html);
        let ua = Stylesheet::parse(fagus_style::USER_AGENT_CSS, Origin::UserAgent, "ua.css");
        let author = Stylesheet::parse(css, Origin::Author, "test.css");
        let rules = RuleSet::new(&[&ua, &author], &MediaEnvironment::default());
        let styled = fagus_style::compute_styles(&document, &rules, (500.0, 300.0));
        let svg = document
            .descendants(NodeId::DOCUMENT)
            .find(|&node| document.element(node).is_some_and(|data| data.is_svg("svg")))
            .expect("svg");
        let fonts = fagus_text::FontDb::new();
        let none = |_: &Document, _: &StyledDocument, _: NodeId| String::new();
        let measure = Marks::Measure;
        let measured = document.svg_document(
            svg,
            &[],
            &with_transforms(&document, &styled, &measure, none, None),
        );
        let boxes = boxes(&document, svg, &measured, &fonts);
        let marks = Marks::Boxes(&boxes);
        let images = fagus_layout::Images::default();
        document.svg_document(
            svg,
            &[],
            &with_transforms(&document, &styled, &marks, none, Some(&images)),
        )
    }

    #[test]
    fn writes_transforms_of_the_boxes_they_refer_to() {
        let markup = markup(
            r#"<svg width="200" height="100" viewBox="0 0 400 200"><rect id="fill" x="100" y="100" width="100" height="50"/>
            <rect id="view" width="10" height="10"/><rect id="attribute" transform="translate(5)" transform-origin="1"/>
            <circle id="clip" cx="50" cy="50" r="10" clip-path="inset(5px) fill-box"/><rect id="none"/></svg>"#,
            "#fill { transform-box: fill-box; transform: rotate(180deg); transform-origin: 50% 50% }
             #view { transform: translate(50%, 10%) }
             #attribute { transform-box: view-box }",
        );
        let matrix = |id: &str| -> Vec<f32> {
            let element = markup.find(&format!("id=\"{id}\"")).expect("element");
            let start = element + markup[element..].find("transform=\"matrix(").expect("transform") + 18;
            let end = start + markup[start..].find(')').expect("end");
            markup[start..end]
                .split(' ')
                .map(|value| value.parse().expect("number"))
                .collect()
        };
        let near = |a: &[f32], b: &[f32]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-4);
        // About the center of the rectangle's own box, (150, 125).
        assert!(
            near(&matrix("fill"), &[-1.0, 0.0, 0.0, -1.0, 300.0, 250.0]),
            "{markup}"
        );
        // Percentages of the `viewBox`.
        assert!(
            near(&matrix("view"), &[1.0, 0.0, 0.0, 1.0, 200.0, 20.0]),
            "{markup}"
        );
        // The attribute's transform about its origin, in place of both attributes.
        assert!(
            near(&matrix("attribute"), &[1.0, 0.0, 0.0, 1.0, 5.0, 0.0]),
            "{markup}"
        );
        assert!(!markup.contains("transform-origin"), "{markup}");
        // The inset of the circle's box, (40, 40, 20, 20), as a clip path of its own.
        assert!(markup.contains(r#"d="M45 45H55V55H45V45Z""#), "{markup}");
        assert!(
            markup.contains(r#"<circle id="clip" cx="50" cy="50" r="10" clip-path="url(#fagus-clip-"#),
            "{markup}"
        );
        assert!(markup.contains(r#"<rect id="none"></rect>"#), "{markup}");
    }

    #[test]
    fn writes_css_masks_as_mask_elements_in_the_boxes_they_name() {
        let markup = markup(
            r#"<svg width="200" height="100"><rect id="masked" x="20" y="10" width="100" height="50" mask="url(#old)"/>
            <rect id="plain" width="10" height="10"/></svg>"#,
            "#masked { mask-image: linear-gradient(black, black); mask-size: 50% 100%; mask-repeat: no-repeat;
             mask-position: right top; mask-origin: fill-box; mask-mode: luminance }",
        );
        let masked = markup.find("<rect id=\"masked\"").expect("masked");
        let start = markup[..masked].rfind("<mask id=\"fagus-mask-").expect("mask");
        let mask = &markup[start..masked];
        // A layer of luminance, half as wide as the object bounding box, at its right.
        assert!(mask.contains("mask-type=\"luminance\""), "{mask}");
        assert!(
            mask.contains(r#"<rect x="70" y="10" width="50" height="50" fill="url(#fagus-mask-"#),
            "{mask}"
        );
        // It takes the place of the element's own mask.
        let element = &markup[masked..];
        assert!(
            element.starts_with(
                "<rect id=\"masked\" x=\"20\" y=\"10\" width=\"100\" height=\"50\" mask=\"url(#fagus-mask-"
            ),
            "{element}"
        );
        assert!(!markup.contains("url(#old)"), "{markup}");
        assert!(
            markup.contains(r#"<rect id="plain" width="10" height="10"></rect>"#),
            "{markup}"
        );
    }
}

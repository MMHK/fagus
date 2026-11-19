//! Boxes filtered by SVG filters whose primitives CSS filter functions cannot express
//! (`feTurbulence`, `feMorphology`, lighting, `feDisplacementMap`, `feConvolveMatrix`, `feImage`,
//! `feTile`, tables in `feComponentTransfer`), as Chrome prints them: what the box draws is
//! rasterized at 300 ppi (see [`crate::rasterizer`]), goes through the filters and is drawn as an
//! image of the filter region. Its text is pixels then, in Chrome's PDF as well.
//!
//! The filters run in resvg, in an SVG document that applies them to a group whose bounding box
//! is the box's border box, and whose user space starts at its top left corner, as Chrome
//! applies them to HTML elements. The group draws the box's pixels through a filter of its own
//! with an `feImage` of them, so that the pixels do not make its bounding box larger.

use std::fmt::Write as _;
use std::sync::Arc;

use fagus_paint::{FilteredPaint, PixelFilterStep, RectF};
use fagus_style::values::Filter;
use fagus_text::FontDb;
use tiny_skia::{Pixmap, Transform};
use usvg::{ImageHrefResolver, ImageKind, Node};

/// Pixels per CSS pixel at 300 ppi, the resolution Chrome rasterizes filtered boxes at.
pub(crate) const PIXELS_PER_PX: f32 = 300.0 / 96.0;

/// The most pixels of a filtered box: larger ones are rasterized at a lower resolution.
const MAX_PIXELS: f32 = 12_000_000.0;

/// The size of the group that an empty box's filters filter.
const MIN_SIZE: f32 = 0.01;

/// What the `feImage` of the box's pixels refers to.
const SOURCE: &str = "fagus:source";

/// The id of the filter that draws the box's pixels.
const SOURCE_FILTER: &str = "fagus-source";

/// The filtered pixels of `paint` and where they go, in the CSS pixels of its items; `device`
/// maps those to the pixels of the device (at 300 ppi on a page), whose grid the pixels follow
/// where it is not turned. `None` where nothing shows. Where only the pixels of the device in
/// `visible` show (those of the page), a region of the filters that reaches far past them is cut
/// to them and what the filters read around them (see [`Filters::cut`]), as Chrome draws only
/// what the page shows: a box much larger than a page is then drawn at 300 ppi too, in no more
/// pixels than the page has.
pub(crate) fn pixels(
    paint: &FilteredPaint,
    fonts: &FontDb,
    device: Transform,
    visible: Option<RectF>,
) -> Option<(Pixmap, RectF)> {
    let bounds = paint.bounds;
    // An empty box is filtered too: filters that draw on their own (`feFlood`) show.
    if !(bounds.width >= 0.0 && bounds.height >= 0.0) {
        return None;
    }
    // The filter region, in the user space of the filters.
    let filters = filters(paint, fonts)?;
    let mut region = RectF {
        x: filters.region.x + bounds.x,
        y: filters.region.y + bounds.y,
        ..filters.region
    };
    let cut = visible.and_then(|visible| filters.cut(region, device, visible));
    if let Some(cut) = cut {
        if !(cut.width > 0.0 && cut.height > 0.0) {
            return None;
        }
        region = cut;
    }
    let grid = Grid::new(region, device)?;
    let pixels =
        crate::rasterizer::rasterize(&paint.items, fonts, grid.width, grid.height, grid.transform())?;
    let png = Arc::new(png(pixels)?);
    let placed = grid.rect();
    let source = RectF {
        x: placed.x - bounds.x,
        y: placed.y - bounds.y,
        ..placed
    };
    // The regions of the filters, cut where the region is: in the user space of the filters.
    let regions = cut.and_then(|cut| {
        let cut = RectF {
            x: cut.x - bounds.x,
            y: cut.y - bounds.y,
            ..cut
        };
        Some((filters.regions.as_deref()?, cut))
    });
    let document = document(paint, source, (grid.scale.0 * grid.scale.1).sqrt(), regions);
    let mut options = crate::svg::options(document.as_bytes(), fonts);
    options.image_href_resolver = ImageHrefResolver {
        resolve_data: crate::svg::decodable_images(),
        resolve_string: Box::new(move |href, _| (href == SOURCE).then(|| ImageKind::PNG(Arc::clone(&png)))),
    };
    let tree = crate::svg::parse_tree(document.as_bytes(), &options).ok()?;
    drop(document);
    let (mut filtered, _held) = crate::raster::pixmap(grid.width, grid.height)?;
    crate::svg::render(
        &tree,
        grid.transform().pre_translate(bounds.x, bounds.y),
        &mut filtered.as_mut(),
    )?;
    Some((filtered, placed))
}

/// Pixels as a PNG image, compressed fast: it is decoded right away.
fn png(pixels: Pixmap) -> Option<Vec<u8>> {
    let (width, height) = (pixels.width(), pixels.height());
    let mut data = pixels.take();
    for pixel in data.as_chunks_mut::<4>().0 {
        let alpha = pixel[3];
        if alpha > 0 && alpha < 255 {
            for channel in &mut pixel[..3] {
                *channel =
                    ((u32::from(*channel) * 255 + u32::from(alpha) / 2) / u32::from(alpha)).min(255) as u8;
            }
        }
    }
    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let mut writer = encoder.write_header().ok()?;
    writer.write_image_data(&data).ok()?;
    writer.finish().ok()?;
    Some(png)
}

/// The union of the regions of the filters of `group` and the groups in it (left, top, right
/// and bottom), which have no transforms of their own.
fn regions(group: &usvg::Group) -> Option<[f32; 4]> {
    let own = group
        .filters_bounding_box()
        .map(|rect| [rect.left(), rect.top(), rect.right(), rect.bottom()]);
    group
        .children()
        .iter()
        .filter_map(|child| match child {
            Node::Group(group) => regions(group),
            _ => None,
        })
        .chain(own)
        .reduce(|a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])])
}

/// The filters of a box as usvg resolves them.
struct Filters {
    /// The union of their regions, in their user space (from the top left corner of the box's
    /// border box).
    region: RectF,
    /// The region of each by its id, where each filter is used once and only reads pixels near
    /// the pixels it makes: `None` with CSS filter functions or a primitive that reads pixels
    /// anywhere in its region (`feTile`, `feImage`, stitched `feTurbulence`, `feConvolveMatrix`
    /// that wraps around).
    regions: Option<Vec<(String, RectF)>>,
    /// How far from a pixel all the primitives together read the pixels it is made of: in
    /// pixels of their user space, and in pixels of the device.
    reach: (f32, f32),
}

impl Filters {
    /// The part of `region` (in the CSS pixels of the items) that the pixels in `visible` of
    /// the device `device` maps to are made of, where the region reaches far past it (a quarter
    /// of it or more lies outside, or it has more than [`MAX_PIXELS`] on the device) and its
    /// filters can be cut (see [`Filters::regions`]): what the filters make there is what
    /// they make of all of the region. `None` where the region is not cut.
    fn cut(&self, region: RectF, device: Transform, visible: RectF) -> Option<RectF> {
        self.regions.as_ref()?;
        if !(device.kx == 0.0 && device.ky == 0.0 && device.sx > 0.0 && device.sy > 0.0) {
            return None;
        }
        // Rounding to whole pixels, a few more.
        let margin = self.reach.0 * device.sx.max(device.sy) + self.reach.1 + 8.0;
        let left = (visible.x - margin - device.tx) / device.sx;
        let top = (visible.y - margin - device.ty) / device.sy;
        let right = (visible.x + visible.width + margin - device.tx) / device.sx;
        let bottom = (visible.y + visible.height + margin - device.ty) / device.sy;
        if region.x >= left
            && region.y >= top
            && region.x + region.width <= right
            && region.y + region.height <= bottom
        {
            return None;
        }
        let cut = intersection(
            region,
            RectF {
                x: left,
                y: top,
                width: right - left,
                height: bottom - top,
            },
        );
        let area = |rect: RectF| rect.width * rect.height;
        let pixels = area(region) * device.sx * device.sy;
        (area(cut) < area(region) * 0.75 || pixels > MAX_PIXELS).then_some(cut)
    }
}

/// The filters around a box, as usvg resolves them.
fn filters(paint: &FilteredPaint, fonts: &FontDb) -> Option<Filters> {
    let document = document(
        paint,
        RectF {
            x: 0.0,
            y: 0.0,
            ..paint.bounds
        },
        1.0,
        None,
    );
    let tree = crate::svg::parse_tree(
        document.as_bytes(),
        &crate::svg::options(document.as_bytes(), fonts),
    )
    .ok()?;
    let [left, top, right, bottom] = regions(tree.root())?;
    let mut found = Vec::new();
    let mut reach = Some((0.0, 0.0));
    collect(tree.root(), &mut found, &mut reach);
    let ids = paint.filters.iter().map(|step| match step {
        PixelFilterStep::Svg { id, .. } => Some(&**id),
        PixelFilterStep::Function(_) => None,
    });
    let cuttable = ids
        .clone()
        .all(|id| id.is_some_and(|id| found.iter().filter(|(found, _)| found == id).count() == 1))
        && found.len() == paint.filters.len() + 1;
    Some(Filters {
        region: RectF {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        },
        regions: (cuttable && reach.is_some()).then_some(found),
        reach: reach.unwrap_or_default(),
    })
}

/// Adds the filters of `group` and the groups in it with their regions to `found`, and how far
/// their primitives read to `reach`, which becomes `None` where one reads anywhere.
fn collect(group: &usvg::Group, found: &mut Vec<(String, RectF)>, reach: &mut Option<(f32, f32)>) {
    for filter in group.filters() {
        let rect = filter.rect();
        found.push((
            filter.id().to_owned(),
            RectF {
                x: rect.x(),
                y: rect.y(),
                width: rect.width(),
                height: rect.height(),
            },
        ));
        // The box's own pixels, placed by Fagus.
        if filter.id() == SOURCE_FILTER {
            continue;
        }
        for primitive in filter.primitives() {
            *reach = reach
                .zip(primitive_reach(primitive.kind()))
                .map(|(sum, more)| (sum.0 + more.0, sum.1 + more.1));
        }
    }
    for child in group.children() {
        if let Node::Group(child) = child {
            collect(child, found, reach);
        }
    }
}

/// How far from a pixel a primitive reads the pixels it makes it of, at most: in pixels of the
/// user space and in pixels of the device (as resvg filters); `None` for one that reads pixels
/// anywhere in its region.
fn primitive_reach(kind: &usvg::filter::Kind) -> Option<(f32, f32)> {
    use usvg::filter::Kind;
    Some(match kind {
        // Box blurs reach three times their half width, about 2.8 standard deviations; what
        // the recursive blur of small deviations gives beyond four is less than a level.
        Kind::GaussianBlur(blur) => (4.0 * blur.std_dev_x().get().max(blur.std_dev_y().get()), 4.0),
        Kind::DropShadow(shadow) => (
            shadow.dx().abs().max(shadow.dy().abs())
                + 4.0 * shadow.std_dev_x().get().max(shadow.std_dev_y().get()),
            4.0,
        ),
        Kind::Offset(offset) => (offset.dx().abs().max(offset.dy().abs()), 1.0),
        Kind::Morphology(morphology) => (morphology.radius_x().get().max(morphology.radius_y().get()), 1.0),
        // resvg displaces by up to half the square of the scale (see `adjusted`).
        Kind::DisplacementMap(map) => (map.scale() * map.scale() / 2.0, 1.0),
        Kind::ConvolveMatrix(convolve) if convolve.edge_mode() != usvg::filter::EdgeMode::Wrap => (
            0.0,
            convolve.matrix().columns().max(convolve.matrix().rows()) as f32,
        ),
        // Surface normals of three by three pixels.
        Kind::DiffuseLighting(_) | Kind::SpecularLighting(_) => (0.0, 2.0),
        Kind::Turbulence(turbulence) if !turbulence.stitch_tiles() => (0.0, 0.0),
        Kind::Blend(_)
        | Kind::ColorMatrix(_)
        | Kind::ComponentTransfer(_)
        | Kind::Composite(_)
        | Kind::Flood(_)
        | Kind::Merge(_) => (0.0, 0.0),
        Kind::ConvolveMatrix(_) | Kind::Turbulence(_) | Kind::Image(_) | Kind::Tile(_) => return None,
    })
}

/// The pixels a filtered box is rasterized into: `width` × `height` of them from `origin` (in
/// CSS pixels), `scale` per CSS pixel across and down.
struct Grid {
    origin: (f32, f32),
    scale: (f32, f32),
    width: u32,
    height: u32,
}

impl Grid {
    /// The pixels over `region` where `device` maps CSS pixels to those of the device: the
    /// device's own where it only scales and moves, at most [`MAX_PIXELS`] of them.
    fn new(region: RectF, device: Transform) -> Option<Self> {
        let finite = [region.x, region.y, region.width, region.height]
            .iter()
            .all(|value| value.is_finite());
        if !finite || region.width <= 0.0 || region.height <= 0.0 {
            return None;
        }
        let aligned = device.kx == 0.0 && device.ky == 0.0 && device.sx > 0.0 && device.sy > 0.0;
        let average = (device.sx * device.sy - device.kx * device.ky).abs().sqrt();
        let (mut sx, mut sy) = if aligned {
            (device.sx, device.sy)
        } else {
            (average, average)
        };
        if !(sx > 0.0 && sy > 0.0 && sx.is_finite() && sy.is_finite()) {
            return None;
        }
        let area = region.width * sx * region.height * sy;
        let shrink = (MAX_PIXELS / area).sqrt();
        let mut grid = if aligned && shrink >= 1.0 {
            // Whole pixels of the device around the region.
            let left = (region.x * sx + device.tx).floor();
            let top = (region.y * sy + device.ty).floor();
            let right = ((region.x + region.width) * sx + device.tx).ceil();
            let bottom = ((region.y + region.height) * sy + device.ty).ceil();
            Self {
                origin: ((left - device.tx) / sx, (top - device.ty) / sy),
                scale: (sx, sy),
                width: (right - left) as u32,
                height: (bottom - top) as u32,
            }
        } else {
            if shrink < 1.0 {
                (sx, sy) = (sx * shrink, sy * shrink);
            }
            Self {
                origin: (region.x, region.y),
                scale: (sx, sy),
                width: (region.width * sx).ceil() as u32,
                height: (region.height * sy).ceil() as u32,
            }
        };
        grid.width = grid.width.max(1);
        grid.height = grid.height.max(1);
        Some(grid)
    }

    /// From CSS pixels to the grid's pixels.
    fn transform(&self) -> Transform {
        let (sx, sy) = self.scale;
        Transform::from_row(sx, 0.0, 0.0, sy, -self.origin.0 * sx, -self.origin.1 * sy)
    }

    /// The grid's pixels in CSS pixels.
    fn rect(&self) -> RectF {
        RectF {
            x: self.origin.0,
            y: self.origin.1,
            width: self.width as f32 / self.scale.0,
            height: self.height as f32 / self.scale.1,
        }
    }
}

/// The SVG document that filters a box with `paint.bounds`, its pixels being the image of
/// [`SOURCE`] over `source` (relative to the top left corner of the border box), drawn at
/// `scale` pixels per CSS pixel. With `regions`, the region of each filter (by its id) is cut
/// to the rectangle given with them.
fn document(
    paint: &FilteredPaint,
    source: RectF,
    scale: f32,
    regions: Option<(&[(String, RectF)], RectF)>,
) -> String {
    // An empty box filters a group of a tiny size: usvg drops empty ones.
    let (width, height) = (
        paint.bounds.width.max(MIN_SIZE),
        paint.bounds.height.max(MIN_SIZE),
    );
    let mut definitions = String::new();
    // The filters, each the value of a `filter` attribute.
    let mut chain: Vec<String> = Vec::new();
    let mut ids: Vec<&str> = Vec::new();
    for (index, step) in paint.filters.iter().enumerate() {
        match step {
            PixelFilterStep::Svg { markup, id } => {
                if !ids.contains(&&**id) {
                    ids.push(id);
                    let adjusted = adjusted(
                        markup,
                        Some(scale),
                        Some((paint.bounds.width, paint.bounds.height)),
                    );
                    let own = regions.and_then(|(regions, cut)| {
                        let (_, own) = regions.iter().find(|(found, _)| found == &**id)?;
                        Some(intersection(*own, cut))
                    });
                    match own {
                        Some(own) => definitions.push_str(&with_region(&adjusted, own)),
                        None => definitions.push_str(&adjusted),
                    }
                }
                chain.push(format!("url(#{})", escaped(id)));
            }
            PixelFilterStep::Function(Filter::ColorMatrix { matrix, linear }) => {
                // A color matrix has no CSS function: a filter of its own, which reaches as
                // far as usvg lets blurs reach.
                let values: Vec<String> = matrix.iter().map(f32::to_string).collect();
                let _ = write!(
                    definitions,
                    "<filter id=\"fagus-matrix-{index}\" x=\"-50%\" y=\"-50%\" width=\"200%\" height=\"200%\" \
                     color-interpolation-filters=\"{}\"><feColorMatrix type=\"matrix\" values=\"{}\"/></filter>",
                    if *linear { "linearRGB" } else { "sRGB" },
                    values.join(" ")
                );
                chain.push(format!("url(#fagus-matrix-{index})"));
            }
            PixelFilterStep::Function(function) => {
                chain.extend(css_function(function));
            }
        }
    }
    let RectF {
        x: sx,
        y: sy,
        width: sw,
        height: sh,
    } = source;
    // Each filter filters a group of its own, the first innermost: resvg puts what a filter of
    // several yields at the top left corner of their region rather than of its own.
    let mut groups = String::new();
    for filter in chain.iter().rev() {
        let _ = write!(groups, "<g filter=\"{filter}\">");
    }
    let ends = "</g>".repeat(chain.len());
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" \
         width=\"{width}\" height=\"{height}\"><defs>{definitions}\
         <filter id=\"{SOURCE_FILTER}\" filterUnits=\"userSpaceOnUse\" x=\"{sx}\" y=\"{sy}\" width=\"{sw}\" height=\"{sh}\">\
         <feImage xlink:href=\"{SOURCE}\" x=\"{sx}\" y=\"{sy}\" width=\"{sw}\" height=\"{sh}\" preserveAspectRatio=\"none\"/>\
         </filter></defs>{groups}<g filter=\"url(#{SOURCE_FILTER})\">\
         <rect width=\"{width}\" height=\"{height}\"/></g>{ends}</svg>"
    )
}

/// `markup` adjusted for resvg 0.47 to filter as Skia does in Chrome, where `scale` device
/// pixels make a CSS pixel and the box is `size` large (where these are known: the lighting and
/// the noise in the box's units are left as they are where not):
///
/// - `feDisplacementMap`: resvg multiplies the displacement by the scale twice, so its square
///   root displaces as far as the scale says. A negative scale displaces the other way, which
///   this cannot say: it displaces as far, but the way of a positive one. resvg reads the map's
///   colors premultiplied, Skia unpremultiplied: an `feColorMatrix` makes the map opaque first.
/// - `feTile`: resvg says the tiles are in sRGB whatever they are in; tiles in linear light are
///   turned into sRGB after it.
/// - `feTurbulence`: Skia's noise at a point is the reference code's a pixel (of user space)
///   further right and down (`SkPerlinNoiseShader`, as `WebKit` counts pixels from 1): the
///   noise is moved by an `feOffset` a pixel up and left.
/// - `feDiffuseLighting` and `feSpecularLighting`: Skia maps the surface's depth to the device
///   (`surfaceScale` in the user space), resvg takes it in device pixels, which makes slopes
///   `scale` times less steep.
fn adjusted(markup: &str, scale: Option<f32>, size: Option<(f32, f32)>) -> String {
    let bounding = crate::svg::root_attribute(markup, "primitiveUnits")
        .is_some_and(|units| units.trim() == "objectBoundingBox");
    // A pixel of user space in the units of the primitives.
    let pixel = match (bounding, size) {
        (false, _) => Some((1.0, 1.0)),
        (true, Some(size)) => Some((1.0 / size.0.max(1e-3), 1.0 / size.1.max(1e-3))),
        (true, None) => None,
    };
    let linear_default = crate::svg::root_attribute(markup, "color-interpolation-filters")
        .is_none_or(|value| value.trim() != "sRGB");
    // The results of the primitives so far, and whether resvg holds them in linear light.
    let mut results: Vec<(Option<&str>, bool)> = Vec::new();
    let mut out = String::with_capacity(markup.len() + 64);
    let mut rest = markup;
    let (mut noises, mut maps, mut tiles) = (0, 0, 0);
    while let Some(start) = rest.find("<fe") {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let Some(length) = rest.find('>') else { break };
        let tag = &rest[..length];
        let name_end = tag
            .find(|c: char| c.is_whitespace() || c == '/')
            .unwrap_or(tag.len());
        let name = &tag[1..name_end];
        // The rest of the element after its start tag: `/>` or `>` with children and an end tag.
        let closing = if tag.ends_with('/') {
            0
        } else {
            let end = format!("</{name}>");
            rest[length..].find(&end).map_or(0, |at| at + end.len() - 1)
        };
        let after = length + 1 + closing;
        let input_linear = match crate::svg::attribute(tag, "in").map(str::trim) {
            Some(
                "SourceGraphic" | "SourceAlpha" | "BackgroundImage" | "BackgroundAlpha" | "FillPaint"
                | "StrokePaint",
            ) => false,
            Some(input) => results
                .iter()
                .rev()
                .find(|(result, _)| *result == Some(input))
                .is_some_and(|(_, linear)| *linear),
            None => results.last().is_some_and(|(_, linear)| *linear),
        };
        let own_linear = crate::svg::attribute(tag, "color-interpolation-filters")
            .map_or(linear_default, |value| value.trim() != "sRGB");
        results.push((
            crate::svg::attribute(tag, "result"),
            match name {
                "feFlood" | "feImage" | "feTile" => false,
                "feOffset" => input_linear,
                _ => own_linear,
            },
        ));
        match name {
            "feDisplacementMap" => {
                let mut tag = with_attribute(tag, "scale", |value| {
                    let value: f32 = value.unwrap_or("0").trim().parse().unwrap_or(0.0);
                    value.abs().sqrt().to_string()
                });
                // The map's colors without their alpha, opaque, which premultiplied are
                // those it has unpremultiplied (where its alpha, the channel by default, is no
                // channel it displaces by).
                let by_alpha = ["xChannelSelector", "yChannelSelector"]
                    .iter()
                    .any(|name| crate::svg::attribute(&tag, name).is_none_or(|value| value.trim() == "A"));
                if crate::svg::attribute(&tag, "in").is_some() && !by_alpha {
                    let map = format!("fagus-map-{maps}");
                    maps += 1;
                    out.push_str("<feColorMatrix");
                    for attribute in ["in2", "color-interpolation-filters", "x", "y", "width", "height"] {
                        if let Some(value) = crate::svg::attribute(&tag, attribute) {
                            let name = if attribute == "in2" { "in" } else { attribute };
                            let _ = write!(out, " {name}=\"{value}\"");
                        }
                    }
                    let _ = write!(
                        out,
                        " type=\"matrix\" values=\"1 0 0 0 0 0 1 0 0 0 0 0 1 0 0 0 0 0 0 1\" result=\"{map}\"/>"
                    );
                    tag = with_attribute(&tag, "in2", |_| map);
                }
                out.push_str(&tag);
                out.push_str(&rest[length..after]);
            }
            "feDiffuseLighting" | "feSpecularLighting" if let Some(scale) = scale => {
                out.push_str(&with_attribute(tag, "surfaceScale", |value| {
                    let value: f32 = value.unwrap_or("1").trim().parse().unwrap_or(1.0);
                    (value * scale).to_string()
                }));
                out.push_str(&rest[length..after]);
            }
            "feTurbulence" if let Some(pixel) = pixel => {
                let result = crate::svg::attribute(tag, "result").map(str::to_owned);
                let name = format!("fagus-noise-{noises}");
                noises += 1;
                out.push_str(&with_attribute(tag, "result", |_| name.clone()));
                out.push_str(&rest[length..after]);
                let _ = write!(
                    out,
                    "<feOffset in=\"{name}\" dx=\"{}\" dy=\"{}\"",
                    -pixel.0, -pixel.1
                );
                for attribute in ["color-interpolation-filters", "x", "y", "width", "height"] {
                    if let Some(value) = crate::svg::attribute(tag, attribute) {
                        let _ = write!(out, " {attribute}=\"{value}\"");
                    }
                }
                if let Some(result) = result {
                    let _ = write!(out, " result=\"{result}\"");
                }
                out.push_str("/>");
            }
            "feTile" if input_linear => {
                // resvg says the tiles are in sRGB, which they are not: they go through the
                // conversion to sRGB, and are in sRGB then.
                let result = crate::svg::attribute(tag, "result");
                let name = format!("fagus-tile-{tiles}");
                tiles += 1;
                out.push_str(&with_attribute(tag, "result", |_| name.clone()));
                out.push_str(&rest[length..after]);
                let _ = write!(
                    out,
                    "<feComponentTransfer in=\"{name}\" color-interpolation-filters=\"sRGB\""
                );
                for attribute in ["x", "y", "width", "height"] {
                    if let Some(value) = crate::svg::attribute(tag, attribute) {
                        let _ = write!(out, " {attribute}=\"{value}\"");
                    }
                }
                if let Some(result) = result {
                    let _ = write!(out, " result=\"{result}\"");
                }
                out.push('>');
                let table = to_srgb_table();
                for channel in ["R", "G", "B"] {
                    let _ = write!(out, "<feFunc{channel} type=\"table\" tableValues=\"{table}\"/>");
                }
                out.push_str("</feComponentTransfer>");
            }
            _ => out.push_str(&rest[..after]),
        }
        rest = &rest[after..];
    }
    out.push_str(rest);
    out
}

/// The values of a table transfer function that maps linear light to sRGB.
fn to_srgb_table() -> String {
    const STEPS: usize = 255;
    let mut table = String::with_capacity(STEPS * 7);
    for step in 0..=STEPS {
        let linear = step as f32 / STEPS as f32;
        let srgb = if linear <= 0.003_130_8 {
            linear * 12.92
        } else {
            1.055 * libm::powf(linear, 1.0 / 2.4) - 0.055
        };
        if step > 0 {
            table.push(' ');
        }
        let _ = write!(table, "{srgb:.4}");
    }
    table
}

/// An SVG document with its `<filter>` elements adjusted for resvg to filter as Skia does (see
/// [`adjusted`]), drawn at `scale` pixels per pixel of its user space; `None` for one whose
/// filters need nothing.
pub(crate) fn adjusted_filters(data: &[u8], scale: f32) -> Option<String> {
    let text = std::str::from_utf8(data).ok()?;
    if ![
        "<feDisplacementMap",
        "<feTurbulence",
        "<feTile",
        "<feDiffuseLighting",
        "<feSpecularLighting",
    ]
    .iter()
    .any(|needle| text.contains(needle))
    {
        return None;
    }
    let mut out = String::with_capacity(text.len() + 256);
    let mut rest = text;
    while let Some(start) = rest.find("<filter") {
        let Some(length) = rest[start..].find("</filter>") else {
            break;
        };
        let end = start + length + "</filter>".len();
        out.push_str(&rest[..start]);
        out.push_str(&adjusted(&rest[start..end], Some(scale), None));
        rest = &rest[end..];
    }
    out.push_str(rest);
    Some(out)
}

/// The common part of two rectangles, empty (without width or height) where they have none.
fn intersection(a: RectF, b: RectF) -> RectF {
    let (x, y) = (a.x.max(b.x), a.y.max(b.y));
    RectF {
        x,
        y,
        width: ((a.x + a.width).min(b.x + b.width) - x).max(0.0),
        height: ((a.y + a.height).min(b.y + b.height) - y).max(0.0),
    }
}

/// The markup of a `<filter>` with its region set to `region`, in its user space.
fn with_region(markup: &str, region: RectF) -> String {
    let Some(tag) = crate::svg::root_tag(markup) else {
        return markup.to_owned();
    };
    let start = tag.as_ptr() as usize - markup.as_ptr() as usize;
    let mut new = tag.to_owned();
    for (name, value) in [
        ("filterUnits", "userSpaceOnUse".to_owned()),
        ("x", region.x.to_string()),
        ("y", region.y.to_string()),
        ("width", region.width.to_string()),
        ("height", region.height.to_string()),
    ] {
        new = with_attribute(&new, name, |_| value);
    }
    format!("{}{new}{}", &markup[..start], &markup[start + tag.len()..])
}

/// A start tag (without its `>`) with the value of `name` replaced by what `value` makes of
/// the one it has, or added.
fn with_attribute(tag: &str, name: &str, value: impl FnOnce(Option<&str>) -> String) -> String {
    if let Some((_, range)) = crate::svg::attribute_at(tag, name) {
        let new = value(Some(&tag[range.clone()]));
        return format!("{}{new}{}", &tag[..range.start], &tag[range.end..]);
    }
    let (head, slash) = match tag.strip_suffix('/') {
        Some(head) => (head, "/"),
        None => (tag, ""),
    };
    format!("{head} {name}=\"{}\"{slash}", value(None))
}

/// A CSS filter function as usvg reads it, with standard deviations as lengths.
fn css_function(filter: &Filter) -> Option<String> {
    Some(match filter {
        Filter::Blur(sigma) => format!("blur({sigma}px)"),
        Filter::Brightness(amount) => format!("brightness({amount})"),
        Filter::Contrast(amount) => format!("contrast({amount})"),
        Filter::Grayscale(amount) => format!("grayscale({amount})"),
        Filter::HueRotate(degrees) => format!("hue-rotate({degrees}deg)"),
        Filter::Invert(amount) => format!("invert({amount})"),
        Filter::Opacity(amount) => format!("opacity({amount})"),
        Filter::Saturate(amount) => format!("saturate({amount})"),
        Filter::Sepia(amount) => format!("sepia({amount})"),
        Filter::DropShadow { x, y, blur, color } => {
            let color = color.unwrap_or(fagus_style::values::Rgba::BLACK);
            format!(
                "drop-shadow(rgba({}, {}, {}, {}) {x}px {y}px {blur}px)",
                crate::channel(color.r),
                crate::channel(color.g),
                crate::channel(color.b),
                color.a.clamp(0.0, 1.0)
            )
        }
        Filter::ColorMatrix { .. } | Filter::Url(_) => return None,
    })
}

/// An id for an attribute value.
fn escaped(id: &str) -> String {
    id.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
}

/// The pixels as an image for the PDF, trimmed to what is not transparent, and where it goes
/// within `rect` (where all of them go); `None` if all are transparent.
pub(crate) fn image(pixels: Pixmap, rect: RectF) -> Option<(krilla::image::Image, RectF)> {
    let (width, height) = (pixels.width() as usize, pixels.height() as usize);
    let mut data = pixels.take();
    let alpha = |data: &[u8], x: usize, y: usize| data[(y * width + x) * 4 + 3];
    let top = (0..height).find(|y| (0..width).any(|x| alpha(&data, x, *y) > 0))?;
    let bottom = (top..height)
        .rev()
        .find(|y| (0..width).any(|x| alpha(&data, x, *y) > 0))?
        + 1;
    let left = (0..width).find(|x| (top..bottom).any(|y| alpha(&data, *x, y) > 0))?;
    let right = (left..width)
        .rev()
        .find(|x| (top..bottom).any(|y| alpha(&data, *x, y) > 0))?
        + 1;
    let (columns, rows) = (right - left, bottom - top);
    // The colors take the place of the pixels in their buffer, which is never read behind
    // where they are written, so that the image needs a quarter more memory, not twice.
    let mut alphas = Vec::with_capacity(columns * rows);
    let mut written = 0;
    for y in top..bottom {
        for x in left..right {
            let at = (y * width + x) * 4;
            let a = data[at + 3];
            let unmultiply = |value: u8| {
                if a == 0 {
                    0
                } else {
                    ((u32::from(value) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8
                }
            };
            let color = [
                unmultiply(data[at]),
                unmultiply(data[at + 1]),
                unmultiply(data[at + 2]),
            ];
            data[written..written + 3].copy_from_slice(&color);
            written += 3;
            alphas.push(a);
        }
    }
    data.truncate(written);
    data.shrink_to_fit();
    if alphas.iter().all(|value| *value == 255) {
        alphas = Vec::new();
    }
    let image = krilla::image::Image::from_custom(
        crate::raster::RgbaImage {
            color: data,
            alpha: alphas,
            width: columns as u32,
            height: rows as u32,
        },
        false,
    )
    .ok()?;
    let (sx, sy) = (rect.width / width as f32, rect.height / height as f32);
    Some((
        image,
        RectF {
            x: rect.x + left as f32 * sx,
            y: rect.y + top as f32 * sy,
            width: columns as f32 * sx,
            height: rows as f32 * sy,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fagus_paint::Item;
    use fagus_style::values::Rgba;

    fn paint(filter: &str) -> FilteredPaint {
        FilteredPaint {
            items: vec![Item::Rect {
                rect: RectF {
                    x: 10.0,
                    y: 20.0,
                    width: 40.0,
                    height: 20.0,
                },
                color: Rgba::from_u8(0, 0, 255, 1.0),
                radii: None,
            }],
            bounds: RectF {
                x: 10.0,
                y: 20.0,
                width: 40.0,
                height: 20.0,
            },
            filters: vec![PixelFilterStep::Svg {
                markup: Arc::from(filter),
                id: Arc::from("f"),
            }],
        }
    }

    #[test]
    fn filters_pixels_in_the_filter_region_on_the_device_grid() {
        let fonts = FontDb::new();
        // A dilation by 2px: the box grows by two pixels on every side, within the region of
        // 10 % around it.
        let dilated = paint("<filter id=\"f\"><feMorphology operator=\"dilate\" radius=\"2\"/></filter>");
        let device = Transform::from_scale(PIXELS_PER_PX, PIXELS_PER_PX);
        let (pixels, rect) = pixels(&dilated, &fonts, device, None).expect("pixels");
        // The region is 10 % larger on every side, on whole pixels of 300 ppi.
        assert!(
            (rect.x - 6.0).abs() < 0.33 && (rect.y - 18.0).abs() < 0.33,
            "{rect:?}"
        );
        assert!((rect.x * PIXELS_PER_PX - (rect.x * PIXELS_PER_PX).round()).abs() < 1e-3);
        assert_eq!(pixels.width() as f32, (rect.width * PIXELS_PER_PX).round());
        let at = |x: f32, y: f32| {
            let column = ((x - rect.x) * PIXELS_PER_PX) as u32;
            let row = ((y - rect.y) * PIXELS_PER_PX) as u32;
            pixels
                .pixel(column, row)
                .map(|color| (color.blue(), color.alpha()))
        };
        assert_eq!(at(9.0, 30.0), Some((255, 255)));
        assert_eq!(at(7.5, 30.0), Some((0, 0)));
        let (_, trimmed) = image(pixels.clone(), rect).expect("an image");
        assert!(
            (trimmed.x - 8.0).abs() < 0.33 && (trimmed.width - 44.0).abs() < 0.7,
            "{trimmed:?}"
        );
    }

    #[test]
    fn adjusts_primitives_to_filter_as_skia() {
        assert_eq!(
            adjusted(
                "<filter><feDisplacementMap in2=\"t\" scale=\"16\"/><feOffset/></filter>",
                Some(2.0),
                Some((10.0, 10.0))
            ),
            "<filter><feDisplacementMap in2=\"t\" scale=\"4\"/><feOffset/></filter>"
        );
        assert_eq!(
            adjusted(
                "<feDisplacementMap in=\"SourceGraphic\" in2=\"t\" scale=\"1\" xChannelSelector=\"R\" yChannelSelector=\"G\"/>",
                Some(2.0),
                Some((10.0, 10.0))
            ),
            "<feColorMatrix in=\"t\" type=\"matrix\" values=\"1 0 0 0 0 0 1 0 0 0 0 0 1 0 0 0 0 0 0 1\" \
             result=\"fagus-map-0\"/><feDisplacementMap in=\"SourceGraphic\" in2=\"fagus-map-0\" scale=\"1\" \
             xChannelSelector=\"R\" yChannelSelector=\"G\"/>"
        );
        assert_eq!(
            adjusted(
                "<filter><feDiffuseLighting><feDistantLight/></feDiffuseLighting></filter>",
                Some(3.0),
                Some((10.0, 10.0))
            ),
            "<filter><feDiffuseLighting surfaceScale=\"3\"><feDistantLight/></feDiffuseLighting></filter>"
        );
        assert_eq!(
            adjusted(
                "<filter><feTurbulence baseFrequency=\"0.1\" result=\"n\"/></filter>",
                Some(3.0),
                Some((10.0, 10.0))
            ),
            "<filter><feTurbulence baseFrequency=\"0.1\" result=\"fagus-noise-0\"/>\
             <feOffset in=\"fagus-noise-0\" dx=\"-1\" dy=\"-1\" result=\"n\"/></filter>"
        );
        // A map of red 1 moves pixels 4px left: what lies 4px right of them.
        let fonts = FontDb::new();
        let mut displaced = paint(
            "<filter id=\"f\"><feFlood flood-color=\"#f00\" result=\"m\"/>\
             <feDisplacementMap in=\"SourceGraphic\" in2=\"m\" scale=\"8\" xChannelSelector=\"R\" yChannelSelector=\"B\"/></filter>",
        );
        displaced.filters.truncate(1);
        let (pixels, rect) = pixels(&displaced, &fonts, Transform::identity(), None).expect("pixels");
        let (_, placed) = image(pixels.clone(), rect).expect("an image");
        // Half of the scale for a channel of 1 (minus a half): 4px left, and B of 0 moves 4px
        // down (by -0.5 × 8 = -4, what lies 4px above).
        assert!(
            (placed.x - 6.0).abs() < 1.01 && (placed.width - 40.0).abs() < 1.01,
            "{placed:?}"
        );
    }

    #[test]
    fn adjusts_the_filters_of_svg_documents() {
        let svg = "<svg><filter id=\"n\"><feTurbulence/></filter><filter id=\"l\">\
                   <feDiffuseLighting/></filter><rect filter=\"url(#n)\"/></svg>";
        assert_eq!(
            adjusted_filters(svg.as_bytes(), 3.0).as_deref(),
            Some(
                "<svg><filter id=\"n\"><feTurbulence result=\"fagus-noise-0\"/>\
                 <feOffset in=\"fagus-noise-0\" dx=\"-1\" dy=\"-1\"/></filter><filter id=\"l\">\
                 <feDiffuseLighting surfaceScale=\"3\"/></filter><rect filter=\"url(#n)\"/></svg>"
            )
        );
        assert_eq!(
            adjusted_filters(b"<svg><filter><feFlood/></filter></svg>", 3.0),
            None
        );
    }

    #[test]
    fn writes_css_functions_after_svg_filters() {
        let mut turbulence = paint("<filter id=\"f\"><feTurbulence baseFrequency=\"0.1\"/></filter>");
        turbulence
            .filters
            .push(PixelFilterStep::Function(Filter::Blur(2.0)));
        turbulence
            .filters
            .push(PixelFilterStep::Function(Filter::ColorMatrix {
                matrix: [0.0; 20],
                linear: false,
            }));
        let svg = document(
            &turbulence,
            RectF {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            1.0,
            None,
        );
        assert!(
            svg.contains("<g filter=\"url(#fagus-matrix-2)\"><g filter=\"blur(2px)\"><g filter=\"url(#f)\">"),
            "{svg}"
        );
        assert!(svg.contains("feColorMatrix type=\"matrix\""));
        let fonts = FontDb::new();
        let (pixels, _) = pixels(&turbulence, &fonts, Transform::identity(), None).expect("pixels");
        // The matrix makes everything transparent.
        assert!(pixels.data().iter().all(|value| *value == 0));
    }

    /// A box with `filter` that is `width` × `height` pixels from the top left corner.
    fn large(filter: &str, width: f32, height: f32) -> FilteredPaint {
        let rect = RectF {
            x: 0.0,
            y: 0.0,
            width,
            height,
        };
        FilteredPaint {
            items: vec![Item::Rect {
                rect,
                color: Rgba::from_u8(0, 0, 255, 1.0),
                radii: None,
            }],
            bounds: rect,
            filters: vec![PixelFilterStep::Svg {
                markup: Arc::from(filter),
                id: Arc::from("f"),
            }],
        }
    }

    #[test]
    fn cuts_regions_far_past_what_shows_to_it() {
        let fonts = FontDb::new();
        let device = Transform::from_scale(PIXELS_PER_PX, PIXELS_PER_PX);
        // A page of 100 × 100 pixels shows a corner of a box of 4000 × 3000.
        let page = RectF {
            x: 0.0,
            y: 0.0,
            width: 100.0 * PIXELS_PER_PX,
            height: 100.0 * PIXELS_PER_PX,
        };
        let dilated = large(
            "<filter id=\"f\"><feMorphology operator=\"dilate\" radius=\"2\"/></filter>",
            4000.0,
            3000.0,
        );
        let (cut, rect) = pixels(&dilated, &fonts, device, Some(page)).expect("pixels");
        // At 300 ppi, the page with a few pixels around it for the dilation to read.
        assert!(
            (-6.0..-2.0).contains(&rect.x) && (-6.0..-2.0).contains(&rect.y),
            "{rect:?}"
        );
        assert!(
            (104.0..112.0).contains(&rect.width) && (104.0..112.0).contains(&rect.height),
            "{rect:?}"
        );
        assert_eq!(cut.width() as f32, (rect.width * PIXELS_PER_PX).round());
        assert_eq!(
            cut.pixel(10, 10).map(|color| (color.blue(), color.alpha())),
            Some((255, 255))
        );
        // Uncut, the region would have been drawn at less than 300 ppi.
        let (whole, rect) = pixels(&dilated, &fonts, device, None).expect("pixels");
        assert!(whole.width() as f32 / rect.width < 1.0, "{rect:?}");
        // A filter that tiles reads pixels anywhere in its region: it is not cut.
        let tiled = large(
            "<filter id=\"f\"><feOffset width=\"10\" height=\"10\"/><feTile/></filter>",
            4000.0,
            3000.0,
        );
        let (_, rect) = pixels(&tiled, &fonts, device, Some(page)).expect("pixels");
        assert!(rect.width > 4000.0, "{rect:?}");
        // Nothing shows of a box beside the page.
        let mut beside = large(
            "<filter id=\"f\"><feMorphology operator=\"dilate\" radius=\"2\"/></filter>",
            4000.0,
            3000.0,
        );
        beside.bounds.x = 5000.0;
        assert!(pixels(&beside, &fonts, device, Some(page)).is_none());
    }

    #[test]
    fn sets_the_region_of_filters() {
        assert_eq!(
            with_region(
                "<filter id=\"f\" x=\"-5%\" width=\"110%\"><feOffset/></filter>",
                RectF {
                    x: -2.0,
                    y: 1.5,
                    width: 30.0,
                    height: 40.0,
                }
            ),
            "<filter id=\"f\" x=\"-2\" width=\"30\" filterUnits=\"userSpaceOnUse\" y=\"1.5\" \
             height=\"40\"><feOffset/></filter>"
        );
    }
}

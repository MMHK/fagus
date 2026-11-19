//! SVG images, drawn as vector graphics. Parsing never reads files: images inside an SVG must
//! be `data:` URLs, and text uses the fonts of the document.

use std::fmt::Write as _;
use std::rc::Rc;
use std::sync::Arc;

use rustc_hash::FxHashMap;

use fagus_text::FontDb;
use usvg::{ImageHrefResolver, Options, Tree};

/// Whether data looks like an SVG document (or gzip-compressed SVGZ).
pub fn is_svg(data: &[u8]) -> bool {
    if data.starts_with(&[0x1f, 0x8b]) {
        return true;
    }
    let start = data.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(data);
    let head = &start[..start.len().min(1024)];
    let text = String::from_utf8_lossy(head);
    let trimmed = text.trim_start();
    (trimmed.starts_with('<')) && text.contains("<svg")
}

/// Whether an SVG document may have text (compressed ones may), which is set in the document's
/// fonts.
fn has_text(data: &[u8]) -> bool {
    data.windows(5).any(|window| window == b"<text") || data.starts_with(&[0x1f, 0x8b])
}

/// [`parse_to_draw`] (or, without `draw`, [`parse`]), the tree kept in `images` from an earlier
/// parse of the same data with the same fonts. The text of an SVG image (not `inline`) does not
/// see the fonts of the document's `@font-face` rules, as in Chrome, where it is a document of
/// its own, but those of its own rules that `images` keeps (see [`crate::Images::add_svg_faces`]).
pub(crate) fn parse_kept(
    data: &Arc<Vec<u8>>,
    fonts: &FontDb,
    images: &crate::memo::Images,
    draw: bool,
    inline: bool,
) -> Option<Arc<Tree>> {
    let prepared = images.get_or_prepare(
        data,
        crate::memo::Kind::Svg { draw },
        has_text(data).then_some(fonts),
        || {
            let installed;
            let fonts = if has_text(data) {
                // Its own `@font-face` rules stand in for the document's; an inline SVG takes
                // the document's, unless it is one of an SVG image drawn as a document of its
                // own, whose faces are kept for it too.
                installed = match images.svg_faces(data) {
                    Some(faces) => {
                        let mut own = fonts.without_document_faces().into_owned();
                        for face in faces.iter() {
                            own.add_document_face(Arc::clone(face));
                        }
                        std::borrow::Cow::Owned(own)
                    }
                    None if inline => std::borrow::Cow::Borrowed(fonts),
                    None => fonts.without_document_faces(),
                };
                &*installed
            } else {
                fonts
            };
            let tree = if draw {
                parse_to_draw(data, fonts)
            } else {
                parse(data, fonts)
            };
            crate::memo::Prepared::Svg(tree.map(Arc::new))
        },
    );
    match prepared {
        crate::memo::Prepared::Svg(tree) => tree,
        crate::memo::Prepared::Raster(_) => None,
    }
}

/// Parses an SVG document with usvg, its `feTurbulence` filters with at most 9 octaves, as
/// Blink caps them (`FETurbulence::CreateImageFilter`: no more show in 8-bit color): resvg draws
/// as many as the document asks for, a billion of them for each pixel. Its arcs are at most
/// [`MAX_ARC`] across (see [`capped_arcs`]). An SVG that makes usvg panic (an image of 1e30
/// units scaled to an image as large overflows its sizes) does not parse, nor does one whose
/// clip paths reach past [`MAX_CLIP`] (see [`clips_finite`]).
pub(crate) fn parse_tree(data: &[u8], options: &Options<'_>) -> Result<Tree, usvg::Error> {
    let parse = |data: &[u8]| {
        fagus_css::guarded(|| Tree::from_data(&capped_arcs(&capped_octaves(data)), options))
            .unwrap_or(Err(usvg::Error::InvalidSize))
            .and_then(|tree| {
                if clips_finite(tree.root()) {
                    Ok(tree)
                } else {
                    Err(usvg::Error::InvalidSize)
                }
            })
    };
    if data.starts_with(&[0x1f, 0x8b]) {
        return parse(&usvg::decompress_svgz(data)?);
    }
    parse(data)
}

/// The largest coordinate of a clip path of an SVG, in the user space of its root: PDF writers
/// (krilla) transform clip paths to the page and fail on coordinates past what `f32` holds,
/// which clip paths under transforms such as `scale(1e30)`, nested, reach.
const MAX_CLIP: f32 = 1e30;

/// Whether the clip paths of the SVG of root `root` stay within [`MAX_CLIP`], under the
/// transforms of the elements they clip. A clip path that clips many elements under the same
/// transform is looked at once.
fn clips_finite(root: &usvg::Group) -> bool {
    use usvg::{Node, Transform};
    let bits = |transform: Transform| {
        let Transform {
            sx,
            kx,
            ky,
            sy,
            tx,
            ty,
        } = transform;
        [sx, kx, ky, sy, tx, ty].map(f32::to_bits)
    };
    let mut seen = rustc_hash::FxHashSet::default();
    // Groups, the transform they are under and whether they are in a clip path.
    let mut stack = vec![(root, Transform::identity(), false)];
    while let Some((group, outer, clipping)) = stack.pop() {
        let transform = outer.pre_concat(group.transform());
        let Transform {
            sx,
            kx,
            ky,
            sy,
            tx,
            ty,
        } = transform;
        if ![sx, kx, ky, sy, tx, ty].iter().all(|value| value.is_finite()) {
            return false;
        }
        // A clip path's own clip path is in the user space of the element too.
        let mut clip = group.clip_path();
        while let Some(path) = clip {
            let clip_transform = transform.pre_concat(path.transform());
            if seen.insert((std::ptr::from_ref(path) as usize, bits(clip_transform))) {
                stack.push((path.root(), clip_transform, true));
            }
            clip = path.clip_path();
        }
        if let Some(mask) = group.mask() {
            stack.push((mask.root(), transform, clipping));
        }
        for child in group.children() {
            match child {
                Node::Group(child) => stack.push((child, transform, clipping)),
                Node::Text(text) => stack.push((text.flattened(), transform, clipping)),
                Node::Path(path) if clipping => {
                    let bounds = path.data().bounds().transform(transform);
                    if !bounds.is_some_and(|bounds| {
                        [bounds.left(), bounds.top(), bounds.right(), bounds.bottom()]
                            .iter()
                            .all(|value| value.abs() <= MAX_CLIP)
                    }) {
                        return false;
                    }
                }
                Node::Path(_) | Node::Image(_) => {}
            }
        }
    }
    true
}

/// The largest radius (`r`, `rx`, `ry`) and coordinate in path data with arcs that usvg is
/// handed.
const MAX_ARC: f64 = 1e12;

/// `data` with the values of `r`, `rx` and `ry` attributes and the numbers of path data with
/// arcs (`d`) beyond [`MAX_ARC`] lowered to it. usvg cuts arcs into curves a tenth of a unit
/// from the arc at most, and so a circle a hundred billion billion billion units across into
/// millions of curves, which took gigabytes and minutes for one icon.
fn capped_arcs(data: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    let mut result: Option<Vec<u8>> = None;
    let mut copied = 0;
    let mut from = 0;
    while let Some(found) = data[from..].iter().position(|&byte| byte == b'=') {
        let equals = from + found;
        from = equals + 1;
        // The attribute's name, after white space.
        let mut name_start = equals;
        while name_start > 0 && data[name_start - 1].is_ascii_alphabetic() {
            name_start -= 1;
        }
        let name = &data[name_start..equals];
        let named = matches!(name, b"r" | b"rx" | b"ry" | b"d");
        if !named || name_start == 0 || !data[name_start - 1].is_ascii_whitespace() {
            continue;
        }
        let Some(&quote) = data.get(equals + 1).filter(|quote| matches!(quote, b'"' | b'\'')) else {
            continue;
        };
        let start = equals + 2;
        let Some(length) = data[start..].iter().position(|&byte| byte == quote) else {
            break;
        };
        let end = start + length;
        from = end;
        let value = &data[start..end];
        if name == b"d" && !value.iter().any(|byte| matches!(byte, b'A' | b'a')) {
            continue;
        }
        let mut at = 0;
        while at < value.len() {
            let number = number_at(value, at);
            if number.is_empty() {
                at += 1;
                continue;
            }
            let span = start + at..start + at + number.len();
            at += number.len();
            let magnitude = std::str::from_utf8(number)
                .ok()
                .and_then(|number| number.parse::<f64>().ok())
                .map_or(0.0, f64::abs);
            if magnitude > MAX_ARC {
                let result = result.get_or_insert_with(|| Vec::with_capacity(data.len()));
                result.extend_from_slice(&data[copied..span.start]);
                if number.starts_with(b"-") {
                    result.push(b'-');
                }
                result.extend_from_slice(b"1e12");
                copied = span.end;
            }
        }
    }
    match result {
        Some(mut result) => {
            result.extend_from_slice(&data[copied..]);
            std::borrow::Cow::Owned(result)
        }
        None => std::borrow::Cow::Borrowed(data),
    }
}

/// The number at `at` in SVG path data or a length (a sign, digits with a point, an exponent),
/// empty where none starts there.
fn number_at(text: &[u8], at: usize) -> &[u8] {
    let digits = |from: usize| {
        from + text[from.min(text.len())..]
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count()
    };
    let mut end = at;
    if matches!(text.get(end), Some(b'+' | b'-')) {
        end += 1;
    }
    let whole = digits(end);
    let mut fraction = whole;
    if text.get(whole) == Some(&b'.') {
        fraction = digits(whole + 1);
    }
    let mantissa = fraction - end - usize::from(fraction > whole);
    if mantissa == 0 {
        return &[];
    }
    end = fraction;
    if matches!(text.get(end), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(text.get(end + 1), Some(b'+' | b'-')));
        let exponent = digits(end + 1 + sign);
        if exponent > end + 1 + sign {
            end = exponent;
        }
    }
    &text[at..end]
}

/// The most octaves of noise a `feTurbulence` filter adds up.
const MAX_OCTAVES: f64 = 9.0;

/// `data` with the values of `numOctaves` attributes above [`MAX_OCTAVES`] lowered to it.
fn capped_octaves(data: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    const NAME: &[u8] = b"numOctaves";
    let mut result: Option<Vec<u8>> = None;
    let mut copied = 0;
    let mut from = 0;
    while let Some(found) = data[from..].windows(NAME.len()).position(|window| window == NAME) {
        let mut at = from + found + NAME.len();
        from = at;
        let skip = |at: &mut usize| {
            while data.get(*at).is_some_and(u8::is_ascii_whitespace) {
                *at += 1;
            }
        };
        skip(&mut at);
        if data.get(at) != Some(&b'=') {
            continue;
        }
        at += 1;
        skip(&mut at);
        let Some(&quote) = data.get(at).filter(|quote| matches!(quote, b'"' | b'\'')) else {
            continue;
        };
        let start = at + 1;
        let Some(length) = data[start..].iter().position(|byte| *byte == quote) else {
            break;
        };
        let end = start + length;
        let value = std::str::from_utf8(&data[start..end])
            .ok()
            .and_then(|value| value.trim().parse::<f64>().ok());
        if value.is_some_and(|value| value > MAX_OCTAVES) {
            let result = result.get_or_insert_with(|| Vec::with_capacity(data.len()));
            result.extend_from_slice(&data[copied..start]);
            result.extend_from_slice(b"9");
            copied = end;
        }
        from = end;
    }
    match result {
        Some(mut result) => {
            result.extend_from_slice(&data[copied..]);
            std::borrow::Cow::Owned(result)
        }
        None => std::borrow::Cow::Borrowed(data),
    }
}

/// The largest radius of an `feMorphology` filter, in CSS pixels: Skia takes no more than 256
/// pixels (`SkMorphologyImageFilter`, against slow draws: crbug.com/1123035), of the 300 dots
/// per inch at which Chrome rasterizes filters for print (measured: a dilation by 100 pixels
/// reaches 82 pixels far). resvg takes time growing with the square of the radius for each
/// pixel, which for the radius of a billion pixels never ends.
pub(crate) const MAX_MORPHOLOGY_RADIUS: f32 = 256.0 * 96.0 / 300.0;

/// Whether an SVG document has `feMorphology` filters.
pub(crate) fn has_morphology(data: &[u8]) -> bool {
    data.windows(b"<feMorphology".len())
        .any(|window| window == b"<feMorphology")
}

/// How far the `feMorphology` filters of `tree` reach where the SVG is drawn `scale` times its
/// size: the largest of their radii in CSS pixels, and the CSS pixels a unit of user space of the
/// one that reaches farthest takes (the scale of the element it filters).
pub(crate) fn morphology_reach(tree: &Tree, scale: f32) -> (f32, f32) {
    fn walk(group: &usvg::Group, scale: f32, reach: &mut (f32, f32)) {
        if !group.filters().is_empty() {
            let transform = group.abs_transform();
            let factor = scale
                * libm::hypotf(transform.sx, transform.ky).max(libm::hypotf(transform.kx, transform.sy));
            for filter in group.filters() {
                for primitive in filter.primitives() {
                    if let usvg::filter::Kind::Morphology(morphology) = primitive.kind() {
                        let radius = morphology.radius_x().get().max(morphology.radius_y().get());
                        if radius * factor > reach.0 {
                            *reach = (radius * factor, factor);
                        }
                    }
                }
            }
        }
        for child in group.children() {
            if let usvg::Node::Group(child) = child {
                walk(child, scale, reach);
            }
        }
    }
    let mut reach = (0.0, scale);
    walk(tree.root(), scale, &mut reach);
    reach
}

/// The radius of an `feMorphology` filter, in CSS pixels, from which it is drawn in two passes,
/// across and down (see [`drawn_morphology`]).
pub(crate) const SEPARATE_MORPHOLOGY_RADIUS: f32 = 4.0;

/// `data` with its `feMorphology` filters as they are drawn where a unit of their user space
/// takes `scale` CSS pixels, in filters whose primitives take user space units (the default):
/// radii reach no farther than [`MAX_MORPHOLOGY_RADIUS`] CSS pixels, as in Chrome, and a filter
/// that reaches farther than [`SEPARATE_MORPHOLOGY_RADIUS`] is two, across and then down (each a
/// hundredth of a pixel the other way, as usvg takes no radius for one of a unit), so that
/// resvg, which takes the extremes of all pixels around each one, takes time growing with the
/// radius rather than its square. That moves the edges of the result by a pixel of resvg's
/// (the third of a CSS pixel) up and to the left.
pub(crate) fn drawn_morphology(data: &[u8], scale: f32) -> std::borrow::Cow<'_, [u8]> {
    let Ok(text) = std::str::from_utf8(data) else {
        return std::borrow::Cow::Borrowed(data);
    };
    let max = (MAX_MORPHOLOGY_RADIUS / scale).max(f32::MIN_POSITIVE);
    let separate = SEPARATE_MORPHOLOGY_RADIUS / scale;
    let thin = (0.01 / scale).max(f32::MIN_POSITIVE);
    let mut result: Option<String> = None;
    let mut copied = 0;
    let mut from = 0;
    let mut passes = 0;
    while let Some(found) = text[from..].find("<feMorphology") {
        let start = from + found;
        let Some(length) = text[start..].find('>') else {
            break;
        };
        from = start + length + 1;
        let tag = &text[start + 1..from - 1];
        // The filter around it, if its primitives take fractions of the bounding box.
        let bounding = text[..start].rfind("<filter").is_some_and(|filter| {
            text[filter..].find('>').is_some_and(|end| {
                attribute(&text[filter + 1..filter + end], "primitiveUnits")
                    .is_some_and(|units| units.trim() == "objectBoundingBox")
            })
        });
        let Some((_, value)) = attribute_at(tag, "radius").filter(|_| !bounding) else {
            continue;
        };
        let numbers: Option<Vec<f32>> = tag[value.clone()]
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|part| !part.is_empty())
            .map(|part| part.parse().ok())
            .collect();
        let (x, y) = match numbers.as_deref() {
            Some(&[radius]) => (radius, radius),
            Some(&[x, y]) => (x, y),
            _ => continue,
        };
        if !(x > 0.0 && y > 0.0 && x.is_finite() && y.is_finite()) {
            continue;
        }
        let capped = x > max || y > max;
        let (x, y) = (x.min(max), y.min(max));
        let self_closing = tag.ends_with('/');
        let separated = x.max(y) > separate && self_closing;
        if !capped && !separated {
            continue;
        }
        let result = result.get_or_insert_with(|| String::with_capacity(text.len() + 256));
        result.push_str(&text[copied..start]);
        if separated {
            // Across, then down, into the result of the filter.
            let tag = &tag[..tag.len() - 1];
            let named = format!("fagus-morphology-{passes}");
            passes += 1;
            let across = with_attribute(
                &with_attribute(tag, "radius", &format!("{x} {thin}")),
                "result",
                &named,
            );
            let mut down = with_attribute(tag, "radius", &format!("{thin} {y}"));
            down = with_attribute(&down, "in", &named);
            let _ = write!(result, "<{across}/><{down}/>");
        } else {
            let _ = write!(result, "<{}>", with_attribute(tag, "radius", &format!("{x} {y}")));
        }
        copied = from;
    }
    match result {
        Some(mut result) => {
            result.push_str(&text[copied..]);
            std::borrow::Cow::Owned(result.into_bytes())
        }
        None => std::borrow::Cow::Borrowed(data),
    }
}

/// A start tag (without `<` and `>`) with attribute `name` set to `value`.
fn with_attribute(tag: &str, name: &str, value: &str) -> String {
    let (head, slash) = match tag.strip_suffix('/') {
        Some(head) => (head, "/"),
        None => (tag, ""),
    };
    match attribute_at(head, name) {
        Some((span, _)) => format!(
            "{} {name}=\"{value}\"{}{slash}",
            &head[..span.start],
            &head[span.end..]
        ),
        None => format!("{head} {name}=\"{value}\"{slash}"),
    }
}

/// The images of `data:` URLs in an SVG, as usvg reads them (BMP and TIFF images as PNG images of
/// their pixels), without those whose pixels do not decode: krilla only decodes them as it writes
/// the PDF, and would fail the document for one, where Chrome leaves the image out.
fn decodable_images() -> usvg::ImageHrefDataResolverFn<'static> {
    let resolve = ImageHrefResolver::default_data_resolver();
    Box::new(move |mime, data, options| {
        use fagus_layout::ImageFormat;
        // An SVG image in the SVG is parsed with its filters and arcs capped as well.
        let data = match capped_octaves(&data) {
            std::borrow::Cow::Owned(capped) => Arc::new(capped),
            std::borrow::Cow::Borrowed(_) => data,
        };
        let data = match capped_arcs(&data) {
            std::borrow::Cow::Owned(capped) => Arc::new(capped),
            std::borrow::Cow::Borrowed(_) => data,
        };
        // BMP and TIFF images, which usvg does not read, as PNG images of their pixels.
        let kind = match crate::decode::as_png(&data) {
            Some(png) => usvg::ImageKind::PNG(Arc::new(png)),
            None => resolve(mime, data, options)?,
        };
        let decodes = match &kind {
            usvg::ImageKind::PNG(data) => crate::decode::rgba(data, ImageFormat::Png).is_some(),
            usvg::ImageKind::GIF(data) => crate::decode::rgba(data, ImageFormat::Gif).is_some(),
            usvg::ImageKind::WEBP(data) => crate::decode::rgba(data, ImageFormat::Webp).is_some(),
            // JPEG data is embedded as it is, without decoding it.
            usvg::ImageKind::JPEG(_) | usvg::ImageKind::SVG(_) => true,
        };
        decodes.then_some(kind)
    })
}

/// Parsing options: no file access, and the document's fonts for text, chosen by the font
/// matching of CSS (the families of `@font-face` rules hide installed ones, generic families
/// stand for the fonts they stand for in HTML), the standard font where an SVG names none.
pub(crate) fn options<'a>(data: &[u8], fonts: &'a FontDb) -> Options<'a> {
    options_with_faces(data, fonts).0
}

/// [`options`], and the faces that its text takes, if it has text.
fn options_with_faces<'a>(data: &[u8], fonts: &'a FontDb) -> (Options<'a>, Option<Arc<Faces<'a>>>) {
    let mut options = Options {
        image_href_resolver: ImageHrefResolver {
            resolve_data: decodable_images(),
            resolve_string: Box::new(|_, _| None),
        },
        ..Options::default()
    };
    // Fonts are only loaded for SVGs with text, those the text takes.
    if !has_text(data) {
        return (options, None);
    }
    let faces = Arc::new(Faces {
        fonts,
        loaded: std::sync::Mutex::new(Vec::new()),
        synthetic: std::sync::atomic::AtomicBool::new(false),
    });
    let (select, fallback) = (Arc::clone(&faces), Arc::clone(&faces));
    fagus_text::STANDARD.clone_into(&mut options.font_family);
    options.fontdb = Arc::new(usvg::fontdb::Database::new());
    options.font_resolver = usvg::FontResolver {
        select_font: Box::new(move |font, database| select.select(font, database)),
        select_fallback: Box::new(move |character, used, database| {
            fallback.fallback(character, used, database)
        }),
    };
    (options, Some(faces))
}

/// Parses an SVG document whose text takes the document's fonts, with synthetic bold and
/// italics where a face lacks the weight or the slant, as in HTML: usvg imitates neither, so the
/// tree is written out with its text and read again, with the spans of such text outlined in
/// their fill, a stroke as wide as [`crate::fake_bold_width`], and text whose spans all lean
/// slanted about its baseline as [`crate::SLANT`] says. Chunks anchored at their middle or end
/// with inner shifts are corrected likewise (see [`crate::text_anchor`]), and small capitals
/// that the faces lack are made up (see [`crate::small_caps`]).
fn parse_text(data: &[u8], fonts: &FontDb) -> Option<Tree> {
    let (first, faces) = options_with_faces(data, fonts);
    let tree = parse_tree(data, &first).ok()?;
    let synthetic = faces
        .as_ref()
        .is_some_and(|faces| faces.synthetic.load(std::sync::atomic::Ordering::Relaxed));
    let anchors = faces.is_some() && crate::text_anchor::needs_fix(tree.root());
    let small_caps = faces.is_some() && crate::small_caps::needed(tree.root());
    if !synthetic && !anchors && !small_caps {
        return Some(tree);
    }
    let mut written = tree.to_string(&usvg::WriteOptions {
        preserve_text: true,
        ..usvg::WriteOptions::default()
    });
    if anchors {
        written = crate::text_anchor::fix(&written);
    }
    if small_caps {
        written = crate::small_caps::synthesize(&written, fonts);
    }
    let synthesized = if synthetic {
        synthesize(&written, fonts)
    } else {
        written
    };
    parse_tree(synthesized.as_bytes(), &options(synthesized.as_bytes(), fonts))
        .ok()
        .or(Some(tree))
}

/// The SVG that usvg wrote out (`written`), with its text in faces that lack the weight made
/// bolder and in faces that lack the slant slanted.
fn synthesize(written: &str, fonts: &FontDb) -> String {
    let mut out = String::with_capacity(written.len() + 256);
    let mut rest = written;
    while let Some(start) = rest.find("<text") {
        let Some(end) = rest[start..]
            .find("</text>")
            .map(|end| start + end + "</text>".len())
        else {
            break;
        };
        out.push_str(&rest[..start]);
        synthesize_text(&rest[start..end], fonts, &mut out);
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// Writes a `<text>` element that usvg wrote to `out`, its spans made bolder and the text
/// slanted where their faces lack the weight or the slant.
fn synthesize_text(text: &str, fonts: &FontDb, out: &mut String) {
    let mut element = String::with_capacity(text.len() + 64);
    let mut rest = text;
    // Whether every span is slanted, and the baselines of the chunks.
    let mut slanted = true;
    let mut baselines: Vec<&str> = Vec::new();
    while let Some(start) = rest.find("<tspan") {
        let Some(end) = rest[start..].find('>').map(|end| start + end) else {
            break;
        };
        let tag = &rest[start + 1..end];
        element.push_str(&rest[..=start]);
        if let Some(y) = attribute(tag, "y") {
            baselines.push(y);
        }
        match span_font(tag, fonts) {
            Some((face, weight, style, size)) => {
                let face = fonts.face(face);
                slanted &= face.synthetic_italic(style);
                let stroke = attribute(tag, "stroke").is_none_or(|stroke| stroke == "none");
                let fill = attribute(tag, "fill").unwrap_or("#000000");
                if face.synthetic_bold(weight) && stroke && fill != "none" {
                    // usvg writes `stroke="none"` for text without a stroke.
                    match attribute_at(tag, "stroke") {
                        Some((span, _)) => {
                            element.push_str(&tag[..span.start]);
                            element.push_str(&tag[span.end..]);
                        }
                        None => element.push_str(tag),
                    }
                    let _ = write!(
                        element,
                        " stroke=\"{fill}\" stroke-width=\"{}\"",
                        crate::fake_bold_width(size)
                    );
                    if let Some(opacity) = attribute(tag, "fill-opacity") {
                        let _ = write!(element, " stroke-opacity=\"{opacity}\"");
                    }
                } else {
                    element.push_str(tag);
                }
            }
            None => element.push_str(tag),
        }
        rest = &rest[end..];
    }
    element.push_str(rest);
    // Text on one baseline leans about it; text of several lines or shifted glyphs is left
    // upright.
    let baseline = baselines
        .first()
        .filter(|first| baselines.iter().all(|y| y == *first))
        .and_then(|y| y.parse::<f32>().ok());
    match baseline {
        Some(y) if slanted && !text.contains(" dy=") && !text.contains(" rotate=") => {
            let slant = crate::SLANT;
            let _ = write!(out, "<g transform=\"matrix(1 0 {} 1 {} 0)\">", -slant, slant * y);
            out.push_str(&element);
            out.push_str("</g>");
        }
        _ => out.push_str(&element),
    }
}

/// The face, weight, style and size of a span that usvg wrote (its start tag `tag`); `None`
/// for a `<tspan>` that only holds spans.
pub(crate) fn span_font(
    tag: &str,
    fonts: &FontDb,
) -> Option<(fagus_text::FaceId, f32, fagus_text::FontStyle, f32)> {
    let size: f32 = attribute(tag, "font-size")?.parse().ok()?;
    let weight: f32 = attribute(tag, "font-weight").map_or(Some(400.0), |weight| weight.parse().ok())?;
    let style = match attribute(tag, "font-style") {
        Some("italic") => fagus_text::FontStyle::Italic,
        Some("oblique") => fagus_text::FontStyle::OBLIQUE,
        _ => fagus_text::FontStyle::Normal,
    };
    let families: Vec<&str> = attribute(tag, "font-family")
        .unwrap_or(fagus_text::STANDARD)
        .split(',')
        .map(|family| family.trim().trim_matches(['\'', '"']))
        .collect();
    Some((fonts.query(&families, weight, style)?, weight, style, size))
}

/// The faces of the document as usvg takes them: each loaded into its font database when text
/// first takes it.
struct Faces<'a> {
    fonts: &'a FontDb,
    /// The faces loaded so far, whether as capitalized (see [`crate::small_caps::capitalized`]),
    /// with their ids in the database.
    loaded: std::sync::Mutex<Vec<(fagus_text::FaceId, bool, usvg::fontdb::ID)>>,
    /// Whether text takes a face that lacks its weight or its slant, which is imitated.
    synthetic: std::sync::atomic::AtomicBool,
}

impl Faces<'_> {
    /// The face that CSS font matching selects for a font of SVG text.
    fn select(
        &self,
        font: &usvg::Font,
        database: &mut Arc<usvg::fontdb::Database>,
    ) -> Option<usvg::fontdb::ID> {
        let mut families: Vec<&str> = font
            .families()
            .iter()
            .map(|family| match family {
                usvg::FontFamily::Serif => "serif",
                usvg::FontFamily::SansSerif => "sans-serif",
                usvg::FontFamily::Cursive => "cursive",
                usvg::FontFamily::Fantasy => "fantasy",
                usvg::FontFamily::Monospace => "monospace",
                usvg::FontFamily::Named(name) => name.as_str(),
            })
            .collect();
        // Made-up small capitals: the face with its lowercase letters drawn as capitals.
        let capitals = families.first() == Some(&crate::small_caps::CAPITALS);
        if capitals {
            families.remove(0);
        }
        let style = match font.style() {
            usvg::FontStyle::Normal => fagus_text::FontStyle::Normal,
            usvg::FontStyle::Italic => fagus_text::FontStyle::Italic,
            usvg::FontStyle::Oblique => fagus_text::FontStyle::OBLIQUE,
        };
        let stretch = match font.stretch() {
            usvg::FontStretch::UltraCondensed => 50.0,
            usvg::FontStretch::ExtraCondensed => 62.5,
            usvg::FontStretch::Condensed => 75.0,
            usvg::FontStretch::SemiCondensed => 87.5,
            usvg::FontStretch::Normal => 100.0,
            usvg::FontStretch::SemiExpanded => 112.5,
            usvg::FontStretch::Expanded => 125.0,
            usvg::FontStretch::ExtraExpanded => 150.0,
            usvg::FontStretch::UltraExpanded => 200.0,
        };
        let weight = f32::from(font.weight());
        let face = self.fonts.query_stretch(&families, weight, style, stretch)?;
        let info = self.fonts.face(face);
        if info.synthetic_bold(weight) || info.synthetic_italic(style) {
            self.synthetic.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        if capitals && let Some(id) = self.load_capitalized(face, database) {
            return Some(id);
        }
        self.load(face, database)
    }

    /// The id of the capitalized variant of `face` (see [`crate::small_caps::capitalized`]),
    /// loaded into the database if it is not yet; `None` where the face cannot be capitalized.
    fn load_capitalized(
        &self,
        face: fagus_text::FaceId,
        database: &mut Arc<usvg::fontdb::Database>,
    ) -> Option<usvg::fontdb::ID> {
        let mut loaded = self.loaded.lock().ok()?;
        if let Some((.., id)) = loaded
            .iter()
            .find(|(known, capitals, _)| *known == face && *capitals)
        {
            return Some(*id);
        }
        let info = self.fonts.face(face);
        let data: &[u8] = (*info.data).as_ref();
        let capitalized = crate::small_caps::capitalized(data, info.index)?;
        let source: Arc<dyn AsRef<[u8]> + Send + Sync> = Arc::new(capitalized);
        let database = Arc::make_mut(database);
        let ids = database.load_font_source(usvg::fontdb::Source::Binary(source));
        let id = ids.iter().copied().find(|id| {
            database
                .face(*id)
                .is_some_and(|loaded| loaded.index == info.index)
        })?;
        loaded.push((face, true, id));
        Some(id)
    }

    /// A face with a glyph for `character` that text in the face `used[0]` falls back to, as
    /// in HTML; none of the faces in `used`.
    fn fallback(
        &self,
        character: char,
        used: &[usvg::fontdb::ID],
        database: &mut Arc<usvg::fontdb::Database>,
    ) -> Option<usvg::fontdb::ID> {
        let base = {
            let loaded = self.loaded.lock().ok()?;
            let first = used.first()?;
            loaded.iter().find(|(.., id)| id == first)?.0
        };
        let face = self.fonts.face(base);
        let found = self.fonts.fallback(
            &[face.family.as_str()],
            base,
            character,
            (face.weight, face.style, 100.0),
        )?;
        self.load(found, database).filter(|id| !used.contains(id))
    }

    /// The id of `face` in the database, loaded into it if it is not yet.
    fn load(
        &self,
        face: fagus_text::FaceId,
        database: &mut Arc<usvg::fontdb::Database>,
    ) -> Option<usvg::fontdb::ID> {
        let mut loaded = self.loaded.lock().ok()?;
        if let Some((.., id)) = loaded
            .iter()
            .find(|(known, capitals, _)| *known == face && !capitals)
        {
            return Some(*id);
        }
        let info = self.fonts.face(face);
        let source: Arc<dyn AsRef<[u8]> + Send + Sync> = Arc::clone(&info.data) as _;
        let database = Arc::make_mut(database);
        let ids = database.load_font_source(usvg::fontdb::Source::Binary(source));
        let id = ids.iter().copied().find(|id| {
            database
                .face(*id)
                .is_some_and(|loaded| loaded.index == info.index)
        })?;
        loaded.push((face, false, id));
        Some(id)
    }
}

/// Parses an SVG document.
pub(crate) fn parse(data: &[u8], fonts: &FontDb) -> Option<Tree> {
    let resolved = resolve_media(data);
    parse_effects(resolved.as_ref().map_or(data, String::as_bytes), data, fonts)
}

/// [`parse_text`], with the filters adjusted to filter as Skia does (see
/// [`crate::filtered::adjusted_filters`]), the clip paths that hold text written as masks (see
/// [`crate::clip_text`]) and the shapes that have a non-scaling stroke (`vector-effect`) written
/// again so that their strokes are not scaled (see [`crate::non_scaling`]). `original` is the
/// document as it was given, which has its natural width.
/// A document that a rewrite leaves unreadable is read as it was.
fn parse_effects(data: &[u8], original: &[u8], fonts: &FontDb) -> Option<Tree> {
    effects_at(data, natural_size(original).0, fonts)
}

/// [`parse_effects`] with non-scaling strokes as wide as they say where the SVG is drawn
/// `width` CSS pixels wide (its natural width; `None` where it has none).
fn effects_at(data: &[u8], width: Option<f32>, fonts: &FontDb) -> Option<Tree> {
    // Filters adjusted for resvg, which draws them (through krilla-svg), to filter as Skia: at
    // krilla-svg's resolution for an SVG drawn at its own size, four pixels a point.
    let resolution = krilla_svg::SvgSettings::default().filter_scale * crate::SCALE;
    // Displacement maps that tainted colors make pass their inputs through.
    let untainted = std::str::from_utf8(data).ok().and_then(crate::tainted::rewrite);
    let data = untainted.as_ref().map_or(data, String::as_bytes);
    let filtered = crate::filtered::adjusted_filters(data, resolution);
    let data = filtered.as_ref().map_or(data, String::as_bytes);
    let masked = crate::clip_text::rewrite(data).filter(|masked| {
        let options = usvg::roxmltree::ParsingOptions {
            allow_dtd: true,
            ..usvg::roxmltree::ParsingOptions::default()
        };
        usvg::roxmltree::Document::parse_with_options(masked, options).is_ok()
    });
    let data = masked.as_ref().map_or(data, String::as_bytes);
    let rewritten = crate::non_scaling::rewrite(data, width, |data| parse_text(data, fonts));
    rewritten
        .and_then(|rewritten| parse_text(rewritten.as_bytes(), fonts))
        .or_else(|| parse_text(data, fonts))
}

/// The document as it prints, where that differs from what usvg reads: with the `@media` rules
/// of its style sheets resolved for print (see [`media_rules`]), the colors of
/// `light-dark()` resolved (see [`light_colors`]) and the root's `background-color` hidden from
/// usvg (see [`without_root_background`]).
fn resolve_media(data: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(data).ok()?;
    let media = media_rules(text);
    let light = light_colors(media.as_deref().unwrap_or(text)).or(media);
    without_root_background(light.as_deref().unwrap_or(text)).or(light)
}

/// `text` with the `background-color` of its root, in its `style` attribute or as an attribute,
/// renamed so that usvg does not see it; `None` where it has none. usvg fills the `viewBox` with
/// it; browsers have no such attribute, and paint the root's CSS background as the canvas's,
/// over all of the box an SVG image is drawn in (see [`root_background`]), and that of an inline
/// SVG as that of its box.
fn without_root_background(text: &str) -> Option<String> {
    const NAME: &str = "background-color";
    let tag = root_tag(text)?;
    let start = tag.as_ptr() as usize - text.as_ptr() as usize;
    let mut inserts: Vec<(usize, &str)> = Vec::new();
    if let Some((_, value)) = attribute_at(tag, "style") {
        let lower = tag[value.clone()].to_ascii_lowercase();
        inserts.extend(
            lower
                .match_indices(NAME)
                .map(|(at, _)| (start + value.start + at, "-fagus-")),
        );
    }
    if let Some((whole, _)) = attribute_at(tag, NAME)
        && let Some(at) = tag[whole.clone()].find(NAME)
    {
        inserts.push((start + whole.start + at, "data-"));
    }
    if inserts.is_empty() {
        return None;
    }
    inserts.sort_unstable();
    let mut out = String::with_capacity(text.len() + inserts.len() * 8);
    let mut copied = 0;
    for (at, insert) in inserts {
        out.push_str(&text[copied..at]);
        out.push_str(insert);
        copied = at;
    }
    out.push_str(&text[copied..]);
    Some(out)
}

/// `text` with each `light-dark(light, dark)` replaced by its light color, as Chrome prints
/// it (draw.io writes its colors so); usvg reads no `light-dark()`, and a style attribute with
/// one is lost to it whole. `None` where it has none.
fn light_colors(text: &str) -> Option<String> {
    const FUNCTION: &str = "light-dark(";
    if !text.contains(FUNCTION) {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(FUNCTION) {
        out.push_str(&rest[..start]);
        let inner = &rest[start + FUNCTION.len()..];
        // The first argument ends at the first comma outside parentheses, the function at the
        // first closing parenthesis outside them.
        let (mut depth, mut comma, mut end) = (0_usize, None, None);
        for (index, character) in inner.char_indices() {
            match character {
                '(' => depth += 1,
                ')' if depth == 0 => {
                    end = Some(index);
                    break;
                }
                ')' => depth -= 1,
                ',' if depth == 0 && comma.is_none() => comma = Some(index),
                _ => {}
            }
        }
        let (Some(comma), Some(end)) = (comma, end) else {
            out.push_str(&rest[start..]);
            return Some(out);
        };
        let light = inner[..comma].trim();
        out.push_str(light_colors(light).as_deref().unwrap_or(light));
        rest = &inner[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

/// The document with the `@media` rules of its style sheets resolved for print, where it has
/// any: the rules inside those that apply kept, the others left out (usvg leaves out all).
fn media_rules(text: &str) -> Option<String> {
    if !text.contains("@media") {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<style") {
        let Some(open) = rest[start..].find('>').map(|end| start + end + 1) else {
            break;
        };
        let Some(close) = rest[open..].find("</style").map(|end| open + end) else {
            break;
        };
        out.push_str(&rest[..open]);
        resolve_media_rules(
            &rest[open..close],
            &fagus_css::MediaEnvironment::default(),
            &mut out,
        );
        rest = &rest[close..];
    }
    out.push_str(rest);
    Some(out)
}

/// Writes `css` to `out` with its `@media` rules resolved for `environment`.
fn resolve_media_rules(css: &str, environment: &fagus_css::MediaEnvironment, out: &mut String) {
    let mut rest = css;
    while let Some(start) = find_outside_strings(rest, "@media") {
        out.push_str(&rest[..start]);
        let after = &rest[start + "@media".len()..];
        let Some(open) = find_outside_strings(after, "{") else {
            rest = &rest[start..];
            break;
        };
        let Some(close) = matching_brace(&after[open..]).map(|end| open + end) else {
            rest = &rest[start..];
            break;
        };
        if environment.matches(after[..open].trim()) {
            resolve_media_rules(&after[open + 1..close], environment, out);
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
}

/// The `@font-face` rules of the CSS of an SVG `<style>` element. Those of an inline SVG are
/// rules of the document, as in Chrome, which its text and the HTML take.
pub fn font_face_rules(css: &str) -> String {
    let mut out = String::new();
    let mut rest = css;
    while let Some(start) = find_outside_strings(rest, "@font-face") {
        let after = &rest[start..];
        let Some(open) = find_outside_strings(after, "{") else {
            break;
        };
        let Some(close) = matching_brace(&after[open..]).map(|end| open + end) else {
            break;
        };
        out.push_str(&after[..=close]);
        out.push('\n');
        rest = &after[close + 1..];
    }
    out
}

/// Where `needle` first stands in CSS outside strings and comments.
fn find_outside_strings(css: &str, needle: &str) -> Option<usize> {
    let mut index = 0;
    let bytes = css.as_bytes();
    while index < bytes.len() {
        match bytes[index] {
            quote @ (b'"' | b'\'') => {
                index += 1;
                while index < bytes.len() && bytes[index] != quote {
                    index += if bytes[index] == b'\\' { 2 } else { 1 };
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index = css[index + 2..]
                    .find("*/")
                    .map_or(bytes.len(), |end| index + 2 + end + 1);
            }
            _ if bytes[index..].starts_with(needle.as_bytes()) => return Some(index),
            _ => {}
        }
        index += 1;
    }
    None
}

/// Where the brace that closes the one `css` starts with stands in it.
fn matching_brace(css: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut offset = 0;
    loop {
        let rest = &css[offset..];
        let open = find_outside_strings(rest, "{");
        let close = find_outside_strings(rest, "}")?;
        match open {
            Some(open) if open < close => {
                depth += 1;
                offset += open + 1;
            }
            _ => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(offset + close);
                }
                offset += close + 1;
            }
        }
    }
}

/// How much larger than its `viewBox` an SVG with text on a path is parsed for drawing.
const TEXT_PATH_SCALE: f32 = 16.0;

/// Parses an SVG document to draw it into a box: its size is only its aspect ratio then. An SVG
/// with text on a path and a `viewBox` is parsed [`TEXT_PATH_SCALE`] times larger: usvg finds
/// the places of the glyphs along the path to half a pixel at the size it parses, where Chrome
/// is exact.
pub(crate) fn parse_to_draw(data: &[u8], fonts: &FontDb) -> Option<Tree> {
    let resolved = resolve_media(data);
    let scaled = scaled(resolved.as_ref().map_or(data, String::as_bytes));
    match scaled {
        Some(text) => parse_effects(text.as_bytes(), data, fonts).or_else(|| parse(data, fonts)),
        None => parse(data, fonts),
    }
}

/// Whether an SVG document has shapes with a non-scaling stroke (`vector-effect`).
pub(crate) fn has_non_scaling_stroke(data: &[u8]) -> bool {
    data.windows(b"non-scaling-stroke".len())
        .any(|window| window == b"non-scaling-stroke")
}

/// [`parse_to_draw`] for an SVG drawn `width` CSS pixels wide rather than at its natural width:
/// its non-scaling strokes as wide as they say at that size, as Chrome draws them.
pub(crate) fn parse_drawn(data: &[u8], fonts: &FontDb, width: f32) -> Option<Tree> {
    let resolved = resolve_media(data);
    let resolved = resolved.as_ref().map_or(data, String::as_bytes);
    let scaled = scaled(resolved);
    effects_at(
        scaled.as_ref().map_or(resolved, String::as_bytes),
        Some(width),
        fonts,
    )
}

/// Whether the root of an inline SVG without a `viewBox` has a `width` or `height` in
/// percentages: its content then takes its lengths in percentages from the size of the box it
/// is drawn in, which only layout knows (see [`parse_sized`]).
pub(crate) fn has_relative_size(data: &[u8]) -> bool {
    let text = String::from_utf8_lossy(&data[..data.len().min(64 * 1024)]);
    root_tag(&text).is_some_and(|tag| {
        attribute(tag, "viewBox").is_none()
            && ["width", "height"]
                .iter()
                .any(|name| attribute(tag, name).is_some_and(|value| value.trim_end().ends_with('%')))
    })
}

/// Whether the root of an SVG image has neither a `viewBox` nor a `width` or `height` other
/// than a percentage: browsers lay it out at the size of the box it is drawn in (its concrete
/// object size), which its percentages are of, rather than stretch it there (see
/// [`parse_sized`]), so that its strokes keep their widths.
pub(crate) fn fills_its_box(data: &[u8]) -> bool {
    let text = String::from_utf8_lossy(&data[..data.len().min(64 * 1024)]);
    root_tag(&text).is_some_and(|tag| {
        attribute(tag, "viewBox").is_none()
            && ["width", "height"]
                .iter()
                .all(|name| attribute(tag, name).is_none_or(|value| value.trim_end().ends_with('%')))
    })
}

/// The background color that the `style` attribute of an SVG document's root gives it: the
/// root's background is the canvas's, which browsers fill all of the image's box with (its
/// viewport, beyond the `viewBox`), under what the SVG draws; usvg leaves CSS backgrounds out.
pub(crate) fn root_background(data: &[u8]) -> Option<fagus_style::values::Rgba> {
    use fagus_css::lightningcss::properties::Property;
    use fagus_css::lightningcss::values::color::CssColor;
    let text = String::from_utf8_lossy(&data[..data.len().min(64 * 1024)]);
    let style = attribute(root_tag(&text)?, "style")?;
    let block = fagus_css::parse_declarations(style)?;
    let mut color = None;
    for property in block
        .declarations
        .declarations
        .iter()
        .chain(&block.declarations.important_declarations)
    {
        match property {
            Property::BackgroundColor(value) => color = Some(value.clone()),
            Property::Background(layers) => color = layers.last().map(|layer| layer.color.clone()),
            _ => {}
        }
    }
    let Ok(CssColor::RGBA(rgba)) = color?.to_rgb() else {
        return None;
    };
    (rgba.alpha > 0).then(|| fagus_style::values::Rgba {
        r: f32::from(rgba.red) / 255.0,
        g: f32::from(rgba.green) / 255.0,
        b: f32::from(rgba.blue) / 255.0,
        a: f32::from(rgba.alpha) / 255.0,
    })
}

/// Whether the `viewBox` of an SVG's root is empty, which disables rendering it (SVG 2,
/// §8.6): it draws nothing.
pub(crate) fn has_empty_view_box(data: &[u8]) -> bool {
    let text = String::from_utf8_lossy(&data[..data.len().min(64 * 1024)]);
    root_tag(&text)
        .and_then(|tag| attribute(tag, "viewBox"))
        .is_some_and(|view_box| {
            let numbers: Option<Vec<f32>> = view_box
                .split(|c: char| c.is_whitespace() || c == ',')
                .filter(|part| !part.is_empty())
                .map(|part| part.parse().ok())
                .collect();
            matches!(numbers.as_deref(), Some([_, _, width, height]) if *width == 0.0 || *height == 0.0)
        })
}

/// [`parse_to_draw`] for an inline SVG drawn in a box `width` by `height` CSS pixels large: its
/// root as large as the box, as Chrome lays out the root of an inline SVG, so that lengths in
/// percentages in it (a `<line x2="100%">`) are of that size and its `viewBox` fits the box.
pub(crate) fn parse_sized(data: &[u8], fonts: &FontDb, width: f32, height: f32) -> Option<Tree> {
    let text = std::str::from_utf8(data).ok()?;
    parse_to_draw(sized_root(text, width, height)?.as_bytes(), fonts)
}

/// [`parse_to_draw`] for an inline SVG (at the size of its box where `sized` gives it) whose
/// drawing reaches `margins` (top, right, bottom, left) beyond its viewport, `size` large in its
/// own units: its root nested, unclipped (`overflow="visible"`), in a root that is larger by
/// the margins, its `viewBox` starting at minus the top and left margins, so that the content
/// lies where it did and the margins show what it draws there.
pub(crate) fn parse_unclipped(
    data: &[u8],
    fonts: &FontDb,
    sized: Option<(f32, f32)>,
    size: (f32, f32),
    margins: [f32; 4],
) -> Option<Tree> {
    let text = std::str::from_utf8(data).ok()?;
    let text = match sized {
        Some((width, height)) => std::borrow::Cow::Owned(sized_root(text, width, height)?),
        None => std::borrow::Cow::Borrowed(text),
    };
    parse_to_draw(unclipped_root(&text, size, margins)?.as_bytes(), fonts)
}

/// The document `text` with its root, `size` large, nested in a root larger by `margins` (see
/// [`parse_unclipped`]).
fn unclipped_root(text: &str, (width, height): (f32, f32), margins: [f32; 4]) -> Option<String> {
    let tag = root_tag(text)?;
    // An empty root draws nothing.
    if tag.ends_with('/') {
        return None;
    }
    let [top, right, bottom, left] = margins;
    let (outer_width, outer_height) = (width + left + right, height + top + bottom);
    let mut inner = tag.to_owned();
    for name in ["x", "y", "width", "height", "overflow"] {
        if let Some((span, _)) = attribute_at(&inner, name) {
            inner.replace_range(span, "");
        }
    }
    // Its own style may clip it too.
    if let Some((_, value)) = attribute_at(&inner, "style") {
        inner.insert_str(value.end, ";overflow:visible");
    }
    let _ = write!(
        inner,
        " x=\"0\" y=\"0\" width=\"{width}\" height=\"{height}\" overflow=\"visible\""
    );
    let start = tag.as_ptr() as usize - text.as_ptr() as usize;
    let mut out = String::with_capacity(text.len() + 256);
    out.push_str(&text[..start - 1]);
    let _ = write!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" \
         width=\"{outer_width}\" height=\"{outer_height}\" viewBox=\"{} {} {outer_width} {outer_height}\"><",
        0.0 - left + 0.0,
        0.0 - top + 0.0
    );
    out.push_str(&inner);
    out.push_str(&text[start + tag.len()..]);
    out.push_str("</svg>");
    Some(out)
}

/// The document `text` with the `width` and `height` of its root set to `width` and `height`.
fn sized_root(text: &str, width: f32, height: f32) -> Option<String> {
    let tag = root_tag(text)?;
    let mut new_tag = tag.to_owned();
    for name in ["width", "height"] {
        if let Some((span, _)) = attribute_at(&new_tag, name) {
            new_tag.replace_range(span, "");
        }
    }
    let _ = write!(new_tag, " width=\"{width}\" height=\"{height}\"");
    let start = tag.as_ptr() as usize - text.as_ptr() as usize;
    let mut out = String::with_capacity(text.len() + 32);
    out.push_str(&text[..start]);
    out.push_str(&new_tag);
    out.push_str(&text[start + tag.len()..]);
    Some(out)
}

/// The SVG image `data` as the fragment of its URL shows it (SVG 2, §5.6, "Linking into SVG
/// content"), as Chrome draws sprites: `#svgView(viewBox(…);preserveAspectRatio(…))` and the
/// `id` of a `<view>` element set the root's `viewBox` and `preserveAspectRatio`; the `id` of
/// another element is the target of `:target` in the image's style sheets. `None` where the
/// fragment changes nothing.
pub fn with_fragment(data: &[u8], fragment: &str) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(data).ok()?;
    let fragment = fragment.trim();
    if fragment.is_empty() {
        return None;
    }
    if let Some(spec) = fragment
        .strip_prefix("svgView(")
        .and_then(|spec| spec.strip_suffix(')'))
    {
        let mut attributes = Vec::new();
        for part in spec.split(';') {
            let part = part.trim();
            if let Some(view_box) = part.strip_prefix("viewBox(").and_then(|v| v.strip_suffix(')')) {
                attributes.push(("viewBox", view_box.replace(',', " ")));
            } else if let Some(ratio) = part
                .strip_prefix("preserveAspectRatio(")
                .and_then(|v| v.strip_suffix(')'))
            {
                attributes.push(("preserveAspectRatio", ratio.replace(',', " ")));
            }
        }
        return (!attributes.is_empty())
            .then(|| with_root_attributes(text, &attributes))
            .flatten();
    }
    // The start tag of the element of that id.
    let tag = ["\"", "'"].iter().find_map(|quote| {
        let at = text.find(&format!("id={quote}{fragment}{quote}"))?;
        let start = text[..at].rfind('<')?;
        let end = start + text[start..].find('>')?;
        Some(&text[start + 1..end])
    });
    if let Some(tag) = tag
        && tag.split(|c: char| c.is_whitespace() || c == '/').next() == Some("view")
    {
        let attributes: Vec<(&str, String)> = ["viewBox", "preserveAspectRatio"]
            .into_iter()
            .filter_map(|name| Some((name, attribute(&format!(" {tag}"), name)?.to_owned())))
            .collect();
        return (!attributes.is_empty())
            .then(|| with_root_attributes(text, &attributes))
            .flatten();
    }
    // `:target` in the style sheets matches the element of that id.
    let ident = fragment
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_');
    if !ident || tag.is_none() || !text.contains(":target") {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find("<style") {
        let Some(close) = rest[open..].find("</style>").map(|close| open + close) else {
            break;
        };
        out.push_str(&rest[..open]);
        out.push_str(&rest[open..close].replace(":target", &format!("#{fragment}")));
        rest = &rest[close..];
    }
    out.push_str(rest);
    (out != text).then(|| out.into_bytes())
}

/// The `<use>` elements of SVG markup that refer to an element of another document by URL
/// (`<use href="sprite.svg#icon">`): each start tag's span, the attribute value's span, and the
/// document and the id it names.
fn external_use_tags(markup: &str) -> Vec<(std::ops::Range<usize>, &str, &str)> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = markup[from..].find("<use").map(|at| from + at) {
        from = at + 4;
        if !markup[from..].starts_with(|c: char| c.is_whitespace()) {
            continue;
        }
        let Some(end) = markup[from..].find('>').map(|end| from + end) else {
            break;
        };
        let tag = &markup[at + 1..end];
        for name in ["href", "xlink:href"] {
            if let Some((_, value)) = attribute_at(tag, name) {
                let span = at + 1 + value.start..at + 1 + value.end;
                if let Some((document, id)) = markup[span.clone()].trim().split_once('#')
                    && !document.is_empty()
                    && !id.is_empty()
                {
                    found.push((span, document, id));
                }
            }
        }
        from = end;
    }
    found
}

/// The documents that the `<use>` elements of SVG markup refer to by URL, each once.
pub fn external_uses(markup: &str) -> Vec<&str> {
    let mut documents: Vec<&str> = external_use_tags(markup)
        .into_iter()
        .map(|(_, document, _)| document)
        .collect();
    documents.sort_unstable();
    documents.dedup();
    documents
}

/// SVG markup whose `<use>` elements refer to elements of the SVG documents `documents` (by
/// their URLs) by URL, as Chrome draws it: the content of each document (its symbols, their
/// gradients and style sheets) copied into `<defs>` with its ids prefixed so that they cannot
/// clash, and the `<use>` elements referring to the copies. `None` where none is resolved.
pub fn with_external_uses(markup: &str, documents: &[(&str, &[u8])]) -> Option<String> {
    let tags = external_use_tags(markup);
    let mut defs = String::new();
    let mut prefixes: FxHashMap<&str, Rc<str>> = FxHashMap::default();
    // A document under several URLs (`icons.svg?v=1`, `icons.svg?v=2`) is copied in once.
    let mut copied: FxHashMap<ContentKey, (&[u8], Rc<str>)> = FxHashMap::default();
    for (index, &(url, data)) in documents.iter().enumerate() {
        let key = content_key(data);
        if let Some((known, prefix)) = copied.get(&key)
            && *known == data
        {
            prefixes.insert(url, Rc::clone(prefix));
            continue;
        }
        let Some(content) = std::str::from_utf8(data).ok().and_then(|text| {
            let tag = root_tag(text)?;
            let start = tag.as_ptr() as usize - text.as_ptr() as usize + tag.len() + 1;
            let end = text.rfind("</svg>")?;
            (start <= end && !tag.ends_with('/')).then(|| &text[start..end])
        }) else {
            continue;
        };
        let prefix: Rc<str> = format!("fagus-use-{index}-").into();
        defs.push_str(&prefixed_ids(content, &prefix));
        copied.insert(key, (data, Rc::clone(&prefix)));
        prefixes.insert(url, prefix);
    }
    let mut out = String::with_capacity(markup.len() + defs.len() + 32);
    let mut last = 0;
    let mut changed = false;
    for (span, document, id) in tags {
        let Some(prefix) = prefixes.get(document) else {
            continue;
        };
        out.push_str(&markup[last..span.start]);
        out.push('#');
        out.push_str(prefix);
        out.push_str(id);
        last = span.end;
        changed = true;
    }
    if !changed {
        return None;
    }
    out.push_str(&markup[last..]);
    let close = out.rfind("</svg>")?;
    out.insert_str(close, &format!("<defs>{defs}</defs>"));
    Some(out)
}

/// The `<image>` elements of SVG markup that refer to an image by URL rather than by a `data:`
/// URL: the span of each one's reference and the URL.
fn external_image_tags(markup: &str) -> Vec<(std::ops::Range<usize>, &str)> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = markup[from..].find("<image").map(|at| from + at) {
        from = at + 6;
        if !markup[from..].starts_with(|c: char| c.is_whitespace()) {
            continue;
        }
        let Some(end) = markup[from..].find('>').map(|end| from + end) else {
            break;
        };
        let tag = &markup[at + 1..end];
        // `href` wins over `xlink:href` (SVG 2, §5.6.2).
        if let Some((_, value)) = attribute_at(tag, "href").or_else(|| attribute_at(tag, "xlink:href")) {
            let span = at + 1 + value.start..at + 1 + value.end;
            let url = markup[span.clone()].trim();
            if !url.is_empty()
                && !url.starts_with('#')
                && !url
                    .get(..5)
                    .is_some_and(|scheme| scheme.eq_ignore_ascii_case("data:"))
            {
                found.push((span, url));
            }
        }
        from = end;
    }
    found
}

/// The images that the `<image>` elements of SVG markup refer to by URL, each once: an inline
/// `<svg>` loads them as the document does its images (an SVG drawn as an image loads none).
pub fn external_images(markup: &str) -> Vec<&str> {
    let mut urls: Vec<&str> = external_image_tags(markup)
        .into_iter()
        .map(|(_, url)| url)
        .collect();
    urls.sort_unstable();
    urls.dedup();
    urls
}

/// SVG markup whose `<image>` elements refer to the images `images` (by their URLs, with their
/// data) by URL, those referring to them by `data:` URLs instead; `None` where none is.
pub fn with_external_images(markup: &str, images: &[(&str, &[u8])]) -> Option<String> {
    let images: FxHashMap<&str, (&[u8], ContentKey)> = images
        .iter()
        .map(|&(url, data)| (url, (data, content_key(data))))
        .collect();
    let mut out = String::with_capacity(markup.len());
    let mut last = 0;
    let mut written = rustc_hash::FxHashSet::default();
    let mut repeated = 0_usize;
    for (span, url) in external_image_tags(markup) {
        let Some(&(data, key)) = images.get(url) else {
            continue;
        };
        // Each image is written for each element that shows it; those shown again are written
        // up to a limit (an image of a megabyte shown thousands of times took gigabytes).
        if !written.insert(key) {
            repeated += data.len();
            if repeated > MAX_REPEATED_IMAGES {
                continue;
            }
        }
        // usvg tells raster formats apart by their bytes where the type is `text/plain`.
        let mime = if is_svg(data) {
            "image/svg+xml"
        } else {
            "text/plain"
        };
        out.push_str(&markup[last..span.start]);
        out.push_str("data:");
        out.push_str(mime);
        out.push_str(";base64,");
        base64_into(data, &mut out);
        last = span.end;
    }
    if last == 0 {
        return None;
    }
    out.push_str(&markup[last..]);
    Some(out)
}

/// How many bytes of images [`with_external_images`] writes into an SVG again for the elements
/// that show an image another one shows, at most; the elements past it are left out.
const MAX_REPEATED_IMAGES: usize = 64 << 20;

/// A key for data: its hash and its length.
type ContentKey = (u64, usize);

/// The [`ContentKey`] of `data`.
fn content_key(data: &[u8]) -> ContentKey {
    use std::hash::{Hash, Hasher};
    let mut hasher = rustc_hash::FxHasher::default();
    data.hash(&mut hasher);
    (hasher.finish(), data.len())
}

/// Appends `data` in Base64 (RFC 4648, §4, with padding) to `out`.
fn base64_into(data: &[u8], out: &mut String) {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    out.reserve(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let bytes = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let bits = u32::from(bytes[0]) << 16 | u32::from(bytes[1]) << 8 | u32::from(bytes[2]);
        for index in 0..4 {
            if index <= chunk.len() {
                out.push(char::from(ALPHABET[(bits >> (18 - 6 * index) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
}

/// SVG markup with `prefix` before every id it defines (`id="…"`) and refers to (`href="#…"`,
/// `url(#…)`).
fn prefixed_ids(markup: &str, prefix: &str) -> String {
    const PATTERNS: [&str; 7] = [
        "id=\"", "id='", "href=\"#", "href='#", "url(#", "url('#", "url(\"#",
    ];
    let mut out = String::with_capacity(markup.len() + 256);
    // Where each pattern is next, found again only once passed: a sprite of thousands of ids
    // was searched to its end for each of them.
    let mut next = PATTERNS.map(|pattern| markup.find(pattern));
    let mut cursor = 0;
    loop {
        let Some((index, at)) = next
            .iter()
            .enumerate()
            .filter_map(|(index, at)| Some((index, (*at)?)))
            .min_by_key(|&(_, at)| at)
        else {
            out.push_str(&markup[cursor..]);
            return out;
        };
        let pattern = PATTERNS[index];
        let end = at + pattern.len();
        // An `id` attribute stands after white space (not `grid="…"`).
        let attribute = pattern.starts_with("id=");
        let applies = !attribute || markup[cursor..at].ends_with(|c: char| c.is_whitespace());
        out.push_str(&markup[cursor..end]);
        if applies {
            out.push_str(prefix);
        }
        cursor = end;
        for (pattern, at) in PATTERNS.iter().zip(&mut next) {
            if at.is_some_and(|at| at < cursor) {
                *at = markup[cursor..].find(pattern).map(|found| cursor + found);
            }
        }
    }
}

/// The document `text` with the attributes `attributes` of its root set.
fn with_root_attributes(text: &str, attributes: &[(&str, String)]) -> Option<Vec<u8>> {
    let tag = root_tag(text)?;
    let mut new_tag = tag.to_owned();
    for (name, value) in attributes {
        if let Some((span, _)) = attribute_at(&new_tag, name) {
            new_tag.replace_range(span, "");
        }
        let _ = write!(new_tag, " {name}=\"{}\"", value.replace('"', ""));
    }
    let start = tag.as_ptr() as usize - text.as_ptr() as usize;
    let mut out = String::with_capacity(text.len() + 64);
    out.push_str(&text[..start]);
    out.push_str(&new_tag);
    out.push_str(&text[start + tag.len()..]);
    Some(out.into_bytes())
}

/// The document as [`parse_to_draw`] parses it where that differs from the document: with its
/// root scaled for text on a path.
fn scaled(data: &[u8]) -> Option<String> {
    std::str::from_utf8(data)
        .ok()
        .filter(|text| text.contains("<textPath"))
        .and_then(scaled_root)
}

/// The document `text` with the `width` and `height` of its root set to its `viewBox` size
/// times [`TEXT_PATH_SCALE`]; `None` without a `viewBox`.
fn scaled_root(text: &str) -> Option<String> {
    let tag = root_tag(text)?;
    let numbers: Vec<f32> = attribute(tag, "viewBox")?
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|part| !part.is_empty())
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    let [_, _, width, height] = numbers[..] else {
        return None;
    };
    if !(width > 0.0 && height > 0.0) {
        return None;
    }
    let mut new_tag = tag.to_owned();
    for name in ["width", "height"] {
        if let Some((span, _)) = attribute_at(&new_tag, name) {
            new_tag.replace_range(span, "");
        }
    }
    new_tag += &[
        " width=\"",
        &(width * TEXT_PATH_SCALE).to_string(),
        "\" height=\"",
    ]
    .concat();
    new_tag += &[&(height * TEXT_PATH_SCALE).to_string(), "\""].concat();
    let start = tag.as_ptr() as usize - text.as_ptr() as usize;
    let mut out = String::with_capacity(text.len() + 32);
    out.push_str(&text[..start]);
    out.push_str(&new_tag);
    out.push_str(&text[start + tag.len()..]);
    Some(out)
}

/// How an SVG fills a box of another aspect ratio than its own.
pub use fagus_layout::SvgFit as Fit;

/// How an SVG (an `inline` one, or an image) fills a box, from the start tag of its root
/// element.
pub fn fit(data: &[u8], inline: bool) -> Fit {
    let decompressed;
    let data = if data.starts_with(&[0x1f, 0x8b]) {
        decompressed = usvg::decompress_svgz(data).ok();
        match &decompressed {
            Some(data) => data.as_slice(),
            None => return Fit::Stretch,
        }
    } else {
        data
    };
    let text = String::from_utf8_lossy(&data[..data.len().min(64 * 1024)]);
    let Some(tag) = root_tag(&text) else {
        return Fit::Stretch;
    };
    if attribute(tag, "viewBox").is_none() {
        return if inline { Fit::Unscaled } else { Fit::Stretch };
    }
    let mut cover = false;
    let (mut x, mut y) = (0.5, 0.5);
    for word in attribute(tag, "preserveAspectRatio")
        .unwrap_or_default()
        .split_whitespace()
    {
        match word {
            "none" => return Fit::Stretch,
            "slice" => cover = true,
            "meet" | "defer" => {}
            align if align.len() == 8 && align.starts_with('x') => {
                let fraction = |part: &str| match part {
                    "Min" => Some(0.0),
                    "Mid" => Some(0.5),
                    "Max" => Some(1.0),
                    _ => None,
                };
                if let (Some(across), "Y", Some(down)) =
                    (fraction(&align[1..4]), &align[4..5], fraction(&align[5..8]))
                {
                    (x, y) = (across, down);
                }
            }
            _ => {}
        }
    }
    Fit::Uniform { cover, x, y }
}

/// The XML document `text` from its root element on: after the XML declaration, comments and
/// the document type.
pub fn from_root(text: &str) -> Option<&str> {
    let mut rest = text;
    loop {
        rest = &rest[rest.find('<')?..];
        if rest.starts_with("<?") {
            rest = &rest[rest.find("?>")? + 2..];
        } else if rest.starts_with("<!--") {
            rest = &rest[rest.find("-->")? + 3..];
        } else if rest.starts_with("<!") {
            // A document type, with an internal subset in brackets perhaps.
            let end = match (rest.find('['), rest.find('>')) {
                (Some(bracket), Some(close)) if bracket < close => bracket + rest[bracket..].find("]>")? + 2,
                (_, Some(close)) => close + 1,
                _ => return None,
            };
            rest = &rest[end..];
        } else {
            return Some(rest);
        }
    }
}

/// The start tag of the root element of an XML document, without `<` and `>`.
pub(crate) fn root_tag(text: &str) -> Option<&str> {
    let rest = from_root(text)?;
    let mut quote = None;
    for (index, character) in rest.char_indices() {
        match (quote, character) {
            (None, '"' | '\'') => quote = Some(character),
            (Some(open), _) if character == open => quote = None,
            (None, '>') => return Some(&rest[1..index]),
            _ => {}
        }
    }
    None
}

/// The value of attribute `name` on the first element of `markup`.
pub(crate) fn root_attribute<'a>(markup: &'a str, name: &str) -> Option<&'a str> {
    attribute(root_tag(markup)?, name)
}

/// The value of attribute `name` in a start tag.
pub(crate) fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    attribute_at(tag, name).map(|(_, value)| &tag[value])
}

/// Where attribute `name` is in a start tag: from the white space before it to the end of its
/// quoted value, and its value.
pub(crate) fn attribute_at(
    tag: &str,
    name: &str,
) -> Option<(std::ops::Range<usize>, std::ops::Range<usize>)> {
    let rest = tag;
    let mut quote = None;
    let mut index = 0;
    // Names stand outside quoted values, after white space: XML's, of one byte each (a no-break
    // space is part of a name).
    while index < rest.len() {
        let character = rest[index..].chars().next()?;
        match (quote, character) {
            (None, '"' | '\'') => quote = Some(character),
            (Some(open), _) if character == open => quote = None,
            (None, ' ' | '\t' | '\n' | '\r') if rest[index + 1..].starts_with(name) => {
                let after_name = index + 1 + name.len();
                let after = rest[after_name..].trim_start();
                if let Some(value) = after.strip_prefix('=') {
                    let value = value.trim_start();
                    let open = value.chars().next()?;
                    if open == '"' || open == '\'' {
                        let start = rest.len() - value.len() + 1;
                        return value[1..]
                            .find(open)
                            .map(|end| (index..start + end + 1, start..start + end));
                    }
                }
            }
            _ => {}
        }
        index += character.len_utf8();
    }
    None
}

/// The natural size of an SVG image in CSS pixels, as browsers take it from its root (CSS Images
/// 3, §4.1): its `width` and `height` where they are absolute lengths (a percentage or a missing
/// size is none), and the ratio of those sizes where both are, else of its `viewBox`.
pub fn natural_size(data: &[u8]) -> (Option<f32>, Option<f32>, Option<f32>) {
    natural_size_of(data, false)
}

/// The natural size of an SVG shown as an image (`<img>`, a background image): as
/// [`natural_size`], except that an image whose root lacks a width or a height and has
/// `preserveAspectRatio="none"` has no natural size and no ratio at all, so that it takes the
/// default object size (300 × 150 pixels for `<img>`, the background area), as in Chrome
/// (Blink's `SVGImage::GetIntrinsicSizingInfo`).
pub fn image_natural_size(data: &[u8]) -> (Option<f32>, Option<f32>, Option<f32>) {
    natural_size_of(data, true)
}

fn natural_size_of(data: &[u8], image: bool) -> (Option<f32>, Option<f32>, Option<f32>) {
    let decompressed;
    let data = if data.starts_with(&[0x1f, 0x8b]) {
        decompressed = usvg::decompress_svgz(data).ok();
        match &decompressed {
            Some(data) => data.as_slice(),
            None => return (None, None, None),
        }
    } else {
        data
    };
    let text = String::from_utf8_lossy(&data[..data.len().min(64 * 1024)]);
    let Some(tag) = root_tag(&text) else {
        return (None, None, None);
    };
    let width = attribute(tag, "width").and_then(absolute_length);
    let height = attribute(tag, "height").and_then(absolute_length);
    let view_box = attribute(tag, "viewBox").and_then(|view_box| {
        let numbers: Vec<f32> = view_box
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|part| !part.is_empty())
            .map(str::parse)
            .collect::<Result<_, _>>()
            .ok()?;
        match numbers[..] {
            [_, _, width, height] if width > 0.0 && height > 0.0 => Some(width / height),
            _ => None,
        }
    });
    // Both sizes give the ratio, else the `viewBox` does (as Blink's `LayoutSVGRoot`).
    let ratio = match (width, height) {
        (Some(width), Some(height)) if width > 0.0 && height > 0.0 => Some(width / height),
        _ => view_box,
    };
    if image
        && (width.is_none() || height.is_none())
        && attribute(tag, "preserveAspectRatio").is_some_and(|value| value.trim().starts_with("none"))
    {
        return (None, None, None);
    }
    (width, height, ratio)
}

/// An absolute length of an SVG attribute in CSS pixels; `em` counts 16 pixels, as for an SVG
/// image in a browser.
fn absolute_length(value: &str) -> Option<f32> {
    let value = value.trim();
    let split = value
        .find(|c: char| c.is_ascii_alphabetic() || c == '%')
        .unwrap_or(value.len());
    let (number, unit) = value.split_at(split);
    let number: f32 = number.trim().parse().ok()?;
    let scale = match unit.to_ascii_lowercase().as_str() {
        "" | "px" => 1.0,
        "pt" => 4.0 / 3.0,
        "pc" => 16.0,
        "in" => 96.0,
        "cm" => 96.0 / 2.54,
        "mm" => 96.0 / 25.4,
        "q" => 96.0 / 101.6,
        "em" => 16.0,
        _ => return None,
    };
    (number.is_finite() && number >= 0.0).then_some(number * scale)
}

/// Draws an SVG into pixels with resvg; `None` where tiny-skia panics on its geometry (a mask of
/// shapes of a trillion pixels drawn on a mask of its own), and the pixels are then not to be
/// used.
pub(crate) fn render(
    tree: &Tree,
    transform: tiny_skia::Transform,
    pixmap: &mut tiny_skia::PixmapMut<'_>,
) -> Option<()> {
    fagus_css::guarded(|| resvg::render(tree, transform, pixmap))
}

/// The alpha channel of an SVG image drawn at its size, for `shape-outside`: Chrome takes the
/// float area from the image as it draws it (Blink's `Shape::CreateRasterShape`). `None` where
/// it cannot be drawn.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the size is checked to be of at most 64M pixels"
)]
pub fn alpha(image: &fagus_layout::Image, fonts: &FontDb) -> Option<fagus_layout::AlphaMask> {
    let tree = parse(&image.data, fonts)?;
    let (columns, rows) = (image.width.round(), image.height.round());
    if !(columns >= 1.0 && rows >= 1.0 && columns * rows <= 64.0 * 1024.0 * 1024.0) {
        return None;
    }
    let (width, height) = (columns as u32, rows as u32);
    let mut pixmap = tiny_skia::Pixmap::new(width, height)?;
    let natural = (tree.size().width(), tree.size().height());
    let (x, y, drawn_width, drawn_height) = fit(&image.data, false).place(natural, columns, rows);
    let transform =
        tiny_skia::Transform::from_row(drawn_width / natural.0, 0.0, 0.0, drawn_height / natural.1, x, y);
    render(&tree, transform, &mut pixmap.as_mut())?;
    let alpha = pixmap
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| pixel[3])
        .collect();
    Some(fagus_layout::AlphaMask { width, height, alpha })
}

/// The pixels of an SVG drawn into a box of `width` × `height` pixels as `fit` places it, as
/// unpremultiplied RGBA: for filters that change its pixels.
pub(crate) fn pixels(tree: &Tree, fit: Fit, width: f32, height: f32) -> Option<(u32, u32, Vec<u8>)> {
    let (columns, rows) = (width.round(), height.round());
    if !(columns >= 1.0 && rows >= 1.0 && columns * rows <= 64.0 * 1024.0 * 1024.0) {
        return None;
    }
    let (columns, rows) = (columns as u32, rows as u32);
    let mut pixmap = tiny_skia::Pixmap::new(columns, rows)?;
    let natural = (tree.size().width(), tree.size().height());
    let (x, y, drawn_width, drawn_height) = fit.place(natural, columns as f32, rows as f32);
    let transform =
        tiny_skia::Transform::from_row(drawn_width / natural.0, 0.0, 0.0, drawn_height / natural.1, x, y);
    render(tree, transform, &mut pixmap.as_mut())?;
    let mut pixels = pixmap.take();
    for pixel in pixels.as_chunks_mut::<4>().0 {
        let alpha = pixel[3];
        if alpha > 0 && alpha < 255 {
            for channel in &mut pixel[..3] {
                *channel =
                    ((u32::from(*channel) * 255 + u32::from(alpha) / 2) / u32::from(alpha)).min(255) as u8;
            }
        }
    }
    Some((columns, rows, pixels))
}

/// Where an inline SVG puts its `<foreignObject>` elements (written as groups that mark their
/// places, see [`fagus_dom::FOREIGN_OBJECT`]): the size of the SVG as it is drawn, how it fills
/// its box, and for each element the index in its group's id, the transform from its user space
/// to the drawn SVG, and its `x`, `y`, `width` and `height`, in the order the SVG draws them.
/// Elements the SVG does not draw (`display: none`, the branches of a `<switch>` not taken) and
/// those of no size are left out.
#[allow(clippy::type_complexity, reason = "plain data for the caller to map")]
pub fn foreign_objects(
    data: &Arc<Vec<u8>>,
    fonts: &FontDb,
    images: &crate::Images,
) -> Option<((f32, f32), Fit, Vec<(usize, [f32; 6], [f32; 4])>)> {
    fn walk(group: &usvg::Group, out: &mut Vec<(usize, [f32; 6], [f32; 4])>) {
        for node in group.children() {
            let usvg::Node::Group(group) = node else {
                continue;
            };
            let index = group
                .id()
                .strip_prefix(fagus_dom::FOREIGN_OBJECT)
                .and_then(|index| index.parse::<usize>().ok());
            match index {
                Some(index) => {
                    let Some(usvg::Node::Path(path)) = group.children().first() else {
                        continue;
                    };
                    let bounds = path.data().bounds();
                    let transform = group.abs_transform();
                    if !out.iter().any(|(known, ..)| *known == index) {
                        out.push((
                            index,
                            [
                                transform.sx,
                                transform.ky,
                                transform.kx,
                                transform.sy,
                                transform.tx,
                                transform.ty,
                            ],
                            [bounds.x(), bounds.y(), bounds.width(), bounds.height()],
                        ));
                    }
                }
                None => walk(group, out),
            }
        }
    }
    if !data
        .windows(fagus_dom::FOREIGN_OBJECT.len())
        .any(|window| window == fagus_dom::FOREIGN_OBJECT.as_bytes())
    {
        return None;
    }
    let tree = parse_kept(data, fonts, images, true, true)?;
    let mut objects = Vec::new();
    walk(tree.root(), &mut objects);
    (!objects.is_empty()).then(|| {
        (
            (tree.size().width(), tree.size().height()),
            fit(data, true),
            objects,
        )
    })
}

/// The size of an SVG image (or an `inline` SVG element) in CSS pixels.
/// Whether the SVG image `data` draws nothing at all (it holds definitions only).
pub fn draws_nothing(data: &Arc<Vec<u8>>, fonts: &FontDb, images: &crate::Images) -> bool {
    parse_kept(data, fonts, images, scaled(data).is_none(), false)
        .is_some_and(|tree| !tree.root().has_children())
}

pub fn size(data: &Arc<Vec<u8>>, fonts: &FontDb, images: &crate::Images, inline: bool) -> Option<(f32, f32)> {
    // Parsed as it is drawn where that is the same, so that drawing finds the tree.
    let tree = parse_kept(data, fonts, images, scaled(data).is_none(), inline)?;
    Some((tree.size().width(), tree.size().height()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_css_background_of_the_root_for_the_whole_box() {
        let green = Some(fagus_style::values::Rgba {
            r: 0.0,
            g: 128.0 / 255.0,
            b: 0.0,
            a: 1.0,
        });
        let svg = |root: &str| {
            format!("<?xml version='1.0'?><svg xmlns='http://www.w3.org/2000/svg' {root}><rect/></svg>")
        };
        assert_eq!(
            root_background(svg("style='background: green'").as_bytes()),
            green
        );
        assert_eq!(
            root_background(svg("style='background: yellow url(a.png); background-color: green'").as_bytes()),
            green
        );
        assert_eq!(
            root_background(svg("style='background: green !important; background-color: red'").as_bytes()),
            green
        );
        assert_eq!(
            root_background(svg("style='background: transparent'").as_bytes()),
            None
        );
        assert_eq!(root_background(svg("fill='green'").as_bytes()), None);
        // usvg, which would fill the `viewBox` with a `background-color`, does not see it.
        assert_eq!(
            without_root_background(&svg("background-color='red' style='Background-Color: green'")),
            Some(svg(
                "data-background-color='red' style='-fagus-Background-Color: green'"
            ))
        );
        assert_eq!(without_root_background(&svg("style='fill: red'")), None);
    }

    #[test]
    fn caps_and_separates_morphology_radii_where_drawn() {
        // A unit of user space takes ten CSS pixels: Chrome reaches 81.92 CSS pixels at most.
        let svg = "<filter id='f'><feMorphology operator='dilate' radius='1e9 0.2' result='r'/>\
                   <feMorphology radius='0.3'/></filter>\
                   <filter primitiveUnits='objectBoundingBox'><feMorphology radius='5'/></filter>";
        let drawn = drawn_morphology(svg.as_bytes(), 10.0);
        let drawn = std::str::from_utf8(&drawn).unwrap();
        let (max, thin) = (MAX_MORPHOLOGY_RADIUS / 10.0, 0.01_f32 / 10.0);
        assert_eq!(
            drawn,
            format!(
                "<filter id='f'><feMorphology operator='dilate' radius=\"{max} {thin}\" \
                 result=\"fagus-morphology-0\"/><feMorphology operator='dilate' \
                 radius=\"{thin} 0.2\" result='r' in=\"fagus-morphology-0\"/>\
                 <feMorphology radius='0.3'/></filter>\
                 <filter primitiveUnits='objectBoundingBox'><feMorphology radius='5'/></filter>"
            )
        );
        // Usvg reads what it draws.
        let svg = format!(
            "<svg xmlns='http://www.w3.org/2000/svg' width='10' height='10'>{}<rect width='5' \
             height='5' filter='url(#f)'/></svg>",
            &drawn[..drawn.find("<filter primitiveUnits").unwrap()]
        );
        let tree = Tree::from_str(&svg, &Options::default()).unwrap();
        let (reach, scale) = morphology_reach(&tree, 10.0);
        assert!((reach - MAX_MORPHOLOGY_RADIUS).abs() < 0.01, "{reach}");
        assert!((scale - 10.0).abs() < 1e-4);
        // Nothing to do for small radii, and none in what is not text.
        assert!(matches!(
            drawn_morphology(b"<feMorphology radius='3'/>", 1.0),
            std::borrow::Cow::Borrowed(_)
        ));
        assert!(matches!(
            drawn_morphology(&[0xff, 0xfe], 1.0),
            std::borrow::Cow::Borrowed(_)
        ));
    }

    #[test]
    fn caps_the_octaves_of_turbulence_as_blink() {
        let svg = b"<feTurbulence numOctaves = '1000000000'/><feTurbulence numOctaves=\"3\"/>\
                    <feTurbulence numOctaves=\" 1e9 \"/>";
        assert_eq!(
            &*capped_octaves(svg),
            b"<feTurbulence numOctaves = '9'/><feTurbulence numOctaves=\"3\"/>\
              <feTurbulence numOctaves=\"9\"/>"
        );
        assert!(matches!(capped_octaves(b"<svg/>"), std::borrow::Cow::Borrowed(_)));
    }

    #[test]
    fn caps_arcs_that_usvg_would_cut_into_millions_of_curves() {
        let svg = b"<circle r='1e38'/><ellipse rx=\"-2e20px\" ry='5'/><path d='M0 0 A1 1 0 1 0 1e30.5 0'/>\
                    <path d='M0 0 L 1e30 0'/><text x='1e30' id='r=1e30'>1e30</text>";
        assert_eq!(
            &*capped_arcs(svg),
            &b"<circle r='1e12'/><ellipse rx=\"-1e12px\" ry='5'/><path d='M0 0 A1 1 0 1 0 1e12.5 0'/>\
               <path d='M0 0 L 1e30 0'/><text x='1e30' id='r=1e30'>1e30</text>"[..]
        );
        assert!(matches!(
            capped_arcs(b"<circle r='10'/>"),
            std::borrow::Cow::Borrowed(_)
        ));
    }

    #[test]
    fn leaves_out_images_in_svgs_that_do_not_decode() {
        let resolve = decodable_images();
        let options = Options::default();
        let gif = Arc::new(
            b"GIF89a\x01\x00\x01\x00\x00\xff\x00,\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x00;".to_vec(),
        );
        assert!(resolve("image/gif", gif, &options).is_none());
    }

    #[test]
    fn prints_the_light_colors_of_light_dark() {
        let svg = "<rect fill=\"light-dark(#fff, #000)\" style=\"fill: light-dark(rgb(1, 2, 3), \
                   var(--dark, #121212)); stroke: light-dark(light-dark(red, blue), green)\"/>";
        assert_eq!(
            light_colors(svg).as_deref(),
            Some("<rect fill=\"#fff\" style=\"fill: rgb(1, 2, 3); stroke: red\"/>")
        );
        assert_eq!(light_colors("<rect fill=\"red\"/>"), None);
        // Unclosed: left as it is.
        assert_eq!(
            light_colors("a light-dark(red").as_deref(),
            Some("a light-dark(red")
        );
    }

    #[test]
    fn takes_the_alpha_of_svg_images_for_shapes() {
        // The left half of a 4 × 2 SVG is opaque.
        let data = b"<svg xmlns='http://www.w3.org/2000/svg' width='4' height='2'><rect width='2' height='2'/></svg>";
        let image = fagus_layout::Image {
            data: Arc::new(data.to_vec()),
            format: fagus_layout::ImageFormat::Svg,
            width: 4.0,
            height: 2.0,
            natural: (true, true),
            ratio: Some(2.0),
            inline: false,
            orientation: 1,
            filters: Arc::from([]),
            pad: 0,
        };
        let mask = alpha(&image, &FontDb::new()).expect("drawn");
        assert_eq!((mask.width, mask.height), (4, 2));
        assert_eq!(mask.alpha, [255, 255, 0, 0, 255, 255, 0, 0]);
    }

    #[test]
    fn shows_what_an_inline_svg_draws_beyond_its_viewport() {
        let fonts = FontDb::new();
        let markup = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50" viewBox="0 0 10 5" style="color: red"><rect x="-2" width="15" height="5"/></svg>"#;
        let tree = parse_to_draw(markup, &fonts).expect("a tree");
        let drawn = tree.root().abs_layer_bounding_box();
        assert!((drawn.left() + 20.0).abs() < 0.01 && (drawn.right() - 130.0).abs() < 0.01);
        let text = std::str::from_utf8(markup).expect("text");
        let wrapped = unclipped_root(text, (100.0, 50.0), [0.0, 31.0, 0.0, 21.0]).expect("wrapped");
        assert!(wrapped.contains("viewBox=\"-21 0 152 50\""));
        assert!(wrapped.contains("style=\"color: red;overflow:visible\""));
        let tree =
            parse_unclipped(markup, &fonts, None, (100.0, 50.0), [0.0, 31.0, 0.0, 21.0]).expect("a tree");
        assert!((tree.size().width() - 152.0).abs() < 0.01);
        // The content stays where it was, now inside the larger viewport.
        let drawn = tree.root().abs_layer_bounding_box();
        assert!((drawn.left() - 1.0).abs() < 0.01 && (drawn.right() - 151.0).abs() < 0.01);
        // An empty root draws nothing to show.
        assert_eq!(unclipped_root("<svg/>", (1.0, 1.0), [1.0; 4]), None);
    }

    #[test]
    fn knows_svgs_laid_out_at_the_size_of_their_box_and_those_that_draw_nothing() {
        assert!(fills_its_box(
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"><rect/></svg>"
        ));
        assert!(fills_its_box(b"<svg width=\"100%\" height=\"50%\"></svg>"));
        assert!(!fills_its_box(b"<svg width=\"10\"></svg>"));
        assert!(!fills_its_box(b"<svg viewBox=\"0 0 10 10\"></svg>"));
        assert!(has_empty_view_box(
            b"<svg width=\"8px\" viewBox=\"0 0 8 0\"></svg>"
        ));
        assert!(has_empty_view_box(b"<svg viewBox=\"0,0,0,8\"></svg>"));
        assert!(!has_empty_view_box(b"<svg viewBox=\"0 0 8 8\"></svg>"));
        assert!(!has_empty_view_box(b"<svg width=\"8\"></svg>"));
    }

    #[test]
    fn lays_out_inline_svgs_at_the_size_of_their_box() {
        // KaTeX's `\cancel`: a line across a root as wide as its box.
        let cancel = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100%\" height=\"0.8306em\">\
                      <line x1=\"0\" y1=\"100%\" x2=\"100%\" y2=\"0\" stroke-width=\"0.046em\"/></svg>";
        assert!(has_relative_size(cancel.as_bytes()));
        assert!(!has_relative_size(
            b"<svg width=\"100%\" viewBox=\"0 0 10 10\"></svg>"
        ));
        assert!(!has_relative_size(b"<svg width=\"10\" height=\"1em\"></svg>"));
        let sized = sized_root(cancel, 10.5, 14.75).expect("root");
        let tag = root_tag(&sized).expect("root");
        assert_eq!(
            (attribute(tag, "width"), attribute(tag, "height")),
            (Some("10.5"), Some("14.75"))
        );
        assert!(
            sized.ends_with(
                "<line x1=\"0\" y1=\"100%\" x2=\"100%\" y2=\"0\" stroke-width=\"0.046em\"/></svg>"
            )
        );
        let mut fonts = FontDb::new();
        fonts.add_default_fonts();
        let tree = parse_sized(cancel.as_bytes(), &fonts, 10.5, 14.75).expect("tree");
        assert_eq!((tree.size().width(), tree.size().height()), (10.5, 14.75));
        // The line ends at the right edge of the box.
        let bounds = tree.root().abs_bounding_box();
        assert!((bounds.right() - 10.5).abs() < 0.01, "{bounds:?}");
    }

    #[test]
    fn shows_svg_images_as_the_fragment_of_their_url_says() {
        let sprite = "<svg xmlns=\"http://www.w3.org/2000/svg\"><view id=\"phone\" viewBox=\"0 0 24 24\"/>\
                      <rect id=\"r\" width=\"5\" height=\"5\"/></svg>";
        let root = |data: Vec<u8>, name: &str| {
            let text = String::from_utf8(data).expect("text");
            root_attribute(&text, name).map(str::to_owned)
        };
        // A `<view>` and `svgView()` set the root's `viewBox`.
        let viewed = with_fragment(sprite.as_bytes(), "phone").expect("view");
        assert_eq!(root(viewed, "viewBox").as_deref(), Some("0 0 24 24"));
        let viewed = with_fragment(
            sprite.as_bytes(),
            "svgView(viewBox(0,0,12,12);preserveAspectRatio(xMinYMin slice))",
        )
        .expect("svgView");
        assert_eq!(root(viewed.clone(), "viewBox").as_deref(), Some("0 0 12 12"));
        assert_eq!(
            root(viewed, "preserveAspectRatio").as_deref(),
            Some("xMinYMin slice")
        );
        // Another element is the target of `:target` (an SVG stack); without such a rule, or an
        // element of that id, nothing changes.
        assert_eq!(with_fragment(sprite.as_bytes(), "r"), None);
        assert_eq!(with_fragment(sprite.as_bytes(), "missing"), None);
        let stack = "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>g { display: none } g:target { display: inline }</style>\
                     <g id=\"red\"><circle r=\"5\"/></g></svg>";
        let targeted =
            String::from_utf8(with_fragment(stack.as_bytes(), "red").expect("target")).expect("text");
        assert!(targeted.contains("g#red { display: inline }"), "{targeted}");
    }

    #[test]
    fn copies_the_symbols_that_use_takes_from_other_documents() {
        let markup = "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" \
                      viewBox=\"0 0 24 24\"><use href=\"icons.svg#pin\"></use>\
                      <use xlink:href=\"#local\"></use></svg>";
        assert_eq!(external_uses(markup), ["icons.svg"]);
        let icons = "<?xml version=\"1.0\"?><svg xmlns=\"http://www.w3.org/2000/svg\"><defs>\
                     <linearGradient id=\"g\"><stop offset=\"0\"/></linearGradient></defs>\
                     <symbol id=\"pin\" viewBox=\"0 0 24 24\"><path d=\"M0 0h9\" fill=\"url(#g)\"/></symbol></svg>";
        let resolved = with_external_uses(markup, &[("icons.svg", icons.as_bytes())]).expect("resolved");
        assert!(
            resolved.contains("<use href=\"#fagus-use-0-pin\"></use><use xlink:href=\"#local\"></use>"),
            "{resolved}"
        );
        assert!(resolved.contains("<symbol id=\"fagus-use-0-pin\" viewBox=\"0 0 24 24\"><path d=\"M0 0h9\" fill=\"url(#fagus-use-0-g)\"/>"), "{resolved}");
        assert!(resolved.ends_with("</symbol></defs></svg>"), "{resolved}");
        // The symbol is drawn.
        let mut fonts = FontDb::new();
        fonts.add_default_fonts();
        let tree = parse(resolved.as_bytes(), &fonts).expect("tree");
        assert!(tree.root().has_children());
        assert_eq!(with_external_uses(markup, &[]), None);
    }

    #[test]
    fn copies_in_the_images_that_image_elements_refer_to() {
        let markup = "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\">\
                      <image href=\"logo.png\" width=\"10\" height=\"10\"/>\
                      <image xlink:href=\"logo.png\" x=\"10\"/><image href=\"data:image/png;base64,AA==\"/>\
                      <image href=\"#local\"/></svg>";
        assert_eq!(external_images(markup), ["logo.png"]);
        let resolved = with_external_images(markup, &[("logo.png", b"\x89PNG\r\n")]).expect("resolved");
        assert!(
            resolved.contains(
                "<image href=\"data:text/plain;base64,iVBORw0K\" width=\"10\" height=\"10\"/>\
                               <image xlink:href=\"data:text/plain;base64,iVBORw0K\" x=\"10\"/>"
            ),
            "{resolved}"
        );
        assert!(
            resolved.ends_with("<image href=\"data:image/png;base64,AA==\"/><image href=\"#local\"/></svg>")
        );
        assert_eq!(with_external_images(markup, &[]), None);
        let mut encoded = String::new();
        base64_into(b"ab", &mut encoded);
        base64_into(b"a", &mut encoded);
        assert_eq!(encoded, "YWI=YQ==");
    }

    #[test]
    fn fits_by_the_root_aspect_ratio() {
        let svg = |attributes: &str| {
            format!(
                "<?xml version=\"1.0\"?><!-- logo --><svg xmlns=\"http://www.w3.org/2000/svg\" width=\"240\" height=\"80\" {attributes}><title>preserveAspectRatio=\"none\"</title></svg>"
            )
        };
        let meet = Fit::Uniform {
            cover: false,
            x: 0.5,
            y: 0.5,
        };
        assert_eq!(fit(svg("viewBox=\"0 0 240 80\"").as_bytes(), false), meet);
        assert_eq!(fit(svg("").as_bytes(), false), Fit::Stretch, "no viewBox");
        assert_eq!(fit(svg("").as_bytes(), true), Fit::Unscaled, "inline, no viewBox");
        assert_eq!(
            fit(
                svg("viewBox='0 0 240 80' preserveAspectRatio='xMinYMax slice'").as_bytes(),
                false
            ),
            Fit::Uniform {
                cover: true,
                x: 0.0,
                y: 1.0
            }
        );
        assert_eq!(
            fit(
                svg("data-x=' viewBox=\"1\"' viewBox=\"0 0 1 1\" preserveAspectRatio=\"none\"").as_bytes(),
                false
            ),
            Fit::Stretch
        );
        // A 240 × 80 logo in a square: as wide as the box, centered.
        assert_eq!(meet.place((240.0, 80.0), 48.0, 48.0), (0.0, 16.0, 48.0, 16.0));
    }

    #[test]
    fn reads_natural_sizes_like_browsers() {
        let natural = |attributes: &str| {
            natural_size(format!("<svg xmlns=\"http://www.w3.org/2000/svg\" {attributes}></svg>").as_bytes())
        };
        assert_eq!(
            natural("width='120' height='3pt'"),
            (Some(120.0), Some(4.0), Some(30.0))
        );
        // A percentage is no natural size; the viewBox gives the ratio.
        assert_eq!(
            natural("width='100%' viewBox='0 0 40 10'"),
            (None, None, Some(4.0))
        );
        assert_eq!(natural("width='2cm'"), (Some(96.0 / 2.54 * 2.0), None, None));
        // Both sizes give the ratio, whatever the `viewBox`.
        assert_eq!(
            natural("width='50' height='25' viewBox='0 0 1000 1000'"),
            (Some(50.0), Some(25.0), Some(2.0))
        );
        assert_eq!(natural(""), (None, None, None));
        // As an image, one without a size and with `preserveAspectRatio="none"` has neither a
        // size nor a ratio, as in Chrome; with both sizes, it keeps them.
        let image = |attributes: &str| {
            image_natural_size(
                format!("<svg xmlns=\"http://www.w3.org/2000/svg\" {attributes}></svg>").as_bytes(),
            )
        };
        let stretched = "viewBox='0 0 1000 500' preserveAspectRatio='none'";
        assert_eq!(image(&format!("height='25' {stretched}")), (None, None, None));
        assert_eq!(image(&format!("width='50' {stretched}")), (None, None, None));
        assert_eq!(image(stretched), (None, None, None));
        assert_eq!(
            image(&format!("width='50' height='25' {stretched}")),
            (Some(50.0), Some(25.0), Some(2.0))
        );
        assert_eq!(
            image("height='25' viewBox='0 0 1000 500'"),
            (None, Some(25.0), Some(2.0))
        );
    }

    #[test]
    fn reads_size_without_touching_files() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="120" height="40" viewBox="0 0 60 20">
            <rect width="60" height="20" fill="#2563eb"/><image xlink:href="/etc/hosts" width="10" height="10"/></svg>"##;
        assert!(is_svg(svg));
        assert!(!is_svg(b"\x89PNG"));
        let images = crate::Images::default();
        assert_eq!(
            size(&Arc::new(svg.to_vec()), &FontDb::new(), &images, false),
            Some((120.0, 40.0))
        );
        let tree = parse(svg, &FontDb::new()).expect("svg");
        // The image that points to a file was dropped, not read.
        assert_eq!(tree.root().children().len(), 1);
    }

    /// The spans of the text of a tree, with their fonts' families and whether they are
    /// stroked.
    fn spans(tree: &Tree) -> Vec<(String, bool)> {
        fn walk(group: &usvg::Group, out: &mut Vec<(String, bool)>) {
            for node in group.children() {
                match node {
                    usvg::Node::Group(group) => walk(group, out),
                    usvg::Node::Text(text) => {
                        for chunk in text.chunks() {
                            for span in chunk.spans() {
                                let family = span
                                    .font()
                                    .families()
                                    .first()
                                    .map(|family| family.to_string().replace('"', ""));
                                out.push((family.unwrap_or_default(), span.stroke().is_some()));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut out = Vec::new();
        walk(tree.root(), &mut out);
        out
    }

    #[test]
    fn sets_text_in_the_document_fonts_with_synthetic_bold() {
        let mut fonts = FontDb::new();
        fonts
            .add_font_data(
                std::fs::read(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../tests/fonts/Inter-Regular.ttf"
                ))
                .expect("font"),
            )
            .expect("Inter");
        let svg = |weight: u16| {
            format!(
                "<svg xmlns='http://www.w3.org/2000/svg' width='100' height='20'>\
                 <text y='15' font-family='Inter' font-weight='{weight}' fill='#123456'>Fagus</text></svg>"
            )
        };
        // Inter has no bold face: bold text is outlined in its fill.
        let bold = parse(svg(700).as_bytes(), &fonts).expect("svg");
        assert_eq!(spans(&bold), [("Inter".to_owned(), true)]);
        let regular = parse(svg(400).as_bytes(), &fonts).expect("svg");
        assert_eq!(spans(&regular), [("Inter".to_owned(), false)]);
        // Only the faces that text takes are loaded.
        assert_eq!(bold.fontdb().len(), 1);
        let italic = "<svg xmlns='http://www.w3.org/2000/svg' width='100' height='20'>\
                      <text y='15' font-family='Inter' font-style='italic'>Fagus</text></svg>";
        let written = parse(italic.as_bytes(), &fonts)
            .expect("svg")
            .to_string(&usvg::WriteOptions::default());
        assert!(written.contains("matrix(1 0 -0.25 1 3.75 0)"), "{written}");
    }

    #[test]
    fn finds_font_face_rules() {
        let css = "text { fill: red } @font-face { font-family: A; src: url('a{.ttf') }\
                   /* @font-face { font-family: B } */ @font-face{font-family:C;src:url(data:font/ttf;base64,AA)}";
        assert_eq!(
            font_face_rules(css),
            "@font-face { font-family: A; src: url('a{.ttf') }\n@font-face{font-family:C;src:url(data:font/ttf;base64,AA)}\n"
        );
        assert_eq!(font_face_rules("rect { fill: red }"), "");
    }

    #[test]
    fn resolves_media_rules_for_print() {
        let svg = b"<svg xmlns='http://www.w3.org/2000/svg'><style>rect { fill: red }\
                    @media print { .screen { display: none } }\
                    @media screen { rect { fill: blue } } /* @media */ circle { fill: 'x{' }</style></svg>";
        let resolved = resolve_media(svg).expect("@media");
        assert!(resolved.contains("rect { fill: red } .screen { display: none } "));
        assert!(!resolved.contains("blue"));
        assert!(resolved.ends_with("/* @media */ circle { fill: 'x{' }</style></svg>"));
        assert_eq!(resolve_media(b"<svg/>"), None);
    }

    #[test]
    fn does_not_parse_svgs_whose_clips_reach_past_what_f32_holds() {
        let parsed = |body: &str| {
            let svg =
                format!("<svg xmlns='http://www.w3.org/2000/svg' width='200' height='200'>{body}</svg>");
            parse_tree(svg.as_bytes(), &Options::default()).is_ok()
        };
        // Nested viewports scaled by 1e30 five times over: krilla cannot transform their clips.
        let nested = "<svg width='50' height='50' transform='scale(1e30)'>".repeat(5);
        assert!(!parsed(&format!(
            "{nested}<rect width='5' height='5'/>{}",
            "</svg>".repeat(5)
        )));
        assert!(!parsed(
            "<g transform='scale(1e20)'><g transform='scale(1e20)'><rect width='9' height='9' \
             clip-path='url(#c)'/></g></g><clipPath id='c'><rect width='3' height='3'/></clipPath>"
        ));
        // Large ones within it parse.
        assert!(parsed(
            "<svg width='50' height='50' transform='scale(1e9)'><rect width='5' height='5'/></svg>"
        ));
        assert!(parsed(
            "<clipPath id='c'><rect width='1e20' height='1e20'/></clipPath><rect width='9' height='9' clip-path='url(#c)'/>"
        ));
    }
}

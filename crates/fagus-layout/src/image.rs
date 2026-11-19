//! Images: layout only needs their format and size; the PDF writer embeds the original data.

use std::sync::Arc;

use fagus_dom::NodeId;
use rustc_hash::FxHashMap;

/// The largest image Fagus accepts, in pixels: decoding a larger one could exhaust memory.
pub const MAX_PIXELS: u64 = 200_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Gif,
    Webp,
    /// BMP, read as Chrome reads it (see [`crate::bmp`]).
    Bmp,
    /// TIFF, which Chrome does not show and Safari does: the first image of the file.
    Tiff,
    /// SVG, drawn as vector graphics; its size comes from the SVG's own `width`, `height` and
    /// `viewBox`.
    Svg,
}

/// An image file with its intrinsic size.
#[derive(Clone, Debug)]
pub struct Image {
    pub data: Arc<Vec<u8>>,
    pub format: ImageFormat,
    /// Intrinsic size in CSS pixels: one image pixel per CSS pixel, as in browsers. For an SVG
    /// without `width` or `height`, the size its `viewBox` gives it, which only places what it
    /// draws.
    pub width: f32,
    pub height: f32,
    /// Whether `width` and `height` are natural sizes. An SVG without the attributes has none;
    /// its box takes its size from the box's style and surroundings, with the SVG's ratio at
    /// most (CSS Images 3, §4.1).
    pub natural: (bool, bool),
    /// The natural ratio, width divided by height, if the image has one.
    pub ratio: Option<f32>,
    /// An inline `<svg>` element rather than an SVG image: without a `viewBox` it is drawn at
    /// its own scale and cut off at its box, where an SVG image stretches to its box, as in
    /// browsers.
    pub inline: bool,
    /// How the pixels are turned to be seen (the EXIF orientation, 1 to 8, 1 as stored), as
    /// browsers turn them (`image-orientation: from-image`); `width` and `height` are those of
    /// the turned image.
    pub orientation: u8,
    /// What its pixels go through before they are embedded, in order (`filter`).
    pub filters: Arc<[PixelFilter]>,
    /// Transparent pixels around the image, on every side, that make room for a blur.
    pub pad: u32,
}

/// What a raster image's pixels go through for `filter`, or to be a part of a border image.
#[derive(Clone, Debug, PartialEq)]
pub enum PixelFilter {
    /// Only the pixels within these bounds (left, top, right and bottom, fractions of the image
    /// as it is seen), as an image of their own: a part of a border image.
    Part([f32; 4]),
    /// A filter that maps colors pixel by pixel.
    Color(fagus_style::values::Filter),
    /// A Gaussian blur with this standard deviation, in pixels of the image.
    Blur(f32),
    /// The image's alpha in one color: the shadow that `drop-shadow()` makes of it.
    Silhouette(fagus_style::values::Rgba),
    /// Only the part of the image where its box clips it, before a blur: within `bounds` (left,
    /// top, right and bottom), with rounded corners of `radii`, all as fractions of its width
    /// and height as it is seen.
    Crop {
        bounds: [f32; 4],
        radii: Option<[(f32, f32); 4]>,
    },
}

/// Why an image could not be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageError {
    /// Not a PNG, JPEG, GIF, WebP, BMP or TIFF image.
    Unsupported,
    /// More than 200 megapixels.
    TooLarge,
}

impl PartialEq for Image {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.data, &other.data) || self.data == other.data
    }
}

impl Image {
    /// Reads the format and size of image data without decoding it.
    pub fn probe(data: Arc<Vec<u8>>) -> Result<Self, ImageError> {
        let format = match imagesize::image_type(&data).map_err(|_| ImageError::Unsupported)? {
            imagesize::ImageType::Png => ImageFormat::Png,
            imagesize::ImageType::Jpeg => ImageFormat::Jpeg,
            imagesize::ImageType::Gif => ImageFormat::Gif,
            imagesize::ImageType::Webp => ImageFormat::Webp,
            imagesize::ImageType::Bmp => ImageFormat::Bmp,
            imagesize::ImageType::Tiff => ImageFormat::Tiff,
            _ => return Err(ImageError::Unsupported),
        };
        // imagesize reads the height of a top-down BMP (negative) and OS/2 headers wrong.
        let size = match format {
            ImageFormat::Bmp => crate::bmp::size(&data)
                .map(|(width, height)| imagesize::ImageSize {
                    width: width as usize,
                    height: height as usize,
                })
                .ok_or(ImageError::Unsupported)?,
            _ => imagesize::blob_size(&data).map_err(|_| ImageError::Unsupported)?,
        };
        if size.width as u64 * size.height as u64 > MAX_PIXELS {
            return Err(ImageError::TooLarge);
        }
        let orientation = exif_orientation(&data, format).unwrap_or(1);
        let (mut width, mut height) = (size.width as f32, size.height as f32);
        // Turned a quarter, the image is as wide as it is stored high.
        if orientation >= 5 {
            (width, height) = (height, width);
        }
        Ok(Self {
            data,
            format,
            width,
            height,
            natural: (true, true),
            ratio: (width > 0.0 && height > 0.0).then(|| width / height),
            inline: false,
            orientation,
            filters: Arc::from([]),
            pad: 0,
        })
    }

    /// The image as an element with `style` shows it: as stored with `image-orientation: none`.
    /// `image-rendering` changes nothing in print: Chrome writes every image without
    /// `/Interpolate`, smooth, pixelated, scaled up or down (Chrome 157, measured).
    /// The image at a resolution of `resolution` image pixels per CSS pixel (`image-set()`):
    /// its natural size divided by it.
    pub fn at_resolution(image: &Arc<Self>, resolution: f32) -> Arc<Self> {
        if (resolution - 1.0).abs() < 1e-6 || !resolution.is_finite() || resolution <= 0.0 {
            return Arc::clone(image);
        }
        let mut scaled = (**image).clone();
        scaled.width /= resolution;
        scaled.height /= resolution;
        Arc::new(scaled)
    }

    /// The image as an element with an effective zoom of `zoom` shows it: its natural size
    /// multiplied by the zoom, as an image pixel is that many CSS pixels then (CSS Viewport 1,
    /// §5).
    pub fn zoomed(image: &Arc<Self>, zoom: f32) -> Arc<Self> {
        if (zoom - 1.0).abs() < 1e-6 || !zoom.is_finite() || zoom <= 0.0 {
            return Arc::clone(image);
        }
        let mut zoomed = (**image).clone();
        zoomed.width *= zoom;
        zoomed.height *= zoom;
        Arc::new(zoomed)
    }

    pub fn styled(image: &Arc<Self>, style: &fagus_style::ComputedStyle) -> Arc<Self> {
        use fagus_style::values::ImageOrientation;
        let unturned = style.image_orientation == ImageOrientation::None && image.orientation > 1;
        if !unturned {
            return Arc::clone(image);
        }
        let mut styled = (**image).clone();
        if image.orientation >= 5 {
            (styled.width, styled.height) = (image.height, image.width);
            styled.ratio = image.ratio.map(f32::recip);
        }
        styled.orientation = 1;
        Arc::new(styled)
    }
}

/// The EXIF orientation of a JPEG (APP1), PNG (`eXIf`) or WebP (`EXIF`) image, or the
/// orientation of a TIFF image (its first IFD), if it has one from 1 to 8.
fn exif_orientation(data: &[u8], format: ImageFormat) -> Option<u8> {
    let be16 = |at: usize| Some(u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?));
    let tiff: &[u8] = match format {
        ImageFormat::Jpeg => {
            // The segments after SOI, up to the start of the scan.
            let mut at = 2;
            loop {
                if data.get(at) != Some(&0xFF) {
                    return None;
                }
                let marker = *data.get(at + 1)?;
                if marker == 0xDA || marker == 0xD9 {
                    return None;
                }
                let length = usize::from(be16(at + 2)?);
                let segment = data.get(at + 4..at + 2 + length)?;
                if marker == 0xE1 && segment.starts_with(b"Exif\0\0") {
                    break &segment[6..];
                }
                at += 2 + length;
            }
        }
        ImageFormat::Png => {
            let mut at = 8;
            loop {
                let length = u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?) as usize;
                let kind = data.get(at + 4..at + 8)?;
                if kind == b"eXIf" {
                    break data.get(at + 8..at + 8 + length)?;
                }
                if kind == b"IDAT" || kind == b"IEND" {
                    return None;
                }
                at = at.checked_add(12 + length)?;
            }
        }
        ImageFormat::Webp => {
            let mut at = 12;
            loop {
                let kind = data.get(at..at + 4)?;
                let length = u32::from_le_bytes(data.get(at + 4..at + 8)?.try_into().ok()?) as usize;
                if kind == b"EXIF" {
                    let exif = data.get(at + 8..at + 8 + length)?;
                    break exif.strip_prefix(b"Exif\0\0").unwrap_or(exif);
                }
                // Chunks are padded to even lengths.
                at = at.checked_add(8 + length + length % 2)?;
            }
        }
        ImageFormat::Tiff => data,
        ImageFormat::Gif | ImageFormat::Bmp | ImageFormat::Svg => return None,
    };
    // A TIFF header, then the first IFD: entries of tag, type, count and value.
    let little = match tiff.get(..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let u16_at = |at: usize| {
        let bytes: [u8; 2] = tiff.get(at..at + 2)?.try_into().ok()?;
        Some(if little {
            u16::from_le_bytes(bytes)
        } else {
            u16::from_be_bytes(bytes)
        })
    };
    let u32_at = |at: usize| {
        let bytes: [u8; 4] = tiff.get(at..at + 4)?.try_into().ok()?;
        Some(if little {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        })
    };
    let ifd = u32_at(4)? as usize;
    for entry in 0..usize::from(u16_at(ifd)?) {
        let at = ifd + 2 + entry * 12;
        if u16_at(at)? == 0x0112 {
            return u16_at(at + 8)
                .and_then(|value| u8::try_from(value).ok())
                .filter(|value| (1..=8).contains(value));
        }
    }
    None
}

/// The images of a document by URL, as written in the document.
#[derive(Debug, Default)]
pub struct Images {
    by_url: FxHashMap<String, Arc<Image>>,
    /// Inline `<svg>` elements as SVG images, by element.
    inline: FxHashMap<NodeId, Arc<Image>>,
    /// The alpha of images that `shape-outside` names, by URL.
    alphas: FxHashMap<String, Arc<crate::AlphaMask>>,
    /// The SVG elements that `clip-path`, `mask` and `filter` name by URL (`url(#id)`).
    references: FxHashMap<String, Arc<SvgReference>>,
    /// The `<foreignObject>` elements of inline `<svg>` elements, by the `<svg>` element.
    foreign: FxHashMap<NodeId, Arc<crate::ForeignObjects>>,
    /// SVG images with HTML in `<foreignObject>` elements as documents of their own, by the
    /// address of the image data (which they hold on to, so that it stays theirs).
    documents: FxHashMap<usize, (Arc<Vec<u8>>, Arc<SvgDocument>)>,
    /// The URLs of SVG images that draw nothing, which mask nothing (see [`Images::is_blank`]).
    blank: rustc_hash::FxHashSet<String>,
    /// The candidate an `<img>` shows from its `srcset` or the sources of its `<picture>`, by
    /// element: its URL and density (image pixels per CSS pixel).
    sources: FxHashMap<NodeId, (String, f32)>,
    /// The `<iframe>`, `<object>` and `<embed>` elements that show a document of their own,
    /// with the blank image that stands for it in layout (see [`crate::FrameDocument`]).
    frames: FxHashMap<NodeId, Arc<Image>>,
    /// The addresses of the data of the images of [`Images::frames`].
    frame_data: rustc_hash::FxHashSet<usize>,
    /// The documents of frames laid out in their boxes, by the address of the data of the
    /// frame's image (which they hold on to, so that it stays theirs).
    frame_documents: FxHashMap<usize, (Arc<Vec<u8>>, Arc<crate::FrameDocument>)>,
}

/// The data of the image that stands for the document of a frame: an SVG that draws nothing,
/// where the document is not drawn in its place.
const BLANK_FRAME: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"300\" height=\"150\"/>";

/// An SVG image with HTML in `<foreignObject>` elements, laid out as a document of its own, as
/// Chrome draws it: the SVG with only its own style sheets and installed fonts, the HTML laid out
/// in it, on one page of the image's size. It is drawn in place of the image, scaled into the
/// box as the image would be.
#[derive(Debug)]
pub struct SvgDocument {
    /// The page the SVG is laid out on, as large as [`SvgDocument::size`].
    pub page: crate::Page,
    /// The images of the document: the SVG itself, and those its HTML shows.
    pub images: Images,
    /// The size of the image in CSS pixels, which the page has.
    pub size: (f32, f32),
    /// How the image fills a box of another aspect ratio.
    pub fit: crate::SvgFit,
}

/// An SVG element that CSS names by URL, made ready for the boxes it applies to.
#[derive(Clone, Debug, PartialEq)]
pub enum SvgReference {
    /// A `<clipPath>` as one path.
    Clip(SvgClip),
    /// A `<clipPath>` (`clip`) or a `<mask>` that is drawn instead: `markup` is the element
    /// with what it refers to, which an SVG document of a box's size draws as the alpha that
    /// clips or masks the box.
    Markup { markup: Arc<str>, clip: bool },
    /// A `<filter>` as CSS filter functions.
    Filter(Arc<[fagus_style::values::Filter]>),
    /// A `<filter>` whose primitives CSS filter functions cannot express: `markup` is the
    /// element with what it refers to, which filters the pixels of the boxes it applies to.
    PixelFilter { markup: Arc<str> },
}

/// The outline of an SVG `<clipPath>`: in pixels from the top left corner of a box's border
/// box, or, with `bounding_box`, in fractions of its width and height.
#[derive(Clone, Debug, PartialEq)]
pub struct SvgClip {
    pub segments: Vec<PathSegment>,
    pub even_odd: bool,
    pub bounding_box: bool,
}

/// A part of an outline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathSegment {
    Move(f32, f32),
    Line(f32, f32),
    /// A cubic Bézier curve: two control points and the end.
    Cubic([f32; 6]),
    Close,
}

impl Images {
    pub fn insert_inline(&mut self, element: NodeId, image: Arc<Image>) {
        self.inline.insert(element, image);
    }

    /// The image of an inline `<svg>` element.
    pub fn inline(&self, element: NodeId) -> Option<&Arc<Image>> {
        self.inline.get(&element)
    }

    /// Keeps the `<foreignObject>` elements of the inline `<svg>` element `element`.
    pub fn insert_foreign(&mut self, element: NodeId, objects: Arc<crate::ForeignObjects>) {
        self.foreign.insert(element, objects);
    }

    /// The `<foreignObject>` elements of an inline `<svg>` element, if it has any.
    pub fn foreign(&self, element: NodeId) -> Option<&Arc<crate::ForeignObjects>> {
        self.foreign.get(&element)
    }

    /// Keeps the SVG image whose data is `data` as a document of its own (see
    /// [`SvgDocument`]).
    pub fn insert_document(&mut self, data: &Arc<Vec<u8>>, document: Arc<SvgDocument>) {
        self.documents
            .insert(Arc::as_ptr(data) as usize, (Arc::clone(data), document));
    }

    /// The SVG image with the data `data` as a document of its own, if it has HTML in it.
    pub fn document(&self, data: &Arc<Vec<u8>>) -> Option<&Arc<SvgDocument>> {
        self.documents
            .get(&(Arc::as_ptr(data) as usize))
            .filter(|(kept, _)| Arc::ptr_eq(kept, data))
            .map(|(_, document)| document)
    }

    /// The inline `<svg>` elements as SVG images, with the layers of those with
    /// `<foreignObject>` elements, in no particular order.
    pub fn inline_svgs(&self) -> impl Iterator<Item = &Arc<Image>> {
        self.inline
            .values()
            .chain(self.foreign.values().flat_map(|objects| objects.layers.iter()))
    }

    /// Whether any SVG image or frame is kept as a document of its own.
    pub fn has_documents(&self) -> bool {
        !self.documents.is_empty() || !self.frame_documents.is_empty()
    }

    /// Makes `element` a frame that shows a document of its own, and returns the image that
    /// stands for that document in layout: blank, 300 × 150 pixels without a ratio (HTML,
    /// §15.4.1), with data of its own by which painting finds the document laid out for it.
    pub fn insert_frame(&mut self, element: NodeId) -> Arc<Image> {
        let image = Arc::new(Image {
            data: Arc::new(BLANK_FRAME.to_vec()),
            format: ImageFormat::Svg,
            width: 300.0,
            height: 150.0,
            natural: (true, true),
            ratio: None,
            inline: false,
            orientation: 1,
            filters: Arc::from([]),
            pad: 0,
        });
        self.frame_data.insert(Arc::as_ptr(&image.data) as usize);
        if let Some(earlier) = self.frames.insert(element, Arc::clone(&image)) {
            self.frame_data.remove(&(Arc::as_ptr(&earlier.data) as usize));
        }
        image
    }

    /// Gives the image of the frame `element` the natural height `height` (that of its
    /// document, see [`crate::FrameDocument::responsive_height`]), and drops the documents laid
    /// out for frames, whose boxes change.
    pub fn resize_frame(&mut self, element: NodeId, height: f32) {
        if let Some(image) = self.frames.get_mut(&element) {
            *image = Arc::new(Image {
                height,
                ..(**image).clone()
            });
        }
        self.frame_documents.clear();
    }

    /// The image of the frame `element` (see [`Images::insert_frame`]), if it shows a document.
    pub fn frame(&self, element: NodeId) -> Option<&Arc<Image>> {
        self.frames.get(&element)
    }

    /// The frames that show documents, with their images, in no particular order.
    pub fn frames(&self) -> impl Iterator<Item = (NodeId, &Arc<Image>)> {
        self.frames.iter().map(|(&element, image)| (element, image))
    }

    /// Whether `data` is that of the image of a frame (see [`Images::insert_frame`]).
    pub fn is_frame(&self, data: &Arc<Vec<u8>>) -> bool {
        self.frame_data.contains(&(Arc::as_ptr(data) as usize)) || self.frame_document(data).is_some()
    }

    /// Keeps the document laid out for the frame whose image has the data `data`.
    pub fn insert_frame_document(&mut self, data: &Arc<Vec<u8>>, document: Arc<crate::FrameDocument>) {
        self.frame_documents
            .insert(Arc::as_ptr(data) as usize, (Arc::clone(data), document));
    }

    /// The document laid out for the frame whose image has the data `data`, if there is one.
    pub fn frame_document(&self, data: &Arc<Vec<u8>>) -> Option<&Arc<crate::FrameDocument>> {
        self.frame_documents
            .get(&(Arc::as_ptr(data) as usize))
            .filter(|(kept, _)| Arc::ptr_eq(kept, data))
            .map(|(_, document)| document)
    }

    pub fn insert(&mut self, url: impl Into<String>, image: Arc<Image>) {
        self.by_url.insert(url.into(), image);
    }

    /// Keeps the candidate the `<img>` element `element` shows from `srcset` or `<picture>`.
    pub fn insert_source(&mut self, element: NodeId, url: impl Into<String>, density: f32) {
        self.sources.insert(element, (url.into(), density));
    }

    /// The URL of the image an `<img>` element shows and its density: the candidate chosen from
    /// `srcset` or `<picture>`, or its `src` at a density of 1.
    pub fn source<'a>(&'a self, element: NodeId, data: &'a fagus_dom::ElementData) -> Option<(&'a str, f32)> {
        match self.sources.get(&element) {
            Some((url, density)) => Some((url.as_str(), *density)),
            None => data.attribute("src").map(|src| (src.trim(), 1.0)),
        }
    }

    /// Marks the SVG image at `url` as one that draws nothing.
    pub fn insert_blank(&mut self, url: impl Into<String>) {
        self.blank.insert(url.into());
    }

    /// Whether the image at `url` is an SVG image that draws nothing: as a mask image, Chrome
    /// takes it for one that is not loaded.
    pub fn is_blank(&self, url: &str) -> bool {
        self.blank.contains(url)
    }

    pub fn get(&self, url: &str) -> Option<&Arc<Image>> {
        self.by_url.get(url)
    }

    /// The images by their URLs, in no particular order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Arc<Image>)> {
        self.by_url.iter().map(|(url, image)| (url.as_str(), image))
    }

    pub fn contains(&self, url: &str) -> bool {
        self.by_url.contains_key(url)
    }

    /// Keeps the alpha of the image at `url`, for `shape-outside`.
    pub fn insert_alpha(&mut self, url: impl Into<String>, alpha: Arc<crate::AlphaMask>) {
        self.alphas.insert(url.into(), alpha);
    }

    /// The alpha of the image at `url`, if it was kept for `shape-outside`.
    pub fn alpha(&self, url: &str) -> Option<&Arc<crate::AlphaMask>> {
        self.alphas.get(url)
    }

    /// Keeps the SVG element that `url` names for `clip-path`, `mask` or `filter`.
    pub fn insert_reference(&mut self, url: impl Into<String>, reference: Arc<SvgReference>) {
        self.references.insert(url.into(), reference);
    }

    /// The SVG element that `url` names, if it was found and could be used.
    pub fn reference(&self, url: &str) -> Option<&Arc<SvgReference>> {
        self.references.get(url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probes_png_size() {
        // A 3 × 2 PNG header (signature and IHDR chunk).
        let mut data = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
        data.extend_from_slice(b"IHDR");
        data.extend_from_slice(&3u32.to_be_bytes());
        data.extend_from_slice(&2u32.to_be_bytes());
        data.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
        let image = Image::probe(Arc::new(data)).expect("png");
        assert_eq!(
            (image.format, image.width, image.height),
            (ImageFormat::Png, 3.0, 2.0)
        );
        assert_eq!(
            Image::probe(Arc::new(b"<html>".to_vec())).err(),
            Some(ImageError::Unsupported)
        );
    }

    #[test]
    fn probes_bmp_and_tiff_images() {
        // A top-down BMP (its height negative) of 3 × 2 pixels: imagesize reads its height as
        // 4 billion.
        let mut bmp = b"BM".to_vec();
        bmp.extend(66_u32.to_le_bytes());
        bmp.extend([0; 4]);
        bmp.extend(54_u32.to_le_bytes());
        bmp.extend(40_u32.to_le_bytes());
        bmp.extend(3_i32.to_le_bytes());
        bmp.extend((-2_i32).to_le_bytes());
        bmp.extend([1, 0, 24, 0]);
        bmp.extend([0; 24]);
        bmp.extend([0; 12]);
        let image = Image::probe(Arc::new(bmp)).expect("bmp");
        assert_eq!(
            (image.format, image.width, image.height),
            (ImageFormat::Bmp, 3.0, 2.0)
        );
        // A TIFF of 4 × 2 pixels, turned a quarter by its orientation tag (6).
        let mut tiff = b"II\x2a\0\x08\0\0\0\x03\0".to_vec();
        for (tag, value) in [(256_u16, 4_u16), (257, 2), (0x0112, 6)] {
            tiff.extend(tag.to_le_bytes());
            tiff.extend(3_u16.to_le_bytes());
            tiff.extend(1_u32.to_le_bytes());
            tiff.extend(u32::from(value).to_le_bytes());
        }
        tiff.extend([0; 4]);
        let image = Image::probe(Arc::new(tiff)).expect("tiff");
        assert_eq!(
            (image.format, image.orientation, image.width, image.height),
            (ImageFormat::Tiff, 6, 2.0, 4.0)
        );
    }

    #[test]
    fn turns_images_by_their_exif_orientation() {
        use fagus_style::values::{ImageOrientation, ImageRendering};
        // A JPEG header with an APP1 Exif segment (big-endian TIFF, orientation 6), then a
        // SOF0 frame of 4 × 2 pixels.
        let jpeg = |orientation: u16| {
            let mut tiff = b"MM\0\x2a\0\0\0\x08\0\x01".to_vec();
            tiff.extend_from_slice(&[0x01, 0x12, 0, 3, 0, 0, 0, 1]);
            tiff.extend_from_slice(&orientation.to_be_bytes());
            tiff.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
            let mut data = vec![0xFF, 0xD8, 0xFF, 0xE1];
            data.extend_from_slice(&u16::try_from(tiff.len() + 8).expect("short").to_be_bytes());
            data.extend_from_slice(b"Exif\0\0");
            data.extend_from_slice(&tiff);
            data.extend_from_slice(&[0xFF, 0xC0, 0, 11, 8, 0, 2, 0, 4, 1, 1, 0x11, 0]);
            data.extend_from_slice(&[0xFF, 0xD9]);
            Image::probe(Arc::new(data)).expect("jpeg")
        };
        let turned = jpeg(6);
        assert_eq!((turned.orientation, turned.width, turned.height), (6, 2.0, 4.0));
        let flipped = jpeg(2);
        assert_eq!(
            (flipped.orientation, flipped.width, flipped.height),
            (2, 4.0, 2.0)
        );
        assert_eq!(jpeg(9).orientation, 1, "no such orientation");
        // `image-orientation: none` shows the pixels as stored; `image-rendering` changes nothing.
        let turned = Arc::new(turned);
        let style = |orientation, rendering| fagus_style::ComputedStyle {
            image_orientation: orientation,
            image_rendering: rendering,
            ..fagus_style::ComputedStyle::default()
        };
        let stored = Image::styled(&turned, &style(ImageOrientation::None, ImageRendering::Auto));
        assert_eq!((stored.orientation, stored.width, stored.height), (1, 4.0, 2.0));
        for rendering in [ImageRendering::Auto, ImageRendering::Pixelated] {
            let same = Image::styled(&turned, &style(ImageOrientation::FromImage, rendering));
            assert!(Arc::ptr_eq(&same, &turned));
        }
    }
}

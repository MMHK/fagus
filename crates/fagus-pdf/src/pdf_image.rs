//! PDF files as images, as Safari and Prince show them (Chrome shows none): a page of the file,
//! drawn as a form `XObject` with its text, vector graphics, fonts and images as they are. This
//! is how a letterhead designed elsewhere becomes the background of the pages
//! (`@page { background: url(letterhead.pdf) }`).
//!
//! The page is the first, or the one the fragment of the URL names (`letterhead.pdf#page=2`,
//! RFC 8118). The image is as large as the page's crop box, with its `/Rotate` and `/UserUnit`,
//! a point (1/72 inch) for 4/3 CSS pixels. Links and other annotations of the page are not
//! taken over, nor its structure tree.

use std::io::Read;
use std::sync::Arc;

use fagus_layout::{Image, ImageFormat};
use hayro_syntax::object::{Array, Name, Stream};
use hayro_syntax::{LoadPdfError, Pdf, PdfVersion};

/// The largest PDF file shown as an image.
pub const MAX_BYTES: usize = 128 << 20;

/// The most bytes the content stream of a page may hold once decoded: krilla decodes it to
/// write the page as a form `XObject`.
const MAX_CONTENT_BYTES: u64 = 128 << 20;

/// The most compressed bytes of a content stream with other filters than Flate (LZW, run
/// length …), which Fagus cannot measure before decoding: decoded, they hold at most a few
/// thousand times as much.
const MAX_OTHER_FILTERED_BYTES: usize = 32 << 10;

/// Why a PDF file cannot be shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PdfImageError {
    /// The file is not a PDF that can be read.
    Broken,
    /// The file needs a password to be opened.
    Password,
    /// The file is larger than [`MAX_BYTES`], or the page's content than Fagus decodes.
    TooLarge,
    /// The file is PDF 2.0, newer than the PDF 1.7 Fagus writes.
    Version,
    /// The fragment names a page the file does not have (the page, from 1, and how many there
    /// are).
    NoPage(u32, usize),
}

/// Whether `data` is a PDF file: its header (`%PDF-`) within the first 1024 bytes, as viewers
/// find it.
pub fn is_pdf(data: &[u8]) -> bool {
    data.get(..data.len().min(1024))
        .is_some_and(|start| start.windows(5).any(|window| window == b"%PDF-"))
}

/// The page that the fragment of a URL (what follows `#`) names, from 0: `page=N` among the
/// parameters of RFC 8118 (`#page=2&zoom=50`), the first page without one or with one that is no
/// page number.
pub fn page_of(fragment: Option<&str>) -> u32 {
    fragment
        .into_iter()
        .flat_map(|fragment| fragment.split('&'))
        .filter_map(|parameter| parameter.split_once('='))
        .filter(|(name, _)| name.trim().eq_ignore_ascii_case("page"))
        .find_map(|(_, value)| value.trim().parse::<u32>().ok().filter(|page| *page >= 1))
        .map_or(0, |page| page - 1)
}

/// The image of the page of the PDF `data` that `fragment` names (see [`page_of`]): its size in
/// CSS pixels.
pub fn probe(data: Arc<Vec<u8>>, fragment: Option<&str>) -> Result<Image, PdfImageError> {
    if data.len() > MAX_BYTES {
        return Err(PdfImageError::TooLarge);
    }
    let page = page_of(fragment);
    let pdf = open(&data)?;
    let pages = pdf.pages();
    let Some(chosen) = pages.get(page as usize) else {
        return Err(PdfImageError::NoPage(page + 1, pages.len()));
    };
    content_size(chosen.raw())?;
    let (width, height) = chosen.render_dimensions();
    // A user unit makes the units of the page larger than a point (PDF 1.6).
    let unit = chosen
        .raw()
        .get::<f32>(b"UserUnit")
        .filter(|unit| unit.is_finite() && *unit > 0.0 && *unit <= 75_000.0)
        .unwrap_or(1.0);
    let (width, height) = (width * unit * 96.0 / 72.0, height * unit * 96.0 / 72.0);
    if !(width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0) {
        return Err(PdfImageError::Broken);
    }
    Ok(Image {
        data,
        format: ImageFormat::Pdf(page),
        width,
        height,
        natural: (true, true),
        ratio: Some(width / height),
        inline: false,
        orientation: 1,
        filters: Arc::from([]),
        pad: 0,
    })
}

/// The PDF file `data`, opened as krilla embeds it: one that needs no password (one its owner
/// only restricts opens without one), of PDF 1.7 at most.
pub(crate) fn open(data: &Arc<Vec<u8>>) -> Result<Pdf, PdfImageError> {
    let pdf = Pdf::new(Arc::clone(data)).map_err(|error| match error {
        LoadPdfError::Decryption(_) => PdfImageError::Password,
        LoadPdfError::Invalid => PdfImageError::Broken,
    })?;
    if pdf.version() > PdfVersion::Pdf17 {
        return Err(PdfImageError::Version);
    }
    if pdf.pages().is_empty() {
        return Err(PdfImageError::Broken);
    }
    Ok(pdf)
}

/// How many bytes the content of the page `page` (its dictionary) holds decoded, an estimate
/// that decodes no more than [`MAX_CONTENT_BYTES`]: a content stream compressed with Flate is
/// decoded without being kept.
pub(crate) fn content_size(page: &hayro_syntax::object::Dict<'_>) -> Result<u64, PdfImageError> {
    content_size_within(page, MAX_CONTENT_BYTES)
}

/// [`content_size`] with a limit of `limit` bytes.
fn content_size_within(page: &hayro_syntax::object::Dict<'_>, limit: u64) -> Result<u64, PdfImageError> {
    let streams: Vec<Stream<'_>> = match page.get::<Stream<'_>>(b"Contents") {
        Some(stream) => vec![stream],
        None => page
            .get::<Array<'_>>(b"Contents")
            .map(|array| array.iter::<Stream<'_>>().collect())
            .unwrap_or_default(),
    };
    let mut total = 0_u64;
    for stream in &streams {
        let filters: Vec<Vec<u8>> = match stream.dict().get::<Name<'_>>(b"Filter") {
            Some(name) => vec![name.to_vec()],
            None => stream
                .dict()
                .get::<Array<'_>>(b"Filter")
                .map(|array| array.iter::<Name<'_>>().map(|name| name.to_vec()).collect())
                .unwrap_or_default(),
        };
        let raw = stream.raw_data();
        let size = match filters.as_slice() {
            [] => raw.len() as u64,
            [flate] if flate == b"FlateDecode" || flate == b"Fl" => {
                // Read to the limit, and one byte more to tell that it is passed.
                let mut decoder = flate2::read::ZlibDecoder::new(&raw[..]).take(limit + 1);
                std::io::copy(&mut decoder, &mut std::io::sink()).unwrap_or(0)
            }
            _ if raw.len() <= MAX_OTHER_FILTERED_BYTES => {
                stream.decoded().map_or(0, |decoded| decoded.len() as u64)
            }
            _ => return Err(PdfImageError::TooLarge),
        };
        total = total.saturating_add(size);
        if total > limit {
            return Err(PdfImageError::TooLarge);
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PDF file of `objects` (numbered from 1, the first the catalog), with a cross-reference
    /// table.
    pub(crate) fn pdf(version: &str, objects: &[&[u8]]) -> Vec<u8> {
        let mut out = format!("%PDF-{version}\n").into_bytes();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n", index + 1).as_bytes());
            out.extend(*object);
            out.extend(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
        for offset in offsets {
            out.extend(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        out
    }

    fn two_pages() -> Vec<u8> {
        pdf(
            "1.4",
            &[
                b"<< /Type /Catalog /Pages 2 0 R >>",
                b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 595.28 841.89] >>",
                b"<< /Type /Page /Parent 2 0 R >>",
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 150] /CropBox [10 10 160 85] \
                   /Rotate 90 /UserUnit 2 >>",
            ],
        )
    }

    #[test]
    fn finds_the_header() {
        assert!(is_pdf(b"%PDF-1.7\n"));
        assert!(is_pdf(b"\xEF\xBB\xBF garbage %PDF-1.4"));
        assert!(!is_pdf(b"<svg/>"));
        let mut late = vec![b' '; 1024];
        late.extend(b"%PDF-1.4");
        assert!(!is_pdf(&late));
    }

    #[test]
    fn reads_the_page_of_the_fragment() {
        assert_eq!(page_of(None), 0);
        assert_eq!(page_of(Some("page=2")), 1);
        assert_eq!(page_of(Some("zoom=50&page=3")), 2);
        assert_eq!(page_of(Some("Page=4")), 3);
        assert_eq!(page_of(Some("page=0")), 0);
        assert_eq!(page_of(Some("page=x")), 0);
        assert_eq!(page_of(Some("nameddest=intro")), 0);
    }

    #[test]
    fn sizes_pages_by_their_crop_box() {
        let data = Arc::new(two_pages());
        let first = probe(Arc::clone(&data), None).expect("first page");
        assert_eq!(first.format, ImageFormat::Pdf(0));
        assert!((first.width - 595.28 * 4.0 / 3.0).abs() < 0.01, "{}", first.width);
        assert!(
            (first.height - 841.89 * 4.0 / 3.0).abs() < 0.01,
            "{}",
            first.height
        );
        // The crop box of 150 × 75 points, turned a quarter and in units of 2 points.
        let second = probe(Arc::clone(&data), Some("page=2")).expect("second page");
        assert_eq!(second.format, ImageFormat::Pdf(1));
        assert!(
            (second.width - 75.0 * 2.0 * 4.0 / 3.0).abs() < 0.01,
            "{}",
            second.width
        );
        assert!(
            (second.height - 150.0 * 2.0 * 4.0 / 3.0).abs() < 0.01,
            "{}",
            second.height
        );
        assert_eq!(
            probe(data, Some("page=3")).err(),
            Some(PdfImageError::NoPage(3, 2))
        );
    }

    #[test]
    fn refuses_what_it_cannot_show() {
        assert_eq!(
            probe(Arc::new(b"%PDF-1.4 nothing".to_vec()), None).err(),
            Some(PdfImageError::Broken)
        );
        let newer = pdf(
            "2.0",
            &[
                b"<< /Type /Catalog /Pages 2 0 R >>",
                b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>",
            ],
        );
        assert_eq!(probe(Arc::new(newer), None).err(), Some(PdfImageError::Version));
    }

    #[test]
    fn measures_content_without_keeping_it() {
        use std::io::Write;
        // 3 MiB of spaces compress to a few KiB: more than a limit of 2 MiB.
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
        encoder.write_all(&vec![b' '; 3 << 20]).expect("compress");
        let compressed = encoder.finish().expect("compress");
        let mut stream = format!(
            "<< /Length {} /Filter /FlateDecode >>\nstream\n",
            compressed.len()
        )
        .into_bytes();
        stream.extend(&compressed);
        stream.extend(b"\nendstream");
        let data = Arc::new(pdf(
            "1.4",
            &[
                b"<< /Type /Catalog /Pages 2 0 R >>",
                b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents [4 0 R 4 0 R] >>",
                &stream,
            ],
        ));
        let pdf = open(&data).expect("a PDF");
        let page = pdf.pages()[0].raw();
        assert_eq!(content_size_within(page, 8 << 20), Ok(6 << 20));
        // Each stream within the limit, both above it.
        assert_eq!(content_size_within(page, 5 << 20), Err(PdfImageError::TooLarge));
        assert_eq!(content_size_within(page, 2 << 20), Err(PdfImageError::TooLarge));
    }
}

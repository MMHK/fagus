//! SVG images with HTML in `<foreignObject>` elements, as draw.io, diagrams.net and Mermaid
//! export them: Chrome draws such an image as a document of its own, whose HTML is laid out in
//! the SVG (see [`fagus_layout::SvgDocument`]). Its style sheets are only the SVG's own (the
//! document's rules do not reach it), its fonts only the installed ones (not those of the
//! document's `@font-face` rules), its resources only `data:` URLs, and it is laid out at the
//! image's size, then scaled with the image (Chrome 157, measured).

use std::hash::{Hash, Hasher};
use std::sync::{Arc, PoisonError};

use fagus_css::{MediaEnvironment, Origin, RuleSet, Stylesheet};
use fagus_layout::{Image, SvgDocument};
use fagus_text::FontDb;
use rustc_hash::FxHasher;

use crate::{Document, Inner};

/// SVG images laid out as documents, by a hash of their data and the depth they lie at, with
/// the data: an engine lays them out once for its documents and their layouts.
#[derive(Default)]
pub(crate) struct Cache(Vec<Cached>);

/// A document of [`Cache`]: the hash of the image data, the depth, the data and the document.
type Cached = (u64, u8, Arc<Vec<u8>>, Arc<SvgDocument>);

impl Cache {
    /// The documents kept at most.
    const MAX_ENTRIES: usize = 32;
}

/// How deep SVG images with HTML may be nested in the HTML of one another.
pub(crate) const MAX_DEPTH: u8 = 3;

/// The largest side of an SVG image laid out as a document, in pixels.
const MAX_SIDE: f32 = 100_000.0;

impl Inner {
    /// The SVG image `image` (of the document `input`) as a document of its own, if it has HTML
    /// in `<foreignObject>` elements: the SVG as the root element of the document, laid out on a
    /// page of the image's size.
    /// Laid out once for the engine's documents: they all see the same fonts, those installed.
    pub(crate) fn svg_document(
        &self,
        input: &Document,
        image: &Image,
        fonts: &FontDb,
    ) -> Option<Arc<SvgDocument>> {
        if input.svg_depth >= MAX_DEPTH
            || !image
                .data
                .windows(b"<foreignObject".len())
                .any(|window| window == b"<foreignObject")
        {
            return None;
        }
        let hash = {
            let mut hasher = FxHasher::default();
            image.data.hash(&mut hasher);
            hasher.finish()
        };
        let cached = self
            .svg_documents
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .0
            .iter()
            .find(|(known, depth, data, _)| {
                *known == hash && *depth == input.svg_depth && **data == *image.data
            })
            .map(|(.., document)| Arc::clone(document));
        let document = if let Some(document) = cached {
            document
        } else {
            let document = Arc::new(self.lay_out_svg(input, image, fonts)?);
            let mut cache = self.svg_documents.lock().unwrap_or_else(PoisonError::into_inner);
            if cache.0.len() >= Cache::MAX_ENTRIES {
                cache.0.remove(0);
            }
            cache.0.push((
                hash,
                input.svg_depth,
                Arc::clone(&image.data),
                Arc::clone(&document),
            ));
            document
        };
        // Drawn, the SVG takes the faces it was laid out with: those of the image's own
        // `@font-face` rules, not the document's.
        let faces: Vec<Arc<fagus_text::Face>> = self
            .images
            .svg_faces(&image.data)
            .map(|faces| faces.to_vec())
            .unwrap_or_default();
        for svg in document.images.inline_svgs() {
            self.images.add_svg_faces(&svg.data, faces.clone());
        }
        Some(document)
    }

    /// Lays out the SVG image `image` as [`Inner::svg_document`] says.
    fn lay_out_svg(&self, input: &Document, image: &Image, fonts: &FontDb) -> Option<SvgDocument> {
        let (width, height) = (image.width, image.height);
        if !(width > 0.0 && height > 0.0 && width <= MAX_SIDE && height <= MAX_SIDE) {
            return None;
        }
        let text = std::str::from_utf8(&image.data).ok()?;
        let html = markup(text)?;
        let document = fagus_dom::Document::parse_html(&html);
        let mut sheets = vec![Arc::clone(&self.user_agent)];
        // The style sheets of the SVG apply to all of it, its HTML too.
        for node in document.descendants(fagus_dom::NodeId::DOCUMENT) {
            let Some(element) = document.element(node) else {
                continue;
            };
            if element.is_svg("style") || element.is("style") {
                let css = document.text_content(node);
                sheets.push(Arc::new(
                    Stylesheet::parse(&css, Origin::Author, "svg <style>").owned_by(node),
                ));
            }
        }
        sheets.push(Arc::new(Stylesheet::parse(
            &frame_css(width, height),
            Origin::Author,
            "fagus:svg-image.css",
        )));
        let references: Vec<&Stylesheet> = sheets.iter().map(AsRef::as_ref).collect();
        let rules = RuleSet::new(&references, &MediaEnvironment { width, height });
        // The faces of the document's `@font-face` rules are not the image's; those left keep
        // their ids, so that the document's fonts draw the text laid out with them.
        let fonts = fonts.without_document_faces();
        let styled = fagus_style::compute_styles_with(
            &document,
            &rules,
            (width, height),
            &fagus_style::ContainerSizes::default(),
            &fagus_layout::StyleFonts(&fonts),
        );
        // The text of the SVG itself takes the faces of its own `@font-face` rules too, as it
        // does where the image is drawn whole (see `Inner::svg_image_faces`). The HTML does not:
        // the ids of those faces would not be the document's.
        let with_own = match self.images.svg_faces(&image.data) {
            Some(faces) => {
                let mut with_own = fonts.clone().into_owned();
                for face in faces.iter() {
                    with_own.add_document_face(Arc::clone(face));
                }
                std::borrow::Cow::Owned(with_own)
            }
            None => std::borrow::Cow::Borrowed(&*fonts),
        };
        // Only `data:` URLs load: the image has no base, resources or loader.
        let own = Document {
            svg_depth: input.svg_depth + 1,
            ..Document::html(String::new())
        };
        let mut quiet = Vec::new();
        let images = self.document_images(&own, (&document, &rules), &styled, &with_own, &mut quiet);
        let (pages, _) = fagus_layout::layout_with_targets(
            &document,
            &styled,
            &rules,
            &fonts,
            &images,
            fagus_layout::Targets::default(),
            self.locale.as_deref(),
        );
        let page = pages.into_iter().next()?;
        Some(SvgDocument {
            page,
            images,
            size: (width, height),
            fit: fagus_pdf::svg::fit(&image.data, false),
        })
    }
}

/// The SVG document `text` as HTML whose body holds the SVG: without the XML declaration and
/// document type before its root, which would put the HTML parser into quirks mode.
fn markup(text: &str) -> Option<String> {
    let root = fagus_pdf::svg::from_root(text)?;
    if !root.starts_with("<svg") {
        return None;
    }
    let mut html = String::with_capacity(root.len() + 64);
    html.push_str("<!DOCTYPE html><html><head></head><body>");
    html.push_str(root);
    html.push_str("</body></html>");
    Some(html)
}

/// The style sheet that makes the SVG the frame of the document, `width` × `height`, as the root
/// of an image is: the page that size, the SVG on all of it, whatever the SVG's own rules say
/// about its size.
fn frame_css(width: f32, height: f32) -> String {
    format!(
        "@page {{ size: {width}px {height}px; margin: 0 }}\n\
         html, body {{ margin: 0 !important; padding: 0 !important; border: 0 !important }}\n\
         body > svg {{ display: block !important; box-sizing: border-box !important; \
         width: {width}px !important; height: {height}px !important; \
         min-width: 0 !important; min-height: 0 !important; max-width: none !important; \
         max-height: none !important; margin: 0 !important; padding: 0 !important; \
         border: 0 !important; position: static !important; float: none !important }}\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn puts_the_svg_into_an_html_body_without_its_prolog() {
        let svg = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!-- draw.io -->\n\
                   <!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \"svg11.dtd\" [ <!ENTITY a \"b\"> ]>\n\
                   <svg xmlns=\"http://www.w3.org/2000/svg\"><foreignObject/></svg>";
        let html = markup(svg).expect("an SVG root");
        assert_eq!(
            html,
            "<!DOCTYPE html><html><head></head><body>\
             <svg xmlns=\"http://www.w3.org/2000/svg\"><foreignObject/></svg></body></html>"
        );
        let document = fagus_dom::Document::parse_html(&html);
        assert!(!document.quirks_mode());
        assert!(markup("<html><foreignObject/></html>").is_none());
    }
}

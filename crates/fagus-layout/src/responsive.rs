//! The image an `<img>` element shows from its `srcset` and `sizes` and the `<source>` elements
//! of its `<picture>` (HTML, §4.8.4.3, "select an image source").

use fagus_css::MediaEnvironment;
use fagus_css::srcset::{self, Candidate};
use fagus_dom::{Document, ElementData, NodeId};

/// The image types Fagus draws, as the `type` of a `<source>` names them (parameters and case
/// aside). A source of another type, such as JPEG XL, is skipped, so that the next one shows.
/// TIFF is one, as in Safari (Chrome skips it).
const TYPES: &[&str] = &[
    "image/png",
    "image/apng",
    "image/jpeg",
    "image/jpg",
    "image/pjpeg",
    "image/gif",
    "image/webp",
    "image/bmp",
    "image/x-ms-bmp",
    "image/tiff",
    "image/avif",
    "image/svg+xml",
];

/// Where a choice of `srcset` is made: on the screen Chrome loads the document on, or on the
/// page it prints it on.
#[derive(Clone, Copy, Debug)]
pub enum Medium<'a> {
    /// The screen of [`srcset::LOAD_SCREEN`].
    LoadScreen,
    /// The page, as the media queries of style sheets see it.
    Print(&'a MediaEnvironment),
}

/// The candidate the `<img>` `element` shows from its own `srcset` or the sources of its
/// `<picture>`: its URL and density. `None` for an image that only has a `src` (or none).
pub fn chosen_source<'a>(
    document: &'a Document,
    element: NodeId,
    medium: Medium<'_>,
) -> Option<(&'a str, f32)> {
    let data = document.element(element).filter(|data| data.is("img"))?;
    let screen = matches!(medium, Medium::LoadScreen);
    let environment = match medium {
        Medium::LoadScreen => &srcset::LOAD_SCREEN,
        Medium::Print(environment) => environment,
    };
    // The `<source>` elements before the image in its `<picture>`, the first that applies.
    let sources = picture_sources(document, element);
    let mut responds = data.attribute("srcset").is_some();
    for source in sources {
        responds = true;
        let Some(candidates) = source.attribute("srcset").map(srcset::parse_srcset) else {
            continue;
        };
        if candidates.is_empty() {
            continue;
        }
        if let Some(media) = source
            .attribute("media")
            .map(str::trim)
            .filter(|media| !media.is_empty())
        {
            let applies = if screen {
                environment.matches_on_screen(media)
            } else {
                environment.matches(media)
            };
            if !applies {
                continue;
            }
        }
        if source.attribute("type").is_some_and(|kind| !supported(kind)) {
            continue;
        }
        return choose(&candidates, None, source, environment, screen);
    }
    if !responds {
        return None;
    }
    let candidates = data
        .attribute("srcset")
        .map(srcset::parse_srcset)
        .unwrap_or_default();
    let src = data.attribute("src").map(str::trim);
    choose(&candidates, src, data, environment, screen)
}

/// Whether the `<picture>` of the `<img>` `element` has a source with a `media` query, which
/// Chrome evaluates again when it prints.
pub fn has_media_sources(document: &Document, element: NodeId) -> bool {
    picture_sources(document, element).any(|source| source.attribute("media").is_some())
}

/// The `<source>` elements before `element` in its parent `<picture>`.
fn picture_sources(document: &Document, element: NodeId) -> impl Iterator<Item = &ElementData> {
    let picture = document
        .parent_element(element)
        .filter(|&parent| document.element(parent).is_some_and(|data| data.is("picture")));
    picture
        .into_iter()
        .flat_map(|picture| document.child_elements(picture))
        .take_while(move |&child| child != element)
        .filter_map(|child| document.element(child))
        .filter(|data| data.is("source"))
}

fn choose<'a>(
    candidates: &[Candidate<'a>],
    src: Option<&'a str>,
    data: &ElementData,
    environment: &MediaEnvironment,
    screen: bool,
) -> Option<(&'a str, f32)> {
    let size = srcset::source_size(data.attribute("sizes"), environment, screen);
    srcset::choose(candidates, src, size, 1.0)
}

/// Whether Fagus draws images of the MIME type `kind`.
fn supported(kind: &str) -> bool {
    let kind = kind.split(';').next().unwrap_or_default().trim();
    kind.is_empty() || TYPES.iter().any(|known| known.eq_ignore_ascii_case(kind))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chosen(html: &str, medium: Medium<'_>) -> Option<(String, f32)> {
        let document = Document::parse_html(html);
        let image = document.find("img")?;
        chosen_source(&document, image, medium).map(|(url, density)| (url.to_owned(), density))
    }

    #[test]
    fn chooses_sources_as_chrome_loads_them() {
        let screen = Medium::LoadScreen;
        let pick = |html: &str| chosen(html, screen).map(|(url, _)| url);
        // Only a `src`: nothing to choose.
        assert_eq!(pick(r#"<img src="a.png">"#), None);
        assert_eq!(
            pick(r#"<img src="a.png" srcset="b.png 2x">"#).as_deref(),
            Some("a.png")
        );
        assert_eq!(
            chosen(r#"<img srcset="b.png 2x">"#, screen),
            Some(("b.png".into(), 2.0))
        );
        // Widths with `sizes` on the 780 px window.
        let widths = r#"srcset="s.jpg 320w, m.jpg 640w, l.jpg 1280w""#;
        assert_eq!(
            pick(&format!(r#"<img src="x.jpg" {widths} sizes="200px">"#)).as_deref(),
            Some("s.jpg")
        );
        assert_eq!(pick(&format!("<img {widths}>")).as_deref(), Some("l.jpg"));
        // The first source that applies: JPEG XL is skipped, a `print` source does not apply on
        // the screen, a `screen` one does.
        let picture = |sources: &str| format!(r#"<picture>{sources}<img src="img.jpg"></picture>"#);
        assert_eq!(
            pick(&picture(r#"<source type="image/jxl" srcset="a.jxl"><source type="image/webp; codecs=x" srcset="w.webp">"#)).as_deref(),
            Some("w.webp")
        );
        // AVIF and BMP as in Chrome, TIFF as in Safari.
        assert_eq!(
            pick(&picture(
                r#"<source type="image/jxl" srcset="a.jxl"><source type="image/tiff" srcset="t.tif">"#
            ))
            .as_deref(),
            Some("t.tif")
        );
        assert_eq!(
            pick(&picture(
                r#"<source type="image/avif" srcset="a.avif"><source srcset="w.webp">"#
            ))
            .as_deref(),
            Some("a.avif")
        );
        assert_eq!(
            pick(&picture(r#"<source type="image/bmp" srcset="b.bmp">"#)).as_deref(),
            Some("b.bmp")
        );
        assert_eq!(
            pick(&picture(r#"<source media="print" srcset="p.png">"#)).as_deref(),
            Some("img.jpg")
        );
        assert_eq!(
            pick(&picture(r#"<source media="screen" srcset="s.png">"#)).as_deref(),
            Some("s.png")
        );
        assert_eq!(
            pick(&picture(
                r#"<source media="(orientation: portrait)" srcset="p.png"><source srcset="">"#
            ))
            .as_deref(),
            Some("img.jpg")
        );
        // On the page, `print` applies.
        let page = MediaEnvironment::default();
        assert_eq!(
            chosen(
                &picture(r#"<source media="print" srcset="p.png">"#),
                Medium::Print(&page)
            )
            .map(|(url, _)| url)
            .as_deref(),
            Some("p.png")
        );
    }
}

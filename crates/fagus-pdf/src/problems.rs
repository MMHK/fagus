//! Why a document does not meet PDF/A or PDF/UA, in words: the messages of krilla's validation
//! errors, which name the fonts and attached files they concern where krilla says which, and
//! for transparency, which PDF/A-1 forbids, what is transparent and on which page.

use fagus_layout::ImageFormat;
use fagus_paint::Item;
use krilla::configure::ValidationError as Problem;
use krilla::configure::validate::VersionedFeature;
use krilla::embed::EmbedError;
use krilla::surface::Location;
use krilla::text::Font;

use crate::{Attachment, Conformance};

/// The message of a problem. `font` names a font by its family; `attachments` are the files
/// attached to the document, whose names the messages about attachments give; `conformance` is
/// the PDF/A level the document is to meet.
pub(crate) fn describe(
    problem: &Problem,
    font: &dyn Fn(&Font) -> String,
    attachments: &[Attachment],
    conformance: Option<Conformance>,
) -> String {
    match problem {
        Problem::TooLongString => {
            "a text is longer than the 32,767 characters the standard allows (the title, a \
             description or a link)"
                .to_owned()
        }
        Problem::TooLongName => {
            "a name is longer than the 127 bytes the standard allows (the name of a font, or of \
             a spot color)"
                .to_owned()
        }
        Problem::TooLongArray => {
            "a list in the PDF is longer than the 8,191 entries the standard allows (a very long \
             line of text)"
                .to_owned()
        }
        Problem::TooLongDictionary => {
            "a page has more than the 4,095 entries the standard allows (too many links)".to_owned()
        }
        Problem::TooLargeFloat => {
            "a number in the PDF is larger than the 32,767 the standard allows (a page or a \
             shape too large)"
                .to_owned()
        }
        Problem::TooManyIndirectObjects => {
            "the document is too large: it needs more than the 8,388,607 PDF objects the \
             standard allows"
                .to_owned()
        }
        Problem::TooHighQNestingLevel => {
            "clips, transforms and effects are nested deeper than the 28 levels the standard \
             allows"
                .to_owned()
        }
        Problem::ContainsPostScript(_) => {
            "a gradient is drawn with a PostScript function, which the standard forbids".to_owned()
        }
        Problem::MissingCMYKProfile => {
            "a CMYK color is used without the ICC profile the standard requires for it".to_owned()
        }
        Problem::InconsistentSeparationFallback(colorant) => {
            use krilla::color::separation::SeparationColorant;
            let name = match colorant {
                SeparationColorant::NoColorant => "None",
                SeparationColorant::AllColorants => "All",
                SeparationColorant::Custom(name) => name,
            };
            format!("the spot color `{name}` is used with different alternative colors")
        }
        Problem::ContainsNotDefGlyph(face, _, text) => {
            format!(
                "{} has no glyph for `{text}`, and no other font has one",
                font(face)
            )
        }
        Problem::NoCodepointMapping(face, ..) | Problem::InvalidCodepointMapping(face, ..) => {
            format!("text in {} cannot be mapped to Unicode", font(face))
        }
        Problem::UnicodePrivateArea(face, _, character, _) => format!(
            "{} shows U+{:04X} from the private use area, which has no meaning",
            font(face),
            u32::from(*character)
        ),
        Problem::RestrictedLicense(face) => {
            format!("the license of {} does not allow embedding it", font(face))
        }
        Problem::NoDocumentLanguage => "the document has no language (add `lang` to <html>)".to_owned(),
        Problem::NoDocumentTitle => "the document has no title (add a <title>)".to_owned(),
        Problem::MissingAltText(_) => {
            "an image has no alternative text (add `alt`, empty for decoration)".to_owned()
        }
        Problem::MissingHeadingTitle => "a heading has no text".to_owned(),
        Problem::MissingDocumentOutline => "the document has no outline".to_owned(),
        Problem::MissingAnnotationAltText(_) => "a link has no text that describes it".to_owned(),
        Problem::MissingDocumentDate => "the document has no creation date".to_owned(),
        Problem::Transparency(_) => "the document uses transparency".to_owned(),
        Problem::ImageInterpolation(_) => {
            "an image is smoothed when it is scaled, which the standard forbids".to_owned()
        }
        Problem::EmbeddedFile(error, _) => attachment_problem(error, attachments, conformance),
        Problem::MissingTagging => "the document is not tagged".to_owned(),
        Problem::EmbeddedPDF(_) => {
            "the document shows a PDF file as an image (a page template), whose content cannot be \
             checked against the standard; use an SVG version of it (`pdftocairo -svg`)"
                .to_owned()
        }
        Problem::RequiresNewerPdfVersion(feature, _) => format!(
            "{} needs a newer PDF version than the standard allows",
            match feature {
                VersionedFeature::StructureOrderTabbing => "tabbing through links in reading order",
                VersionedFeature::HeaderFooterArtifactSubtypes => "marking headers and footers",
                VersionedFeature::TableHeaderScope => "the scope of table header cells",
            }
        ),
    }
}

/// Why attached files do not qualify, naming those it concerns.
fn attachment_problem(
    error: &EmbedError,
    attachments: &[Attachment],
    conformance: Option<Conformance>,
) -> String {
    // The names of the files that `filter` picks, as a list in words, and whether there are
    // several.
    let names = |filter: &dyn Fn(&Attachment) -> bool| {
        let names: Vec<String> = attachments
            .iter()
            .filter(|attachment| filter(attachment))
            .map(|attachment| format!("`{}`", attachment.name))
            .collect();
        match names.as_slice() {
            [] => None,
            [one] => Some((one.clone(), false)),
            [rest @ .., last] => Some((format!("{} and {last}", rest.join(", ")), true)),
        }
    };
    // The files that `filter` picks, as the subject of a sentence, with its verb.
    let subject = |filter: &dyn Fn(&Attachment) -> bool, (one, several): (&str, &str)| match names(filter) {
        Some((names, false)) => format!("the attached file {names} {one}"),
        Some((names, true)) => format!("the attached files {names} {several}"),
        None => format!("an attached file {one}"),
    };
    match error {
        EmbedError::Existence => {
            let names = names(&|_| true).map_or_else(|| "files".to_owned(), |(names, _)| names);
            match conformance.map(Conformance::part) {
                Some(1) => format!(
                    "PDF/A-1 allows no attachments; attach {names} to a PDF/A-3 or PDF/A-4f \
                     document instead"
                ),
                Some(4) => format!(
                    "PDF/A-4 allows only PDF/A files as attachments; attach {names} to a PDF/A-4f \
                     document instead"
                ),
                _ => format!(
                    "PDF/A-2 allows only PDF/A files as attachments; attach {names} to a PDF/A-3 \
                     document instead"
                ),
            }
        }
        EmbedError::MissingDescription => format!(
            "{}, which PDF/A-3 and PDF/UA require",
            subject(
                &|attachment| attachment.description.is_none(),
                ("has no description", "have no description")
            )
        ),
        EmbedError::MissingMimeType => format!(
            "{} (like `text/csv`), which PDF/A-3 requires",
            subject(
                &|attachment| krilla::embed::MimeType::new(&attachment.mime_type).is_none(),
                ("has no valid MIME type", "have no valid MIME type")
            )
        ),
        EmbedError::MissingDate => {
            "an attached file has no modification date, which PDF/A-3 requires (it is the \
             document's creation date)"
                .to_owned()
        }
    }
}

/// What is transparent, for the message about transparency in PDF/A-1. The order is that of
/// the message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Cause {
    Opacity = 1,
    Blend,
    Mask,
    Color,
    Text,
    Gradient,
    Image,
    Svg,
    Shadow,
    Filter,
}

impl Cause {
    const ALL: [Self; 10] = [
        Self::Opacity,
        Self::Blend,
        Self::Mask,
        Self::Color,
        Self::Text,
        Self::Gradient,
        Self::Image,
        Self::Svg,
        Self::Shadow,
        Self::Filter,
    ];

    fn of(item: &Item) -> Self {
        match item {
            Item::PushOpacity(_) => Self::Opacity,
            Item::PushBlend(_) | Item::PushIsolated => Self::Blend,
            Item::PushMask(_) => Self::Mask,
            Item::Text { .. } => Self::Text,
            Item::Gradient { .. } => Self::Gradient,
            Item::Image { image, .. } if image.format == ImageFormat::Svg => Self::Svg,
            Item::Image { .. } => Self::Image,
            Item::BoxShadow(_) | Item::TextShadow { .. } => Self::Shadow,
            Item::Filtered(_) => Self::Filter,
            Item::Tiles { tile, .. } => Self::of(tile),
            _ => Self::Color,
        }
    }

    fn words(self) -> &'static str {
        match self {
            Self::Opacity => "`opacity`",
            Self::Blend => "blend modes",
            Self::Mask => "masks",
            Self::Color => "translucent colors",
            Self::Text => "text in translucent colors or color fonts",
            Self::Gradient => "gradients with translucent colors",
            Self::Image => "images with transparent pixels",
            Self::Svg => "SVG images with transparency",
            Self::Shadow => "shadows",
            Self::Filter => "filters",
        }
    }
}

/// Where an item is drawn, for krilla to report with what it finds there: the index of its page
/// and what it is.
pub(crate) fn locate(page: usize, item: &Item) -> Location {
    let page = u64::try_from(page).unwrap_or(u64::MAX >> 5);
    Location::new(((page + 1) << 4) | Cause::of(item) as u64).unwrap_or(Location::MIN)
}

/// The message about the transparency that PDF/A-1 forbids, found where `locations` say: what
/// is transparent, and the first page with each.
pub(crate) fn transparency(locations: &[Option<Location>]) -> String {
    let mut first: Vec<(Cause, u64)> = Vec::new();
    let mut elsewhere = false;
    for location in locations {
        let found = location.and_then(|location| {
            let cause = Cause::ALL
                .into_iter()
                .find(|cause| *cause as u64 == location.get() & 15)?;
            Some((cause, (location.get() >> 4).max(1)))
        });
        match found {
            Some((cause, page)) => match first.iter_mut().find(|(known, _)| *known == cause) {
                Some((_, known)) => *known = (*known).min(page),
                None => first.push((cause, page)),
            },
            None => elsewhere = true,
        }
    }
    first.sort_unstable();
    let list = |items: Vec<String>| match items.as_slice() {
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
        [] => String::new(),
    };
    let pages: Vec<u64> = first.iter().map(|(_, page)| *page).collect();
    let uses = match pages.first() {
        // All first on the same page: the page once.
        Some(page) if pages.iter().all(|other| other == page) && !elsewhere => format!(
            "{}, first on page {page}",
            list(first.iter().map(|(cause, _)| cause.words().to_owned()).collect())
        ),
        _ => {
            let mut items: Vec<String> = first
                .iter()
                .map(|(cause, page)| format!("{} (first on page {page})", cause.words()))
                .collect();
            if elsewhere {
                items.push("other content".to_owned());
            }
            list(items)
        }
    };
    format!("PDF/A-1 forbids transparency, which the document uses for {uses}; PDF/A-2b and later allow it")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{Problem, describe, locate, transparency};
    use crate::{Attachment, Relationship};
    use krilla::embed::EmbedError;

    fn attachment(name: &str, mime_type: &str, description: Option<&str>) -> Attachment {
        Attachment {
            name: name.to_owned(),
            mime_type: mime_type.to_owned(),
            description: description.map(str::to_owned),
            relationship: Relationship::Supplement,
            data: Arc::new(Vec::new()),
        }
    }

    #[test]
    fn names_the_attachments_that_do_not_qualify() {
        let font = |_: &krilla::text::Font| "Inter".to_owned();
        let attachments = [
            attachment("timesheet.csv", "text/csv", Some("Hours")),
            attachment("notes", "plain", None),
            attachment("data.json", "application/json", None),
        ];
        let message = |error| describe(&Problem::EmbeddedFile(error, None), &font, &attachments, None);
        assert_eq!(
            message(EmbedError::Existence),
            "PDF/A-2 allows only PDF/A files as attachments; attach `timesheet.csv`, `notes` and \
             `data.json` to a PDF/A-3 document instead"
        );
        assert_eq!(
            message(EmbedError::MissingDescription),
            "the attached files `notes` and `data.json` have no description, which PDF/A-3 and \
             PDF/UA require"
        );
        assert_eq!(
            message(EmbedError::MissingMimeType),
            "the attached file `notes` has no valid MIME type (like `text/csv`), which PDF/A-3 \
             requires"
        );
    }

    #[test]
    fn names_the_attachments_that_a_level_refuses() {
        let font = |_: &krilla::text::Font| "Inter".to_owned();
        let attachments = [attachment("daten.csv", "text/csv", Some("Daten"))];
        let message = |conformance| {
            describe(
                &Problem::EmbeddedFile(EmbedError::Existence, None),
                &font,
                &attachments,
                Some(conformance),
            )
        };
        assert_eq!(
            message(crate::Conformance::PdfA1b),
            "PDF/A-1 allows no attachments; attach `daten.csv` to a PDF/A-3 or PDF/A-4f document \
             instead"
        );
        assert_eq!(
            message(crate::Conformance::PdfA4),
            "PDF/A-4 allows only PDF/A files as attachments; attach `daten.csv` to a PDF/A-4f \
             document instead"
        );
        assert_eq!(
            message(crate::Conformance::PdfA2u),
            "PDF/A-2 allows only PDF/A files as attachments; attach `daten.csv` to a PDF/A-3 \
             document instead"
        );
    }

    #[test]
    fn says_what_is_transparent_and_where() {
        use fagus_paint::{Item, RectF};
        use fagus_style::values::Rgba;
        let rect = Item::Rect {
            rect: RectF {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            color: Rgba::BLACK,
            radii: None,
        };
        let opacity = Item::PushOpacity(0.5);
        // Each cause once, at the first page it is on; the page once where all share it.
        assert_eq!(
            transparency(&[
                Some(locate(0, &rect)),
                Some(locate(0, &opacity)),
                Some(locate(0, &rect))
            ]),
            "PDF/A-1 forbids transparency, which the document uses for `opacity` and translucent \
             colors, first on page 1; PDF/A-2b and later allow it"
        );
        assert_eq!(
            transparency(&[
                Some(locate(4, &rect)),
                Some(locate(2, &opacity)),
                Some(locate(1, &rect))
            ]),
            "PDF/A-1 forbids transparency, which the document uses for `opacity` (first on page \
             3) and translucent colors (first on page 2); PDF/A-2b and later allow it"
        );
        assert_eq!(
            transparency(&[None, Some(locate(0, &opacity))]),
            "PDF/A-1 forbids transparency, which the document uses for `opacity` (first on page \
             1) and other content; PDF/A-2b and later allow it"
        );
        assert_eq!(
            transparency(&[None]),
            "PDF/A-1 forbids transparency, which the document uses for other content; PDF/A-2b \
             and later allow it"
        );
    }

    #[test]
    fn describes_every_problem_in_words() {
        use krilla::configure::validate::VersionedFeature;
        let font = |_: &krilla::text::Font| "Inter".to_owned();
        let problems = [
            Problem::TooLongString,
            Problem::TooLongName,
            Problem::TooLongArray,
            Problem::TooLongDictionary,
            Problem::TooLargeFloat,
            Problem::TooManyIndirectObjects,
            Problem::TooHighQNestingLevel,
            Problem::ContainsPostScript(None),
            Problem::MissingCMYKProfile,
            Problem::InconsistentSeparationFallback(krilla::color::separation::SeparationColorant::Custom(
                "Gold".to_owned(),
            )),
            Problem::NoDocumentLanguage,
            Problem::NoDocumentTitle,
            Problem::MissingAltText(None),
            Problem::MissingHeadingTitle,
            Problem::MissingDocumentOutline,
            Problem::MissingAnnotationAltText(None),
            Problem::MissingDocumentDate,
            Problem::Transparency(None),
            Problem::ImageInterpolation(None),
            Problem::EmbeddedFile(EmbedError::MissingDate, None),
            Problem::MissingTagging,
            Problem::EmbeddedPDF(None),
            Problem::RequiresNewerPdfVersion(VersionedFeature::TableHeaderScope, None),
        ];
        for problem in &problems {
            let message = describe(problem, &font, &[], None);
            // No Rust names: words in lower case, as the messages go after a colon.
            assert!(
                message.starts_with(|first: char| !first.is_ascii_uppercase()),
                "{message}"
            );
            let debug = format!("{problem:?}");
            let name = debug.split('(').next().unwrap_or_default();
            assert!(!message.contains(name), "{message}");
        }
    }
}

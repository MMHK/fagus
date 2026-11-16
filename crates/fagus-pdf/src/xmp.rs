//! Extending a PDF's XMP metadata after krilla has written it.
//!
//! krilla writes the XMP metadata itself and offers no way to add to it, but Factur-X needs its
//! own properties there. The PDF gets an incremental update (PDF 1.7, §7.5.6): a new version of
//! the metadata stream, a cross-reference section for it, and a trailer that points back to the
//! previous one. PDF/A allows incremental updates.

use std::io::Write;

use crate::parse;

/// Where the list of PDF/A extension schemas ends in krilla's metadata.
const SCHEMAS_END: &str = "</rdf:Bag></pdfaExtension:schemas>";

/// Additions to a PDF's XMP metadata.
#[derive(Clone, Debug, Default)]
pub struct Extension {
    /// RDF descriptions with new properties.
    pub descriptions: String,
    /// PDF/A extension schemas (`rdf:li` items) that declare the new properties; they join the
    /// document's list of extension schemas.
    pub schemas: String,
}

/// Adds descriptions and extension schemas to the XMP metadata of `pdf`.
pub(crate) fn extend(mut pdf: Vec<u8>, extension: &Extension) -> Option<Vec<u8>> {
    let references = parse::cross_references(&pdf)?;
    let trailer = &references.trailer;
    let size = parse::entry(trailer, "/Size")?.trim().to_owned();
    let root = parse::entry(trailer, "/Root")?.trim().to_owned();
    let info = parse::entry(trailer, "/Info").map(|info| info.trim().to_owned());
    let id = parse::entry(trailer, "/ID").map(|id| id.trim().to_owned());

    // The catalog names the metadata stream.
    let reference = |value: &str| value.split_whitespace().next()?.parse::<u32>().ok();
    let root_number = reference(&root)?;
    let catalog = parse::object(&pdf, root_number, *references.offsets.get(&root_number)?)?;
    let catalog = std::str::from_utf8(parse::trim(&pdf[catalog.body])).ok()?;
    let number = reference(parse::entry(catalog, "/Metadata")?)?;
    let metadata = parse::object(&pdf, number, *references.offsets.get(&number)?)?;
    let mut xmp = std::str::from_utf8(&pdf[metadata.data?]).ok()?.to_owned();
    // A property may appear only once: new schemas join the existing list.
    if let Some(end) = xmp.find(SCHEMAS_END) {
        xmp.insert_str(end, &extension.schemas);
    } else if !extension.schemas.is_empty() {
        let close = xmp.rfind("</rdf:RDF>")?;
        xmp.insert_str(
            close,
            &format!(
                "<rdf:Description rdf:about=\"\" xmlns:pdfaExtension=\"http://www.aiim.org/pdfa/ns/extension/\" \
                 xmlns:pdfaSchema=\"http://www.aiim.org/pdfa/ns/schema#\" \
                 xmlns:pdfaProperty=\"http://www.aiim.org/pdfa/ns/property#\">\
                 <pdfaExtension:schemas><rdf:Bag>{}</rdf:Bag></pdfaExtension:schemas></rdf:Description>",
                extension.schemas
            ),
        );
    }
    let close = xmp.rfind("</rdf:RDF>")?;
    let extended = format!("{}{}{}", &xmp[..close], extension.descriptions, &xmp[close..]);

    // The end-of-line marker before `endstream` is not part of the data (PDF/A checks the length).
    let offset = pdf.len() + 1;
    write!(
        pdf,
        "\n{number} 0 obj\n<</Length {}/Type/Metadata/Subtype/XML>>\nstream\n{extended}\nendstream\nendobj\n",
        extended.len()
    )
    .ok()?;
    let xref = pdf.len();
    write!(
        pdf,
        "xref\n{number} 1\n{offset:010} 00000 n\r\ntrailer\n<</Size {size}/Root {root}"
    )
    .ok()?;
    if let Some(info) = info {
        write!(pdf, "/Info {info}").ok()?;
    }
    if let Some(id) = id {
        write!(pdf, "/ID{id}").ok()?;
    }
    write!(pdf, "/Prev {}>>\nstartxref\n{xref}\n%%EOF\n", references.start).ok()?;
    Some(pdf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_an_update_with_the_extended_metadata() {
        let catalog = "1 0 obj\n<</Type/Catalog/Metadata 2 0 R>>\nendobj\n";
        let metadata = "2 0 obj\n<</Length 52/Type/Metadata/Subtype/XML>>\nstream\n\
                        <x:xmpmeta><rdf:RDF><rdf:Description/></rdf:RDF></x:xmpmeta>\nendstream\nendobj\n";
        let header = "%PDF-1.7\n";
        let xref = header.len() + catalog.len() + metadata.len();
        let original = format!(
            "{header}{catalog}{metadata}xref\n0 3\n0000000000 65535 f\r\n{:010} 00000 n\r\n{:010} 00000 n\r\n\
             trailer\n<</Size 3/Root 1 0 R/ID[(a)(a)]>>\nstartxref\n{xref}\n%%EOF\n",
            header.len(),
            header.len() + catalog.len()
        )
        .into_bytes();
        let length = original.len();
        let extension = Extension {
            descriptions: "<rdf:Description fx:x=\"1\"/>".to_owned(),
            schemas: "<rdf:li>fx</rdf:li>".to_owned(),
        };
        let updated = extend(original, &extension).expect("extended");
        let tail = String::from_utf8_lossy(&updated[length..]);
        assert!(tail.contains("2 0 obj"), "{tail}");
        assert!(tail.contains("<rdf:Description fx:x=\"1\"/></rdf:RDF>"));
        assert!(
            tail.contains("<pdfaExtension:schemas><rdf:Bag><rdf:li>fx</rdf:li></rdf:Bag>"),
            "{tail}"
        );
        assert!(
            tail.contains(&format!("/Size 3/Root 1 0 R/ID[(a)(a)]/Prev {xref}>>")),
            "{tail}"
        );
        let length: usize = tail
            .split("/Length ")
            .nth(1)
            .and_then(|rest| rest.split('/').next())
            .and_then(|digits| digits.parse().ok())
            .expect("length");
        let data = tail
            .split_once(">>\nstream\n")
            .and_then(|(_, rest)| rest.split_once("\nendstream"))
            .map(|(data, _)| data);
        assert_eq!(
            data.map(str::len),
            Some(length),
            "the length leaves out the end-of-line marker"
        );
        let offset: usize = tail
            .split("xref\n2 1\n")
            .nth(1)
            .and_then(|rest| rest.get(..10))
            .and_then(|digits| digits.parse().ok())
            .expect("offset");
        assert!(
            updated[offset..].starts_with(b"2 0 obj"),
            "the cross-reference entry points at the object"
        );
    }
}

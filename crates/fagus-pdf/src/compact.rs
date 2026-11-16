//! Object streams: the many small objects of a PDF (pages, font dictionaries, annotations, the
//! structure tree) compressed together, with a cross-reference stream (PDF 1.7, §7.5.7 and
//! §7.5.8).
//!
//! krilla writes every object on its own, uncompressed, with a cross-reference table; for a
//! tagged PDF the structure elements then make up most of the file. The PDF is rewritten as one
//! revision: streams stay as they are, all other objects go into object streams. Incremental
//! updates (such as the one for Factur-X metadata) are merged, the newest version of each object
//! winning. Anything unexpected leaves the PDF as it was.

use std::fmt::Write as _;
use std::io::Write;

use flate2::Compression;
use flate2::write::ZlibEncoder;
#[cfg(feature = "parallel")]
use rayon::prelude::*;

use crate::parse::{self, Object};

/// Objects per object stream.
const PER_STREAM: usize = 128;

/// The PDF with its objects in object streams and a cross-reference stream, or `None` if it is
/// not a PDF this understands. `catalog` holds entries for the document catalog that krilla does
/// not write; those it has already are left as they are.
pub(crate) fn compact(pdf: &[u8], catalog: &[(&str, &str)]) -> Option<Vec<u8>> {
    let references = parse::cross_references(pdf)?;
    let root = parse::entry(&references.trailer, "/Root")
        .and_then(|root| root.split_whitespace().next()?.parse::<u32>().ok());
    let mut objects: Vec<Object> = Vec::with_capacity(references.offsets.len());
    for (&number, &offset) in &references.offsets {
        objects.push(parse::object(pdf, number, offset)?);
    }
    let header_end = objects.iter().map(|object| object.whole.start).min()?;
    let header = &pdf[..header_end];
    if !header.starts_with(b"%PDF-") {
        return None;
    }

    let mut out = Vec::with_capacity(pdf.len());
    out.extend_from_slice(header);
    let highest = objects.last()?.number;
    // Entries of the cross-reference stream (type, second and third field), by object number.
    let mut entries: Vec<(u8, u64, u16)> = vec![(0, 0, 0); highest as usize + 1];
    entries[0] = (0, 0, u16::MAX);
    for object in objects.iter().filter(|object| object.data.is_some()) {
        entries[object.number as usize] = (1, out.len() as u64, 0);
        out.extend_from_slice(&pdf[object.whole.clone()]);
        out.push(b'\n');
    }
    let small: Vec<&Object> = objects.iter().filter(|object| object.data.is_none()).collect();
    let mut next = highest + 1;
    // The object streams with their numbers, filled first and compressed side by side.
    let mut streams: Vec<(u32, usize, usize, Vec<u8>)> = Vec::new();
    for group in small.chunks(PER_STREAM) {
        let number = next;
        next += 1;
        let mut index = String::new();
        let mut data = Vec::new();
        for (position, object) in group.iter().enumerate() {
            write!(index, "{} {} ", object.number, data.len()).ok()?;
            let body = parse::trim(&pdf[object.body.clone()]);
            match (Some(object.number) == root, body.strip_suffix(b">>")) {
                (true, Some(open)) => {
                    data.extend_from_slice(open);
                    let text = std::str::from_utf8(body).ok()?;
                    for (key, value) in catalog {
                        if parse::entry(text, key).is_none() {
                            data.extend_from_slice(key.as_bytes());
                            data.extend_from_slice(value.as_bytes());
                        }
                    }
                    data.extend_from_slice(b">>");
                }
                _ => data.extend_from_slice(body),
            }
            data.push(b'\n');
            entries[object.number as usize] = (2, u64::from(number), u16::try_from(position).ok()?);
        }
        index.push('\n');
        let mut content = index.into_bytes();
        let first = content.len();
        content.extend_from_slice(&data);
        streams.push((number, group.len(), first, content));
    }
    #[cfg(feature = "parallel")]
    let compressed: Vec<Option<Vec<u8>>> = streams
        .par_iter()
        .map(|(_, _, _, content)| deflate(content))
        .collect();
    #[cfg(not(feature = "parallel"))]
    let compressed: Vec<Option<Vec<u8>>> = streams
        .iter()
        .map(|(_, _, _, content)| deflate(content))
        .collect();
    for ((number, count, first, _), compressed) in streams.iter().zip(compressed) {
        let compressed = compressed?;
        entries.push((1, out.len() as u64, 0));
        write!(
            out,
            "{number} 0 obj\n<</Type/ObjStm/N {count}/First {first}/Filter/FlateDecode/Length {}>>\nstream\n",
            compressed.len()
        )
        .ok()?;
        out.extend_from_slice(&compressed);
        out.extend_from_slice(b"\nendstream\nendobj\n");
    }

    // The cross-reference stream lists itself too, and carries the trailer's entries.
    let xref = next;
    let xref_offset = out.len() as u64;
    entries.push((1, xref_offset, 0));
    let width = (1..8)
        .find(|bytes| entries.iter().all(|entry| entry.1 < 1_u64 << (8 * bytes)))
        .unwrap_or(8);
    let mut table = Vec::with_capacity(entries.len() * (width + 3));
    for (kind, field, generation) in &entries {
        table.push(*kind);
        table.extend_from_slice(&field.to_be_bytes()[8 - width..]);
        table.extend_from_slice(&generation.to_be_bytes());
    }
    let table = deflate(&table)?;
    let mut trailer = String::new();
    for key in ["/Root", "/Info", "/ID"] {
        if let Some(value) = parse::entry(&references.trailer, key) {
            trailer.push_str(key);
            trailer.push(' ');
            trailer.push_str(value.trim());
        }
    }
    write!(
        out,
        "{xref} 0 obj\n<</Type/XRef/Size {}/W[1 {width} 2]{trailer}/Filter/FlateDecode/Length {}>>\nstream\n",
        entries.len(),
        table.len()
    )
    .ok()?;
    out.extend_from_slice(&table);
    write!(out, "\nendstream\nendobj\nstartxref\n{xref_offset}\n%%EOF\n").ok()?;
    Some(out)
}

fn deflate(data: &[u8]) -> Option<Vec<u8>> {
    let mut encoder = ZlibEncoder::new(Vec::with_capacity(data.len() / 3), Compression::new(6));
    encoder.write_all(data).ok()?;
    encoder.finish().ok()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::io::Read;

    use fagus_paint::{Item, PageList, RectF};
    use fagus_style::values::Rgba;
    use fagus_text::FontDb;

    use super::*;
    use crate::parse::{
        cross_references, dictionary_end, entry, find, number_after, object, rfind, skip_whitespace, trim,
    };
    use crate::{Extension, Link, LinkTarget, Metadata, Options};

    fn inflate(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        flate2::read::ZlibDecoder::new(data)
            .read_to_end(&mut out)
            .expect("deflated data");
        out
    }

    /// The dictionary and the decoded data of a stream object.
    fn stream(pdf: &[u8], number: usize, offset: usize) -> (String, Vec<u8>) {
        let object = object(pdf, u32::try_from(number).expect("number"), offset).expect("object");
        let data = object.data.expect("a stream");
        let start = skip_whitespace(pdf, object.body.start);
        let end = dictionary_end(pdf, start).expect("dictionary");
        let dictionary = std::str::from_utf8(&pdf[start..end]).expect("text").to_owned();
        (dictionary, inflate(&pdf[data]))
    }

    /// The bodies of all objects of a compacted PDF, read back through its cross-reference
    /// stream and object streams.
    fn read_back(pdf: &[u8]) -> BTreeMap<u32, Vec<u8>> {
        let start = rfind(pdf, b"startxref").expect("startxref");
        let xref_offset = number_after(pdf, start + 9).expect("offset").0;
        let xref_number = number_after(pdf, xref_offset).expect("number").0;
        let (dictionary, table) = stream(pdf, xref_number, xref_offset);
        assert!(dictionary.contains("/Type/XRef"));
        for key in ["/Root", "/Info", "/ID"] {
            assert!(entry(&dictionary, key).is_some(), "{key}");
        }
        let widths: Vec<usize> = entry(&dictionary, "/W")
            .expect("W")
            .trim_matches([' ', '[', ']'])
            .split_whitespace()
            .map(|width| width.parse().expect("width"))
            .collect();
        let row_size: usize = widths.iter().sum();
        let rows: Vec<(usize, usize, usize)> = table
            .chunks(row_size)
            .map(|row| {
                let field = |bytes: &[u8]| {
                    bytes
                        .iter()
                        .fold(0, |value, byte| value << 8 | usize::from(*byte))
                };
                (
                    field(&row[..widths[0]]),
                    field(&row[widths[0]..widths[0] + widths[1]]),
                    field(&row[widths[0] + widths[1]..]),
                )
            })
            .collect();
        let object_streams: BTreeMap<usize, Vec<(u32, Vec<u8>)>> = rows
            .iter()
            .filter(|row| row.0 == 2)
            .map(|row| row.1)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .map(|number| {
                let (dictionary, data) = stream(pdf, number, rows[number].1);
                assert!(dictionary.contains("/Type/ObjStm"));
                let first: usize = entry(&dictionary, "/First")
                    .expect("First")
                    .trim()
                    .parse()
                    .expect("first");
                let index: Vec<usize> = std::str::from_utf8(&data[..first])
                    .expect("index")
                    .split_whitespace()
                    .map(|value| value.parse().expect("number"))
                    .collect();
                let objects = index
                    .chunks(2)
                    .enumerate()
                    .map(|(position, pair)| {
                        let end = index
                            .get(position * 2 + 3)
                            .map_or(data.len(), |next| first + next);
                        let body = trim(&data[first + pair[1]..end]).to_vec();
                        (u32::try_from(pair[0]).expect("number"), body)
                    })
                    .collect();
                (number, objects)
            })
            .collect();
        let mut objects = BTreeMap::new();
        for (number, &(kind, second, third)) in rows.iter().enumerate() {
            let is_object_stream = object_streams.contains_key(&number);
            match kind {
                1 if number != xref_number && !is_object_stream => {
                    let object = object(pdf, u32::try_from(number).expect("number"), second).expect("object");
                    objects.insert(object.number, trim(&pdf[object.body]).to_vec());
                }
                2 => {
                    let (found, body) = object_streams[&second][third].clone();
                    assert_eq!(found as usize, number);
                    objects.insert(found, body);
                }
                _ => {}
            }
        }
        objects
    }

    /// A PDF whose title and link text could fool a careless reader, with or without an
    /// incremental update of its metadata.
    fn sample(update: bool) -> Vec<u8> {
        let rect = RectF {
            x: 10.0,
            y: 10.0,
            width: 50.0,
            height: 20.0,
        };
        let page = PageList {
            width: 200.0,
            height: 100.0,
            items: vec![Item::Rect {
                rect,
                color: Rgba::BLACK,
                radii: None,
            }],
            bleed: 0.0,
            marks: fagus_layout::PageMarks::default(),
            orientation: fagus_layout::PageOrientation::Upright,
        };
        let link = Link {
            rect: [10.0, 10.0, 50.0, 20.0],
            target: LinkTarget::Uri("https://quittio.dev/endobj".to_owned()),
            node: None,
            description: Some("endobj ) ( \\ stream >> << /Length 3 %".to_owned()),
        };
        let options = Options {
            links: vec![vec![link.clone()], vec![link]],
            xmp: update.then(|| Extension {
                descriptions:
                    "<rdf:Description rdf:about=\"\" xmlns:x=\"urn:x\"><x:a>b</x:a></rdf:Description>"
                        .to_owned(),
                schemas: String::new(),
            }),
            ..Options::default()
        };
        let metadata = Metadata {
            title: Some("Rechnung (endobj) \\".to_owned()),
            ..Metadata::default()
        };
        crate::write_uncompacted(&[page.clone(), page], &FontDb::new(), &metadata, &options)
            .expect("written")
            .pdf
    }

    #[test]
    fn keeps_every_object() {
        for update in [false, true] {
            let plain = sample(update);
            let compacted = compact(&plain, &[]).expect("compacted");
            let original: BTreeMap<u32, Vec<u8>> = cross_references(&plain)
                .expect("cross-reference table")
                .offsets
                .iter()
                .map(|(&number, &offset)| {
                    let object = object(&plain, number, offset).expect("object");
                    (number, trim(&plain[object.body]).to_vec())
                })
                .collect();
            assert_eq!(read_back(&compacted), original);
            assert!(compacted.len() < plain.len());
            if update {
                // The newest metadata wins.
                assert!(
                    original
                        .values()
                        .any(|body| find(body, b"<x:a>b</x:a>").is_some())
                );
            }
        }
    }

    #[test]
    fn adds_entries_to_the_catalog() {
        let plain = sample(false);
        let entries = [
            ("/ViewerPreferences", "<</DisplayDocTitle true>>"),
            ("/Type", "/Other"),
        ];
        let objects = read_back(&compact(&plain, &entries).expect("compacted"));
        let catalogs: Vec<String> = objects
            .values()
            .map(|body| String::from_utf8_lossy(body).into_owned())
            .filter(|body| body.contains("/Catalog"))
            .collect();
        assert_eq!(catalogs.len(), 1, "{catalogs:?}");
        assert!(
            catalogs[0].ends_with("/ViewerPreferences<</DisplayDocTitle true>>>>"),
            "{}",
            catalogs[0]
        );
        // An entry the catalog has stays as it is.
        assert!(!catalogs[0].contains("/Other"), "{}", catalogs[0]);
    }

    #[test]
    fn leaves_what_it_does_not_understand() {
        assert!(compact(b"%PDF-1.7\nnot really", &[]).is_none());
        let plain = sample(false);
        let broken = [&plain[..plain.len() - 30], b"startxref\n99999999\n%%EOF\n"].concat();
        assert!(compact(&broken, &[]).is_none());
    }
}

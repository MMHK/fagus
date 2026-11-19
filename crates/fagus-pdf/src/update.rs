//! Changing objects of a PDF after krilla has written it, as an incremental update (PDF 1.7,
//! §7.5.6): the new versions of the objects that change and the new objects, a cross-reference
//! section for them, and a trailer that points back to the previous one. Compacting the PDF
//! afterwards merges the update into one revision.

use std::fmt::Write as _;
use std::io::Write;

use crate::parse::{self, CrossReferences, Object};

/// The objects of a PDF, and what an update changes about them.
pub(crate) struct Update<'a> {
    pdf: &'a [u8],
    references: CrossReferences,
    /// The bodies of the objects that change or are new, by number, in order.
    changed: std::collections::BTreeMap<u32, Vec<u8>>,
    /// The number of the next new object.
    next: u32,
}

impl<'a> Update<'a> {
    /// An update of `pdf`; `None` if it is not a PDF this understands.
    pub(crate) fn new(pdf: &'a [u8]) -> Option<Self> {
        let references = parse::cross_references(pdf)?;
        let size: u32 = parse::entry(&references.trailer, "/Size")?.trim().parse().ok()?;
        let next = references
            .offsets
            .keys()
            .next_back()
            .map_or(size, |&last| size.max(last + 1));
        Some(Self {
            pdf,
            references,
            changed: std::collections::BTreeMap::new(),
            next,
        })
    }

    /// The numbers of the objects in the PDF as krilla wrote it.
    pub(crate) fn numbers(&self) -> impl Iterator<Item = u32> + '_ {
        self.references.offsets.keys().copied()
    }

    /// An object as krilla wrote it.
    pub(crate) fn object(&self, number: u32) -> Option<Object> {
        parse::object(self.pdf, number, *self.references.offsets.get(&number)?)
    }

    /// The body of an object (between `obj` and `endobj`), as the update leaves it.
    pub(crate) fn body(&self, number: u32) -> Option<Vec<u8>> {
        if let Some(body) = self.changed.get(&number) {
            return Some(body.clone());
        }
        let object = self.object(number)?;
        Some(parse::trim(&self.pdf[object.body]).to_vec())
    }

    /// The dictionary of an object (of a stream: the dictionary before its data), as the update
    /// leaves it.
    pub(crate) fn dictionary(&self, number: u32) -> Option<String> {
        let body = self.body(number)?;
        let start = parse::skip_whitespace(&body, 0);
        if !body[start..].starts_with(b"<<") {
            return None;
        }
        let end = parse::dictionary_end(&body, start)?;
        String::from_utf8(body[start..end].to_vec()).ok()
    }

    /// The data of a stream as krilla wrote it.
    pub(crate) fn data(&self, number: u32) -> Option<&'a [u8]> {
        let object = self.object(number)?;
        Some(&self.pdf[object.data?])
    }

    /// The number of the document catalog.
    pub(crate) fn root(&self) -> Option<u32> {
        reference(parse::entry(&self.references.trailer, "/Root")?)
    }

    /// Gives an object a new body.
    pub(crate) fn set(&mut self, number: u32, body: Vec<u8>) {
        self.changed.insert(number, body);
    }

    /// Gives a stream a new dictionary and new data; the dictionary gets the length.
    pub(crate) fn set_stream(&mut self, number: u32, dictionary: &str, data: &[u8]) {
        self.set(number, stream(dictionary, data));
    }

    /// Gives a stream a new dictionary, its data as it is (its length too).
    pub(crate) fn set_dictionary(&mut self, number: u32, dictionary: &str) -> Option<()> {
        let body = self.body(number)?;
        let start = parse::skip_whitespace(&body, 0);
        let end = parse::dictionary_end(&body, start)?;
        let mut changed = dictionary.as_bytes().to_vec();
        changed.extend_from_slice(&body[end..]);
        self.set(number, changed);
        Some(())
    }

    /// Adds an object; returns its number.
    pub(crate) fn add(&mut self, body: Vec<u8>) -> u32 {
        let number = self.reserve();
        self.set(number, body);
        number
    }

    /// The number of an object to add later with [`Update::set`].
    pub(crate) fn reserve(&mut self) -> u32 {
        let number = self.next;
        self.next += 1;
        number
    }

    /// The PDF with the update appended, or as it was if nothing changes.
    pub(crate) fn finish(self) -> Option<Vec<u8>> {
        let mut pdf = self.pdf.to_vec();
        if self.changed.is_empty() {
            return Some(pdf);
        }
        let mut offsets = Vec::with_capacity(self.changed.len());
        for (number, body) in &self.changed {
            pdf.push(b'\n');
            offsets.push((*number, pdf.len()));
            writeln!(pdf, "{number} 0 obj").ok()?;
            pdf.extend_from_slice(body);
            pdf.extend_from_slice(b"\nendobj\n");
        }
        let xref = pdf.len();
        pdf.extend_from_slice(b"xref\n");
        for (number, offset) in offsets {
            write!(pdf, "{number} 1\n{offset:010} 00000 n\r\n").ok()?;
        }
        let trailer = &self.references.trailer;
        write!(pdf, "trailer\n<</Size {}", self.next).ok()?;
        for key in ["/Root", "/Info", "/ID"] {
            if let Some(value) = parse::entry(trailer, key) {
                write!(pdf, "{key} {}", value.trim()).ok()?;
            }
        }
        write!(
            pdf,
            "/Prev {}>>\nstartxref\n{xref}\n%%EOF\n",
            self.references.start
        )
        .ok()?;
        Some(pdf)
    }
}

/// The body of a stream object with `dictionary` (without its length) and `data`.
pub(crate) fn stream(dictionary: &str, data: &[u8]) -> Vec<u8> {
    let open = dictionary.trim().strip_suffix(">>").unwrap_or(dictionary);
    let mut body = format!("{open}/Length {}>>\nstream\n", data.len()).into_bytes();
    body.extend_from_slice(data);
    body.extend_from_slice(b"\nendstream");
    body
}

/// The number of the object an indirect reference (`12 0 R`) refers to.
pub(crate) fn reference(value: &str) -> Option<u32> {
    let mut words = value.split_whitespace();
    let number = words.next()?.parse().ok()?;
    (words.next() == Some("0") && words.next() == Some("R")).then_some(number)
}

/// The references in an array of them (`[12 0 R 13 0 R]`).
pub(crate) fn references(array: &str) -> Vec<u32> {
    let inner = array.trim().trim_start_matches('[').trim_end_matches(']');
    let words: Vec<&str> = inner.split_whitespace().collect();
    words
        .chunks(3)
        .filter_map(|chunk| match chunk {
            [number, "0", "R"] => number.parse().ok(),
            _ => None,
        })
        .collect()
}

/// A dictionary (`<<` … `>>`) with an entry set: replaced where the key is there, added where
/// it is not.
pub(crate) fn with_entry(dictionary: &str, key: &str, value: &str) -> String {
    let dictionary = without_entry(dictionary, key);
    let open = dictionary.trim_end().strip_suffix(">>").unwrap_or(&dictionary);
    format!("{open}{key} {value}>>")
}

/// A dictionary without the entry of `key`.
pub(crate) fn without_entry(dictionary: &str, key: &str) -> String {
    let Some(value) = parse::entry(dictionary, key) else {
        return dictionary.to_owned();
    };
    // The value is a slice of the dictionary; the key comes right before it.
    let value_start = value.as_ptr() as usize - dictionary.as_ptr() as usize;
    let key_start = value_start - key.len();
    let value_end = value_start + value.len();
    format!("{}{}", &dictionary[..key_start], &dictionary[value_end..])
}

/// A PDF text string: literal for printable ASCII and line breaks, UTF-16 with a byte order
/// mark otherwise.
pub(crate) fn text(value: &str) -> String {
    if value
        .chars()
        .all(|character| matches!(character, ' '..='~' | '\n' | '\r' | '\t'))
    {
        let mut out = String::with_capacity(value.len() + 2);
        out.push('(');
        for character in value.chars() {
            match character {
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                '(' | ')' | '\\' => {
                    out.push('\\');
                    out.push(character);
                }
                _ => out.push(character),
            }
        }
        out.push(')');
        return out;
    }
    let mut out = String::from("<FEFF");
    for unit in value.encode_utf16() {
        let _ = write!(out, "{unit:04X}");
    }
    out.push('>');
    out
}

/// A PDF name (with its slash) for a value: bytes other than regular printable characters are
/// written as `#` and two hexadecimal digits.
pub(crate) fn name(value: &str) -> String {
    let mut out = String::from("/");
    for byte in value.bytes() {
        if byte.is_ascii_graphic() && !b"()<>[]{}/%#".contains(&byte) {
            out.push(byte as char);
        } else {
            let _ = write!(out, "#{byte:02X}");
        }
    }
    out
}

/// A number for a PDF, with at most four decimals.
pub(crate) fn number(value: f32) -> String {
    let rounded = (f64::from(value) * 10_000.0).round() / 10_000.0;
    if rounded.fract().abs() < f64::EPSILON {
        format!("{}", rounded as i64)
    } else {
        let text = format!("{rounded:.4}");
        text.trim_end_matches('0').to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_strings_names_and_numbers() {
        assert_eq!(text("Name (first)"), "(Name \\(first\\))");
        assert_eq!(text("Two\nlines"), "(Two\\nlines)");
        assert_eq!(text("Straße"), "<FEFF005300740072006100DF0065>");
        assert_eq!(name("on"), "/on");
        assert_eq!(name("a b/ä"), "/a#20b#2F#C3#A4");
        assert_eq!(number(12.0), "12");
        assert_eq!(number(0.75), "0.75");
        assert_eq!(number(-3.333_333), "-3.3333");
    }

    #[test]
    fn sets_and_removes_entries() {
        let dictionary = "<</Type/Annot/Subtype/Link/A<</S/URI/URI(x)>>/F 4>>";
        assert_eq!(
            with_entry(dictionary, "/Subtype", "/Widget"),
            "<</Type/Annot/A<</S/URI/URI(x)>>/F 4/Subtype /Widget>>"
        );
        assert_eq!(
            without_entry(dictionary, "/A"),
            "<</Type/Annot/Subtype/Link/F 4>>"
        );
        assert_eq!(references("[3 0 R 12 0 R]"), [3, 12]);
        assert_eq!(reference(" 7 0 R "), Some(7));
    }
}

//! The incremental update that carries a signature: a signature field on the first page, the
//! signature dictionary with room for the signature, and a cross-reference section of the same
//! kind as the file's own (a table, or a stream for files with object streams), appended to the
//! finished PDF (ISO 32000-1, §7.5.6). Nothing of the file before it changes, so the signature
//! covers every byte the reader sees except its own value.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Read as _;
use std::ops::Range;

use crate::parse;

/// The entries of the signature dictionary besides the signature itself, as PDF text.
pub(crate) struct Fields {
    /// `/M`, `/Name`, `/Reason`, `/Location`, `/ContactInfo`, already written as PDF.
    pub(crate) entries: String,
}

/// The PDF with the update appended and room for the signature.
pub(crate) struct Prepared {
    pub(crate) pdf: Vec<u8>,
    /// The hexadecimal string that will hold the signature, from `<` to `>`.
    pub(crate) contents: Range<usize>,
}

impl Prepared {
    /// The bytes the signature covers: everything but the hexadecimal string.
    pub(crate) fn signed_ranges(&self) -> [&[u8]; 2] {
        [&self.pdf[..self.contents.start], &self.pdf[self.contents.end..]]
    }

    /// Writes the signature (DER) into its room; `false` if it does not fit.
    pub(crate) fn fill(&mut self, signature: &[u8]) -> bool {
        let room = self.contents.len() - 2;
        if signature.len() * 2 > room {
            return false;
        }
        let mut hex = String::with_capacity(room);
        for byte in signature {
            write!(hex, "{byte:02x}").expect("written to a string");
        }
        hex.extend(std::iter::repeat_n('0', room - hex.len()));
        self.pdf[self.contents.start + 1..self.contents.end - 1].copy_from_slice(hex.as_bytes());
        true
    }
}

/// Width of each number of the byte range placeholder.
const RANGE_DIGITS: usize = 10;

/// Appends the signature's update to `pdf`, with room for a signature of `room` bytes.
pub(crate) fn prepare(pdf: &[u8], fields: &Fields, room: usize) -> Result<Prepared, String> {
    let file = File::read(pdf).ok_or("the PDF's cross-reference sections could not be read")?;
    let root = reference(parse::entry(&file.trailer, "/Root").ok_or("the PDF has no catalog")?)
        .ok_or("the PDF's catalog is not an indirect object")?;
    let catalog = file.dictionary(root)?;
    let first_page = file.first_page(&catalog)?;
    let page = file.dictionary(first_page)?;

    let mut next = file.size;
    let signature = next;
    let widget = next + 1;
    let appearance = next + 2;
    next += 3;
    // Objects of the update, by number: those that change and the new ones.
    let mut objects: BTreeMap<u32, String> = BTreeMap::new();

    // The widget goes into the page's annotations.
    let widget_reference = format!("{widget} 0 R");
    match parse::entry(&page, "/Annots").map(str::trim) {
        None => {
            objects.insert(
                first_page,
                set_entry(&page, "/Annots", &format!("[{widget_reference}]")),
            );
        }
        Some(annotations) if annotations.starts_with('[') => {
            objects.insert(
                first_page,
                set_entry(&page, "/Annots", &append(annotations, &widget_reference)?),
            );
        }
        Some(annotations) => {
            let number = reference(annotations).ok_or("the page's annotations cannot be read")?;
            let array = file.body(number)?;
            objects.insert(number, append(array.trim(), &widget_reference)?);
        }
    }

    // The field goes into the form of the catalog, which gets one if it has none.
    let mut field_name = 1;
    let mut catalog_entries: Vec<(&str, String)> = Vec::new();
    match parse::entry(&catalog, "/AcroForm").map(str::trim) {
        None => catalog_entries.push(("/AcroForm", format!("<</Fields[{widget_reference}]/SigFlags 3>>"))),
        Some(form) => {
            let (form_number, form) = if form.starts_with("<<") {
                (None, form.to_owned())
            } else {
                let number = reference(form).ok_or("the PDF's form cannot be read")?;
                (Some(number), file.body(number)?.trim().to_owned())
            };
            let fields = match parse::entry(&form, "/Fields").map(str::trim) {
                None => format!("[{widget_reference}]"),
                Some(fields) if fields.starts_with('[') => {
                    field_name = unused_name(&file, fields);
                    append(fields, &widget_reference)?
                }
                Some(fields) => {
                    let number = reference(fields).ok_or("the form's fields cannot be read")?;
                    let array = file.body(number)?;
                    field_name = unused_name(&file, array.trim());
                    objects.insert(number, append(array.trim(), &widget_reference)?);
                    fields.to_owned()
                }
            };
            let form = set_entry(&set_entry(&form, "/Fields", &fields), "/SigFlags", "3");
            match form_number {
                Some(number) => {
                    objects.insert(number, form);
                }
                None => catalog_entries.push(("/AcroForm", form)),
            }
        }
    }
    // PDF 1.7 knows the signatures of `PAdES` (`ETSI.CAdES.detached`) through an extension.
    if pdf.starts_with(b"%PDF-1.7") && parse::entry(&catalog, "/Extensions").is_none() {
        catalog_entries.push((
            "/Extensions",
            "<</ESIC<</BaseVersion/1.7/ExtensionLevel 2>>>>".to_owned(),
        ));
    }
    if !catalog_entries.is_empty() {
        let mut changed = catalog.clone();
        for (key, value) in &catalog_entries {
            changed = set_entry(&changed, key, value);
        }
        objects.insert(root, changed);
    }

    // An invisible signature: a widget of no size, printed (as PDF/A requires of annotations)
    // and locked, with an empty appearance (which PDF/A-1 requires of every field).
    objects.insert(
        widget,
        format!(
            "<</Type/Annot/Subtype/Widget/FT/Sig/T(Signature{field_name})/V {signature} 0 R\
             /F 132/Rect[0 0 0 0]/P {first_page} 0 R/AP<</N {appearance} 0 R>>>>"
        ),
    );
    objects.insert(
        appearance,
        "<</Type/XObject/Subtype/Form/BBox[0 0 0 0]/Resources<<>>/Length 0>>\nstream\n\nendstream".to_owned(),
    );
    let placeholder = format!("[0 {0:>RANGE_DIGITS$} {0:>RANGE_DIGITS$} {0:>RANGE_DIGITS$}]", "");
    objects.insert(
        signature,
        format!(
            "<</Type/Sig/Filter/Adobe.PPKLite/SubFilter/ETSI.CAdES.detached{}/ByteRange{placeholder}\
             /Contents<{}>>>",
            fields.entries,
            "0".repeat(room * 2)
        ),
    );

    let mut out = pdf.to_vec();
    if !matches!(out.last(), Some(b'\n' | b'\r')) {
        out.push(b'\n');
    }
    let mut offsets: BTreeMap<u32, usize> = BTreeMap::new();
    let mut contents = None;
    for (number, body) in &objects {
        offsets.insert(*number, out.len());
        writeln!(TextOut(&mut out), "{number} 0 obj").map_err(|_| "write")?;
        if *number == signature {
            let start = out.len() + body.rfind("/Contents<").ok_or("contents")? + "/Contents".len();
            contents = Some(start..start + room * 2 + 2);
        }
        out.extend_from_slice(body.as_bytes());
        out.extend_from_slice(b"\nendobj\n");
    }
    let mut trailer = String::new();
    for key in ["/Root", "/Info", "/ID"] {
        if let Some(value) = parse::entry(&file.trailer, key) {
            write!(trailer, "{key} {}", value.trim()).map_err(|_| "write")?;
        }
    }
    write!(trailer, "/Prev {}", file.start).map_err(|_| "write")?;
    let xref = out.len();
    if file.stream {
        // The cross-reference stream lists itself too.
        let number = next;
        next += 1;
        offsets.insert(number, xref);
        let width = (1..8)
            .find(|bytes| {
                offsets
                    .values()
                    .all(|&offset| (offset as u64) < 1_u64 << (8 * bytes))
            })
            .unwrap_or(8);
        let mut index = String::new();
        let mut table = Vec::new();
        for (object, offset) in &offsets {
            write!(index, "{object} 1 ").map_err(|_| "write")?;
            table.push(1);
            table.extend_from_slice(&(*offset as u64).to_be_bytes()[8 - width..]);
            table.extend_from_slice(&[0, 0]);
        }
        write!(
            TextOut(&mut out),
            "{number} 0 obj\n<</Type/XRef/Size {next}/W[1 {width} 2]/Index[{}]{trailer}/Length {}>>\nstream\n",
            index.trim_end(),
            table.len()
        )
        .map_err(|_| "write")?;
        out.extend_from_slice(&table);
        out.extend_from_slice(b"\nendstream\nendobj\n");
    } else {
        out.extend_from_slice(b"xref\n");
        // Subsections of consecutive numbers.
        let numbers: Vec<u32> = offsets.keys().copied().collect();
        let mut start = 0;
        while start < numbers.len() {
            let mut end = start + 1;
            while end < numbers.len() && numbers[end] == numbers[end - 1] + 1 {
                end += 1;
            }
            writeln!(TextOut(&mut out), "{} {}", numbers[start], end - start).map_err(|_| "write")?;
            for number in &numbers[start..end] {
                write!(TextOut(&mut out), "{:010} 00000 n\r\n", offsets[number]).map_err(|_| "write")?;
            }
            start = end;
        }
        write!(TextOut(&mut out), "trailer\n<</Size {next}{trailer}>>\n").map_err(|_| "write")?;
    }
    write!(TextOut(&mut out), "startxref\n{xref}\n%%EOF\n").map_err(|_| "write")?;

    let contents = contents.ok_or("the signature dictionary is missing")?;
    let range = format!(
        "[0 {:>RANGE_DIGITS$} {:>RANGE_DIGITS$} {:>RANGE_DIGITS$}]",
        contents.start,
        contents.end,
        out.len() - contents.end
    );
    if range.len() != placeholder.len() {
        return Err("the PDF is too large to be signed".to_owned());
    }
    let at = parse::rfind(&out[..contents.start], placeholder.as_bytes()).ok_or("byte range")?;
    out[at..at + range.len()].copy_from_slice(range.as_bytes());
    Ok(Prepared { pdf: out, contents })
}

/// Writes formatted text into a byte buffer.
struct TextOut<'a>(&'a mut Vec<u8>);

impl std::fmt::Write for TextOut<'_> {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        self.0.extend_from_slice(text.as_bytes());
        Ok(())
    }
}

/// The number of `N 0 R`.
pub(crate) fn reference(value: &str) -> Option<u32> {
    let mut parts = value.split_whitespace();
    let number = parts.next()?.parse().ok()?;
    (parts.next()? == "0" && parts.next()? == "R" && parts.next().is_none()).then_some(number)
}

/// An array (`[…]`) with `item` added at its end.
fn append(array: &str, item: &str) -> Result<String, String> {
    let inner = array
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .ok_or_else(|| format!("`{array}` is not an array"))?;
    let inner = inner.trim();
    Ok(if inner.is_empty() {
        format!("[{item}]")
    } else {
        format!("[{inner} {item}]")
    })
}

/// A dictionary with `key` set to `value`: replaced where it has the key, added otherwise.
fn set_entry(dictionary: &str, key: &str, value: &str) -> String {
    if let Some(old) = parse::entry(dictionary, key) {
        // `entry` returns a part of the dictionary: its position follows from the addresses.
        let start = old.as_ptr() as usize - dictionary.as_ptr() as usize;
        let end = start + old.len();
        let space = if value.starts_with(['/', '<', '[', '(']) {
            ""
        } else {
            " "
        };
        return format!("{}{space}{value}{}", &dictionary[..start], &dictionary[end..]);
    }
    let open = dictionary.trim_end();
    let open = open.strip_suffix(">>").unwrap_or(open);
    let space = if value.starts_with(['/', '<', '[', '(']) {
        ""
    } else {
        " "
    };
    format!("{open}{key}{space}{value}>>")
}

/// The first number `n` for which no field of `fields` (an array of references) is called
/// `Signaturen`.
fn unused_name(file: &File<'_>, fields: &str) -> u32 {
    let names: Vec<String> = fields
        .trim_matches(['[', ']'])
        .split(" R")
        .filter_map(|reference| reference.split_whitespace().next()?.parse::<u32>().ok())
        .filter_map(|number| file.body(number).ok())
        .filter_map(|body| Some(parse::entry(&body, "/T")?.trim().to_owned()))
        .collect();
    (1..=u32::try_from(names.len()).unwrap_or(u32::MAX).saturating_add(1))
        .find(|n| !names.iter().any(|name| name == &format!("(Signature{n})")))
        .unwrap_or(1)
}

/// Where an object is.
#[derive(Clone, Copy)]
enum Location {
    /// At an offset of the file.
    Offset(usize),
    /// In an object stream, at an index.
    Compressed(u32, usize),
}

/// A finished PDF, read through its cross-reference sections.
pub(crate) struct File<'a> {
    pdf: &'a [u8],
    objects: BTreeMap<u32, Location>,
    /// The newest trailer, or the dictionary of the newest cross-reference stream.
    pub(crate) trailer: String,
    /// Where the newest cross-reference section starts.
    start: usize,
    /// Whether the newest section is a stream.
    stream: bool,
    /// One more than the highest object number.
    size: u32,
}

impl<'a> File<'a> {
    pub(crate) fn read(pdf: &'a [u8]) -> Option<Self> {
        let startxref = parse::rfind(pdf, b"startxref")?;
        let start = parse::number_after(pdf, startxref + 9)?.0;
        let mut objects = BTreeMap::new();
        let mut position = start;
        let mut newest: Option<(String, bool)> = None;
        // Sections back through `/Prev`, newest first; the newest location of an object wins.
        for _ in 0..64 {
            let trailer = if pdf.get(position..)?.starts_with(b"xref") {
                table(pdf, position + 4, &mut objects)?
            } else {
                xref_stream(pdf, position, &mut objects)?
            };
            if trailer.contains("/Encrypt") {
                return None;
            }
            let stream = !pdf.get(position..)?.starts_with(b"xref");
            let previous = parse::entry(&trailer, "/Prev").and_then(|value| value.trim().parse().ok());
            // A hybrid file's table points to a stream with more objects.
            if let Some(hybrid) =
                parse::entry(&trailer, "/XRefStm").and_then(|value| value.trim().parse().ok())
            {
                xref_stream(pdf, hybrid, &mut objects)?;
            }
            newest.get_or_insert((trailer, stream));
            match previous {
                Some(previous) => position = previous,
                None => break,
            }
        }
        let (trailer, stream) = newest?;
        let size = parse::entry(&trailer, "/Size")?.trim().parse::<u32>().ok()?;
        let size = size.max(objects.keys().next_back().map_or(0, |last| last + 1));
        Some(Self {
            pdf,
            objects,
            trailer,
            start,
            stream,
            size,
        })
    }

    /// The text of object `number`: what lies between `obj` and `endobj`.
    fn body(&self, number: u32) -> Result<String, String> {
        let missing = || format!("object {number} of the PDF cannot be read");
        let bytes = match *self.objects.get(&number).ok_or_else(missing)? {
            Location::Offset(offset) => {
                let object = parse::object(self.pdf, number, offset).ok_or_else(missing)?;
                if object.data.is_some() {
                    return Err(missing());
                }
                parse::trim(&self.pdf[object.body]).to_vec()
            }
            Location::Compressed(stream, index) => {
                let Some(Location::Offset(offset)) = self.objects.get(&stream).copied() else {
                    return Err(missing());
                };
                compressed(self.pdf, stream, offset, number, index).ok_or_else(missing)?
            }
        };
        String::from_utf8(bytes).map_err(|_| missing())
    }

    /// The text of object `number`, which must be a dictionary.
    pub(crate) fn dictionary(&self, number: u32) -> Result<String, String> {
        let body = self.body(number)?;
        if body.starts_with("<<") {
            Ok(body)
        } else {
            Err(format!("object {number} of the PDF is not a dictionary"))
        }
    }

    /// The number of the first page, through the page tree.
    pub(crate) fn first_page(&self, catalog: &str) -> Result<u32, String> {
        let mut node = parse::entry(catalog, "/Pages")
            .and_then(reference)
            .ok_or("the PDF has no pages")?;
        for _ in 0..64 {
            let dictionary = self.dictionary(node)?;
            if parse::entry(&dictionary, "/Type").is_some_and(|kind| kind.trim() == "/Page") {
                return Ok(node);
            }
            let kids = parse::entry(&dictionary, "/Kids").ok_or("a page tree node has no kids")?;
            let first = kids.trim().trim_start_matches('[');
            let end = first.find('R').ok_or("the PDF has no pages")?;
            node = reference(&first[..=end]).ok_or("the page tree cannot be read")?;
        }
        Err("the page tree is too deep".to_owned())
    }
}

/// Reads a cross-reference table from `position` (after `xref`) and returns its trailer.
fn table(pdf: &[u8], mut position: usize, objects: &mut BTreeMap<u32, Location>) -> Option<String> {
    loop {
        position = parse::skip_whitespace(pdf, position);
        if pdf.get(position..)?.starts_with(b"trailer") {
            break;
        }
        let (first, after) = parse::number_after(pdf, position)?;
        let (count, after) = parse::number_after(pdf, after)?;
        position = parse::skip_whitespace(pdf, after);
        for entry in 0..count {
            let line = pdf.get(position..position + 18)?;
            let offset: usize = std::str::from_utf8(&line[..10]).ok()?.parse().ok()?;
            let number = u32::try_from(first + entry).ok()?;
            if line[17] == b'n' {
                objects.entry(number).or_insert(Location::Offset(offset));
            }
            position = parse::skip_whitespace(pdf, position + 18);
        }
    }
    let dictionary = parse::skip_whitespace(pdf, position + 7);
    let end = parse::dictionary_end(pdf, dictionary)?;
    Some(std::str::from_utf8(&pdf[dictionary..end]).ok()?.to_owned())
}

/// Reads the cross-reference stream at `offset` and returns its dictionary.
fn xref_stream(pdf: &[u8], offset: usize, objects: &mut BTreeMap<u32, Location>) -> Option<String> {
    let number = u32::try_from(parse::number_after(pdf, offset)?.0).ok()?;
    let (dictionary, data) = stream(pdf, number, offset)?;
    if parse::entry(&dictionary, "/Type")?.trim() != "/XRef" {
        return None;
    }
    let widths = numbers(parse::entry(&dictionary, "/W")?)?;
    let [kind_width, second_width, third_width] = widths[..] else {
        return None;
    };
    let size = parse::entry(&dictionary, "/Size")?.trim().parse::<usize>().ok()?;
    let index = match parse::entry(&dictionary, "/Index") {
        Some(index) => numbers(index)?,
        None => vec![0, size],
    };
    let row = kind_width + second_width + third_width;
    if row == 0 {
        return None;
    }
    let field = |bytes: &[u8]| {
        bytes
            .iter()
            .fold(0_usize, |value, &byte| value << 8 | usize::from(byte))
    };
    let mut rows = data.chunks_exact(row);
    for pair in index.chunks(2) {
        let [first, count] = pair[..] else {
            return None;
        };
        for entry in 0..count {
            let row = rows.next()?;
            // A missing type field means type 1.
            let kind = if kind_width == 0 {
                1
            } else {
                field(&row[..kind_width])
            };
            let second = field(&row[kind_width..kind_width + second_width]);
            let third = field(&row[kind_width + second_width..]);
            let number = u32::try_from(first + entry).ok()?;
            match kind {
                1 => {
                    objects.entry(number).or_insert(Location::Offset(second));
                }
                2 => {
                    objects
                        .entry(number)
                        .or_insert(Location::Compressed(u32::try_from(second).ok()?, third));
                }
                _ => {}
            }
        }
    }
    Some(dictionary)
}

/// The dictionary and the decoded data of the stream object `number` at `offset`.
fn stream(pdf: &[u8], number: u32, offset: usize) -> Option<(String, Vec<u8>)> {
    let object = parse::object(pdf, number, offset)?;
    let data = &pdf[object.data?];
    let start = parse::skip_whitespace(pdf, object.body.start);
    let end = parse::dictionary_end(pdf, start)?;
    let dictionary = std::str::from_utf8(&pdf[start..end]).ok()?.to_owned();
    if parse::entry(&dictionary, "/DecodeParms").is_some() {
        return None;
    }
    let data = match parse::entry(&dictionary, "/Filter").map(str::trim) {
        None => data.to_vec(),
        Some("/FlateDecode" | "[/FlateDecode]") => {
            let mut out = Vec::new();
            flate2::read::ZlibDecoder::new(data).read_to_end(&mut out).ok()?;
            out
        }
        Some(_) => return None,
    };
    Some((dictionary, data))
}

/// Object `number`, at `index` of the object stream `stream` at `offset`.
fn compressed(pdf: &[u8], stream_number: u32, offset: usize, number: u32, index: usize) -> Option<Vec<u8>> {
    let (dictionary, data) = stream(pdf, stream_number, offset)?;
    let first: usize = parse::entry(&dictionary, "/First")?.trim().parse().ok()?;
    let pairs = numbers(std::str::from_utf8(data.get(..first)?).ok()?)?;
    let found = *pairs.get(index * 2)?;
    if found != number as usize {
        return None;
    }
    let start = first + pairs.get(index * 2 + 1)?;
    let end = pairs.get(index * 2 + 3).map_or(data.len(), |next| first + next);
    Some(parse::trim(data.get(start..end)?).to_vec())
}

/// The numbers of a text like `[1 2 3]`.
fn numbers(text: &str) -> Option<Vec<usize>> {
    text.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split_whitespace()
        .map(|number| number.parse().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sets_entries_of_dictionaries() {
        assert_eq!(
            set_entry("<</Type/Page/Annots[1 0 R]>>", "/Annots", "[1 0 R 2 0 R]"),
            "<</Type/Page/Annots[1 0 R 2 0 R]>>"
        );
        assert_eq!(
            set_entry("<</Type/Page>>", "/Annots", "[2 0 R]"),
            "<</Type/Page/Annots[2 0 R]>>"
        );
        assert_eq!(
            set_entry("<</SigFlags 1/Fields[]>>", "/SigFlags", "3"),
            "<</SigFlags 3/Fields[]>>"
        );
        assert_eq!(append("[ ]", "4 0 R").expect("array"), "[4 0 R]");
        assert_eq!(append("[1 0 R]", "4 0 R").expect("array"), "[1 0 R 4 0 R]");
        assert_eq!(reference(" 12 0 R "), Some(12));
        assert_eq!(reference("[12 0 R]"), None);
    }
}

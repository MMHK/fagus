//! The incremental updates that carry a signature: a signature field on the first page (or an
//! empty field of the PDF signed in), the signature dictionary with room for the signature, and
//! a cross-reference section of the same kind as the file's own (a table, or a stream for files
//! with object streams), appended to the finished PDF (ISO 32000-1, §7.5.6); and the update that
//! adds the validation data of long-term signatures (the document security store, ETSI EN
//! 319 142-1, §5.4). Nothing of the file before an update changes, so the signature covers every
//! byte the reader sees except its own value.
//!
//! In an encrypted PDF, the update's strings and streams are encrypted with the file's key, as
//! the objects before them are, except the signature itself (ISO 32000-2, §7.6.2).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Read as _;
use std::ops::Range;

use crate::encrypt::{self, Encryptor};
use crate::parse;

/// What an update signs with.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A signature (`/Sig`, `ETSI.CAdES.detached`).
    Signature,
    /// A document timestamp (`/DocTimeStamp`, `ETSI.RFC3161`), for `PAdES` B-LTA.
    Timestamp,
}

/// The entries of the signature dictionary besides the signature itself, as PDF text.
pub(crate) struct Fields {
    pub(crate) kind: Kind,
    /// `/M`, `/Name`, `/Reason`, `/Location`, `/ContactInfo`, already written as PDF, with
    /// their strings not encrypted.
    pub(crate) entries: String,
    /// The permissions of a certification signature (`DocMDP`, 1 to 3).
    pub(crate) certify: Option<u8>,
    /// Whether to sign in the PDF's first empty signature field, a visible one, instead of a
    /// new invisible field.
    pub(crate) in_field: bool,
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

/// The encryption dictionary of `pdf`, if it is encrypted.
pub(crate) fn encryption(pdf: &[u8]) -> Result<Option<String>, String> {
    let file = File::read(pdf).ok_or("the PDF's cross-reference sections could not be read")?;
    file.encryption()
}

/// An update being made: the objects that change and the new ones, by number.
struct Revision<'f, 'a> {
    file: &'f File<'a>,
    objects: BTreeMap<u32, Vec<u8>>,
    next: u32,
}

impl Revision<'_, '_> {
    /// A new object number.
    fn reserve(&mut self) -> u32 {
        self.next += 1;
        self.next - 1
    }

    /// Sets an object to `body`, whose strings are written as they are to be read: encrypted
    /// here for an encrypted PDF. Strings of the PDF that `body` keeps are encrypted already
    /// unless the object comes from an object stream, whose strings are not.
    fn set(
        &mut self,
        number: u32,
        body: &str,
        fresh: bool,
        encryptor: &mut Option<&mut Encryptor>,
    ) -> Result<(), String> {
        let bytes = match encryptor {
            Some(encryptor) if fresh || self.file.compressed(number) => encryptor
                .encrypt_strings(body.as_bytes())
                .ok_or("the update could not be encrypted")?,
            _ => body.as_bytes().to_vec(),
        };
        self.objects.insert(number, bytes);
        Ok(())
    }

    /// Sets an object to a new stream of `dictionary` (without its length) and `data`.
    fn set_stream(
        &mut self,
        number: u32,
        dictionary: &str,
        data: &[u8],
        encryptor: &mut Option<&mut Encryptor>,
    ) -> Result<(), String> {
        let (dictionary, data) = match encryptor {
            Some(encryptor) => (
                String::from_utf8(
                    encryptor
                        .encrypt_strings(dictionary.as_bytes())
                        .ok_or("the update could not be encrypted")?,
                )
                .map_err(|_| "the update could not be encrypted")?,
                encryptor
                    .encrypt(data)
                    .ok_or("the update could not be encrypted")?,
            ),
            None => (dictionary.to_owned(), data.to_vec()),
        };
        self.objects
            .insert(number, crate::update::stream(&dictionary, &data));
        Ok(())
    }

    /// The file with the update appended, and where each of its objects starts.
    fn write(mut self) -> Result<(Vec<u8>, BTreeMap<u32, usize>), String> {
        let file = self.file;
        let mut out = file.pdf.to_vec();
        if !matches!(out.last(), Some(b'\n' | b'\r')) {
            out.push(b'\n');
        }
        let mut offsets: BTreeMap<u32, usize> = BTreeMap::new();
        for (number, body) in &self.objects {
            offsets.insert(*number, out.len());
            writeln!(TextOut(&mut out), "{number} 0 obj").map_err(|_| "write")?;
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let mut trailer = String::new();
        for key in ["/Root", "/Info", "/ID", "/Encrypt"] {
            if let Some(value) = parse::entry(&file.trailer, key) {
                write!(trailer, "{key} {}", value.trim()).map_err(|_| "write")?;
            }
        }
        write!(trailer, "/Prev {}", file.start).map_err(|_| "write")?;
        let xref = out.len();
        let mut written = offsets.clone();
        if file.stream {
            // The cross-reference stream lists itself too; it is not encrypted.
            let number = self.reserve();
            written.insert(number, xref);
            let width = (1..8)
                .find(|bytes| {
                    written
                        .values()
                        .all(|&offset| (offset as u64) < 1_u64 << (8 * bytes))
                })
                .unwrap_or(8);
            let mut index = String::new();
            let mut table = Vec::new();
            for (object, offset) in &written {
                write!(index, "{object} 1 ").map_err(|_| "write")?;
                table.push(1);
                table.extend_from_slice(&(*offset as u64).to_be_bytes()[8 - width..]);
                table.extend_from_slice(&[0, 0]);
            }
            write!(
                TextOut(&mut out),
                "{number} 0 obj\n<</Type/XRef/Size {}/W[1 {width} 2]/Index[{}]{trailer}/Length {}>>\nstream\n",
                self.next,
                index.trim_end(),
                table.len()
            )
            .map_err(|_| "write")?;
            out.extend_from_slice(&table);
            out.extend_from_slice(b"\nendstream\nendobj\n");
        } else {
            out.extend_from_slice(b"xref\n");
            // Subsections of consecutive numbers.
            let numbers: Vec<u32> = written.keys().copied().collect();
            let mut start = 0;
            while start < numbers.len() {
                let mut end = start + 1;
                while end < numbers.len() && numbers[end] == numbers[end - 1] + 1 {
                    end += 1;
                }
                writeln!(TextOut(&mut out), "{} {}", numbers[start], end - start).map_err(|_| "write")?;
                for number in &numbers[start..end] {
                    write!(TextOut(&mut out), "{:010} 00000 n\r\n", written[number]).map_err(|_| "write")?;
                }
                start = end;
            }
            write!(TextOut(&mut out), "trailer\n<</Size {}{trailer}>>\n", self.next).map_err(|_| "write")?;
        }
        write!(TextOut(&mut out), "startxref\n{xref}\n%%EOF\n").map_err(|_| "write")?;
        Ok((out, offsets))
    }
}

/// Reads `pdf` for an update, with the file key of an encrypted one.
fn open<'a>(pdf: &'a [u8], encryptor: Option<&Encryptor>) -> Result<File<'a>, String> {
    let mut file = File::read(pdf).ok_or("the PDF's cross-reference sections could not be read")?;
    if file.encryption()?.is_some() {
        let encryptor = encryptor.ok_or("the PDF is encrypted, and its password was not given")?;
        file.key = Some(*encryptor.key());
    }
    Ok(file)
}

/// Appends the signature's update to `pdf`, with room for a signature of `room` bytes.
pub(crate) fn prepare(
    pdf: &[u8],
    fields: &Fields,
    room: usize,
    mut encryptor: Option<&mut Encryptor>,
) -> Result<Prepared, String> {
    let file = open(pdf, encryptor.as_deref())?;
    let root = reference(parse::entry(&file.trailer, "/Root").ok_or("the PDF has no catalog")?)
        .ok_or("the PDF's catalog is not an indirect object")?;
    let catalog = file.dictionary(root)?;
    let mut revision = Revision {
        file: &file,
        objects: BTreeMap::new(),
        next: file.size,
    };
    let signature = revision.reserve();
    let signature_reference = format!("{signature} 0 R");

    // The form of the catalog, which gets one if it has none, and its fields.
    let mut catalog_entries: Vec<(&str, String)> = Vec::new();
    let (form_number, form) = match parse::entry(&catalog, "/AcroForm").map(str::trim) {
        None => (None, "<<>>".to_owned()),
        Some(form) if form.starts_with("<<") => (None, form.to_owned()),
        Some(form) => {
            let number = reference(form).ok_or("the PDF's form cannot be read")?;
            (Some(number), file.dictionary(number)?)
        }
    };
    let (fields_number, field_array) = match parse::entry(&form, "/Fields").map(str::trim) {
        None => (None, "[]".to_owned()),
        Some(array) if array.starts_with('[') => (None, array.to_owned()),
        Some(array) => {
            let number = reference(array).ok_or("the form's fields cannot be read")?;
            (Some(number), file.body(number)?.trim().to_owned())
        }
    };
    let existing = file.fields(&field_array);
    if fields.certify.is_some() && existing.iter().any(|field| field.signed) {
        return Err("a certification signature must be the first signature of the PDF".to_owned());
    }
    if fields.certify.is_some()
        && parse::entry(&catalog, "/Perms").is_some_and(|perms| perms.contains("/DocMDP"))
    {
        return Err("the PDF is certified already".to_owned());
    }

    let mut new_field_array = None;
    if fields.in_field {
        // An empty signature field of the PDF: it gets the signature as its value.
        let field = existing
            .iter()
            .find(|field| field.signature && !field.signed)
            .ok_or("the PDF has no empty signature field")?;
        let body = file.dictionary(field.number)?;
        revision.set(
            field.number,
            &set_entry(&body, "/V", &signature_reference),
            false,
            &mut encryptor,
        )?;
    } else {
        // A new invisible field on the first page: a widget of no size, printed (as PDF/A
        // requires of annotations) and locked, with an empty appearance (which PDF/A-1
        // requires of every field).
        let first_page = file.first_page(&catalog)?;
        let page = file.dictionary(first_page)?;
        let widget = revision.reserve();
        let widget_reference = format!("{widget} 0 R");
        match parse::entry(&page, "/Annots").map(str::trim) {
            None => revision.set(
                first_page,
                &set_entry(&page, "/Annots", &format!("[{widget_reference}]")),
                false,
                &mut encryptor,
            )?,
            Some(annotations) if annotations.starts_with('[') => revision.set(
                first_page,
                &set_entry(&page, "/Annots", &append(annotations, &widget_reference)?),
                false,
                &mut encryptor,
            )?,
            Some(annotations) => {
                let number = reference(annotations).ok_or("the page's annotations cannot be read")?;
                let array = file.body(number)?;
                revision.set(
                    number,
                    &append(array.trim(), &widget_reference)?,
                    false,
                    &mut encryptor,
                )?;
            }
        }
        let name = (1..=u32::try_from(existing.len())
            .unwrap_or(u32::MAX)
            .saturating_add(1))
            .find(|n| !existing.iter().any(|field| field.name == format!("Signature{n}")))
            .unwrap_or(1);
        // A document timestamp's field has no appearance where none is needed (PDF/A-2 and
        // later exempt annotations of no size): validators take a new appearance after a
        // certification that allows no changes for a change of the form.
        let appearance =
            (fields.kind == Kind::Signature || pdf.starts_with(b"%PDF-1.4") || pdf.starts_with(b"%PDF-1.3"))
                .then(|| revision.reserve());
        revision.set(
            widget,
            &format!(
                "<</Type/Annot/Subtype/Widget/FT/Sig/T(Signature{name})/V {signature_reference}\
                     /F 132/Rect[0 0 0 0]/P {first_page} 0 R{}>>",
                appearance.map_or_else(String::new, |appearance| format!("/AP<</N {appearance} 0 R>>"))
            ),
            true,
            &mut encryptor,
        )?;
        if let Some(appearance) = appearance {
            revision.set_stream(
                appearance,
                "<</Type/XObject/Subtype/Form/BBox[0 0 0 0]/Resources<<>>>>",
                b"",
                &mut encryptor,
            )?;
        }
        new_field_array = Some(append(&field_array, &widget_reference)?);
    }

    // The form, with the new field and the flags of a signed PDF.
    let mut form = set_entry(&form, "/SigFlags", "3");
    if let Some(array) = new_field_array {
        match fields_number {
            Some(number) => revision.set(number, &array, false, &mut encryptor)?,
            None => form = set_entry(&form, "/Fields", &array),
        }
    }
    match form_number {
        Some(number) => revision.set(number, &form, false, &mut encryptor)?,
        None => catalog_entries.push(("/AcroForm", form)),
    }
    // PDF 1.7 knows the signatures of `PAdES` (`ETSI.CAdES.detached`) through an extension.
    if pdf.starts_with(b"%PDF-1.7") {
        let extension = "<</BaseVersion/1.7/ExtensionLevel 2>>";
        match parse::entry(&catalog, "/Extensions").map(str::trim) {
            None => catalog_entries.push(("/Extensions", format!("<</ESIC{extension}>>"))),
            Some(extensions) if extensions.starts_with("<<") && !extensions.contains("/ESIC") => {
                catalog_entries.push(("/Extensions", set_entry(extensions, "/ESIC", extension)));
            }
            Some(_) => {}
        }
    }
    if fields.certify.is_some() {
        let perms = match parse::entry(&catalog, "/Perms").map(str::trim) {
            Some(perms) if perms.starts_with("<<") => set_entry(perms, "/DocMDP", &signature_reference),
            Some(_) => return Err("the permissions of the PDF cannot be read".to_owned()),
            None => format!("<</DocMDP {signature_reference}>>"),
        };
        catalog_entries.push(("/Perms", perms));
    }
    if !catalog_entries.is_empty() {
        let mut changed = catalog.clone();
        for (key, value) in &catalog_entries {
            changed = set_entry(&changed, key, value);
        }
        revision.set(root, &changed, false, &mut encryptor)?;
    }

    // The signature dictionary: its strings encrypted, the signature itself not.
    let (kind, sub_filter) = match fields.kind {
        Kind::Signature => ("/Sig", "/ETSI.CAdES.detached"),
        Kind::Timestamp => ("/DocTimeStamp", "/ETSI.RFC3161"),
    };
    let mut entries = fields.entries.clone();
    if let Some(permissions) = fields.certify {
        write!(
            entries,
            "/Reference[<</Type/SigRef/TransformMethod/DocMDP\
             /TransformParams<</Type/TransformParams/P {permissions}/V/1.2>>>>]"
        )
        .map_err(|_| "write")?;
    }
    let entries = match &mut encryptor {
        Some(encryptor) => String::from_utf8(
            encryptor
                .encrypt_strings(entries.as_bytes())
                .ok_or("the update could not be encrypted")?,
        )
        .map_err(|_| "the update could not be encrypted")?,
        None => entries,
    };
    let placeholder = format!("[0 {0:>RANGE_DIGITS$} {0:>RANGE_DIGITS$} {0:>RANGE_DIGITS$}]", "");
    let body = format!(
        "<</Type{kind}/Filter/Adobe.PPKLite/SubFilter{sub_filter}{entries}/ByteRange{placeholder}\
         /Contents<{}>>>",
        "0".repeat(room * 2)
    );
    let at = body.rfind("/Contents<").ok_or("contents")? + "/Contents".len();
    revision.objects.insert(signature, body.into_bytes());

    let (mut out, offsets) = revision.write()?;
    let start = offsets[&signature] + format!("{signature} 0 obj\n").len() + at;
    let contents = start..start + room * 2 + 2;
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

/// The validation data of long-term signatures: certificates, OCSP responses and CRLs, each DER.
#[derive(Default)]
pub(crate) struct Validation {
    pub(crate) certificates: Vec<Vec<u8>>,
    pub(crate) ocsp_responses: Vec<Vec<u8>>,
    pub(crate) crls: Vec<Vec<u8>>,
}

/// Appends an update that adds `validation` to the document security store (`/DSS`) of the
/// catalog, which gets one if it has none.
pub(crate) fn add_validation(
    pdf: &[u8],
    validation: &Validation,
    mut encryptor: Option<&mut Encryptor>,
) -> Result<Vec<u8>, String> {
    let file = open(pdf, encryptor.as_deref())?;
    let root = reference(parse::entry(&file.trailer, "/Root").ok_or("the PDF has no catalog")?)
        .ok_or("the PDF's catalog is not an indirect object")?;
    let catalog = file.dictionary(root)?;
    let mut revision = Revision {
        file: &file,
        objects: BTreeMap::new(),
        next: file.size,
    };
    let (store_number, mut store) = match parse::entry(&catalog, "/DSS").map(str::trim) {
        None => (None, "<<>>".to_owned()),
        Some(store) if store.starts_with("<<") => (None, store.to_owned()),
        Some(store) => {
            let number = reference(store).ok_or("the PDF's document security store cannot be read")?;
            (Some(number), file.dictionary(number)?)
        }
    };
    for (key, items) in [
        ("/Certs", &validation.certificates),
        ("/OCSPs", &validation.ocsp_responses),
        ("/CRLs", &validation.crls),
    ] {
        if items.is_empty() {
            continue;
        }
        let mut array = match parse::entry(&store, key).map(str::trim) {
            None => "[]".to_owned(),
            Some(array) if array.starts_with('[') => array.to_owned(),
            Some(array) => {
                let number = reference(array).ok_or("the PDF's document security store cannot be read")?;
                file.body(number)?.trim().to_owned()
            }
        };
        for item in items {
            let number = revision.reserve();
            let compressed = deflate(item);
            revision.set_stream(number, "<</Filter/FlateDecode>>", &compressed, &mut encryptor)?;
            array = append(&array, &format!("{number} 0 R"))?;
        }
        store = set_entry(&store, key, &array);
    }
    let mut changed = catalog.clone();
    if let Some(number) = store_number {
        revision.set(number, &store, false, &mut encryptor)?;
    } else {
        let number = revision.reserve();
        revision.set(number, &store, true, &mut encryptor)?;
        changed = set_entry(&changed, "/DSS", &format!("{number} 0 R"));
    }
    // PDF 1.7 knows the document security store and document timestamps through the fifth
    // level of ETSI's extension (the second, of the signature, knows only the signature).
    if pdf.starts_with(b"%PDF-1.7")
        && let Some(extensions) = parse::entry(&changed, "/Extensions").map(str::trim)
        && extensions.starts_with("<<")
    {
        let extensions = set_entry(extensions, "/ESIC", "<</BaseVersion/1.7/ExtensionLevel 5>>");
        changed = set_entry(&changed, "/Extensions", &extensions);
    }
    if changed != catalog {
        revision.set(root, &changed, false, &mut encryptor)?;
    }
    Ok(revision.write()?.0)
}

/// Data compressed with Flate.
fn deflate(data: &[u8]) -> Vec<u8> {
    use std::io::Write as _;
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(data).expect("written to memory");
    encoder.finish().expect("written to memory")
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

/// A field of the PDF's form, as signing needs to know it.
pub(crate) struct FormField {
    pub(crate) number: u32,
    /// Its partial name (`/T`).
    pub(crate) name: String,
    /// Whether it is a signature field (`/FT/Sig`).
    pub(crate) signature: bool,
    /// Whether it has a value: for a signature field, whether it is signed.
    pub(crate) signed: bool,
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
    /// The file key of an encrypted PDF.
    key: Option<[u8; 32]>,
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
            key: None,
        })
    }

    /// Reads the objects of an encrypted PDF with its file key.
    #[cfg(test)]
    pub(crate) fn unlock(&mut self, key: [u8; 32]) {
        self.key = Some(key);
    }

    /// The encryption dictionary, if the PDF is encrypted.
    pub(crate) fn encryption(&self) -> Result<Option<String>, String> {
        match parse::entry(&self.trailer, "/Encrypt").map(str::trim) {
            None => Ok(None),
            Some(dictionary) if dictionary.starts_with("<<") => Ok(Some(dictionary.to_owned())),
            Some(dictionary) => {
                let number = reference(dictionary).ok_or("the PDF's encryption cannot be read")?;
                self.dictionary(number).map(Some)
            }
        }
    }

    /// Whether object `number` lies in an object stream, where its strings are not encrypted
    /// on their own.
    fn compressed(&self, number: u32) -> bool {
        matches!(self.objects.get(&number), Some(Location::Compressed(..)))
    }

    /// The fields of a form's array of fields (`[…]`), with their names as text.
    pub(crate) fn fields(&self, array: &str) -> Vec<FormField> {
        array
            .trim_matches(['[', ']'])
            .split(" R")
            .filter_map(|reference| reference.split_whitespace().next()?.parse::<u32>().ok())
            .filter_map(|number| {
                let body = self.dictionary(number).ok()?;
                let name = parse::entry(&body, "/T")
                    .map(|value| self.text(number, value.trim()))
                    .unwrap_or_default();
                Some(FormField {
                    number,
                    name,
                    signature: parse::entry(&body, "/FT").is_some_and(|kind| kind.trim() == "/Sig"),
                    signed: parse::entry(&body, "/V").is_some_and(|value| value.trim() != "null"),
                })
            })
            .collect()
    }

    /// A text string of object `number`, decrypted where the PDF is encrypted.
    fn text(&self, number: u32, value: &str) -> String {
        let bytes = if value.starts_with('(') && value.ends_with(')') && value.len() >= 2 {
            encrypt::literal(&value.as_bytes()[1..value.len() - 1])
        } else if value.starts_with('<') && value.ends_with('>') {
            encrypt::unhex(value.trim_matches(['<', '>']).as_bytes())
        } else {
            return String::new();
        };
        let bytes = match self.key {
            Some(key) if !self.compressed(number) => encrypt::decrypt(&key, &bytes).unwrap_or_default(),
            _ => bytes,
        };
        match bytes.strip_prefix(&[0xFE, 0xFF]) {
            Some(utf16) => String::from_utf16_lossy(
                &utf16
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|&pair| u16::from_be_bytes(pair))
                    .collect::<Vec<_>>(),
            ),
            None => bytes.iter().map(|&byte| char::from(byte)).collect(),
        }
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
                compressed(self.pdf, stream, offset, number, index, self.key.as_ref()).ok_or_else(missing)?
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
    let (dictionary, data) = stream(pdf, number, offset, None)?;
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

/// The dictionary and the decoded data of the stream object `number` at `offset`, decrypted
/// with `key`.
fn stream(pdf: &[u8], number: u32, offset: usize, key: Option<&[u8; 32]>) -> Option<(String, Vec<u8>)> {
    let object = parse::object(pdf, number, offset)?;
    let data = &pdf[object.data?];
    let decrypted;
    let data = match key {
        Some(key) => {
            decrypted = encrypt::decrypt(key, data)?;
            &decrypted[..]
        }
        None => data,
    };
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
fn compressed(
    pdf: &[u8],
    stream_number: u32,
    offset: usize,
    number: u32,
    index: usize,
    key: Option<&[u8; 32]>,
) -> Option<Vec<u8>> {
    let (dictionary, data) = stream(pdf, stream_number, offset, key)?;
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

//! `fagus worker`: renders documents for a caller over standard input and output, in the frames
//! of `docs/protocol.md`. One process serves many documents with its fonts and caches warm.

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// The version of the protocol.
pub(crate) const PROTOCOL: u32 = 1;

/// The largest frame accepted.
const MAX_FRAME: usize = 1 << 30;

const HELLO: u8 = b'H';
const RENDER: u8 = b'R';
const ASK: u8 = b'?';
const RESOURCE: u8 = b'r';
const PDF: u8 = b'P';
const ERROR: u8 = b'E';
const FONT: u8 = b'F';
const DONE: u8 = b'K';

/// A message: its type, its JSON header and its data blocks.
#[derive(Debug)]
pub(crate) struct Frame {
    pub kind: u8,
    pub header: Value,
    pub blobs: Vec<Vec<u8>>,
}

/// Both ends of the connection to the caller.
pub(crate) struct Channel<R, W> {
    input: R,
    output: W,
}

impl<R: Read, W: Write> Channel<R, W> {
    pub(crate) fn new(input: R, output: W) -> Self {
        Self { input, output }
    }

    /// Reads a frame; `None` when the input has ended.
    pub(crate) fn read(&mut self) -> io::Result<Option<Frame>> {
        let mut length = [0; 4];
        match self.input.read_exact(&mut length) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(error) => return Err(error),
        }
        let length = u32::from_le_bytes(length) as usize;
        if !(5..=MAX_FRAME).contains(&length) {
            return Err(invalid(&format!("a frame of {length} bytes")));
        }
        let mut body = vec![0; length];
        self.input.read_exact(&mut body)?;
        let kind = body[0];
        let header_length = u32::from_le_bytes([body[1], body[2], body[3], body[4]]) as usize;
        let header_end = 5usize
            .checked_add(header_length)
            .filter(|end| *end <= body.len())
            .ok_or_else(|| invalid("a header longer than its frame"))?;
        let header: Value = serde_json::from_slice(&body[5..header_end])
            .map_err(|error| invalid(&format!("a header that is not JSON: {error}")))?;
        let mut blobs = Vec::new();
        let mut offset = header_end;
        for length in header
            .get("blobs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let length = length
                .as_u64()
                .and_then(|length| usize::try_from(length).ok())
                .ok_or_else(|| invalid("a data block without a length"))?;
            let end = offset
                .checked_add(length)
                .filter(|end| *end <= body.len())
                .ok_or_else(|| invalid("data blocks longer than their frame"))?;
            blobs.push(body[offset..end].to_vec());
            offset = end;
        }
        Ok(Some(Frame { kind, header, blobs }))
    }

    /// Writes a frame; the header gets the lengths of the data blocks.
    pub(crate) fn write(&mut self, kind: u8, mut header: Value, blobs: &[&[u8]]) -> io::Result<()> {
        if !blobs.is_empty()
            && let Value::Object(map) = &mut header
        {
            map.insert(
                "blobs".to_owned(),
                blobs.iter().map(|blob| Value::from(blob.len())).collect(),
            );
        }
        let header = serde_json::to_vec(&header).map_err(io::Error::other)?;
        let length = 1 + 4 + header.len() + blobs.iter().map(|blob| blob.len()).sum::<usize>();
        let length = u32::try_from(length).map_err(|_| invalid("a frame larger than 4 GiB"))?;
        self.output.write_all(&length.to_le_bytes())?;
        self.output.write_all(&[kind])?;
        self.output.write_all(&(header.len() as u32).to_le_bytes())?;
        self.output.write_all(&header)?;
        for blob in blobs {
            self.output.write_all(blob)?;
        }
        self.output.flush()
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("protocol error: {message}"))
}

/// A render request (the header of `R`). Its flags are those of the protocol's JSON.
#[allow(clippy::struct_excessive_bools)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Request {
    id: u64,
    html: usize,
    /// Whether the markup is XHTML, parsed as XML.
    #[serde(default)]
    xhtml: bool,
    #[serde(default)]
    base: Option<PathBuf>,
    #[serde(default)]
    root: Option<PathBuf>,
    #[serde(default)]
    stylesheets: Vec<Stylesheet>,
    #[serde(default)]
    resources: Vec<Resource>,
    #[serde(default)]
    ask: bool,
    #[serde(default)]
    conformance: Option<String>,
    #[serde(default = "yes")]
    tagged: bool,
    #[serde(default)]
    accessible: bool,
    #[serde(default)]
    attachments: Vec<Attachment>,
    #[serde(default)]
    factur_x: Option<FacturX>,
    #[serde(default)]
    created: Option<String>,
}

/// The default of fields that are on unless the caller turns them off.
fn yes() -> bool {
    true
}

#[derive(Deserialize)]
struct Stylesheet {
    name: String,
    blob: usize,
}

#[derive(Deserialize)]
struct Resource {
    url: String,
    blob: usize,
}

#[derive(Deserialize)]
struct Attachment {
    name: String,
    mime: String,
    #[serde(default)]
    relationship: Option<String>,
    #[serde(default)]
    description: Option<String>,
    blob: usize,
}

#[derive(Deserialize)]
struct FacturX {
    blob: usize,
    #[serde(default)]
    profile: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Serialize)]
struct Diagnostic<'a> {
    severity: &'static str,
    code: &'a str,
    message: &'a str,
    source: &'a str,
    line: u32,
    column: u32,
}

fn diagnostics(list: &[fagus::Diagnostic]) -> Vec<Diagnostic<'_>> {
    list.iter()
        .map(|diagnostic| Diagnostic {
            severity: match diagnostic.severity {
                fagus::Severity::Info => "info",
                fagus::Severity::Warning => "warning",
                fagus::Severity::Error => "error",
            },
            code: diagnostic.code,
            message: &diagnostic.message,
            source: &diagnostic.source,
            line: diagnostic.line,
            column: diagnostic.column,
        })
        .collect()
}

/// Asks the caller for resources the document needs, while it renders.
struct Asker<R, W> {
    channel: Arc<Mutex<Channel<R, W>>>,
    id: u64,
}

impl<R: Read + Send, W: Write + Send> fagus::ResourceLoader for Asker<R, W> {
    fn load(&self, url: &str) -> Option<Vec<u8>> {
        let mut channel = self.channel.lock().ok()?;
        channel
            .write(ASK, json!({ "id": self.id, "url": url }), &[])
            .ok()?;
        let frame = channel.read().ok()??;
        if frame.kind != RESOURCE || frame.header.get("found") != Some(&Value::Bool(true)) {
            return None;
        }
        frame.blobs.into_iter().next()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> io::Result<std::sync::MutexGuard<'_, T>> {
    mutex.lock().map_err(|_| io::Error::other("poisoned lock"))
}

/// Serves render requests until the input ends.
pub(crate) fn serve<R, W>(mut engine: fagus::Engine, input: R, output: W) -> io::Result<()>
where
    R: Read + Send + 'static,
    W: Write + Send + 'static,
{
    let channel = Arc::new(Mutex::new(Channel::new(input, output)));

    lock(&channel)?.write(
        HELLO,
        json!({ "protocol": PROTOCOL, "version": env!("CARGO_PKG_VERSION") }),
        &[],
    )?;
    loop {
        let Some(frame) = lock(&channel)?.read()? else {
            return Ok(());
        };
        match frame.kind {
            RENDER => {
                let id = frame.header.get("id").and_then(Value::as_u64);
                let reply = render(&engine, &channel, frame);
                let mut channel = lock(&channel)?;
                match reply {
                    Ok(output) => channel.write(
                        PDF,
                        json!({
                            "id": id,
                            "pages": output.pages,
                            "diagnostics": diagnostics(&output.diagnostics),
                            "timings": {
                                "parse": output.timings.parse.as_secs_f64() * 1000.0,
                                "style": output.timings.style.as_secs_f64() * 1000.0,
                                "layout": output.timings.layout.as_secs_f64() * 1000.0,
                                "paint": output.timings.paint.as_secs_f64() * 1000.0,
                                "pdf": output.timings.pdf.as_secs_f64() * 1000.0,
                            },
                        }),
                        &[&output.pdf],
                    )?,
                    Err((message, found)) => {
                        channel.write(
                            ERROR,
                            json!({ "id": id, "message": message, "diagnostics": diagnostics(&found) }),
                            &[],
                        )?;
                    }
                }
            }
            FONT => {
                let reply = match frame.blobs.into_iter().next() {
                    Some(data) => engine.add_font_data(data).map_err(|error| error.to_string()),
                    None => Err("the font has no data".to_owned()),
                };
                let mut channel = lock(&channel)?;
                match reply {
                    Ok(faces) => channel.write(DONE, json!({ "faces": faces }), &[])?,
                    Err(message) => {
                        channel.write(ERROR, json!({ "message": message, "diagnostics": [] }), &[])?;
                    }
                }
            }
            other => {
                lock(&channel)?.write(
                    ERROR,
                    json!({ "message": format!("unknown message type `{}`", char::from(other)), "diagnostics": [] }),
                    &[],
                )?;
            }
        }
    }
}

/// Renders the document of a request; a failure comes with the diagnostics found before it.
fn render<R, W>(
    engine: &fagus::Engine,
    channel: &Arc<Mutex<Channel<R, W>>>,
    frame: Frame,
) -> Result<fagus::Output, (String, Vec<fagus::Diagnostic>)>
where
    R: Read + Send + 'static,
    W: Write + Send + 'static,
{
    let document = document(channel, frame).map_err(|message| (message, Vec::new()))?;
    engine
        .render_with_diagnostics(&document)
        .map_err(|failure| (failure.error.to_string(), failure.diagnostics))
}

/// The document of a render request.
fn document<R, W>(channel: &Arc<Mutex<Channel<R, W>>>, frame: Frame) -> Result<fagus::Document, String>
where
    R: Read + Send + 'static,
    W: Write + Send + 'static,
{
    let request: Request =
        serde_json::from_value(frame.header).map_err(|error| format!("invalid request: {error}"))?;
    let mut blobs: Vec<Option<Vec<u8>>> = frame.blobs.into_iter().map(Some).collect();
    let mut take = |index: usize| {
        blobs
            .get_mut(index)
            .and_then(Option::take)
            .ok_or_else(|| format!("data block {index} is missing"))
    };
    let html = String::from_utf8(take(request.html)?).map_err(|_| "the HTML is not UTF-8".to_owned())?;
    let mut document = if request.xhtml {
        fagus::Document::xhtml(html)
    } else {
        fagus::Document::html(html)
    };
    if let Some(base) = request.base {
        document = document.base_directory(base);
    }
    if let Some(root) = request.root {
        document = document.root(root);
    }
    for stylesheet in request.stylesheets {
        let css = String::from_utf8(take(stylesheet.blob)?)
            .map_err(|_| format!("the stylesheet `{}` is not UTF-8", stylesheet.name))?;
        document = document.stylesheet(stylesheet.name, css);
    }
    for resource in request.resources {
        document = document.resource(resource.url, take(resource.blob)?);
    }
    if request.ask {
        document = document.loader(Arc::new(Asker {
            channel: Arc::clone(channel),
            id: request.id,
        }));
    }
    if let Some(level) = request.conformance {
        let level =
            fagus::Conformance::from_level(&level).ok_or_else(|| format!("unknown PDF/A level `{level}`"))?;
        document = document.conformance(level);
    }
    document = document.tagged(request.tagged);
    if request.accessible {
        document = document.accessible();
    }
    for attachment in request.attachments {
        // PDF/A-3 and PDF/UA need a description: the file name, as the command line gives.
        let description = attachment
            .description
            .filter(|description| !description.is_empty())
            .unwrap_or_else(|| attachment.name.clone());
        document = document.attach(fagus::Attachment {
            data: Arc::new(take(attachment.blob)?),
            name: attachment.name,
            mime_type: attachment.mime,
            description: Some(description),
            relationship: match attachment.relationship.as_deref().unwrap_or("unspecified") {
                "source" => fagus::Relationship::Source,
                "data" => fagus::Relationship::Data,
                "alternative" => fagus::Relationship::Alternative,
                "supplement" => fagus::Relationship::Supplement,
                "unspecified" => fagus::Relationship::Unspecified,
                other => return Err(format!("unknown relationship `{other}`")),
            },
        });
    }
    if let Some(invoice) = request.factur_x {
        let xml = take(invoice.blob)?;
        document = match invoice.profile.as_deref() {
            None => document.factur_x(xml),
            Some(profile) => document.factur_x_profile(
                xml,
                crate::profile(profile).ok_or_else(|| format!("unknown profile `{profile}`"))?,
            ),
        };
        if let Some(version) = invoice.version {
            document = document.factur_x_version(version);
        }
        if let Some(description) = invoice.description {
            document = document.factur_x_description(description);
        }
    }
    if let Some(created) = request.created {
        document = document.created(date(&created).ok_or_else(|| format!("invalid date `{created}`"))?);
    }
    Ok(document)
}

/// A date of the form `2026-11-02T12:00:00Z`, or with an offset (`+01:00`), in UTC.
pub(crate) fn date(text: &str) -> Option<fagus::Date> {
    crate::date::parse(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes frames as the caller would.
    fn frame(kind: u8, header: &Value, blobs: &[&[u8]]) -> Vec<u8> {
        let mut channel = Channel::new(io::empty(), Vec::new());
        channel.write(kind, header.clone(), blobs).expect("written");
        channel.output
    }

    /// Splits the worker's output into frames.
    fn frames(output: &[u8]) -> Vec<Frame> {
        let mut channel = Channel::new(output, io::sink());
        std::iter::from_fn(|| channel.read().expect("a frame")).collect()
    }

    fn engine() -> fagus::Engine {
        let mut engine = fagus::Engine::new();
        engine
            .add_font_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fonts"))
            .expect("fonts");
        engine
    }

    /// A writer the test can read after the worker has finished.
    #[derive(Clone, Default)]
    struct Shared(Arc<Mutex<Vec<u8>>>);

    impl Write for Shared {
        fn write(&mut self, data: &[u8]) -> io::Result<usize> {
            self.0.lock().expect("lock").extend_from_slice(data);
            Ok(data.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn renders_and_asks_for_resources() {
        let html = br"<style>@font-face { font-family: Doc; src: url(font.ttf); } body { font-family: Doc }</style><p>Hallo</p>";
        let font = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fonts/Inter-Regular.ttf"
        ))
        .expect("font");
        // The request, then the answer to the question the worker will ask.
        let mut input = frame(
            RENDER,
            &json!({ "id": 7, "html": 0, "ask": true, "created": "2026-11-02T12:00:00Z" }),
            &[html],
        );
        input.extend(frame(RESOURCE, &json!({ "found": true }), &[&font]));
        input.extend(frame(FONT, &json!({}), &[b"no font"]));
        let output = Shared::default();
        serve(engine(), io::Cursor::new(input), output.clone()).expect("served");
        let output = output.0.lock().expect("lock").clone();
        let frames = frames(&output);
        let kinds: String = frames.iter().map(|frame| char::from(frame.kind)).collect();
        assert_eq!(kinds, "H?PE");
        assert_eq!(frames[0].header["protocol"], PROTOCOL);
        assert_eq!(frames[1].header, json!({ "id": 7, "url": "font.ttf" }));
        assert_eq!(frames[2].header["id"], 7);
        assert_eq!(frames[2].header["pages"], 1);
        assert!(frames[2].blobs[0].starts_with(b"%PDF-"));
        assert!(
            frames[3].header["message"]
                .as_str()
                .is_some_and(|message| !message.is_empty())
        );
    }

    #[test]
    fn reports_bad_requests() {
        let input = frame(RENDER, &json!({ "id": 1, "html": 3 }), &[b"<p>x</p>"]);
        let output = Shared::default();
        serve(engine(), io::Cursor::new(input), output.clone()).expect("served");
        let output = output.0.lock().expect("lock").clone();
        let frames = frames(&output);
        assert_eq!(frames[1].kind, ERROR);
        assert_eq!(frames[1].header["message"], "data block 3 is missing");
    }

    #[test]
    fn describes_attachments_by_their_names() {
        // PDF/A-3 and PDF/UA refuse attachments without a description; the name stands in.
        let attachment = |description: Value| json!({ "name": "daten.csv", "mime": "text/csv", "description": description, "blob": 1 });
        let mut input = Vec::new();
        for (id, description) in [(1, Value::Null), (2, json!("")), (3, json!("Die Daten"))] {
            input.extend(frame(
                RENDER,
                &json!({
                    "id": id,
                    "html": 0,
                    "conformance": "3b",
                    "attachments": [attachment(description)],
                    "created": "2026-11-02T12:00:00Z",
                }),
                &[b"<p>Anhang</p>", b"a;b"],
            ));
        }
        let output = Shared::default();
        serve(engine(), io::Cursor::new(input), output.clone()).expect("served");
        let output = output.0.lock().expect("lock").clone();
        let frames = frames(&output);
        let kinds: String = frames.iter().map(|frame| char::from(frame.kind)).collect();
        assert_eq!(kinds, "HPPP", "{:?}", frames.last().map(|frame| &frame.header));
    }

    #[test]
    fn sets_the_version_and_description_of_invoices() {
        let xml: &[u8] = b"<rsm:CrossIndustryInvoice><ram:GuidelineSpecifiedDocumentContextParameter>\
            <ram:ID>urn:cen.eu:en16931:2017#compliant#urn:xeinkauf.de:kosit:xrechnung_3.0</ram:ID>\
            </ram:GuidelineSpecifiedDocumentContextParameter></rsm:CrossIndustryInvoice>";
        let mut input = Vec::new();
        for (id, version, description) in [
            (1, json!("3.0"), json!("XRechnung Invoice")),
            (2, Value::Null, Value::Null),
            (3, json!(""), Value::Null),
        ] {
            input.extend(frame(
                RENDER,
                &json!({
                    "id": id,
                    "html": 0,
                    "facturX": { "blob": 1, "version": version, "description": description },
                    "created": "2026-11-02T12:00:00Z",
                }),
                &[b"<p>Rechnung</p>", xml],
            ));
        }
        let output = Shared::default();
        serve(engine(), io::Cursor::new(input), output.clone()).expect("served");
        let output = output.0.lock().expect("lock").clone();
        let frames = frames(&output);
        let kinds: String = frames.iter().map(|frame| char::from(frame.kind)).collect();
        assert_eq!(kinds, "HPPE");
        let has = |pdf: &[u8], text: &str| pdf.windows(text.len()).any(|window| window == text.as_bytes());
        assert!(has(&frames[1].blobs[0], "<fx:Version>3.0</fx:Version>"));
        assert!(has(&frames[2].blobs[0], "<fx:Version>1.0</fx:Version>"));
        assert_eq!(
            frames[3].header["message"],
            "the document does not meet Factur-X: the invoice version is empty"
        );
    }

    #[test]
    fn sends_the_diagnostics_of_failures() {
        // PDF/UA refuses a document without a title; the CSS warning found before comes along.
        let input = frame(
            RENDER,
            &json!({ "id": 4, "html": 0, "accessible": true }),
            &[b"<style>p { colr: red }</style><p>Brief</p>"],
        );
        let output = Shared::default();
        serve(engine(), io::Cursor::new(input), output.clone()).expect("served");
        let output = output.0.lock().expect("lock").clone();
        let frames = frames(&output);
        assert_eq!(frames[1].kind, ERROR);
        assert_eq!(frames[1].header["id"], 4);
        assert_eq!(frames[1].header["diagnostics"][0]["code"], "css.unknown-property");
        assert_eq!(frames[1].header["diagnostics"][0]["severity"], "warning");
    }

    #[test]
    fn parses_xhtml_as_xml() {
        // An empty-element tag closes the `div` in XHTML only; in HTML it holds the paragraph.
        let html: &[u8] = br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><div style="break-after: page"/><p>x</p></body></html>"#;
        let mut input = frame(RENDER, &json!({ "id": 1, "html": 0 }), &[html]);
        input.extend(frame(
            RENDER,
            &json!({ "id": 2, "html": 0, "xhtml": true }),
            &[html],
        ));
        let output = Shared::default();
        serve(engine(), io::Cursor::new(input), output.clone()).expect("served");
        let output = output.0.lock().expect("lock").clone();
        let frames = frames(&output);
        assert_eq!(frames[1].header["pages"], 1);
        assert_eq!(frames[2].header["pages"], 2);
    }

    #[test]
    fn reads_dates() {
        let date = date("2026-11-02T12:30:05Z").expect("date");
        assert_eq!(
            (
                date.year,
                date.month,
                date.day,
                date.hour,
                date.minute,
                date.second
            ),
            (2026, 11, 2, 12, 30, 5)
        );
        assert!(super::date("2026-13-02T12:30:05Z").is_none());
        assert!(super::date("gestern").is_none());
    }
}

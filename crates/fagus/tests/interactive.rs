//! Interactive forms (`Document::forms`) and layers (`-fagus-layer`) in the PDF.

use fagus::{Conformance, Date, Document, Engine, Output};

const CREATED: Date = Date {
    year: 2026,
    month: 11,
    day: 19,
    hour: 12,
    minute: 0,
    second: 0,
};

fn engine() -> Engine {
    let mut engine = Engine::new();
    engine
        .add_font_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fonts"))
        .expect("fonts");
    engine
}

fn render(document: Document) -> Output {
    engine().render(&document.created(CREATED)).expect("rendered")
}

/// The bytes of a PDF followed by the decoded data of each of its compressed streams.
fn decoded(pdf: &[u8]) -> Vec<u8> {
    let find =
        |haystack: &[u8], needle: &[u8]| haystack.windows(needle.len()).position(|window| window == needle);
    let mut all = pdf.to_vec();
    let mut rest = pdf;
    while let Some(start) = find(rest, b"/Length ") {
        let digits: Vec<u8> = rest[start + 8..]
            .iter()
            .copied()
            .take_while(u8::is_ascii_digit)
            .collect();
        let length: Option<usize> = String::from_utf8(digits)
            .ok()
            .and_then(|digits| digits.parse().ok());
        let data = find(&rest[start..], b"stream\n").map(|at| start + at + 7);
        let (Some(length), Some(data)) = (length, data) else {
            rest = &rest[start + 8..];
            continue;
        };
        let Some(stream) = rest.get(data..data + length) else {
            break;
        };
        let mut decoder = flate2::read::ZlibDecoder::new(stream);
        let mut out = Vec::new();
        if std::io::Read::read_to_end(&mut decoder, &mut out).is_ok() {
            all.extend_from_slice(&out);
        }
        rest = &rest[data + length..];
    }
    all
}

fn count(pdf: &[u8], text: &str) -> usize {
    decoded(pdf)
        .windows(text.len())
        .filter(|window| *window == text.as_bytes())
        .count()
}

fn contains(pdf: &[u8], text: &str) -> bool {
    count(pdf, text) > 0
}

fn codes(output: &Output) -> Vec<&'static str> {
    output
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

const FORM: &str = "<!DOCTYPE html><html lang=en><title>Order</title>\
    <p><label>Name <input name=customer.name value='Ada Lovelace' required maxlength=40></label>\
    <p><label>Email <input type=email name=customer.email placeholder=you@example.com></label>\
    <p><label><input type=checkbox name=terms checked> I agree</label>\
    <p><label><input type=radio name=plan value=basic> Basic</label>\
       <label><input type=radio name=plan value=pro checked> Pro</label>\
    <p><select name=country><option value=de>Germany<option value=fr selected>France</select>\
    <p><textarea name=notes readonly>Two\nlines</textarea>\
    <p><input type=submit value=Send><span inert><input name=sample value=Sample></span>";

#[test]
fn prints_controls_as_they_look_without_forms() {
    let static_pdf = render(Document::html(FORM)).pdf;
    assert!(!contains(&static_pdf, "/AcroForm"));
    assert_eq!(render(Document::html(FORM).forms(false)).pdf, static_pdf);
    // A document without controls gets the same bytes with forms.
    let text = "<!DOCTYPE html><p>No controls";
    assert_eq!(
        render(Document::html(text).forms(true)).pdf,
        render(Document::html(text)).pdf
    );
}

#[test]
fn makes_fields_of_controls() {
    let output = render(Document::html(FORM).forms(true));
    let pdf = &output.pdf;
    assert_eq!(output.pages, 1);
    assert!(contains(pdf, "/AcroForm"));
    assert!(!contains(pdf, "/NeedAppearances"));
    assert!(!contains(pdf, "fagus-field:"));
    // The appearances leave the page tree: one page.
    assert!(contains(pdf, "/Count 1"));
    // Names with periods make a tree of fields.
    for name in [
        "(customer)",
        "(name)",
        "(email)",
        "(terms)",
        "(plan)",
        "(country)",
        "(notes)",
    ] {
        assert!(contains(pdf, &format!("/T {name}")), "{name}");
    }
    assert!(!contains(pdf, "(sample)"));
    assert!(!contains(pdf, "(Send)") || !contains(pdf, "/T (Send)"));
    assert!(contains(pdf, "/V (Ada Lovelace)"));
    assert!(contains(pdf, "/MaxLen 40"));
    assert!(contains(pdf, "/FT/Btn/V /on"));
    assert!(contains(pdf, "/FT/Btn/V /pro"));
    assert!(contains(pdf, "/Opt[[(de)(Germany)][(fr)(France)]]/V (fr)"));
    assert!(contains(pdf, "/V (Two\\nlines)"));
    // Seven widgets (two radio buttons in one field), each with its appearance.
    assert_eq!(count(pdf, "/Subtype/Widget"), 7);
    assert_eq!(count(pdf, "/AP<<"), 7);
    assert_eq!(count(pdf, "/AS /Off"), 1);
    assert!(contains(pdf, "/AS /pro"));
    assert!(contains(pdf, "/Subtype /Form"));
    // Tagged: each widget in a form field of the structure.
    assert_eq!(count(pdf, "/S/Form"), 7);
    assert!(contains(pdf, "/Type/OBJR"));
}

#[test]
fn writes_the_same_form_every_time() {
    let first = render(Document::html(FORM).forms(true)).pdf;
    let second = render(Document::html(FORM).forms(true)).pdf;
    assert_eq!(first, second);
}

#[test]
fn makes_fields_for_pdf_a_and_pdf_ua() {
    for conformance in [Conformance::PdfA2b, Conformance::PdfA3a] {
        let output = render(Document::html(FORM).forms(true).conformance(conformance));
        assert!(contains(&output.pdf, "/AcroForm"));
        assert!(codes(&output).is_empty(), "{:?}", output.diagnostics);
    }
    let output = render(Document::html(FORM).forms(true).accessible());
    assert!(contains(&output.pdf, "/TU (Name)"));
}

const LAYERS: &str = "<!DOCTYPE html><html lang=en><title>Layers</title>\
    <style>.note { -fagus-layer: 'Notes' screen } .copy { -fagus-layer: \"Copy\" print }\
    .more { -fagus-layer: 'Details' }</style>\
    <h1>Invoice</h1><p class=note>Pay online.<p class=copy>Copy<p class=more>More <b class=note>note</b>";

#[test]
fn puts_content_on_layers() {
    let output = render(Document::html(LAYERS));
    let pdf = &output.pdf;
    assert!(codes(&output).is_empty(), "{:?}", output.diagnostics);
    assert!(contains(pdf, "/OCProperties"));
    for name in ["(Notes)", "(Copy)", "(Details)"] {
        assert_eq!(count(pdf, &format!("/Type/OCG/Name {name}")), 1, "{name}");
    }
    assert!(contains(pdf, "/Print<</PrintState/OFF>>"));
    assert!(contains(pdf, "/View<</ViewState/OFF>>"));
    assert!(contains(pdf, "/AS["));
    // Four sequences: the second note is on its layer inside the details.
    assert_eq!(count(pdf, "/OC /FagusLayer"), 4);
    assert_eq!(count(pdf, "/OC /FagusLayer0 BDC"), 2);
    // The marks are gone.
    assert!(!contains(pdf, "-50000"));
    // The content stays in the structure.
    assert!(contains(pdf, "/MCID"));
}

#[test]
fn keeps_layers_to_what_standards_allow() {
    let output = render(Document::html(LAYERS).conformance(Conformance::PdfA2b));
    assert_eq!(codes(&output), ["pdf.layer-shown", "pdf.layer-shown"]);
    assert!(contains(&output.pdf, "/OCProperties"));
    assert!(!contains(&output.pdf, "/AS["));
    assert!(!contains(&output.pdf, "/Usage"));

    let output = render(Document::html(LAYERS).accessible());
    assert_eq!(codes(&output), ["pdf.layer-shown", "pdf.layer-shown"]);
    assert!(!contains(&output.pdf, "/AS["));

    let output = render(Document::html(LAYERS).conformance(Conformance::PdfA1b));
    assert_eq!(codes(&output), ["pdf.layers-unsupported"]);
    assert!(!contains(&output.pdf, "/OCProperties"));
    assert!(!contains(&output.pdf, "/OC /"));
}

#[test]
fn puts_fields_on_layers() {
    let output = render(
        Document::html("<!DOCTYPE html><div style=\"-fagus-layer: 'Form'\"><input name=a value=x></div>")
            .forms(true),
    );
    let pdf = &output.pdf;
    assert!(contains(pdf, "/Type/OCG/Name (Form)"));
    // The widget is on the layer of its control.
    let decoded = decoded(pdf);
    let widget = decoded
        .windows(15)
        .position(|window| window == b"/Subtype/Widget")
        .expect("a widget");
    let rest = String::from_utf8_lossy(&decoded[widget..(widget + 200).min(decoded.len())]).into_owned();
    assert!(rest.contains("/OC "), "{rest}");
}

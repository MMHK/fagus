//! Features that change the PDF after krilla wrote it, together: fill-in forms and layers (an
//! update before the file is compacted), encryption (while it is compacted) and signatures (an
//! incremental update after it). qpdf and OpenSSL check the files where they are installed.

use std::path::{Path, PathBuf};
use std::process::Command;

use fagus::{Date, Document, Encryption, Engine, Signature};

const CREATED: Date = Date::new(2026, 11, 19, 12, 0, 0);

const FORM: &str = "<!DOCTYPE html><html lang=en><title>Order</title>\
    <p><label>Name <input name=customer value='Ada Lovelace'></label>\
    <p><label><input type=checkbox name=terms checked> I agree</label>\
    <p><select name=country><option value=de>Germany<option value=fr selected>France</select>";

const LAYERS: &str = "<!DOCTYPE html><html lang=en><title>Invoice</title>\
    <p style='-fagus-layer: \"Notes\" screen'>Pay online with the link below.\
    <p style='-fagus-layer: \"Copy\" print'>COPY";

fn engine() -> Engine {
    let mut engine = Engine::new();
    engine
        .add_font_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fonts"))
        .expect("fonts");
    engine
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/signing")
        .join(name)
}

fn signature() -> Signature {
    let p12 = std::fs::read(fixture("rsa.p12")).expect("fixture");
    Signature::pkcs12(&p12, "fagus-test-only").expect("PKCS#12 file")
}

/// Whether a program runs here; the checks with it are skipped otherwise.
fn installed(program: &str) -> bool {
    let found = Command::new(program).arg("--version").output().is_ok();
    if !found {
        eprintln!("{program} is not installed; the files are not checked with it");
    }
    found
}

/// Writes `pdf` to a file of its own, named for the test.
fn file(name: &str, pdf: &[u8]) -> PathBuf {
    let path = std::env::temp_dir().join(format!("fagus-interplay-{name}-{}.pdf", std::process::id()));
    std::fs::write(&path, pdf).expect("written");
    path
}

/// qpdf with the arguments and the file: its exit code and output.
fn qpdf(arguments: &[&str], file: &Path) -> (i32, String) {
    let output = Command::new("qpdf")
        .args(arguments)
        .arg(file)
        .output()
        .expect("qpdf runs");
    let text =
        String::from_utf8_lossy(&output.stdout).into_owned() + &String::from_utf8_lossy(&output.stderr);
    (output.status.code().unwrap_or(-1), text)
}

/// The fields of the form as qpdf reads them: (name, field type, value) of each.
fn fields(file: &Path, password: &str) -> Vec<(String, String, String)> {
    let password = format!("--password={password}");
    let (status, json) = qpdf(&["--json", "--json-key=acroform", &password], file);
    assert_eq!(status, 0, "{json}");
    let json: serde_json::Value = serde_json::from_str(&json).expect("JSON");
    json["acroform"]["fields"]
        .as_array()
        .expect("fields")
        .iter()
        .map(|field| {
            // qpdf writes strings with `u:` before them.
            let text = |key: &str| {
                field[key].as_str().map_or_else(
                    || field[key].to_string(),
                    |text| text.strip_prefix("u:").unwrap_or(text).to_owned(),
                )
            };
            (text("fullname"), text("fieldtype"), text("value"))
        })
        .collect()
}

#[test]
fn encrypts_fill_in_forms() {
    let pdf = engine()
        .render(
            &Document::html(FORM)
                .created(CREATED)
                .forms(true)
                .encrypt(Encryption::new().user_password("benutzer")),
        )
        .expect("rendered")
        .pdf;
    let has = |text: &str| pdf.windows(text.len()).any(|window| window == text.as_bytes());
    assert!(has("/Encrypt"));
    assert!(!has("Ada Lovelace"), "the values are encrypted");
    if !installed("qpdf") {
        return;
    }
    let file = file("forms-encrypted", &pdf);
    let (status, output) = qpdf(&["--check", "--password=benutzer"], &file);
    assert_eq!(status, 0, "{output}");
    let fields = fields(&file, "benutzer");
    assert!(
        fields.contains(&("customer".to_owned(), "/Tx".to_owned(), "Ada Lovelace".to_owned())),
        "{fields:?}"
    );
    assert!(
        fields
            .iter()
            .any(|(name, kind, value)| name == "terms" && kind == "/Btn" && value != "/Off")
    );
    assert!(
        fields
            .iter()
            .any(|(name, kind, value)| name == "country" && kind == "/Ch" && value == "fr")
    );
    assert_ne!(qpdf(&["--check", "--password=falsch"], &file).0, 0);
    std::fs::remove_file(file).expect("removed");
}

#[test]
fn signs_fill_in_forms_in_the_same_form() {
    let pdf = engine()
        .render(
            &Document::html(FORM)
                .created(CREATED)
                .forms(true)
                .sign(signature().reason("Bestellung")),
        )
        .expect("rendered")
        .pdf;
    // The signature is an update after the file with the form.
    let unsigned = engine()
        .render(&Document::html(FORM).created(CREATED).forms(true))
        .expect("rendered")
        .pdf;
    assert!(pdf.starts_with(&unsigned));
    let file = file("forms-signed", &pdf);
    if installed("qpdf") {
        let (status, output) = qpdf(&["--check"], &file);
        assert_eq!(status, 0, "{output}");
        let fields = fields(&file, "");
        let kinds: Vec<&str> = fields.iter().map(|(_, kind, _)| kind.as_str()).collect();
        assert!(kinds.contains(&"/Sig"), "{fields:?}");
        assert!(
            fields.contains(&("customer".to_owned(), "/Tx".to_owned(), "Ada Lovelace".to_owned())),
            "{fields:?}"
        );
        assert!(fields.iter().any(|(name, _, _)| name == "terms"));
        assert!(fields.iter().any(|(name, _, _)| name == "country"));
        // The signature field took a name no field of the form has.
        let mut names: Vec<&str> = fields.iter().map(|(name, _, _)| name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), fields.len(), "{fields:?}");
    }
    if installed("openssl") && installed("python3") {
        let verified = Command::new("python3")
            .arg(fixture("verify.py"))
            .arg(&file)
            .output()
            .expect("verify.py runs");
        assert!(
            verified.status.success(),
            "{}",
            String::from_utf8_lossy(&verified.stdout)
        );
    }
    std::fs::remove_file(file).expect("removed");
}

#[test]
fn encrypts_layers() {
    let pdf = engine()
        .render(
            &Document::html(LAYERS)
                .created(CREATED)
                .encrypt(Encryption::new().user_password("benutzer")),
        )
        .expect("rendered")
        .pdf;
    let has = |text: &str| pdf.windows(text.len()).any(|window| window == text.as_bytes());
    assert!(has("/OCProperties"), "the layers stay in the catalog");
    assert!(!has("(Notes)"), "their names are encrypted");
    if !installed("qpdf") {
        return;
    }
    let file = file("layers-encrypted", &pdf);
    let (status, output) = qpdf(&["--check", "--password=benutzer"], &file);
    assert_eq!(status, 0, "{output}");
    let (status, json) = qpdf(
        &[
            "--json",
            "--json-key=qpdf",
            "--password=benutzer",
            "--decode-level=all",
        ],
        &file,
    );
    assert_eq!(status, 0);
    assert!(json.contains("\"/Name\": \"u:Notes\""), "the names decrypt");
    assert!(json.contains("\"/Name\": \"u:Copy\""));
    std::fs::remove_file(file).expect("removed");
}

#[test]
fn does_not_sign_an_encrypted_form() {
    let error = engine()
        .render(
            &Document::html(FORM)
                .forms(true)
                .encrypt(Encryption::new().user_password("benutzer"))
                .sign(signature()),
        )
        .expect_err("refused");
    assert!(
        error
            .to_string()
            .contains("an encrypted PDF cannot be signed yet")
    );
}

//! Password protection, as readers see it: qpdf (where it is installed) opens the files with
//! their passwords, refuses a wrong one and shows the permissions; standards that forbid
//! encryption refuse it, PDF/UA allows it.

use std::path::{Path, PathBuf};
use std::process::Command;

use fagus::{Conformance, Date, Document, Encryption, Engine, Permission, Signature};

const HTML: &str = "<!doctype html><html lang=de><title>Gehaltsabrechnung</title>\
    <h1>Gehalt</h1><p>Text mit <a href='https://example.com/'>Link</a></p>";

fn engine() -> Engine {
    let mut engine = Engine::new();
    engine
        .add_font_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fonts"))
        .expect("fonts");
    engine
}

fn contains(pdf: &[u8], text: &str) -> bool {
    pdf.windows(text.len()).any(|window| window == text.as_bytes())
}

/// qpdf, unless it is not installed.
fn qpdf() -> Option<PathBuf> {
    let found = Command::new("qpdf").arg("--version").output().is_ok();
    if !found {
        eprintln!("qpdf is not installed; the files are not checked with it");
    }
    found.then(|| PathBuf::from("qpdf"))
}

/// qpdf with a password: its exit code and output.
fn run(qpdf: &Path, file: &Path, password: &str, action: &str) -> (i32, String) {
    let output = Command::new(qpdf)
        .arg(format!("--password={password}"))
        .arg(action)
        .arg(file)
        .output()
        .expect("qpdf runs");
    let text =
        String::from_utf8_lossy(&output.stdout).into_owned() + &String::from_utf8_lossy(&output.stderr);
    (output.status.code().unwrap_or(-1), text)
}

#[test]
fn encrypts_what_readers_open_with_the_password() {
    let engine = engine();
    let document = Document::html(HTML).created(Date::from_unix(1_800_000_000));
    let encrypted = |encryption: Encryption| {
        engine
            .render(&document.clone().encrypt(encryption))
            .expect("rendered")
            .pdf
    };

    let both = Encryption::new()
        .user_password("benutzer")
        .owner_password("eigentümer")
        .permissions([Permission::Print, Permission::Copy]);
    let pdf = encrypted(both.clone());
    assert!(pdf.starts_with(b"%PDF-1.7"));
    assert!(contains(&pdf, "/Filter/Standard/V 5/R 6/"));
    assert!(!contains(&pdf, "Gehalt"), "the metadata is encrypted too");
    // Random keys: another file each time, unless a seed fixes them.
    assert_ne!(encrypted(both.clone()), pdf);
    let seeded = both.clone().seed([9; 32]);
    assert_eq!(encrypted(seeded.clone()), encrypted(seeded));

    let only_owner = encrypted(Encryption::new().owner_password("eigentümer").permissions([]));

    let Some(qpdf) = qpdf() else { return };
    let directory = std::env::temp_dir().join(format!("fagus-encryption-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("directory");
    let file = directory.join("both.pdf");
    std::fs::write(&file, &pdf).expect("written");
    let (code, output) = run(&qpdf, &file, "benutzer", "--check");
    assert_eq!(code, 0, "{output}");
    let (code, output) = run(&qpdf, &file, "benutzer", "--show-encryption");
    assert_eq!(code, 0, "{output}");
    for line in [
        "R = 6",
        "Supplied password is user password",
        "print high resolution: allowed",
        "extract for any purpose: allowed",
        "extract for accessibility: allowed",
        "modify other: not allowed",
        "modify annotations: not allowed",
        "modify document assembly: not allowed",
        "stream encryption method: AESv3",
        "string encryption method: AESv3",
    ] {
        assert!(output.contains(line), "{line}: {output}");
    }
    let (_, output) = run(&qpdf, &file, "eigentümer", "--show-encryption");
    assert!(output.contains("Supplied password is owner password"), "{output}");
    let (code, output) = run(&qpdf, &file, "falsch", "--check");
    assert_eq!(code, 2, "{output}");
    assert!(output.contains("invalid password"), "{output}");

    // Decrypted, the document is there again.
    let decrypted = directory.join("decrypted.pdf");
    let status = Command::new(&qpdf)
        .arg("--password=benutzer")
        .arg("--decrypt")
        .arg("--qdf")
        .arg(&file)
        .arg(&decrypted)
        .status()
        .expect("qpdf runs");
    assert!(status.success());
    let plain = std::fs::read(&decrypted).expect("decrypted");
    assert!(contains(&plain, "Gehaltsabrechnung") && contains(&plain, "https://example.com/"));

    // Only an owner password: opens without one, with nothing allowed.
    let file = directory.join("owner.pdf");
    std::fs::write(&file, &only_owner).expect("written");
    let (code, output) = run(&qpdf, &file, "", "--show-encryption");
    assert_eq!(code, 0, "{output}");
    for line in [
        "P = -3392",
        "Supplied password is user password",
        "print low resolution: not allowed",
        "extract for any purpose: not allowed",
        "extract for accessibility: allowed",
    ] {
        assert!(output.contains(line), "{line}: {output}");
    }
    assert_eq!(run(&qpdf, &file, "", "--check").0, 0);
    std::fs::remove_dir_all(&directory).expect("removed");
}

#[test]
fn refuses_to_encrypt_what_must_stay_open() {
    let engine = engine();
    let encryption = Encryption::new().user_password("benutzer");
    let message = |document: Document| {
        engine
            .render(&document.encrypt(encryption.clone()))
            .expect_err("refused")
            .to_string()
    };
    assert_eq!(
        message(Document::html(HTML).conformance(Conformance::PdfA2b)),
        "the document does not meet PDF/A-2b: the PDF cannot be encrypted (PDF/A forbids encryption)"
    );
    assert!(
        message(Document::html(HTML).factur_x(b"<x/>".to_vec()))
            .starts_with("the document does not meet Factur-X: the invoice cannot be encrypted")
    );
    assert_eq!(
        engine
            .render(&Document::html(HTML).encrypt(Encryption::new().owner_password("")))
            .expect_err("no password")
            .to_string(),
        "the PDF could not be written: encryption needs a user password or an owner password"
    );
}

/// A directory of its own for the files of a test.
fn directory(test: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("fagus-{test}-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("directory");
    directory
}

/// A file in a directory of [`directory`].
fn temporary(directory: &Path, name: &str, pdf: &[u8]) -> PathBuf {
    let file = directory.join(name);
    std::fs::write(&file, pdf).expect("written");
    file
}

#[test]
fn encrypts_accessible_pdfs_for_assistive_technology() {
    let engine = engine();
    // No permissions at all: the extraction for accessibility stays allowed, as PDF/UA asks.
    let output = engine
        .render(
            &Document::html(HTML)
                .accessible()
                .encrypt(Encryption::new().user_password("benutzer").permissions([])),
        )
        .expect("rendered");
    assert!(contains(&output.pdf, "/P -3392/"), "bit 10 set, nothing else");
    let Some(qpdf) = qpdf() else { return };
    let directory = directory("encryption-accessible");
    let file = temporary(&directory, "accessible.pdf", &output.pdf);
    let (code, output) = run(&qpdf, &file, "benutzer", "--show-encryption");
    assert_eq!(code, 0, "{output}");
    assert!(output.contains("extract for accessibility: allowed"), "{output}");
    assert!(
        output.contains("extract for any purpose: not allowed"),
        "{output}"
    );
    // Decrypted, it is the tagged PDF/UA file, with its metadata.
    let decrypted = file.with_file_name("accessible-decrypted.pdf");
    let status = Command::new(&qpdf)
        .args(["--password=benutzer", "--decrypt", "--qdf"])
        .arg(&file)
        .arg(&decrypted)
        .status()
        .expect("qpdf runs");
    assert!(status.success());
    let plain = std::fs::read(&decrypted).expect("decrypted");
    assert!(contains(&plain, "pdfuaid:part") && contains(&plain, "/StructTreeRoot"));
    std::fs::remove_dir_all(&directory).expect("removed");
}

#[test]
fn prepares_passwords_beyond_ascii_with_saslprep() {
    let engine = engine();
    let Some(qpdf) = qpdf() else { return };
    let directory = directory("encryption-saslprep");
    // Typed decomposed, with a no-break space; in Arabic; with an emoji. qpdf takes the UTF-8
    // of the password as it is given, so it opens the files with the prepared passwords.
    for (index, (given, prepared)) in [
        ("Pa\u{308}sswo\u{308}rd\u{a0}1", "Pässwörd 1"),
        ("كلمة السر", "كلمة السر"),
        ("geheim 🙂", "geheim 🙂"),
        ("ﬁle\u{ad}name", "filename"),
    ]
    .into_iter()
    .enumerate()
    {
        let pdf = engine
            .render(&Document::html(HTML).encrypt(Encryption::new().user_password(given)))
            .expect("rendered")
            .pdf;
        let file = temporary(&directory, &format!("password-{index}.pdf"), &pdf);
        let (code, output) = run(&qpdf, &file, prepared, "--check");
        assert_eq!(code, 0, "{prepared}: {output}");
        assert_eq!(run(&qpdf, &file, "falsch", "--check").0, 2);
    }
    std::fs::remove_dir_all(&directory).expect("removed");
    // A password that SASLprep refuses is refused, without showing it.
    let error = engine
        .render(&Document::html(HTML).encrypt(Encryption::new().user_password("geheim\u{1}")))
        .expect_err("refused")
        .to_string();
    assert!(
        error.starts_with("the PDF could not be written: the user password cannot be used: it contains"),
        "{error}"
    );
    assert!(!error.contains("geheim"));
}

#[test]
fn refuses_to_sign_an_encrypted_pdf_for_now() {
    let p12 = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/signing/rsa.p12"
    ))
    .expect("fixture");
    let signature = Signature::pkcs12(&p12, "fagus-test-only").expect("PKCS#12 file");
    let error = engine()
        .render(
            &Document::html(HTML)
                .encrypt(Encryption::new().user_password("benutzer"))
                .sign(signature),
        )
        .expect_err("refused");
    assert_eq!(
        error.to_string(),
        "the PDF could not be signed: an encrypted PDF cannot be signed yet; sign it or encrypt it"
    );
}

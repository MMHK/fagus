//! Digital signatures of the PDF, as `PAdES` baseline signatures (ETSI EN 319 142-1): B-B, and
//! B-T with a timestamp. The signature is an invisible signature field, added to the finished
//! PDF as an incremental update (see [`update`]), with CMS signed data over every byte of the
//! file but the signature itself (see [`envelope`]).
//!
//! Keys come from a PKCS#12 file (RSA or ECDSA P-256) or from a [`Signer`] outside, such as a
//! hardware security module or a key management service: Fagus hands it the digest to sign.
//! Timestamps come from a [`Timestamper`] as well, since Fagus never goes to the network.
//! The output is reproducible: the time of signing is an input, RSA PKCS#1 v1.5 signatures are
//! deterministic, and ECDSA signatures follow RFC 6979.

mod envelope;
mod update;

use std::fmt::Write as _;
use std::sync::Arc;

use der::{Decode, Encode};
use p256::pkcs8::DecodePrivateKey as _;
use rsa::pkcs8::DecodePublicKey as _;
use rsa::signature::hazmat::{PrehashSigner as _, PrehashVerifier as _};
use sha2::{Digest as _, Sha256};
use x509_cert::Certificate;

use crate::{Date, PdfError};

/// The algorithm of a signature, which follows from the signer's certificate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Algorithm {
    /// RSA with PKCS#1 v1.5 padding and SHA-256.
    RsaSha256,
    /// ECDSA on the curve P-256 with SHA-256.
    EcdsaP256Sha256,
}

impl Algorithm {
    /// The name of the algorithm: `RSA-SHA256` or `ECDSA-P256-SHA256`.
    pub fn name(self) -> &'static str {
        match self {
            Self::RsaSha256 => "RSA-SHA256",
            Self::EcdsaP256Sha256 => "ECDSA-P256-SHA256",
        }
    }
}

/// Signs digests with a key Fagus does not hold, such as one in a hardware security module or a
/// key management service.
///
/// `sign` gets the SHA-256 digest (32 bytes) of the CMS signed attributes and returns the
/// signature of that digest, as the key's owner computes it over data hashed already:
/// for [`Algorithm::RsaSha256`] the PKCS#1 v1.5 signature (with the `DigestInfo` of SHA-256),
/// for [`Algorithm::EcdsaP256Sha256`] the ECDSA signature, DER-encoded or as the 64 bytes of r
/// and s. Fagus checks the signature against the certificate.
pub trait Signer: Send + Sync {
    fn sign(&self, digest: &[u8], algorithm: Algorithm) -> Result<Vec<u8>, String>;
}

impl<F> Signer for F
where
    F: Fn(&[u8], Algorithm) -> Result<Vec<u8>, String> + Send + Sync,
{
    fn sign(&self, digest: &[u8], algorithm: Algorithm) -> Result<Vec<u8>, String> {
        self(digest, algorithm)
    }
}

/// Gets timestamps from a time-stamping authority (RFC 3161), for `PAdES` B-T.
///
/// `timestamp` gets the SHA-256 digest (32 bytes) of the signature value and returns the
/// authority's answer: the timestamp token (DER), or the whole `TimeStampResp`. Fagus checks
/// that it is a timestamp of that digest. [`timestamp_request`] makes the request to send.
pub trait Timestamper: Send + Sync {
    fn timestamp(&self, digest: &[u8]) -> Result<Vec<u8>, String>;
}

impl<F> Timestamper for F
where
    F: Fn(&[u8]) -> Result<Vec<u8>, String> + Send + Sync,
{
    fn timestamp(&self, digest: &[u8]) -> Result<Vec<u8>, String> {
        self(digest)
    }
}

/// The RFC 3161 request (`TimeStampReq`, DER) for a timestamp of a SHA-256 `digest`, which asks
/// the authority for its certificate too: what an HTTP POST to the authority sends, with the
/// content type `application/timestamp-query`.
pub fn timestamp_request(digest: &[u8; 32]) -> Vec<u8> {
    // SEQUENCE { version 1, messageImprint { sha256, digest }, certReq TRUE }
    let mut request = vec![
        0x30, 0x39, 0x02, 0x01, 0x01, 0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03,
        0x04, 0x02, 0x01, 0x05, 0x00, 0x04, 0x20,
    ];
    request.extend_from_slice(digest);
    request.extend_from_slice(&[0x01, 0x01, 0xff]);
    request
}

/// A key Fagus holds.
enum Key {
    Rsa(Box<rsa::pkcs1v15::SigningKey<Sha256>>),
    Ecdsa(Box<p256::ecdsa::SigningKey>),
}

impl Signer for Key {
    fn sign(&self, digest: &[u8], _: Algorithm) -> Result<Vec<u8>, String> {
        match self {
            Self::Rsa(key) => key
                .sign_prehash(digest)
                .map(|signature| Box::<[u8]>::from(signature).into_vec())
                .map_err(|error| error.to_string()),
            // Deterministic (RFC 6979).
            Self::Ecdsa(key) => key
                .sign_prehash(digest)
                .map(|signature: p256::ecdsa::Signature| signature.to_der().as_bytes().to_vec())
                .map_err(|error| error.to_string()),
        }
    }
}

/// A signature to put on the PDF: who signs, with what, and what the signature says.
#[derive(Clone)]
pub struct Signature {
    /// The certificates, the signer's first.
    certificates: Arc<Vec<Vec<u8>>>,
    signer: Arc<dyn Signer>,
    timestamper: Option<Arc<dyn Timestamper>>,
    name: Option<String>,
    reason: Option<String>,
    location: Option<String>,
    contact_info: Option<String>,
    time: Option<Date>,
}

impl std::fmt::Debug for Signature {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Signature")
            .field("certificates", &self.certificates.len())
            .field("timestamped", &self.timestamper.is_some())
            .field("name", &self.name)
            .field("reason", &self.reason)
            .field("location", &self.location)
            .field("contact_info", &self.contact_info)
            .field("time", &self.time)
            .finish_non_exhaustive()
    }
}

impl Signature {
    /// A signature with the key and certificates of a PKCS#12 file (`.p12`, `.pfx`) and its
    /// password. The key is RSA or ECDSA on P-256; the file's other certificates go into the
    /// signature too, so that readers can follow the chain to its root.
    pub fn pkcs12(data: &[u8], password: &str) -> Result<Self, PdfError> {
        let failure = |message: String| PdfError::Sign(message);
        let store =
            p12_keystore::KeyStore::from_pkcs12(data, password, p12_keystore::Pkcs12ImportPolicy::Relaxed)
                .map_err(|error| failure(format!("the PKCS#12 file cannot be read: {error}")))?;
        let (_, chain) = store
            .private_key_chain()
            .ok_or_else(|| failure("the PKCS#12 file holds no private key".to_owned()))?;
        let certificates: Vec<Vec<u8>> = chain
            .certs()
            .iter()
            .map(|certificate| certificate.as_der().to_vec())
            .collect();
        let first = certificates
            .first()
            .ok_or_else(|| failure("the PKCS#12 file holds no certificate for its key".to_owned()))?;
        let certificate = Certificate::from_der(first)
            .map_err(|error| failure(format!("the certificate cannot be read: {error}")))?;
        let algorithm = algorithm(&certificate).map_err(failure)?;
        let key_der = chain.key().as_der();
        let mismatch = || failure("the certificate does not belong to the private key".to_owned());
        let spki = certificate
            .tbs_certificate()
            .subject_public_key_info()
            .to_der()
            .map_err(|error| failure(error.to_string()))?;
        let key = match algorithm {
            Algorithm::RsaSha256 => {
                let key = rsa::PrivateKey::from_pkcs8_der(key_der)
                    .map_err(|error| failure(format!("the RSA key cannot be read: {error}")))?;
                let public = rsa::PublicKey::from_public_key_der(&spki).map_err(|_| mismatch())?;
                if key.to_public_key() != public {
                    return Err(mismatch());
                }
                Key::Rsa(Box::new(rsa::pkcs1v15::SigningKey::<Sha256>::new(key)))
            }
            Algorithm::EcdsaP256Sha256 => {
                let key = p256::ecdsa::SigningKey::from_pkcs8_der(key_der)
                    .map_err(|error| failure(format!("the ECDSA key cannot be read: {error}")))?;
                let public = p256::ecdsa::VerifyingKey::from_public_key_der(&spki).map_err(|_| mismatch())?;
                if *key.verifying_key() != public {
                    return Err(mismatch());
                }
                Key::Ecdsa(Box::new(key))
            }
        };
        Ok(Self::new(certificates, Arc::new(key)))
    }

    /// A signature by a [`Signer`] outside, such as a hardware security module, with the
    /// signer's certificate first in `certificates` and then those of its chain, each DER or
    /// PEM (a PEM text may hold several).
    pub fn with_signer(certificates: Vec<Vec<u8>>, signer: impl Signer + 'static) -> Self {
        Self::new(certificates, Arc::new(signer))
    }

    fn new(certificates: Vec<Vec<u8>>, signer: Arc<dyn Signer>) -> Self {
        Self {
            certificates: Arc::new(certificates),
            signer,
            timestamper: None,
            name: None,
            reason: None,
            location: None,
            contact_info: None,
            time: None,
        }
    }

    /// Timestamps the signature (`PAdES` B-T).
    #[must_use]
    pub fn timestamp(mut self, timestamper: impl Timestamper + 'static) -> Self {
        self.timestamper = Some(Arc::new(timestamper));
        self
    }

    /// The name of the signer, as the signature shows it (`/Name`); readers show the name of
    /// the certificate otherwise.
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Why the document is signed (`/Reason`).
    #[must_use]
    pub fn reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// Where the document is signed (`/Location`).
    #[must_use]
    pub fn location(mut self, location: impl Into<String>) -> Self {
        self.location = Some(location.into());
        self
    }

    /// How to reach the signer (`/ContactInfo`).
    #[must_use]
    pub fn contact_info(mut self, contact_info: impl Into<String>) -> Self {
        self.contact_info = Some(contact_info.into());
        self
    }

    /// The time of signing (`/M`); without it, the time the caller of [`sign`] gives.
    #[must_use]
    pub fn time(mut self, time: Date) -> Self {
        self.time = Some(time);
        self
    }

    /// The certificates, the signer's first, read from DER or PEM.
    fn chain(&self) -> Result<Vec<Certificate>, String> {
        let mut chain = Vec::new();
        for data in self.certificates.iter() {
            if data.starts_with(b"-----BEGIN") {
                chain.extend(
                    Certificate::load_pem_chain(data)
                        .map_err(|error| format!("a certificate cannot be read: {error}"))?,
                );
            } else {
                chain.push(
                    Certificate::from_der(data)
                        .map_err(|error| format!("a certificate cannot be read: {error}"))?,
                );
            }
        }
        if chain.is_empty() {
            return Err("the signature has no certificate".to_owned());
        }
        Ok(chain)
    }
}

/// The algorithm of signatures by the holder of `certificate`.
fn algorithm(certificate: &Certificate) -> Result<Algorithm, String> {
    let key = &certificate.tbs_certificate().subject_public_key_info().algorithm;
    if key.oid == envelope::RSA_ENCRYPTION {
        return Ok(Algorithm::RsaSha256);
    }
    if key.oid == envelope::EC_PUBLIC_KEY {
        let curve = key
            .parameters
            .as_ref()
            .and_then(|parameters| parameters.decode_as::<der::asn1::ObjectIdentifier>().ok());
        if curve == Some(envelope::PRIME256V1) {
            return Ok(Algorithm::EcdsaP256Sha256);
        }
        return Err("only ECDSA keys on the curve P-256 can sign".to_owned());
    }
    Err(format!("keys of the type {} cannot sign", key.oid))
}

/// Checks a signature value against the certificate and returns it as CMS takes it.
fn checked(
    certificate: &Certificate,
    algorithm: Algorithm,
    digest: &[u8],
    value: Vec<u8>,
) -> Result<Vec<u8>, String> {
    let wrong = || {
        format!(
            "the signer's {} signature does not match the certificate (it must sign the digest \
             as given, without hashing it again)",
            algorithm.name()
        )
    };
    let spki = certificate
        .tbs_certificate()
        .subject_public_key_info()
        .to_der()
        .map_err(|error| error.to_string())?;
    match algorithm {
        Algorithm::RsaSha256 => {
            let key = rsa::PublicKey::from_public_key_der(&spki).map_err(|error| error.to_string())?;
            let signature = rsa::pkcs1v15::Signature::try_from(value.as_slice()).map_err(|_| wrong())?;
            rsa::pkcs1v15::VerifyingKey::<Sha256>::new(key)
                .verify_prehash(digest, &signature)
                .map_err(|_| wrong())?;
            Ok(value)
        }
        Algorithm::EcdsaP256Sha256 => {
            let key =
                p256::ecdsa::VerifyingKey::from_public_key_der(&spki).map_err(|error| error.to_string())?;
            let der = envelope::ecdsa_der(&value).ok_or_else(wrong)?;
            let signature = p256::ecdsa::DerSignature::from_bytes(&der).map_err(|_| wrong())?;
            key.verify_prehash(digest, &signature).map_err(|_| wrong())?;
            Ok(der)
        }
    }
}

/// A text string of PDF: literal for printable ASCII, UTF-16BE otherwise.
fn text(value: &str) -> String {
    if value.chars().all(|character| matches!(character, ' '..='~')) {
        let mut out = String::from("(");
        for character in value.chars() {
            if matches!(character, '(' | ')' | '\\') {
                out.push('\\');
            }
            out.push(character);
        }
        out.push(')');
        return out;
    }
    let mut out = String::from("<FEFF");
    for unit in value.encode_utf16() {
        write!(out, "{unit:04X}").expect("written to a string");
    }
    out.push('>');
    out
}

/// Bytes reserved for the signature besides the certificates and the signature value: the
/// structure of the signed data and the signed attributes.
const OVERHEAD: usize = 2048;

/// Bytes reserved for a timestamp token, which holds a signature and usually the certificates
/// of the authority.
const TIMESTAMP_ROOM: usize = 12 * 1024;

/// Signs `pdf` as an incremental update. `time` is the time of signing unless the signature has
/// its own.
pub fn sign(pdf: &[u8], signature: &Signature, time: Date) -> Result<Vec<u8>, PdfError> {
    sign_inner(pdf, signature, time).map_err(PdfError::Sign)
}

fn sign_inner(pdf: &[u8], signature: &Signature, time: Date) -> Result<Vec<u8>, String> {
    let chain = signature.chain()?;
    let algorithm = algorithm(&chain[0])?;
    let time = signature.time.unwrap_or(time);
    let mut entries = format!(
        "/M(D:{:04}{:02}{:02}{:02}{:02}{:02}Z)",
        time.year, time.month, time.day, time.hour, time.minute, time.second
    );
    for (key, value) in [
        ("/Name", &signature.name),
        ("/Reason", &signature.reason),
        ("/Location", &signature.location),
        ("/ContactInfo", &signature.contact_info),
    ] {
        if let Some(value) = value {
            entries.push_str(key);
            entries.push_str(&text(value));
        }
    }
    let fields = update::Fields { entries };
    // By their DER, so that the room (and the file) does not depend on how they were given.
    let certificates: usize = chain
        .iter()
        .map(|certificate| certificate.to_der().map_or(0, |der| der.len()))
        .sum();
    let value_size = match algorithm {
        Algorithm::RsaSha256 => 1024,
        Algorithm::EcdsaP256Sha256 => 72,
    };
    let mut room = certificates
        + value_size
        + OVERHEAD
        + if signature.timestamper.is_some() {
            TIMESTAMP_ROOM
        } else {
            0
        };
    // A second attempt with the room the first needed, should a timestamp be larger than
    // expected.
    for _ in 0..2 {
        let mut prepared = update::prepare(pdf, &fields, room)?;
        let mut hasher = Sha256::new();
        for range in prepared.signed_ranges() {
            hasher.update(range);
        }
        let digest = hasher.finalize();
        let error = |error: der::Error| format!("the signature cannot be encoded: {error}");
        let attributes = envelope::signed_attributes(&digest, &chain[0]).map_err(error)?;
        let to_sign = envelope::attributes_digest(&attributes).map_err(error)?;
        let value = signature
            .signer
            .sign(&to_sign, algorithm)
            .map_err(|message| format!("the signer failed: {message}"))?;
        let value = checked(&chain[0], algorithm, &to_sign, value)?;
        let answer = match &signature.timestamper {
            None => None,
            Some(timestamper) => {
                let digest = Sha256::digest(&value);
                let answer = timestamper
                    .timestamp(&digest)
                    .map_err(|message| format!("the timestamp failed: {message}"))?;
                Some((answer, digest))
            }
        };
        let token = match &answer {
            None => None,
            Some((answer, digest)) => Some(envelope::timestamp_token(answer, digest)?),
        };
        let signed = envelope::signed_data(&chain, attributes, algorithm, &value, token).map_err(error)?;
        if prepared.fill(&signed) {
            return Ok(prepared.pdf);
        }
        room = signed.len() + OVERHEAD;
    }
    Err("the signature does not fit into the room reserved for it".to_owned())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use cms::content_info::ContentInfo;
    use cms::signed_data::SignedData;
    use fagus_paint::{Item, PageList, RectF};
    use fagus_style::values::Rgba;
    use fagus_text::FontDb;

    use super::*;
    use crate::parse;
    use crate::{Conformance, Link, LinkTarget, Metadata, Options};

    const PASSWORD: &str = "fagus-test-only";
    const TIME: Date = Date {
        year: 2026,
        month: 11,
        day: 19,
        hour: 9,
        minute: 30,
        second: 0,
    };

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/signing")
            .join(name)
    }

    fn read(name: &str) -> Vec<u8> {
        std::fs::read(fixture(name)).expect("fixture")
    }

    /// A PDF of two pages with a link on each, as the engine writes it.
    fn sample(conformance: Option<Conformance>) -> Vec<u8> {
        let page = PageList {
            width: 200.0,
            height: 100.0,
            items: vec![Item::Rect {
                rect: RectF {
                    x: 10.0,
                    y: 10.0,
                    width: 50.0,
                    height: 20.0,
                },
                color: Rgba::BLACK,
                radii: None,
            }],
            bleed: 0.0,
            marks: fagus_layout::PageMarks::default(),
            orientation: fagus_layout::PageOrientation::Upright,
        };
        let link = Link {
            rect: [10.0, 10.0, 50.0, 20.0],
            target: LinkTarget::Uri("https://fagus.niklasforst.de".to_owned()),
            node: None,
            description: Some("Fagus".to_owned()),
            role: crate::LinkRole::Element,
        };
        let options = Options {
            conformance,
            links: vec![vec![link.clone()], vec![link]],
            ..Options::default()
        };
        let metadata = Metadata {
            title: Some("Rechnung".to_owned()),
            created: Some(TIME),
            ..Metadata::default()
        };
        crate::write(vec![page.clone(), page], &FontDb::new(), &metadata, &options)
            .expect("written")
            .pdf
    }

    fn pkcs12(name: &str) -> Signature {
        Signature::pkcs12(&read(name), PASSWORD).expect("PKCS#12 file")
    }

    /// The last signature of `pdf`, checked: its byte range covers the file but the signature,
    /// the message digest is that of those bytes, and the signature over the signed attributes
    /// verifies with the signer's certificate. Returns the signed data and the signed bytes.
    fn verify(pdf: &[u8]) -> (SignedData, Vec<u8>, Vec<u8>) {
        let at = parse::rfind(pdf, b"/ByteRange[").expect("byte range") + 11;
        let end = at + pdf[at..].iter().position(|&byte| byte == b']').expect("end");
        let range: Vec<usize> = std::str::from_utf8(&pdf[at..end])
            .expect("text")
            .split_whitespace()
            .map(|number| number.parse().expect("number"))
            .collect();
        assert_eq!(range[0], 0);
        assert_eq!(range[2] + range[3], pdf.len(), "the range reaches the end");
        assert!(pdf[..range[1]].ends_with(b"/Contents"));
        assert_eq!(pdf[range[1]], b'<');
        assert_eq!(pdf[range[2] - 1], b'>');
        let hex = std::str::from_utf8(&pdf[range[1] + 1..range[2] - 1]).expect("hex");
        let bytes: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&hex[index..index + 2], 16).expect("hex"))
            .collect();
        // The signature, without the zeros after it.
        let (_, _, padding) = envelope::element(&bytes).expect("DER");
        let der = bytes[..bytes.len() - padding.len()].to_vec();
        assert!(padding.iter().all(|&byte| byte == 0));
        let content_info = ContentInfo::from_der(&der).expect("content info");
        let signed_data: SignedData = content_info.content.decode_as().expect("signed data");
        let signed: Vec<u8> = [&pdf[..range[1]], &pdf[range[2]..]].concat();
        let signer = &signed_data.signer_infos.0.as_slice()[0];
        let attributes = signer.signed_attrs.as_ref().expect("signed attributes");
        let digest = attributes
            .iter()
            .find(|attribute| attribute.oid.to_string() == "1.2.840.113549.1.9.4")
            .expect("message digest")
            .values
            .get(0)
            .expect("value")
            .decode_as::<der::asn1::OctetString>()
            .expect("octets");
        assert_eq!(digest.as_bytes(), Sha256::digest(&signed).as_slice());
        let oids: Vec<String> = attributes
            .iter()
            .map(|attribute| attribute.oid.to_string())
            .collect();
        assert!(
            oids.contains(&"1.2.840.113549.1.9.16.2.47".to_owned()),
            "signing certificate v2"
        );
        assert!(
            !oids.contains(&"1.2.840.113549.1.9.5".to_owned()),
            "no signing time (PAdES)"
        );
        let certificate = signed_data
            .certificates
            .as_ref()
            .expect("certificates")
            .0
            .iter()
            .find_map(|choice| match choice {
                cms::cert::CertificateChoices::Certificate(certificate)
                    if certificate.tbs_certificate().serial_number()
                        == match &signer.sid {
                            cms::signed_data::SignerIdentifier::IssuerAndSerialNumber(sid) => {
                                &sid.serial_number
                            }
                            cms::signed_data::SignerIdentifier::SubjectKeyIdentifier(_) => {
                                panic!("issuer and serial number")
                            }
                        } =>
                {
                    Some(certificate.clone())
                }
                _ => None,
            })
            .expect("the signer's certificate");
        let algorithm = algorithm(&certificate).expect("algorithm");
        let to_sign = envelope::attributes_digest(attributes).expect("digest");
        checked(
            &certificate,
            algorithm,
            &to_sign,
            signer.signature.as_bytes().to_vec(),
        )
        .expect("the signature verifies");
        (signed_data, der, signed)
    }

    /// `openssl cms -verify` on the signature, if OpenSSL is there.
    fn openssl_verify(name: &str, der: &[u8], signed: &[u8]) {
        if Command::new("openssl").arg("version").output().is_err() {
            eprintln!("no openssl: {name} not checked with it");
            return;
        }
        let directory = std::env::temp_dir().join(format!("fagus-sign-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("directory");
        std::fs::write(directory.join("signature.der"), der).expect("written");
        std::fs::write(directory.join("data"), signed).expect("written");
        let output = Command::new("openssl")
            .args(["cms", "-verify", "-binary", "-inform", "DER", "-in"])
            .arg(directory.join("signature.der"))
            .arg("-content")
            .arg(directory.join("data"))
            .arg("-CAfile")
            .arg(fixture("ca.pem"))
            .args(["-purpose", "any", "-out"])
            .arg(directory.join("out"))
            .output()
            .expect("openssl");
        std::fs::remove_dir_all(&directory).ok();
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn signs_with_pkcs12_files() {
        let pdf = sample(None);
        for name in ["rsa.p12", "ecdsa.p12", "rsa-legacy.p12"] {
            let signature = pkcs12(name).reason("Rechnung").location("Köln");
            let signed = sign(&pdf, &signature, TIME).expect("signed");
            // An incremental update: the file as it was, then the signature.
            assert!(signed.starts_with(&pdf), "{name}");
            let (signed_data, der, data) = verify(&signed);
            // The signer and the test CA.
            assert_eq!(
                signed_data.certificates.expect("certificates").0.len(),
                2,
                "{name}"
            );
            openssl_verify(name, &der, &data);
            let text = String::from_utf8_lossy(&signed[pdf.len()..]).into_owned();
            assert!(text.contains("/M(D:20261119093000Z)"), "{text}");
            assert!(
                text.contains("/Reason(Rechnung)/Location<FEFF004B00F6006C006E>"),
                "{text}"
            );
            assert!(text.contains("/SubFilter/ETSI.CAdES.detached"), "{text}");
            // Signed twice, the same bytes.
            assert_eq!(sign(&pdf, &signature, TIME).expect("signed"), signed, "{name}");
        }
    }

    #[test]
    fn adds_the_field_to_the_first_page_and_the_form() {
        let pdf = sample(None);
        let signed = sign(&pdf, &pkcs12("rsa.p12"), TIME).expect("signed");
        let file = update::File::read(&signed).expect("readable");
        let catalog = file
            .dictionary(
                update::reference(parse::entry(&file.trailer, "/Root").expect("root")).expect("reference"),
            )
            .expect("catalog");
        let form = parse::entry(&catalog, "/AcroForm").expect("form");
        assert!(form.contains("/SigFlags 3"), "{form}");
        assert!(catalog.contains("/Extensions<</ESIC<</BaseVersion/1.7/ExtensionLevel 2>>>>"));
        let page = file
            .dictionary(file.first_page(&catalog).expect("page"))
            .expect("page");
        let annotations = parse::entry(&page, "/Annots").expect("annotations");
        // The link and the signature.
        assert_eq!(annotations.matches(" R").count(), 2, "{annotations}");
        // Signed again: a second field with another name.
        let twice = sign(&signed, &pkcs12("ecdsa.p12"), TIME).expect("signed");
        assert!(parse::find(&twice[signed.len()..], b"/T(Signature2)").is_some());
        verify(&twice);
    }

    #[test]
    fn signs_files_with_cross_reference_tables() {
        // PDF/A-1b is PDF 1.4: a cross-reference table, no object streams.
        let pdf = sample(Some(Conformance::PdfA1b));
        assert!(parse::find(&pdf, b"\nxref\n").is_some());
        let signed = sign(&pdf, &pkcs12("rsa.p12"), TIME).expect("signed");
        let (_, der, data) = verify(&signed);
        openssl_verify("pdfa1b", &der, &data);
        assert!(
            parse::find(&signed[pdf.len()..], b"\nxref\n").is_some()
                || signed[pdf.len()..].starts_with(b"xref")
        );
        assert!(update::File::read(&signed).is_some());
    }

    #[test]
    fn signs_with_signers_outside() {
        let pdf = sample(None);
        for name in ["rsa.p12", "ecdsa.p12"] {
            let own = pkcs12(name);
            let expected = sign(&pdf, &own, TIME).expect("signed");
            let certificates = (*own.certificates).clone();
            let key = Arc::clone(&own.signer);
            // The same key outside gives the same file.
            let outside = Signature::with_signer(certificates.clone(), move |digest: &[u8], algorithm| {
                key.sign(digest, algorithm)
            });
            assert_eq!(sign(&pdf, &outside, TIME).expect("signed"), expected, "{name}");
            // ECDSA signatures may come as r and s.
            if name == "ecdsa.p12" {
                let key = Arc::clone(&own.signer);
                let raw = Signature::with_signer(certificates.clone(), move |digest: &[u8], algorithm| {
                    let der = key.sign(digest, algorithm)?;
                    let signature =
                        p256::ecdsa::Signature::from_der(&der).map_err(|error| error.to_string())?;
                    Ok(signature.to_bytes().to_vec())
                });
                assert_eq!(sign(&pdf, &raw, TIME).expect("signed"), expected);
            }
            // A signer that hashes the digest once more signs the wrong thing.
            let key = Arc::clone(&own.signer);
            let wrong = Signature::with_signer(certificates, move |digest: &[u8], algorithm| {
                key.sign(&Sha256::digest(digest), algorithm)
            });
            let error = sign(&pdf, &wrong, TIME).expect_err("refused").to_string();
            assert!(error.contains("does not match the certificate"), "{error}");
        }
        // PEM certificates.
        let own = pkcs12("rsa.p12");
        let key = Arc::clone(&own.signer);
        let pem = Signature::with_signer(
            vec![read("rsa.pem"), read("ca.pem")],
            move |digest: &[u8], algorithm| key.sign(digest, algorithm),
        );
        // The same file as with DER certificates.
        assert_eq!(
            sign(&sample(None), &pem, TIME).expect("signed"),
            sign(&sample(None), &own, TIME).expect("signed")
        );
    }

    #[test]
    fn reports_what_keeps_it_from_signing() {
        let error = Signature::pkcs12(&read("rsa.p12"), "wrong").expect_err("wrong password");
        assert!(
            error.to_string().contains("the PKCS#12 file cannot be read"),
            "{error}"
        );
        let refusing = Signature::with_signer(vec![read("rsa.pem")], |_: &[u8], _| Err("no".to_owned()));
        let error = sign(&sample(None), &refusing, TIME).expect_err("refused");
        assert_eq!(
            error.to_string(),
            "the PDF could not be signed: the signer failed: no"
        );
        let nothing =
            Signature::with_signer(vec![b"not a certificate".to_vec()], |_: &[u8], _| Ok(Vec::new()));
        assert!(sign(&sample(None), &nothing, TIME).is_err());
        assert!(sign(b"%PDF-1.7\nnot really", &pkcs12("rsa.p12"), TIME).is_err());
    }

    /// A time-stamping authority with the test certificates, through `openssl ts`.
    fn openssl_timestamp(digest: &[u8]) -> Result<Vec<u8>, String> {
        let directory = std::env::temp_dir().join(format!("fagus-tsa-{}", std::process::id()));
        std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        let configuration = format!(
            "[tsa]\ndefault_tsa = test\n[test]\nserial = {}\nsigner_cert = {}\nsigner_key = {}\n\
             signer_digest = sha256\ndefault_policy = 1.2.3.4\ndigests = sha256\n\
             ess_cert_id_alg = sha256\naccuracy = secs:1\n",
            // OpenSSL's configuration takes backslashes as escapes.
            directory.join("serial").display().to_string().replace('\\', "/"),
            fixture("tsa.pem").display().to_string().replace('\\', "/"),
            fixture("tsa.key").display().to_string().replace('\\', "/")
        );
        std::fs::write(directory.join("tsa.cnf"), configuration).map_err(|error| error.to_string())?;
        std::fs::write(directory.join("serial"), "01\n").map_err(|error| error.to_string())?;
        let digest: &[u8; 32] = digest.try_into().map_err(|_| "digest")?;
        std::fs::write(directory.join("request.tsq"), timestamp_request(digest))
            .map_err(|error| error.to_string())?;
        let output = Command::new("openssl")
            .args(["ts", "-reply", "-config"])
            .arg(directory.join("tsa.cnf"))
            .arg("-queryfile")
            .arg(directory.join("request.tsq"))
            .arg("-out")
            .arg(directory.join("response.tsr"))
            .output()
            .map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned());
        }
        std::fs::read(directory.join("response.tsr")).map_err(|error| error.to_string())
    }

    #[test]
    fn timestamps_signatures() {
        if Command::new("openssl").arg("version").output().is_err() {
            eprintln!("no openssl: timestamps not checked");
            return;
        }
        let pdf = sample(None);
        let signature = pkcs12("rsa.p12").timestamp(openssl_timestamp);
        let signed = sign(&pdf, &signature, TIME).expect("signed");
        let (signed_data, der, data) = verify(&signed);
        let signer = &signed_data.signer_infos.0.as_slice()[0];
        let token = signer
            .unsigned_attrs
            .as_ref()
            .and_then(|attributes| {
                attributes
                    .iter()
                    .find(|attribute| attribute.oid.to_string() == "1.2.840.113549.1.9.16.2.14")
            })
            .expect("a timestamp");
        let token = token.values.get(0).expect("token").to_der().expect("der");
        let imprint = envelope::timestamp_token(&token, &Sha256::digest(signer.signature.as_bytes()))
            .expect("a timestamp of the signature");
        assert_eq!(imprint, token.as_slice());
        openssl_verify("timestamped", &der, &data);
        // A timestamp of something else is refused.
        let other = pkcs12("rsa.p12").timestamp(|_: &[u8]| openssl_timestamp(&[1; 32]));
        let error = sign(&pdf, &other, TIME).expect_err("refused").to_string();
        assert!(error.contains("not one of this signature"), "{error}");
    }

    #[test]
    fn writes_text_strings() {
        assert_eq!(text("Rechnung (Kopie) \\"), "(Rechnung \\(Kopie\\) \\\\)");
        assert_eq!(text("Köln"), "<FEFF004B00F6006C006E>");
    }

    #[test]
    fn asks_for_timestamps() {
        let request = timestamp_request(&[7; 32]);
        assert_eq!(request.len(), 59);
        assert_eq!(usize::from(request[1]) + 2, request.len());
    }
}

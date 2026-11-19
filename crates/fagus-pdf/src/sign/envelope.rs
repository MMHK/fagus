//! The signature itself: CMS signed data (RFC 5652) of the kind `PAdES` requires (ETSI EN 319 142-1,
//! `ETSI.CAdES.detached`). The document's digest is signed through signed attributes, which name
//! the signer's certificate (`signing-certificate-v2`) and leave the time out: the claimed time is
//! the signature dictionary's `/M`. A timestamp of the signature comes as an unsigned attribute.

use cms::cert::{CertificateChoices, IssuerAndSerialNumber};
use cms::content_info::{CmsVersion, ContentInfo};
use cms::signed_data::{
    CertificateSet, EncapsulatedContentInfo, SignedData, SignerIdentifier, SignerInfo, SignerInfos,
};
use der::asn1::{ObjectIdentifier, OctetString, SetOfVec};
use der::{Any, Decode, Encode, Sequence};
use sha2::{Digest, Sha256};
use x509_cert::Certificate;
use x509_cert::attr::Attribute;
use x509_cert::ext::pkix::name::GeneralName;
use x509_cert::serial_number::SerialNumber;
use x509_cert::spki::AlgorithmIdentifierOwned;

use super::Algorithm;

const ID_DATA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.1");
const ID_SIGNED_DATA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.2");
const ID_CONTENT_TYPE: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.3");
const ID_MESSAGE_DIGEST: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.4");
const ID_SIGNING_CERTIFICATE_V2: ObjectIdentifier =
    ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.16.2.47");
const ID_TIMESTAMP_TOKEN: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.16.2.14");
const ID_TST_INFO: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.16.1.4");
const ID_SHA256: ObjectIdentifier = ObjectIdentifier::new_unwrap("2.16.840.1.101.3.4.2.1");
const SHA256_WITH_RSA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.11");
const ECDSA_WITH_SHA256: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.4.3.2");
pub(super) const RSA_ENCRYPTION: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.1");
pub(super) const EC_PUBLIC_KEY: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.2.1");
pub(super) const PRIME256V1: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.3.1.7");

/// `ESSCertIDv2` (RFC 5035), with the hash algorithm left at its default, SHA-256, which DER
/// leaves out.
#[derive(Sequence)]
struct EssCertIdV2 {
    cert_hash: OctetString,
    issuer_serial: IssuerSerial,
}

/// `IssuerSerial` (RFC 5035).
#[derive(Sequence)]
struct IssuerSerial {
    issuer: Vec<GeneralName>,
    serial_number: SerialNumber,
}

/// `SigningCertificateV2` (RFC 5035), without policies.
#[derive(Sequence)]
struct SigningCertificateV2 {
    certs: Vec<EssCertIdV2>,
}

fn sha256(oid: ObjectIdentifier) -> AlgorithmIdentifierOwned {
    AlgorithmIdentifierOwned {
        oid,
        parameters: None,
    }
}

fn attribute<T: der::EncodeValue + der::Tagged>(oid: ObjectIdentifier, value: &T) -> der::Result<Attribute> {
    let mut values = SetOfVec::new();
    values.insert(Any::encode_from(value)?)?;
    Ok(Attribute { oid, values })
}

/// The signed attributes of a signature over a document with `digest` (SHA-256), by the holder
/// of `certificate`.
pub(super) fn signed_attributes(
    digest: &[u8],
    certificate: &Certificate,
) -> der::Result<SetOfVec<Attribute>> {
    let tbs = certificate.tbs_certificate();
    let signing_certificate = SigningCertificateV2 {
        certs: vec![EssCertIdV2 {
            cert_hash: OctetString::new(Sha256::digest(certificate.to_der()?).to_vec())?,
            issuer_serial: IssuerSerial {
                issuer: vec![GeneralName::DirectoryName(tbs.issuer().clone())],
                serial_number: tbs.serial_number().clone(),
            },
        }],
    };
    let mut attributes = SetOfVec::new();
    attributes.insert(attribute(ID_CONTENT_TYPE, &ID_DATA)?)?;
    attributes.insert(attribute(ID_MESSAGE_DIGEST, &OctetString::new(digest.to_vec())?)?)?;
    attributes.insert(attribute(ID_SIGNING_CERTIFICATE_V2, &signing_certificate)?)?;
    Ok(attributes)
}

/// The digest the signer signs: SHA-256 of the signed attributes, encoded as a SET OF.
pub(super) fn attributes_digest(attributes: &SetOfVec<Attribute>) -> der::Result<[u8; 32]> {
    Ok(Sha256::digest(attributes.to_der()?).into())
}

/// The CMS signed data (as `ContentInfo`, DER) of a signature.
pub(super) fn signed_data(
    certificates: &[Certificate],
    attributes: SetOfVec<Attribute>,
    algorithm: Algorithm,
    signature: &[u8],
    timestamp: Option<&[u8]>,
) -> der::Result<Vec<u8>> {
    let signer = &certificates[0];
    let signature_algorithm = match algorithm {
        // RFC 4055: the parameters of the RSA algorithms are NULL.
        Algorithm::RsaSha256 => AlgorithmIdentifierOwned {
            oid: SHA256_WITH_RSA,
            parameters: Some(Any::null()),
        },
        Algorithm::EcdsaP256Sha256 => sha256(ECDSA_WITH_SHA256),
    };
    let unsigned_attrs = match timestamp {
        None => None,
        Some(token) => {
            let mut attributes = SetOfVec::new();
            attributes.insert(Attribute {
                oid: ID_TIMESTAMP_TOKEN,
                values: SetOfVec::try_from(vec![Any::from_der(token)?])?,
            })?;
            Some(attributes)
        }
    };
    let signer_info = SignerInfo {
        version: CmsVersion::V1,
        sid: SignerIdentifier::IssuerAndSerialNumber(IssuerAndSerialNumber {
            issuer: signer.tbs_certificate().issuer().clone(),
            serial_number: signer.tbs_certificate().serial_number().clone(),
        }),
        digest_alg: sha256(ID_SHA256),
        signed_attrs: Some(attributes),
        signature_algorithm,
        signature: OctetString::new(signature.to_vec())?,
        unsigned_attrs,
    };
    let mut set = SetOfVec::new();
    for certificate in certificates {
        set.insert(CertificateChoices::Certificate(certificate.clone()))?;
    }
    let signed_data = SignedData {
        version: CmsVersion::V1,
        digest_algorithms: SetOfVec::try_from(vec![sha256(ID_SHA256)])?,
        encap_content_info: EncapsulatedContentInfo {
            econtent_type: ID_DATA,
            econtent: None,
        },
        certificates: Some(CertificateSet(set)),
        crls: None,
        signer_infos: SignerInfos(SetOfVec::try_from(vec![signer_info])?),
    };
    ContentInfo {
        content_type: ID_SIGNED_DATA,
        content: Any::encode_from(&signed_data)?,
    }
    .to_der()
}

/// One element of DER (or BER with definite lengths): its tag, its content and what follows.
pub(super) fn element(bytes: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, rest) = bytes.split_first()?;
    let (&first, rest) = rest.split_first()?;
    let (length, rest) = if first < 0x80 {
        (usize::from(first), rest)
    } else {
        let count = usize::from(first & 0x7f);
        if count == 0 || count > 4 || rest.len() < count {
            return None;
        }
        let length = rest[..count]
            .iter()
            .fold(0_usize, |length, &byte| length << 8 | usize::from(byte));
        (length, &rest[count..])
    };
    (rest.len() >= length).then(|| (tag, &rest[..length], &rest[length..]))
}

const SEQUENCE: u8 = 0x30;
const OID: u8 = 0x06;
const INTEGER: u8 = 0x02;
const OCTET_STRING: u8 = 0x04;
const EXPLICIT_0: u8 = 0xa0;

/// The timestamp token in what a time-stamping authority answered, which may be the token
/// (`TimeStampToken`, a `ContentInfo`) or the whole response (`TimeStampResp`, RFC 3161),
/// checked to be a timestamp of `digest` (SHA-256).
pub(super) fn timestamp_token<'a>(answer: &'a [u8], digest: &[u8]) -> Result<&'a [u8], String> {
    let invalid = || "the timestamp is not an RFC 3161 timestamp token".to_owned();
    let (tag, content, rest) = element(answer).ok_or_else(invalid)?;
    if tag != SEQUENCE || !rest.is_empty() {
        return Err(invalid());
    }
    let (first, _, _) = element(content).ok_or_else(invalid)?;
    let token = if first == SEQUENCE {
        // TimeStampResp ::= SEQUENCE { status PKIStatusInfo, timeStampToken OPTIONAL }
        let (_, status, rest) = element(content).ok_or_else(invalid)?;
        let (tag, value, _) = element(status).ok_or_else(invalid)?;
        // granted (0) or grantedWithMods (1)
        if tag != INTEGER || !matches!(value, [0 | 1]) {
            return Err("the time-stamping authority refused the request".to_owned());
        }
        let (tag, _, after) = element(rest).ok_or_else(invalid)?;
        if tag != SEQUENCE || !after.is_empty() {
            return Err(invalid());
        }
        rest
    } else {
        answer
    };
    let imprint = message_imprint(token).ok_or_else(invalid)?;
    if imprint != digest {
        return Err("the timestamp is not one of this signature".to_owned());
    }
    Ok(token)
}

/// The SHA-256 message imprint of a timestamp token; `None` if it is no token or hashed with
/// another algorithm.
fn message_imprint(token: &[u8]) -> Option<&[u8]> {
    // ContentInfo ::= SEQUENCE { contentType, [0] EXPLICIT SignedData }
    let (_, content_info, _) = element(token)?;
    let (tag, content_type, rest) = element(content_info)?;
    if tag != OID || content_type != ID_SIGNED_DATA.as_bytes() {
        return None;
    }
    let (tag, explicit, _) = element(rest)?;
    if tag != EXPLICIT_0 {
        return None;
    }
    // SignedData ::= SEQUENCE { version, digestAlgorithms, encapContentInfo, … }
    let (_, signed_data, _) = element(explicit)?;
    let (_, _, rest) = element(signed_data)?;
    let (_, _, rest) = element(rest)?;
    let (_, encapsulated, _) = element(rest)?;
    let (tag, content_type, rest) = element(encapsulated)?;
    if tag != OID || content_type != ID_TST_INFO.as_bytes() {
        return None;
    }
    let (_, explicit, _) = element(rest)?;
    let (tag, tst_info, _) = element(explicit)?;
    if tag != OCTET_STRING {
        return None;
    }
    // TSTInfo ::= SEQUENCE { version, policy, messageImprint, … }
    let (_, tst_info, _) = element(tst_info)?;
    let (_, _, rest) = element(tst_info)?;
    let (_, _, rest) = element(rest)?;
    let (_, imprint, _) = element(rest)?;
    // MessageImprint ::= SEQUENCE { hashAlgorithm AlgorithmIdentifier, hashedMessage OCTET STRING }
    let (_, algorithm, rest) = element(imprint)?;
    let (tag, oid, _) = element(algorithm)?;
    if tag != OID || oid != ID_SHA256.as_bytes() {
        return None;
    }
    let (tag, hashed, _) = element(rest)?;
    (tag == OCTET_STRING).then_some(hashed)
}

/// A DER-encoded ECDSA signature (`ECDSA-Sig-Value`) of P-256, from DER or from the 64 bytes of
/// r and s that some key stores return.
pub(super) fn ecdsa_der(signature: &[u8]) -> Option<Vec<u8>> {
    if signature.len() == 64 {
        let signature = p256::ecdsa::Signature::from_slice(signature).ok()?;
        return Some(signature.to_der().as_bytes().to_vec());
    }
    let signature = p256::ecdsa::DerSignature::from_bytes(signature).ok()?;
    Some(signature.as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_elements() {
        assert_eq!(element(&[0x04, 2, 1, 2, 9]), Some((0x04, &[1, 2][..], &[9][..])));
        assert_eq!(element(&[0x04, 0x81, 1, 7]), Some((0x04, &[7][..], &[][..])));
        assert_eq!(element(&[0x04, 3, 1]), None);
    }

    #[test]
    fn refuses_what_is_no_timestamp() {
        assert!(timestamp_token(b"\x30\x03\x02\x01\x02", &[0; 32]).is_err());
        assert!(timestamp_token(b"not der", &[0; 32]).is_err());
        // A response that says `rejection` (2).
        let error = timestamp_token(b"\x30\x05\x30\x03\x02\x01\x02", &[0; 32]).expect_err("refused");
        assert!(error.contains("refused"), "{error}");
    }
}

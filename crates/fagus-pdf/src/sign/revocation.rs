//! The validation data of long-term signatures (`PAdES` B-LT): for each certificate of the
//! signature and of its timestamp, a request for its revocation status, which the caller answers
//! from the network with OCSP responses (RFC 6960) or certificate revocation lists (RFC 5280),
//! and what the answers are, for the document security store.

use cms::cert::CertificateChoices;
use cms::content_info::ContentInfo;
use cms::signed_data::SignedData;
use der::asn1::ObjectIdentifier;
use der::{Decode, Encode};
use sha1::{Digest as _, Sha1};
use x509_cert::Certificate;
use x509_cert::crl::CertificateList;
use x509_cert::ext::pkix::name::{DistributionPointName, GeneralName};
use x509_cert::ext::pkix::{AuthorityInfoAccessSyntax, CrlDistributionPoints};

use super::envelope::element;
use super::update::Validation;

/// `id-ad-ocsp`: the access method of an OCSP responder in the authority information access.
const ID_AD_OCSP: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.6.1.5.5.7.48.1");
/// `id-pkix-ocsp-basic`: the type of the usual OCSP response.
const ID_PKIX_OCSP_BASIC: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.6.1.5.5.7.48.1.1");

/// What the revocation status of a certificate is asked with: the certificate, the certificate
/// of its issuer, where to ask (the OCSP responders and CRL distribution points the certificate
/// names) and the OCSP request to send to a responder.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct RevocationRequest {
    /// The certificate (DER).
    pub certificate: Vec<u8>,
    /// The certificate of its issuer (DER).
    pub issuer: Vec<u8>,
    /// The URLs of the OCSP responders the certificate names, to POST
    /// [`RevocationRequest::ocsp_request`] to with the content type
    /// `application/ocsp-request`.
    pub ocsp_urls: Vec<String>,
    /// The URLs of the certificate revocation lists the certificate names, to GET.
    pub crl_urls: Vec<String>,
    /// The OCSP request (`OCSPRequest`, DER) for the certificate, with SHA-1 for its
    /// identifier, as RFC 6960 asks every responder to understand.
    pub ocsp_request: Vec<u8>,
}

/// Gets the revocation status of certificates, for `PAdES` B-LT, since Fagus never goes to the
/// network.
///
/// `revocation` gets a [`RevocationRequest`] for each certificate of the signature (and of its
/// timestamp) but the roots and returns what the OCSP responder or the CRL distribution point
/// answered: OCSP responses (`OCSPResponse`, DER) and certificate revocation lists (DER), any
/// number of each, and certificates that validating them needs (such as that of an OCSP
/// responder the response does not hold). Nothing, for a certificate whose status cannot be
/// had, leaves it out of the validation data.
pub trait Revocation: Send + Sync {
    fn revocation(&self, request: &RevocationRequest) -> Result<Vec<Vec<u8>>, String>;
}

impl<F> Revocation for F
where
    F: Fn(&RevocationRequest) -> Result<Vec<Vec<u8>>, String> + Send + Sync,
{
    fn revocation(&self, request: &RevocationRequest) -> Result<Vec<Vec<u8>>, String> {
        self(request)
    }
}

/// The certificates of a timestamp token.
pub(super) fn token_certificates(token: &[u8]) -> Vec<Certificate> {
    let Ok(content_info) = ContentInfo::from_der(token) else {
        return Vec::new();
    };
    let Ok(signed_data) = content_info.content.decode_as::<SignedData>() else {
        return Vec::new();
    };
    signed_data
        .certificates
        .map(|set| {
            set.0
                .iter()
                .filter_map(|choice| match choice {
                    CertificateChoices::Certificate(certificate) => Some(certificate.clone()),
                    CertificateChoices::Other(_) => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The requests for the revocation status of `certificates` (each once): of each that is not
/// self-signed and whose issuer is among them.
pub(super) fn requests(certificates: &[Certificate]) -> Result<Vec<RevocationRequest>, String> {
    let mut requests = Vec::new();
    for certificate in certificates {
        let tbs = certificate.tbs_certificate();
        if tbs.issuer() == tbs.subject() {
            continue;
        }
        let Some(issuer) = certificates
            .iter()
            .find(|candidate| candidate.tbs_certificate().subject() == tbs.issuer())
        else {
            continue;
        };
        let der = |certificate: &Certificate| certificate.to_der().map_err(|error| error.to_string());
        let (ocsp_urls, crl_urls) = urls(certificate);
        requests.push(RevocationRequest {
            certificate: der(certificate)?,
            issuer: der(issuer)?,
            ocsp_urls,
            crl_urls,
            ocsp_request: ocsp_request(certificate, issuer)?,
        });
    }
    Ok(requests)
}

/// The URLs of the OCSP responders and of the CRLs a certificate names.
fn urls(certificate: &Certificate) -> (Vec<String>, Vec<String>) {
    let uri = |name: &GeneralName| match name {
        GeneralName::UniformResourceIdentifier(uri) => Some(uri.to_string()),
        _ => None,
    };
    let ocsp = certificate
        .tbs_certificate()
        .get_extension::<AuthorityInfoAccessSyntax>()
        .ok()
        .flatten()
        .map(|(_, access)| {
            access
                .0
                .iter()
                .filter(|description| description.access_method == ID_AD_OCSP)
                .filter_map(|description| uri(&description.access_location))
                .collect()
        })
        .unwrap_or_default();
    let crl = certificate
        .tbs_certificate()
        .get_extension::<CrlDistributionPoints>()
        .ok()
        .flatten()
        .map(|(_, points)| {
            points
                .0
                .iter()
                .filter_map(|point| match &point.distribution_point {
                    Some(DistributionPointName::FullName(names)) => Some(names.iter().filter_map(uri)),
                    _ => None,
                })
                .flatten()
                .collect()
        })
        .unwrap_or_default();
    (ocsp, crl)
}

/// DER: an element of `tag` around `content`.
fn tlv(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    let length = content.len();
    if length < 0x80 {
        out.push(length as u8);
    } else {
        let bytes = length.to_be_bytes();
        let skip = bytes.iter().take_while(|&&byte| byte == 0).count();
        out.push(0x80 | (bytes.len() - skip) as u8);
        out.extend_from_slice(&bytes[skip..]);
    }
    out.extend_from_slice(content);
    out
}

/// The OCSP request for `certificate` (RFC 6960, §4.1.1), without extensions: one `CertID` with
/// SHA-1 hashes of the issuer's name and key.
fn ocsp_request(certificate: &Certificate, issuer: &Certificate) -> Result<Vec<u8>, String> {
    let error = |error: der::Error| error.to_string();
    let name = certificate.tbs_certificate().issuer().to_der().map_err(error)?;
    let key = issuer
        .tbs_certificate()
        .subject_public_key_info()
        .subject_public_key
        .raw_bytes();
    let serial = certificate
        .tbs_certificate()
        .serial_number()
        .to_der()
        .map_err(error)?;
    // AlgorithmIdentifier { id-sha1, NULL }
    let mut cert_id = vec![0x30, 0x09, 0x06, 0x05, 0x2b, 0x0e, 0x03, 0x02, 0x1a, 0x05, 0x00];
    cert_id.extend(tlv(0x04, &Sha1::digest(&name)));
    cert_id.extend(tlv(0x04, &Sha1::digest(key)));
    cert_id.extend(serial);
    // OCSPRequest { TBSRequest { requestList { Request { CertID } } } }
    Ok(tlv(
        0x30,
        &tlv(0x30, &tlv(0x30, &tlv(0x30, &tlv(0x30, &cert_id)))),
    ))
}

/// Sorts an answer of the caller into the validation data: an OCSP response, a CRL or a
/// certificate. Fails for anything else, and for an OCSP response that is not successful.
pub(super) fn sort(answer: Vec<u8>, validation: &mut Validation) -> Result<(), String> {
    let (tag, content, rest) = element(&answer)
        .filter(|(tag, _, rest)| *tag == 0x30 && rest.is_empty())
        .ok_or("the revocation data is neither an OCSP response nor a CRL (DER)")?;
    let _ = (tag, rest);
    // OCSPResponse ::= SEQUENCE { responseStatus ENUMERATED, responseBytes [0] EXPLICIT … }
    if let Some((0x0a, status, rest)) = element(content) {
        if status != [0] {
            return Err(format!(
                "the OCSP responder did not answer the request (status {})",
                status.first().copied().unwrap_or_default()
            ));
        }
        let basic = element(rest)
            .filter(|(tag, ..)| *tag == 0xa0)
            .and_then(|(_, bytes, _)| element(bytes))
            .and_then(|(_, bytes, _)| element(bytes))
            .is_some_and(|(tag, oid, _)| tag == 0x06 && oid == ID_PKIX_OCSP_BASIC.as_bytes());
        if !basic {
            return Err("the OCSP response is not a basic OCSP response".to_owned());
        }
        validation.ocsp_responses.push(answer);
        return Ok(());
    }
    if CertificateList::<x509_cert::certificate::Rfc5280>::from_der(&answer).is_ok() {
        validation.crls.push(answer);
        return Ok(());
    }
    if Certificate::from_der(&answer).is_ok() {
        if !validation.certificates.contains(&answer) {
            validation.certificates.push(answer);
        }
        return Ok(());
    }
    Err("the revocation data is neither an OCSP response nor a CRL (DER)".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_lengths() {
        assert_eq!(tlv(0x04, &[1, 2]), [0x04, 2, 1, 2]);
        let long = tlv(0x04, &[0; 200]);
        assert_eq!(&long[..3], [0x04, 0x81, 200]);
        let longer = tlv(0x04, &[0; 300]);
        assert_eq!(&longer[..4], [0x04, 0x82, 1, 44]);
    }

    #[test]
    fn refuses_what_is_not_validation_data() {
        let mut validation = Validation::default();
        assert!(sort(b"nothing".to_vec(), &mut validation).is_err());
        // An OCSP response of the status `unauthorized` (6).
        let error = sort(vec![0x30, 0x03, 0x0a, 0x01, 0x06], &mut validation).expect_err("refused");
        assert!(error.contains("status 6"), "{error}");
    }
}

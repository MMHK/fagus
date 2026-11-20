//! The render request of the worker protocol (`docs/protocol.md`) as Rust types, and the JSON it
//! is answered with.
//!
//! The command line tool (`fagus worker`) and the C ABI library (`fagus-ffi`) build documents from
//! the same request, so that the two cannot drift apart. What needs the worker's channel (asking
//! the caller for resources, signing, timestamping, revocation) stays with the caller: this crate
//! takes a [`ResourceLoader`] and leaves the signature to be applied afterwards.

pub mod date;
pub mod layout;

use std::path::PathBuf;
use std::sync::Arc;

use fagus::{Document, FacturXProfile, Limits, ResourceLoader};
use serde::Deserialize;

/// The profile of a Factur-X invoice by its name in the protocol and on the command line.
pub fn profile(name: &str) -> Option<FacturXProfile> {
    Some(match name {
        "minimum" => FacturXProfile::Minimum,
        "basicwl" => FacturXProfile::BasicWl,
        "basic" => FacturXProfile::Basic,
        "en16931" => FacturXProfile::En16931,
        "extended" => FacturXProfile::Extended,
        "xrechnung" => FacturXProfile::XRechnung,
        _ => return None,
    })
}

/// A render request (the header of `R`). Its flags are those of the protocol's JSON.
#[allow(clippy::struct_excessive_bools)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub id: u64,
    pub html: usize,
    /// Whether the markup is XHTML, parsed as XML.
    #[serde(default)]
    pub xhtml: bool,
    #[serde(default)]
    pub base: Option<PathBuf>,
    #[serde(default)]
    pub root: Option<PathBuf>,
    #[serde(default)]
    pub stylesheets: Vec<Stylesheet>,
    #[serde(default)]
    pub resources: Vec<Resource>,
    #[serde(default)]
    pub ask: bool,
    #[serde(default)]
    pub conformance: Option<String>,
    #[serde(default = "yes")]
    pub tagged: bool,
    #[serde(default)]
    pub accessible: bool,
    #[serde(default)]
    pub forms: bool,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    #[serde(default)]
    pub factur_x: Option<FacturX>,
    #[serde(default)]
    pub created: Option<String>,
    /// Whether to report where the elements with an `id` ended up.
    #[serde(default)]
    pub layout: bool,
    #[serde(default)]
    pub encryption: Option<Encryption>,
    #[serde(default)]
    pub signature: Option<SignatureRequest>,
    /// The memory budget in bytes (0 for none), if not the worker's.
    #[serde(default)]
    pub memory_budget: Option<u64>,
    /// The time limit in milliseconds (0 for none), if not the worker's.
    #[serde(default)]
    pub time_limit: Option<f64>,
}

/// The digital signature of a render request.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureRequest {
    /// The data block of a PKCS#12 file.
    #[serde(default)]
    pub pkcs12: Option<usize>,
    #[serde(default)]
    pub password: Option<String>,
    /// The data blocks of the certificates of a signer the caller asks (`S`).
    #[serde(default)]
    pub certificates: Vec<usize>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub contact_info: Option<String>,
    #[serde(default)]
    pub time: Option<String>,
    /// Whether the caller timestamps the signature (`T`).
    #[serde(default)]
    pub timestamp: bool,
    /// What a certification signature allows: `no-changes`, `fill-forms` or `annotate`.
    #[serde(default)]
    pub certify: Option<String>,
    /// The `id` of the element in whose box the signature shows.
    #[serde(default)]
    pub element: Option<String>,
    /// The area of a page in which the signature shows.
    #[serde(default)]
    pub area: Option<Area>,
    /// Whether the caller gets the revocation status of the certificates (`V`), for B-LT.
    #[serde(default)]
    pub revocation: bool,
    /// Whether an archive timestamp follows the validation data (B-LTA), asked for with `T`.
    #[serde(default)]
    pub archive_timestamp: bool,
}

/// An area of a page: the number of the page (from 1) and the area in points from its top left
/// corner.
#[derive(Deserialize)]
pub struct Area {
    pub page: usize,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// The default of fields that are on unless the caller turns them off.
fn yes() -> bool {
    true
}

#[derive(Deserialize)]
pub struct Stylesheet {
    pub name: String,
    pub blob: usize,
}

#[derive(Deserialize)]
pub struct Resource {
    pub url: String,
    pub blob: usize,
}

#[derive(Deserialize)]
pub struct Attachment {
    pub name: String,
    pub mime: String,
    #[serde(default)]
    pub relationship: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    pub blob: usize,
}

#[derive(Deserialize)]
pub struct FacturX {
    pub blob: usize,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Encryption {
    #[serde(default)]
    pub user_password: Option<String>,
    #[serde(default)]
    pub owner_password: Option<String>,
    /// Every permission if left out.
    #[serde(default)]
    pub permissions: Option<Vec<String>>,
}

/// The document of a render request, from the request and its data blocks.
///
/// `blobs` is `None` for a data block that was taken by an earlier call (a signature's, say): this
/// function takes each block it uses exactly once. A request that asks for resources needs a
/// `loader`; one with a `signature` is left for the caller to sign, because a signature may need
/// the caller's channel.
pub fn document(
    request: Request,
    blobs: &mut [Option<Vec<u8>>],
    limits: Limits,
    loader: Option<Arc<dyn ResourceLoader>>,
) -> Result<Document, String> {
    let mut take = |index: usize| -> Result<Vec<u8>, String> {
        blobs
            .get_mut(index)
            .and_then(Option::take)
            .ok_or_else(|| format!("data block {index} is missing"))
    };
    let html = String::from_utf8(take(request.html)?).map_err(|_| "the HTML is not UTF-8".to_owned())?;
    let mut document = if request.xhtml {
        Document::xhtml(html)
    } else {
        Document::html(html)
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
        let loader = loader.ok_or_else(|| {
            "the request asks for resources, which only the worker's caller can supply".to_owned()
        })?;
        document = document.loader(loader);
    }
    if let Some(level) = request.conformance {
        let level = fagus::Conformance::from_level(&level)
            .ok_or_else(|| format!("unknown PDF/A level `{level}`"))?;
        document = document.conformance(level);
    }
    document = document.tagged(request.tagged).forms(request.forms);
    if request.accessible {
        document = document.accessible();
    }
    for attachment in request.attachments {
        // PDF/A-3 and PDF/UA need a description: the file name, as the command line gives.
        let description = attachment
            .description
            .filter(|description| !description.is_empty())
            .unwrap_or_else(|| attachment.name.clone());
        let relationship = match attachment.relationship.as_deref().unwrap_or("unspecified") {
            "source" => fagus::Relationship::Source,
            "data" => fagus::Relationship::Data,
            "alternative" => fagus::Relationship::Alternative,
            "supplement" => fagus::Relationship::Supplement,
            "unspecified" => fagus::Relationship::Unspecified,
            other => return Err(format!("unknown relationship `{other}`")),
        };
        document = document.attach(
            fagus::Attachment::new(attachment.name, attachment.mime, take(attachment.blob)?)
                .description(description)
                .relationship(relationship),
        );
    }
    if let Some(invoice) = request.factur_x {
        let xml = take(invoice.blob)?;
        document = match invoice.profile.as_deref() {
            None => document.factur_x(xml),
            Some(name) => document.factur_x_profile(
                xml,
                profile(name).ok_or_else(|| format!("unknown profile `{name}`"))?,
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
        document =
            document.created(date::parse(&created).ok_or_else(|| format!("invalid date `{created}`"))?);
    }
    if request.layout {
        document = document.report_layout(true);
    }
    if request.memory_budget.is_some() || request.time_limit.is_some() {
        let mut limits = limits;
        if let Some(bytes) = request.memory_budget {
            limits = limits.memory((bytes > 0).then_some(bytes));
        }
        if let Some(milliseconds) = request.time_limit {
            let limit = std::time::Duration::try_from_secs_f64(milliseconds / 1000.0)
                .map_err(|_| format!("invalid time limit `{milliseconds}`"))?;
            limits = limits.time((!limit.is_zero()).then_some(limit));
        }
        document = document.limits(limits);
    }
    if let Some(encryption) = request.encryption {
        let mut protection = fagus::Encryption::new();
        if let Some(password) = encryption.user_password {
            protection = protection.user_password(password);
        }
        if let Some(password) = encryption.owner_password {
            protection = protection.owner_password(password);
        }
        if let Some(names) = encryption.permissions {
            let permissions = names
                .iter()
                .map(|name| {
                    fagus::Permission::from_name(name)
                        .ok_or_else(|| format!("unknown permission `{name}`"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            protection = protection.permissions(permissions);
        }
        document = document.encrypt(protection);
    }
    Ok(document)
}

/// A diagnostic as the protocol's JSON has it.
#[derive(serde::Serialize)]
pub struct DiagnosticJson<'a> {
    severity: &'static str,
    code: &'a str,
    message: &'a str,
    source: &'a str,
    line: u32,
    column: u32,
}

/// The diagnostics as the protocol's JSON has them.
pub fn diagnostics(list: &[fagus::Diagnostic]) -> Vec<DiagnosticJson<'_>> {
    list.iter()
        .map(|diagnostic| DiagnosticJson {
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

/// What a render answers with besides its PDF: its pages, diagnostics, timings and, if it asked
/// for one, its layout. The worker adds `id`, `encrypted`, `signed`, `forms` and the memory it
/// used.
pub fn output_json(output: &fagus::Output) -> serde_json::Value {
    let mut header = serde_json::json!({
        "pages": output.pages,
        "diagnostics": diagnostics(&output.diagnostics),
        "timings": {
            "parse": output.timings.parse.as_secs_f64() * 1000.0,
            "style": output.timings.style.as_secs_f64() * 1000.0,
            "layout": output.timings.layout.as_secs_f64() * 1000.0,
            "paint": output.timings.paint.as_secs_f64() * 1000.0,
            "pdf": output.timings.pdf.as_secs_f64() * 1000.0,
        },
    });
    if let Some(layout) = &output.layout {
        header["layout"] = layout::layout_json(layout);
    }
    header
}

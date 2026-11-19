//! The `fagus` command line tool.

mod date;
mod layout_json;
mod memory;
mod worker;

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "fagus", version, about = "HTML and CSS to PDF, without a browser")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant, reason = "parsed once per run")]
enum Command {
    /// Renders an HTML file to PDF.
    Render {
        /// The HTML file.
        input: PathBuf,
        /// The PDF file to write.
        #[arg(short, long)]
        output: PathBuf,
        /// Directories with font files (repeatable).
        #[arg(long = "font-dir")]
        font_dirs: Vec<PathBuf>,
        /// Use the fonts installed on the system too, as Chrome does (the output then depends
        /// on the machine).
        #[arg(long)]
        system_fonts: bool,
        /// The language of the texts form controls show of their own (button labels, date
        /// formats) where the document gives them no `lang`, e.g. `de-DE` (default: en-US).
        /// Text without `lang` is then shaped in it, as Chrome shapes it in the language of its
        /// user interface.
        #[arg(long)]
        locale: Option<String>,
        /// Family used for `sans-serif`.
        #[arg(long)]
        sans_serif: Option<String>,
        /// Family used for `serif`.
        #[arg(long)]
        serif: Option<String>,
        /// Family used for `monospace`.
        #[arg(long)]
        monospace: Option<String>,
        /// Directory that the document may read files from (default: the input's directory).
        #[arg(long)]
        root: Option<PathBuf>,
        /// Write PDF/A: 1b, 2b, 2u, 2a, 3b, 3u, 3a, 4, 4e or 4f.
        #[arg(long, value_parser = ["1b", "2b", "2u", "2a", "3b", "3u", "3a", "4", "4e", "4f"])]
        pdfa: Option<String>,
        /// Write a PDF without tags (the logical structure that screen readers read).
        #[arg(long)]
        untagged: bool,
        /// Write PDF/UA-1, the standard for accessible PDF (tagged; needs a title, a language and
        /// alternative text for images).
        #[arg(long)]
        accessible: bool,
        /// Attach a file (repeatable).
        #[arg(long = "attach")]
        attachments: Vec<PathBuf>,
        /// Make an electronic invoice (ZUGFeRD 2 / Factur-X) with this XML.
        #[arg(long)]
        factur_x: Option<PathBuf>,
        /// The profile of the invoice XML (default: the one the XML declares).
        #[arg(long, value_parser = ["minimum", "basicwl", "basic", "en16931", "extended", "xrechnung"])]
        profile: Option<String>,
        /// The `Version` of the Factur-X XMP metadata (default: 1.0).
        #[arg(long)]
        factur_x_version: Option<String>,
        /// The description of the attached invoice XML (default: "Factur-X/ZUGFeRD invoice").
        #[arg(long)]
        factur_x_description: Option<String>,
        /// Fail on any diagnostic (CSS that is ignored, a missing resource, …), without writing
        /// the PDF.
        #[arg(long)]
        strict: bool,
        /// Write where the elements with an `id` ended up (their pages and boxes, and the rows of
        /// their tables) to this file as JSON.
        #[arg(long)]
        layout_json: Option<PathBuf>,
        /// Print the time of each phase.
        #[arg(long)]
        timings: bool,
        /// Render this many times (for benchmarks) and report the median.
        #[arg(long, default_value_t = 1)]
        repeat: usize,
        /// The creation date written into the PDF, e.g. `2026-11-13T12:00:00Z` or
        /// `2026-11-13T14:00:00+02:00` (default: now), so that the same input gives the same file.
        #[arg(long)]
        created: Option<String>,
        #[command(flatten)]
        encryption: EncryptionArgs,
    },
    /// Renders documents for another program over standard input and output (docs/protocol.md).
    Worker {
        /// Directories with font files (repeatable).
        #[arg(long = "font-dir")]
        font_dirs: Vec<PathBuf>,
        /// Use the fonts installed on the system too, as Chrome does (the output then depends
        /// on the machine).
        #[arg(long)]
        system_fonts: bool,
        /// The language of the texts form controls show of their own (button labels, date
        /// formats) where the document gives them no `lang`, e.g. `de-DE` (default: en-US).
        /// Text without `lang` is then shaped in it, as Chrome shapes it in the language of its
        /// user interface.
        #[arg(long)]
        locale: Option<String>,
        /// Family used for `sans-serif`.
        #[arg(long)]
        sans_serif: Option<String>,
        /// Family used for `serif`.
        #[arg(long)]
        serif: Option<String>,
        /// Family used for `monospace`.
        #[arg(long)]
        monospace: Option<String>,
    },
}

/// Password protection. Arguments of a process are visible to other users of the machine, so the
/// passwords can come from files or the environment too.
#[derive(clap::Args)]
struct EncryptionArgs {
    /// Encrypt the PDF (AES-256) with this password, needed to open it [default: the
    /// environment variable `FAGUS_USER_PASSWORD`].
    #[arg(long)]
    user_password: Option<String>,
    /// Read the user password from this file (its first line).
    #[arg(long, value_name = "FILE", conflicts_with = "user_password")]
    user_password_file: Option<PathBuf>,
    /// Encrypt the PDF (AES-256) with this password, which lifts the restrictions of
    /// `--permissions` [default: the environment variable `FAGUS_OWNER_PASSWORD`].
    #[arg(long)]
    owner_password: Option<String>,
    /// Read the owner password from this file (its first line).
    #[arg(long, value_name = "FILE", conflicts_with = "owner_password")]
    owner_password_file: Option<PathBuf>,
    /// What an encrypted PDF allows without the owner password, separated by commas: print,
    /// print-low-quality, modify, copy, annotate, fill-forms, assemble, or none (default:
    /// everything).
    #[arg(
        long,
        value_delimiter = ',',
        value_parser = ["print", "print-low-quality", "modify", "copy", "annotate", "fill-forms", "assemble", "none"]
    )]
    permissions: Option<Vec<String>>,
}

impl EncryptionArgs {
    /// The encryption the arguments ask for, if any, with the passwords of the environment
    /// variables where the command line gives none.
    fn encryption(self) -> Result<Option<fagus::Encryption>> {
        self.encryption_with(|name| std::env::var(name).ok())
    }

    fn encryption_with(
        self,
        environment: impl Fn(&str) -> Option<String>,
    ) -> Result<Option<fagus::Encryption>> {
        let read = |path: Option<PathBuf>| -> Result<Option<String>> {
            let Some(path) = path else { return Ok(None) };
            let text =
                std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
            Ok(Some(text.lines().next().unwrap_or_default().to_owned()))
        };
        let user = read(self.user_password_file)?
            .or(self.user_password)
            .or_else(|| environment("FAGUS_USER_PASSWORD"))
            .filter(|password| !password.is_empty());
        let owner = read(self.owner_password_file)?
            .or(self.owner_password)
            .or_else(|| environment("FAGUS_OWNER_PASSWORD"))
            .filter(|password| !password.is_empty());
        if user.is_none() && owner.is_none() {
            if self.permissions.is_some() {
                bail!("--permissions needs --user-password or --owner-password");
            }
            return Ok(None);
        }
        let mut encryption = fagus::Encryption::new();
        if let Some(user) = user {
            encryption = encryption.user_password(user);
        }
        if let Some(owner) = owner {
            encryption = encryption.owner_password(owner);
        }
        if let Some(names) = self.permissions {
            if names.len() > 1 && names.iter().any(|name| name == "none") {
                bail!("--permissions none cannot go with other permissions");
            }
            encryption =
                encryption.permissions(names.iter().filter_map(|name| fagus::Permission::from_name(name)));
        }
        Ok(Some(encryption))
    }
}

/// The message of `--strict` when the document has `count` diagnostics.
fn strict_failure(count: usize) -> String {
    let noun = if count == 1 { "diagnostic" } else { "diagnostics" };
    format!("{count} {noun} in strict mode")
}

/// An invoice profile by its name on the command line and in the protocol.
fn profile(name: &str) -> Option<fagus::FacturXProfile> {
    Some(match name {
        "minimum" => fagus::FacturXProfile::Minimum,
        "basicwl" => fagus::FacturXProfile::BasicWl,
        "basic" => fagus::FacturXProfile::Basic,
        "en16931" => fagus::FacturXProfile::En16931,
        "extended" => fagus::FacturXProfile::Extended,
        "xrechnung" => fagus::FacturXProfile::XRechnung,
        _ => return None,
    })
}

/// A MIME type from a file name's extension.
fn mime_type(name: &str) -> &'static str {
    match name.rsplit('.').next().map(str::to_ascii_lowercase).as_deref() {
        Some("xml") => "text/xml",
        Some("pdf") => "application/pdf",
        Some("csv") => "text/csv",
        Some("json") => "application/json",
        Some("txt") => "text/plain",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        _ => "application/octet-stream",
    }
}

/// How many threads compress the PDF while it is written, unless `RAYON_NUM_THREADS` says
/// otherwise. More than one gains a few percent on large documents (the page contents are
/// compressed while the next pages are drawn); more than two gain nothing, but cost processor
/// time and threads, which add up in many workers side by side.
#[cfg(feature = "parallel")]
const PDF_THREADS: usize = 2;

fn main() -> Result<()> {
    #[cfg(feature = "parallel")]
    if std::env::var_os("RAYON_NUM_THREADS").is_none() {
        // Fails only where the pool exists already, which then serves as it is.
        let _ = rayon::ThreadPoolBuilder::new()
            .num_threads(PDF_THREADS)
            .build_global();
    }
    match Cli::parse().command {
        Command::Render {
            input,
            output,
            font_dirs,
            system_fonts,
            locale,
            sans_serif,
            serif,
            monospace,
            root,
            pdfa,
            untagged,
            accessible,
            attachments,
            factur_x,
            profile,
            factur_x_version,
            factur_x_description,
            strict,
            layout_json,
            timings,
            repeat,
            created,
            encryption,
        } => {
            let html =
                std::fs::read_to_string(&input).with_context(|| format!("reading {}", input.display()))?;
            let mut engine = fagus::Engine::new();
            for directory in &font_dirs {
                engine
                    .add_font_dir(directory)
                    .with_context(|| format!("reading fonts in {}", directory.display()))?;
            }
            if system_fonts {
                engine.add_system_fonts();
            }
            engine.set_locale(locale);
            if sans_serif.is_some() || serif.is_some() || monospace.is_some() {
                engine.set_generic_families(sans_serif, serif, monospace);
            }
            // A bare file name has an empty parent: its directory is the current one.
            let base = input
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .map_or_else(|| PathBuf::from("."), PathBuf::from);
            // As browsers do for files, the extension says whether the markup is XHTML.
            let xhtml = input
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    extension.eq_ignore_ascii_case("xhtml") || extension.eq_ignore_ascii_case("xht")
                });
            let document = if xhtml {
                fagus::Document::xhtml(html)
            } else {
                fagus::Document::html(html)
            };
            let mut document = document.base_directory(base);
            if let Some(root) = root {
                document = document.root(root);
            }
            if let Some(created) = created {
                let Some(date) = worker::date(&created) else {
                    bail!(
                        "invalid date `{created}`, expected e.g. 2026-11-13T12:00:00Z or 2026-11-13T14:00:00+02:00"
                    );
                };
                document = document.created(date);
            }
            if let Some(level) = pdfa.as_deref().and_then(fagus::Conformance::from_level) {
                document = document.conformance(level);
            }
            document = document.tagged(!untagged);
            if accessible {
                document = document.accessible();
            }
            for path in &attachments {
                let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
                let name = path.file_name().map_or_else(
                    || "attachment".to_owned(),
                    |name| name.to_string_lossy().into_owned(),
                );
                document = document.attach(fagus::Attachment {
                    mime_type: mime_type(&name).to_owned(),
                    description: Some(name.clone()),
                    name,
                    relationship: fagus::Relationship::Supplement,
                    data: std::sync::Arc::new(data),
                });
            }
            if let Some(path) = factur_x {
                let xml = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
                document = match profile.as_deref().and_then(self::profile) {
                    None => document.factur_x(xml),
                    Some(profile) => document.factur_x_profile(xml, profile),
                };
                if let Some(version) = factur_x_version {
                    document = document.factur_x_version(version);
                }
                if let Some(description) = factur_x_description {
                    document = document.factur_x_description(description);
                }
            }
            if layout_json.is_some() {
                document = document.report_layout(true);
            }
            if let Some(encryption) = encryption.encryption()? {
                document = document.encrypt(encryption);
            }
            let mut durations = Vec::with_capacity(repeat);
            let mut result = None;
            for _ in 0..repeat.max(1) {
                let start = std::time::Instant::now();
                match engine.render_with_diagnostics(&document) {
                    Ok(rendered) => result = Some(rendered),
                    Err(failure) => {
                        // The diagnostics found before the failure often explain it.
                        for diagnostic in &failure.diagnostics {
                            eprintln!("{diagnostic}");
                        }
                        return Err(failure.error.into());
                    }
                }
                durations.push(start.elapsed());
            }
            let rendered = result.expect("rendered at least once");
            for diagnostic in &rendered.diagnostics {
                eprintln!("{diagnostic}");
            }
            if strict && !rendered.diagnostics.is_empty() {
                bail!("{}", strict_failure(rendered.diagnostics.len()));
            }
            std::fs::write(&output, &rendered.pdf)
                .with_context(|| format!("writing {}", output.display()))?;
            if let (Some(path), Some(layout)) = (&layout_json, &rendered.layout) {
                let mut json = serde_json::to_string_pretty(&layout_json::layout_json(layout))?;
                json.push('\n');
                std::fs::write(path, json).with_context(|| format!("writing {}", path.display()))?;
            }
            if timings {
                durations.sort();
                let t = rendered.timings;
                eprintln!(
                    "{} pages, median {:.2} ms (parse {:.2}, style {:.2}, layout {:.2}, paint {:.2}, pdf {:.2})",
                    rendered.pages,
                    durations[durations.len() / 2].as_secs_f64() * 1000.0,
                    t.parse.as_secs_f64() * 1000.0,
                    t.style.as_secs_f64() * 1000.0,
                    t.layout.as_secs_f64() * 1000.0,
                    t.paint.as_secs_f64() * 1000.0,
                    t.pdf.as_secs_f64() * 1000.0,
                );
            }
            Ok(())
        }
        Command::Worker {
            font_dirs,
            system_fonts,
            locale,
            sans_serif,
            serif,
            monospace,
        } => {
            let mut engine = fagus::Engine::new();
            for directory in &font_dirs {
                engine
                    .add_font_dir(directory)
                    .with_context(|| format!("reading fonts in {}", directory.display()))?;
            }
            if system_fonts {
                engine.add_system_fonts();
            }
            engine.set_locale(locale);
            if sans_serif.is_some() || serif.is_some() || monospace.is_some() {
                engine.set_generic_families(sans_serif, serif, monospace);
            }
            let output = std::io::BufWriter::new(std::io::stdout());
            worker::serve(engine, std::io::stdin(), output).context("serving the caller")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_diagnostics_in_strict_mode() {
        assert_eq!(strict_failure(1), "1 diagnostic in strict mode");
        assert_eq!(strict_failure(2), "2 diagnostics in strict mode");
    }

    #[test]
    fn says_what_strict_mode_fails_on() {
        use clap::CommandFactory;
        let command = Cli::command();
        let render = command.find_subcommand("render").expect("render");
        let strict = render
            .get_arguments()
            .find(|argument| argument.get_id() == "strict")
            .and_then(|argument| argument.get_help())
            .expect("help")
            .to_string();
        assert!(strict.starts_with("Fail on any diagnostic"), "{strict}");
    }

    #[test]
    fn takes_the_version_and_description_of_invoices() {
        let cli = Cli::try_parse_from([
            "fagus",
            "render",
            "invoice.html",
            "-o",
            "invoice.pdf",
            "--factur-x",
            "xrechnung.xml",
            "--factur-x-version",
            "3.0",
            "--factur-x-description",
            "XRechnung Invoice",
        ])
        .expect("parsed");
        let Command::Render {
            factur_x_version,
            factur_x_description,
            ..
        } = cli.command
        else {
            panic!("not render");
        };
        assert_eq!(factur_x_version.as_deref(), Some("3.0"));
        assert_eq!(factur_x_description.as_deref(), Some("XRechnung Invoice"));
    }

    fn encryption(arguments: &[&str]) -> Result<Option<fagus::Encryption>> {
        encryption_in(arguments, &[])
    }

    fn encryption_in(arguments: &[&str], environment: &[(&str, &str)]) -> Result<Option<fagus::Encryption>> {
        let cli = Cli::try_parse_from(
            ["fagus", "render", "a.html", "-o", "a.pdf"]
                .iter()
                .chain(arguments),
        )?;
        let Command::Render { encryption, .. } = cli.command else {
            panic!("not render");
        };
        encryption.encryption_with(|name| {
            environment
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        })
    }

    #[test]
    fn takes_passwords_from_arguments_and_files() {
        let file = std::env::temp_dir().join(format!("fagus-password-{}", std::process::id()));
        std::fs::write(&file, "aus der Datei\r\nnext line\n").expect("written");
        let path = file.to_str().expect("a path");
        let debug = |arguments: &[&str]| format!("{:?}", encryption(arguments).expect("parsed"));
        assert_eq!(debug(&[]), "None");
        assert!(debug(&["--user-password", "geheim"]).contains("user_password: Some"));
        assert!(debug(&["--owner-password-file", path]).contains("owner_password: Some"));
        assert!(
            debug(&["--owner-password", "x", "--permissions", "print,copy"])
                .contains("permissions: Some([Print, Copy])")
        );
        assert!(debug(&["--owner-password", "x", "--permissions", "none"]).contains("permissions: Some([])"));
        // An empty password is none.
        assert_eq!(debug(&["--user-password", ""]), "None");
        assert!(encryption(&["--permissions", "print"]).is_err());
        assert!(encryption(&["--user-password", "x", "--permissions", "none,print"]).is_err());
        assert!(encryption(&["--user-password", "x", "--user-password-file", path]).is_err());
        assert!(encryption(&["--user-password", "x", "--permissions", "everything"]).is_err());
        assert!(encryption(&["--user-password-file", "/no/such/file"]).is_err());
        // The environment stands in for options that are not given.
        let from = |arguments: &[&str], environment: &[(&str, &str)]| {
            format!("{:?}", encryption_in(arguments, environment).expect("parsed"))
        };
        assert!(from(&[], &[("FAGUS_OWNER_PASSWORD", "o")]).contains("owner_password: Some"));
        assert!(
            from(&["--user-password-file", path], &[("FAGUS_USER_PASSWORD", "u")])
                .contains("user_password: Some")
        );
        assert_eq!(from(&[], &[("FAGUS_USER_PASSWORD", "")]), "None");
        std::fs::remove_file(&file).expect("removed");
    }

    #[test]
    fn writes_the_layout_as_json() {
        let cli = Cli::try_parse_from([
            "fagus",
            "render",
            "a.html",
            "-o",
            "a.pdf",
            "--layout-json",
            "a.json",
        ])
        .expect("parsed");
        let Command::Render { layout_json, .. } = cli.command else {
            panic!("not render");
        };
        assert_eq!(layout_json, Some(PathBuf::from("a.json")));
        let mut engine = fagus::Engine::new();
        engine
            .add_font_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fonts"))
            .expect("fonts");
        let output = engine
            .render(
                &fagus::Document::html(
                    "<!DOCTYPE html><style>@page { size: 400px 300px; margin: 20px } body { margin: 0 }
                     td, th { padding: 0 } table { border-spacing: 0 }</style>
                     <table id=t><thead><tr><th style='height: 20px'>A</th></tr></thead>
                     <tbody><tr id=r><td style='height: 30px'>1</td></tr></tbody></table>",
                )
                .report_layout(true),
            )
            .expect("rendered");
        let json = layout_json::layout_json(&output.layout.expect("layout"));
        let box_of = |page: u32, y: f64, height: f64| serde_json::json!({ "page": page, "x": 15.0, "y": y, "width": json["elements"][0]["boxes"][0]["width"], "height": height });
        assert_eq!(
            json["pages"],
            serde_json::json!([{ "width": 300.0, "height": 225.0 }])
        );
        assert_eq!(json["elements"][0]["id"], "t");
        assert_eq!(json["elements"][0]["tag"], "table");
        assert_eq!(
            json["elements"][0]["boxes"],
            serde_json::json!([box_of(1, 15.0, 37.5)])
        );
        assert_eq!(
            json["elements"][0]["rows"],
            serde_json::json!([
                { "section": "header", "index": 0, "boxes": [box_of(1, 15.0, 15.0)] },
                { "section": "body", "index": 0, "id": "r", "boxes": [box_of(1, 30.0, 22.5)] },
            ])
        );
        assert_eq!(
            json["elements"][1]["boxes"],
            serde_json::json!([box_of(1, 30.0, 22.5)])
        );
        assert!(json["elements"][1].get("rows").is_none());
    }

    #[test]
    fn renders_with_every_generic_family() {
        let cli = Cli::try_parse_from([
            "fagus",
            "render",
            "letter.html",
            "-o",
            "letter.pdf",
            "--sans-serif",
            "Inter",
            "--serif",
            "Source Serif 4",
            "--monospace",
            "JetBrains Mono",
        ])
        .expect("parsed");
        let Command::Render {
            sans_serif,
            serif,
            monospace,
            ..
        } = cli.command
        else {
            panic!("not render");
        };
        assert_eq!(sans_serif.as_deref(), Some("Inter"));
        assert_eq!(serif.as_deref(), Some("Source Serif 4"));
        assert_eq!(monospace.as_deref(), Some("JetBrains Mono"));
    }
}

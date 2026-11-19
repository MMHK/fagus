//! The corpus against Chrome: every document of `tests/corpus` printed by Fagus and by Chrome,
//! both PDFs drawn by the same rasterizer and compared with a tolerance for antialiasing and
//! glyphs a pixel apart.
//!
//! Documents that use print features Chrome does not have (running elements, named strings,
//! cross references, leaders, footnotes; see [`chrome_lacks`]) are not compared: Chrome prints
//! them differently by design.
//!
//! Fagus renders as `fagus render --font-dir tests/fonts --root tests --system-fonts` would:
//! Chrome uses the fonts installed on the machine, so Fagus may use them too. A document matches
//! when both have the same number of pages and on no page more than a share of the pixels
//! differs (see [`crate::raster::tolerant_share`]).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crate::process;
use crate::raster::{Rasterizer, tolerant_share};

/// How the comparison is made.
#[derive(Clone, Debug)]
pub struct Options {
    /// `tests` of the repository: the corpus is in `corpus`, the fonts in `fonts`.
    pub tests: PathBuf,
    pub chrome: PathBuf,
    /// More arguments for Chrome (such as `--no-sandbox` on CI runners).
    pub chrome_args: Vec<String>,
    /// Where the PDFs and the results go.
    pub out: PathBuf,
    pub jobs: usize,
    /// Fagus uses the fonts installed on the machine, as Chrome does.
    pub system_fonts: bool,
    /// Chrome's PDFs from an earlier run are used again where they exist.
    pub reuse_chrome: bool,
    /// Dots per inch of the comparison.
    pub dpi: u32,
    /// How far a channel may lie outside its neighborhood before a pixel counts as different.
    pub threshold: u8,
    /// The share of differing pixels a page may have.
    pub page_share: f64,
    /// Only documents whose path contains this.
    pub filter: Option<String>,
}

/// The comparison of one document.
#[derive(Clone, Debug)]
pub struct Comparison {
    /// The path in the corpus.
    pub document: String,
    pub chrome_pages: usize,
    pub fagus_pages: usize,
    /// The share of differing pixels of each page both have.
    pub shares: Vec<f64>,
    /// Why the document could not be compared.
    pub error: Option<String>,
    /// The print feature Chrome lacks that the document uses; it is then not compared.
    pub skipped: Option<&'static str>,
}

impl Comparison {
    pub fn matches(&self, page_share: f64) -> bool {
        self.skipped.is_none()
            && self.error.is_none()
            && self.chrome_pages == self.fagus_pages
            && self.shares.iter().all(|&share| share <= page_share)
    }

    /// The largest share of differing pixels of a page.
    pub fn worst(&self) -> f64 {
        self.shares.iter().copied().fold(0.0, f64::max)
    }
}

/// Whether a file name ends in `.html`.
fn is_html(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("html"))
}

/// The documents of the corpus: its HTML files and those one directory down, sorted.
pub fn documents(corpus: &Path) -> std::io::Result<Vec<String>> {
    let mut documents = Vec::new();
    for entry in std::fs::read_dir(corpus)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type()?.is_dir() {
            for inner in std::fs::read_dir(entry.path())? {
                let inner = inner?.file_name().to_string_lossy().into_owned();
                if is_html(&inner) {
                    documents.push(format!("{name}/{inner}"));
                }
            }
        } else if is_html(&name) {
            documents.push(name);
        }
    }
    documents.sort();
    Ok(documents)
}

/// The print feature of CSS Generated Content for Paged Media that `html` uses and Chrome does
/// not support, if any.
pub fn chrome_lacks(html: &str) -> Option<&'static str> {
    let lower = html.to_ascii_lowercase();
    [
        ("running(", "running elements"),
        ("element(", "running elements"),
        ("string-set", "named strings"),
        ("target-counter", "cross references"),
        ("target-text", "cross references"),
        ("leader(", "leaders"),
        ("float: footnote", "footnotes"),
        ("::footnote", "footnotes"),
    ]
    .into_iter()
    .find(|(needle, _)| lower.contains(needle))
    .map(|(_, feature)| feature)
}

/// The URL of a file for Chrome.
fn file_url(path: &Path) -> String {
    let mut url = String::from("file://");
    for character in path.to_string_lossy().chars() {
        match character {
            ' ' => url.push_str("%20"),
            '#' => url.push_str("%23"),
            '%' => url.push_str("%25"),
            '?' => url.push_str("%3F"),
            character => url.push(character),
        }
    }
    url
}

/// Compares the corpus; returns one comparison per document.
pub fn compare(options: &Options) -> anyhow::Result<Vec<Comparison>> {
    let tests = options.tests.canonicalize()?;
    let corpus = tests.join("corpus");
    let out = &options.out;
    for directory in ["chrome", "fagus", "tmp"] {
        std::fs::create_dir_all(out.join(directory))?;
    }
    let out = out.canonicalize()?;
    let mut engine = fagus::Engine::new();
    engine.add_font_dir(tests.join("fonts"))?;
    if options.system_fonts {
        engine.add_system_fonts();
    }
    let rasterizer = Rasterizer::new(out.join("tmp"))?;
    rasterizer.version().map_err(anyhow::Error::msg)?;
    let documents: Vec<String> = documents(&corpus)?
        .into_iter()
        .filter(|document| {
            options
                .filter
                .as_ref()
                .is_none_or(|filter| document.contains(filter))
        })
        .collect();
    let profiles = AtomicUsize::new(0);
    let total = documents.len();
    let comparisons = process::map(
        &documents,
        options.jobs,
        |document| {
            let name = document.trim_end_matches(".html").replace('/', "_");
            let source = corpus.join(document);
            let skipped = std::fs::read_to_string(&source)
                .ok()
                .and_then(|html| chrome_lacks(&html));
            if skipped.is_some() {
                return Comparison {
                    document: document.clone(),
                    chrome_pages: 0,
                    fagus_pages: 0,
                    shares: Vec::new(),
                    error: None,
                    skipped,
                };
            }
            let chrome = out.join("chrome").join(format!("{name}.pdf"));
            let fagus = out.join("fagus").join(format!("{name}.pdf"));
            let profile = out
                .join("tmp")
                .join(format!("profile-{}", profiles.fetch_add(1, Ordering::Relaxed)));
            let result = print_with_chrome(options, &source, &chrome, &profile)
                .and_then(|()| render_with_fagus(&engine, &tests, &source, &fagus))
                .and_then(|()| compare_pdfs(options, &rasterizer, &chrome, &fagus));
            let _ = std::fs::remove_dir_all(&profile);
            match result {
                Ok((chrome_pages, fagus_pages, shares)) => Comparison {
                    document: document.clone(),
                    chrome_pages,
                    fagus_pages,
                    shares,
                    error: None,
                    skipped: None,
                },
                Err(error) => Comparison {
                    document: document.clone(),
                    chrome_pages: 0,
                    fagus_pages: 0,
                    shares: Vec::new(),
                    error: Some(error),
                    skipped: None,
                },
            }
        },
        |done| {
            if done % 25 == 0 || done == total {
                eprintln!("{done} / {total} documents");
            }
        },
    );
    Ok(comparisons)
}

fn print_with_chrome(options: &Options, source: &Path, pdf: &Path, profile: &Path) -> Result<(), String> {
    if options.reuse_chrome && pdf.exists() {
        return Ok(());
    }
    let _ = std::fs::remove_file(pdf);
    let status = process::run(
        Command::new(&options.chrome)
            .args([
                "--headless",
                "--disable-gpu",
                "--no-pdf-header-footer",
                "--no-first-run",
                "--no-default-browser-check",
                "--allow-file-access-from-files",
            ])
            .arg(format!("--user-data-dir={}", profile.display()))
            .arg(format!("--print-to-pdf={}", pdf.display()))
            .args(&options.chrome_args)
            .arg(file_url(source)),
        Duration::from_secs(180),
    )
    .map_err(|error| format!("cannot run Chrome: {error}"))?;
    match status {
        Some(_) if pdf.exists() => Ok(()),
        Some(status) => Err(format!("Chrome wrote no PDF ({status})")),
        None => Err("Chrome took too long".to_owned()),
    }
}

fn render_with_fagus(engine: &fagus::Engine, tests: &Path, source: &Path, pdf: &Path) -> Result<(), String> {
    let html = std::fs::read_to_string(source).map_err(|error| error.to_string())?;
    let document = fagus::Document::html(html)
        .base_directory(source.parent().unwrap_or(tests))
        .root(tests)
        .created(fagus::Date::from_unix(0));
    let output = engine
        .render(&document)
        .map_err(|error| format!("Fagus: {error}"))?;
    std::fs::write(pdf, output.pdf).map_err(|error| error.to_string())
}

/// The page counts of both PDFs and the share of differing pixels of each page.
fn compare_pdfs(
    options: &Options,
    rasterizer: &Rasterizer,
    chrome: &Path,
    fagus: &Path,
) -> Result<(usize, usize, Vec<f64>), String> {
    // Enough pages for the longest documents of the corpus.
    const PAGES: usize = 200;
    let read = |path: &Path| std::fs::read(path).map_err(|error| error.to_string());
    let chrome_pages = rasterizer.rasterize(&read(chrome)?, options.dpi, PAGES)?;
    let fagus_pages = rasterizer.rasterize(&read(fagus)?, options.dpi, PAGES)?;
    let shares = chrome_pages
        .iter()
        .zip(&fagus_pages)
        .map(|(a, b)| tolerant_share(a, b, options.threshold))
        .collect();
    Ok((chrome_pages.len(), fagus_pages.len(), shares))
}

/// The results as lines of text, and a summary in Markdown.
pub fn report(comparisons: &[Comparison], page_share: f64, chrome_version: &str) -> (String, String) {
    let mut lines = String::new();
    let mut matched = 0;
    for comparison in comparisons {
        if let Some(feature) = comparison.skipped {
            let _ = writeln!(
                lines,
                "SKIP   {}  uses {feature}, which Chrome lacks",
                comparison.document
            );
            continue;
        }
        let status = if comparison.matches(page_share) {
            matched += 1;
            "MATCH"
        } else if comparison.error.is_some() {
            "ERROR"
        } else {
            "DIFFER"
        };
        let _ = write!(lines, "{status:6} {}", comparison.document);
        match &comparison.error {
            Some(error) => {
                let _ = writeln!(lines, "  {error}");
            }
            None => {
                let _ = writeln!(
                    lines,
                    "  pages {} / {}, worst page {:.3} %",
                    comparison.chrome_pages,
                    comparison.fagus_pages,
                    comparison.worst() * 100.0
                );
            }
        }
    }
    let total = compared(comparisons);
    let skipped = comparisons.len() - total;
    let mut summary = format!(
        "### Corpus against Chrome\n\n{matched} of {total} documents match {} ({:.1} %): same page count, at most {:.2} % of the pixels of a page differ. {skipped} documents use print features Chrome lacks and are not compared.\n\n",
        if chrome_version.is_empty() {
            "Chrome".to_owned()
        } else {
            chrome_version.to_owned()
        },
        share(matched, total) * 100.0,
        page_share * 100.0
    );
    let differing: Vec<&Comparison> = comparisons
        .iter()
        .filter(|comparison| comparison.skipped.is_none() && !comparison.matches(page_share))
        .collect();
    if !differing.is_empty() {
        summary.push_str("| Document | Pages (Chrome / Fagus) | Worst page |\n|---|---|--:|\n");
        for comparison in differing {
            let _ = writeln!(
                summary,
                "| `{}` | {} | {} |",
                comparison.document,
                comparison
                    .error
                    .clone()
                    .unwrap_or_else(|| format!("{} / {}", comparison.chrome_pages, comparison.fagus_pages)),
                if comparison.error.is_some() {
                    "–".to_owned()
                } else {
                    format!("{:.3} %", comparison.worst() * 100.0)
                }
            );
        }
    }
    (lines, summary)
}

/// The number of documents that were compared (not skipped).
pub fn compared(comparisons: &[Comparison]) -> usize {
    comparisons
        .iter()
        .filter(|comparison| comparison.skipped.is_none())
        .count()
}

/// `part` of `total` as a share from 0 to 1 (1 for nothing).
pub fn share(part: usize, total: usize) -> f64 {
    if total == 0 {
        1.0
    } else {
        part as f64 / total as f64
    }
}

/// The version line of a Chrome binary.
pub fn chrome_version(chrome: &Path) -> String {
    Command::new(chrome)
        .arg("--version")
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_the_print_features_chrome_lacks() {
        assert_eq!(
            chrome_lacks("h1 { string-set: title content() }"),
            Some("named strings")
        );
        assert_eq!(
            chrome_lacks("@top-center { content: element(header) }"),
            Some("running elements")
        );
        assert_eq!(
            chrome_lacks("@page { @top-center { content: counter(page) } }"),
            None
        );
    }

    #[test]
    fn writes_file_urls_for_chrome() {
        assert_eq!(file_url(Path::new("/a b/c#d.html")), "file:///a%20b/c%23d.html");
    }

    #[test]
    fn a_document_matches_with_the_same_pages_within_the_share() {
        let comparison = Comparison {
            document: "a.html".to_owned(),
            chrome_pages: 2,
            fagus_pages: 2,
            shares: vec![0.0, 0.001],
            error: None,
            skipped: None,
        };
        assert!(comparison.matches(0.002));
        assert!(!comparison.matches(0.0005));
        let fewer = Comparison {
            fagus_pages: 1,
            shares: vec![0.0],
            ..comparison.clone()
        };
        assert!(!fewer.matches(0.002));
        let skipped = Comparison {
            document: "b.html".to_owned(),
            skipped: Some("footnotes"),
            ..comparison.clone()
        };
        assert!(!skipped.matches(0.002));
        let (lines, summary) = report(&[comparison, fewer, skipped], 0.002, "Chrome 1");
        assert!(lines.starts_with("MATCH  a.html  pages 2 / 2"));
        assert!(lines.contains("SKIP   b.html  uses footnotes"));
        assert!(summary.contains("1 of 2 documents match Chrome 1 (50.0 %)"));
        assert!(summary.contains("1 documents use print features"));
        assert!(!summary.contains("`b.html`"));
        assert!(summary.contains("| `a.html` | 2 / 1 |"));
    }

    #[test]
    fn finds_the_documents_of_the_corpus() {
        let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus");
        let documents = documents(&corpus).unwrap();
        assert!(documents.contains(&"tables.html".to_owned()));
        assert!(documents.iter().any(|document| document.starts_with("invoice/")));
        assert!(documents.is_sorted());
    }
}

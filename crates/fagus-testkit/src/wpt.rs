//! Reftests of web-platform-tests (WPT), rendered by Fagus on both sides: the test and its
//! reference are printed to PDF, drawn at 96 dpi (one pixel per CSS pixel) and compared as WPT
//! compares screenshots, with the fuzziness the test allows, after edges one pixel apart are
//! forgiven (see [`EDGE_TOLERANCE`]).
//!
//! Which files are reftests follows the WPT manifest (`tools/manifest/sourcefile.py`): files with
//! markup and a `<link rel=match>` or `<link rel=mismatch>`, outside `support`, `resources` and
//! `tools`, that are no manual, visual or crash tests. A reftest normally compares an 800 × 600
//! viewport; Fagus prints it on a page of that size without margins and compares the first page.
//! Print reftests (`-print` before the extension, or in a directory `print`) are printed on pages
//! of 5 × 3 inches with margins of half an inch and compared page by page. Both page setups go in
//! a style sheet put before the document's own, so a test's own `@page` rules win.
//!
//! Tests that need JavaScript (a `<script>` in the test or a reference), the WPT server (`.sub.`
//! files) or are SVG documents are not run; each is counted as not run, with the reason.
//!
//! A test without a reference of its own (a manual test, which a person judges by its text) runs
//! against a reference of this repository where one exists: `tools/wpt/references/<path of the
//! test without extension>-ref.<extension>`. Such a test is printed as a paged test if its flags
//! say `paged`, on a page large enough for what the test expects on one page ([`PAGED_PAGE`]).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::process;
use crate::raster::{Difference, Image, Rasterizer, tolerant_difference};

/// The style sheet before the document's own for a reftest: an 800 × 600 viewport.
const SCREEN_PAGE: &str = "@page{size:800px 600px;margin:0}";
/// The style sheet before the document's own for a print reftest (WPT's default page).
const PRINT_PAGE: &str = "@page{size:5in 3in;margin:0.5in}";
/// The style sheet before the document's own for a test that runs against a reference of this
/// repository and is flagged `paged`: a letter page with WPT's margins of half an inch, so that
/// the running heads of the margin boxes have room and what the test expects on one page fits.
const PAGED_PAGE: &str = "@page{size:8.5in 11in;margin:0.5in}";
/// One pixel per CSS pixel.
const DPI: u32 = 96;
/// The pages of a print reftest compared at most.
const MAX_PAGES: usize = 20;
/// How far a channel may lie outside the range of the pixels around it in the other image (see
/// [`tolerant_difference`]). Browsers snap boxes and glyphs to whole pixels; in a PDF they keep
/// their exact positions, and a rasterizer draws the edges of glyphs and of filled shapes at
/// the same fractional position differently, so an edge may come out a pixel apart or in
/// another shade of antialiasing. Content that differs by more than that edge still counts.
const EDGE_TOLERANCE: u8 = 2;
/// How deep chains of references (a reference with references of its own) are followed.
const MAX_CHAIN: usize = 8;

/// How a test is printed: on a page of WPT's viewport (only the first page counts), on WPT's
/// page for print reftests or, for a paged test with a reference of this repository, on a letter
/// page; the last two are compared page by page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Page {
    Screen,
    Print,
    Paged,
}

impl Page {
    fn style(self) -> &'static str {
        match self {
            Self::Screen => SCREEN_PAGE,
            Self::Print => PRINT_PAGE,
            Self::Paged => PAGED_PAGE,
        }
    }

    /// Whether all pages are compared, not only the first.
    pub fn paged(self) -> bool {
        self != Self::Screen
    }
}

/// The prefix of the paths of the references of this repository, which are not in the checkout.
pub const OWN: &str = "fagus:";

/// Where the files of the tests are: the checkout of web-platform-tests, and the directory with
/// the references of this repository for tests that have none.
#[derive(Clone, Debug)]
pub struct Sources {
    pub checkout: PathBuf,
    pub references: PathBuf,
}

impl Sources {
    /// The file of a path in the checkout, or of one of our references ([`OWN`]).
    pub fn file(&self, path: &str) -> PathBuf {
        match path.strip_prefix(OWN) {
            Some(own) => self.references.join(own),
            None => self.checkout.join(path),
        }
    }

    /// The path of our reference for the test `path`, if there is one.
    fn own_reference(&self, path: &str) -> Option<String> {
        let (stem, extension) = path.rsplit_once('.')?;
        let reference = format!("{stem}-ref.{extension}");
        self.references
            .join(&reference)
            .is_file()
            .then(|| format!("{OWN}{reference}"))
    }
}

/// Whether a reference must look the same as the test or different.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    Match,
    Mismatch,
}

/// The fuzziness a test allows (`<meta name=fuzzy>`): ranges for the largest difference of a
/// channel and for the number of pixels that differ, both inclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fuzzy {
    pub max_channel: (u32, u32),
    pub pixels: (usize, usize),
}

/// A page range of `<meta name=reftest-pages>`, 1-based and inclusive; `None` is open.
pub type PageRange = (Option<usize>, Option<usize>);

/// What a test or reference file says about itself.
#[derive(Clone, Debug, Default)]
pub struct Meta {
    /// The references, as paths in the checkout.
    pub references: Vec<(String, Relation)>,
    /// Fuzziness for one reference (by its path) or, with `None`, for all.
    pub fuzzy: Vec<(Option<String>, Fuzzy)>,
    /// Page ranges for a file (by its path; `None` is the file itself).
    pub pages: Vec<(Option<String>, Vec<PageRange>)>,
    /// Whether the file has a script (it needs JavaScript).
    pub script: bool,
    /// Whether the file uses WPT's `testharness.js` (it is a JavaScript test).
    pub testharness: bool,
    /// Whether the file is missing or unreadable.
    pub missing: bool,
    /// Its flags (`<meta name=flags>`), such as `paged` for a test of paged media.
    pub flags: Vec<String>,
}

/// Why a test is not run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NotRun {
    /// The test or a reference has a script.
    Script,
    /// The test or a reference is a `.sub.` file, which the WPT server fills in.
    Server,
    /// The test is an SVG document.
    Svg,
    /// A test without a reference (a manual or visual test), which only a person can judge.
    NoReference,
    /// The expectations skip it.
    Skipped,
}

impl NotRun {
    pub fn describe(self) -> &'static str {
        match self {
            Self::Script => "needs JavaScript",
            Self::Server => "needs the WPT server",
            Self::Svg => "SVG document",
            Self::NoReference => "without a reference",
            Self::Skipped => "skipped",
        }
    }
}

/// A reftest found in the checkout.
#[derive(Clone, Debug)]
pub struct Test {
    /// The path in the checkout, with `/`.
    pub path: String,
    /// The directory of the list it belongs to.
    pub group: String,
    pub page: Page,
    pub not_run: Option<NotRun>,
    /// The files the test may render: itself and its references, with theirs.
    pub files: Vec<String>,
}

/// The result of a test that was run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Pass,
    Fail,
    /// Rendering failed (an error of the engine, a missing reference, the rasterizer failed).
    Error,
    Timeout,
}

impl Status {
    pub fn name(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::Error => "ERROR",
            Self::Timeout => "TIMEOUT",
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Finding the tests

/// What the name of a file says about it, as the WPT manifest reads it.
fn type_flag(name: &str) -> Option<&str> {
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    let (_, flags) = stem.rsplit_once('-')?;
    Some(flags.split('.').next().unwrap_or(flags))
}

/// Whether the file is markup the manifest looks into, and whether it is XML.
fn markup(path: &str) -> Option<bool> {
    let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
    match extension.as_str() {
        "html" | "htm" => Some(false),
        "xhtml" | "xht" | "xml" | "svg" => Some(true),
        _ => None,
    }
}

/// Whether the manifest takes the file for a support file or a crash test.
fn not_a_test(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    let directories = &parts[..parts.len() - 1];
    let name = parts[parts.len() - 1];
    directories.is_empty()
        || parts[0] == "common"
        || directories
            .iter()
            .any(|part| matches!(*part, "resources" | "support" | "tools"))
        || parts.starts_with(&["css", "CSS2", "archive"])
        || parts.starts_with(&["css", "common"])
        || name.starts_with('.')
        || type_flag(name) == Some("crash")
        || directories.contains(&"crashtests")
}

/// Whether the manifest takes the file for a manual or visual test by its name.
fn manual_by_name(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    matches!(type_flag(name), Some("manual" | "visual"))
}

/// Whether the name says the file is a reference: `-ref`, `_ref2`, `-notref` and the like in
/// its name, or a directory `reference` (the manifest's `reference_file_re`).
fn reference_by_name(path: &str) -> bool {
    if path.contains("/reference/") {
        return true;
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    stem.split(['-', '_']).any(|word| {
        let word = word.strip_prefix("not").unwrap_or(word);
        word.strip_prefix("ref")
            .is_some_and(|digits| digits.bytes().all(|byte| byte.is_ascii_digit()))
    })
}

/// Whether a reftest is a print reftest.
fn is_print(path: &str) -> bool {
    let (directories, name) = path.rsplit_once('/').unwrap_or(("", path));
    type_flag(name) == Some("print") || directories.split('/').any(|part| part == "print")
}

/// The empty page, which some tests take as their reference.
const ABOUT_BLANK: &str = "about:blank";

/// Resolves `href` in the file `from` (both paths in the checkout) as a URL: `/` starts at the
/// checkout, a query or fragment is dropped, `.` and `..` are applied.
pub fn resolve(from: &str, href: &str) -> String {
    let href = href.trim();
    if href.starts_with("about:") {
        return ABOUT_BLANK.to_owned();
    }
    let href = href.split(['?', '#']).next().unwrap_or_default();
    let mut parts: Vec<&str> = if href.starts_with('/') {
        Vec::new()
    } else {
        let mut parts: Vec<&str> = from.split('/').collect();
        parts.pop();
        parts
    };
    for part in href.trim_start_matches('/').split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    parts.join("/")
}

/// Parses `<meta name=fuzzy>`: `maxDifference=10-15;totalPixels=200-300`, the same without
/// names, single numbers for exact values, optionally after a reference and a colon.
pub fn parse_fuzzy(from: &str, content: &str) -> Option<(Option<String>, Fuzzy)> {
    let (key, value) = match content.rsplit_once(':') {
        Some((reference, value)) => (Some(resolve(from, reference)), value),
        None => (None, content),
    };
    let mut named: HashMap<&str, (usize, usize)> = HashMap::new();
    let mut positional = Vec::new();
    let ranges: Vec<&str> = value.split(';').collect();
    if ranges.len() != 2 {
        return None;
    }
    for range in ranges {
        let (name, range) = match range.split_once('=') {
            Some((name, range)) => (Some(name.trim()), range),
            None => (None, range),
        };
        let (low, high) = range.split_once('-').unwrap_or((range, range));
        let range = (low.trim().parse().ok()?, high.trim().parse().ok()?);
        match name {
            Some(name @ ("maxDifference" | "totalPixels")) => {
                named.insert(name, range);
            }
            Some(_) => return None,
            None => positional.push(range),
        }
    }
    let mut positional = positional.into_iter();
    let max_channel = named.remove("maxDifference").or_else(|| positional.next())?;
    let pixels = named.remove("totalPixels").or_else(|| positional.next())?;
    Some((
        key,
        Fuzzy {
            max_channel: (max_channel.0 as u32, max_channel.1 as u32),
            pixels,
        },
    ))
}

/// Parses `<meta name=reftest-pages>`: `-2,4,6,7,9-`, optionally after a file and a colon.
pub fn parse_pages(from: &str, content: &str) -> Option<(Option<String>, Vec<PageRange>)> {
    let (key, value) = match content.rsplit_once(':') {
        Some((file, value)) => (Some(resolve(from, file)), value),
        None => (None, content),
    };
    let mut ranges = Vec::new();
    for item in value.split(',') {
        let item = item.trim();
        let number = |text: &str| -> Option<Option<usize>> {
            let text = text.trim();
            if text.is_empty() {
                Some(None)
            } else {
                text.parse().ok().filter(|&page| page > 0).map(Some)
            }
        };
        if let Some((low, high)) = item.split_once('-') {
            ranges.push((number(low)?, number(high)?));
        } else {
            let page = number(item)??;
            ranges.push((Some(page), Some(page)));
        }
    }
    Some((key, ranges))
}

/// Reads what a file says about itself.
pub fn read_meta(sources: &Sources, path: &str) -> Meta {
    if path == ABOUT_BLANK {
        return Meta::default();
    }
    let Ok(bytes) = std::fs::read(sources.file(path)) else {
        return Meta {
            missing: true,
            ..Meta::default()
        };
    };
    let text = String::from_utf8_lossy(&bytes);
    parse_meta(path, &text)
}

/// What the markup `text` of the file `path` says about itself.
pub fn parse_meta(path: &str, text: &str) -> Meta {
    let mut meta = Meta::default();
    let document = if markup(path) == Some(true) {
        fagus_dom::Document::parse_xhtml(text)
    } else {
        fagus_dom::Document::parse_html(text)
    };
    for id in document.descendants(fagus_dom::NodeId::DOCUMENT) {
        let Some(element) = document.element(id) else {
            continue;
        };
        if element.is("script") || element.is_svg("script") {
            meta.script = true;
            let source = element
                .attribute("src")
                .or_else(|| {
                    element
                        .attributes
                        .iter()
                        .find(|attribute| &*attribute.name.local == "href")
                        .map(|attribute| attribute.value.as_str())
                })
                .unwrap_or_default();
            if source.ends_with("testharness.js") {
                meta.testharness = true;
            }
        } else if element.is("link") {
            let relation = match element.attribute("rel").map(str::trim) {
                Some("match") => Relation::Match,
                Some("mismatch") => Relation::Mismatch,
                _ => continue,
            };
            if let Some(href) = element.attribute("href") {
                meta.references.push((resolve(path, href), relation));
            }
        } else if element.is("meta") {
            let content = element.attribute("content").unwrap_or_default();
            match element.attribute("name") {
                Some("fuzzy") => meta.fuzzy.extend(parse_fuzzy(path, content)),
                Some("reftest-pages") => meta.pages.extend(parse_pages(path, content)),
                Some("flags") => meta.flags.extend(content.split_whitespace().map(str::to_owned)),
                _ => {}
            }
        }
        // Event handler attributes run scripts too, as does a reftest that waits for one.
        if element
            .attributes
            .iter()
            .any(|attribute| attribute.name.local.starts_with("on") && attribute.name.local.len() > 2)
            || (element.is("html") && element.classes.iter().any(|class| class == "reftest-wait"))
        {
            meta.script = true;
        }
    }
    meta
}

/// The files of the checkout's directories, with the metadata of the files read so far.
pub struct Collection {
    pub tests: Vec<Test>,
    pub metas: HashMap<String, Arc<Meta>>,
    /// Our references in the directories that no test uses: their test is gone or has a
    /// reference of its own now.
    pub unused_references: Vec<String>,
}

/// Lists the files below `directory` (a path in the checkout), sorted.
fn files_below(checkout: &Path, directory: &str, files: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(checkout.join(directory)) else {
        return;
    };
    let mut names: Vec<(String, bool)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let directory = entry.file_type().ok()?.is_dir();
            Some((entry.file_name().into_string().ok()?, directory))
        })
        .collect();
    names.sort();
    for (name, is_directory) in names {
        let path = format!("{directory}/{name}");
        if is_directory {
            files_below(checkout, &path, files);
        } else {
            files.push(path);
        }
    }
}

/// Finds the reftests in `directories` of the checkout, and the tests without a reference that
/// we have one for.
pub fn collect(sources: &Sources, directories: &[String], jobs: usize) -> Collection {
    let checkout = sources.checkout.as_path();
    let mut candidates = Vec::new();
    for directory in directories {
        let mut files = Vec::new();
        files_below(checkout, directory, &mut files);
        candidates.extend(
            files
                .into_iter()
                .filter(|path| markup(path).is_some() && !not_a_test(path))
                .map(|path| (path, directory.clone())),
        );
    }
    let metas = process::map(
        &candidates,
        jobs,
        |(path, _)| Arc::new(read_meta(sources, path)),
        |_| {},
    );
    let mut known: HashMap<String, Arc<Meta>> = candidates
        .iter()
        .map(|(path, _)| path.clone())
        .zip(metas)
        .collect();
    let mut tests = Vec::new();
    let mut without_reference = Vec::new();
    let mut own = HashSet::new();
    for (path, group) in &candidates {
        let mut meta = Arc::clone(&known[path]);
        if meta.testharness {
            continue;
        }
        let mut page = if is_print(path) { Page::Print } else { Page::Screen };
        if manual_by_name(path) || meta.references.is_empty() {
            let manual = !meta.script && (manual_by_name(path) || !reference_by_name(path));
            let Some(reference) = sources.own_reference(path).filter(|_| manual) else {
                if manual {
                    without_reference.push((path, group));
                }
                continue;
            };
            let mut with_reference = (*meta).clone();
            with_reference.references = vec![(reference.clone(), Relation::Match)];
            meta = Arc::new(with_reference);
            known.insert(path.clone(), Arc::clone(&meta));
            own.insert(reference);
            if page == Page::Screen && meta.flags.iter().any(|flag| flag == "paged") {
                page = Page::Paged;
            }
        }
        let files = reachable(sources, path, &mut known);
        let not_run = if path.to_ascii_lowercase().ends_with(".svg") {
            Some(NotRun::Svg)
        } else if files.iter().any(|file| file.contains(".sub.")) {
            Some(NotRun::Server)
        } else if files.iter().any(|file| known[file].script) {
            Some(NotRun::Script)
        } else {
            None
        };
        tests.push(Test {
            path: path.clone(),
            group: group.clone(),
            page,
            not_run,
            files,
        });
    }
    // Files without references that no test uses as one are manual or visual tests.
    let used: HashSet<&str> = tests
        .iter()
        .flat_map(|test| test.files.iter().skip(1).map(String::as_str))
        .collect();
    let mut manual: Vec<Test> = without_reference
        .into_iter()
        .filter(|(path, _)| !used.contains(path.as_str()))
        .map(|(path, group)| Test {
            path: path.clone(),
            group: group.clone(),
            page: if is_print(path) { Page::Print } else { Page::Screen },
            not_run: Some(NotRun::NoReference),
            files: vec![path.clone()],
        })
        .collect();
    tests.append(&mut manual);
    tests.sort_by(|a, b| a.path.cmp(&b.path));
    let mut unused_references = Vec::new();
    for directory in directories {
        let mut files = Vec::new();
        files_below(&sources.references, directory, &mut files);
        unused_references.extend(
            files
                .into_iter()
                .map(|file| format!("{OWN}{file}"))
                .filter(|reference| !own.contains(reference)),
        );
    }
    Collection {
        tests,
        metas: known,
        unused_references,
    }
}

/// The file `path` and the references reachable from it, reading the metadata of those not read
/// yet.
fn reachable(sources: &Sources, path: &str, known: &mut HashMap<String, Arc<Meta>>) -> Vec<String> {
    let mut files = vec![path.to_owned()];
    let mut index = 0;
    while index < files.len() && files.len() < 64 {
        let file = files[index].clone();
        let meta = Arc::clone(
            known
                .entry(file.clone())
                .or_insert_with(|| Arc::new(read_meta(sources, &file))),
        );
        for (reference, _) in &meta.references {
            if !files.contains(reference) {
                files.push(reference.clone());
            }
        }
        index += 1;
    }
    for file in &files {
        known
            .entry(file.clone())
            .or_insert_with(|| Arc::new(read_meta(sources, file)));
    }
    files
}

// ---------------------------------------------------------------------------------------------
// Rendering

/// Puts a style sheet with `css` at the start of the document's head, before its own rules.
pub fn with_page_style(source: &str, css: &str) -> String {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let lower = source.to_ascii_lowercase();
    let at = tag_end(&lower, "<head")
        .or_else(|| tag_end(&lower, "<html"))
        .or_else(|| tag_end(&lower, "<!doctype"))
        .or_else(|| {
            lower
                .starts_with("<?xml")
                .then(|| lower.find("?>").map(|end| end + 2))
                .flatten()
        })
        .unwrap_or(0);
    format!("{}<style>{css}</style>{}", &source[..at], &source[at..])
}

/// Where the first start tag `open` (`<head`, lower case) of `lower` ends.
fn tag_end(lower: &str, open: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(found) = lower[from..].find(open) {
        let after = from + found + open.len();
        match lower.as_bytes().get(after) {
            Some(byte) if byte.is_ascii_whitespace() || matches!(byte, b'>' | b'/') => {
                return lower[after..].find('>').map(|end| after + end + 1);
            }
            _ => from = after,
        }
    }
    None
}

/// Why a file could not be rendered.
#[derive(Clone, Debug)]
pub enum Failure {
    Error(String),
    Timeout,
}

/// The pages of a rendered file (at most [`MAX_PAGES`], or the first for a reftest) and how many
/// it has.
#[derive(Debug)]
pub struct Rendered {
    pub pages: Vec<Image>,
    pub count: usize,
}

type Rendering = Arc<Result<Rendered, Failure>>;
type Key = (String, Page);

/// Renders the files of the tests, keeping each until the last test that may use it is done.
pub struct Renderer {
    sources: Sources,
    engine: Arc<fagus::Engine>,
    rasterizer: Rasterizer,
    timeout: Duration,
    cache: Mutex<HashMap<Key, Arc<OnceLock<Rendering>>>>,
    uses: Mutex<HashMap<Key, usize>>,
    /// Where the pages of failing comparisons are written as PNG files, if anywhere.
    pub images: Option<PathBuf>,
}

impl Renderer {
    pub fn new(sources: &Sources, scratch: &Path, timeout: Duration) -> anyhow::Result<Self> {
        let checkout = sources.checkout.canonicalize()?;
        let mut engine = fagus::Engine::new();
        // WPT expects Ahem to be installed. Text in other families is set in the font built
        // into Fagus, the same on every machine.
        let ahem = std::fs::read(checkout.join("fonts/Ahem.ttf"))?;
        engine.add_font_data(ahem)?;
        let inter = || Some("Inter".to_owned());
        engine.set_generic_families(inter(), inter(), inter());
        Ok(Self {
            sources: Sources {
                checkout,
                references: std::path::absolute(&sources.references)?,
            },
            engine: Arc::new(engine),
            rasterizer: Rasterizer::new(scratch)?,
            timeout,
            cache: Mutex::default(),
            uses: Mutex::default(),
            images: None,
        })
    }

    pub fn rasterizer(&self) -> &Rasterizer {
        &self.rasterizer
    }

    /// Counts the files the tests may render, so that each is dropped after its last use.
    pub fn plan(&self, tests: &[&Test]) {
        let mut uses = self.uses.lock().unwrap_or_else(PoisonError::into_inner);
        for test in tests {
            for file in &test.files {
                *uses.entry((file.clone(), test.page)).or_default() += 1;
            }
        }
    }

    /// Lets go of the files of a test that is done.
    pub fn done(&self, test: &Test) {
        let mut uses = self.uses.lock().unwrap_or_else(PoisonError::into_inner);
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        for file in &test.files {
            let key = (file.clone(), test.page);
            if let Some(count) = uses.get_mut(&key) {
                *count -= 1;
                if *count == 0 {
                    uses.remove(&key);
                    cache.remove(&key);
                }
            }
        }
    }

    /// The pages of `path`, rendered once for all the tests that use it.
    pub fn get(&self, path: &str, page: Page) -> Rendering {
        let cell = {
            let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
            Arc::clone(cache.entry((path.to_owned(), page)).or_default())
        };
        Arc::clone(cell.get_or_init(|| Arc::new(self.render(path, page))))
    }

    fn render(&self, path: &str, page: Page) -> Result<Rendered, Failure> {
        let checkout = &self.sources.checkout;
        let file = self.sources.file(path);
        let bytes = if path == ABOUT_BLANK {
            Vec::new()
        } else {
            std::fs::read(&file).map_err(|error| Failure::Error(format!("{path}: {error}")))?
        };
        let source = with_page_style(&String::from_utf8_lossy(&bytes), page.style());
        let document = if markup(path) == Some(true) {
            fagus::Document::xhtml(source)
        } else {
            fagus::Document::html(source)
        };
        let document = document
            .base_directory(file.parent().unwrap_or(checkout))
            .root(checkout)
            .tagged(false)
            .created(fagus::Date::from_unix(0));
        // On a thread of its own, so that a document that takes too long can be left behind.
        let engine = Arc::clone(&self.engine);
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("wpt".to_owned())
            .spawn(move || {
                let _ = sender.send(engine.render(&document));
            })
            .map_err(|error| Failure::Error(error.to_string()))?;
        let output = match receiver.recv_timeout(self.timeout) {
            Ok(Ok(output)) => output,
            Ok(Err(error)) => return Err(Failure::Error(format!("{path}: {error}"))),
            Err(mpsc::RecvTimeoutError::Timeout) => return Err(Failure::Timeout),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(Failure::Error(format!("{path}: the rendering thread stopped")));
            }
        };
        let last = if page.paged() { MAX_PAGES } else { 1 };
        let pages = self
            .rasterizer
            .rasterize(&output.pdf, DPI, last)
            .map_err(|error| Failure::Error(format!("{path}: {error}")))?;
        Ok(Rendered {
            pages,
            count: output.pages,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Comparing

/// The pages of `rendered` that `ranges` select (all without ranges), as WPT selects them.
fn select<'a>(rendered: &'a Rendered, ranges: Option<&Vec<PageRange>>) -> Vec<&'a Image> {
    let Some(ranges) = ranges else {
        return rendered.pages.iter().collect();
    };
    let count = rendered.count;
    let mut chosen = std::collections::BTreeSet::new();
    for &(low, high) in ranges {
        let low = low.unwrap_or(1);
        let high = high.unwrap_or(count);
        if low > count {
            break;
        }
        chosen.extend(low..=high);
    }
    chosen
        .into_iter()
        .filter_map(|page| rendered.pages.get(page - 1))
        .collect()
}

/// Whether two renderings count as equal under `fuzzy`, as wptrunner decides it; with the
/// difference of the first page that is not.
pub fn equal(left: &[&Image], right: &[&Image], fuzzy: Option<Fuzzy>) -> (bool, Option<(usize, Difference)>) {
    if left.len() != right.len() {
        return (false, None);
    }
    for (index, (a, b)) in left.iter().zip(right).enumerate() {
        let found = tolerant_difference(a, b, EDGE_TOLERANCE);
        let same = match fuzzy {
            None
            | Some(Fuzzy {
                max_channel: (0, 0),
                pixels: (0, 0),
            }) => found.pixels == 0,
            Some(Fuzzy { max_channel, pixels }) => {
                let channel = u32::from(found.max_channel);
                (found.pixels == 0 && pixels.0 == 0)
                    || (channel == 0 && max_channel.0 == 0)
                    || ((max_channel.0..=max_channel.1).contains(&channel)
                        && (pixels.0..=pixels.1).contains(&found.pixels))
            }
        };
        if !same {
            return (false, Some((index, found)));
        }
    }
    (true, None)
}

/// The fuzziness for comparing `left` with its reference `right`: what `left` says for `right`,
/// else for all its references, else what the test says for all.
fn fuzzy_for(metas: &HashMap<String, Arc<Meta>>, test: &str, left: &str, right: &str) -> Option<Fuzzy> {
    let find = |file: &str, key: Option<&str>| {
        metas.get(file).and_then(|meta| {
            meta.fuzzy
                .iter()
                .find(|(reference, _)| reference.as_deref() == key)
                .map(|(_, fuzzy)| *fuzzy)
        })
    };
    find(left, Some(right))
        .or_else(|| find(left, None))
        .or_else(|| find(test, None))
}

/// The page ranges the test gives for `file`.
fn pages_for<'a>(
    metas: &'a HashMap<String, Arc<Meta>>,
    test: &str,
    file: &str,
) -> Option<&'a Vec<PageRange>> {
    metas.get(test).and_then(|meta| {
        meta.pages
            .iter()
            .find(|(key, _)| key.as_deref().unwrap_or(test) == file)
            .map(|(_, ranges)| ranges)
    })
}

/// Runs a test: compares it with its references as wptrunner does. A test passes if the
/// comparison with one of its references passes and, where that reference has references of its
/// own, one of the chains below it passes too.
pub fn run(renderer: &Renderer, collection: &Collection, test: &Test) -> (Status, String) {
    let metas = &collection.metas;
    let references = |file: &str| -> Vec<(String, Relation)> {
        metas
            .get(file)
            .map(|meta| meta.references.clone())
            .unwrap_or_default()
    };
    let mut stack: Vec<(String, String, Relation, usize)> = references(&test.path)
        .into_iter()
        .rev()
        .map(|(reference, relation)| (test.path.clone(), reference, relation, 1))
        .collect();
    let mut last = String::new();
    while let Some((left, right, relation, depth)) = stack.pop() {
        let left_rendering = renderer.get(&left, test.page);
        let right_rendering = renderer.get(&right, test.page);
        let (a, b) = match (&*left_rendering, &*right_rendering) {
            (Ok(a), Ok(b)) => (a, b),
            (Err(Failure::Timeout), _) | (_, Err(Failure::Timeout)) => {
                return (Status::Timeout, String::new());
            }
            (Err(Failure::Error(error)), _) | (_, Err(Failure::Error(error))) => {
                return (Status::Error, error.clone());
            }
        };
        let fuzzy = fuzzy_for(metas, &test.path, &left, &right);
        let left_ranges = pages_for(metas, &test.path, &left);
        let right_ranges = pages_for(metas, &test.path, &right);
        let left_pages = select(a, left_ranges);
        let right_pages = select(b, right_ranges);
        let (same, found) = equal(&left_pages, &right_pages, fuzzy);
        // Beyond the pages compared, the counts of all pages must agree too.
        let counts_differ =
            test.page.paged() && a.count != b.count && left_ranges.is_none() && right_ranges.is_none();
        let same = same && !counts_differ;
        let passed = same == (relation == Relation::Match);
        if passed {
            let next = references(&right);
            if next.is_empty() || depth >= MAX_CHAIN {
                return (Status::Pass, String::new());
            }
            stack.extend(
                next.into_iter()
                    .rev()
                    .map(|(reference, relation)| (right.clone(), reference, relation, depth + 1)),
            );
        } else {
            last = describe(&right, relation, &left_pages, &right_pages, found, fuzzy);
            if let Some(images) = &renderer.images {
                save_images(images, &test.path, &left, &right, &left_pages, &right_pages);
            }
        }
    }
    (Status::Fail, last)
}

/// Writes the pages of a failing comparison to `images/<test>/`, named after their files.
fn save_images(
    images: &Path,
    test: &str,
    left: &str,
    right: &str,
    left_pages: &[&Image],
    right_pages: &[&Image],
) {
    let directory = images.join(test.replace('/', "__"));
    if std::fs::create_dir_all(&directory).is_err() {
        return;
    }
    for (file, pages) in [(left, left_pages), (right, right_pages)] {
        let name = file.rsplit('/').next().unwrap_or(file);
        for (index, page) in pages.iter().enumerate() {
            let _ = page.save_png(&directory.join(format!("{name}-{}.png", index + 1)));
        }
    }
}

/// Why a comparison failed, in one line.
fn describe(
    reference: &str,
    relation: Relation,
    left: &[&Image],
    right: &[&Image],
    found: Option<(usize, Difference)>,
    fuzzy: Option<Fuzzy>,
) -> String {
    let reference = reference.rsplit('/').next().unwrap_or(reference);
    if relation == Relation::Mismatch {
        return format!("looks the same as {reference}, which it must not");
    }
    match found {
        Some((page, difference)) => {
            let mut text = format!(
                "{} pixels differ from {reference} (up to {}",
                difference.pixels, difference.max_channel
            );
            if left.len() > 1 {
                let _ = write!(text, ", page {}", page + 1);
            }
            text.push(')');
            if let Some(fuzzy) = fuzzy {
                let _ = write!(
                    text,
                    ", allowed {}-{} pixels up to {}-{}",
                    fuzzy.pixels.0, fuzzy.pixels.1, fuzzy.max_channel.0, fuzzy.max_channel.1
                );
            }
            text
        }
        None => format!(
            "other pages than {reference} ({} and {})",
            left.len(),
            right.len()
        ),
    }
}

// ---------------------------------------------------------------------------------------------
// Expectations

/// What a test is expected to do, if not to pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Expected {
    Fail,
    Skip,
}

/// One line of the expectations file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expectation {
    pub expected: Expected,
    pub comment: Option<String>,
}

/// The expectations file: the tests that are expected to fail or are skipped. Every other test
/// is expected to pass.
#[derive(Clone, Debug, Default)]
pub struct Expectations {
    /// The comment lines at the start, kept when the file is written again.
    pub header: Vec<String>,
    pub tests: BTreeMap<String, Expectation>,
}

impl Expectations {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut expectations = Self::default();
        let mut in_header = true;
        for (number, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                if in_header {
                    expectations.header.push(line.to_owned());
                }
                continue;
            }
            in_header = false;
            let (entry, comment) = match trimmed.split_once(" #") {
                Some((entry, comment)) => (entry.trim(), Some(comment.trim().to_owned())),
                None => (trimmed, None),
            };
            let mut words = entry.split_whitespace();
            let expected = match words.next() {
                Some("FAIL") => Expected::Fail,
                Some("SKIP") => Expected::Skip,
                _ => return Err(format!("line {}: expected FAIL or SKIP and a test", number + 1)),
            };
            let (Some(path), None) = (words.next(), words.next()) else {
                return Err(format!("line {}: expected one test after the status", number + 1));
            };
            expectations
                .tests
                .insert(path.to_owned(), Expectation { expected, comment });
        }
        while expectations
            .header
            .last()
            .is_some_and(|line| line.trim().is_empty())
        {
            expectations.header.pop();
        }
        Ok(expectations)
    }

    pub fn get(&self, path: &str) -> Option<&Expectation> {
        self.tests.get(path)
    }

    pub fn write(&self) -> String {
        let mut text = String::new();
        for line in &self.header {
            text.push_str(line);
            text.push('\n');
        }
        if !self.header.is_empty() {
            text.push('\n');
        }
        for (path, expectation) in &self.tests {
            let status = match expectation.expected {
                Expected::Fail => "FAIL",
                Expected::Skip => "SKIP",
            };
            let _ = write!(text, "{status} {path}");
            if let Some(comment) = &expectation.comment {
                let _ = write!(text, "  # {comment}");
            }
            text.push('\n');
        }
        text
    }
}

// ---------------------------------------------------------------------------------------------
// Results

/// The result of one test.
#[derive(Clone, Debug)]
pub struct Outcome {
    pub test: Test,
    /// The status, or why it was not run.
    pub result: Result<(Status, String), NotRun>,
    pub expected: Option<Expected>,
}

impl Outcome {
    /// A test that failed although it was expected to pass.
    pub fn regressed(&self) -> bool {
        matches!(self.result, Ok((status, _)) if status != Status::Pass) && self.expected.is_none()
    }

    /// A test that passed although it was expected to fail.
    pub fn improved(&self) -> bool {
        matches!(self.result, Ok((Status::Pass, _))) && self.expected == Some(Expected::Fail)
    }
}

/// The numbers of one directory.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    pub directory: String,
    /// The reftests found.
    pub tests: usize,
    /// The tests run, and those of them that passed.
    pub run: usize,
    pub passed: usize,
    /// The tests not run, by reason.
    pub not_run: BTreeMap<NotRun, usize>,
}

impl Counts {
    /// The share of the tests run that passed, in percent.
    pub fn rate(&self) -> f64 {
        if self.run == 0 {
            0.0
        } else {
            self.passed as f64 * 100.0 / self.run as f64
        }
    }
}

/// What a run found, as `summary.json` keeps it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Summary {
    /// The commit of web-platform-tests.
    pub commit: String,
    pub directories: Vec<Counts>,
    pub regressions: Vec<String>,
    pub unexpected_passes: Vec<String>,
}

impl Summary {
    pub fn new(commit: String, directories: &[String], outcomes: &[Outcome]) -> Self {
        let mut counts: Vec<Counts> = directories
            .iter()
            .map(|directory| Counts {
                directory: directory.clone(),
                ..Counts::default()
            })
            .collect();
        let mut summary = Self {
            commit,
            ..Self::default()
        };
        for outcome in outcomes {
            let Some(count) = counts
                .iter_mut()
                .find(|count| count.directory == outcome.test.group)
            else {
                continue;
            };
            count.tests += 1;
            match &outcome.result {
                Ok((status, _)) => {
                    count.run += 1;
                    count.passed += usize::from(*status == Status::Pass);
                }
                Err(reason) => *count.not_run.entry(*reason).or_default() += 1,
            }
            if outcome.regressed() {
                summary.regressions.push(outcome.test.path.clone());
            }
            if outcome.improved() {
                summary.unexpected_passes.push(outcome.test.path.clone());
            }
        }
        summary.directories = counts.into_iter().filter(|count| count.tests > 0).collect();
        summary
    }

    /// The numbers of all directories together.
    pub fn total(&self) -> Counts {
        let mut total = Counts {
            directory: "total".to_owned(),
            ..Counts::default()
        };
        for count in &self.directories {
            total.tests += count.tests;
            total.run += count.run;
            total.passed += count.passed;
            for (reason, number) in &count.not_run {
                *total.not_run.entry(*reason).or_default() += number;
            }
        }
        total
    }

    /// The table of pass rates, in Markdown.
    pub fn table(&self) -> String {
        let mut text =
            String::from("| Directory | Passed | Run | Pass rate | Not run |\n|---|--:|--:|--:|---|\n");
        let row = |text: &mut String, count: &Counts, bold: bool| {
            let not_run = count
                .not_run
                .iter()
                .map(|(reason, number)| format!("{number} {}", reason.describe()))
                .collect::<Vec<_>>()
                .join(", ");
            let name = if bold {
                format!("**{}**", count.directory)
            } else {
                format!("`{}`", count.directory)
            };
            let rate = if count.run == 0 {
                "–".to_owned()
            } else {
                format!("{:.1} %", count.rate())
            };
            let _ = writeln!(
                text,
                "| {name} | {} | {} | {rate} | {} |",
                count.passed,
                count.run,
                if not_run.is_empty() {
                    "–".to_owned()
                } else {
                    not_run
                }
            );
        };
        for count in &self.directories {
            row(&mut text, count, false);
        }
        row(&mut text, &self.total(), true);
        text
    }
}

/// The start and end of the generated part of `docs/compatibility.md`.
pub const DOCS_START: &str = "<!-- The table up to the end marker is written by tools/wpt/run.sh --docs. -->";
pub const DOCS_END: &str = "<!-- End of the generated table. -->";

/// `page` with the part between the markers replaced by the table of `summary`.
pub fn update_docs(page: &str, summary: &Summary) -> Result<String, String> {
    let start = page
        .find(DOCS_START)
        .ok_or("the page has no start marker for the table")?
        + DOCS_START.len();
    let end = page[start..]
        .find(DOCS_END)
        .ok_or("the page has no end marker for the table")?
        + start;
    let commit = summary.commit.get(..10).unwrap_or(&summary.commit);
    let generated = format!(
        "\n\nweb-platform-tests at commit [`{commit}`](https://github.com/web-platform-tests/wpt/tree/{}):\n\n{}\n",
        summary.commit,
        summary.table()
    );
    Ok(format!("{}{generated}{}", &page[..start], &page[end..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_names_as_the_manifest_does() {
        assert_eq!(type_flag("float-001-print.html"), Some("print"));
        assert_eq!(type_flag("float-print.tentative.html"), Some("print"));
        assert_eq!(type_flag("float.html"), None);
        assert!(is_print("css/css-page/page-001-print.html"));
        assert!(is_print("css/css-break/print/table-001.html"));
        assert!(!is_print("css/css-page/page-print-001.html"));
        assert!(not_a_test("css/css-grid/support/grid.html"));
        assert!(not_a_test("css/css-grid/crashtests/grid.html"));
        assert!(not_a_test("css/css-grid/grid-001-crash.html"));
        assert!(!not_a_test("css/css-grid/reference/grid-ref.html"));
        assert!(!not_a_test("css/css-grid/grid-001.html"));
        assert!(manual_by_name("css/css-grid/grid-manual.html"));
        assert!(reference_by_name("css/a/grid-001-ref.html"));
        assert!(reference_by_name("css/a/grid-001-notref.xht"));
        assert!(reference_by_name("css/a/ref_grid2.html"));
        assert!(reference_by_name("css/reference/green.html"));
        assert!(!reference_by_name("css/a/prefix-001.html"));
        assert!(!reference_by_name("css/a/refresh-001.html"));
    }

    #[test]
    fn resolves_references_as_urls() {
        assert_eq!(resolve("css/a/b.html", "b-ref.html"), "css/a/b-ref.html");
        assert_eq!(
            resolve("css/a/b.html", "../reference/r.xht"),
            "css/reference/r.xht"
        );
        assert_eq!(resolve("css/a/b.html", "/css/x/r.html?x=1#y"), "css/x/r.html");
        assert_eq!(resolve("css/a/b.html", "./r.html"), "css/a/r.html");
        assert_eq!(resolve("css/a/b.html", "about:blank"), "about:blank");
    }

    #[test]
    fn parses_fuzziness_with_and_without_names() {
        let fuzzy = |max_channel, pixels| Fuzzy { max_channel, pixels };
        assert_eq!(
            parse_fuzzy("a/t.html", "maxDifference=10-15;totalPixels=200-300"),
            Some((None, fuzzy((10, 15), (200, 300))))
        );
        assert_eq!(
            parse_fuzzy("a/t.html", "totalPixels=0-5; maxDifference=0-2"),
            Some((None, fuzzy((0, 2), (0, 5))))
        );
        assert_eq!(
            parse_fuzzy("a/t.html", "15;300"),
            Some((None, fuzzy((15, 15), (300, 300))))
        );
        assert_eq!(
            parse_fuzzy("a/t.html", "t-ref.html:0-1;0-10"),
            Some((Some("a/t-ref.html".to_owned()), fuzzy((0, 1), (0, 10))))
        );
        assert_eq!(parse_fuzzy("a/t.html", "0-1"), None);
        assert_eq!(parse_fuzzy("a/t.html", "size=1;2"), None);
    }

    #[test]
    fn parses_page_ranges() {
        assert_eq!(
            parse_pages("a/t.html", "-2,4,9-"),
            Some((None, vec![(None, Some(2)), (Some(4), Some(4)), (Some(9), None)]))
        );
        assert_eq!(parse_pages("a/t.html", "0"), None);
        let rendered = Rendered {
            pages: (0..5)
                .map(|page| Image {
                    width: 1,
                    height: 1,
                    pixels: vec![page; 3],
                })
                .collect(),
            count: 5,
        };
        let pages = select(&rendered, Some(&vec![(None, Some(2)), (Some(4), None)]));
        assert_eq!(
            pages.iter().map(|page| page.pixels[0]).collect::<Vec<_>>(),
            [0, 1, 3, 4]
        );
    }

    #[test]
    fn finds_references_scripts_and_fuzziness() {
        let meta = parse_meta(
            "css/a/t.html",
            r#"<!doctype html><link rel="match" href="t-ref.html"><link rel=mismatch href="/css/reference/blank.html">
            <meta name="fuzzy" content="0-2;0-40"><p>Test</p>"#,
        );
        assert_eq!(
            meta.references,
            [
                ("css/a/t-ref.html".to_owned(), Relation::Match),
                ("css/reference/blank.html".to_owned(), Relation::Mismatch)
            ]
        );
        assert_eq!(meta.fuzzy.len(), 1);
        assert!(!meta.script);
        assert!(parse_meta("t.html", "<link rel=match href=r.html><script>1</script>").script);
        assert!(parse_meta("t.html", "<link rel=match href=r.html><body onload=f()>").script);
        assert!(
            parse_meta(
                "t.html",
                "<html class='reftest-wait'><link rel=match href=r.html>"
            )
            .script
        );
        let harness = parse_meta(
            "t.html",
            "<link rel=match href=r.html><script src=/resources/testharness.js></script>",
        );
        assert!(harness.testharness);
    }

    #[test]
    fn runs_tests_without_a_reference_against_our_own() {
        let root = std::env::temp_dir().join(format!("fagus-wpt-own-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let write = |path: &str, text: &str| {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        let paged = "<meta name=flags content='paged dom'><p>Test passes if</p>";
        write("checkout/css/x/paged.html", paged);
        write("checkout/css/x/screen.html", "<p>Test passes if</p>");
        write("checkout/css/x/manual.html", "<p>Test passes if</p>");
        write(
            "checkout/css/x/linked.html",
            "<link rel=match href=linked-ref.html>",
        );
        write("checkout/css/x/linked-ref.html", "<p>");
        write("references/css/x/paged-ref.html", "<p>Reference</p>");
        write("references/css/x/screen-ref.html", "<p>Reference</p>");
        write("references/css/x/linked-ref.html", "<p>Reference</p>");
        write("references/css/y/other-ref.html", "<p>Reference</p>");
        let sources = Sources {
            checkout: root.join("checkout"),
            references: root.join("references"),
        };
        let collection = collect(&sources, &["css/x".to_owned()], 1);
        let _ = std::fs::remove_dir_all(&root);
        let found: Vec<(&str, Page, Option<NotRun>, Vec<&str>)> = collection
            .tests
            .iter()
            .map(|test| {
                (
                    test.path.as_str(),
                    test.page,
                    test.not_run,
                    test.files.iter().map(String::as_str).collect(),
                )
            })
            .collect();
        assert_eq!(
            found,
            [
                (
                    "css/x/linked.html",
                    Page::Screen,
                    None,
                    vec!["css/x/linked.html", "css/x/linked-ref.html"]
                ),
                (
                    "css/x/manual.html",
                    Page::Screen,
                    Some(NotRun::NoReference),
                    vec!["css/x/manual.html"]
                ),
                (
                    "css/x/paged.html",
                    Page::Paged,
                    None,
                    vec!["css/x/paged.html", "fagus:css/x/paged-ref.html"]
                ),
                (
                    "css/x/screen.html",
                    Page::Screen,
                    None,
                    vec!["css/x/screen.html", "fagus:css/x/screen-ref.html"]
                ),
            ]
        );
        // Ours for a test with a reference of its own is unused; other directories do not count.
        assert_eq!(collection.unused_references, ["fagus:css/x/linked-ref.html"]);
        assert_eq!(
            sources.file("fagus:css/x/paged-ref.html"),
            root.join("references/css/x/paged-ref.html")
        );
    }

    #[test]
    fn puts_the_page_style_before_the_documents_own() {
        let css = "@page{}";
        assert_eq!(
            with_page_style("<!DOCTYPE html><html><HEAD><style>x</style>", css),
            "<!DOCTYPE html><html><HEAD><style>@page{}</style><style>x</style>"
        );
        assert_eq!(
            with_page_style("<!doctype html><header>", css),
            "<!doctype html><style>@page{}</style><header>"
        );
        assert_eq!(
            with_page_style("<?xml version=\"1.0\"?><html xmlns=\"x\"><body/></html>", css),
            "<?xml version=\"1.0\"?><html xmlns=\"x\"><style>@page{}</style><body/></html>"
        );
        assert_eq!(with_page_style("\u{feff}<p>", css), "<style>@page{}</style><p>");
    }

    fn image(fill: u8, changed: usize) -> Image {
        let mut pixels = vec![255; 30];
        for pixel in 0..changed {
            pixels[pixel * 3] = fill;
        }
        Image {
            width: 10,
            height: 1,
            pixels,
        }
    }

    #[test]
    fn compares_with_fuzziness_as_wptrunner() {
        let white = image(255, 0);
        let off = image(250, 3);
        assert!(equal(&[&white], &[&white], None).0);
        assert!(!equal(&[&white], &[&off], None).0);
        let allowed = Fuzzy {
            max_channel: (0, 5),
            pixels: (0, 3),
        };
        assert!(equal(&[&white], &[&off], Some(allowed)).0);
        let too_few = Fuzzy {
            max_channel: (0, 5),
            pixels: (0, 2),
        };
        assert!(!equal(&[&white], &[&off], Some(too_few)).0);
        // Equal images pass where the ranges allow none.
        let exact = Fuzzy {
            max_channel: (5, 5),
            pixels: (0, 3),
        };
        assert!(equal(&[&white], &[&white], Some(exact)).0);
        assert!(!equal(&[&white, &white], &[&white], None).0);
    }

    #[test]
    fn reads_and_writes_expectations() {
        let text = "# Expected results\n#\n\nFAIL css/b.html  # needs scrolling\nSKIP css/a.html\n";
        let expectations = Expectations::parse(text).unwrap();
        assert_eq!(expectations.header, ["# Expected results", "#"]);
        assert_eq!(
            expectations.get("css/b.html"),
            Some(&Expectation {
                expected: Expected::Fail,
                comment: Some("needs scrolling".to_owned())
            })
        );
        assert_eq!(
            expectations.write(),
            "# Expected results\n#\n\nSKIP css/a.html\nFAIL css/b.html  # needs scrolling\n"
        );
        assert!(Expectations::parse("PASS css/a.html").is_err());
        assert!(Expectations::parse("FAIL").is_err());
    }

    #[test]
    fn counts_results_by_directory() {
        let test = |path: &str| Test {
            path: path.to_owned(),
            group: "css/a".to_owned(),
            page: Page::Screen,
            not_run: None,
            files: vec![path.to_owned()],
        };
        let outcomes = [
            Outcome {
                test: test("css/a/1.html"),
                result: Ok((Status::Pass, String::new())),
                expected: None,
            },
            Outcome {
                test: test("css/a/2.html"),
                result: Ok((Status::Fail, String::new())),
                expected: None,
            },
            Outcome {
                test: test("css/a/3.html"),
                result: Ok((Status::Pass, String::new())),
                expected: Some(Expected::Fail),
            },
            Outcome {
                test: test("css/a/4.html"),
                result: Err(NotRun::Script),
                expected: None,
            },
        ];
        let summary = Summary::new("abc".to_owned(), &["css/a".to_owned()], &outcomes);
        assert_eq!(summary.regressions, ["css/a/2.html"]);
        assert_eq!(summary.unexpected_passes, ["css/a/3.html"]);
        let counts = &summary.directories[0];
        assert_eq!((counts.tests, counts.run, counts.passed), (4, 3, 2));
        assert!(
            summary
                .table()
                .contains("| `css/a` | 2 | 3 | 66.7 % | 1 needs JavaScript |")
        );
        let page = format!("# Compatibility\n\n{DOCS_START}\nold\n{DOCS_END}\nafter\n");
        let updated = update_docs(&page, &summary).unwrap();
        assert!(updated.contains("| **total** | 2 | 3 | 66.7 % | 1 needs JavaScript |"));
        assert!(!updated.contains("old"));
        assert!(updated.ends_with(&format!("{DOCS_END}\nafter\n")));
    }
}

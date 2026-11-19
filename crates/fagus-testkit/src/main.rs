//! `fagus-conformance`: runs the reftests of web-platform-tests and compares the corpus with
//! Chrome. `tools/wpt/run.sh` and `tools/chrome-corpus.sh` are the usual way to call it.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use fagus_testkit::wpt::{self, Expectation, Expectations, Expected, Outcome, Status, Summary};
use fagus_testkit::{corpus, process};

#[derive(Parser)]
#[command(name = "fagus-conformance", about = "Conformance tests for Fagus")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Runs the reftests of web-platform-tests, rendering tests and references with Fagus.
    Wpt {
        /// The checkout of web-platform-tests (tools/wpt/fetch.sh).
        #[arg(long, default_value = "target/wpt/checkout")]
        checkout: PathBuf,
        /// The list of directories to run.
        #[arg(long, default_value = "tools/wpt/dirs.txt")]
        dirs: PathBuf,
        /// The tests expected to fail or skipped.
        #[arg(long, default_value = "tools/wpt/expectations.txt")]
        expectations: PathBuf,
        /// References of our own for tests without one, at the test's path with `-ref`.
        #[arg(long, default_value = "tools/wpt/references")]
        references: PathBuf,
        /// Where the results go.
        #[arg(long, default_value = "target/wpt/results")]
        out: PathBuf,
        /// Only the tests whose path contains this.
        #[arg(long)]
        filter: Option<String>,
        /// Tests rendered at once (default: one per processor).
        #[arg(long)]
        jobs: Option<usize>,
        /// Seconds a test or reference may take to render.
        #[arg(long, default_value_t = 60)]
        timeout: u64,
        /// Writes the expectations of this run into the expectations file.
        #[arg(long)]
        update: bool,
        /// Writes the table of pass rates into this page of the documentation.
        #[arg(long)]
        docs: Option<PathBuf>,
        /// Writes the pages of failing tests and their references as PNG files to
        /// <out>/images.
        #[arg(long)]
        images: bool,
    },
    /// Prints the corpus with Chrome and with Fagus and compares the pages.
    Corpus {
        /// The Chrome binary.
        #[arg(long)]
        chrome: PathBuf,
        /// More arguments for Chrome (repeatable), e.g. --chrome-arg=--no-sandbox.
        #[arg(long = "chrome-arg", allow_hyphen_values = true)]
        chrome_args: Vec<String>,
        /// The tests directory of the repository (with corpus/ and fonts/).
        #[arg(long, default_value = "tests")]
        tests: PathBuf,
        /// Where the PDFs and results go.
        #[arg(long, default_value = "target/chrome-corpus")]
        out: PathBuf,
        /// Documents compared at once (default: up to four).
        #[arg(long)]
        jobs: Option<usize>,
        /// Fagus without the fonts installed on the machine.
        #[arg(long)]
        no_system_fonts: bool,
        /// Uses Chrome's PDFs of an earlier run where they exist.
        #[arg(long)]
        reuse_chrome: bool,
        /// The documents known to differ from Chrome (FAIL) or not to compare (SKIP).
        #[arg(long, default_value = "tools/chrome-expectations.txt")]
        expectations: PathBuf,
        /// Writes the expectations of this run into the expectations file.
        #[arg(long)]
        update: bool,
        /// The share of pixels of a page that may differ.
        #[arg(long, default_value_t = 0.005)]
        page_share: f64,
        /// Only the documents whose path contains this.
        #[arg(long)]
        filter: Option<String>,
    },
}

fn main() -> ExitCode {
    let result = match Cli::parse().command {
        Command::Wpt {
            checkout,
            dirs,
            expectations,
            references,
            out,
            filter,
            jobs,
            timeout,
            update,
            docs,
            images,
        } => run_wpt(&WptOptions {
            sources: wpt::Sources { checkout, references },
            dirs,
            expectations,
            out,
            filter,
            jobs: jobs.unwrap_or_else(process::default_jobs),
            timeout: Duration::from_secs(timeout),
            update,
            docs,
            images,
        }),
        Command::Corpus {
            chrome,
            chrome_args,
            tests,
            out,
            jobs,
            no_system_fonts,
            reuse_chrome,
            expectations,
            update,
            page_share,
            filter,
        } => run_corpus(
            &expectations,
            update,
            corpus::Options {
                tests,
                chrome,
                chrome_args,
                out,
                jobs: jobs.unwrap_or_else(|| process::default_jobs().min(4)),
                system_fonts: !no_system_fonts,
                reuse_chrome,
                dpi: 50,
                threshold: 48,
                page_share,
                filter,
                skip: Vec::new(),
            },
        ),
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("Error: {error:#}");
            ExitCode::from(2)
        }
    }
}

struct WptOptions {
    sources: wpt::Sources,
    dirs: PathBuf,
    expectations: PathBuf,
    out: PathBuf,
    filter: Option<String>,
    jobs: usize,
    timeout: Duration,
    update: bool,
    docs: Option<PathBuf>,
    images: bool,
}

/// Runs the reftests; `Ok(false)` if a test that is expected to pass failed.
fn run_wpt(options: &WptOptions) -> Result<bool> {
    let directories: Vec<String> = std::fs::read_to_string(&options.dirs)
        .with_context(|| format!("reading {}", options.dirs.display()))?
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect();
    let expectations = read_expectations(&options.expectations)?;
    if options.docs.is_some() && options.filter.is_some() {
        bail!("--docs needs a run of all tests, without --filter");
    }
    let commit = commit(&options.sources.checkout)?;
    std::fs::create_dir_all(&options.out)?;
    let collection = wpt::collect(&options.sources, &directories, options.jobs);
    if !collection.unused_references.is_empty() {
        bail!(
            "these references of {} belong to no test without a reference (remove them): {}",
            options.sources.references.display(),
            collection.unused_references.join(", ")
        );
    }
    let tests: Vec<&wpt::Test> = collection
        .tests
        .iter()
        .filter(|test| {
            options
                .filter
                .as_ref()
                .is_none_or(|filter| test.path.contains(filter))
        })
        .collect();
    let to_run: Vec<&wpt::Test> = tests
        .iter()
        .copied()
        .filter(|test| {
            test.not_run.is_none()
                && expectations
                    .get(&test.path)
                    .is_none_or(|expectation| expectation.expected != Expected::Skip)
        })
        .collect();
    let mut renderer = wpt::Renderer::new(&options.sources, &options.out.join("tmp"), options.timeout)?;
    let _ = std::fs::remove_dir_all(options.out.join("images"));
    if options.images {
        renderer.images = Some(options.out.join("images"));
    }
    let version = renderer.rasterizer().version().map_err(anyhow::Error::msg)?;
    eprintln!(
        "web-platform-tests {commit}: {} reftests, {} to run, {} jobs, {version}",
        tests.len(),
        to_run.len(),
        options.jobs
    );
    renderer.plan(&to_run);
    let total = to_run.len();
    let results = process::map(
        &to_run,
        options.jobs,
        |test| {
            let result = wpt::run(&renderer, &collection, test);
            renderer.done(test);
            result
        },
        |done| {
            if done % 500 == 0 || done == total {
                eprintln!("{done} / {total}");
            }
        },
    );
    let mut ran: BTreeMap<&str, (Status, String)> = to_run
        .iter()
        .map(|test| test.path.as_str())
        .zip(results)
        .collect();
    let outcomes: Vec<Outcome> = tests
        .iter()
        .map(|test| {
            let expected = expectations
                .get(&test.path)
                .map(|expectation| expectation.expected);
            let result = match (test.not_run, ran.remove(test.path.as_str())) {
                (Some(reason), _) => Err(reason),
                (None, Some(result)) => Ok(result),
                (None, None) => Err(wpt::NotRun::Skipped),
            };
            Outcome {
                test: (*test).clone(),
                result,
                expected,
            }
        })
        .collect();
    let _ = std::fs::remove_dir_all(options.out.join("tmp"));
    write_results(&options.out, &outcomes)?;
    let summary = Summary::new(commit, &directories, &outcomes);
    std::fs::write(
        options.out.join("summary.json"),
        serde_json::to_string_pretty(&summary)?,
    )?;
    let report = report(&summary, options.filter.is_some());
    std::fs::write(options.out.join("summary.md"), &report)?;
    println!("{report}");
    if let Some(path) = std::env::var_os("GITHUB_STEP_SUMMARY") {
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new().append(true).create(true).open(path)?;
        writeln!(file, "{report}")?;
    }
    let updated = updated(&expectations, &collection, &outcomes, options.filter.is_some());
    std::fs::write(options.out.join("expectations.txt"), updated.write())?;
    if options.update {
        std::fs::write(&options.expectations, updated.write())?;
        eprintln!("updated {}", options.expectations.display());
    }
    if let Some(docs) = &options.docs {
        let page = std::fs::read_to_string(docs).with_context(|| format!("reading {}", docs.display()))?;
        let page = wpt::update_docs(&page, &summary)
            .map_err(|error| anyhow::anyhow!("{}: {error}", docs.display()))?;
        std::fs::write(docs, page)?;
        eprintln!("updated {}", docs.display());
    }
    Ok(options.update || summary.regressions.is_empty())
}

/// The commit of the checkout (a detached `HEAD`, as tools/wpt/fetch.sh leaves it).
fn commit(checkout: &Path) -> Result<String> {
    let head = std::fs::read_to_string(checkout.join(".git/HEAD")).with_context(|| {
        format!(
            "{} is no checkout of web-platform-tests; run tools/wpt/fetch.sh",
            checkout.display()
        )
    })?;
    Ok(head.trim().to_owned())
}

/// Writes the results of each directory to `out/results/<directory>.txt`.
fn write_results(out: &Path, outcomes: &[Outcome]) -> Result<()> {
    let results = out.join("results");
    let _ = std::fs::remove_dir_all(&results);
    let mut files: BTreeMap<&str, String> = BTreeMap::new();
    for outcome in outcomes {
        let text = files.entry(outcome.test.group.as_str()).or_default();
        let (status, detail) = match &outcome.result {
            Ok((status, detail)) => (status.name(), detail.clone()),
            Err(reason) => ("NOTRUN", reason.describe().to_owned()),
        };
        let _ = write!(text, "{status:7} {}", outcome.test.path);
        if outcome.regressed() {
            text.push_str("  REGRESSION");
        } else if outcome.improved() {
            text.push_str("  UNEXPECTED PASS");
        }
        if !detail.is_empty() {
            let _ = write!(text, "  {detail}");
        }
        text.push('\n');
    }
    for (directory, text) in files {
        let path = results.join(format!("{directory}.txt"));
        std::fs::create_dir_all(path.parent().expect("in results"))?;
        std::fs::write(path, text)?;
    }
    Ok(())
}

/// The summary of a run in Markdown.
fn report(summary: &Summary, filtered: bool) -> String {
    let mut text = format!(
        "### Web platform tests{}\n\n{}\n",
        if filtered { " (filtered)" } else { "" },
        summary.table()
    );
    for (title, tests) in [
        (
            "Regressions (expected to pass, but did not)",
            &summary.regressions,
        ),
        (
            "Unexpected passes (update tools/wpt/expectations.txt)",
            &summary.unexpected_passes,
        ),
    ] {
        if !tests.is_empty() {
            let _ = writeln!(text, "\n{title}: {}\n", tests.len());
            for test in tests.iter().take(200) {
                let _ = writeln!(text, "- `{test}`");
            }
        }
    }
    text
}

/// The expectations after this run: failing tests listed, passing ones not, skipped ones kept.
fn updated(
    expectations: &Expectations,
    collection: &wpt::Collection,
    outcomes: &[Outcome],
    filtered: bool,
) -> Expectations {
    let mut updated = expectations.clone();
    if !filtered {
        // Tests that are gone, or no longer reftests.
        updated
            .tests
            .retain(|path, _| collection.tests.iter().any(|test| &test.path == path));
    }
    for outcome in outcomes {
        let Ok((status, _)) = &outcome.result else {
            continue;
        };
        let path = &outcome.test.path;
        if *status == Status::Pass {
            updated.tests.remove(path);
        } else if !updated.tests.contains_key(path) {
            updated.tests.insert(
                path.clone(),
                Expectation {
                    expected: Expected::Fail,
                    comment: None,
                },
            );
        }
    }
    if updated.header.is_empty() {
        updated.header = vec![
            "# The reftests of web-platform-tests that are expected to fail (FAIL) or are not run (SKIP);"
                .to_owned(),
            "# every other test is expected to pass. tools/wpt/run.sh --update rewrites it.".to_owned(),
        ];
    }
    updated
}

/// Compares the corpus with Chrome; `Ok(false)` if a document differs that the expectations do
/// not list as differing.
fn run_corpus(path: &Path, update: bool, mut options: corpus::Options) -> Result<bool> {
    let expectations = read_expectations(path)?;
    options.skip = expectations
        .tests
        .iter()
        .filter(|(_, expectation)| expectation.expected == Expected::Skip)
        .map(|(document, _)| document.clone())
        .collect();
    let version = corpus::chrome_version(&options.chrome);
    eprintln!(
        "{}",
        if version.is_empty() {
            "Chrome (no version)"
        } else {
            &version
        }
    );
    let comparisons = corpus::compare(&options)?;
    let (lines, summary) = corpus::report(&comparisons, options.page_share, &version, &expectations);
    std::fs::write(options.out.join("results.txt"), &lines)?;
    std::fs::write(options.out.join("summary.md"), &summary)?;
    let _ = std::fs::remove_dir_all(options.out.join("tmp"));
    print!("{lines}\n{summary}");
    if let Some(summary_path) = std::env::var_os("GITHUB_STEP_SUMMARY") {
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(summary_path)?;
        writeln!(file, "{summary}")?;
    }
    let verdict = corpus::judge(&comparisons, options.page_share, &expectations);
    // On GitHub Actions, these lines become annotations of the run.
    for document in &verdict.now_matching {
        println!(
            "::warning::{document} matches Chrome now; remove it from {}",
            path.display()
        );
    }
    for document in &verdict.new_differences {
        println!(
            "::error::{document} differs from Chrome and is not listed in {}",
            path.display()
        );
    }
    let mut updated = corpus::updated(&expectations, &comparisons, options.page_share);
    if options.filter.is_some() {
        updated = expectations.clone();
    }
    if updated.header.is_empty() {
        updated.header = vec![
            "# The documents of tests/corpus that are known to differ from Chrome (FAIL), with the reason,"
                .to_owned(),
            "# or that are not compared (SKIP). tools/chrome-corpus.sh fails on any other document that"
                .to_owned(),
            "# differs; --update rewrites the list.".to_owned(),
        ];
    }
    std::fs::write(options.out.join("expectations.txt"), updated.write())?;
    if update && options.filter.is_none() {
        std::fs::write(path, updated.write())?;
        eprintln!("updated {}", path.display());
        return Ok(true);
    }
    Ok(verdict.new_differences.is_empty())
}

/// The expectations file at `path`, empty if there is none.
fn read_expectations(path: &Path) -> Result<Expectations> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            Expectations::parse(&text).map_err(|error| anyhow::anyhow!("{}: {error}", path.display()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Expectations::default()),
        Err(error) => Err(error).context(format!("reading {}", path.display())),
    }
}

//! The heap of one render of a document, through dhat (as the global allocator).
//!
//!     cargo run --release -- peak invoice.html dhat-heap.json   # what is held at the peak
//!     cargo run --release -- phases invoice.html                # the heap over the phases
//!     cargo run --release -- sizes                              # the size of a computed style
//!
//! `peak` writes dhat's profile, which `peak.py` sums by the code that allocated what is held
//! at the peak; dh_view.html of the dhat repository shows it too. The fonts are those of
//! tests/fonts. Allocations are slow under dhat: a 100-page invoice takes minutes.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

const MB: f64 = 1024.0 * 1024.0;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let usage = "usage: fagus-heap peak DOCUMENT OUT.json | phases DOCUMENT | sizes";
    match args.get(1).map(String::as_str) {
        Some("sizes") => println!(
            "ComputedStyle: {} bytes, the ComputedValues it shares: {} bytes",
            std::mem::size_of::<fagus_style::ComputedStyle>(),
            std::mem::size_of::<fagus_style::ComputedValues>()
        ),
        Some("peak") if args.len() == 4 => peak(Path::new(&args[2]), &args[3]),
        Some("phases") if args.len() == 3 => phases(Path::new(&args[2])),
        _ => eprintln!("{usage}"),
    }
}

fn engine_and_document(path: &Path) -> (fagus::Engine, fagus::Document) {
    let mut engine = fagus::Engine::new();
    engine
        .add_font_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../tests/fonts"))
        .expect("the fonts of tests/fonts");
    let html = std::fs::read_to_string(path).expect("the document");
    let base = path.parent().unwrap_or(Path::new("."));
    (engine, fagus::Document::html(html).base_directory(base))
}

/// Renders the document under dhat's heap profiler and writes its profile to `out`.
fn peak(path: &Path, out: &str) {
    let (engine, document) = engine_and_document(path);
    let _profiler = dhat::Profiler::builder().file_name(out).build();
    let output = engine.render(&document).expect("rendered");
    let stats = dhat::HeapStats::get();
    eprintln!(
        "{} pages; heap at its peak {:.1} MB",
        output.pages,
        stats.max_bytes as f64 / MB
    );
}

/// Renders the document while a thread samples the heap, and prints the largest heap in each
/// phase of the render and at its end, by the render's own timings.
fn phases(path: &Path) {
    let (engine, document) = engine_and_document(path);
    let _profiler = dhat::Profiler::builder().testing().build();
    let samples = Arc::new(Mutex::new(Vec::new()));
    let stop = Arc::new(AtomicBool::new(false));
    let start = Instant::now();
    let sampler = {
        let (samples, stop) = (Arc::clone(&samples), Arc::clone(&stop));
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let heap = dhat::HeapStats::get().curr_bytes;
                samples.lock().expect("samples").push((start.elapsed(), heap));
                std::thread::sleep(Duration::from_micros(500));
            }
        })
    };
    let begin = start.elapsed();
    let output = engine.render(&document).expect("rendered");
    stop.store(true, Ordering::Relaxed);
    sampler.join().expect("sampler");
    let samples = samples.lock().expect("samples");
    let timings = &output.timings;
    let mut from = begin;
    for (name, took) in [
        ("parse", timings.parse),
        ("style", timings.style),
        ("layout", timings.layout),
        ("paint", timings.paint),
        ("pdf", timings.pdf),
    ] {
        let to = from + took;
        let within: Vec<usize> = samples
            .iter()
            .filter(|(at, _)| *at >= from && *at < to)
            .map(|(_, heap)| *heap)
            .collect();
        let most = within.iter().copied().max().unwrap_or(0);
        let end = within.last().copied().unwrap_or(0);
        println!(
            "{name:7} {:9.1} ms  heap at most {:6.1} MB, at the end {:6.1} MB",
            took.as_secs_f64() * 1000.0,
            most as f64 / MB,
            end as f64 / MB
        );
        from = to;
    }
    println!("{} pages", output.pages);
}

//! Renders arbitrary markup: HTML, or XHTML when it starts with an XML declaration.
#![no_main]

use fagus::Document;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let markup = fagus_fuzz::text(data).into_owned();
    let document = if markup.starts_with("<?xml") {
        Document::xhtml(markup)
    } else {
        Document::html(markup)
    };
    fagus_fuzz::render(&document);
});

//! Reads arbitrary PDF data (its cross-reference table or stream, objects, page tree, encryption
//! and content streams) as a page template, its second page as an element, as a border image and
//! with a filter, and writes its pages into the PDF as form `XObject`s.
#![no_main]

use fagus::Document;
use libfuzzer_sys::fuzz_target;

const HTML: &str = "<style>@page { size: 200px; margin: 20px; background: url(i.pdf) 0 0 / 100% 100% }
    @page :nth(2n) { background-image: url('i.pdf#page=2') }</style>\
    <img src='i.pdf#page=2'><img src=i.pdf style='width: 20px; filter: drop-shadow(1px 1px red)'>\
    <div style='width: 10px; border: 5px solid; border-image: url(i.pdf) 30%; break-after: page'></div>\
    <p>Two";

fuzz_target!(|data: &[u8]| {
    // As a PDF, whatever its first bytes are (seeds have the header, and their offsets stay).
    let mut pdf = Vec::with_capacity(data.len() + 9);
    if !data.starts_with(b"%PDF-") {
        pdf.extend_from_slice(b"%PDF-1.7\n");
    }
    pdf.extend_from_slice(data);
    fagus_fuzz::render(&Document::html(HTML).resource("i.pdf", pdf));
});

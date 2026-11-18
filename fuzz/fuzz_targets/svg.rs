//! Renders arbitrary SVG: as an image file (an `<img>`, a background) and inline in HTML,
//! which takes different ways through the engine.
#![no_main]

use fagus::Document;
use libfuzzer_sys::fuzz_target;

const HTML: &str = "<img src=s.svg><img src=s.svg style='width: 20px; filter: blur(1px)'>\
    <div style='width: 20px; height: 20px; background: url(s.svg) 0 0 / 10px; mask: url(s.svg)'></div>\
    <svg width=20 height=20><use href='s.svg#a'/></svg>";

fuzz_target!(|data: &[u8]| {
    fagus_fuzz::render(&Document::html(HTML).resource("s.svg", data.to_vec()));
    let svg = fagus_fuzz::text(data);
    fagus_fuzz::render(&Document::html(format!("<p>x</p>{svg}<p>y</p>")));
});

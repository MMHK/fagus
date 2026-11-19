//! Decodes arbitrary AVIF data (the HEIF container and the AV1 pictures in it) as the image of
//! an element, an image whose pixels a filter changes and an image in SVG.
#![no_main]

use fagus::Document;
use libfuzzer_sys::fuzz_target;

const HTML: &str = "<img src=i><img src=i style='width: 20px; image-orientation: none; filter: blur(1px)'>\
    <svg width=20 height=20><image href=i width=20 height=20 /></svg>";

fuzz_target!(|data: &[u8]| {
    fagus_fuzz::render(&Document::html(HTML).resource("i", data.to_vec()));
});

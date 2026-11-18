//! Loads arbitrary font data (TrueType, OpenType, collections, WOFF and WOFF2), shapes text with
//! its faces, and renders a document that uses it through `@font-face`, which subsets the font
//! into the PDF.
#![no_main]

use fagus::Document;
use libfuzzer_sys::fuzz_target;

const TEXT: &str = "Hallo Welt! fi ffl 0123 €ÄÖÜß Ωλ Привет مرحبا שלום नमस्ते 漢字 🙂\u{301}";

const HTML: &str = "<style>@font-face { font-family: F; src: url(f) } \
    @font-face { font-family: V; src: url(f); font-weight: 100 900; font-style: oblique 0deg 20deg } \
    body { font-family: F } .v { font-family: V; font-weight: 650; font-style: italic; \
    font-variation-settings: 'wdth' 80; font-feature-settings: 'smcp', 'liga' 0 } \
    .s { text-shadow: 0 0 2px red; text-decoration: underline; font-synthesis: weight style } \
    .vt { writing-mode: vertical-rl; text-orientation: upright }</style>\
    <p>Hallo Welt! fi ffl 0123 €ÄÖÜß</p><p class=v>Ωλ Привет مرحبا 🙂</p>\
    <p class=s><b>bold</b> <i>italic</i> <small style='font-variant: small-caps'>Caps</small></p>\
    <div class=vt>縦書き abc</div><math><mi>x</mi><mo>∑</mo></math>";

fuzz_target!(|data: &[u8]| {
    let mut fonts = fagus_text::FontDb::new();
    if let Ok(faces) = fonts.add_font_data(data.to_vec()) {
        for face in faces {
            let shaped = fonts.shape(face, TEXT);
            for glyph in &shaped.glyphs {
                let _ = fonts.face(face).glyph_bounds(glyph.id, 12.0);
            }
        }
    }
    fagus_fuzz::render(&Document::html(HTML).resource("f", data.to_vec()));
});

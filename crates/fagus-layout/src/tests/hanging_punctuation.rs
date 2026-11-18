//! `hanging-punctuation` (CSS Text 3, §8.2): punctuation at the start of the first formatted
//! line and at the end of lines that is not measured for fit, alignment or justification and
//! stands outside the line box. Chrome does not support it; Safari does.

use super::render;
use crate::fragment::{Fragment, FragmentKind};

/// The glyphs of a page's text in the order they stand: the character each starts, its left
/// and right edge (top and bottom in vertical text) and the top (left) of its line.
fn glyphs(fragment: &Fragment) -> Vec<(char, f32, f32, f32)> {
    fn walk(fragment: &Fragment, (x, y): (f32, f32), out: &mut Vec<(char, f32, f32, f32)>) {
        let (x, y) = (x + fragment.rect.x.to_px(), y + fragment.rect.y.to_px());
        if let FragmentKind::Text(run) = &fragment.kind {
            let vertical = run.style.writing_mode.is_vertical();
            let mut at = if vertical { y } else { x };
            for glyph in &run.glyphs {
                let character = run.text[glyph.cluster.0 as usize..].chars().next().unwrap_or(' ');
                out.push((character, at, at + glyph.advance, if vertical { x } else { y }));
                at += glyph.advance;
            }
        }
        for child in &fragment.children {
            walk(child, (x, y), out);
        }
    }
    let mut out = Vec::new();
    walk(fragment, (0.0, 0.0), &mut out);
    out
}

/// The glyphs of the first page of a paragraph `<p>` with `content` and the style `p`.
fn paragraph(p: &str, content: &str) -> Vec<(char, f32, f32, f32)> {
    let pages = render(&format!(
        "<style>@page {{ margin: 0 }} body {{ margin: 0; font: 20px/1 Inter }} p {{ margin: 0; {p} }}</style><p>{content}</p>"
    ));
    glyphs(&pages[0].fragments[0])
}

/// The number of lines of `glyphs`.
fn line_count(glyphs: &[(char, f32, f32, f32)]) -> usize {
    let mut tops: Vec<i32> = glyphs.iter().map(|glyph| glyph.3.round() as i32).collect();
    tops.dedup();
    tops.len()
}

/// The glyph of `character`, the `nth` of them.
fn glyph(glyphs: &[(char, f32, f32, f32)], character: char, nth: usize) -> (char, f32, f32, f32) {
    *glyphs
        .iter()
        .filter(|glyph| glyph.0 == character)
        .nth(nth)
        .unwrap_or_else(|| panic!("{character} in {glyphs:?}"))
}

/// Whether two positions are the same, but for rounding to layout units.
fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.05
}

#[test]
fn hangs_an_opening_quote_at_the_start_of_the_first_line_only() {
    let text = "\u{201C}Ab \u{201C}Cd";
    let plain = paragraph("width: 60px", text);
    let hung = paragraph("width: 60px; hanging-punctuation: first", text);
    assert_eq!(line_count(&plain), 2, "{plain:?}");
    let quote = glyph(&plain, '\u{201C}', 0);
    let width = quote.2 - quote.1;
    assert!(width > 0.0);
    // The first quote stands left of the line box, the text where the quote stood.
    let first = glyph(&hung, '\u{201C}', 0);
    assert!(close(first.1, -width), "{hung:?}");
    assert!(close(glyph(&hung, 'A', 0).1, 0.0), "{hung:?}");
    // The second line is no first formatted line: its quote takes room on it.
    assert_eq!(line_count(&hung), 2, "{hung:?}");
    assert!(close(glyph(&hung, '\u{201C}', 1).1, 0.0), "{hung:?}");
    // Nothing changes without the property.
    assert!(close(quote.1, 0.0));
}

#[test]
fn hangs_into_the_indent_and_gives_the_first_line_the_room() {
    // A line just as wide as the text without its opening quote holds it all.
    let text = "\u{201C}Ab cd";
    let plain = paragraph("width: max-content", text);
    let width = glyph(&plain, 'd', 0).2 - glyph(&plain, '\u{201C}', 0).2;
    let style = format!("width: {width}px");
    assert_eq!(line_count(&paragraph(&style, text)), 2);
    let hung = paragraph(&format!("{style}; hanging-punctuation: first"), text);
    assert_eq!(line_count(&hung), 1, "{hung:?}");
    // With an indent, the quote hangs in it.
    let indented = paragraph(
        &format!("{style}; text-indent: 30px; hanging-punctuation: first"),
        text,
    );
    let quote = glyph(&plain, '\u{201C}', 0);
    assert!(
        close(glyph(&indented, '\u{201C}', 0).1, 30.0 - (quote.2 - quote.1)),
        "{indented:?}"
    );
}

#[test]
fn hangs_the_closing_quote_of_the_last_line_only() {
    let text = "Ab\u{201D} cd\u{201D}";
    let style = "width: 60px; text-align: right; hanging-punctuation: last";
    let hung = paragraph(style, text);
    assert_eq!(line_count(&hung), 2, "{hung:?}");
    // The quote that ends the first line takes room on it; that of the last hangs.
    assert!(close(glyph(&hung, '\u{201D}', 0).2, 60.0), "{hung:?}");
    assert!(close(glyph(&hung, 'd', 0).2, 60.0), "{hung:?}");
    assert!(glyph(&hung, '\u{201D}', 1).2 > 60.5, "{hung:?}");
}

#[test]
fn forces_stops_and_commas_to_hang_at_the_end_of_every_line() {
    let text = "Ab, cd.";
    let hung = paragraph(
        "width: 60px; text-align: right; hanging-punctuation: force-end",
        text,
    );
    assert_eq!(line_count(&hung), 2, "{hung:?}");
    assert!(close(glyph(&hung, 'b', 0).2, 60.0), "{hung:?}");
    assert!(close(glyph(&hung, 'd', 0).2, 60.0), "{hung:?}");
    let comma = glyph(&hung, ',', 0);
    assert!(close(comma.1, 60.0), "{hung:?}");
    // Justified, the line is expanded without the stop, which is pushed past its end.
    let text = "Ab cd ef, gh";
    let justified = paragraph(
        "width: 90px; text-align: justify; hanging-punctuation: force-end",
        text,
    );
    assert_eq!(line_count(&justified), 2, "{justified:?}");
    assert!(close(glyph(&justified, 'f', 0).2, 90.0), "{justified:?}");
    assert!(close(glyph(&justified, ',', 0).1, 90.0), "{justified:?}");
}

#[test]
fn lets_stops_hang_with_allow_end_only_where_they_do_not_fit() {
    let text = "Ab cd.";
    let plain = paragraph("width: max-content", text);
    let stop = glyph(&plain, '.', 0);
    let fitting = paragraph(
        "width: 200px; text-align: right; hanging-punctuation: allow-end",
        text,
    );
    assert!(close(glyph(&fitting, '.', 0).2, 200.0), "{fitting:?}");
    // A line as wide as the text without the stop holds it, which hangs as far as it does not
    // fit, here all of it.
    let width = stop.1;
    let allowed = paragraph(
        &format!("width: {width}px; text-align: right; hanging-punctuation: allow-end"),
        text,
    );
    assert_eq!(line_count(&allowed), 1, "{allowed:?}");
    assert!(close(glyph(&allowed, '.', 0).1, width), "{allowed:?}");
    // Half of it does not fit: it hangs by that half.
    let wider = width + (stop.2 - stop.1) / 2.0;
    let half = paragraph(
        &format!("width: {wider}px; text-align: right; hanging-punctuation: allow-end"),
        text,
    );
    assert!(close(glyph(&half, '.', 0).2, stop.2), "{half:?}");
    // `force-end` hangs it all the same.
    let forced = paragraph(
        &format!("width: {wider}px; text-align: right; hanging-punctuation: force-end"),
        text,
    );
    assert!(close(glyph(&forced, '.', 0).1, wider), "{forced:?}");
}

#[test]
fn hangs_nothing_behind_padding_or_after_other_punctuation() {
    let style = "width: 200px; text-align: right; hanging-punctuation: force-end last";
    // End padding of the inline box between the stop and the end of the line.
    let padded = paragraph(style, "Ab <span style='padding-right: 4px'>cd.</span>");
    assert!(close(glyph(&padded, '.', 0).2, 196.0), "{padded:?}");
    // Without padding, through the end of the box.
    let boxed = paragraph(style, "Ab <span>cd.</span>");
    assert!(close(glyph(&boxed, '.', 0).1, 200.0), "{boxed:?}");
    // At most one mark hangs at each edge: the quote after the stop, not the stop.
    let both = paragraph(style, "Ab cd.\u{201D}");
    assert!(close(glyph(&both, '\u{201D}', 0).1, 200.0), "{both:?}");
    assert!(close(glyph(&both, '.', 0).2, 200.0), "{both:?}");
    // Nor does other punctuation.
    let other = paragraph(style, "Ab cd!");
    assert!(close(glyph(&other, '!', 0).2, 200.0), "{other:?}");
}

#[test]
fn hangs_at_the_edges_of_right_to_left_lines() {
    // The start of a right-to-left line is on the right: the opening quote hangs there, the
    // stop at its end on the left.
    let hung = paragraph(
        "width: 200px; direction: rtl; text-align: left; hanging-punctuation: first force-end",
        "\u{201C}Ab cd.",
    );
    let quote = glyph(&hung, '\u{201C}', 0);
    let stop = glyph(&hung, '.', 0);
    assert!(stop.2 <= 0.02 && stop.1 < -0.5, "{hung:?}");
    let left = hung.iter().map(|glyph| glyph.1).fold(f32::INFINITY, f32::min);
    assert!(close(left, stop.1), "{hung:?}");
    // Aligned to the right, the quote hangs past the right edge.
    let right = paragraph(
        "width: 200px; direction: rtl; hanging-punctuation: first force-end",
        "\u{201C}Ab cd.",
    );
    let quote_right = glyph(&right, '\u{201C}', 0);
    assert!(close(quote_right.1, 200.0), "{right:?}");
    assert!(quote.2 - quote.1 > 0.0);
}

#[test]
fn leaves_hanging_punctuation_out_of_the_intrinsic_widths() {
    let width = |p: &str| {
        let glyphs = paragraph(&format!("width: max-content; {p}"), "\u{201C}Ab cd.");
        glyphs
            .iter()
            .map(|glyph| glyph.2)
            .fold(f32::NEG_INFINITY, f32::max)
            - glyphs.iter().map(|glyph| glyph.1).fold(f32::INFINITY, f32::min)
    };
    let plain = paragraph("width: max-content", "\u{201C}Ab cd.");
    let quote = glyph(&plain, '\u{201C}', 0);
    let stop = glyph(&plain, '.', 0);
    let all = width("");
    // The box is as wide as the text without the marks, which reach out of it.
    let hung = paragraph(
        "width: max-content; hanging-punctuation: first force-end; text-align: right",
        "\u{201C}Ab cd.",
    );
    let inner = all - (quote.2 - quote.1) - (stop.2 - stop.1);
    assert!(close(glyph(&hung, 'A', 0).1, 0.0), "{hung:?}");
    assert!(close(glyph(&hung, 'd', 0).2, inner), "{hung:?}");
    // `allow-end` counts for the max-content width.
    let allowed = paragraph(
        "width: max-content; hanging-punctuation: allow-end; text-align: right",
        "\u{201C}Ab cd.",
    );
    assert!(close(glyph(&allowed, '.', 0).2, all), "{allowed:?}");
    // And neither counts for the min-content width.
    // The longest word is as wide as the box without its stop, which hangs.
    let narrow = paragraph(
        "width: min-content; hanging-punctuation: allow-end; text-align: right",
        "Ab defg.",
    );
    assert!(close(glyph(&narrow, 'd', 0).1, 0.0), "{narrow:?}");
    assert!(
        close(glyph(&narrow, 'b', 0).2, glyph(&narrow, 'g', 0).2),
        "{narrow:?}"
    );
}

#[test]
fn hangs_at_the_end_of_vertical_lines() {
    let hung = paragraph(
        "writing-mode: vertical-rl; height: 200px; text-align: end; hanging-punctuation: force-end",
        "Ab cd.",
    );
    assert!(close(glyph(&hung, 'd', 0).2, 200.0), "{hung:?}");
    assert!(close(glyph(&hung, '.', 0).1, 200.0), "{hung:?}");
}

#[test]
fn hangs_no_mark_that_bidi_text_moves_away_from_the_edge() {
    // The stop between two Hebrew words is right to left with them: on the line it stands left
    // of the first word, not at the end of the left-to-right line, and does not hang.
    let text = "ab \u{5D0}\u{5D1}. \u{5D2}\u{5D3}";
    let hung = paragraph(
        "width: 70px; text-align: right; hanging-punctuation: force-end",
        text,
    );
    assert_eq!(line_count(&hung), 2, "{hung:?}");
    let top = hung[0].3;
    let right = hung
        .iter()
        .filter(|glyph| (glyph.3 - top).abs() < 0.5 && glyph.0 != ' ')
        .map(|glyph| glyph.2)
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(close(right, 70.0), "{hung:?}");
}

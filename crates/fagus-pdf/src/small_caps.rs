//! `font-variant: small-caps` in SVG text set in a face without small capitals: usvg asks the
//! face for its `smcp` glyphs and sets lowercase letters where it has none; Chrome makes small
//! capitals up, as in HTML, from the capitals at 70 % of the font size, rounded (Blink's
//! `SmallCapsIterator`). The spans of the text as usvg writes it out are split into runs of
//! such capitals and of the other characters. The runs of capitals keep their lowercase letters,
//! as Chrome keeps them for copying, and are set in the face with its lowercase letters mapped
//! to its capitals (see [`capitalized`]), which the family [`CAPITALS`] before the span's own
//! asks for.

use std::fmt::Write as _;

use fagus_text::FontDb;

/// The family that asks for the capitalized variant of the face the families after it select.
pub(crate) const CAPITALS: &str = "fagus-small-capitals";

/// Whether a text of the tree has a span in small capitals.
pub(crate) fn needed(group: &usvg::Group) -> bool {
    group.children().iter().any(|node| match node {
        usvg::Node::Group(group) => needed(group),
        usvg::Node::Text(text) => text
            .chunks()
            .iter()
            .any(|chunk| chunk.spans().iter().any(usvg::TextSpan::small_caps)),
        _ => false,
    })
}

/// The SVG that usvg wrote out (with its text kept) with its spans in small capitals, in faces
/// without them, made up of capitals.
pub(crate) fn synthesize(written: &str, fonts: &FontDb) -> String {
    let mut out = String::with_capacity(written.len() + 256);
    let mut rest = written;
    while let Some(start) = rest.find("<tspan") {
        let Some(tag_end) = rest[start..].find('>').map(|end| start + end) else {
            break;
        };
        let tag = &rest[start + 1..tag_end];
        let text_end = rest[tag_end..].find('<').map_or(rest.len(), |end| tag_end + end);
        let made_up = crate::svg::attribute(tag, "font-variant") == Some("small-caps")
            && rest[text_end..].starts_with("</tspan>")
            && crate::svg::span_font(tag, fonts)
                .is_some_and(|(face, ..)| !fonts.face(face).substitutes(*b"smcp"));
        if !made_up {
            out.push_str(&rest[..=tag_end]);
            rest = &rest[tag_end + 1..];
            continue;
        }
        out.push_str(&rest[..start]);
        split(tag, &rest[tag_end + 1..text_end], &mut out);
        rest = &rest[text_end + "</tspan>".len()..];
    }
    out.push_str(rest);
    out
}

/// Writes the span with the start tag `tag` (without `<` and `>`) and the escaped text `text`
/// as spans of runs: letters that have a capital in the capitalized face at the smaller size,
/// the other characters as they are.
fn split(tag: &str, text: &str, out: &mut String) {
    let mut tag = tag.to_owned();
    if let Some((span, _)) = crate::svg::attribute_at(&tag, "font-variant") {
        tag.replace_range(span, "");
    }
    let size: f32 = crate::svg::attribute(&tag, "font-size")
        .and_then(|size| size.parse().ok())
        .unwrap_or(16.0);
    let mut small_tag = match crate::svg::attribute_at(&tag, "font-size") {
        Some((_, value)) => {
            let mut small = tag.clone();
            small.replace_range(value, &(size * 0.7).round().to_string());
            small
        }
        None => format!("{tag} font-size=\"{}\"", (size * 0.7).round()),
    };
    match crate::svg::attribute_at(&small_tag, "font-family") {
        Some((_, value)) => small_tag.insert_str(value.start, &format!("{CAPITALS}, ")),
        None => {
            let _ = write!(small_tag, " font-family=\"{CAPITALS}, {}\"", fagus_text::STANDARD);
        }
    }
    // Runs of characters, an entity counting as one that has no capital.
    let mut runs: Vec<(bool, String)> = Vec::new();
    let mut rest = text;
    while let Some(character) = rest.chars().next() {
        let (piece, small) = if character == '&' {
            let end = rest.find(';').map_or(1, |end| end + 1);
            (rest[..end].to_owned(), false)
        } else {
            (character.to_string(), capital(character).is_some())
        };
        let consumed = if character == '&' {
            piece.len()
        } else {
            character.len_utf8()
        };
        match runs.last_mut() {
            Some((last, run)) if *last == small => run.push_str(&piece),
            _ => runs.push((small, piece)),
        }
        rest = &rest[consumed..];
    }
    for (small, run) in runs {
        let _ = write!(out, "<{}>{run}</tspan>", if small { &small_tag } else { &tag });
    }
}

/// The capital of a letter, where it is one other character.
fn capital(character: char) -> Option<char> {
    let mut upper = character.to_uppercase();
    match (upper.next(), upper.next()) {
        (Some(capital), None) if capital != character => Some(capital),
        _ => None,
    }
}

/// The font `data` (in a collection, its face `index`) with its letters that have a capital
/// mapped to the glyph of that capital: a `cmap` table of its own appended, which the face's
/// table directory points to. `None` for fonts without a character map.
pub(crate) fn capitalized(data: &[u8], index: u32) -> Option<Vec<u8>> {
    use skrifa::MetadataProvider;
    let read = |at: usize, length: usize| -> Option<u32> {
        let bytes = data.get(at..at + length)?;
        Some(
            bytes
                .iter()
                .fold(0, |value, byte| (value << 8) | u32::from(*byte)),
        )
    };
    // Where the face's table directory starts.
    let directory = if data.starts_with(b"ttcf") {
        read(12 + 4 * index as usize, 4)? as usize
    } else {
        0
    };
    let font = skrifa::FontRef::from_index(data, index).ok()?;
    let charmap = font.charmap();
    let mut mappings: Vec<(u32, u32)> = charmap
        .mappings()
        .map(|(character, glyph)| {
            let glyph = char::from_u32(character)
                .and_then(capital)
                .and_then(|capital| charmap.map(capital))
                .unwrap_or(glyph);
            (character, glyph.to_u32())
        })
        .collect();
    mappings.sort_unstable();
    mappings.dedup_by_key(|(character, _)| *character);
    // Groups of characters and glyphs that go up together (format 12).
    let mut groups: Vec<(u32, u32, u32)> = Vec::new();
    for (character, glyph) in mappings {
        match groups.last_mut() {
            Some((start, end, start_glyph))
                if *end + 1 == character && *start_glyph + (character - *start) == glyph =>
            {
                *end = character;
            }
            _ => groups.push((character, character, glyph)),
        }
    }
    let count = u32::try_from(groups.len()).ok()?;
    let subtable_length = 16 + 12 * count;
    let mut cmap = Vec::with_capacity(20 + subtable_length as usize);
    // Version 0, two encodings (Unicode full and Windows UCS-4), both at the one subtable.
    cmap.extend_from_slice(&[0, 0, 0, 2]);
    cmap.extend_from_slice(&[0, 0, 0, 4, 0, 0, 0, 20]);
    cmap.extend_from_slice(&[0, 3, 0, 10, 0, 0, 0, 20]);
    cmap.extend_from_slice(&[0, 12, 0, 0]);
    cmap.extend_from_slice(&subtable_length.to_be_bytes());
    cmap.extend_from_slice(&[0, 0, 0, 0]);
    cmap.extend_from_slice(&count.to_be_bytes());
    for (start, end, glyph) in groups {
        cmap.extend_from_slice(&start.to_be_bytes());
        cmap.extend_from_slice(&end.to_be_bytes());
        cmap.extend_from_slice(&glyph.to_be_bytes());
    }
    let tables = read(directory + 4, 2)? as usize;
    let record = (0..tables)
        .map(|table| directory + 12 + 16 * table)
        .find(|&record| data.get(record..record + 4) == Some(b"cmap"))?;
    let mut out = data.to_vec();
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
    let offset = u32::try_from(out.len()).ok()?;
    let length = u32::try_from(cmap.len()).ok()?;
    out.extend_from_slice(&cmap);
    out[record + 8..record + 12].copy_from_slice(&offset.to_be_bytes());
    out[record + 12..record + 16].copy_from_slice(&length.to_be_bytes());
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makes_small_capitals_up() {
        let mut out = String::new();
        split(
            "tspan font-family=\"Inter\" font-size=\"24\" font-variant=\"small-caps\"",
            "Small &amp; ß",
            &mut out,
        );
        // The lowercase letters stay, in the capitalized face; `ß` has no single capital: it
        // stays as it is, like the entity.
        assert_eq!(
            out,
            "<tspan font-family=\"Inter\" font-size=\"24\">S</tspan>\
             <tspan font-family=\"fagus-small-capitals, Inter\" font-size=\"17\">mall</tspan>\
             <tspan font-family=\"Inter\" font-size=\"24\"> &amp; ß</tspan>"
        );
    }

    #[test]
    fn keeps_the_lowercase_text_of_made_up_capitals() {
        fn walk(group: &usvg::Group, texts: &mut Vec<String>) {
            for node in group.children() {
                match node {
                    usvg::Node::Group(group) => walk(group, texts),
                    usvg::Node::Text(text) => texts.extend(
                        text.layouted()
                            .iter()
                            .flat_map(|span| span.positioned_glyphs.iter().map(|glyph| glyph.text.clone())),
                    ),
                    _ => {}
                }
            }
        }
        let data = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fonts/Inter-Regular.ttf"
        ))
        .expect("font");
        // The capitalized face draws `a` with the glyph of `A`.
        let capitalized = capitalized(&data, 0).expect("a character map");
        let glyph = |data: &[u8], character: char| {
            use skrifa::MetadataProvider;
            skrifa::FontRef::new(data)
                .expect("font")
                .charmap()
                .map(character)
                .expect("glyph")
        };
        assert_eq!(glyph(&capitalized, 'a'), glyph(&data, 'A'));
        assert_eq!(glyph(&capitalized, 'A'), glyph(&data, 'A'));
        assert_eq!(glyph(&capitalized, '1'), glyph(&data, '1'));
        // SVG text in small capitals keeps its lowercase letters as the glyphs' text.
        let mut fonts = FontDb::new();
        fonts.add_font_data(data).expect("Inter");
        let svg = "<svg xmlns='http://www.w3.org/2000/svg' width='200' height='40'>\
                   <text y='30' font-family='Inter' font-size='24' font-variant='small-caps'>Small</text></svg>";
        let tree = crate::svg::parse(svg.as_bytes(), &fonts).expect("svg");
        let mut texts = Vec::new();
        walk(tree.root(), &mut texts);
        assert_eq!(texts.concat(), "Small");
    }
}

//! `text-anchor: middle` and `end` with `dx` inside a text chunk or `letter-spacing`: Chrome
//! counts the shifts of the characters after the first (an inner `<tspan dx>`) and the spacing
//! after the last character in the advance it anchors, usvg neither, so the chunk lands the
//! shifts and the spacing (half of them for `middle`) too far along. The text as usvg writes it
//! out is corrected by moving the chunk's first character back by as much.

use std::fmt::Write as _;

/// Whether a text of the tree has a chunk anchored at its middle or end with shifts after its
/// first character or spacing after its last, which [`fix`] corrects.
pub(crate) fn needs_fix(group: &usvg::Group) -> bool {
    group.children().iter().any(|node| match node {
        usvg::Node::Group(group) => needs_fix(group),
        usvg::Node::Text(text) => {
            text.writing_mode() == usvg::WritingMode::LeftToRight && {
                let mut start = 0;
                text.chunks().iter().any(|chunk| {
                    let count = chunk.text().chars().count();
                    let inner = text.dx().get(start + 1..start + count).unwrap_or_default();
                    start += count;
                    let end = chunk.text().len();
                    let spaced = chunk
                        .spans()
                        .iter()
                        .any(|span| span.end() == end && span.letter_spacing() != 0.0);
                    chunk.anchor() != usvg::TextAnchor::Start && (spaced || inner.iter().any(|dx| *dx != 0.0))
                })
            }
        }
        _ => false,
    })
}

/// The SVG that usvg wrote out (with its text kept) with the first character of each chunk
/// anchored at its middle or end moved back by the shifts of the characters after it and the
/// spacing after its last (half of them for `middle`).
pub(crate) fn fix(written: &str) -> String {
    let mut out = String::with_capacity(written.len());
    let mut rest = written;
    while let Some(start) = rest.find("<text") {
        let Some(end) = rest[start..]
            .find("</text>")
            .map(|end| start + end + "</text>".len())
        else {
            break;
        };
        out.push_str(&rest[..start]);
        let element = &rest[start..end];
        match fix_text(element) {
            Some(fixed) => out.push_str(&fixed),
            None => out.push_str(element),
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// A `<text>` element as usvg writes it, corrected; `None` if nothing changes.
fn fix_text(element: &str) -> Option<String> {
    let open_end = element.find('>')?;
    let open = &element[..open_end];
    let chunks = chunks(&element[open_end + 1..]);
    let (span, mut dx) = if let Some((span, value)) = crate::svg::attribute_at(open, "dx") {
        let dx: Vec<f32> = open[value]
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|part| !part.is_empty())
            .map(str::parse)
            .collect::<Result<_, _>>()
            .ok()?;
        (span, dx)
    } else {
        // Without shifts of its own, the text gets one for each character.
        let after_name = open.find(char::is_whitespace).unwrap_or(open.len());
        (
            after_name..after_name,
            vec![0.0; chunks.iter().map(|chunk| chunk.0).sum()],
        )
    };
    let mut changed = false;
    let mut start = 0;
    for (count, factor, spacing) in chunks {
        let end = (start + count).min(dx.len());
        if factor > 0.0 && start < end {
            let inner: f32 = dx[start + 1..end].iter().sum::<f32>() + spacing;
            if inner != 0.0 {
                dx[start] -= inner * factor;
                changed = true;
            }
        }
        start += count;
    }
    if !changed {
        return None;
    }
    let mut list = String::new();
    for (index, value) in dx.iter().enumerate() {
        if index > 0 {
            list.push(' ');
        }
        let _ = write!(list, "{value}");
    }
    let mut fixed = String::with_capacity(element.len() + 16);
    fixed.push_str(&element[..span.start]);
    let _ = write!(fixed, " dx=\"{list}\"");
    fixed.push_str(&element[span.end..]);
    Some(fixed)
}

/// The chunks of the content of a `<text>` element as usvg writes it (a `<tspan>` each, in a
/// `<textPath>` perhaps, with a `<tspan>` for each span inside): the number of characters of
/// each, how much of its inner shifts its anchor takes (1 for `end`, ½ for `middle`) and the
/// letter spacing of its last span.
fn chunks(content: &str) -> Vec<(usize, f32, f32)> {
    let mut chunks: Vec<(usize, f32, f32)> = Vec::new();
    let mut depth = 0usize;
    let mut rest = content;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('<') {
            let Some(end) = after.find('>') else {
                break;
            };
            let tag = &after[..end];
            if tag.starts_with("/tspan") {
                depth = depth.saturating_sub(1);
            } else if tag.starts_with("tspan") && !tag.ends_with('/') {
                if depth == 0 {
                    let factor = match crate::svg::attribute(tag, "text-anchor") {
                        Some("end") => 1.0,
                        Some("middle") => 0.5,
                        _ => 0.0,
                    };
                    chunks.push((0, factor, 0.0));
                } else if depth == 1
                    && let Some(chunk) = chunks.last_mut()
                {
                    chunk.2 = crate::svg::attribute(tag, "letter-spacing")
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(0.0);
                }
                depth += 1;
            }
            rest = &after[end + 1..];
        } else {
            let text_end = rest.find('<').unwrap_or(rest.len());
            if depth > 0
                && let Some(chunk) = chunks.last_mut()
            {
                chunk.0 += characters(&rest[..text_end]);
            }
            rest = &rest[text_end..];
        }
    }
    chunks
}

/// The number of characters of escaped XML text: an entity counts one.
fn characters(text: &str) -> usize {
    let mut count = 0;
    let mut rest = text;
    while let Some(character) = rest.chars().next() {
        count += 1;
        rest = if character == '&' {
            rest.find(';').map_or(&rest[1..], |end| &rest[end + 1..])
        } else {
            &rest[character.len_utf8()..]
        };
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchors_chunks_with_their_inner_shifts() {
        let written = "<svg><text dx=\"10 0 0 20 0 0 5\" xml:space=\"preserve\">\
                       <tspan x=\"250\" y=\"20\" text-anchor=\"end\"><tspan font-size=\"16\">A&amp;A</tspan>\
                       <tspan font-size=\"16\">BBB</tspan></tspan>\
                       <tspan x=\"250\" y=\"40\" text-anchor=\"middle\"><tspan>C</tspan></tspan></text>\
                       <text><tspan x=\"1\">plain</tspan></text></svg>";
        // The first chunk (6 characters) ends 20 pixels earlier; the second (one character,
        // no inner shift) stays.
        assert_eq!(
            fix(written),
            written.replace("dx=\"10 0 0 20 0 0 5\"", "dx=\"-10 0 0 20 0 0 5\"")
        );
        assert_eq!(
            chunks("<tspan text-anchor=\"middle\"><tspan>x&lt;y</tspan></tspan>"),
            [(3, 0.5, 0.0)]
        );
    }

    #[test]
    fn anchors_chunks_with_the_spacing_after_their_last_character() {
        // Chrome anchors "LS-1" with the spacing after its "1"; usvg leaves that out.
        let written = "<svg><text xml:space=\"preserve\"><tspan x=\"50\" y=\"20\" text-anchor=\"middle\">\
                       <tspan font-size=\"9\" letter-spacing=\"1\">LS-1</tspan></tspan></text>\
                       <text xml:space=\"preserve\"><tspan x=\"50\" y=\"40\">\
                       <tspan font-size=\"9\" letter-spacing=\"1\">AB</tspan></tspan></text></svg>";
        assert_eq!(
            fix(written),
            written.replacen("<text xml:space", "<text dx=\"-0.5 0 0 0\" xml:space", 1)
        );
    }
}

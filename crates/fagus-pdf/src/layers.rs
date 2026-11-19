//! Layers (optional content, PDF 1.7 §8.11): content that a viewer can show and hide, and that
//! may show on screen only or in print only (`-fagus-layer`).
//!
//! krilla writes no optional content, so the content of a layer is marked while it is drawn and
//! the marks become optional content afterwards. A layer begins with a clip to a rectangle that
//! reaches far beyond every page, and ends with a second one that is pushed and popped at once:
//! clips that change nothing should they stay. The left edges of the rectangles lie far off the
//! page at positions that no content has and that tell the layer apart; they are placed in the
//! user space of the page or of the group they are drawn in, whatever the transform, so that
//! the coordinates krilla writes are the same. Once the PDF is written, the start becomes
//! `/OC /name BDC` and the end `EMC`, in the content of the pages and of the groups (form
//! `XObjects`) that have them, and the document gets its optional content groups.

use std::fmt::Write as _;
use std::io::Read;
use std::sync::Arc;

use fagus_style::values::{Layer, LayerShown};
use krilla::geom::{PathBuilder, Transform};
use krilla::paint::FillRule;
use krilla::surface::Surface;
use rustc_hash::FxHashMap;

use crate::update::{self, Update};

/// The left edge of the start of the first layer; that of layer `n` lies 4 further left.
const START: f32 = -50_000.0;
/// The left edge of the end of a layer.
const END: f32 = -45_000.0;
/// How far the rectangles reach right, up and down.
const REACH: f32 = 60_000.0;

/// The layers of a document while it is drawn.
#[derive(Debug, Default)]
pub(crate) struct Layers {
    /// Whether layers are written; PDF/A-1 has none.
    pub(crate) enabled: bool,
    /// The layers in the order they first appear; one per name, as the first with it says.
    pub(crate) list: Vec<Arc<Layer>>,
    by_name: FxHashMap<Arc<str>, usize>,
    /// The layers open: the stream they are drawn in and how many groups were open in it,
    /// theirs included.
    open: Vec<(usize, usize)>,
}

impl Layers {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            ..Self::default()
        }
    }

    /// The index of a layer, by its name.
    fn index(&mut self, layer: &Arc<Layer>) -> usize {
        if let Some(&index) = self.by_name.get(&layer.name) {
            return index;
        }
        self.list.push(Arc::clone(layer));
        self.by_name.insert(Arc::clone(&layer.name), self.list.len() - 1);
        self.list.len() - 1
    }

    /// Marks the start of a layer, pushing its clip; `false` where it is not marked (layers are
    /// not written, or the transform cannot be undone). `at` tells the stream drawn (any number
    /// that differs from those of the other streams drawn at the same time) and how many groups
    /// are open in it, the layer's included.
    pub(crate) fn start(
        &mut self,
        surface: &mut Surface<'_>,
        layer: &Arc<Layer>,
        at: (usize, usize),
    ) -> bool {
        if !self.enabled {
            return false;
        }
        let index = self.index(layer);
        let left = START - 4.0 * index as f32;
        let Some(path) = marker(surface.ctm(), left) else {
            return false;
        };
        surface.push_clip_path(&path, &FillRule::NonZero);
        self.open.push(at);
        true
    }

    /// Whether the `Pop` that comes next ends a layer, `at` the stream and the groups open in
    /// it as for [`Layers::start`]; if so, the layer is closed.
    pub(crate) fn ends_at(&mut self, at: (usize, usize)) -> bool {
        let ends = self.open.last() == Some(&at);
        if ends {
            self.open.pop();
        }
        ends
    }
}

/// Marks the end of a layer, before the `Pop` of its start.
pub(crate) fn end(surface: &mut Surface<'_>) {
    if let Some(path) = marker(surface.ctm(), END) {
        surface.push_clip_path(&path, &FillRule::NonZero);
        surface.pop();
    }
}

/// A rectangle from `left` to [`REACH`] across and from `-REACH` to `REACH` down, in the space
/// that `ctm` maps the surface's coordinates to; `None` where `ctm` cannot be undone.
fn marker(ctm: Transform, left: f32) -> Option<krilla::geom::Path> {
    let inverse = ctm.invert()?;
    let mut builder = PathBuilder::new();
    let point = |x: f32, y: f32| {
        tiny_skia::Point::from_xy(
            inverse.sx() * x + inverse.kx() * y + inverse.tx(),
            inverse.ky() * x + inverse.sy() * y + inverse.ty(),
        )
    };
    let corners = [
        point(left, -REACH),
        point(REACH, -REACH),
        point(REACH, REACH),
        point(left, REACH),
    ];
    if !corners
        .iter()
        .all(|corner| corner.x.is_finite() && corner.y.is_finite())
    {
        return None;
    }
    builder.move_to(corners[0].x, corners[0].y);
    for corner in &corners[1..] {
        builder.line_to(corner.x, corner.y);
    }
    builder.close();
    builder.finish()
}

/// What a mark in a content stream is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mark {
    Start(usize),
    End,
}

/// The content of a stream with its marks made optional content: `/OC /FagusLayerN BDC` where
/// a layer starts, `EMC` where it ends. `None` where it has no marks, or where it has content
/// that this does not read (inline images).
fn rewrite(content: &[u8]) -> Option<Vec<u8>> {
    let tokens = tokens(content)?;
    let mut out = Vec::with_capacity(content.len());
    let mut copied = 0;
    let mut found = false;
    let mut index = 0;
    while index < tokens.len() {
        let Some((mark, length)) = mark_at(content, &tokens[index..]) else {
            index += 1;
            continue;
        };
        let start = tokens[index].start;
        let end = tokens[index + length - 1].end;
        out.extend_from_slice(&content[copied..start]);
        match mark {
            // `q` and the clip become the start of the marked content and the `q` alone.
            Mark::Start(layer) => {
                out.extend_from_slice(format!("/OC /FagusLayer{layer} BDC\nq\n").as_bytes());
            }
            // `q`, the clip, its `Q` and the `Q` of the start become `Q` and the end.
            Mark::End => out.extend_from_slice(b"Q\nEMC\n"),
        }
        copied = end;
        index += length;
        found = true;
    }
    if !found {
        return None;
    }
    out.extend_from_slice(&content[copied..]);
    Some(out)
}

/// The tokens of a content stream, by where they lie; strings, dictionaries and arrays are
/// tokens of their own. `None` for an inline image, whose data this does not read.
fn tokens(content: &[u8]) -> Option<Vec<std::ops::Range<usize>>> {
    let delimiter = |byte: u8| b" \t\r\n\x0c\0()<>[]{}/%".contains(&byte);
    let mut tokens = Vec::new();
    let mut position = 0;
    while let Some(&byte) = content.get(position) {
        let start = position;
        match byte {
            b' ' | b'\t' | b'\r' | b'\n' | b'\x0c' | b'\0' => {
                position += 1;
                continue;
            }
            b'%' => {
                while content
                    .get(position)
                    .is_some_and(|byte| !matches!(byte, b'\r' | b'\n'))
                {
                    position += 1;
                }
                continue;
            }
            b'(' => position = crate::parse::string_end(content, position)? + 1,
            b'<' if content.get(position + 1) == Some(&b'<') => {
                position = crate::parse::dictionary_end(content, position)?;
            }
            b'<' => position += crate::parse::find(&content[position..], b">")? + 1,
            b'[' | b']' | b'{' | b'}' | b'>' | b')' => position += 1,
            b'/' => {
                position += 1;
                while content.get(position).is_some_and(|&byte| !delimiter(byte)) {
                    position += 1;
                }
            }
            _ => {
                while content.get(position).is_some_and(|&byte| !delimiter(byte)) {
                    position += 1;
                }
            }
        }
        if &content[start..position] == b"BI" {
            return None;
        }
        tokens.push(start..position);
    }
    Some(tokens)
}

/// The mark that the tokens start with, and how many tokens it takes: `q`, a rectangle (`m`,
/// three `l` and `h`) whose left edge tells the mark, and `W n`; an end with the two `Q` after.
fn mark_at(content: &[u8], tokens: &[std::ops::Range<usize>]) -> Option<(Mark, usize)> {
    let word = |index: usize| tokens.get(index).map(|token| &content[token.clone()]);
    let number = |index: usize| -> Option<f32> { std::str::from_utf8(word(index)?).ok()?.parse().ok() };
    let pattern: [&[u8]; 6] = [b"q", b"m", b"l", b"l", b"l", b"h"];
    let positions = [0, 3, 6, 9, 12, 13];
    if pattern
        .iter()
        .zip(positions)
        .any(|(expected, index)| word(index) != Some(*expected))
        || word(14) != Some(b"W")
        || word(15) != Some(b"n")
    {
        return None;
    }
    let left = number(1)?;
    let near = |value: f32, expected: f32| (value - expected).abs() < 0.5;
    for index in [2, 5, 8, 11] {
        number(index)?;
    }
    if !(near(number(4)?, REACH) && near(number(7)?, REACH) && near(number(10)?, left)) {
        return None;
    }
    if near(left, END) {
        return (word(16) == Some(b"Q") && word(17) == Some(b"Q")).then_some((Mark::End, 18));
    }
    let steps = (START - left) / 4.0;
    if steps < -0.1 || !near(steps * 4.0, steps.round() * 4.0) {
        return None;
    }
    Some((Mark::Start(steps.round() as usize), 16))
}

/// Turns the marks of the layers in the PDF into optional content and adds the optional
/// content groups to the document; `archival` (PDF/A, PDF/UA) leaves out what decides by the
/// use of the document, which those standards do not allow, so that every layer shows on screen
/// and in print.
/// Returns the object number of each layer's group, in the order of `layers`.
pub(crate) fn write(update: &mut Update<'_>, layers: &[Arc<Layer>], archival: bool) -> Option<Vec<u32>> {
    if layers.is_empty() {
        return Some(Vec::new());
    }
    let groups: Vec<u32> = layers
        .iter()
        .map(|layer| {
            let usage = match layer.shown {
                _ if archival => String::new(),
                LayerShown::Always => String::new(),
                LayerShown::Screen => "/Usage<</View<</ViewState/ON>>/Print<</PrintState/OFF>>>>".to_owned(),
                LayerShown::Print => "/Usage<</View<</ViewState/OFF>>/Print<</PrintState/ON>>>>".to_owned(),
            };
            update.add(format!("<</Type/OCG/Name {}{usage}>>", update::text(&layer.name)).into_bytes())
        })
        .collect();
    let mut properties = String::from("<<");
    for (index, group) in groups.iter().enumerate() {
        let _ = write!(properties, "/FagusLayer{index} {group} 0 R");
    }
    properties.push_str(">>");

    // The content streams of the pages, with the pages' resources.
    let mut page_resources: FxHashMap<u32, String> = FxHashMap::default();
    let numbers: Vec<u32> = update.numbers().collect();
    for &number in &numbers {
        let Some(dictionary) = update.dictionary(number) else {
            continue;
        };
        if crate::parse::entry(&dictionary, "/Type").map(str::trim) != Some("/Page") {
            continue;
        }
        let (Some(contents), Some(resources)) = (
            crate::parse::entry(&dictionary, "/Contents").and_then(update::reference),
            crate::parse::entry(&dictionary, "/Resources"),
        ) else {
            continue;
        };
        page_resources.insert(contents, resources.trim().to_owned());
    }
    let mut shared: Vec<u32> = Vec::new();
    for &number in &numbers {
        let Some(dictionary) = update.dictionary(number) else {
            continue;
        };
        let form = crate::parse::entry(&dictionary, "/Subtype").map(str::trim) == Some("/Form");
        let page = page_resources.get(&number);
        if !form && page.is_none() {
            continue;
        }
        let Some(data) = update.data(number) else {
            continue;
        };
        let deflated = crate::parse::entry(&dictionary, "/Filter").map(str::trim) == Some("/FlateDecode");
        let content = if deflated {
            let mut content = Vec::new();
            flate2::read::ZlibDecoder::new(data)
                .read_to_end(&mut content)
                .ok()?;
            content
        } else {
            data.to_vec()
        };
        let Some(rewritten) = rewrite(&content) else {
            continue;
        };
        let stored = if deflated { deflate(&rewritten)? } else { rewritten };
        let mut bare = update::without_entry(&dictionary, "/Length");
        let resources = match page {
            Some(resources) => Some(resources.clone()),
            None => crate::parse::entry(&dictionary, "/Resources").map(|value| value.trim().to_owned()),
        };
        // The groups by name in the resources of every stream with marks: in their own object,
        // or in the stream's dictionary (a page's are always in an object of their own).
        match resources.as_deref().and_then(update::reference) {
            Some(reference) => shared.push(reference),
            None if page.is_none() => {
                let resources = resources.unwrap_or_else(|| "<<>>".to_owned());
                let resources = update::with_entry(&resources, "/Properties", &properties);
                bare = update::with_entry(&bare, "/Resources", &resources);
            }
            None => {}
        }
        update.set_stream(number, &bare, &stored);
    }
    shared.sort_unstable();
    shared.dedup();
    for number in shared {
        let dictionary = update.dictionary(number)?;
        update.set(
            number,
            update::with_entry(&dictionary, "/Properties", &properties).into_bytes(),
        );
    }

    // The document's optional content: every group listed and in the order of the layers;
    // those shown in print only are off on screen.
    let list = |keep: &dyn Fn(&Layer) -> bool| {
        let mut list = String::new();
        for (_, group) in layers.iter().zip(&groups).filter(|(layer, _)| keep(layer)) {
            let _ = write!(list, "{group} 0 R ");
        }
        list
    };
    let all = list(&|_| true);
    let off = list(&|layer| !archival && layer.shown == LayerShown::Print);
    let on = list(&|layer| archival || layer.shown != LayerShown::Print);
    let used = |shown: LayerShown| layers.iter().any(|layer| layer.shown == shown);
    let automatic = if archival || !(used(LayerShown::Screen) || used(LayerShown::Print)) {
        String::new()
    } else {
        let decided = list(&|layer| layer.shown != LayerShown::Always);
        format!(
            "/AS[<</Event/View/OCGs[{decided}]/Category[/View]>><</Event/Print/OCGs[{decided}]/Category[/Print]>>]"
        )
    };
    let configuration = format!(
        "<</Name(Layers)/Order[{all}]/ON[{on}]/OFF[{off}]{automatic}>>",
        all = all.trim_end(),
        on = on.trim_end(),
        off = off.trim_end(),
    );
    let root = update.root()?;
    let catalog = update.dictionary(root)?;
    let catalog = update::with_entry(
        &catalog,
        "/OCProperties",
        &format!("<</OCGs[{}]/D{configuration}>>", all.trim_end()),
    );
    update.set(root, catalog.into_bytes());
    Some(groups)
}

fn deflate(data: &[u8]) -> Option<Vec<u8>> {
    use std::io::Write;
    let mut encoder =
        flate2::write::ZlibEncoder::new(Vec::with_capacity(data.len() / 3), flate2::Compression::new(6));
    encoder.write_all(data).ok()?;
    encoder.finish().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_marks_into_optional_content() {
        let content = b"q 1 0 0 1 0 0 cm 0 0 1 1 re\nf\nQ\n\
            q -50008 60841.92 m 60000 60841.92 l 60000 -59158.08 l -50008 -59158.08 l\nh\nW\nn/Span<</MCID 0>>BDC\n\
            BT [<0001>(a\\) Q)]TJ\nET\nEMC\n\
            q -45000.004 1 m 59999.996 1 l 60000 2 l -45000 2 l\nh\nW\nn\nQ\nQ\nBT\nET\n";
        let rewritten = rewrite(content).expect("marks");
        assert_eq!(
            String::from_utf8(rewritten).expect("text"),
            "q 1 0 0 1 0 0 cm 0 0 1 1 re\nf\nQ\n/OC /FagusLayer2 BDC\nq\n/Span<</MCID 0>>BDC\n\
             BT [<0001>(a\\) Q)]TJ\nET\nEMC\nQ\nEMC\n\nBT\nET\n"
        );
        // Clips that are not marks stay.
        let clip = b"q 0 0 m 10 0 l 10 10 l 0 10 l\nh\nW\nn\nQ\n";
        assert_eq!(rewrite(clip), None);
        assert_eq!(rewrite(b"BI /W 1 ID x EI"), None);
    }
}

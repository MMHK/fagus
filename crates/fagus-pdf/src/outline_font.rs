//! TrueType fonts made of outlines that Fagus draws itself, for faces whose glyphs PDF fonts
//! cannot hold as they are: PingFang keeps them in Apple's `hvgl` table, which neither krilla
//! nor PDF viewers read. Such a font has the glyphs that a document uses from one instance of
//! the face, in a `glyf` table, numbered from 1 in the order of their numbers in the face, and
//! the instance's PostScript name (`PingFangSC-Regular`, as Chrome names the font). krilla
//! embeds it as it embeds other TrueType fonts, so that text keeps its characters.

use std::collections::BTreeSet;

use fagus_text::{Face, Variations};
use rustc_hash::FxHashMap;
use skrifa::MetadataProvider;
use skrifa::outline::OutlinePen;
use skrifa::raw::TableProvider;

/// A font made of outlines, and the numbers its glyphs have for those of the face.
pub(crate) struct OutlineFont {
    pub data: Vec<u8>,
    pub glyphs: FxHashMap<u32, u16>,
}

/// A glyph in `glyf` form: its contours of points (x, y, on the curve), in font units.
#[derive(Default)]
struct Contours {
    contours: Vec<Vec<(i16, i16, bool)>>,
    /// The current point, unrounded.
    last: (f32, f32),
}

fn unit(value: f32) -> i16 {
    value.round().clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16
}

impl Contours {
    fn point(&mut self, x: f32, y: f32, on: bool) {
        if let Some(contour) = self.contours.last_mut() {
            contour.push((unit(x), unit(y), on));
        }
        if on {
            self.last = (x, y);
        }
    }
}

impl OutlinePen for Contours {
    fn move_to(&mut self, x: f32, y: f32) {
        self.contours.push(Vec::new());
        self.point(x, y, true);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.point(x, y, true);
    }

    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.point(cx, cy, false);
        self.point(x, y, true);
    }

    /// A cubic curve, which `hvgl` does not have, as the quadratic one closest to it in the
    /// middle.
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let (x0, y0) = self.last;
        let cx = (3.0 * (cx0 + cx1) - x0 - x) / 4.0;
        let cy = (3.0 * (cy0 + cy1) - y0 - y) / 4.0;
        self.quad_to(cx, cy, x, y);
    }

    fn close(&mut self) {
        // The contour closes by itself: a last point on its first is left out.
        if let Some(contour) = self.contours.last_mut() {
            if contour.len() > 1 && contour.last() == contour.first() {
                contour.pop();
            }
            if contour.len() < 2 {
                self.contours.pop();
            }
        }
    }
}

impl Contours {
    /// The box around the points: left, bottom, right and top.
    fn bounds(&self) -> Option<[i16; 4]> {
        let mut points = self.contours.iter().flatten();
        let &(x, y, _) = points.next()?;
        Some(
            points.fold([x, y, x, y], |[left, bottom, right, top], &(x, y, _)| {
                [left.min(x), bottom.min(y), right.max(x), top.max(y)]
            }),
        )
    }

    /// The glyph as a simple glyph of a `glyf` table; empty without contours.
    fn encode(&self) -> Vec<u8> {
        let Some([left, bottom, right, top]) = self.bounds() else {
            return Vec::new();
        };
        let mut data = Vec::new();
        data.extend((self.contours.len() as i16).to_be_bytes());
        for value in [left, bottom, right, top] {
            data.extend(value.to_be_bytes());
        }
        let mut end = 0u16;
        for contour in &self.contours {
            end += contour.len() as u16;
            data.extend((end - 1).to_be_bytes());
        }
        // No instructions.
        data.extend(0u16.to_be_bytes());
        let (mut flags, mut xs, mut ys) = (Vec::new(), Vec::new(), Vec::new());
        let (mut x0, mut y0) = (0i16, 0i16);
        for &(x, y, on) in self.contours.iter().flatten() {
            let mut flag = u8::from(on);
            // A change that fits a byte, with its sign in the flag (positive with
            // X_IS_SAME_OR_POSITIVE_X_SHORT_VECTOR); none, with that flag alone.
            let mut delta = |value: i16, last: i16, short: u8, same: u8, out: &mut Vec<u8>| {
                let change = i32::from(value) - i32::from(last);
                if change == 0 {
                    flag |= same;
                } else if change.unsigned_abs() <= 255 {
                    flag |= short;
                    if change > 0 {
                        flag |= same;
                    }
                    out.push(change.unsigned_abs() as u8);
                } else {
                    out.extend((change as i16).to_be_bytes());
                }
            };
            delta(x, x0, 0x02, 0x10, &mut xs);
            delta(y, y0, 0x04, 0x20, &mut ys);
            flags.push(flag);
            (x0, y0) = (x, y);
        }
        data.extend(flags);
        data.extend(xs);
        data.extend(ys);
        data
    }

    fn points(&self) -> usize {
        self.contours.iter().map(Vec::len).sum()
    }
}

/// The PostScript name of the face's named instance at `variations` (axes not among them at
/// their defaults), as Core Text names the font of that instance; else the face's own.
fn postscript_name(font: &skrifa::FontRef<'_>, variations: &Variations) -> Option<String> {
    let axes = font.axes();
    let value = |index: usize| {
        let axis = axes.get(index)?;
        let tag = axis.tag().to_be_bytes();
        Some(
            variations
                .iter()
                .rev()
                .find(|(own, _)| *own == tag)
                .map_or(axis.default_value(), |(_, value)| *value),
        )
    };
    let string = |id| {
        font.localized_strings(id)
            .english_or_first()
            .map(|name| name.chars().collect::<String>())
    };
    font.named_instances()
        .iter()
        .find(|instance| {
            instance
                .user_coords()
                .enumerate()
                .all(|(index, coordinate)| value(index).is_some_and(|value| (value - coordinate).abs() < 0.5))
        })
        .and_then(|instance| instance.postscript_name_id())
        .and_then(string)
        .or_else(|| string(skrifa::string::StringId::POSTSCRIPT_NAME))
        .filter(|name| !name.is_empty())
}

/// A `name` table with the family's name and the PostScript name, in UTF-16 for Windows.
fn name_table(family: &str, postscript: &str) -> Vec<u8> {
    let names = [(1u16, family), (6, postscript)];
    let mut records = Vec::new();
    let mut strings = Vec::new();
    for (id, name) in names {
        let encoded: Vec<u8> = name.encode_utf16().flat_map(u16::to_be_bytes).collect();
        // Platform 3 (Windows), encoding 1 (Unicode BMP), US English.
        for value in [3u16, 1, 0x409, id, encoded.len() as u16, strings.len() as u16] {
            records.extend(value.to_be_bytes());
        }
        strings.extend(encoded);
    }
    let mut table = Vec::new();
    for value in [0u16, names.len() as u16, (6 + records.len()) as u16] {
        table.extend(value.to_be_bytes());
    }
    table.extend(records);
    table.extend(strings);
    table
}

/// The sum of a table's big-endian 32-bit words, as OpenType checks tables.
fn checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0u32, |sum, chunk| {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

/// An OpenType font file of tables, sorted by tag, with `head`'s checksum adjustment set.
fn font_file(mut tables: Vec<([u8; 4], Vec<u8>)>) -> Vec<u8> {
    tables.sort_by_key(|(tag, _)| *tag);
    let count = tables.len() as u16;
    let power = 1u16 << count.max(1).ilog2();
    let mut file = Vec::new();
    for value in [
        0x0001u16,
        0x0000,
        count,
        power * 16,
        power.trailing_zeros() as u16,
        count * 16 - power * 16,
    ] {
        file.extend(value.to_be_bytes());
    }
    let mut offset = 12 + 16 * tables.len();
    let mut head = None;
    for (tag, data) in &tables {
        if tag == b"head" {
            head = Some(offset);
        }
        file.extend(tag);
        file.extend(checksum(data).to_be_bytes());
        file.extend((offset as u32).to_be_bytes());
        file.extend((data.len() as u32).to_be_bytes());
        offset += data.len().next_multiple_of(4);
    }
    for (_, data) in &tables {
        file.extend(data);
        file.resize(file.len().next_multiple_of(4), 0);
    }
    if let Some(head) = head {
        let adjustment = 0xB1B0_AFBA_u32.wrapping_sub(checksum(&file));
        file[head + 8..head + 12].copy_from_slice(&adjustment.to_be_bytes());
    }
    file
}

/// A font of the `glyphs` of `face` at `variations`, drawn with [`Face::draw_glyph`]; `None` if
/// the face's metrics do not read.
pub(crate) fn build(face: &Face, variations: &Variations, glyphs: &BTreeSet<u32>) -> Option<OutlineFont> {
    let font = skrifa::FontRef::from_index(&face.data, face.index).ok()?;
    let location = font.axes().location(
        variations
            .iter()
            .map(|(tag, value)| (skrifa::Tag::new(tag), *value)),
    );
    let units_per_em = font.head().ok()?.units_per_em();
    // The outlines in font units: at a font size of an em, less the face's `size-adjust`.
    let size = f32::from(units_per_em) / face.size_adjust.max(f32::MIN_POSITIVE);
    let metrics = font.glyph_metrics(skrifa::instance::Size::unscaled(), &location);
    let hhea = font.hhea().ok()?;

    // Glyph 0 (.notdef) stays first, drawn where the text uses it; the others follow in order.
    let mut map = FxHashMap::default();
    let numbers: Vec<u32> = std::iter::once(0)
        .chain(glyphs.iter().copied().filter(|glyph| *glyph != 0))
        .take(usize::from(u16::MAX))
        .collect();
    let (mut glyf, mut loca, mut hmtx) = (Vec::new(), Vec::new(), Vec::new());
    let (mut max_points, mut max_contours, mut widest) = (0, 0, 0u16);
    let mut extent: Option<[i16; 4]> = None;
    for (new, &old) in numbers.iter().enumerate() {
        map.insert(old, new as u16);
        let mut contours = Contours::default();
        if old != 0 || glyphs.contains(&0) {
            face.draw_glyph(old, size, variations, &mut contours);
        }
        let bounds = contours.bounds();
        if let Some([left, bottom, right, top]) = bounds {
            extent = Some(extent.map_or([left, bottom, right, top], |[l, b, r, t]| {
                [l.min(left), b.min(bottom), r.max(right), t.max(top)]
            }));
        }
        max_points = max_points.max(contours.points());
        max_contours = max_contours.max(contours.contours.len());
        loca.extend((glyf.len() as u32).to_be_bytes());
        glyf.extend(contours.encode());
        glyf.resize(glyf.len().next_multiple_of(4), 0);
        let advance = metrics
            .advance_width(skrifa::GlyphId::new(old))
            .map_or(0, |advance| {
                advance.round().clamp(0.0, f32::from(u16::MAX)) as u16
            });
        widest = widest.max(advance);
        hmtx.extend(advance.to_be_bytes());
        hmtx.extend(bounds.map_or(0, |[left, ..]| left).to_be_bytes());
    }
    loca.extend((glyf.len() as u32).to_be_bytes());
    let count = numbers.len() as u16;
    let [left, bottom, right, top] = extent.unwrap_or_default();

    let mut head = Vec::new();
    head.extend(0x0001_0000u32.to_be_bytes()); // version 1.0
    head.extend(0x0001_0000u32.to_be_bytes()); // font revision
    head.extend(0u32.to_be_bytes()); // checksum adjustment, set last
    head.extend(0x5F0F_3CF5u32.to_be_bytes()); // magic number
    head.extend(0x0001u16.to_be_bytes()); // flags: baseline at y = 0
    head.extend(units_per_em.to_be_bytes());
    head.extend([0; 16]); // created, modified
    for value in [left, bottom, right, top] {
        head.extend(value.to_be_bytes());
    }
    // Style, smallest size, direction hint, long offsets in `loca`, glyph data format.
    for value in [0i16, 8, 2, 1, 0] {
        head.extend(value.to_be_bytes());
    }

    let mut hhea_table = Vec::new();
    hhea_table.extend(0x0001_0000u32.to_be_bytes());
    for value in [
        hhea.ascender().to_i16(),
        hhea.descender().to_i16(),
        hhea.line_gap().to_i16(),
    ] {
        hhea_table.extend(value.to_be_bytes());
    }
    hhea_table.extend(widest.to_be_bytes());
    // Smallest side bearings and largest extent, caret slope (upright), caret offset, four
    // reserved fields, metric data format.
    for value in [left, 0, right, 1, 0, 0, 0, 0, 0, 0, 0] {
        hhea_table.extend(value.to_be_bytes());
    }
    hhea_table.extend(count.to_be_bytes());

    let mut maxp = Vec::new();
    maxp.extend(0x0001_0000u32.to_be_bytes());
    for value in [count, max_points as u16, max_contours as u16, 0, 0, 2] {
        maxp.extend(value.to_be_bytes());
    }
    // No twilight points, storage, functions, instructions or components.
    maxp.extend([0; 16]);

    // Version 3: no glyph names; the face's underline, upright, proportional.
    let mut post = Vec::new();
    post.extend(0x0003_0000u32.to_be_bytes());
    post.extend(0u32.to_be_bytes());
    let underline = font.post().ok();
    for value in [
        underline
            .as_ref()
            .map_or(0, |post| post.underline_position().to_i16()),
        underline
            .as_ref()
            .map_or(0, |post| post.underline_thickness().to_i16()),
    ] {
        post.extend(value.to_be_bytes());
    }
    post.extend([0; 20]);

    let mut tables = vec![
        (*b"glyf", glyf),
        (*b"head", head),
        (*b"hhea", hhea_table),
        (*b"hmtx", hmtx),
        (*b"loca", loca),
        (*b"maxp", maxp),
        (*b"post", post),
    ];
    // The face's `OS/2` table gives the PDF the ascent, descent and cap height of the font,
    // with the weight of the instance.
    if let Some(os2) = font.table_data(skrifa::Tag::new(b"OS/2")) {
        let mut os2 = os2.as_bytes().to_vec();
        let weight = variations
            .iter()
            .rev()
            .find(|(tag, _)| tag == b"wght")
            .map(|(_, weight)| weight.round().clamp(1.0, 1000.0) as u16);
        if let (Some(weight), Some(field)) = (weight, os2.get_mut(4..6)) {
            field.copy_from_slice(&weight.to_be_bytes());
        }
        tables.push((*b"OS/2", os2));
    }
    let postscript = postscript_name(&font, variations).unwrap_or_else(|| face.family.replace(' ', ""));
    tables.push((*b"name", name_table(&face.family, &postscript)));
    Some(OutlineFont {
        data: font_file(tables),
        glyphs: map,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_quadratic_contours_as_glyf_points() {
        let mut contours = Contours::default();
        contours.move_to(0.0, 0.0);
        contours.quad_to(100.4, 0.0, 100.0, 300.0);
        contours.line_to(-200.0, 300.0);
        contours.quad_to(-100.0, 0.0, 0.0, 0.0);
        contours.close();
        assert_eq!(contours.points(), 5, "the closing point is the first");
        assert_eq!(contours.bounds(), Some([-200, 0, 100, 300]));
        let data = contours.encode();
        // One contour, its box, the last point's number, no instructions.
        assert_eq!(&data[..14], &[0, 1, 255, 56, 0, 0, 0, 100, 1, 44, 0, 4, 0, 0]);
        // On, off, on, on, off.
        let flags: Vec<u8> = data[14..19].iter().map(|flag| flag & 1).collect();
        assert_eq!(flags, [1, 0, 1, 1, 0]);
        // A move of 300 does not fit a byte.
        assert_eq!(data[16] & 0x04, 0);
    }

    #[test]
    fn builds_a_font_that_reads_back() {
        let tables = vec![
            (*b"name", name_table("PingFang SC", "PingFangSC-Regular")),
            (*b"head", vec![0; 54]),
        ];
        let file = font_file(tables);
        let font = skrifa::FontRef::new(&file).expect("a font");
        assert!(font.head().is_ok());
        let name: String = font
            .localized_strings(skrifa::string::StringId::POSTSCRIPT_NAME)
            .english_or_first()
            .expect("a PostScript name")
            .chars()
            .collect();
        assert_eq!(name, "PingFangSC-Regular");
        assert_eq!(
            checksum(&file),
            0xB1B0_AFBA,
            "the checksum adjustment balances the file"
        );
    }
}

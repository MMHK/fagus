//! Color palettes of color fonts (`CPAL`) and the CSS that picks them: `font-palette` and
//! `@font-palette-values` (CSS Fonts 4, §9), resolved as Chrome resolves them.

use std::sync::Arc;

/// An RGBA color of a palette, 8 bits a channel, not premultiplied.
pub type PaletteColor = [u8; 4];

/// `font-palette`: which palette of a color font its glyphs take.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum FontPalette {
    /// The font's first palette.
    #[default]
    Normal,
    /// The first palette the font marks as usable on light backgrounds, else the first.
    Light,
    /// The first palette the font marks as usable on dark backgrounds, else the first.
    Dark,
    /// The palette of the `@font-palette-values` rules of this name (a dashed identifier).
    Named(Arc<str>),
    /// `palette-mix()`: two palettes mixed color by color in sRGB, with the share of the
    /// second from 0 to 1.
    Mix(Arc<(FontPalette, FontPalette, f32)>),
}

/// A hash of a palette, the share of a mix by its bits.
pub(crate) fn palette_hash(palette: &FontPalette) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = rustc_hash::FxHasher::default();
    let mut stack = vec![palette];
    while let Some(palette) = stack.pop() {
        match palette {
            FontPalette::Normal => 0_u8.hash(&mut hasher),
            FontPalette::Light => 1_u8.hash(&mut hasher),
            FontPalette::Dark => 2_u8.hash(&mut hasher),
            FontPalette::Named(name) => {
                3_u8.hash(&mut hasher);
                name.hash(&mut hasher);
            }
            FontPalette::Mix(mix) => {
                4_u8.hash(&mut hasher);
                mix.2.to_bits().hash(&mut hasher);
                stack.push(&mix.0);
                stack.push(&mix.1);
            }
        }
    }
    hasher.finish()
}

/// `base-palette` of an `@font-palette-values` rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BasePalette {
    Light,
    Dark,
    Index(u16),
}

/// An `@font-palette-values` rule: a palette of the faces of `families`, made from a palette
/// of theirs with some colors replaced.
#[derive(Clone, Debug, PartialEq)]
pub struct PaletteValues {
    /// The name (a dashed identifier) that `font-palette` refers to.
    pub name: Arc<str>,
    /// The families the rule is for, as `@font-face` rules or installed fonts name them.
    pub families: Vec<String>,
    pub base: Option<BasePalette>,
    /// The colors replaced, by index into the palette, in order: a later one of an index wins.
    pub overrides: Vec<(u32, PaletteColor)>,
}

/// The palettes of a face's `CPAL` table: the color records, where each palette starts in them
/// and how many colors each has, with the type flags of each (bit 0: usable on light
/// backgrounds, bit 1: on dark ones). A palette is only read out when it is used: a table may
/// claim 65535 palettes of 65535 colors.
pub(crate) struct Palettes {
    records: Vec<PaletteColor>,
    starts: Vec<usize>,
    entries: usize,
    pub types: Vec<u32>,
}

impl Palettes {
    /// The palettes of a font; `None` without a `CPAL` table, or with one whose palettes reach
    /// past its color records (a font Chrome rejects, see [`color_tables_sound`]).
    pub(crate) fn read(font: &skrifa::FontRef<'_>) -> Option<Self> {
        use skrifa::raw::TableProvider;
        let cpal = font.cpal().ok()?;
        let entries = usize::from(cpal.num_palette_entries());
        let records: Vec<PaletteColor> = cpal
            .color_records_array()?
            .ok()?
            .iter()
            .map(|record| [record.red(), record.green(), record.blue(), record.alpha()])
            .collect();
        let starts: Vec<usize> = cpal
            .color_record_indices()
            .iter()
            .map(|first| usize::from(first.get()))
            .collect();
        if starts.is_empty() || starts.iter().any(|first| first + entries > records.len()) {
            return None;
        }
        let types = cpal
            .palette_types_array()
            .and_then(Result::ok)
            .map(|types| types.iter().map(|value| value.get().bits()).collect())
            .unwrap_or_default();
        Some(Self {
            records,
            starts,
            entries,
            types,
        })
    }

    /// The colors of palette `index` (the first for one past the last).
    pub(crate) fn colors(&self, index: usize) -> Vec<PaletteColor> {
        let start = self.starts.get(index).copied().unwrap_or(self.starts[0]);
        self.records[start..start + self.entries].to_vec()
    }

    /// The index of the first palette with a type flag, else 0.
    fn typed(&self, flag: u32) -> usize {
        self.types.iter().position(|types| types & flag != 0).unwrap_or(0)
    }

    /// The colors of `palette` for a face of `family` under `rules`: `None` for the font's
    /// own first palette (`normal`, or a name no rule for the family has), as Chrome draws it.
    pub(crate) fn resolve(
        &self,
        palette: &FontPalette,
        family: &str,
        rules: &[PaletteValues],
    ) -> Option<Vec<PaletteColor>> {
        match palette {
            FontPalette::Normal => None,
            FontPalette::Light => Some(self.colors(self.typed(1))),
            FontPalette::Dark => Some(self.colors(self.typed(2))),
            FontPalette::Named(name) => {
                // Of the rules of the name, the last for the family counts.
                let rule = rules.iter().rev().find(|rule| {
                    *rule.name == **name
                        && rule
                            .families
                            .iter()
                            .any(|candidate| candidate.eq_ignore_ascii_case(family))
                })?;
                let base = match rule.base {
                    Some(BasePalette::Light) => self.typed(1),
                    Some(BasePalette::Dark) => self.typed(2),
                    Some(BasePalette::Index(index)) if usize::from(index) < self.starts.len() => {
                        usize::from(index)
                    }
                    _ => 0,
                };
                let mut colors = self.colors(base);
                for (index, color) in &rule.overrides {
                    if let Some(slot) = usize::try_from(*index)
                        .ok()
                        .and_then(|index| colors.get_mut(index))
                    {
                        *slot = *color;
                    }
                }
                Some(colors)
            }
            FontPalette::Mix(mix) => {
                let (first, second, share) = &**mix;
                let first = self
                    .resolve(first, family, rules)
                    .unwrap_or_else(|| self.colors(0));
                let second = self
                    .resolve(second, family, rules)
                    .unwrap_or_else(|| self.colors(0));
                Some(
                    first
                        .iter()
                        .zip(&second)
                        .map(|(a, b)| mix_colors(*a, *b, *share))
                        .collect(),
                )
            }
        }
    }
}

/// Two colors mixed in sRGB with the share of the second, premultiplied by alpha as
/// `color-mix()` mixes.
fn mix_colors(a: PaletteColor, b: PaletteColor, share: f32) -> PaletteColor {
    let alpha = |color: PaletteColor| f32::from(color[3]) / 255.0;
    let (alpha_a, alpha_b) = (alpha(a), alpha(b));
    let mixed_alpha = alpha_a * (1.0 - share) + alpha_b * share;
    let channel = |index: usize| {
        if mixed_alpha <= 0.0 {
            return 0;
        }
        let value = (f32::from(a[index]) * alpha_a * (1.0 - share) + f32::from(b[index]) * alpha_b * share)
            / mixed_alpha;
        #[expect(clippy::cast_possible_truncation, reason = "clamped to a byte")]
        let byte = value.round().clamp(0.0, 255.0) as u8;
        byte
    };
    #[expect(clippy::cast_possible_truncation, reason = "clamped to a byte")]
    let alpha_byte = (mixed_alpha * 255.0).round().clamp(0.0, 255.0) as u8;
    [channel(0), channel(1), channel(2), alpha_byte]
}

/// Whether the color tables of a font are sound as Chrome's font sanitizer (OTS) requires: each
/// palette of `CPAL` within its color records, and each palette index of `COLR` (of a layer, a
/// solid paint or a color stop) within a palette or 0xFFFF (the text color). Chrome rejects a
/// font that is not; PDF writers (krilla) look such indices up unchecked.
pub(crate) fn color_tables_sound(font: &skrifa::FontRef<'_>) -> bool {
    use skrifa::raw::TableProvider;
    let entries = match font.cpal() {
        Ok(cpal) => {
            let entries = usize::from(cpal.num_palette_entries());
            let Some(Ok(records)) = cpal.color_records_array() else {
                return cpal.color_record_indices().is_empty();
            };
            if cpal
                .color_record_indices()
                .iter()
                .any(|first| usize::from(first.get()) + entries > records.len())
            {
                return false;
            }
            entries
        }
        Err(_) => 0,
    };
    let Ok(colr) = font.colr() else {
        return true;
    };
    let valid = |index: u16| index == u16::MAX || usize::from(index) < entries;
    if let Some(layers) = colr.layer_records() {
        let Ok(layers) = layers else {
            return false;
        };
        if !layers.iter().all(|layer| valid(layer.palette_index())) {
            return false;
        }
    }
    paints_sound(&colr, &valid)
}

/// The most paints that drawing a glyph of a `COLR` table (version 1) may go through, its
/// layers and the glyphs it draws included: paints may share their children, so that a table of
/// a few hundred bytes takes 2^60 steps to draw.
const MAX_GLYPH_PAINTS: u64 = 1 << 16;

/// Whether the paints of a `COLR` table (version 1) are sound: their palette indices `valid`,
/// without cycles, each glyph drawn in at most [`MAX_GLYPH_PAINTS`] steps. Each paint is looked
/// at once, by where it is in the table.
fn paints_sound<'a>(colr: &skrifa::raw::tables::colr::Colr<'a>, valid: &dyn Fn(u16) -> bool) -> bool {
    use skrifa::raw::tables::colr::Paint;
    let list = match colr.base_glyph_list() {
        Some(Ok(list)) => Some(list),
        Some(Err(_)) => return false,
        None => None,
    };
    let layers = match colr.layer_list() {
        Some(Ok(layers)) => Some(layers),
        Some(Err(_)) => return false,
        None => None,
    };
    let mut glyphs = rustc_hash::FxHashMap::default();
    let mut roots = Vec::new();
    if let Some(list) = &list {
        for record in list.base_glyph_paint_records() {
            let Ok(paint) = record.paint(list.offset_data()) else {
                return false;
            };
            glyphs.insert(record.glyph_id().to_u16(), paint.clone());
            roots.push(paint);
        }
    }
    if let Some(layers) = &layers {
        for paint in layers.paints().iter() {
            let Ok(paint) = paint else {
                return false;
            };
            roots.push(paint);
        }
    }
    let key = |paint: &Paint<'_>| paint.offset_data().as_bytes().as_ptr() as usize;
    // The paints a paint draws: `None` where one cannot be read.
    let children = |paint: &Paint<'a>| -> Option<Vec<Paint<'a>>> {
        let one = |child: Result<Paint<'a>, _>| child.ok().map(|child| vec![child]);
        match paint {
            Paint::ColrLayers(paint) => {
                let paints = layers.as_ref()?.paints();
                let first = usize::try_from(paint.first_layer_index()).ok()?;
                (first..first + usize::from(paint.num_layers()))
                    .map(|index| paints.get(index).ok())
                    .collect()
            }
            Paint::ColrGlyph(paint) => Some(
                glyphs
                    .get(&paint.glyph_id().to_u16())
                    .cloned()
                    .into_iter()
                    .collect(),
            ),
            Paint::Composite(composite) => Some(vec![
                composite.source_paint().ok()?,
                composite.backdrop_paint().ok()?,
            ]),
            Paint::Glyph(paint) => one(paint.paint()),
            Paint::Glyph2(paint) => one(paint.paint()),
            Paint::Transform(paint) => one(paint.paint()),
            Paint::VarTransform(paint) => one(paint.paint()),
            Paint::Translate(paint) => one(paint.paint()),
            Paint::VarTranslate(paint) => one(paint.paint()),
            Paint::Scale(paint) => one(paint.paint()),
            Paint::VarScale(paint) => one(paint.paint()),
            Paint::ScaleAroundCenter(paint) => one(paint.paint()),
            Paint::VarScaleAroundCenter(paint) => one(paint.paint()),
            Paint::ScaleUniform(paint) => one(paint.paint()),
            Paint::VarScaleUniform(paint) => one(paint.paint()),
            Paint::ScaleUniformAroundCenter(paint) => one(paint.paint()),
            Paint::VarScaleUniformAroundCenter(paint) => one(paint.paint()),
            Paint::Rotate(paint) => one(paint.paint()),
            Paint::VarRotate(paint) => one(paint.paint()),
            Paint::RotateAroundCenter(paint) => one(paint.paint()),
            Paint::VarRotateAroundCenter(paint) => one(paint.paint()),
            Paint::Skew(paint) => one(paint.paint()),
            Paint::VarSkew(paint) => one(paint.paint()),
            Paint::SkewAroundCenter(paint) => one(paint.paint()),
            Paint::VarSkewAroundCenter(paint) => one(paint.paint()),
            Paint::Solid(_)
            | Paint::VarSolid(_)
            | Paint::LinearGradient(_)
            | Paint::VarLinearGradient(_)
            | Paint::RadialGradient(_)
            | Paint::VarRadialGradient(_)
            | Paint::SweepGradient(_)
            | Paint::VarSweepGradient(_) => Some(Vec::new()),
        }
    };
    let colors = |paint: &Paint<'a>| -> bool {
        use skrifa::raw::tables::colr::{ColorStop, VarColorStop};
        // The palette indices of the stops of a gradient, `None` where they do not read.
        let stops: Option<Vec<u16>> = match paint {
            Paint::Solid(solid) => return valid(solid.palette_index()),
            Paint::VarSolid(solid) => return valid(solid.palette_index()),
            Paint::LinearGradient(gradient) => gradient
                .color_line()
                .ok()
                .map(|line| line.color_stops().iter().map(ColorStop::palette_index).collect()),
            Paint::RadialGradient(gradient) => gradient
                .color_line()
                .ok()
                .map(|line| line.color_stops().iter().map(ColorStop::palette_index).collect()),
            Paint::SweepGradient(gradient) => gradient
                .color_line()
                .ok()
                .map(|line| line.color_stops().iter().map(ColorStop::palette_index).collect()),
            Paint::VarLinearGradient(gradient) => gradient.color_line().ok().map(|line| {
                line.color_stops()
                    .iter()
                    .map(VarColorStop::palette_index)
                    .collect()
            }),
            Paint::VarRadialGradient(gradient) => gradient.color_line().ok().map(|line| {
                line.color_stops()
                    .iter()
                    .map(VarColorStop::palette_index)
                    .collect()
            }),
            Paint::VarSweepGradient(gradient) => gradient.color_line().ok().map(|line| {
                line.color_stops()
                    .iter()
                    .map(VarColorStop::palette_index)
                    .collect()
            }),
            _ => return true,
        };
        stops.is_some_and(|stops| stops.into_iter().all(valid))
    };
    // The steps that drawing each paint takes, by paint, and the paints being looked at: a
    // depth-first walk that reaches one of those again has found a cycle.
    let mut steps: rustc_hash::FxHashMap<usize, u64> = rustc_hash::FxHashMap::default();
    let mut open = rustc_hash::FxHashSet::default();
    for root in roots {
        let mut stack = vec![(root, false)];
        while let Some((paint, done)) = stack.pop() {
            let at = key(&paint);
            let Some(below) = children(&paint) else {
                return false;
            };
            if done {
                let total = below
                    .iter()
                    .map(|child| steps.get(&key(child)).copied().unwrap_or(0))
                    .fold(1_u64, u64::saturating_add);
                if total > MAX_GLYPH_PAINTS {
                    return false;
                }
                open.remove(&at);
                steps.insert(at, total);
                continue;
            }
            if steps.contains_key(&at) {
                continue;
            }
            if !open.insert(at) || !colors(&paint) {
                return false;
            }
            stack.push((paint, true));
            stack.extend(
                below
                    .into_iter()
                    .filter(|child| !steps.contains_key(&key(child)))
                    .map(|child| (child, false)),
            );
        }
    }
    true
}

/// The data of a font with the colors of `palette` in place of those of its first palette, as
/// PDF writers that only draw the first palette (krilla) read it: the color records from the
/// start of the record array. Its checksum (`head`) changes with the colors, which PDF writers
/// (krilla) tell fonts apart by. `None` where the font has no `CPAL` table to patch.
pub fn with_palette(data: &[u8], index: u32, palette: &[PaletteColor]) -> Option<Vec<u8>> {
    use skrifa::raw::{TableProvider, types::Tag};
    let font = skrifa::FontRef::from_index(data, index).ok()?;
    let record = font
        .table_directory()
        .table_records()
        .iter()
        .find(|record| record.tag() == Tag::new(b"CPAL"))?;
    let table_start = usize::try_from(record.offset()).ok()?;
    let cpal = font.cpal().ok()?;
    let records_offset = usize::try_from(cpal.color_records_array_offset().offset().to_u32()).ok()?;
    let count = usize::from(cpal.num_color_records());
    let start = table_start.checked_add(records_offset)?;
    let mut out = data.to_vec();
    for (slot, color) in palette.iter().take(count).enumerate() {
        let at = start + slot * 4;
        let bytes = out.get_mut(at..at + 4)?;
        // CPAL stores blue, green, red, alpha.
        bytes.copy_from_slice(&[color[2], color[1], color[0], color[3]]);
    }
    let head = font
        .table_directory()
        .table_records()
        .iter()
        .find(|record| record.tag() == Tag::new(b"head"))?;
    // `checksumAdjustment`, 8 bytes into the table, mixed with an FNV-1a hash of the colors.
    let at = usize::try_from(head.offset()).ok()?.checked_add(8)?;
    let hash = palette.iter().flatten().fold(0x811c_9dc5_u32, |hash, byte| {
        (hash ^ u32::from(*byte)).wrapping_mul(0x0100_0193)
    });
    let bytes = out.get_mut(at..at + 4)?;
    let checksum = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) ^ hash.max(1);
    bytes.copy_from_slice(&checksum.to_be_bytes());
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palettes() -> Palettes {
        Palettes {
            records: vec![
                [0, 0, 0, 255],
                [255, 0, 0, 255],
                [0, 255, 0, 255],
                [0, 0, 255, 255],
                [9, 9, 9, 255],
                [8, 8, 8, 255],
            ],
            starts: vec![0, 2, 4],
            entries: 2,
            types: vec![0, 2, 1],
        }
    }

    #[test]
    fn resolves_palettes_as_chrome() {
        let palettes = palettes();
        let rules = vec![
            PaletteValues {
                name: "--a".into(),
                families: vec!["Other".into()],
                base: Some(BasePalette::Index(2)),
                overrides: Vec::new(),
            },
            PaletteValues {
                name: "--a".into(),
                families: vec!["Colors".into()],
                base: Some(BasePalette::Index(1)),
                overrides: vec![(1, [1, 2, 3, 255]), (99, [4, 4, 4, 255])],
            },
            PaletteValues {
                name: "--b".into(),
                families: vec!["colors".into()],
                base: Some(BasePalette::Index(7)),
                overrides: Vec::new(),
            },
        ];
        let resolve = |palette: &FontPalette| palettes.resolve(palette, "Colors", &rules);
        assert_eq!(resolve(&FontPalette::Normal), None);
        assert_eq!(resolve(&FontPalette::Light), Some(palettes.colors(2)));
        assert_eq!(resolve(&FontPalette::Dark), Some(palettes.colors(1)));
        // The rule for the family; an override past the palette's end is left out.
        assert_eq!(
            resolve(&FontPalette::Named("--a".into())),
            Some(vec![[0, 255, 0, 255], [1, 2, 3, 255]])
        );
        // A base palette the font lacks is the first; a name without a rule for the family
        // leaves the font's own colors.
        assert_eq!(
            resolve(&FontPalette::Named("--b".into())),
            Some(palettes.colors(0))
        );
        assert_eq!(resolve(&FontPalette::Named("--c".into())), None);
        let mix = FontPalette::Mix(Arc::new((FontPalette::Normal, FontPalette::Dark, 0.25)));
        assert_eq!(resolve(&mix), Some(vec![[0, 64, 0, 255], [191, 0, 64, 255]]));
    }

    /// A font of just `tables` (a table directory and their data).
    fn sfnt(tables: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let count = u16::try_from(tables.len()).expect("few tables");
        let mut out = Vec::new();
        out.extend(0x0001_0000_u32.to_be_bytes());
        out.extend(count.to_be_bytes());
        out.extend([0; 6]);
        let mut offset = 12 + 16 * tables.len();
        for (tag, data) in tables {
            out.extend(*tag);
            out.extend(0_u32.to_be_bytes());
            out.extend(u32::try_from(offset).expect("small").to_be_bytes());
            out.extend(u32::try_from(data.len()).expect("small").to_be_bytes());
            offset += data.len().next_multiple_of(4);
        }
        for (_, data) in tables {
            out.extend(data);
            out.resize(out.len().next_multiple_of(4), 0);
        }
        out
    }

    /// A `CPAL` table of `palettes` palettes of `entries` colors starting at `starts`, with
    /// `records` color records.
    fn cpal(entries: u16, starts: &[u16], records: u16) -> Vec<u8> {
        let palettes = u16::try_from(starts.len()).expect("few palettes");
        let mut out = Vec::new();
        out.extend(0_u16.to_be_bytes());
        out.extend(entries.to_be_bytes());
        out.extend(palettes.to_be_bytes());
        out.extend(records.to_be_bytes());
        out.extend((12 + 2 * u32::from(palettes)).to_be_bytes());
        for start in starts {
            out.extend(start.to_be_bytes());
        }
        for record in 0..records {
            let [_, low] = record.to_be_bytes();
            out.extend([low, 0, 0, 255]);
        }
        out
    }

    /// A `COLR` table (version 0) of one glyph with layers in the colors `indices`.
    fn colr_v0(indices: &[u16]) -> Vec<u8> {
        let layers = u16::try_from(indices.len()).expect("few layers");
        let mut out = Vec::new();
        out.extend(0_u16.to_be_bytes());
        out.extend(1_u16.to_be_bytes());
        out.extend(14_u32.to_be_bytes());
        out.extend(20_u32.to_be_bytes());
        out.extend(layers.to_be_bytes());
        out.extend([0, 1, 0, 0]);
        out.extend(layers.to_be_bytes());
        for index in indices {
            out.extend(2_u16.to_be_bytes());
            out.extend(index.to_be_bytes());
        }
        out
    }

    /// A `COLR` table (version 1) of one glyph that composites a chain of `depth` paints, each
    /// of which composites the next with itself, ending in a solid paint of `index`.
    fn colr_v1(depth: u32, index: u16) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend(1_u16.to_be_bytes());
        out.extend(0_u16.to_be_bytes());
        out.extend([0; 10]);
        out.extend(34_u32.to_be_bytes());
        out.extend([0; 16]);
        // The base glyph list, its one record's paint right after it.
        out.extend(1_u32.to_be_bytes());
        out.extend(1_u16.to_be_bytes());
        out.extend(10_u32.to_be_bytes());
        for _ in 0..depth {
            // PaintComposite: the next paint as source and backdrop, 8 bytes on.
            out.extend([32, 0, 0, 8, 3, 0, 0, 8]);
        }
        out.push(2);
        out.extend(index.to_be_bytes());
        out.extend(0x4000_u16.to_be_bytes());
        out
    }

    fn sound(tables: &[(&[u8; 4], Vec<u8>)]) -> bool {
        let data = sfnt(tables);
        color_tables_sound(&skrifa::FontRef::new(&data).expect("a font"))
    }

    #[test]
    fn rejects_color_tables_that_chrome_rejects() {
        assert!(sound(&[
            (b"CPAL", cpal(2, &[0, 2], 4)),
            (b"COLR", colr_v0(&[1, 0xFFFF]))
        ]));
        // A layer in a color past the palette's end.
        assert!(!sound(&[
            (b"CPAL", cpal(2, &[0, 2], 4)),
            (b"COLR", colr_v0(&[2]))
        ]));
        // Layers in colors without a palette.
        assert!(!sound(&[(b"COLR", colr_v0(&[0]))]));
        // Palettes of colors past the color records.
        assert!(!sound(&[
            (b"CPAL", cpal(2, &[0, 3], 4)),
            (b"COLR", colr_v0(&[0]))
        ]));
        assert!(!sound(&[(b"CPAL", cpal(u16::MAX, &[0; 300], 1))]));
        // Paints of version 1, shared 2^10 times over in a chain of composites, each looked at
        // once.
        assert!(sound(&[(b"CPAL", cpal(2, &[0], 2)), (b"COLR", colr_v1(10, 1))]));
        assert!(!sound(&[(b"CPAL", cpal(2, &[0], 2)), (b"COLR", colr_v1(10, 2))]));
        // Shared 2^60 times over: drawing the glyph would take as many steps.
        assert!(!sound(&[(b"CPAL", cpal(2, &[0], 2)), (b"COLR", colr_v1(60, 1))]));
    }

    #[test]
    fn reads_only_the_palettes_it_uses() {
        // A table that claims 65535 palettes of 65535 colors, with one color record: the colors
        // are not made up (that would be 16 GB), the font is not sound.
        let data = sfnt(&[(b"CPAL", cpal(u16::MAX, &vec![0; 65535], 1))]);
        let font = skrifa::FontRef::new(&data).expect("a font");
        assert!(Palettes::read(&font).is_none());
        let data = sfnt(&[(b"CPAL", cpal(3, &vec![0; 65535], 3))]);
        let font = skrifa::FontRef::new(&data).expect("a font");
        let palettes = Palettes::read(&font).expect("palettes");
        assert_eq!(
            palettes.colors(65534),
            vec![[0, 0, 0, 255], [0, 0, 1, 255], [0, 0, 2, 255]]
        );
    }
}

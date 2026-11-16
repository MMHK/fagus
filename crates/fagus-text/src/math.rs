//! The OpenType `MATH` table of a face, which `MathML` layout reads as Chrome reads it through
//! `HarfBuzz` (`OpenTypeMathSupport`): the constants of formula layout, italic corrections and
//! accent attachments of glyphs, and the larger variants and glyph assemblies that stretch an
//! operator. All lengths are in font units.

use skrifa::raw::TableProvider;
use skrifa::raw::tables::math::{GlyphAssembly, Math, StretchAxis};

pub use skrifa::raw::tables::math::MathConstant;

use crate::Face;

/// One part of a glyph assembly (OpenType `GlyphPartRecord`), in font units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphPart {
    pub glyph: u32,
    pub start_connector: f32,
    pub end_connector: f32,
    pub full_advance: f32,
    /// Whether the part repeats to make the assembly longer.
    pub extender: bool,
}

impl Face {
    /// Runs `read` on the face's `MATH` table, if it has one that reads.
    fn with_math<T>(&self, read: impl FnOnce(&Math<'_>) -> Option<T>) -> Option<T> {
        let font = skrifa::FontRef::from_index(&self.data, self.index).ok()?;
        let math = font.math().ok()?;
        read(&math)
    }

    /// Whether the face has a `MATH` table.
    pub fn has_math(&self) -> bool {
        self.with_math(|_| Some(())).is_some()
    }

    /// A constant of the face's `MATH` table, in font units; the percentages
    /// (`scriptPercentScaleDown`, `scriptScriptPercentScaleDown`,
    /// `radicalDegreeBottomRaisePercent`) as fractions. `None` without a table.
    pub fn math_constant(&self, constant: MathConstant) -> Option<f32> {
        self.with_math(|math| {
            let value = math.math_constants().ok()?.constant(constant) as f32;
            Some(match constant {
                MathConstant::ScriptPercentScaleDown
                | MathConstant::ScriptScriptPercentScaleDown
                | MathConstant::RadicalDegreeBottomRaisePercent => value / 100.0,
                _ => value,
            })
        })
    }

    /// The italic correction of a glyph in font units: 0 where the table gives none, `None`
    /// without a table (`HarfBuzz`'s `hb_ot_math_get_glyph_italics_correction`).
    pub fn math_italic_correction(&self, glyph: u32) -> Option<f32> {
        self.with_math(|math| {
            let correction = math
                .math_glyph_info()
                .ok()
                .and_then(|info| info.math_italics_correction_info()?.ok())
                .and_then(|info| info.correction(skrifa::GlyphId::new(glyph)));
            Some(correction.unwrap_or(0) as f32)
        })
    }

    /// Where an accent over a glyph is centered, from the glyph's origin in font units, if the
    /// table says.
    pub fn math_top_accent_attachment(&self, glyph: u32) -> Option<f32> {
        self.with_math(|math| {
            let attachment = math
                .math_glyph_info()
                .ok()?
                .math_top_accent_attachment()?
                .ok()?
                .attachment(skrifa::GlyphId::new(glyph))?;
            Some(attachment as f32)
        })
    }

    /// The larger variants of a glyph along an axis (vertical or horizontal), smallest first, as
    /// the table lists them (the glyph itself is often the first).
    pub fn math_variants(&self, glyph: u32, vertical: bool) -> Vec<u32> {
        self.with_math(|math| {
            let construction = math
                .math_variants()
                .ok()?
                .glyph_construction(skrifa::GlyphId::new(glyph), axis(vertical))?;
            Some(
                construction
                    .math_glyph_variant_records()
                    .iter()
                    .map(|record| u32::from(record.variant_glyph().to_u16()))
                    .collect(),
            )
        })
        .unwrap_or_default()
    }

    /// The parts of the glyph assembly that stretches a glyph along an axis, from the bottom or
    /// the left, with the italic correction of the assembly; `None` where there is none.
    pub fn math_assembly(&self, glyph: u32, vertical: bool) -> Option<(Vec<GlyphPart>, f32)> {
        self.with_math(|math| {
            let construction = math
                .math_variants()
                .ok()?
                .glyph_construction(skrifa::GlyphId::new(glyph), axis(vertical))?;
            let assembly: GlyphAssembly<'_> = construction.glyph_assembly()?.ok()?;
            let parts: Vec<GlyphPart> = assembly
                .part_records()
                .iter()
                .map(|part| GlyphPart {
                    glyph: u32::from(part.glyph_id().to_u16()),
                    start_connector: f32::from(part.start_connector_length().to_u16()),
                    end_connector: f32::from(part.end_connector_length().to_u16()),
                    full_advance: f32::from(part.full_advance().to_u16()),
                    extender: part
                        .part_flags()
                        .contains(skrifa::raw::tables::math::PartFlags::EXTENDER_FLAG),
                })
                .collect();
            if parts.is_empty() {
                return None;
            }
            Some((parts, f32::from(assembly.italics_correction().value().to_i16())))
        })
    }

    /// The least that the parts of a glyph assembly overlap, in font units.
    pub fn math_min_connector_overlap(&self) -> f32 {
        self.with_math(|math| {
            Some(f32::from(
                math.math_variants().ok()?.min_connector_overlap().to_u16(),
            ))
        })
        .unwrap_or(0.0)
    }

    /// The box around all the points of a glyph's outline at `size` pixels, the control points
    /// of its curves too, as Core Text measures glyphs: left, top, right and bottom from the
    /// glyph's origin with y down. `None` for a glyph without outline. The box is measured in
    /// font units and scaled as Core Text scales it, without the fixed-point rounding of
    /// outlines drawn at a size.
    pub fn glyph_control_box(&self, glyph: u32, size: f32) -> Option<[f32; 4]> {
        struct Points(Option<[f32; 4]>);
        impl Points {
            fn add(&mut self, x: f32, y: f32) {
                let [left, top, right, bottom] = self.0.unwrap_or([x, -y, x, -y]);
                self.0 = Some([left.min(x), top.min(-y), right.max(x), bottom.max(-y)]);
            }
        }
        impl skrifa::outline::OutlinePen for Points {
            fn move_to(&mut self, x: f32, y: f32) {
                self.add(x, y);
            }
            fn line_to(&mut self, x: f32, y: f32) {
                self.add(x, y);
            }
            fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
                self.add(cx, cy);
                self.add(x, y);
            }
            fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
                self.add(cx0, cy0);
                self.add(cx1, cy1);
                self.add(x, y);
            }
            fn close(&mut self) {}
        }
        let mut points = Points(None);
        // Drawn at one pixel per font unit.
        let units = self.metrics.units_per_em / self.size_adjust;
        if !units.is_finite() || !self.draw_glyph(glyph, units, &[], &mut points) {
            return None;
        }
        let scale = f64::from(size) * f64::from(self.size_adjust) / f64::from(self.metrics.units_per_em);
        #[allow(clippy::cast_possible_truncation, reason = "a length in pixels")]
        let scaled = |value: f32| (f64::from(value) * scale) as f32;
        points
            .0
            .map(|[left, top, right, bottom]| [scaled(left), scaled(top), scaled(right), scaled(bottom)])
            .filter(|[left, top, right, bottom]| right > left && bottom > top)
    }

    /// The advance of a glyph along a horizontal line, in font units.
    pub fn glyph_advance(&self, glyph: u32) -> f32 {
        use skrifa::MetadataProvider;
        let Ok(font) = skrifa::FontRef::from_index(&self.data, self.index) else {
            return 0.0;
        };
        font.glyph_metrics(
            skrifa::instance::Size::unscaled(),
            skrifa::instance::LocationRef::default(),
        )
        .advance_width(skrifa::GlyphId::new(glyph))
        .unwrap_or(0.0)
    }
}

fn axis(vertical: bool) -> StretchAxis {
    if vertical {
        StretchAxis::Vertical
    } else {
        StretchAxis::Horizontal
    }
}

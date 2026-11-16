//! The math font of a style and the glyphs that stretch an operator to a size: a larger variant
//! or an assembly of parts, as Blink's `StretchyOperatorShaper` picks and places them.

use fagus_style::ComputedStyle;
use fagus_text::{Face, FaceId, FontDb, MathConstant};

use crate::inline::{face_for, used_font_size};

/// The first available font of a style at its size, with its `MATH` table: what Blink's
/// `PrimaryFont()` is to math layout.
pub(crate) struct MathFont<'a> {
    pub(crate) id: FaceId,
    pub(crate) face: &'a Face,
    /// The font size in pixels.
    pub(crate) size: f32,
    /// Pixels per font unit.
    scale: f32,
    fonts: &'a FontDb,
    /// Whether the operators of the style's text stand right to left, mirrored.
    rtl: bool,
}

impl<'a> MathFont<'a> {
    pub(crate) fn of(fonts: &'a FontDb, style: &ComputedStyle) -> Option<Self> {
        let id = face_for(fonts, style)?;
        let face = fonts.face(id);
        let size = used_font_size(fonts, style);
        Some(Self {
            id,
            face,
            size,
            scale: face.scale(size),
            fonts,
            rtl: style.is_rtl(),
        })
    }

    /// A constant of the `MATH` table in pixels (percentages as fractions); `None` without a
    /// table.
    pub(crate) fn constant(&self, constant: MathConstant) -> Option<f32> {
        let value = self.face.math_constant(constant)?;
        Some(match constant {
            MathConstant::ScriptPercentScaleDown
            | MathConstant::ScriptScriptPercentScaleDown
            | MathConstant::RadicalDegreeBottomRaisePercent => value,
            _ => value * self.scale,
        })
    }

    pub(crate) fn x_height(&self) -> f32 {
        self.face.metrics.x_height * self.scale
    }

    /// The thickness of underlines, which rules take without a `MATH` table.
    pub(crate) fn rule_thickness(&self) -> f32 {
        self.face.metrics.underline_thickness * self.scale
    }

    /// The glyph of a character; right to left, that of its mirror image where it has one, as
    /// shaping it right to left gives it (Blink's `GlyphForMathCharacter`).
    pub(crate) fn glyph(&self, character: char) -> Option<u32> {
        if self.rtl {
            let mut buffer = [0; 4];
            let text = character.encode_utf8(&mut buffer);
            let shaped = self.fonts.shape_directed(self.id, text, &[], &[], true);
            if let Some(glyph) = shaped.glyphs.first().filter(|glyph| glyph.id != 0) {
                return Some(glyph.id);
            }
        }
        self.face.glyph(character).filter(|glyph| *glyph != 0)
    }

    pub(crate) fn advance(&self, glyph: u32) -> f32 {
        self.face.glyph_advance(glyph) * self.scale
    }

    /// The box of a glyph's ink: left, top, right and bottom from its origin with y down;
    /// empty for a glyph without ink (see [`glyph_box`]).
    pub(crate) fn bounds(&self, glyph: u32) -> [f32; 4] {
        glyph_box(self.face, glyph, self.size).unwrap_or([0.0; 4])
    }

    pub(crate) fn italic_correction(&self, glyph: u32) -> f32 {
        self.face.math_italic_correction(glyph).unwrap_or(0.0) * self.scale
    }
}

/// The box of a glyph's outline at `size` pixels as Core Text gives it to Chrome on macOS (see
/// [`Face::glyph_control_box`]).
pub(crate) fn glyph_box(face: &Face, glyph: u32, size: f32) -> Option<[f32; 4]> {
    face.glyph_control_box(glyph, size)
}

/// A stretched operator: its glyphs with their origins (from the operator's origin, y down) and
/// its metrics (Blink's `StretchyOperatorShaper::Metrics`).
#[derive(Clone, Debug, Default)]
pub(crate) struct Stretched {
    pub(crate) glyphs: Vec<(u32, f32, f32)>,
    pub(crate) advance: f32,
    pub(crate) ascent: f32,
    pub(crate) descent: f32,
    pub(crate) italic: f32,
}

/// Stretches the glyph of `character` along an axis to at least `target` pixels: the first
/// variant whose ink is that long, else an assembly of the glyph's parts, else its largest
/// variant.
pub(crate) fn stretch(font: &MathFont<'_>, character: char, vertical: bool, target: f32) -> Stretched {
    let Some(base) = font.glyph(character) else {
        return Stretched::default();
    };
    let mut result = Stretched::default();
    let variants = std::iter::once(base).chain(font.face.math_variants(base, vertical));
    for variant in variants {
        let [left, top, right, bottom] = font.bounds(variant);
        result = Stretched {
            glyphs: vec![(variant, 0.0, 0.0)],
            advance: font.advance(variant),
            ascent: -top,
            descent: bottom,
            italic: font.italic_correction(variant),
        };
        let size = if vertical { bottom - top } else { right - left };
        if size >= target {
            return result;
        }
    }
    assembly(font, base, vertical, target).unwrap_or(result)
}

/// The widths of the glyphs a vertical stretchy operator of `character` may take: the glyph,
/// its variants and the parts of its assembly (Blink's
/// `GetMinMaxSizesForVerticalStretchyOperator`). The widest of them is the operator's width.
pub(crate) fn vertical_width(font: &MathFont<'_>, character: char) -> f32 {
    let Some(base) = font.glyph(character) else {
        return 0.0;
    };
    let parts = font
        .face
        .math_assembly(base, true)
        .map(|(parts, _)| parts)
        .unwrap_or_default();
    std::iter::once(base)
        .chain(font.face.math_variants(base, true))
        .chain(parts.iter().map(|part| part.glyph))
        .map(|glyph| crate::math::layout::lu(font.advance(glyph)))
        .fold(0.0, f32::max)
}

/// The most glyphs an assembly has, as many as Blink keeps in a run.
const MAX_GLYPHS: usize = 65535;

/// An assembly of the parts of `base` at least `target` long (`MathML` Core, §3.2.4.4), placed
/// as Blink places it: horizontally on the baseline from the left; vertically from the top
/// down, each part's ink starting where the one above it ends less the overlap, the bottom of
/// the assembly on the baseline.
fn assembly(font: &MathFont<'_>, base: u32, vertical: bool, target: f32) -> Option<Stretched> {
    let (parts, italic) = font.face.math_assembly(base, vertical)?;
    let scale = font.scale;
    let min_overlap = font.face.math_min_connector_overlap() * scale;
    let mut max_overlap = f32::MAX;
    let (mut non_extender_sum, mut extender_sum) = (0.0_f32, 0.0_f32);
    let (mut non_extenders, mut extenders) = (0_usize, 0_usize);
    let last = parts.len() - 1;
    for (index, part) in parts.iter().enumerate() {
        let advance = part.full_advance * scale;
        if part.extender {
            extenders += 1;
            extender_sum += advance;
        } else {
            non_extenders += 1;
            non_extender_sum += advance;
        }
        if part.extender || index != 0 {
            max_overlap = max_overlap.min(part.start_connector * scale);
        }
        if part.extender || index != last {
            max_overlap = max_overlap.min(part.end_connector * scale);
        }
    }
    let extender_net = extender_sum - min_overlap * extenders as f32;
    if extenders == 0 || max_overlap < min_overlap || extender_net <= 0.0 {
        return None;
    }
    let repetitions = ((target - non_extender_sum + min_overlap * (non_extenders as f32 - 1.0))
        / extender_net)
        .ceil()
        .max(0.0);
    let repetitions = (repetitions.min(1e6) as usize).min((MAX_GLYPHS - non_extenders) / extenders);
    let count = non_extenders + repetitions * extenders;
    let mut overlap = max_overlap;
    if count > 1 {
        let theoretical =
            (non_extender_sum + repetitions as f32 * extender_sum - target) / (count as f32 - 1.0);
        overlap = min_overlap.max(overlap.min(theoretical));
    }
    let stretch_size = non_extender_sum + repetitions as f32 * extender_sum - overlap * (count as f32 - 1.0);
    // The glyphs in the order Blink's run holds them: left to right, or top to bottom.
    let mut sequence: Vec<u32> = Vec::with_capacity(count);
    let mut advances: Vec<f32> = Vec::with_capacity(count);
    for part in &parts {
        let times = if part.extender { repetitions } else { 1 };
        for _ in 0..times {
            sequence.push(part.glyph);
            advances.push(part.full_advance * scale);
        }
    }
    if vertical {
        sequence.reverse();
        advances.reverse();
    }
    let mut glyphs = Vec::with_capacity(count);
    let mut position = 0.0_f32;
    let (mut ink_left, mut ink_top, mut ink_right, mut ink_bottom) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for (index, (&glyph, &advance)) in sequence.iter().zip(&advances).enumerate() {
        let advance = if index == count - 1 {
            advance
        } else {
            advance - overlap
        };
        let [left, top, right, bottom] = font.bounds(glyph);
        let (x, y) = if vertical {
            (0.0, position - stretch_size - top)
        } else {
            (position, 0.0)
        };
        if right > left {
            ink_left = ink_left.min(x + left);
            ink_right = ink_right.max(x + right);
            ink_top = ink_top.min(y + top);
            ink_bottom = ink_bottom.max(y + bottom);
        }
        glyphs.push((glyph, x, y));
        position += crate::math::layout::lu_round(advance);
    }
    if ink_left > ink_right {
        (ink_left, ink_top, ink_right, ink_bottom) = (0.0, 0.0, 0.0, 0.0);
    }
    let italic = italic * scale;
    Some(if vertical {
        Stretched {
            advance: ink_right - ink_left,
            ascent: ink_bottom - ink_top,
            descent: 0.0,
            glyphs,
            italic,
        }
    } else {
        Stretched {
            advance: ink_right - ink_left,
            ascent: -ink_top,
            descent: ink_bottom,
            glyphs,
            italic,
        }
    })
}

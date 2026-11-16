//! Emphasis marks (`text-emphasis`, CSS Text Decoration 3, §3), sized and placed as Chrome
//! places them: the first glyph of the mark's text at half the font size, its ink centered over
//! each character that takes a mark, just past the em box of the text.

use fagus_style::ComputedStyle;
use fagus_style::values::AnnotationSide;
use fagus_text::{FaceId, FontDb};
use unicode_properties::{GeneralCategory, UnicodeGeneralCategory};

use crate::fragment::{Glyph, TextRun};
use crate::inline::{Metrics, alphabetic_metrics, face_for};

/// The glyph of an emphasis mark and its vertical metrics.
#[derive(Clone, Debug)]
pub(crate) struct Mark {
    pub face: FaceId,
    pub glyph: u32,
    /// The size of the mark: half the text's, rounded.
    pub size: f32,
    /// The em box of the mark's face at its size: above and below the baseline.
    pub ascent: f32,
    pub descent: f32,
    /// The middle of the glyph's ink, from its origin.
    pub center: f32,
    /// The coordinates of a variable face: those of the text's font, which Chrome scales down
    /// for the marks.
    pub variations: Vec<([u8; 4], f32)>,
}

/// Emphasis marks of a text run, ready to draw: glyphs of `face` at `size`, from `x` after the
/// start of the run on a baseline `offset` below the run's.
#[derive(Clone, Debug)]
pub struct Marks {
    pub face: FaceId,
    pub size: f32,
    pub x: f32,
    pub offset: f32,
    pub glyphs: Vec<Glyph>,
    /// The marks as text: one copy of the mark for each glyph.
    pub text: String,
    /// The coordinates of a variable face, those of the text's font.
    pub variations: Vec<([u8; 4], f32)>,
}

/// Rounds to the units of 1/64 pixel that Chrome lays out in.
fn units(value: f32) -> f32 {
    (value * 64.0).round() / 64.0
}

/// The em box of a face at `size`, above and below the baseline, as Chrome normalizes it: the
/// typographic ascender and descender (else ascender and descender) scaled to add up to the
/// size, kept in their ratio.
pub(crate) fn em_box(fonts: &FontDb, face: FaceId, size: f32) -> (f32, f32) {
    let metrics = &fonts.face(face).metrics;
    let (ascent, descent) = if metrics.typo_ascender > 0.0 {
        (metrics.typo_ascender, metrics.typo_descender)
    } else {
        (metrics.ascender, metrics.descender)
    };
    let height = ascent + descent;
    if height <= 0.0 || ascent < 0.0 || ascent > height {
        return (units(size) - units(size / 2.0), units(size / 2.0));
    }
    let above = units(ascent * size / height);
    (above, units(size) - above)
}

/// How far the em box of text in `face` at `size` lies inside the ascent and descent of the
/// style's first available font (`primary`), in whole pixels: above and below.
pub(crate) fn em_insets(fonts: &FontDb, face: FaceId, size: f32, primary: &Metrics) -> (f32, f32) {
    let (ascent, descent) = em_box(fonts, face, size);
    (
        (primary.ascent - ascent).max(0.0).floor(),
        (primary.descent - descent).max(0.0).floor(),
    )
}

/// The emphasis mark of text in `style` set at `size`: the first glyph of the mark's text in the
/// first face of the style's fonts that has it.
pub(crate) fn mark(fonts: &FontDb, style: &ComputedStyle, size: f32) -> Option<Mark> {
    let text = style.text_emphasis_style.as_ref()?.text();
    let first = text.chars().next()?;
    let preferred = face_for(fonts, style)?;
    let families: Vec<&str> = style.font_family.iter().map(String::as_str).collect();
    let font_style = crate::inline::text_font_style(style.font_style);
    let face = fonts
        .fallback_in(
            &families,
            preferred,
            first,
            (style.font_weight, font_style, style.font_stretch),
            style.lang.as_deref(),
        )
        .unwrap_or(preferred);
    // Where small capitals are made up, Chrome sets the mark in capitals too.
    let upper;
    let text = if made_up_small_caps(fonts, face, style) {
        upper = text.to_uppercase();
        upper.as_str()
    } else {
        text
    };
    // The mark is shaped in the language of its text, which picks a face's forms for it.
    let flow = fagus_text::Flow::LeftToRight;
    let shaped = fonts.shape_in_language(face, text, &[], &[], flow, style.lang.as_deref());
    let glyph = shaped.glyphs.first()?.id;
    let variations = crate::inline::font_variations(fonts, face, style, size);
    let size = (size * 0.5).round();
    let (ascent, descent) = em_box(fonts, face, size);
    let center = if variations.is_empty() {
        fonts
            .face(face)
            .glyph_bounds(glyph, size)
            .map(|[left, _, right, _]| (left, right))
    } else {
        fonts
            .face(face)
            .intercept(glyph, size, &variations, (-10.0 * size, 10.0 * size))
    }
    .map_or(0.0, |(left, right)| f32::midpoint(left, right));
    Some(Mark {
        face,
        glyph,
        size,
        ascent,
        descent,
        center,
        variations,
    })
}

/// Whether text in `style` set in `face` takes small or petite capitals that the face does not
/// have, which are made up from its capitals.
fn made_up_small_caps(fonts: &FontDb, face: FaceId, style: &ComputedStyle) -> bool {
    use fagus_style::values::FontVariantCaps as Caps;
    let feature = match style.font_variant.caps {
        Caps::SmallCaps | Caps::AllSmallCaps => *b"smcp",
        Caps::PetiteCaps | Caps::AllPetiteCaps => *b"pcap",
        _ => return false,
    };
    !fonts.face(face).substitutes(feature) && !fonts.face(face).substitutes(*b"smcp")
}

/// How far an emphasis mark reaches past the text box of a run (the ascent and descent of its
/// style's first font) over or under it: its own height less the inset of the run's em box.
/// Over a ruby annotation on the same side (`annotation`, how far it reaches from the baseline),
/// the mark goes past that instead.
pub(crate) fn reach(fonts: &FontDb, run: &TextRun, annotation: Option<f32>) -> Option<(AnnotationSide, f32)> {
    // Made-up small capitals take the marks of the text they stand for.
    let size = crate::inline::unscaled_size(fonts, run);
    let mark = mark(fonts, &run.style, size)?;
    let primary = alphabetic_metrics(fonts, &run.style);
    let (over, under) = em_insets(fonts, run.face, size, &primary);
    let height = mark.ascent + mark.descent;
    Some(match run.style.text_emphasis_position {
        AnnotationSide::Over => (
            AnnotationSide::Over,
            annotation.map_or(height - over, |edge| edge - primary.ascent + height),
        ),
        AnnotationSide::Under => (
            AnnotationSide::Under,
            annotation.map_or(height - under, |edge| edge - primary.descent + height),
        ),
    })
}

/// Whether a character takes an emphasis mark: not spaces, separators, controls and
/// punctuation, except for the symbols the specification names (as in Chrome's
/// `CanReceiveTextEmphasis`).
pub(crate) fn takes_mark(character: char) -> bool {
    use GeneralCategory as Category;
    let category = character.general_category();
    if matches!(
        category,
        Category::SpaceSeparator
            | Category::LineSeparator
            | Category::ParagraphSeparator
            | Category::Unassigned
            | Category::Control
            | Category::Format
    ) {
        return false;
    }
    // Word separators of CSS Text 3.
    if matches!(
        character,
        '\u{1361}' | '\u{10100}' | '\u{10101}' | '\u{1039F}' | '\u{0F0B}' | '\u{0F0C}'
    ) {
        return false;
    }
    if matches!(
        character,
        '#' | '%'
            | '&'
            | '@'
            | '\u{A7}'
            | '\u{B6}'
            | '\u{609}'
            | '\u{60A}'
            | '\u{66A}'
            | '\u{2030}'
            | '\u{2031}'
            | '\u{204A}'
            | '\u{204B}'
            | '\u{2053}'
            | '\u{303D}'
            | '\u{FE5F}'
            | '\u{FE60}'
            | '\u{FE6A}'
            | '\u{FE6B}'
            | '\u{FF03}'
            | '\u{FF05}'
            | '\u{FF06}'
            | '\u{FF20}'
    ) {
        return true;
    }
    !matches!(
        category,
        Category::DashPunctuation
            | Category::OpenPunctuation
            | Category::ClosePunctuation
            | Category::ConnectorPunctuation
            | Category::OtherPunctuation
            | Category::InitialPunctuation
            | Category::FinalPunctuation
    )
}

/// Whether a character continues the grapheme cluster before it (a combining mark, a joiner or
/// a variation selector), roughly as grapheme clusters are told apart.
fn extends(character: char) -> bool {
    matches!(
        character.general_category(),
        GeneralCategory::NonspacingMark | GeneralCategory::SpacingMark | GeneralCategory::EnclosingMark
    ) || matches!(character, '\u{200C}' | '\u{200D}' | '\u{FE00}'..='\u{FE0F}' | '\u{E0020}'..='\u{E007F}')
}

/// The first characters of the grapheme clusters of `text`.
fn grapheme_starts(text: &str) -> Vec<char> {
    let mut starts = Vec::new();
    let mut joined = false;
    for character in text.chars() {
        if starts.is_empty() || !(joined || extends(character)) {
            starts.push(character);
        }
        joined = character == '\u{200D}';
    }
    starts
}

/// The emphasis marks of a text run: one over (or under) the middle of each grapheme cluster
/// that takes a mark, with the mark's ink centered there and letter spacing left out, on a
/// baseline just past the run's em box (as Chrome's `TextPainter` places it).
///
/// In the base of a ruby column whose annotation is on the same side, the marks go past the
/// annotation: `annotation` tells how far that reaches from the run's baseline.
pub fn marks(fonts: &FontDb, run: &TextRun, annotation: Option<f32>) -> Option<Marks> {
    // Made-up small capitals take the marks of the text they stand for.
    let size = crate::inline::unscaled_size(fonts, run);
    let mark = mark(fonts, &run.style, size)?;
    // The run's baseline is its alphabetic one, in vertical lines too.
    let primary = alphabetic_metrics(fonts, &run.style);
    let (over, under) = em_insets(fonts, run.face, size, &primary);
    let offset = match (run.style.text_emphasis_position, annotation) {
        (AnnotationSide::Over, None) => (-primary.ascent + over - mark.descent).floor(),
        (AnnotationSide::Over, Some(edge)) => (-edge - mark.descent).floor(),
        (AnnotationSide::Under, None) => (primary.descent - under + mark.ascent).ceil(),
        (AnnotationSide::Under, Some(edge)) => (edge + mark.ascent).ceil(),
    };
    let mark_text = run.style.text_emphasis_style.as_ref()?.text();
    let spacing = run.style.letter_spacing;
    // Where the marks start, from the start of the run.
    let mut positions: Vec<f32> = Vec::new();
    let mut x = 0.0;
    let mut index = 0;
    while index < run.glyphs.len() {
        let cluster = run.glyphs[index].cluster;
        let mut advance = 0.0;
        while index < run.glyphs.len() && run.glyphs[index].cluster == cluster {
            advance += run.glyphs[index].advance;
            index += 1;
        }
        let cluster_text = run
            .text
            .get(cluster.0 as usize..cluster.1 as usize)
            .unwrap_or_default();
        let starts = grapheme_starts(cluster_text);
        let share = advance / starts.len().max(1) as f32;
        for (position, character) in starts.into_iter().enumerate() {
            if takes_mark(character) {
                positions.push(x + share * (position as f32 + 0.5) - mark.center - spacing / 2.0);
            }
        }
        x += advance;
    }
    let first = *positions.first()?;
    let mut text = String::new();
    let glyphs = positions
        .iter()
        .enumerate()
        .map(|(index, at)| {
            let start = text.len() as u32;
            text.push_str(mark_text);
            Glyph {
                id: mark.glyph,
                advance: positions.get(index + 1).map_or(0.0, |next| next - at),
                x_offset: 0.0,
                y_offset: 0.0,
                cluster: (start, text.len() as u32),
            }
        })
        .collect();
    Some(Marks {
        face: mark.face,
        size: mark.size,
        x: first,
        offset,
        glyphs,
        text,
        variations: mark.variations,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_letters_but_not_spaces_and_punctuation() {
        for character in ['a', 'Z', '字', 'あ', '1', '#', '%', '&', '@', '§', '+', '$'] {
            assert!(takes_mark(character), "{character}");
        }
        for character in [
            ' ', '\u{3000}', '\u{A0}', ',', '.', '、', '。', '(', ')', '“', '-', '_', '!', '\u{200B}',
        ] {
            assert!(!takes_mark(character), "{character:?}");
        }
    }

    #[test]
    fn combining_marks_belong_to_their_letter() {
        assert_eq!(grapheme_starts("e\u{301}x"), vec!['e', 'x']);
        assert_eq!(grapheme_starts("ab"), vec!['a', 'b']);
        assert_eq!(grapheme_starts("\u{1F468}\u{200D}\u{1F469}"), vec!['\u{1F468}']);
    }
}

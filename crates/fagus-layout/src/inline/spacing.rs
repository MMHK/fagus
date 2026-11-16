//! Spacing that characters take from their neighbours, as Chrome sets it: fullwidth punctuation
//! that loses the blank half of its glyph next to other punctuation and at the edges of lines
//! (`text-spacing-trim`, CSS Text 4, §8.3), and the space between ideographs and the letters and
//! digits of other scripts (`text-autospace`, §8.2).
//!
//! The classes of punctuation, how fonts resolve them and where the halves go follow Blink's
//! `han_kerning.cc`; the spacing between ideographs and other letters follows its
//! `text_auto_space.cc`. Both are under their BSD license (`CHROMIUM-LICENSE`). The classes of
//! UTR #59 come from the data of Unicode 16, under the Unicode License V3 (`UNICODE-LICENSE`).

use std::collections::VecDeque;

use fagus_style::ComputedStyle;
use fagus_style::values::{TextOrientation, TextSpacingTrim};
use fagus_text::{FaceId, FontDb};
use rustc_hash::FxHashMap;
use unicode_properties::{GeneralCategory, GeneralCategoryGroup, UnicodeGeneralCategory};

use super::{Caps, Piece, PieceKind, face_for, font_variations, used_font_size};
use crate::tree::BoxId;

/// How the glyph of one character changes when it loses its blank half: its advance and its
/// offset along the line, in pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Trim {
    advance: f32,
    offset: f32,
}

/// What the edges of a text piece take where lines start or end at it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Edges {
    /// The trim its first glyph has now: from the character before it, or at the start of a
    /// paragraph.
    start_kerned: Option<Trim>,
    /// The trim its first glyph, opening punctuation, takes at the start of a wrapped line.
    start: Option<Trim>,
    /// The trim its last glyph, closing punctuation, takes at the end of a line it does not fit
    /// otherwise.
    end: Option<Trim>,
    /// The space after its last glyph before an ideograph or a letter (`text-autospace`), which
    /// the end of a line drops.
    pub(super) autospace: f32,
    /// For a ruby column: its box and the first and last letters of its base, which take part
    /// in `text-autospace` with the text around the column as the text of the line does.
    ruby: Option<(BoxId, char, char)>,
    /// For a ruby column: the space of `text-autospace` before its base, inside the column.
    pub(super) ruby_lead: f32,
    /// Whether its first glyph kerns with the character before it, so that shaping does not
    /// allow to break before it as it is.
    pub(super) kerned_before: bool,
    /// The share of the kerning with the character before it (in another item) that the
    /// advance and offset of its first glyph hold, which the start of a line drops.
    pub(super) kern_before: f32,
    /// The share of the kerning with the first character of the next item that the advance of
    /// its last glyph holds.
    pub(super) kern_after: f32,
}

impl Edges {
    /// The edges of ruby column `id`, whose base starts with letter `first` and ends with
    /// `last`.
    pub(super) fn ruby(id: BoxId, first: char, last: char) -> Self {
        Self {
            ruby: Some((id, first, last)),
            ..Self::default()
        }
    }

    /// The ruby column these are the edges of, if any.
    pub(super) fn ruby_column(&self) -> Option<BoxId> {
        self.ruby.map(|(id, ..)| id)
    }
}

/// The classes of characters for collapsing fullwidth punctuation (CSS Text 4, §8.3.1), Blink's
/// `HanKerningCharType`. The last five depend on the font's glyphs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CharType {
    Other,
    Open,
    Close,
    Middle,
    /// Opening and closing punctuation that is not fullwidth.
    OpenNarrow,
    CloseNarrow,
    Dot,
    Colon,
    Semicolon,
    OpenQuote,
    CloseQuote,
}

/// The class of a character by itself (Blink's `character_property_data_generator.cc`):
/// opening and closing punctuation (`Ps`, `Pe`) is fullwidth in the CJK Symbols block and where
/// its East Asian width is `F`.
fn char_type(character: char) -> CharType {
    match character {
        '\u{2018}' | '\u{201C}' => CharType::OpenQuote,
        '\u{2019}' | '\u{201D}' => CharType::CloseQuote,
        '\u{3000}' | '\u{00B7}' | '\u{2027}' | '\u{30FB}' => CharType::Middle,
        '\u{3001}' | '\u{3002}' | '\u{FF0C}' | '\u{FF0E}' => CharType::Dot,
        '\u{FF1A}' => CharType::Colon,
        '\u{FF1B}' => CharType::Semicolon,
        _ => {
            let fullwidth = matches!(character, '\u{3000}'..='\u{303F}' | '\u{FF01}'..='\u{FF60}' | '\u{FFE0}'..='\u{FFE6}');
            match (character.general_category(), fullwidth) {
                (GeneralCategory::OpenPunctuation, true) => CharType::Open,
                (GeneralCategory::ClosePunctuation, true) => CharType::Close,
                (GeneralCategory::OpenPunctuation, false) => CharType::OpenNarrow,
                (GeneralCategory::ClosePunctuation, false) => CharType::CloseNarrow,
                _ => CharType::Other,
            }
        }
    }
}

/// Whether a character may be opening punctuation in some font (Blink's
/// `MaybeHanKerningOpen`).
fn maybe_open(character: char) -> bool {
    matches!(char_type(character), CharType::Open | CharType::OpenQuote)
}

/// Whether a character may be closing punctuation in some font.
fn maybe_close(character: char) -> bool {
    matches!(char_type(character), CharType::Close | CharType::CloseQuote)
}

/// Whether opening punctuation of `kind` after a character of `last` loses its blank half.
fn should_kern(kind: CharType, last: CharType) -> bool {
    kind == CharType::Open
        && matches!(
            last,
            CharType::Open | CharType::Middle | CharType::Close | CharType::OpenNarrow
        )
}

/// Whether closing punctuation of `last` before a character of `kind` loses its blank half.
fn should_kern_last(kind: CharType, last: CharType) -> bool {
    last == CharType::Close && matches!(kind, CharType::Close | CharType::Middle | CharType::CloseNarrow)
}

/// What the faces of a layout say about their punctuation, by face and whether the glyphs
/// stand upright in a vertical line: found once per layout. And how the opening punctuation
/// that starts a paragraph was spaced where its text was shaped first (see [`starts_trimmed`]).
#[derive(Default)]
pub(crate) struct FontCache {
    faces: std::cell::RefCell<FxHashMap<(FaceId, bool), FontData>>,
    starts: std::cell::RefCell<FxHashMap<(String, FaceId, u32), bool>>,
}

impl FontCache {
    fn borrow_mut(&self) -> std::cell::RefMut<'_, FxHashMap<(FaceId, bool), FontData>> {
        self.faces.borrow_mut()
    }
}

/// Whether opening punctuation that starts a paragraph loses its blank half: with
/// `trim-start`, by the CSS. But Chrome 157 keeps the shapes of text it has shaped once, by its
/// text and font, whatever `text-spacing-trim` the text has after: the text that starts the
/// paragraph is trimmed where it was the first time it was shaped (measured: `「一番目」` with
/// `trim-start` first, then `normal`, starts trimmed both times; with `normal` first, then
/// `trim-start`, neither). `key` is the text of the piece (and those after it of its style),
/// its face and its size.
fn starts_trimmed(cache: &FontCache, key: (String, FaceId, u32), own: bool) -> bool {
    *cache.starts.borrow_mut().entry(key).or_insert(own)
}

/// What a face says about its punctuation, horizontally or vertically (Blink's
/// `HanKerning::FontData`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct FontData {
    /// Whether the face has half widths for its punctuation (`halt`, `vhal`): without them no
    /// punctuation loses its blank half.
    alternate: bool,
    /// Whether the face collapses punctuation itself (`chws`, `vchw`).
    contextual: bool,
    /// Whether its curly quotes are fullwidth, as in Chinese fonts; Japanese ones have them
    /// proportional.
    quotes_fullwidth: bool,
    dot: CharType,
    colon: CharType,
    semicolon: CharType,
}

impl FontData {
    const NONE: Self = Self {
        alternate: false,
        contextual: false,
        quotes_fullwidth: false,
        dot: CharType::Other,
        colon: CharType::Other,
        semicolon: CharType::Other,
    };

    /// Finds the classes of the punctuation whose glyphs differ between fonts from where the
    /// ink of their glyphs lies in their advance.
    fn new(fonts: &FontDb, face: FaceId, vertical: bool) -> Self {
        // Full stops and commas, colon, semicolon, then the quotes.
        const CHARS: [char; 10] = [
            '\u{3001}', '\u{3002}', '\u{FF0C}', '\u{FF0E}', '\u{FF1A}', '\u{FF1B}', '\u{201C}', '\u{2018}',
            '\u{201D}', '\u{2019}',
        ];
        let font = fonts.face(face);
        let (alternate, contextual) = if vertical {
            (*b"vhal", *b"vchw")
        } else {
            (*b"halt", *b"chws")
        };
        if !font.positions(alternate) {
            return Self::NONE;
        }
        let text: String = CHARS.iter().collect();
        let shaped = if vertical {
            fonts.shape_vertical(face, &text, &[], &[])
        } else {
            fonts.shape(face, &text)
        };
        // A face without these glyphs, or with several glyphs for one, does not take part.
        let mut offset = 0;
        for (glyph, character) in shaped.glyphs.iter().zip(CHARS) {
            if glyph.cluster as usize != offset {
                return Self::NONE;
            }
            offset += character.len_utf8();
        }
        if shaped.glyphs.len() != CHARS.len() {
            return Self::NONE;
        }
        // The glyphs in font units: at a font size of an em, less the face's `size-adjust`.
        let em = font.metrics.units_per_em / font.size_adjust.max(f32::MIN_POSITIVE);
        // Each glyph's advance and the extent of its ink along the line, in font units.
        let glyphs: Vec<Option<(f32, f32, f32)>> = shaped
            .glyphs
            .iter()
            .map(|glyph| {
                if glyph.id == 0 {
                    return None;
                }
                let [left, top, right, bottom] = font.glyph_bounds(glyph.id, em)?;
                let (start, end) = if vertical { (top, bottom) } else { (left, right) };
                Some((glyph.x_advance, start + glyph.x_offset, end + glyph.x_offset))
            })
            .collect();
        let classify = |half: f32, (start, end): (f32, f32)| {
            if end <= half {
                CharType::Close
            } else if start >= half {
                CharType::Open
            } else if end - start <= half && start >= half / 2.0 {
                CharType::Middle
            } else {
                CharType::Other
            }
        };
        // The class of a group of glyphs: of the first, where all have its advance and class.
        let group = |glyphs: &[Option<(f32, f32, f32)>]| {
            let mut present = glyphs.iter().flatten();
            let Some(&(advance, start, end)) = present.next() else {
                return CharType::Other;
            };
            let kind = classify(advance / 2.0, (start, end));
            if present.all(|&(other, start, end)| {
                (other - advance).abs() < 0.5 && classify(advance / 2.0, (start, end)) == kind
            }) {
                kind
            } else {
                CharType::Other
            }
        };
        Self {
            alternate: true,
            contextual: font.positions(contextual),
            quotes_fullwidth: group(&glyphs[6..8]) == CharType::Open
                && group(&glyphs[8..10]) == CharType::Close,
            dot: group(&glyphs[..4]),
            colon: group(&glyphs[4..5]),
            semicolon: group(&glyphs[5..6]),
        }
    }

    /// The class of a character in this face.
    fn kind(&self, character: char) -> CharType {
        match char_type(character) {
            CharType::Dot => self.dot,
            CharType::Colon => self.colon,
            CharType::Semicolon => self.semicolon,
            CharType::OpenQuote if self.quotes_fullwidth => CharType::Open,
            CharType::OpenQuote => CharType::OpenNarrow,
            CharType::CloseQuote if self.quotes_fullwidth => CharType::Close,
            CharType::CloseQuote => CharType::CloseNarrow,
            other => other,
        }
    }
}

/// Features that set the widths of glyphs themselves: where the style turns one on, no
/// punctuation collapses (Blink's `ExclusiveFeatures`).
const EXCLUSIVE: [[u8; 4]; 9] = [
    *b"halt", *b"hwid", *b"palt", *b"pwid", *b"qwid", *b"twid", *b"valt", *b"vhal", *b"vpal",
];

/// The feature by which a face collapses punctuation itself (`chws`, or `vchw` for upright
/// glyphs in a vertical line), which Chrome turns on where `text-spacing-trim` lets punctuation
/// collapse, unless `font-feature-settings` sets it or another feature that sets widths.
pub(super) fn contextual_feature(style: &ComputedStyle, upright: bool) -> Option<[u8; 4]> {
    let (contextual, alternate, proportional) = if upright {
        (*b"vchw", *b"vhal", *b"vpal")
    } else {
        (*b"chws", *b"halt", *b"palt")
    };
    let set = style.font_feature_settings.iter().any(|(tag, value)| {
        *tag == contextual || (*value != 0 && (*tag == alternate || *tag == proportional))
    });
    (style.text_spacing_trim != TextSpacingTrim::SpaceAll && !set).then_some(contextual)
}

/// A character of the paragraph's text, or something else that stands between characters (an
/// atomic inline, a forced break).
#[derive(Clone, Copy)]
enum Entry {
    Char {
        piece: usize,
        /// The byte offset of the character in the piece's text.
        offset: usize,
        character: char,
        /// Whether the character starts a paragraph: the first one, or the first after a forced
        /// break.
        paragraph_start: bool,
    },
    /// A ruby column, whose base starts with letter `first` and ends with `last` (its
    /// annotation is not in the text).
    Ruby {
        piece: usize,
        first: char,
        last: char,
    },
    Barrier,
}

/// Sets the spacing of the paragraph's text pieces: collapses adjacent fullwidth punctuation,
/// trims opening punctuation at the start of a paragraph (`trim-start`), notes what the edges
/// of each piece take at the start and the end of a line, and puts the space of
/// `text-autospace` between ideographs and other letters.
pub(super) fn space(
    fonts: &FontDb,
    pieces: &mut [Piece],
    container: Option<&ComputedStyle>,
    cache: &FontCache,
) {
    let mut trims = false;
    let mut autospace = false;
    for piece in pieces.iter() {
        if let PieceKind::Text { text, .. } = &piece.kind
            && !text.is_ascii()
        {
            trims |= piece.style.text_spacing_trim != TextSpacingTrim::SpaceAll
                && text
                    .chars()
                    .any(|character| char_type(character) != CharType::Other);
            autospace |= piece.style.text_autospace;
        }
        // The base of a ruby column is text of the paragraph for `text-autospace`.
        if let Some((_, first, last)) = piece.edges.ruby
            && !(first.is_ascii() && last.is_ascii())
        {
            autospace |= piece.style.text_autospace;
        }
    }
    if !trims && !autospace {
        return;
    }
    let entries = entries(pieces);
    if trims {
        trim_punctuation(fonts, pieces, &entries, cache);
    }
    if autospace {
        let chinese = container
            .or_else(|| pieces.first().map(|piece| &*piece.style))
            .and_then(|style| style.lang.as_deref())
            .is_some_and(|lang| lang.get(..2).is_some_and(|code| code.eq_ignore_ascii_case("zh")));
        insert_autospace(fonts, pieces, &entries, chinese);
    }
}

/// Spaces the two parts of a text piece split inside a word (by `overflow-wrap`) as the piece
/// was: between their characters as Chrome spaces the text of a line, and at their outer edges
/// as the piece was spaced by the text around it.
pub(super) fn respace_split(fonts: &FontDb, cache: &FontCache, piece: &Piece, halves: &mut [Piece; 2]) {
    space(fonts, halves, None, cache);
    let PieceKind::Text { glyphs: original, .. } = &piece.kind else {
        return;
    };
    // The first glyph of the first part and the last of the second, as they were in the piece.
    let Some(start) = first_glyph(piece) else {
        return;
    };
    let offset = text_len(&halves[0]) as u32;
    let edges = [
        (0, start, first_glyph(&halves[0]), 0),
        (
            1,
            last_glyph(piece).unwrap_or(start),
            last_glyph(&halves[1]),
            offset,
        ),
    ];
    for (half, from, to, shift) in edges {
        let Some(to) = to else {
            continue;
        };
        let source = original[from];
        let part = &mut halves[half];
        if let PieceKind::Text { glyphs, .. } = &mut part.kind
            && glyphs[to].id == source.id
            && glyphs[to].cluster.0 + shift == source.cluster.0
        {
            let grown = source.advance - glyphs[to].advance;
            glyphs[to].advance = source.advance;
            glyphs[to].x_offset = source.x_offset;
            part.width += grown;
            part.min_width += grown;
        }
    }
    halves[0].edges.start_kerned = piece.edges.start_kerned;
    halves[0].edges.start = piece.edges.start;
    halves[1].edges.end = piece.edges.end;
    halves[1].edges.autospace = piece.edges.autospace;
}

/// The characters of the text pieces in order, with what stands between them.
fn entries(pieces: &[Piece]) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut paragraph_start = true;
    for (index, piece) in pieces.iter().enumerate() {
        match &piece.kind {
            PieceKind::Text { text, .. } => {
                for (offset, character) in text.char_indices() {
                    entries.push(Entry::Char {
                        piece: index,
                        offset,
                        character,
                        paragraph_start,
                    });
                    paragraph_start = false;
                }
            }
            // The edges of inline boxes are not in the text.
            PieceKind::Open(_) | PieceKind::Close(_) | PieceKind::Left { .. } | PieceKind::Right { .. } => {}
            _ if let Some((_, first, last)) = piece.edges.ruby => {
                entries.push(Entry::Ruby {
                    piece: index,
                    first,
                    last,
                });
                paragraph_start = false;
            }
            _ => {
                entries.push(Entry::Barrier);
                paragraph_start = false;
            }
        }
        if piece.forced_break_after {
            entries.push(Entry::Barrier);
            paragraph_start = true;
        }
    }
    entries
}

/// The face and the caps of a text piece.
fn text_face(piece: &Piece) -> Option<(FaceId, Caps)> {
    match &piece.kind {
        PieceKind::Text { face, caps, .. } => Some((*face, *caps)),
        _ => None,
    }
}

/// The glyph of the character at byte `offset` of a text piece: the only one of a cluster of
/// that character alone.
fn glyph_at(piece: &Piece, offset: usize, character: char) -> Option<usize> {
    let PieceKind::Text { glyphs, .. } = &piece.kind else {
        return None;
    };
    let mut found = None;
    for (index, glyph) in glyphs.iter().enumerate() {
        if glyph.cluster.0 as usize == offset {
            if found.is_some() || glyph.cluster.1 as usize != offset + character.len_utf8() {
                return None;
            }
            found = Some(index);
        }
    }
    found
}

/// The trim of the glyph of `character` in a face with the style's features: the difference
/// that its half width (`halt`, or `vhal` upright in a vertical line) makes. `None` where the
/// face has none for it, or the style sets widths itself.
fn alternate_trim(
    fonts: &FontDb,
    style: &ComputedStyle,
    (face, caps): (FaceId, Caps),
    character: char,
) -> Option<Trim> {
    let size = caps.size(used_font_size(fonts, style));
    let mut features = super::variant_features(fonts, face, style);
    features.extend_from_slice(caps.features);
    features.extend_from_slice(&style.font_feature_settings);
    if features
        .iter()
        .any(|(tag, value)| *value != 0 && EXCLUSIVE.contains(tag))
    {
        return None;
    }
    let variations = font_variations(fonts, face, style, size);
    let mut buffer = [0; 4];
    let text = character.encode_utf8(&mut buffer);
    let flow = if caps.upright {
        fagus_text::Flow::TopToBottom
    } else {
        fagus_text::Flow::LeftToRight
    };
    let shape = |features: &[([u8; 4], u32)]| {
        fonts.shape_in_language(face, text, features, &variations, flow, style.lang.as_deref())
    };
    let plain = shape(&features);
    features.push((if caps.upright { *b"vhal" } else { *b"halt" }, 1));
    let half = shape(&features);
    let ([plain], [half]) = (plain.glyphs.as_slice(), half.glyphs.as_slice()) else {
        return None;
    };
    let scale = fonts.face(face).scale(size);
    let trim = Trim {
        advance: (half.x_advance - plain.x_advance) * scale,
        offset: (half.x_offset - plain.x_offset) * scale,
    };
    (trim.advance < 0.0).then_some(trim)
}

/// Changes the glyph `glyph` of a text piece by `trim` (or takes the change back).
fn apply(piece: &mut Piece, glyph: usize, trim: Trim, undo: bool) {
    let sign = if undo { -1.0 } else { 1.0 };
    if let PieceKind::Text { glyphs, .. } = &mut piece.kind
        && let Some(glyph) = glyphs.get_mut(glyph)
    {
        glyph.advance += sign * trim.advance;
        glyph.x_offset += sign * trim.offset;
        piece.width += sign * trim.advance;
        piece.min_width += sign * trim.advance;
    }
}

/// Collapses fullwidth punctuation (CSS Text 4, §8.3.2) as Blink's `HanKerning` does, and
/// notes the trims of the edges of the pieces.
fn trim_punctuation(fonts: &FontDb, pieces: &mut [Piece], entries: &[Entry], cache: &FontCache) {
    let font_data = |face: FaceId, upright: bool| {
        *cache
            .borrow_mut()
            .entry((face, upright))
            .or_insert_with(|| FontData::new(fonts, face, upright))
    };
    // The glyphs that lose their blank half, with their trim.
    let mut kerned: Vec<(usize, usize, Trim)> = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let Entry::Char {
            piece,
            offset,
            character,
            paragraph_start,
        } = *entry
        else {
            continue;
        };
        let style = &pieces[piece].style;
        // Only fullwidth punctuation loses half its width.
        if style.text_spacing_trim == TextSpacingTrim::SpaceAll
            || matches!(
                char_type(character),
                CharType::Other | CharType::Middle | CharType::OpenNarrow | CharType::CloseNarrow
            )
        {
            continue;
        }
        let Some((face, caps)) = text_face(&pieces[piece]) else {
            continue;
        };
        let fd = font_data(face, caps.upright);
        if !fd.alternate {
            continue;
        }
        let Some(glyph) = glyph_at(&pieces[piece], offset, character) else {
            continue;
        };
        let neighbour = |entry: Option<&Entry>| match entry {
            Some(&Entry::Char { piece, character, .. }) => Some((piece, character)),
            _ => None,
        };
        let previous = neighbour(index.checked_sub(1).and_then(|previous| entries.get(previous)));
        let next = neighbour(entries.get(index + 1));
        // A face that collapses punctuation itself does so within what is shaped together.
        let same_run = |other: usize| {
            fd.contextual
                && text_face(&pieces[other]).is_some_and(|(other_face, other_caps)| {
                    other_face == face
                        && other_caps == caps
                        && std::sync::Arc::ptr_eq(&pieces[other].style, style)
                        && other.abs_diff(piece) <= 1
                })
        };
        let kind = fd.kind(character);
        let by_previous =
            previous.is_some_and(|(other, before)| !same_run(other) && should_kern(kind, fd.kind(before)));
        let by_next =
            next.is_some_and(|(other, after)| !same_run(other) && should_kern_last(fd.kind(after), kind));
        // Opening punctuation the face itself collapses after the character before, which it
        // takes back where a line starts with it: Chrome shapes a line from its start.
        let by_face = offset == 0
            && previous.is_some_and(|(other, before)| same_run(other) && should_kern(kind, fd.kind(before)));
        let trim_start = matches!(
            style.text_spacing_trim,
            TextSpacingTrim::SpaceFirst | TextSpacingTrim::TrimStart
        );
        let at_start = paragraph_start && offset == 0 && maybe_open(character) && {
            // The text of the item: this piece and those after it in its style.
            let text: String = pieces[piece..]
                .iter()
                .take_while(|other| std::sync::Arc::ptr_eq(&other.style, style))
                .map_while(|other| match &other.kind {
                    PieceKind::Text { text, .. } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            let own = style.text_spacing_trim == TextSpacingTrim::TrimStart;
            starts_trimmed(cache, (text, face, style.font_size.to_bits()), own)
        };
        let first = offset == 0 && maybe_open(character);
        let last = offset + character.len_utf8() == text_len(&pieces[piece]) && maybe_close(character);
        let trim = if by_previous || by_next || at_start || (first && (trim_start || by_face)) || last {
            alternate_trim(fonts, style, (face, caps), character)
        } else {
            None
        };
        let applied = trim.filter(|_| by_previous || by_next || at_start);
        if let Some(applied) = applied {
            kerned.push((piece, glyph, applied));
        }
        let edges = &mut pieces[piece].edges;
        if first {
            if by_previous || at_start {
                edges.start_kerned = applied;
            } else if by_face {
                edges.start_kerned = trim;
            }
            if trim_start {
                edges.start = trim;
            }
        }
        if last && applied.is_none() {
            edges.end = trim;
        }
    }
    for (piece, glyph, trim) in kerned {
        apply(&mut pieces[piece], glyph, trim, false);
    }
}

/// The length of a text piece's text.
fn text_len(piece: &Piece) -> usize {
    match &piece.kind {
        PieceKind::Text { text, .. } => text.len(),
        _ => 0,
    }
}

/// Starts a line that wraps (not the first of a paragraph) at the front of `queue`: opening
/// punctuation there loses its blank half with `space-first` and `trim-start`, and takes back
/// the half it lost to the character before with the other values.
pub(super) fn start_line(queue: &mut VecDeque<Piece>) {
    let Some(piece) = queue
        .iter_mut()
        .find(|piece| !matches!(piece.kind, PieceKind::Open(_) | PieceKind::Placeholder(_)))
    else {
        return;
    };
    if !matches!(piece.kind, PieceKind::Text { .. }) {
        return;
    }
    let trims_start = matches!(
        piece.style.text_spacing_trim,
        TextSpacingTrim::SpaceFirst | TextSpacingTrim::TrimStart
    );
    let first = first_glyph(piece);
    match (trims_start, piece.edges.start_kerned, piece.edges.start) {
        (true, None, Some(trim)) => {
            if let Some(first) = first {
                apply(piece, first, trim, false);
                piece.edges.start_kerned = Some(trim);
            }
        }
        (false, Some(trim), _) => {
            if let Some(first) = first {
                apply(piece, first, trim, true);
                piece.edges.start_kerned = None;
            }
        }
        _ => {}
    }
}

/// The index of the glyph of the first character of a text piece.
fn first_glyph(piece: &Piece) -> Option<usize> {
    let PieceKind::Text { glyphs, .. } = &piece.kind else {
        return None;
    };
    glyphs.iter().position(|glyph| glyph.cluster.0 == 0)
}

/// The index of the glyph of the last character of a text piece.
fn last_glyph(piece: &Piece) -> Option<usize> {
    let PieceKind::Text { glyphs, text, .. } = &piece.kind else {
        return None;
    };
    let end = text.len() as u32;
    let start = text.char_indices().last()?.0 as u32;
    glyphs.iter().rposition(|glyph| glyph.cluster == (start, end))
}

/// How much narrower the piece `index` of `queue` (or the last text piece before the ends of
/// inline boxes there) gets where a line ends after it, if its last glyph is closing punctuation
/// that loses its blank half there, with the width of that glyph: it does so only where that
/// glyph alone does not fit otherwise.
pub(super) fn end_trim(queue: &VecDeque<Piece>, index: usize) -> Option<(usize, f32, f32)> {
    let text = last_text(queue, index)?;
    let piece = &queue[text];
    let trim = piece.edges.end?;
    let PieceKind::Text { glyphs, .. } = &piece.kind else {
        return None;
    };
    let glyph = last_glyph(piece)?;
    Some((text, -trim.advance, glyphs[glyph].advance))
}

/// Trims the last glyph of piece `text` of `queue` at the end of its line.
pub(super) fn trim_end(queue: &mut VecDeque<Piece>, text: usize) {
    let piece = &mut queue[text];
    if let (Some(trim), Some(glyph)) = (piece.edges.end.take(), last_glyph(piece)) {
        apply(piece, glyph, trim, false);
    }
}

/// The piece `index`, or the last before it that the ends of inline boxes follow, if it is
/// text.
fn last_text(queue: &VecDeque<Piece>, index: usize) -> Option<usize> {
    let mut index = index;
    while matches!(queue.get(index)?.kind, PieceKind::Close(_)) {
        index = index.checked_sub(1)?;
    }
    matches!(queue[index].kind, PieceKind::Text { .. }).then_some(index)
}

/// The autospace after piece `index` (or the last text piece before the ends of inline boxes
/// there), which a line that ends there drops.
pub(super) fn end_autospace(pieces: &VecDeque<Piece>, index: usize) -> f32 {
    last_text(pieces, index).map_or(0.0, |text| pieces[text].edges.autospace)
}

/// Drops the autospace at the end of a line from its last text piece.
pub(super) fn end_line(pieces: &mut [Piece]) {
    let Some(last) = pieces
        .iter()
        .rposition(|piece| !matches!(piece.kind, PieceKind::Close(_)))
    else {
        return;
    };
    let piece = &mut pieces[last];
    let space = std::mem::take(&mut piece.edges.autospace);
    if space > 0.0
        && let Some(glyph) = last_glyph(piece)
        && let PieceKind::Text { glyphs, .. } = &mut piece.kind
    {
        glyphs[glyph].advance -= space;
        piece.width -= space;
        piece.min_width -= space;
    }
}

/// The classes of UTR #59 (East Asian Spacing).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Spacing {
    Other,
    Wide,
    Narrow,
    Conditional,
}

const WIDE: Spacing = Spacing::Wide;
const CONDITIONAL: Spacing = Spacing::Conditional;

/// The UTR #59 class of a character, as Blink computes it from ICU: ideographs, kana and hangul
/// are wide; letters, marks and digits of other scripts (but fullwidth and halfwidth forms)
/// narrow; most other punctuation conditional.
fn spacing_class(character: char) -> Spacing {
    let code = u32::from(character);
    let found = EAST_ASIAN_SPACING.binary_search_by(|&(start, end, _)| {
        if end < code {
            std::cmp::Ordering::Less
        } else if start > code {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
    if let Ok(index) = found {
        return EAST_ASIAN_SPACING[index].2;
    }
    let group = character.general_category_group();
    let narrow = matches!(group, GeneralCategoryGroup::Letter | GeneralCategoryGroup::Mark)
        || character.general_category() == GeneralCategory::DecimalNumber;
    if narrow && !matches!(code, 0xFF00..=0xFFEF) {
        Spacing::Narrow
    } else {
        Spacing::Other
    }
}

/// Puts the space of `text-autospace: normal` between ideographs and letters or digits of other
/// scripts (UTR #59), after the character before, as Blink's `TextAutoSpace` does: an eighth of
/// the width of an ideograph of the style's first font. Marks are skipped; `chinese` text takes
/// conditional punctuation for narrow characters.
fn insert_autospace(fonts: &FontDb, pieces: &mut [Piece], entries: &[Entry], chinese: bool) {
    let resolve = |class: Spacing| match class {
        Spacing::Conditional if chinese => Spacing::Narrow,
        Spacing::Conditional => Spacing::Other,
        class => class,
    };
    let enabled = |style: &ComputedStyle| {
        style.text_autospace && !(style.is_vertical() && style.text_orientation == TextOrientation::Upright)
    };
    // The character before: its piece, its byte offset there (`None` for the end of a ruby
    // column), the character and its class.
    let mut last: Option<(usize, Option<usize>, char, Spacing)> = None;
    let mut sizes: FxHashMap<usize, f32> = FxHashMap::default();
    let mut size = |piece: usize, pieces: &[Piece]| {
        *sizes
            .entry(piece)
            .or_insert_with(|| ideographic_advance(fonts, &pieces[piece].style) / 8.0)
    };
    // Where the spaces go: after the character at an offset of a piece, or before it (after a
    // ruby column, or before the base of one, `None`).
    let mut spaces: Vec<(usize, Option<usize>, char, bool, f32)> = Vec::new();
    for entry in entries {
        let (piece, offset, character, end) = match *entry {
            Entry::Char {
                piece,
                offset,
                character,
                ..
            } => (piece, Some(offset), character, character),
            Entry::Ruby { piece, first, last } => (piece, None, first, last),
            Entry::Barrier => {
                last = None;
                continue;
            }
        };
        if character.general_category_group() == GeneralCategoryGroup::Mark {
            continue;
        }
        let class = spacing_class(character);
        if let Some((last_piece, last_offset, last_character, last_class)) = last
            && (class == Spacing::Wide || last_class == Spacing::Wide)
        {
            let (class, last_class) = (resolve(class), resolve(last_class));
            let needs = (last_class == Spacing::Wide && class == Spacing::Narrow)
                || (class == Spacing::Wide && last_class == Spacing::Narrow);
            if needs && enabled(&pieces[piece].style) && enabled(&pieces[last_piece].style) {
                // Chrome puts the space before the base of a ruby column and before what follows
                // one, and otherwise after the character before.
                match (offset, last_offset) {
                    (Some(_), Some(_)) => {
                        let size = size(last_piece, pieces);
                        spaces.push((last_piece, last_offset, last_character, false, size));
                    }
                    _ => spaces.push((piece, offset, character, true, size(piece, pieces))),
                }
            }
        }
        let end_class = if offset.is_some() {
            class
        } else {
            spacing_class(end)
        };
        last = Some((piece, offset, end, end_class));
    }
    for (piece, offset, character, before, size) in spaces {
        let piece = &mut pieces[piece];
        let Some(offset) = offset else {
            // Before the base of a ruby column, inside it.
            piece.edges.ruby_lead += size;
            piece.width += size;
            piece.min_width += size;
            continue;
        };
        let end = (offset + character.len_utf8()) as u32;
        let last = end as usize == text_len(piece);
        if let PieceKind::Text { glyphs, .. } = &mut piece.kind
            && let Some(glyph) = glyphs
                .iter_mut()
                .rev()
                .find(|glyph| glyph.cluster.0 as usize <= offset && glyph.cluster.1 >= end)
        {
            glyph.advance += size;
            if before {
                glyph.x_offset += size;
            }
            piece.width += size;
            piece.min_width += size;
            if last && !before {
                piece.edges.autospace += size;
            }
        }
    }
}

/// The space of `text-autospace` that the character before byte `offset` of a text piece has
/// before the one at `offset`, which a line that breaks there drops.
pub(super) fn autospace_at(fonts: &FontDb, piece: &Piece, offset: usize) -> f32 {
    let PieceKind::Text { text, .. } = &piece.kind else {
        return 0.0;
    };
    let style = &piece.style;
    if !style.text_autospace
        || (style.is_vertical() && style.text_orientation == TextOrientation::Upright)
        || !text.is_char_boundary(offset)
    {
        return 0.0;
    }
    let letter = |character: &char| character.general_category_group() != GeneralCategoryGroup::Mark;
    let (Some(before), Some(after)) = (
        text[..offset].chars().rev().find(letter),
        text[offset..].chars().find(letter),
    ) else {
        return 0.0;
    };
    let chinese = style
        .lang
        .as_deref()
        .is_some_and(|lang| lang.get(..2).is_some_and(|code| code.eq_ignore_ascii_case("zh")));
    let resolve = |class: Spacing| match class {
        Spacing::Conditional if chinese => Spacing::Narrow,
        Spacing::Conditional => Spacing::Other,
        class => class,
    };
    let (before, after) = (resolve(spacing_class(before)), resolve(spacing_class(after)));
    if (before == Spacing::Wide && after == Spacing::Narrow)
        || (before == Spacing::Narrow && after == Spacing::Wide)
    {
        ideographic_advance(fonts, style) / 8.0
    } else {
        0.0
    }
}

/// The advance of the water ideograph in the style's first font, or the font size where that
/// lacks it (Blink's `IdeographicInlineSize`).
fn ideographic_advance(fonts: &FontDb, style: &ComputedStyle) -> f32 {
    let size = used_font_size(fonts, style);
    face_for(fonts, style)
        .filter(|&face| fonts.face(face).has_glyph('\u{6C34}'))
        .map_or(size, |face| {
            fonts.shape(face, "\u{6C34}").advance * fonts.face(face).scale(size)
        })
}

/// The ranges of wide and conditional characters of UTR #59, computed from `Scripts.txt`,
/// `ScriptExtensions.txt`, `EastAsianWidth.txt` and the general categories of Unicode 16 as
/// Blink's `character_property_data_generator.cc` does.
const EAST_ASIAN_SPACING: &[(u32, u32, Spacing)] = &[
    (0x0021, 0x0021, CONDITIONAL),
    (0x0023, 0x0023, CONDITIONAL),
    (0x0025, 0x0026, CONDITIONAL),
    (0x002C, 0x002C, CONDITIONAL),
    (0x002E, 0x002E, CONDITIONAL),
    (0x003A, 0x003B, CONDITIONAL),
    (0x003F, 0x0040, CONDITIONAL),
    (0x005C, 0x005C, CONDITIONAL),
    (0x00A1, 0x00A1, CONDITIONAL),
    (0x00A7, 0x00A7, CONDITIONAL),
    (0x00B6, 0x00B6, CONDITIONAL),
    (0x00BF, 0x00BF, CONDITIONAL),
    (0x02C7, 0x02C7, WIDE),
    (0x02C9, 0x02CB, WIDE),
    (0x02D9, 0x02D9, WIDE),
    (0x02EA, 0x02EB, WIDE),
    (0x0305, 0x0305, WIDE),
    (0x0323, 0x0323, WIDE),
    (0x037E, 0x037E, CONDITIONAL),
    (0x0387, 0x0387, CONDITIONAL),
    (0x055A, 0x055F, CONDITIONAL),
    (0x0589, 0x0589, CONDITIONAL),
    (0x05C0, 0x05C0, CONDITIONAL),
    (0x05C3, 0x05C3, CONDITIONAL),
    (0x05C6, 0x05C6, CONDITIONAL),
    (0x05F3, 0x05F4, CONDITIONAL),
    (0x0609, 0x060A, CONDITIONAL),
    (0x060C, 0x060D, CONDITIONAL),
    (0x061B, 0x061B, CONDITIONAL),
    (0x061D, 0x061F, CONDITIONAL),
    (0x066A, 0x066D, CONDITIONAL),
    (0x06D4, 0x06D4, CONDITIONAL),
    (0x0700, 0x070D, CONDITIONAL),
    (0x07F7, 0x07F9, CONDITIONAL),
    (0x0830, 0x083E, CONDITIONAL),
    (0x085E, 0x085E, CONDITIONAL),
    (0x0964, 0x0965, CONDITIONAL),
    (0x0970, 0x0970, CONDITIONAL),
    (0x09FD, 0x09FD, CONDITIONAL),
    (0x0A76, 0x0A76, CONDITIONAL),
    (0x0AF0, 0x0AF0, CONDITIONAL),
    (0x0C77, 0x0C77, CONDITIONAL),
    (0x0C84, 0x0C84, CONDITIONAL),
    (0x0DF4, 0x0DF4, CONDITIONAL),
    (0x0E4F, 0x0E4F, CONDITIONAL),
    (0x0E5A, 0x0E5B, CONDITIONAL),
    (0x0F04, 0x0F12, CONDITIONAL),
    (0x0F14, 0x0F14, CONDITIONAL),
    (0x0F85, 0x0F85, CONDITIONAL),
    (0x0FD0, 0x0FD4, CONDITIONAL),
    (0x0FD9, 0x0FDA, CONDITIONAL),
    (0x104A, 0x104F, CONDITIONAL),
    (0x10FB, 0x10FB, CONDITIONAL),
    (0x1100, 0x11FF, WIDE),
    (0x1360, 0x1368, CONDITIONAL),
    (0x166E, 0x166E, CONDITIONAL),
    (0x16EB, 0x16ED, CONDITIONAL),
    (0x1735, 0x1736, CONDITIONAL),
    (0x17D4, 0x17D6, CONDITIONAL),
    (0x17D8, 0x17DA, CONDITIONAL),
    (0x1800, 0x1805, CONDITIONAL),
    (0x1807, 0x180A, CONDITIONAL),
    (0x1944, 0x1945, CONDITIONAL),
    (0x1A1E, 0x1A1F, CONDITIONAL),
    (0x1AA0, 0x1AA6, CONDITIONAL),
    (0x1AA8, 0x1AAD, CONDITIONAL),
    (0x1B4E, 0x1B4F, CONDITIONAL),
    (0x1B5A, 0x1B60, CONDITIONAL),
    (0x1B7D, 0x1B7F, CONDITIONAL),
    (0x1BFC, 0x1BFF, CONDITIONAL),
    (0x1C3B, 0x1C3F, CONDITIONAL),
    (0x1C7E, 0x1C7F, CONDITIONAL),
    (0x1CC0, 0x1CC7, CONDITIONAL),
    (0x1CD3, 0x1CD3, CONDITIONAL),
    (0x2016, 0x2017, CONDITIONAL),
    (0x2022, 0x2025, CONDITIONAL),
    (0x2027, 0x2027, CONDITIONAL),
    (0x2030, 0x2038, CONDITIONAL),
    (0x203B, 0x203E, CONDITIONAL),
    (0x2041, 0x2043, CONDITIONAL),
    (0x2047, 0x2051, CONDITIONAL),
    (0x2053, 0x2053, CONDITIONAL),
    (0x2055, 0x205E, CONDITIONAL),
    (0x2CF9, 0x2CFC, CONDITIONAL),
    (0x2CFE, 0x2CFF, CONDITIONAL),
    (0x2D70, 0x2D70, CONDITIONAL),
    (0x2E00, 0x2E01, CONDITIONAL),
    (0x2E06, 0x2E08, CONDITIONAL),
    (0x2E0B, 0x2E0B, CONDITIONAL),
    (0x2E0E, 0x2E16, CONDITIONAL),
    (0x2E18, 0x2E19, CONDITIONAL),
    (0x2E1B, 0x2E1B, CONDITIONAL),
    (0x2E1E, 0x2E1F, CONDITIONAL),
    (0x2E2A, 0x2E2E, CONDITIONAL),
    (0x2E30, 0x2E39, CONDITIONAL),
    (0x2E3C, 0x2E3F, CONDITIONAL),
    (0x2E41, 0x2E41, CONDITIONAL),
    (0x2E43, 0x2E4F, CONDITIONAL),
    (0x2E52, 0x2E54, CONDITIONAL),
    (0x3005, 0x3007, WIDE),
    (0x3013, 0x3013, WIDE),
    (0x3021, 0x302F, WIDE),
    (0x3031, 0x3035, WIDE),
    (0x3038, 0x303C, WIDE),
    (0x3041, 0x3096, WIDE),
    (0x3099, 0x309F, WIDE),
    (0x30A1, 0x30FA, WIDE),
    (0x30FC, 0x30FF, WIDE),
    (0x3105, 0x312F, WIDE),
    (0x3131, 0x318E, WIDE),
    (0x31A0, 0x31BF, WIDE),
    (0x31F0, 0x31FF, WIDE),
    (0x3400, 0x4DBF, WIDE),
    (0x4E00, 0xA48C, WIDE),
    (0xA4FE, 0xA4FF, CONDITIONAL),
    (0xA60D, 0xA60F, CONDITIONAL),
    (0xA673, 0xA673, CONDITIONAL),
    (0xA67E, 0xA67E, CONDITIONAL),
    (0xA6F2, 0xA6F7, CONDITIONAL),
    (0xA874, 0xA877, CONDITIONAL),
    (0xA8CE, 0xA8CF, CONDITIONAL),
    (0xA8F8, 0xA8FA, CONDITIONAL),
    (0xA8FC, 0xA8FC, CONDITIONAL),
    (0xA92E, 0xA92F, CONDITIONAL),
    (0xA95F, 0xA95F, CONDITIONAL),
    (0xA960, 0xA97C, WIDE),
    (0xA9C1, 0xA9CD, CONDITIONAL),
    (0xA9DE, 0xA9DF, CONDITIONAL),
    (0xAA5C, 0xAA5F, CONDITIONAL),
    (0xAADE, 0xAADF, CONDITIONAL),
    (0xAAF0, 0xAAF1, CONDITIONAL),
    (0xABEB, 0xABEB, CONDITIONAL),
    (0xAC00, 0xD7A3, WIDE),
    (0xD7B0, 0xD7C6, WIDE),
    (0xD7CB, 0xD7FB, WIDE),
    (0xF900, 0xFA6D, WIDE),
    (0xFA70, 0xFAD9, WIDE),
    (0x10100, 0x10102, CONDITIONAL),
    (0x1039F, 0x1039F, CONDITIONAL),
    (0x103D0, 0x103D0, CONDITIONAL),
    (0x1056F, 0x1056F, CONDITIONAL),
    (0x10857, 0x10857, CONDITIONAL),
    (0x1091F, 0x1091F, CONDITIONAL),
    (0x1093F, 0x1093F, CONDITIONAL),
    (0x10A50, 0x10A58, CONDITIONAL),
    (0x10A7F, 0x10A7F, CONDITIONAL),
    (0x10AF0, 0x10AF6, CONDITIONAL),
    (0x10B39, 0x10B3F, CONDITIONAL),
    (0x10B99, 0x10B9C, CONDITIONAL),
    (0x10F55, 0x10F59, CONDITIONAL),
    (0x10F86, 0x10F89, CONDITIONAL),
    (0x11047, 0x1104D, CONDITIONAL),
    (0x110BB, 0x110BC, CONDITIONAL),
    (0x110BE, 0x110C1, CONDITIONAL),
    (0x11140, 0x11143, CONDITIONAL),
    (0x11174, 0x11175, CONDITIONAL),
    (0x111C5, 0x111C8, CONDITIONAL),
    (0x111CD, 0x111CD, CONDITIONAL),
    (0x111DB, 0x111DB, CONDITIONAL),
    (0x111DD, 0x111DF, CONDITIONAL),
    (0x11238, 0x1123D, CONDITIONAL),
    (0x112A9, 0x112A9, CONDITIONAL),
    (0x113D4, 0x113D5, CONDITIONAL),
    (0x113D7, 0x113D8, CONDITIONAL),
    (0x1144B, 0x1144F, CONDITIONAL),
    (0x1145A, 0x1145B, CONDITIONAL),
    (0x1145D, 0x1145D, CONDITIONAL),
    (0x114C6, 0x114C6, CONDITIONAL),
    (0x115C1, 0x115D7, CONDITIONAL),
    (0x11641, 0x11643, CONDITIONAL),
    (0x11660, 0x1166C, CONDITIONAL),
    (0x116B9, 0x116B9, CONDITIONAL),
    (0x1173C, 0x1173E, CONDITIONAL),
    (0x1183B, 0x1183B, CONDITIONAL),
    (0x11944, 0x11946, CONDITIONAL),
    (0x119E2, 0x119E2, CONDITIONAL),
    (0x11A3F, 0x11A46, CONDITIONAL),
    (0x11A9A, 0x11A9C, CONDITIONAL),
    (0x11A9E, 0x11AA2, CONDITIONAL),
    (0x11B00, 0x11B09, CONDITIONAL),
    (0x11BE1, 0x11BE1, CONDITIONAL),
    (0x11C41, 0x11C45, CONDITIONAL),
    (0x11C70, 0x11C71, CONDITIONAL),
    (0x11EF7, 0x11EF8, CONDITIONAL),
    (0x11F43, 0x11F4F, CONDITIONAL),
    (0x11FFF, 0x11FFF, CONDITIONAL),
    (0x12470, 0x12474, CONDITIONAL),
    (0x12FF1, 0x12FF2, CONDITIONAL),
    (0x16A6E, 0x16A6F, CONDITIONAL),
    (0x16AF5, 0x16AF5, CONDITIONAL),
    (0x16B37, 0x16B3B, CONDITIONAL),
    (0x16B44, 0x16B44, CONDITIONAL),
    (0x16D6D, 0x16D6F, CONDITIONAL),
    (0x16E97, 0x16E9A, CONDITIONAL),
    (0x16FE0, 0x16FE1, WIDE),
    (0x16FE3, 0x16FE4, WIDE),
    (0x16FF0, 0x16FF1, WIDE),
    (0x17000, 0x187F7, WIDE),
    (0x18800, 0x18CD5, WIDE),
    (0x18CFF, 0x18D08, WIDE),
    (0x1AFF0, 0x1AFF3, WIDE),
    (0x1AFF5, 0x1AFFB, WIDE),
    (0x1AFFD, 0x1AFFE, WIDE),
    (0x1B000, 0x1B122, WIDE),
    (0x1B132, 0x1B132, WIDE),
    (0x1B150, 0x1B152, WIDE),
    (0x1B155, 0x1B155, WIDE),
    (0x1B164, 0x1B167, WIDE),
    (0x1B170, 0x1B2FB, WIDE),
    (0x1BC9F, 0x1BC9F, CONDITIONAL),
    (0x1DA87, 0x1DA8B, CONDITIONAL),
    (0x1E5FF, 0x1E5FF, CONDITIONAL),
    (0x1E95E, 0x1E95F, CONDITIONAL),
    (0x20000, 0x2A6DF, WIDE),
    (0x2A700, 0x2B739, WIDE),
    (0x2B740, 0x2B81D, WIDE),
    (0x2B820, 0x2CEA1, WIDE),
    (0x2CEB0, 0x2EBE0, WIDE),
    (0x2EBF0, 0x2EE5D, WIDE),
    (0x2F800, 0x2FA1D, WIDE),
    (0x30000, 0x3134A, WIDE),
    (0x31350, 0x323AF, WIDE),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paragraph_starts_trim_as_their_text_was_shaped_first() {
        let cache = FontCache::default();
        let mut fonts = FontDb::new();
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fonts/Inter-Regular.ttf");
        let face = fonts.add_font_file(&path).expect("test font")[0];
        let key = |text: &str, size: f32| (text.to_owned(), face, size.to_bits());
        // `normal` first: `trim-start` after keeps the blank half, as in Chrome.
        assert!(!starts_trimmed(&cache, key("「三番目」", 20.0), false));
        assert!(!starts_trimmed(&cache, key("「三番目」", 20.0), true));
        // Another size is other text to the cache.
        assert!(starts_trimmed(&cache, key("「三番目」", 21.0), true));
        // `trim-start` first: `normal` after loses the blank half too.
        assert!(starts_trimmed(&cache, key("「一番目」", 20.0), true));
        assert!(starts_trimmed(&cache, key("「一番目」", 20.0), false));
    }

    #[test]
    fn classifies_punctuation() {
        assert_eq!(char_type('「'), CharType::Open);
        assert_eq!(char_type('」'), CharType::Close);
        assert_eq!(char_type('（'), CharType::Open);
        assert_eq!(char_type('）'), CharType::Close);
        assert_eq!(char_type('('), CharType::OpenNarrow);
        assert_eq!(char_type(']'), CharType::CloseNarrow);
        assert_eq!(char_type('。'), CharType::Dot);
        assert_eq!(char_type('・'), CharType::Middle);
        assert_eq!(char_type('“'), CharType::OpenQuote);
        assert_eq!(char_type('a'), CharType::Other);
        assert!(should_kern(CharType::Open, CharType::Close));
        assert!(!should_kern(CharType::Open, CharType::Other));
        assert!(should_kern_last(CharType::Close, CharType::Close));
        assert!(!should_kern_last(CharType::Open, CharType::Close));
    }

    #[test]
    fn classifies_east_asian_spacing() {
        assert_eq!(spacing_class('漢'), Spacing::Wide);
        assert_eq!(spacing_class('か'), Spacing::Wide);
        assert_eq!(spacing_class('한'), Spacing::Wide);
        assert_eq!(spacing_class('a'), Spacing::Narrow);
        assert_eq!(spacing_class('7'), Spacing::Narrow);
        assert_eq!(spacing_class('é'), Spacing::Narrow);
        assert_eq!(spacing_class('!'), Spacing::Conditional);
        assert_eq!(spacing_class('。'), Spacing::Other);
        assert_eq!(spacing_class('Ａ'), Spacing::Other);
        assert_eq!(spacing_class(' '), Spacing::Other);
    }
}

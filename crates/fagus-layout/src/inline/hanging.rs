//! Hanging punctuation (`hanging-punctuation`, CSS Text 3, §8.2): a punctuation mark at the
//! start or end of a line that is not measured for fit, alignment or justification, so that it
//! stands outside the line box (or in the indent).
//!
//! At most one character hangs at each edge of a line: an opening bracket, quote or ideographic
//! space at the start of the first formatted line (`first`), a closing bracket or quote at the
//! end of the last (`last`), and a stop or comma at the end of any line (`force-end`, and
//! `allow-end` where it does not fit otherwise). A character hangs only where it stands at the
//! edge: not behind the start or end border or padding of an inline box (§8.2), and at the
//! level of the line's paragraph, so that bidi reordering keeps it at the edge of the line.

use fagus_css::hanging::HangingEnd;
use unicode_properties::{GeneralCategory, UnicodeGeneralCategory};

use super::{InlineBox, Piece, PieceKind, hangs};

/// Whether `character` hangs at the start of the first formatted line with `first`: the Unicode
/// categories Ps, Pi and Pf, the ASCII quotation marks and the ideographic space.
pub(super) fn opens(character: char) -> bool {
    matches!(character, '\'' | '"' | '\u{3000}')
        || matches!(
            character.general_category(),
            GeneralCategory::OpenPunctuation
                | GeneralCategory::InitialPunctuation
                | GeneralCategory::FinalPunctuation
        )
}

/// Whether `character` hangs at the end of the last formatted line with `last`: the Unicode
/// categories Pe, Pi and Pf and the ASCII quotation marks.
pub(super) fn closes(character: char) -> bool {
    matches!(character, '\'' | '"')
        || matches!(
            character.general_category(),
            GeneralCategory::ClosePunctuation
                | GeneralCategory::InitialPunctuation
                | GeneralCategory::FinalPunctuation
        )
}

/// Whether `character` is one of the stops and commas that hang at the end of a line with
/// `force-end` and `allow-end` (CSS Text 3, §8.2.1).
pub(super) fn stops(character: char) -> bool {
    matches!(
        character,
        ',' | '.'
            | '\u{060C}'
            | '\u{06D4}'
            | '\u{3001}'
            | '\u{3002}'
            | '\u{FF0C}'
            | '\u{FF0E}'
            | '\u{FE50}'
            | '\u{FE51}'
            | '\u{FE52}'
            | '\u{FF61}'
            | '\u{FF64}'
    )
}

/// Whether `character` may hang at an edge of a line with some value of `hanging-punctuation`.
pub fn may_hang(character: char) -> bool {
    opens(character) || closes(character) || stops(character)
}

/// The advance of the glyphs of the character at bytes `start..end` of a text piece: `None`
/// where it has none of its own, set in one cluster with a character beside it (a ligature, a
/// mark after it).
fn advance_of(piece: &Piece, start: usize, end: usize) -> Option<f32> {
    let PieceKind::Text { glyphs, .. } = &piece.kind else {
        return None;
    };
    let mut advance = None;
    for glyph in glyphs {
        let (from, to) = (glyph.cluster.0 as usize, glyph.cluster.1 as usize);
        if from == start {
            if to != end {
                return None;
            }
            *advance.get_or_insert(0.0) += glyph.advance;
        } else if from < start && to > start {
            return None;
        }
    }
    advance
}

/// Whether a piece stands for nothing at an edge of a line: what takes no room there (the start
/// or end of an inline box without border and padding on that side, an out-of-flow box, a float,
/// an outside list marker) or text of nothing but spaces that hang.
fn passes(piece: &Piece, boxes: &[InlineBox]) -> Option<bool> {
    Some(match &piece.kind {
        PieceKind::Open(index) => boxes.get(*index)?.frame.0 == 0.0,
        PieceKind::Close(index) => boxes.get(*index)?.frame.1 == 0.0,
        PieceKind::Placeholder(_) | PieceKind::Float { .. } | PieceKind::Marker { inside: false, .. } => true,
        PieceKind::Measured => piece.style.is_floating(),
        PieceKind::Text { text, .. } => text.is_empty(),
        _ => false,
    })
}

/// How wide the punctuation is that hangs at the start of the first formatted line made of
/// `pieces` (in logical order from the start of the line): 0 where none does.
pub(super) fn start<'p>(pieces: impl IntoIterator<Item = &'p Piece>, boxes: &[InlineBox]) -> f32 {
    for piece in pieces {
        if let PieceKind::Text { text, .. } = &piece.kind
            && let Some(character) = text.chars().next()
        {
            if !piece.style.hanging_punctuation.first
                || !opens(character)
                || piece.bidi.level != piece.bidi.paragraph
            {
                return 0.0;
            }
            return advance_of(piece, 0, character.len_utf8()).unwrap_or(0.0);
        }
        if passes(piece, boxes) != Some(true) {
            return 0.0;
        }
    }
    0.0
}

/// Punctuation at the end of a line that may hang.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct End {
    /// The advance of the punctuation mark.
    pub(super) width: f32,
    /// Why it hangs.
    pub(super) kind: EndKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EndKind {
    /// A stop or comma, with `force-end` (it hangs) or `allow-end` (as far as it does not fit).
    Stop(HangingEnd),
    /// A closing bracket or quote with `last`: it hangs only at the end of the last formatted
    /// line.
    Close,
}

impl End {
    /// How far the punctuation hangs on a line whose content (with the punctuation) overflows
    /// the room for it by `overflow`, at the end of the last formatted line or not.
    pub(super) fn hanging(self, overflow: f32, last_line: bool) -> f32 {
        match self.kind {
            EndKind::Stop(HangingEnd::Allow) => overflow.clamp(0.0, self.width),
            EndKind::Close if !last_line => 0.0,
            _ => self.width,
        }
    }

    /// How much of the punctuation counts for neither the min-content nor (`max` set) the
    /// max-content width (CSS Text 3, §8.2): punctuation that may hang only where it does not
    /// fit counts for the max-content width.
    pub(super) fn unmeasured(self, max: bool, last_line: bool) -> f32 {
        match self.kind {
            EndKind::Stop(HangingEnd::Allow) if max => 0.0,
            EndKind::Close if !last_line => 0.0,
            _ => self.width,
        }
    }
}

/// The punctuation that may hang at the end of a line that ends with `pieces` (in reverse
/// logical order from the end of the line), past the spaces that hang there.
pub(super) fn end<'p>(pieces: impl IntoIterator<Item = &'p Piece>, boxes: &[InlineBox]) -> Option<End> {
    for piece in pieces {
        if let PieceKind::Text { text, .. } = &piece.kind
            && !text.is_empty()
        {
            // Spaces after the mark have its style, but outside an inline box that sets the
            // property: there the mark does not hang (and lines without it look no further).
            let value = piece.style.hanging_punctuation;
            if value.is_none() {
                return None;
            }
            let content = text.trim_end_matches(hangs);
            // Spaces that take room on the line (`break-spaces`) stand after the punctuation.
            if content.len() < text.len() && piece.trailing_space <= 0.0 {
                return None;
            }
            let Some(character) = content.chars().next_back() else {
                continue;
            };
            let kind = if stops(character) && value.end != HangingEnd::None {
                EndKind::Stop(value.end)
            } else if closes(character) && value.last {
                EndKind::Close
            } else {
                return None;
            };
            if piece.bidi.level != piece.bidi.paragraph {
                return None;
            }
            let width = advance_of(piece, content.len() - character.len_utf8(), content.len())?;
            return Some(End { width, kind });
        }
        if passes(piece, boxes) != Some(true) {
            return None;
        }
    }
    None
}

/// Whether `pieces` hold nothing that ends the content of a formatted line after the
/// punctuation before them: only what [`passes`] at an edge, and spaces.
pub(super) fn nothing_after<'p>(pieces: impl IntoIterator<Item = &'p Piece>) -> bool {
    pieces.into_iter().all(|piece| match &piece.kind {
        PieceKind::Text { text, .. } => text.chars().all(hangs),
        PieceKind::Open(_)
        | PieceKind::Close(_)
        | PieceKind::Placeholder(_)
        | PieceKind::Float { .. }
        | PieceKind::Marker { inside: false, .. } => true,
        PieceKind::Measured => piece.style.is_floating(),
        _ => false,
    })
}

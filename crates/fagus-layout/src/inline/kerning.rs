//! Kerning across the start of a wrapped line, as Chrome shapes the text there again.
//!
//! Blink shapes the text of a paragraph's items in runs of the same font, so that pairs of
//! characters kern across the edges of items and words. Where a line starts at a glyph that
//! shaping kerned with the character before it, Blink cannot cut the shaped run there and shapes
//! the text again from the start of the line up to the next place where shaping allows to break,
//! within the item (`ShapingLineBreaker::ShapeLine`): the first glyph loses its kerning with the
//! character on the line before, and where no such place comes before the end of the item, its
//! last glyph loses its share of the kerning with the item after it too.

use std::collections::VecDeque;

use fagus_text::FontDb;

use super::{Caps, Piece, PieceKind, same_item};

/// Whether the line that starts at the front of `queue` starts at a glyph that shaping kerned
/// with the character before it, so that [`start_line`] looks at the rest of its item.
pub(super) fn starts_kerned(queue: &VecDeque<Piece>) -> bool {
    queue
        .iter()
        .find(|piece| !matches!(piece.kind, PieceKind::Open(_) | PieceKind::Placeholder(_)))
        .is_some_and(|piece| matches!(piece.kind, PieceKind::Text { .. }) && piece.edges.kerned_before)
}

/// Starts a wrapped line at the front of `queue` (see the module's documentation).
pub(super) fn start_line(fonts: &FontDb, queue: &mut VecDeque<Piece>) {
    let Some(first) = queue
        .iter()
        .position(|piece| !matches!(piece.kind, PieceKind::Open(_) | PieceKind::Placeholder(_)))
    else {
        return;
    };
    if !matches!(queue[first].kind, PieceKind::Text { .. }) || !queue[first].edges.kerned_before {
        return;
    }
    let piece = &mut queue[first];
    piece.edges.kerned_before = false;
    let share = std::mem::take(&mut piece.edges.kern_before);
    if share != 0.0
        && let PieceKind::Text { glyphs, .. } = &mut piece.kind
        && let Some(glyph) = glyphs.first_mut()
    {
        glyph.advance -= share;
        glyph.x_offset -= share;
        piece.width -= share;
    }
    // The pieces of the item from the start of the line on.
    let mut last = first;
    while last + 1 < queue.len() && same_item(&queue[last], &queue[last + 1]) {
        last += 1;
    }
    if queue[last].edges.kern_after == 0.0 || breaks_safely(fonts, queue.range(first..=last)) {
        return;
    }
    let piece = &mut queue[last];
    let share = std::mem::take(&mut piece.edges.kern_after);
    if let PieceKind::Text { glyphs, text, .. } = &mut piece.kind
        && let Some(glyph) = glyphs.last_mut()
    {
        glyph.advance -= share;
        piece.width -= share;
        if text.ends_with(' ') {
            piece.trailing_space -= share;
        }
    }
}

/// Whether shaping the text of `pieces` (of one item) as a whole allows to break it somewhere
/// after its start without shaping it again; also where the pieces are not shaped alike, which
/// Blink shapes in runs of their own.
fn breaks_safely<'a>(fonts: &FontDb, mut pieces: impl Iterator<Item = &'a Piece>) -> bool {
    let Some(first) = pieces.next() else {
        return true;
    };
    let PieceKind::Text { face, text, caps, .. } = &first.kind else {
        return true;
    };
    if *caps != Caps::NONE || !fonts.face(*face).axes.is_empty() {
        return true;
    }
    let mut whole = text.clone();
    for piece in pieces {
        match &piece.kind {
            PieceKind::Text {
                face: other,
                text,
                caps: other_caps,
                ..
            } if other == face && *other_caps == Caps::NONE => whole.push_str(text),
            _ => return true,
        }
    }
    let style = &first.style;
    let mut features = super::variant_features(fonts, *face, style);
    features.extend_from_slice(&style.font_feature_settings);
    fonts
        .shape_with(*face, &whole, &features)
        .glyphs
        .iter()
        .skip(1)
        .any(|glyph| glyph.safe_to_break)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use fagus_style::ComputedStyle;

    use super::*;
    use crate::bidi::TextLevels;
    use crate::breaks::own_breaks;
    use crate::inline::shape_text;

    /// A database with Inter, and the pieces of `texts`, each in a style of its own (an item of
    /// its own).
    fn pieces(texts: &[&str]) -> (FontDb, VecDeque<Piece>) {
        let mut fonts = FontDb::new();
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fonts/Inter-Regular.ttf");
        fonts.add_font_file(&path).expect("test font");
        let mut pieces = Vec::new();
        for text in texts {
            let style = Arc::new(ComputedStyle {
                font_family: vec!["Inter".to_owned()].into(),
                font_size: 20.0,
                ..ComputedStyle::default()
            });
            shape_text(
                &fonts,
                text,
                &style,
                (TextLevels::LTR, own_breaks(text)),
                &mut pieces,
            );
        }
        (fonts, pieces.into())
    }

    fn glyph(piece: &Piece, last: bool) -> (f32, f32) {
        let PieceKind::Text { glyphs, .. } = &piece.kind else {
            panic!("text");
        };
        let glyph = if last { glyphs.last() } else { glyphs.first() }.expect("glyph");
        (glyph.advance, glyph.x_offset)
    }

    #[test]
    fn drops_the_kerning_with_the_line_before_and_after_an_item_shaped_again() {
        // `VAT` kerns throughout: shaped again from the line's start, it reaches the item's
        // end and loses the share of the kerning its `T` has with the item after it.
        let (fonts, mut queue) = pieces(&["VAT", ", more"]);
        queue[0].edges.kerned_before = true;
        queue[0].edges.kern_before = -0.5;
        queue[0].edges.kern_after = -0.25;
        let (first, last, width) = (glyph(&queue[0], false), glyph(&queue[0], true), queue[0].width);
        start_line(&fonts, &mut queue);
        assert_eq!(glyph(&queue[0], false), (first.0 + 0.5, first.1 + 0.5));
        assert_eq!(glyph(&queue[0], true).0, last.0 + 0.25);
        assert!((queue[0].width - (width + 0.75)).abs() < 1e-5);
        // The next line starts with it as it is now.
        assert!(!queue[0].edges.kerned_before);
    }

    #[test]
    fn keeps_the_kerning_after_where_shaping_allows_a_break_before() {
        // `Vat` may break before its `t`: shaped again up to there, its end stays.
        let (fonts, mut queue) = pieces(&["Vat", ", more"]);
        queue[0].edges.kerned_before = true;
        queue[0].edges.kern_after = -0.25;
        let last = glyph(&queue[0], true);
        start_line(&fonts, &mut queue);
        assert_eq!(glyph(&queue[0], true), last);
        // Nor does anything change at the start of a line whose first glyph is not kerned.
        let (fonts, mut queue) = pieces(&["VAT", ", more"]);
        queue[0].edges.kern_after = -0.25;
        let last = glyph(&queue[0], true);
        start_line(&fonts, &mut queue);
        assert_eq!(glyph(&queue[0], true), last);
    }
}

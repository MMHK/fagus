//! `text-fit` (CSS Text 5, §6.1) as Chrome 157 applies it, by its default method of scaling the
//! text as it is painted: the text of a line grows or shrinks by a factor so that the line fills
//! its width, and the line grows or shrinks with it (its strut, the text's line heights unless
//! they are fixed lengths); atomic inlines and the margins, borders and padding of inline boxes
//! keep their size. `per-line` fits each line but the last by its own factor, `per-line-all`
//! each line, and `consistent` all lines by the same factor: the one that fits the line that
//! needs the least, measured on the lines laid out once without it. A line that misses its width
//! by less than two pixels is left as it is. Paragraphs with floats, initial letters or ruby are
//! not fitted.
//!
//! Ported from Chromium's `text_fit_utils.cc` (`LineFitter::MeasureScale`, `ScaleLine`,
//! `MeasurePerBlockScale`, `RestrictScale`; BSD license, see `CHROMIUM-LICENSE`).

use std::sync::Arc;

use fagus_style::ComputedStyle;
use fagus_style::values::{TextFit, TextFitKind, TextFitTarget};

use super::{InlineBox, InlineItem, Layouter, Piece, PieceKind};

/// How far a line may miss its width and still be left as it is: two pixels (Blink's
/// `epsilon_`, twice the device pixel ratio, which is 1 in print).
const EPSILON: f32 = 2.0;

/// How the lines of a paragraph are fitted as they are laid out.
#[derive(Clone, Debug)]
pub(super) enum Fitting {
    /// Not at all.
    Off,
    /// Each line by its own factor.
    PerLine(TextFit),
    /// Every line by the same factor, which the lines are measured for first.
    Consistent(TextFit),
    /// Not at all, but each line is measured: its width with the text indent, the width of its
    /// text and the room for it.
    Measure(Vec<Measured>),
    /// Every line by the same factor.
    Fixed(f32),
}

/// A line measured for `consistent` fitting.
#[derive(Clone, Copy, Debug)]
pub(super) struct Measured {
    /// The width of the line's content, without the spaces that hang at its end.
    width: f32,
    /// The width of its text.
    text: f32,
    /// The room for it.
    available: f32,
}

impl Fitting {
    /// How the lines of the paragraph of `container` with `items` are fitted: not where it has
    /// floats, initial letters or ruby (Blink's `ShouldApplyTextFit`). A `consistent` fit is
    /// measured first ([`Fitting::measuring`], [`Fitting::measured`]).
    pub(super) fn of(layouter: &Layouter<'_>, container: &ComputedStyle, items: &[InlineItem]) -> Self {
        let fit = container.text_fit;
        if fit.kind == TextFitKind::None {
            return Self::Off;
        }
        let excluded = items.iter().any(|item| match item {
            InlineItem::Float(_) => true,
            InlineItem::Atomic(id) => layouter.tree.ruby_columns.contains_key(id),
            _ => false,
        });
        if excluded {
            Self::Off
        } else if fit.target == TextFitTarget::Consistent {
            Self::Consistent(fit)
        } else {
            Self::PerLine(fit)
        }
    }

    /// The fitting that measures the lines.
    pub(super) fn measuring() -> Self {
        Self::Measure(Vec::new())
    }

    /// Whether this measures the lines.
    pub(super) fn is_measuring(&self) -> bool {
        matches!(self, Self::Measure(_))
    }

    /// The `consistent` fitting after `measure` measured the lines (Blink's
    /// `MeasurePerBlockScale`): by the smallest of the factors that fit the lines that miss
    /// their width, kept within the limit, where it grows text that `text-fit` grows or
    /// shrinks text that it shrinks.
    pub(super) fn measured(&self, measure: &Self) -> Self {
        let (Self::Consistent(fit), Self::Measure(lines)) = (self, measure) else {
            return self.clone();
        };
        let mut minimum = f32::INFINITY;
        for line in lines {
            let remaining = line.available - line.width;
            if remaining.abs() < EPSILON || line.text <= 0.0 || remaining + line.text <= 0.0 {
                continue;
            }
            minimum = minimum.min((remaining + line.text) / line.text);
        }
        if !minimum.is_finite() {
            return Self::Off;
        }
        let scale = restrict(minimum, fit.kind == TextFitKind::Grow, fit.limit);
        let wanted = match fit.kind {
            TextFitKind::Grow => scale > 1.0,
            TextFitKind::Shrink => scale < 1.0,
            TextFitKind::None => false,
        };
        if wanted { Self::Fixed(scale) } else { Self::Off }
    }

    /// The factor by which the text of a line of `pieces`, after a text indent of `indent`, in
    /// a room of `available`, grows or shrinks: 1 where it does not. `last` tells whether it is
    /// the last line of the paragraph.
    pub(super) fn line_scale(&mut self, pieces: &[Piece], indent: f32, available: f32, last: bool) -> f32 {
        match self {
            Self::Off | Self::Consistent(_) => 1.0,
            Self::Fixed(scale) => *scale,
            Self::Measure(lines) => {
                let (width, text, _) = widths(pieces);
                lines.push(Measured {
                    width,
                    text,
                    available,
                });
                1.0
            }
            Self::PerLine(fit) => {
                let (width, text, fixed) = widths(pieces);
                let scale = line_scale(*fit, (width + indent, text, fixed), available, last);
                if scale.is_finite() { scale } else { 1.0 }
            }
        }
    }
}

/// The width of a line's content (without the spaces that hang at its end), of its text, and
/// of the rest: atomic inlines, the margins, borders and padding of inline boxes, and the
/// letter and word spacing of fixed lengths, which do not scale (Blink's `HasFixedSpacing`).
fn widths(pieces: &[Piece]) -> (f32, f32, f32) {
    let hanging = pieces.last().map_or(0.0, |piece| piece.trailing_space);
    let (mut text, mut fixed) = (0.0, 0.0);
    for piece in pieces {
        match piece.kind {
            PieceKind::Text { .. } => {
                let spacing: f32 = fixed_spacing(piece).iter().sum();
                text += piece.width - spacing;
                fixed += spacing;
            }
            PieceKind::Float { .. } | PieceKind::Placeholder(_) | PieceKind::Measured => {}
            _ => fixed += piece.width,
        }
    }
    if pieces
        .last()
        .is_some_and(|piece| matches!(piece.kind, PieceKind::Text { .. }))
    {
        text -= hanging;
    } else {
        fixed -= hanging;
    }
    (text + fixed, text, fixed)
}

/// The factor by which the text of a line `width` wide with the text indent, `text` of it text
/// and `fixed` the rest, in a room of `available`, grows or shrinks by `fit` (Blink's
/// `LineFitter::MeasureScale`): 1 where it misses the room by less than [`EPSILON`], where `fit`
/// does not grow (or shrink) it, and on the last line of a `per-line` fit; infinite where the
/// line has no text.
fn line_scale(fit: TextFit, (width, text, fixed): (f32, f32, f32), available: f32, last: bool) -> f32 {
    let difference = available - width;
    if difference.abs() < EPSILON {
        return 1.0;
    }
    let grow = fit.kind == TextFitKind::Grow;
    let shrink = fit.kind == TextFitKind::Shrink;
    if (difference > 0.0 && !grow) || (difference < 0.0 && !shrink) {
        return 1.0;
    }
    if fit.target == TextFitTarget::PerLine && last {
        return 1.0;
    }
    if text <= 0.0 {
        return f32::INFINITY;
    }
    restrict((available - fixed) / text, grow, fit.limit)
}

/// Keeps a factor within the limit of `text-fit`: growing up to the limit, shrinking down to it
/// (Blink's `RestrictScale`); a limit on the wrong side of 1 keeps the text as it is.
fn restrict(scale: f32, grow: bool, limit: Option<f32>) -> f32 {
    match limit {
        None => scale,
        Some(limit) if grow => scale.min(limit.max(1.0)),
        Some(limit) => scale.max(limit.min(1.0)),
    }
}

/// A line's content scaled by a factor: its text and the font sizes of its boxes and strut.
pub(super) struct Scaled {
    pub boxes: Vec<InlineBox>,
    pub container: ComputedStyle,
}

/// Scales the text of a line of `pieces` by `scale` as Chrome paints it scaled (Blink's
/// `ScaleLine`): its glyphs and widths grow, and so do the font sizes of the text, of the
/// inline boxes it lies in and of the line's strut (`container`), which the heights of the
/// line and its boxes follow.
pub(super) fn scale_line(
    pieces: &mut [Piece],
    boxes: &[InlineBox],
    container: &ComputedStyle,
    scale: f32,
) -> Scaled {
    let mut styles: Vec<(Arc<ComputedStyle>, Arc<ComputedStyle>)> = Vec::new();
    let mut scaled = |style: &Arc<ComputedStyle>| {
        if let Some((_, done)) = styles.iter().find(|(own, _)| Arc::ptr_eq(own, style)) {
            return Arc::clone(done);
        }
        let done = Arc::new(scaled_style(style, scale));
        styles.push((Arc::clone(style), Arc::clone(&done)));
        done
    };
    let boxes = boxes
        .iter()
        .map(|inline_box| InlineBox {
            style: scaled(&inline_box.style),
            ..inline_box.clone()
        })
        .collect();
    for piece in pieces.iter_mut() {
        let spacing = fixed_spacing(piece);
        if let PieceKind::Text { glyphs, .. } = &mut piece.kind {
            // Letter and word spacing of fixed lengths keep their size, as Chrome sets such
            // text again at the scaled font size (`LineFitter::FitLine`).
            let mut kept = 0.0;
            for (index, glyph) in glyphs.iter_mut().enumerate() {
                let fixed = spacing.get(index).copied().unwrap_or_default();
                glyph.advance = (glyph.advance - fixed) * scale + fixed;
                glyph.x_offset *= scale;
                glyph.y_offset *= scale;
                kept += fixed;
            }
            piece.width = (piece.width - kept) * scale + kept;
            piece.min_width = piece.width.min(piece.min_width * scale);
            piece.trailing_space *= scale;
            piece.hyphen *= scale;
        }
        if matches!(
            piece.kind,
            PieceKind::Text { .. }
                | PieceKind::Open(_)
                | PieceKind::Close(_)
                | PieceKind::Left { .. }
                | PieceKind::Right { .. }
        ) {
            piece.style = scaled(&piece.style);
        }
    }
    Scaled {
        boxes,
        container: scaled_style(container, scale),
    }
}

/// A style whose font size is `scale` times as large, with the letter and word spacing that
/// percentages give scaled along.
fn scaled_style(style: &ComputedStyle, scale: f32) -> ComputedStyle {
    let (letter, word) = fixed_lengths(style);
    ComputedStyle {
        font_size: style.font_size * scale,
        text_fit_scale: style.text_fit_scale * scale,
        letter_spacing: letter + (style.letter_spacing - letter) * scale,
        word_spacing: word + (style.word_spacing - word) * scale,
        ..style.clone()
    }
}

/// The parts of a style's letter and word spacing that are fixed lengths, not percentages of
/// the font size.
fn fixed_lengths(style: &ComputedStyle) -> (f32, f32) {
    let fixed =
        |index: usize, value: f32| style.spacing_percentages[index].map_or(value, |(pixels, _)| pixels);
    (fixed(0, style.letter_spacing), fixed(1, style.word_spacing))
}

/// The letter and word spacing of fixed lengths that each glyph of a text piece carries: letter
/// spacing after the last glyph of each cluster whose character takes it, word spacing after
/// spaces (as the pieces are shaped).
fn fixed_spacing(piece: &Piece) -> Vec<f32> {
    let PieceKind::Text { glyphs, text, .. } = &piece.kind else {
        return Vec::new();
    };
    let (letter, word) = fixed_lengths(&piece.style);
    if letter == 0.0 && word == 0.0 {
        return Vec::new();
    }
    let cursive = if letter == 0.0 {
        Vec::new()
    } else {
        crate::letter_spacing::cursive_bytes(text, None)
    };
    glyphs
        .iter()
        .enumerate()
        .map(|(index, glyph)| {
            let start = glyph.cluster.0 as usize;
            let character = text.get(start..).and_then(|rest| rest.chars().next());
            let last_of_cluster = glyphs
                .get(index + 1)
                .is_none_or(|next| next.cluster != glyph.cluster);
            let mut spacing = 0.0;
            if letter != 0.0
                && last_of_cluster
                && character.is_some_and(|character| {
                    crate::letter_spacing::spaced(character, cursive.get(start).copied().unwrap_or(false))
                })
            {
                spacing += letter;
            }
            if matches!(character, Some(' ' | '\u{a0}')) {
                spacing += word;
            }
            spacing
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{TextFit, TextFitKind, TextFitTarget, line_scale, restrict};

    fn fit(kind: TextFitKind, target: TextFitTarget, limit: Option<f32>) -> TextFit {
        TextFit { kind, target, limit }
    }

    #[test]
    fn scales_the_text_of_a_line_to_fill_it() {
        let grow = fit(TextFitKind::Grow, TextFitTarget::PerLineAll, None);
        // 80px of text in 120px grows by half; the borders of a box keep their size.
        assert!((line_scale(grow, (80.0, 80.0, 0.0), 120.0, false) - 1.5).abs() < 1e-6);
        assert!((line_scale(grow, (84.0, 80.0, 4.0), 124.0, false) - 1.5).abs() < 1e-6);
        // Within two pixels the line is left as it is, and grown text does not shrink.
        assert!((line_scale(grow, (119.0, 119.0, 0.0), 120.0, false) - 1.0).abs() < 1e-6);
        assert!((line_scale(grow, (140.0, 140.0, 0.0), 120.0, false) - 1.0).abs() < 1e-6);
        // `per-line` leaves the last line.
        let per_line = fit(TextFitKind::Grow, TextFitTarget::PerLine, None);
        assert!((line_scale(per_line, (80.0, 80.0, 0.0), 120.0, true) - 1.0).abs() < 1e-6);
        // A line without text cannot be fitted.
        assert!(line_scale(grow, (20.0, 0.0, 20.0), 120.0, false).is_infinite());
    }

    #[test]
    fn keeps_the_factor_within_the_limit() {
        assert!((restrict(0.5, false, Some(0.75)) - 0.75).abs() < 1e-6);
        assert!((restrict(3.0, true, Some(2.0)) - 2.0).abs() < 1e-6);
        // A limit on the wrong side of 1 keeps the text as it is.
        assert!((restrict(3.0, true, Some(0.5)) - 1.0).abs() < 1e-6);
        assert!((restrict(0.5, false, None) - 0.5).abs() < 1e-6);
    }
}

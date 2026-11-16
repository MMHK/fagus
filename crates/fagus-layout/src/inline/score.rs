//! Where the lines of paragraphs with `text-wrap: pretty` and `text-wrap: balance` break, as
//! Chrome breaks them (CSS Text 4, §6.2).
//!
//! Chrome first breaks a paragraph greedily. A pretty paragraph whose last line is short and a
//! single word (or whose two lines before the last are both hyphenated) breaks its last four
//! lines anew; a balanced paragraph of up to six lines breaks all of them anew. The new breaks
//! are those of the least score, in the manner of Knuth and Plass: the squared room each line
//! leaves, penalties for hyphens and lines, and one for the last break, which keeps a single
//! word off the last line. Where that gives another number of lines, the greedy breaks stay. A
//! balanced paragraph it cannot break so is balanced by bisecting the width at which greedy
//! breaking keeps its number of lines.
//!
//! This follows Blink's `score_line_breaker.cc`, `line_break_candidate.cc` (and the candidates
//! of `LineBreaker::AppendCandidates`), `paragraph_line_breaker.cc` and the line break strategy
//! of `inline_layout_algorithm.cc`, under their BSD license (`CHROMIUM-LICENSE`).

use std::collections::VecDeque;
use std::ops::Range;

use fagus_style::ComputedStyle;
use fagus_style::values::{TextAlign, TextWrapStyle, WhiteSpaceCollapse};

use super::{Piece, PieceKind, broken_delta, is_opportunity, line_width};

/// The most lines a balanced paragraph may have (`kMaxLinesForBalance`).
const MAX_BALANCED_LINES: usize = 6;
/// How many lines at the end of a pretty paragraph break anew (`kMaxLinesForOptimal`).
const MAX_PRETTY_LINES: usize = 4;
/// The score of a line that does not fit (`kScoreOverfull`).
const OVERFULL: f32 = 1e12;
/// How much more the penalty of a hyphen costs on the last line (`kLastLinePenaltyMultiplier`).
const LAST_LINE_PENALTY_MULTIPLIER: f32 = 4.0;
/// The penalty of the last break, against a single word on the last line (`kOrphansPenalty`).
const ORPHANS_PENALTY: f32 = 10000.0;
/// The smallest step of Blink's layout units, by which a line may be too wide and still fit.
const LAYOUT_UNIT: f32 = 1.0 / 64.0;

/// The room of the lines of a block for breaking them anew (Blink's `LineWidths`): their
/// width, the first line's indent, and a float that the block starts with, which takes its
/// width off as many lines as its height takes of the block's line height.
#[derive(Clone, Copy, Debug)]
pub(super) struct Room {
    pub(super) width: f32,
    indent: f32,
    excluded: f32,
    excluded_lines: usize,
    /// The piece of that float.
    pub(super) float: Option<usize>,
}

impl Room {
    /// The room of lines `width` wide, the first `indent` less, for `pieces`; `None` where Chrome
    /// cannot know the room of each line without laying them out: where a float comes after
    /// the content starts or is an initial letter, or where one at the start takes room and
    /// atomic inlines or inline boxes off the baseline may make lines taller than
    /// `line_height`, the block's.
    fn of(pieces: &VecDeque<Piece>, (width, indent): (f32, f32), line_height: f32) -> Option<Self> {
        let mut room = Self {
            width,
            indent,
            excluded: 0.0,
            excluded_lines: 0,
            float: None,
        };
        let leading = pieces
            .iter()
            .position(|piece| !matches!(piece.kind, PieceKind::Open(_)));
        for (index, piece) in pieces.iter().enumerate() {
            let PieceKind::Float { size, .. } = &piece.kind else {
                continue;
            };
            if Some(index) != leading || piece.style.initial_letter.is_some() {
                return None;
            }
            room.float = Some(index);
            let (float_width, float_height) = (size.0.to_px(), size.1.to_px());
            if float_width > 0.0 && float_height > 0.0 {
                if line_height <= 0.0 {
                    return None;
                }
                room.excluded = float_width;
                room.excluded_lines = (float_height / line_height).ceil() as usize;
            }
        }
        let uneven = |piece: &Piece| match piece.kind {
            PieceKind::Atomic { .. } => true,
            PieceKind::Open(_) => piece.style.vertical_align != fagus_style::values::VerticalAlign::Baseline,
            _ => false,
        };
        if room.excluded_lines > 0 && pieces.iter().any(uneven) {
            return None;
        }
        Some(room)
    }

    /// The room of line `line` of the block.
    pub(super) fn available(&self, line: usize) -> f32 {
        self.width
            - if line == 0 { self.indent } else { 0.0 }
            - if line < self.excluded_lines {
                self.excluded
            } else {
                0.0
            }
    }
}

/// How far the lines of a block may reach (from their start, with the first line's indent) for
/// `text-wrap: balance` and `pretty`, so that greedy breaking breaks them where Chrome does;
/// empty for greedy lines. `width` is the width of the lines, `indent` the first line's indent,
/// `line_height` the block's. Each part of the block that a forced break ends is a paragraph of
/// its own; only the first is balanced.
pub(super) fn wrap_limits(
    pieces: &VecDeque<Piece>,
    container: &ComputedStyle,
    (width, indent): (f32, f32),
    line_height: f32,
) -> Vec<f32> {
    let wrap = container.text_wrap_style;
    if !matches!(wrap, TextWrapStyle::Balance | TextWrapStyle::Pretty) || disabled(pieces, container) {
        return Vec::new();
    }
    let Some(room) = Room::of(pieces, (width, indent), line_height) else {
        return Vec::new();
    };
    let params = Parameters::of(container, width, wrap == TextWrapStyle::Balance);
    let mut limits = Vec::new();
    let mut start = 0;
    while start < pieces.len() {
        let end = (start..pieces.len())
            .find(|&index| pieces[index].forced_break_after)
            .map_or(pieces.len(), |index| index + 1);
        // The line of the block the paragraph starts on.
        let line = limits.len();
        // A paragraph with a word too wide for its lines breaks greedily, and so do the rest.
        let Some(greedy) = super::greedy_breaks(pieces, start..end, &room, line) else {
            break;
        };
        let paragraph = match (params.balanced, start) {
            (true, 0) => balance(pieces, &greedy, &params, &room, end == pieces.len()),
            (true, _) => None,
            (false, _) => prettify(pieces, (start, line, &greedy), &params, &room),
        };
        limits.extend(paragraph.unwrap_or_else(|| vec![width; greedy.len()]));
        start = end;
    }
    limits
}

/// The limits of the lines of the first paragraph of a balanced block (`greedy`, its greedy
/// lines): broken anew where it has no more than six lines, or else at the width that bisecting
/// finds, where the block has no forced break (`whole`).
fn balance(
    pieces: &VecDeque<Piece>,
    greedy: &[(usize, f32)],
    params: &Parameters,
    room: &Room,
    whole: bool,
) -> Option<Vec<f32>> {
    let count = greedy.len();
    if count <= 1 || count > MAX_BALANCED_LINES {
        return None;
    }
    if let Some(ends) = optimal_breaks(pieces, (0, 0, greedy), params, room) {
        return Some(limits(pieces, &ends, 0, (0, room.indent)));
    }
    // Bisecting needs lines of one width.
    (whole && room.excluded_lines == 0).then(|| vec![bisect(pieces, greedy, room); count])
}

/// The limits of the lines of a pretty paragraph from piece `start` on (`greedy`, its greedy
/// lines, the first of them line `line` of the block): its last four lines broken anew where
/// that is worth it.
fn prettify(
    pieces: &VecDeque<Piece>,
    (start, line, greedy): (usize, usize, &[(usize, f32)]),
    params: &Parameters,
    room: &Room,
) -> Option<Vec<f32>> {
    let count = greedy.len();
    if count <= 1 {
        return None;
    }
    let first = count.saturating_sub(MAX_PRETTY_LINES);
    let window = &greedy[first..];
    let window_start = first.checked_sub(1).map_or(start, |line| greedy[line].0);
    let line_start = |line: usize| {
        if line == 0 {
            window_start
        } else {
            window[line - 1].0
        }
    };
    if !should_optimize(pieces, window, line_start, room.available(line + count - 1)) {
        return None;
    }
    let ends = optimal_breaks(pieces, (window_start, line + first, window), params, room)?;
    let mut limits = vec![room.width; first];
    limits.extend(self::limits(
        pieces,
        &ends,
        window_start,
        (line + first, room.indent),
    ));
    Some(limits)
}

/// Whether Chrome breaks a paragraph greedily whatever its `text-wrap`: where a line starts
/// inside an inline box that clones its decorations, or where spaces break
/// (`disable_score_line_break_`).
fn disabled(pieces: &VecDeque<Piece>, container: &ComputedStyle) -> bool {
    container.white_space.collapse == WhiteSpaceCollapse::BreakSpaces
        || pieces.iter().any(|piece| match piece.kind {
            PieceKind::Open(_) => piece.style.clones_decorations(),
            PieceKind::Text { .. } => piece.style.white_space.collapse == WhiteSpaceCollapse::BreakSpaces,
            _ => false,
        })
}

/// The widths that make greedy breaking end lines after the pieces before `ends`: each line's
/// own width (with the indent of line 0 of the paragraph), and half the room the next word
/// would need more, so that the line keeps its words where it is measured a little otherwise
/// as it is laid out (kerned at its start, say). `start` is the piece the first of them starts
/// at, `first` its line in the paragraph.
fn limits(pieces: &VecDeque<Piece>, ends: &[usize], start: usize, (first, indent): (usize, f32)) -> Vec<f32> {
    let mut from = start;
    ends.iter()
        .enumerate()
        .map(|(row, &end)| {
            let used = line_width(&pieces.range(from..end).collect::<Vec<_>>());
            let next = (end..pieces.len())
                .find(|&index| is_opportunity(&pieces[index]))
                .map(|index| line_width(&pieces.range(from..=index).collect::<Vec<_>>()));
            let slack = next.map_or(0.005, |next| ((next - used) / 2.0).max(0.005));
            from = end;
            used + if first + row == 0 { indent } else { 0.0 } + slack
        })
        .collect()
}

/// Whether the last lines of a pretty paragraph (`window`, the index after each line's last
/// piece and its width; `line_start` gives the first piece of each) are worth breaking anew
/// (`ShouldOptimize`): the last line is shorter than a third of the width and has no break
/// opportunity inside, or the two lines before it end with hyphens.
fn should_optimize(
    pieces: &VecDeque<Piece>,
    window: &[(usize, f32)],
    line_start: impl Fn(usize) -> usize,
    width: f32,
) -> bool {
    let last = window.len() - 1;
    let (end, used) = window[last];
    let breaks_inside = (line_start(last)..end.saturating_sub(1)).any(|index| {
        let piece = &pieces[index];
        // Hyphenation points are no break opportunities of the text; soft hyphens are.
        piece.break_after
            && (piece.hyphen == 0.0
                || matches!(&piece.kind, PieceKind::Text { text, .. } if text.ends_with('\u{AD}')))
    });
    if used < width / 3.0 && !breaks_inside {
        return true;
    }
    let hyphenated = |line: usize| pieces[window[line].0 - 1].hyphen > 0.0;
    window.len() >= 3 && hyphenated(last - 1) && hyphenated(last - 2)
}

/// The parameters of the scores of a paragraph (`SetupParameters`, after Minikin's
/// `computePenalties`).
struct Parameters {
    balanced: bool,
    justified: bool,
    hyphen_penalty: f32,
    line_penalty: f32,
}

impl Parameters {
    fn of(container: &ComputedStyle, width: f32, balanced: bool) -> Self {
        let width_times_font_size = width.max(0.0) * container.font_size;
        let justified = container.text_align == TextAlign::Justify;
        // Justified text hyphenates more readily and has no line penalty.
        let (hyphen_penalty, line_penalty) = if justified {
            (width_times_font_size / 2.0, 0.0)
        } else {
            (width_times_font_size * 2.0, width_times_font_size * 4.0)
        };
        Self {
            balanced,
            justified,
            hyphen_penalty,
            line_penalty,
        }
    }
}

/// A place where a line may break (`LineBreakCandidate`): the index after the last piece of the
/// line it ends, where the next line starts (after the spaces that hang) and where the line ends
/// (before them, with its hyphen), both from the start of the first line, and its penalty.
#[derive(Clone, Copy, Debug)]
struct Candidate {
    end: usize,
    pos_no_break: f32,
    pos_if_break: f32,
    penalty: f32,
    hyphenated: bool,
}

/// The best score of the lines up to a candidate, the candidate before them, and their number
/// (`LineBreakScore`).
#[derive(Clone, Copy, Debug)]
struct Score {
    total: f32,
    previous: usize,
    lines: usize,
}

/// The ends of the lines (the index after each line's last piece) that break the lines from
/// piece `start` on (`greedy`, the greedy lines, as many as are to be broken anew) with the
/// least score (`ScoreLineBreaker::Optimize`); `None` where there are fewer than three break
/// opportunities or the least score takes another number of lines. The first of the lines is
/// line `line` of the block, whose lines have `room`.
fn optimal_breaks(
    pieces: &VecDeque<Piece>,
    (start, line, greedy): (usize, usize, &[(usize, f32)]),
    params: &Parameters,
    room: &Room,
) -> Option<Vec<usize>> {
    let end = greedy.last().map_or(start, |&(end, _)| end);
    let mut candidates = candidates(pieces, start..end, params.hyphen_penalty);
    // Sentinels at the start and the end, and at least three break opportunities between them.
    if candidates.len() < 3 + 2 {
        return None;
    }
    // The last break costs more, against typographic orphans; so do the hyphenation points of
    // the last word before it.
    let last = candidates.len() - 1;
    for candidate in candidates[..last].iter_mut().rev() {
        candidate.penalty += ORPHANS_PENALTY;
        if !candidate.hyphenated {
            break;
        }
    }
    let scores = scores(&candidates, params, room, line);
    // The chosen candidates, from the last back.
    let mut ends = Vec::new();
    let mut index = last;
    while index > 0 {
        ends.push(candidates[index].end);
        index = scores[index].previous;
    }
    ends.reverse();
    (ends.len() == greedy.len()).then_some(ends)
}

/// The places where the lines of the pieces in `range` may break (`LineBreakCandidateContext`):
/// the start, each break opportunity, and the end.
fn candidates(pieces: &VecDeque<Piece>, range: Range<usize>, hyphen_penalty: f32) -> Vec<Candidate> {
    let mut candidates = vec![Candidate {
        end: range.start,
        pos_no_break: 0.0,
        pos_if_break: 0.0,
        penalty: 0.0,
        hyphenated: false,
    }];
    let mut position = 0.0;
    for (index, piece) in pieces.range(range.clone()).enumerate() {
        let index = range.start + index;
        position += piece.width;
        let end = index + 1 == range.end;
        if !end && !is_opportunity(piece) {
            continue;
        }
        let hyphenated = !end && piece.hyphen > 0.0;
        candidates.push(Candidate {
            end: index + 1,
            pos_no_break: position,
            pos_if_break: position - piece.trailing_space
                + if end {
                    0.0
                } else {
                    piece.hyphen + broken_delta(piece)
                },
            penalty: if hyphenated { hyphen_penalty } else { 0.0 },
            hyphenated,
        });
    }
    candidates
}

/// The best score of the lines up to each candidate (`ScoreLineBreaker::ComputeScores`), the
/// first of them line `first` of the block, whose lines have `room`.
fn scores(candidates: &[Candidate], params: &Parameters, room: &Room, first: usize) -> Vec<Score> {
    // The room of a line, and a layout unit more.
    let fit = |line: usize| room.available(first + line).max(0.0) + LAYOUT_UNIT;
    let mut scores = vec![Score {
        total: 0.0,
        previous: 0,
        lines: 0,
    }];
    let mut active = 0;
    for (end, end_candidate) in candidates.iter().enumerate().skip(1) {
        let last = end == candidates.len() - 1;
        let mut best = f32::MAX;
        let mut best_previous = 0;
        let mut last_line = scores[active].lines;
        let mut available = fit(last_line);
        let mut start_edge = end_candidate.pos_if_break - available;
        let mut best_hope = 0.0;
        // Lines that do not fit move the first start of the later ends on.
        let first = active;
        for start in first..end {
            let start_score = scores[start];
            if start_score.lines != last_line {
                last_line = start_score.lines;
                let room = fit(last_line);
                if room.to_bits() != available.to_bits() {
                    available = room;
                    start_edge = end_candidate.pos_if_break - available;
                    best_hope = 0.0;
                }
            }
            if start_score.total + best_hope >= best {
                continue;
            }
            let start_candidate = &candidates[start];
            // The room the line leaves; negative where it does not fit.
            let delta = start_candidate.pos_no_break - start_edge;
            let mut width_score = 0.0;
            let mut additional_penalty = 0.0;
            if (last || !params.justified) && delta < 0.0 {
                width_score = OVERFULL;
            } else if last && !params.balanced {
                // The last line of a pretty paragraph may be short, but not hyphenated.
                additional_penalty = LAST_LINE_PENALTY_MULTIPLIER * start_candidate.penalty;
            } else if delta < 0.0 {
                width_score = OVERFULL;
            } else {
                width_score = delta * delta;
            }
            if delta < 0.0 {
                active = start + 1;
            } else {
                best_hope = width_score;
            }
            let score = start_score.total + width_score + additional_penalty;
            if score <= best {
                best = score;
                best_previous = start;
            }
        }
        scores.push(Score {
            total: best + end_candidate.penalty + params.line_penalty,
            previous: best_previous,
            lines: scores[best_previous].lines + 1,
        });
    }
    scores
}

/// The width at which greedy lines of a paragraph that breaks into `greedy` lines at `width`
/// are balanced (`ParagraphLineBreaker::AttemptParagraphBalancing`): the narrowest that keeps
/// their number, bisected to a pixel from 80% of their mean width, in layout units.
fn bisect(pieces: &VecDeque<Piece>, greedy: &[(usize, f32)], room: &Room) -> f32 {
    let units = |px: f32| (px * 64.0).trunc();
    let count = greedy.len() as f32;
    let sum: f32 = greedy.iter().map(|&(_, used)| units(used)).sum();
    let mean = (sum / count).trunc();
    let (mut lower, mut upper) = ((mean * 0.8).round(), units(room.width));
    let epsilon = 64.0;
    while lower + epsilon < upper {
        let middle = f32::midpoint(upper, lower).trunc();
        let narrower = Room {
            width: middle / 64.0,
            ..*room
        };
        match super::greedy_breaks(pieces, 0..pieces.len(), &narrower, 0) {
            Some(lines) if lines.len() <= greedy.len() => upper = middle,
            _ => lower = middle,
        }
    }
    upper / 64.0
}

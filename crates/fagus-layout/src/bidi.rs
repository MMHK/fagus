//! Bidirectional text (UAX #9 and CSS Writing Modes 3, §2.4): the embedding levels of a
//! block's inline content, and the order in which the pieces of a line stand on the page.
//!
//! The inline content of a block is one text with the bidi controls that `unicode-bidi` and
//! `direction` of its inline boxes stand for, as in Chrome: an object replacement character
//! for each atomic inline, float or out-of-flow box, and the controls closed before each forced
//! line break and opened again after it. Its paragraphs get their levels from the bidi
//! algorithm; a line, broken in logical order, is then reordered by rule L2, and its inline
//! boxes become fragments around the pieces they hold that stand side by side.

use fagus_style::ComputedStyle;
use fagus_style::values::UnicodeBidi;

/// The bidi embedding level of a piece of inline content, with the level of its paragraph.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Level {
    pub(crate) level: u8,
    pub(crate) paragraph: u8,
}

impl Level {
    /// Left to right in a left-to-right paragraph.
    pub(crate) const LTR: Self = Self {
        level: 0,
        paragraph: 0,
    };

    /// Whether the piece is set right to left.
    pub(crate) fn is_rtl(self) -> bool {
        self.level % 2 == 1
    }

    /// Whether the piece's paragraph is right to left.
    pub(crate) fn paragraph_rtl(self) -> bool {
        self.paragraph % 2 == 1
    }
}

/// The levels of the bytes of a text, and the level of those past the ones given.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TextLevels<'a> {
    bytes: &'a [u8],
    rest: Level,
}

impl TextLevels<'_> {
    /// All left to right, in a left-to-right paragraph.
    pub(crate) const LTR: TextLevels<'static> = TextLevels {
        bytes: &[],
        rest: Level::LTR,
    };

    /// All at `level`.
    pub(crate) fn uniform(level: Level) -> TextLevels<'static> {
        TextLevels {
            bytes: &[],
            rest: level,
        }
    }

    /// The levels of `bytes`, in a paragraph at `paragraph`.
    pub(crate) fn new(bytes: &[u8], paragraph: u8) -> TextLevels<'_> {
        TextLevels {
            bytes,
            rest: Level {
                level: paragraph,
                paragraph,
            },
        }
    }

    /// Whether all bytes are at the same level.
    pub(crate) fn is_uniform(&self) -> bool {
        self.bytes.is_empty()
    }

    /// The level of the byte at `offset`.
    pub(crate) fn at(&self, offset: usize) -> Level {
        Level {
            level: self.bytes.get(offset).copied().unwrap_or(self.rest.level),
            ..self.rest
        }
    }
}

/// What an inline item is to the bidi algorithm.
pub(crate) enum Content<'a> {
    /// Text, as it is shaped.
    Text(&'a str),
    /// The start of an inline box with this style.
    Open(&'a ComputedStyle),
    /// The end of the inline box opened last.
    Close,
    /// A forced line break, which ends a paragraph.
    LineBreak,
    /// An atomic inline or a box out of the flow: one neutral character.
    Object,
    /// Nothing (a list marker outside the line).
    None,
}

/// The levels of the inline items of a block, from [`levels`].
pub(crate) struct Levels {
    /// The level of each byte of the content.
    bytes: Vec<u8>,
    /// Where each item's content lies in it.
    items: Vec<std::ops::Range<usize>>,
    /// The paragraphs: the end of each in the content, and its level.
    paragraphs: Vec<(usize, u8)>,
}

impl Levels {
    /// The levels of the text (or the one character) of item `index`.
    pub(crate) fn item(&self, index: usize) -> TextLevels<'_> {
        let range = self.items.get(index).cloned().unwrap_or_default();
        let paragraph = self
            .paragraphs
            .iter()
            .find(|(end, _)| range.start < *end)
            .or(self.paragraphs.last())
            .map_or(0, |(_, level)| *level);
        TextLevels::new(&self.bytes[range], paragraph)
    }
}

const LRE: &str = "\u{202A}";
const RLE: &str = "\u{202B}";
const PDF: &str = "\u{202C}";
const LRO: &str = "\u{202D}";
const RLO: &str = "\u{202E}";
const LRI: &str = "\u{2066}";
const RLI: &str = "\u{2067}";
const FSI: &str = "\u{2068}";
const PDI: &str = "\u{2069}";
const OBJECT: &str = "\u{FFFC}";

/// The controls that open and close the embedding of a box with `style` (CSS Writing Modes 3,
/// §2.4.2).
fn controls(style: &ComputedStyle) -> (&'static str, &'static str) {
    let rtl = style.is_rtl();
    match style.unicode_bidi {
        UnicodeBidi::Normal => ("", ""),
        UnicodeBidi::Embed => (if rtl { RLE } else { LRE }, PDF),
        UnicodeBidi::Isolate => (if rtl { RLI } else { LRI }, PDI),
        UnicodeBidi::BidiOverride => (if rtl { RLO } else { LRO }, PDF),
        UnicodeBidi::IsolateOverride => (
            if rtl {
                "\u{2068}\u{202E}"
            } else {
                "\u{2068}\u{202D}"
            },
            "\u{202C}\u{2069}",
        ),
        UnicodeBidi::Plaintext => (FSI, PDI),
    }
}

/// The levels of inline content in a block of style `container`, item by item; `None` where
/// all of it is left to right in a left-to-right paragraph.
pub(crate) fn levels<'a>(
    container: &ComputedStyle,
    items: impl Iterator<Item = Content<'a>>,
) -> Option<Levels> {
    let items: Vec<Content<'a>> = items.collect();
    // Left-to-right text in a left-to-right paragraph stays as it is.
    let rtl_box = |style: &ComputedStyle| style.is_rtl() && style.unicode_bidi != UnicodeBidi::Normal;
    if !container.is_rtl()
        && !items.iter().any(|item| match item {
            Content::Text(text) => text.chars().any(is_rtl),
            Content::Open(style) => rtl_box(style),
            _ => false,
        })
    {
        return None;
    }
    let mut content = String::new();
    let mut ranges = Vec::new();
    // The controls that close the embeddings open at the end of the content so far, innermost
    // last, with those that opened them.
    let mut open: Vec<(&str, &str)> = Vec::new();
    // A block that overrides the direction does so for all its content.
    if matches!(
        container.unicode_bidi,
        UnicodeBidi::BidiOverride | UnicodeBidi::IsolateOverride
    ) {
        let opener = if container.is_rtl() { RLO } else { LRO };
        content.push_str(opener);
        open.push((opener, PDF));
    }
    for item in items {
        let start = content.len();
        match item {
            Content::Text(text) => content.push_str(text),
            // The item of an isolate's start or end is its isolate initiator or PDI, which
            // stands at the level around the isolate (rule X5a and X6a); those of other boxes
            // are empty.
            Content::Open(style) => {
                let (opener, closer) = controls(style);
                content.push_str(opener);
                open.push((opener, closer));
                ranges.push(if closer.ends_with(PDI) {
                    start..start + FSI.len()
                } else {
                    content.len()..content.len()
                });
                continue;
            }
            Content::Close => {
                if let Some((_, closer)) = open.pop() {
                    content.push_str(closer);
                    let end = content.len();
                    ranges.push(if closer.ends_with(PDI) {
                        end - PDI.len()..end
                    } else {
                        start..start
                    });
                    continue;
                }
            }
            // Embeddings end at a forced break and go on after it (CSS Writing Modes 3, §2.4.3).
            Content::LineBreak => {
                for (_, closer) in open.iter().rev() {
                    content.push_str(closer);
                }
                let at = content.len();
                content.push('\n');
                for (opener, _) in &open {
                    content.push_str(opener);
                }
                ranges.push(at..at + 1);
                continue;
            }
            Content::Object => content.push_str(OBJECT),
            Content::None => {}
        }
        ranges.push(start..content.len());
    }
    let mut bytes = Vec::with_capacity(content.len());
    let mut paragraphs = Vec::new();
    let mut start = 0;
    while start < content.len() {
        let end = content[start..]
            .find('\n')
            .map_or(content.len(), |offset| start + offset + 1);
        let text = &content[start..end];
        let rtl = match container.unicode_bidi {
            // The direction of the first strong character, else the block's (rules P2 and P3).
            UnicodeBidi::Plaintext => match unicode_bidi::get_base_direction(text) {
                unicode_bidi::Direction::Rtl => true,
                unicode_bidi::Direction::Ltr => false,
                unicode_bidi::Direction::Mixed => container.is_rtl(),
            },
            _ => container.is_rtl(),
        };
        let level = if rtl {
            unicode_bidi::Level::rtl()
        } else {
            unicode_bidi::Level::ltr()
        };
        let info = unicode_bidi::ParagraphBidiInfo::new(text, Some(level));
        let from = bytes.len();
        bytes.extend(info.levels.iter().map(unicode_bidi::Level::number));
        reset_segment_separators(text, &mut bytes[from..], level.number());
        paragraphs.push((end, level.number()));
        start = end;
    }
    Some(Levels {
        bytes,
        items: ranges,
        paragraphs,
    })
}

/// Rule L1 of the bidi algorithm for tabs: a segment separator, and the white space before it,
/// goes back to the paragraph's level, so that the text between tabs is ordered on its own and
/// the tabs stay in logical order.
fn reset_segment_separators(text: &str, levels: &mut [u8], paragraph: u8) {
    use unicode_bidi::BidiClass::{BN, FSI, LRI, PDI, RLI, S, WS};
    let mut whitespace = None;
    for (offset, character) in text.char_indices() {
        let class = unicode_bidi::bidi_class(character);
        if class == S {
            let start = whitespace.unwrap_or(offset);
            levels[start..offset + character.len_utf8()].fill(paragraph);
            whitespace = None;
        } else if matches!(class, WS | BN | FSI | LRI | RLI | PDI) {
            whitespace.get_or_insert(offset);
        } else {
            whitespace = None;
        }
    }
}

/// Whether a character makes text right to left: it is written right to left, an Arabic
/// number, or a control that opens a right-to-left embedding.
fn is_rtl(character: char) -> bool {
    u32::from(character) >= 0x0590
        && matches!(
            unicode_bidi::bidi_class(character),
            unicode_bidi::BidiClass::R
                | unicode_bidi::BidiClass::AL
                | unicode_bidi::BidiClass::AN
                | unicode_bidi::BidiClass::RLE
                | unicode_bidi::BidiClass::RLO
                | unicode_bidi::BidiClass::RLI
        )
}

/// The runs of a text set on its own as a paragraph, left to right or right to left, in the
/// order they stand on the page: their byte ranges and levels. One run where all of the text
/// is left to right.
pub(crate) fn visual_runs(text: &str, rtl: bool) -> Vec<(std::ops::Range<usize>, u8)> {
    if !rtl && !text.chars().any(is_rtl) {
        return vec![(0..text.len(), 0)];
    }
    let level = if rtl {
        unicode_bidi::Level::rtl()
    } else {
        unicode_bidi::Level::ltr()
    };
    let info = unicode_bidi::ParagraphBidiInfo::new(text, Some(level));
    let (levels, runs) = info.visual_runs(0..text.len());
    runs.into_iter()
        .map(|run| {
            let level = levels.get(run.start).map_or(0, unicode_bidi::Level::number);
            (run, level)
        })
        .collect()
}

/// A piece of a line in logical order, as [`reorder`] takes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Logical {
    /// The start of inline box `index`.
    Open(usize),
    /// The end of inline box `index`.
    Close(usize),
    /// The start of inline box `index` that isolates its content, whose isolate initiator
    /// stands at `level` between the content around the box and its own (rule L2).
    IsolateOpen { index: usize, level: u8 },
    /// The end of inline box `index` that isolates its content, with its PDI at `level`.
    IsolateClose { index: usize, level: u8 },
    /// Content at a level. Hanging content (the spaces at the end of the line, at the level of
    /// the paragraph by rule L1) stands outside the inline boxes.
    Leaf { level: u8, hanging: bool },
}

/// A piece of a line in the order the pieces stand on the page, from [`reorder`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Visual {
    /// The left side of a fragment of inline box `index`, with the box's margin, border and
    /// padding there if `edge`.
    Left { index: usize, edge: bool },
    /// The right side of a fragment of inline box `index`.
    Right { index: usize, edge: bool },
    /// Piece `index` of the logical line. The `Open` of a box that holds nothing on the line
    /// stands for its content.
    Leaf(usize),
}

/// The pieces of a line (in logical order, with inline boxes `continuing` from the line
/// before) in the order they stand on the page (rule L2). Each inline box becomes fragments
/// around its pieces that stand side by side; as in Chrome, its left side goes to its leftmost
/// fragment and its right side to its rightmost one, where the line has them: the side of its
/// start where it starts on the line, that of its end where it ends there, by its direction.
/// `sides` tells of a box whether it is right to left and whether each of its fragments has
/// both sides (`box-decoration-break: clone`).
pub(crate) fn reorder(
    line: &[Logical],
    continuing: &[usize],
    paragraph: u8,
    sides: impl Fn(usize) -> (bool, bool),
) -> Vec<Visual> {
    // The leaves with their levels and the boxes around them, outermost first, and whether
    // they are the controls of an isolate, which take part in the reordering but stand for
    // nothing.
    let mut leaves: Vec<(usize, Option<u8>, Vec<usize>, bool)> = Vec::new();
    let mut stack = continuing.to_vec();
    let mut started = Vec::new();
    let mut ended = Vec::new();
    // For each open box: where it opened on the line, and whether it holds a leaf there.
    let mut filled: Vec<(Option<usize>, bool)> = continuing.iter().map(|_| (None, false)).collect();
    for (position, piece) in line.iter().enumerate() {
        match *piece {
            Logical::Open(index) | Logical::IsolateOpen { index, .. } => {
                if let Logical::IsolateOpen { level, .. } = *piece {
                    leaves.push((position, Some(level), Vec::new(), true));
                }
                stack.push(index);
                started.push(index);
                filled.push((Some(position), false));
            }
            Logical::Close(index) | Logical::IsolateClose { index, .. } => {
                // A box without content on the line still has a fragment, for its sides.
                if let Some((opened, false)) = filled.pop() {
                    leaves.push((opened.unwrap_or(position), None, stack.clone(), false));
                }
                stack.pop();
                ended.push(index);
                if let Logical::IsolateClose { level, .. } = *piece {
                    leaves.push((position, Some(level), Vec::new(), true));
                }
            }
            Logical::Leaf { level, hanging } => {
                if !hanging {
                    for entry in &mut filled {
                        entry.1 = true;
                    }
                }
                leaves.push((
                    position,
                    Some(level),
                    if hanging { Vec::new() } else { stack.clone() },
                    false,
                ));
            }
        }
    }
    // Boxes opened without content at the end of the line.
    while let Some((opened, holds)) = filled.pop() {
        if let (Some(opened), false) = (opened, holds) {
            leaves.push((opened, None, stack.clone(), false));
        }
        stack.pop();
    }
    leaves.sort_by_key(|leaf| leaf.0);
    // An empty box takes the level of the content after it, or else before it.
    let known: Vec<Option<u8>> = leaves.iter().map(|leaf| leaf.1).collect();
    let levels: Vec<u8> = (0..leaves.len())
        .map(|index| {
            known[index]
                .or_else(|| known[index..].iter().flatten().next().copied())
                .or_else(|| known[..index].iter().rev().flatten().next().copied())
                .unwrap_or(paragraph)
        })
        .collect();
    // Rule L2: from the highest level down to the lowest odd one, reverse every run at that
    // level or higher.
    let mut order: Vec<usize> = (0..leaves.len()).collect();
    let highest = levels.iter().copied().max().unwrap_or(0);
    let lowest_odd = levels.iter().copied().min().unwrap_or(0) | 1;
    for level in (lowest_odd..=highest).rev() {
        let mut start = 0;
        while start < order.len() {
            if levels[order[start]] < level {
                start += 1;
                continue;
            }
            let end = (start..order.len())
                .find(|&at| levels[order[at]] < level)
                .unwrap_or(order.len());
            order[start..end].reverse();
            start = end;
        }
    }
    // The fragments of the boxes around the leaves, in visual order.
    let mut out = Vec::with_capacity(order.len() * 2);
    let mut current: Vec<usize> = Vec::new();
    // Where in `out` each box's fragments start and end.
    let mut fragments: Vec<(usize, usize, usize)> = Vec::new();
    let mut opened_at: Vec<usize> = Vec::new();
    for &leaf in &order {
        if leaves[leaf].3 {
            continue;
        }
        let chain = &leaves[leaf].2;
        let common = current.iter().zip(chain).take_while(|(a, b)| a == b).count();
        while current.len() > common {
            let index = current.pop().unwrap_or_default();
            let start = opened_at.pop().unwrap_or_default();
            fragments.push((index, start, out.len()));
            out.push(Visual::Right { index, edge: false });
        }
        for &index in &chain[common..] {
            current.push(index);
            opened_at.push(out.len());
            out.push(Visual::Left { index, edge: false });
        }
        out.push(Visual::Leaf(leaves[leaf].0));
    }
    while let Some(index) = current.pop() {
        let start = opened_at.pop().unwrap_or_default();
        fragments.push((index, start, out.len()));
        out.push(Visual::Right { index, edge: false });
    }
    // The sides of each box: on its outermost fragments.
    fragments.sort_by_key(|&(index, start, _)| (index, start));
    for (position, &(index, start, end)) in fragments.iter().enumerate() {
        let (rtl, clones) = sides(index);
        let (starts, ends) = (started.contains(&index), ended.contains(&index));
        let (left, right) = if rtl { (ends, starts) } else { (starts, ends) };
        let leftmost = position == 0 || fragments[position - 1].0 != index;
        let rightmost = fragments.get(position + 1).is_none_or(|next| next.0 != index);
        if let Visual::Left { edge, .. } = &mut out[start] {
            *edge = clones || (left && leftmost);
        }
        if let Visual::Right { edge, .. } = &mut out[end] {
            *edge = clones || (right && rightmost);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use fagus_style::values::Direction;

    fn style(direction: Direction, unicode_bidi: UnicodeBidi) -> ComputedStyle {
        ComputedStyle {
            direction,
            unicode_bidi,
            ..ComputedStyle::default()
        }
    }

    #[test]
    fn levels_of_mixed_text() {
        let block = style(Direction::Rtl, UnicodeBidi::Isolate);
        let found = levels(&block, [Content::Text("אב cd 12")].into_iter()).expect("bidi");
        let text = found.item(0);
        let at = |offset| text.at(offset).level;
        // Hebrew at 1, Latin and the numbers after it at 2, the space between Hebrew and
        // Latin at 1.
        assert_eq!((at(0), at(4), at(5), at(7), at(9)), (1, 1, 2, 2, 2));
        assert_eq!(text.at(0).paragraph, 1);
        // Left-to-right content alone needs none.
        let ltr = style(Direction::Ltr, UnicodeBidi::Normal);
        assert!(levels(&ltr, [Content::Text("abc 12")].into_iter()).is_none());
    }

    #[test]
    fn levels_of_boxes() {
        let block = style(Direction::Ltr, UnicodeBidi::Normal);
        let rtl_override = style(Direction::Rtl, UnicodeBidi::IsolateOverride);
        let items = [
            Content::Text("ab "),
            Content::Open(&rtl_override),
            Content::Text("cd"),
            Content::Close,
            Content::LineBreak,
            Content::Object,
        ];
        let found = levels(&block, items.into_iter()).expect("bidi");
        assert_eq!(found.item(0).at(0).level, 0);
        // Overridden to right to left, in an isolate of the direction of its first strong
        // character, as in Chrome.
        assert_eq!(found.item(2).at(1).level, 3);
        assert_eq!(found.item(5).at(0).level, 0);
        // Plain text takes the direction of its first strong character per paragraph.
        let plain = style(Direction::Ltr, UnicodeBidi::Plaintext);
        let items = [Content::Text("שלום ab"), Content::LineBreak, Content::Text("ab")];
        let found = levels(&plain, items.into_iter()).expect("bidi");
        assert_eq!(found.item(0).at(0).paragraph, 1);
        assert_eq!(found.item(2).at(0).paragraph, 0);
    }

    #[test]
    fn visual_runs_of_a_marker() {
        // The suffix, shaped right to left, stands before the number.
        assert_eq!(visual_runs("1. ", true), [(1..3, 1), (0..1, 2)]);
        assert_eq!(visual_runs("1. ", false), [(0..3, 0)]);
    }

    #[test]
    fn reorders_boxes_into_fragments() {
        // abc <b>def שלום</b> עולם xyz, left to right: the box is split in two.
        let leaf = |level| Logical::Leaf {
            level,
            hanging: false,
        };
        let line = [
            leaf(0),
            Logical::Open(0),
            leaf(0),
            leaf(1),
            Logical::Close(0),
            leaf(1),
            leaf(0),
        ];
        let visual = reorder(&line, &[], 0, |_| (false, false));
        assert_eq!(
            visual,
            [
                Visual::Leaf(0),
                Visual::Left { index: 0, edge: true },
                Visual::Leaf(2),
                Visual::Right {
                    index: 0,
                    edge: false
                },
                Visual::Leaf(5),
                Visual::Left {
                    index: 0,
                    edge: false
                },
                Visual::Leaf(3),
                Visual::Right { index: 0, edge: true },
                Visual::Leaf(6),
            ]
        );
        // A right-to-left box that goes on to the next line has its start on the right.
        let line = [Logical::Open(0), leaf(1), leaf(1)];
        let visual = reorder(&line, &[], 1, |_| (true, false));
        assert_eq!(
            visual,
            [
                Visual::Left {
                    index: 0,
                    edge: false
                },
                Visual::Leaf(2),
                Visual::Leaf(1),
                Visual::Right { index: 0, edge: true },
            ]
        );
        // An empty box stands with the content after it.
        let line = [leaf(2), leaf(1), Logical::Open(0), Logical::Close(0), leaf(1)];
        let visual = reorder(&line, &[], 1, |_| (false, false));
        assert_eq!(
            visual,
            [
                Visual::Leaf(4),
                Visual::Left { index: 0, edge: true },
                Visual::Leaf(2),
                Visual::Right { index: 0, edge: true },
                Visual::Leaf(1),
                Visual::Leaf(0),
            ]
        );
    }
}

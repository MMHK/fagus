//! The text around a text being shaped, for the letters of scripts that join (Arabic, Syriac,
//! N'Ko, Mongolian), as Blink shapes them across the edges of inline boxes.
//!
//! Blink shapes the text of each inline item with the text of the whole paragraph as `HarfBuzz`'s
//! context, so that letters take their joining forms across the edges of inline boxes whatever
//! their style (font, size, margins, borders), and across floats and out-of-flow boxes, which
//! have no text there; but not across an atomic inline or a forced break, which stand in that
//! text as an object replacement character and a line feed, nor across the edge of an isolated
//! box (as Chrome shapes it). Where adjacent items have the same font and no margin, border,
//! padding, vertical alignment or bidi embedding separates them, it shapes their text at once
//! and gives each item the glyphs of its characters (`InlineNode::ShapeText`): a ligature
//! across the edge (lam-alef) goes to the item its first character is in, and fonts that join
//! letters by their own tables (Apple's Geeza Pro, whose `morx` table does not look at
//! `HarfBuzz`'s context) join them too. Fagus shapes the text of a word together with the part
//! of the word in such items next to it, and keeps the glyphs of its own characters.

use fagus_style::ComputedStyle;
use fagus_style::values::{LengthPercentage, LengthPercentageAuto, UnicodeBidi, VerticalAlign};

use crate::tree::InlineItem;

/// How many characters next to a text shaping looks at (`HarfBuzz`'s `CONTEXT_LENGTH`).
const CHARS: usize = 5;

/// The most characters of the word in other items that a text is shaped with.
const JOINED: usize = 32;

/// What separates the text of items where shaping does not join across.
const SEPARATOR: char = '\u{FFFC}';

/// The text around a text that shaping looks at; nothing where the text has no letter that
/// joins.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Around {
    /// The characters before the text (before `prefix`) that shaping takes as context.
    pub before: String,
    /// The start of the word in items before that the text is shaped with, and whose glyphs
    /// are left out.
    pub prefix: String,
    /// The rest of the word in items after that the text is shaped with, and whose glyphs are
    /// left out.
    pub suffix: String,
    /// The characters after the text (after `suffix`) that shaping takes as context.
    pub after: String,
}

impl Around {
    /// Nothing around a text.
    pub const NONE: Self = Self {
        before: String::new(),
        prefix: String::new(),
        suffix: String::new(),
        after: String::new(),
    };

    /// Whether there is nothing around the text.
    pub fn is_empty(&self) -> bool {
        self.before.is_empty() && self.prefix.is_empty() && self.suffix.is_empty() && self.after.is_empty()
    }

    /// The text around `text[range]`, where `self` is the text around the whole of `text`: the
    /// prefix and suffix only where the range reaches the start or the end of the text.
    pub fn of(&self, text: &str, range: std::ops::Range<usize>) -> Self {
        if !fagus_text::has_joining(&text[range.clone()]) {
            return Self::default();
        }
        let (before, prefix) = if range.start == 0 {
            (self.before.clone(), self.prefix.clone())
        } else {
            let mut before: Vec<char> = text[..range.start].chars().rev().take(CHARS).collect();
            let missing = CHARS - before.len();
            before.extend(
                self.prefix
                    .chars()
                    .rev()
                    .chain(self.before.chars().rev())
                    .take(missing),
            );
            (before.into_iter().rev().collect(), String::new())
        };
        let (after, suffix) = if range.end == text.len() {
            (self.after.clone(), self.suffix.clone())
        } else {
            let mut after: Vec<char> = text[range.end..].chars().take(CHARS).collect();
            let missing = CHARS - after.len();
            after.extend(self.suffix.chars().chain(self.after.chars()).take(missing));
            (after.into_iter().collect(), String::new())
        };
        Self {
            before,
            prefix,
            suffix,
            after,
        }
    }

    /// The context that [`fagus_text::FontDb::shape_in_context`] takes, before the prefix and
    /// after the suffix.
    pub fn pair(&self) -> (&str, &str) {
        (&self.before, &self.after)
    }
}

/// Whether the start (`end` false) or the end of an inline box of `style` keeps Blink from
/// shaping the text on both sides of it at once: a margin, border or padding there, a vertical
/// alignment, or a bidi embedding or isolation, whose controls are items of their own.
fn breaks_shaping(style: &ComputedStyle, end: bool) -> bool {
    let left = end == style.is_rtl();
    let (margin, padding, border) = if left {
        (&style.margin.left, &style.padding.left, style.border_width.left)
    } else {
        (
            &style.margin.right,
            &style.padding.right,
            style.border_width.right,
        )
    };
    let zero = |length: &LengthPercentage| matches!(length, LengthPercentage::Px(px) | LengthPercentage::Percent(px) if *px == 0.0);
    border != 0.0
        || !zero(padding)
        || !matches!(margin, LengthPercentageAuto::Length(length) if zero(length))
        || style.vertical_align != VerticalAlign::Baseline
        || style.unicode_bidi != UnicodeBidi::Normal
}

/// Whether a box of `style` isolates its content from the text around it.
fn isolates(style: &ComputedStyle) -> bool {
    matches!(
        style.unicode_bidi,
        UnicodeBidi::Isolate | UnicodeBidi::IsolateOverride | UnicodeBidi::Plaintext
    )
}

/// The text around each item of a paragraph (nothing for items other than text), as Blink's
/// shaper sees it; `generated` is the text of generated items (page numbers). `shaped_with(a,
/// b)` says whether text item `a` and the later text item `b` have the same font and direction,
/// so that Blink shapes them at once where nothing else separates them.
pub(super) fn of_items(
    items: &[InlineItem],
    generated: &[Option<String>],
    shaped_with: impl Fn(usize, usize) -> bool,
) -> Vec<Around> {
    let mut text = String::new();
    let mut ranges = Vec::with_capacity(items.len());
    // The styles of the open boxes, and the style of the box each item closes.
    let mut open: Vec<&ComputedStyle> = Vec::new();
    let mut closes: Vec<Option<&ComputedStyle>> = Vec::with_capacity(items.len());
    for (item, generated) in items.iter().zip(generated) {
        let start = text.len();
        let mut closed = None;
        match item {
            InlineItem::Text { text: own, .. } => text.push_str(own),
            InlineItem::PageNumber { .. }
            | InlineItem::TargetPage { .. }
            | InlineItem::NamedString { .. } => {
                text.push_str(generated.as_deref().unwrap_or_default());
            }
            InlineItem::Open { style, .. } => {
                if isolates(style) {
                    text.push(SEPARATOR);
                }
                open.push(style);
            }
            InlineItem::Close => {
                closed = open.pop();
                if closed.is_some_and(isolates) {
                    text.push(SEPARATOR);
                }
            }
            InlineItem::LineBreak { .. } => text.push('\n'),
            InlineItem::Atomic(_) => text.push(SEPARATOR),
            InlineItem::Float(_)
            | InlineItem::OutOfFlow(_)
            | InlineItem::Marker { .. }
            | InlineItem::Leader { .. } => {}
        }
        closes.push(closed);
        ranges.push(start..text.len());
    }
    // Whether Blink shapes the text before box edge `index` with the text after it.
    let passes = |index: usize| match &items[index] {
        InlineItem::Open { style, .. } => !breaks_shaping(style, false),
        InlineItem::Close => closes[index].is_some_and(|style| !breaks_shaping(style, true)),
        _ => false,
    };
    let in_word = |character: char| !character.is_whitespace() && character != SEPARATOR;
    let joins_at = |part: &str, first: bool| {
        let edge = if first {
            part.chars().next()
        } else {
            part.chars().next_back()
        };
        edge.is_some_and(fagus_text::joins) && part.chars().count() <= JOINED
    };
    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let InlineItem::Text { text: own, .. } = item else {
                return Around::default();
            };
            let range = ranges[index].clone();
            // The start of the word in the items before that are shaped with this one; Blink
            // does not shape across a zero width non-joiner at the start of an item.
            let mut start = range.start;
            let mut earlier = index;
            while earlier > 0 && own.starts_with(fagus_text::joins) {
                earlier -= 1;
                match &items[earlier] {
                    InlineItem::Text { text: other, .. } if shaped_with(earlier, index) => {
                        let rest = other.trim_end_matches(in_word);
                        start = ranges[earlier].start + rest.len();
                        if !rest.is_empty() {
                            break;
                        }
                    }
                    _ if passes(earlier) => {}
                    _ => break,
                }
            }
            // The end of the word in the items after that are shaped with this one.
            let mut end = range.end;
            let mut later = index;
            while later + 1 < items.len() && own.ends_with(fagus_text::joins) {
                later += 1;
                match &items[later] {
                    InlineItem::Text { text: other, .. }
                        if shaped_with(index, later) && !other.starts_with('\u{200C}') =>
                    {
                        let rest = other.trim_start_matches(in_word);
                        end = ranges[later].end - rest.len();
                        if !rest.is_empty() {
                            break;
                        }
                    }
                    _ if passes(later) => {}
                    _ => break,
                }
            }
            let (mut prefix, mut suffix) = (&text[start..range.start], &text[range.end..end]);
            if !joins_at(prefix, false) {
                prefix = "";
            }
            if !joins_at(suffix, true) {
                suffix = "";
            }
            let start = range.start - prefix.len();
            let end = range.end + suffix.len();
            let before: Vec<char> = text[..start].chars().rev().take(CHARS).collect();
            let around = Around {
                before: before.into_iter().rev().collect(),
                prefix: prefix.to_owned(),
                suffix: suffix.to_owned(),
                after: text[end..].chars().take(CHARS).collect(),
            };
            around.of(own, 0..own.len())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::Around;

    #[test]
    fn keeps_the_five_characters_next_to_text_that_joins() {
        let text = "abc عربي def";
        let word = text.find('ع').unwrap()..text.rfind(' ').unwrap();
        let around = Around::default().of(text, word);
        assert_eq!((around.before.as_str(), around.after.as_str()), ("abc ", " def"));
        // Text that does not join has no context.
        assert_eq!(Around::default().of(text, 0..3), Around::default());
    }

    #[test]
    fn takes_the_rest_from_the_text_around() {
        let outer = Around {
            before: "12345".to_owned(),
            prefix: "ab".to_owned(),
            suffix: "x".to_owned(),
            after: "yzuvw".to_owned(),
        };
        // Inside, the prefix and suffix are context.
        let around = outer.of("ببب", 2..4);
        assert_eq!(around.before, "45abب");
        assert_eq!(around.after, "بxyzu");
        assert!(around.prefix.is_empty() && around.suffix.is_empty());
        // At the edges, they stay.
        assert_eq!(outer.of("ببب", 0..6), outer);
    }
}

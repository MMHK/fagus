//! Where lines may break in a paragraph, as Blink finds it: over the text of the whole
//! paragraph, across the boundaries of inline boxes, by the `word-break` and `line-break` of
//! each box.

use std::cell::RefCell;
use std::ops::Range;

use fagus_style::ComputedStyle;
use fagus_style::values::{LineBreak, WhiteSpaceCollapse, WordBreak};
use fagus_text::Strictness;

use crate::tree::{BoxKind, BoxTree, InlineItem};

/// The break opportunities of a text on its own, and a break at its end.
pub(crate) fn own_breaks(text: &str) -> (Vec<fagus_text::BreakOpportunity>, bool) {
    (fagus_text::break_opportunities(text), true)
}

/// The text of a paragraph as the line breaker sees it, as in Blink: the text of its items, with
/// an object replacement character for each atomic inline (the text of the base for a ruby
/// column) and a line feed for each forced break, so that lines break where this text allows,
/// also between the texts of different inline boxes.
pub(crate) struct Paragraph {
    pub text: String,
    /// The range of each item in the text.
    pub ranges: Vec<Range<usize>>,
    opportunities: Vec<fagus_text::BreakOpportunity>,
    /// The break opportunities of the text with other values of `line-break`, and between
    /// phrases (`word-break: auto-phrase`), as items ask.
    strict: RefCell<Vec<((Strictness, bool), Opportunities)>>,
}

/// Break opportunities that items share.
type Opportunities = std::rc::Rc<[fagus_text::BreakOpportunity]>;

/// How strictly `line-break` breaks lines, in the language of `style`: Chrome breaks Chinese
/// and Japanese text around more characters, and `auto` in Chinese like `normal`.
pub(crate) fn strictness(style: &ComputedStyle) -> Strictness {
    let language = style.lang.as_deref().map(|lang| {
        let primary = lang.split(['-', '_']).next().unwrap_or_default();
        (
            primary.eq_ignore_ascii_case("ja") || primary.eq_ignore_ascii_case("zh"),
            primary.eq_ignore_ascii_case("zh"),
        )
    });
    let (cjk, chinese) = language.unwrap_or_default();
    match style.line_break {
        LineBreak::Auto if chinese => Strictness::NormalCjk,
        LineBreak::Auto => Strictness::Auto,
        LineBreak::Loose if cjk => Strictness::LooseCjk,
        LineBreak::Loose => Strictness::Loose,
        LineBreak::Normal if cjk => Strictness::NormalCjk,
        LineBreak::Normal => Strictness::Normal,
        LineBreak::Strict => Strictness::Strict,
        LineBreak::Anywhere => Strictness::Anywhere,
    }
}

/// Whether lines of `style` break between the phrases of Japanese text (`word-break:
/// auto-phrase`): Chrome asks ICU for that in Japanese alone.
pub(crate) fn breaks_phrases(style: &ComputedStyle) -> bool {
    style.word_break == WordBreak::AutoPhrase
        && style.lang.as_deref().is_some_and(|lang| {
            lang.split(['-', '_'])
                .next()
                .is_some_and(|primary| primary.eq_ignore_ascii_case("ja"))
        })
}

/// The break opportunities of `text` with `line-break` at `strictness`, between phrases with
/// `phrases`.
fn opportunities_of(
    text: &str,
    (strictness, phrases): (Strictness, bool),
) -> Vec<fagus_text::BreakOpportunity> {
    if phrases {
        fagus_text::phrase_opportunities_with(text, strictness)
    } else {
        fagus_text::break_opportunities_with(text, strictness)
    }
}

impl Paragraph {
    pub fn new(tree: &BoxTree, items: &[InlineItem], generated: &[Option<String>]) -> Self {
        let mut text = String::new();
        let mut ranges = Vec::with_capacity(items.len());
        for (item, generated) in items.iter().zip(generated) {
            let start = text.len();
            match item {
                InlineItem::Text { text: own, .. } => text.push_str(own),
                InlineItem::PageNumber { .. }
                | InlineItem::TargetPage { .. }
                | InlineItem::NamedString { .. } => {
                    text.push_str(generated.as_deref().unwrap_or_default());
                }
                InlineItem::Atomic(id) if tree.ruby_columns.contains_key(id) => {
                    if let BoxKind::Inline { items } = &tree.get(*id).kind {
                        for item in items {
                            if let InlineItem::Text { text: own, .. } = item {
                                text.push_str(own);
                            }
                        }
                    }
                }
                InlineItem::Atomic(_) => text.push('\u{FFFC}'),
                InlineItem::LineBreak { .. } => text.push('\n'),
                _ => {}
            }
            ranges.push(start..text.len());
        }
        let opportunities = fagus_text::break_opportunities(&text);
        Self {
            text,
            ranges,
            opportunities,
            strict: RefCell::default(),
        }
    }

    /// The break opportunities of the whole text with `line-break` at `strictness`, between
    /// phrases or not.
    fn opportunities_with(&self, how: (Strictness, bool)) -> Opportunities {
        let mut strict = self.strict.borrow_mut();
        if let Some((_, found)) = strict.iter().find(|(value, _)| *value == how) {
            return std::rc::Rc::clone(found);
        }
        let found: Opportunities = opportunities_of(&self.text, how).into();
        strict.push((how, std::rc::Rc::clone(&found)));
        found
    }

    /// Where the last word of the paragraph starts, which is not hyphenated, unless it is the
    /// only one (as in Blink's `ShouldHyphenate`): after the last break opportunity before the
    /// spaces at the end of the text.
    pub fn last_word_start(&self) -> Option<usize> {
        let end = self.text.trim_end().len();
        let start = self
            .opportunities
            .iter()
            .map(|opportunity| opportunity.offset)
            .rfind(|&offset| offset < end)?;
        (!self.text[..start].trim().is_empty()).then_some(start)
    }

    /// Whether a line may break before byte `at` of the text.
    pub fn breaks_at(&self, at: usize) -> bool {
        self.opportunities
            .binary_search_by_key(&at, |opportunity| opportunity.offset)
            .is_ok()
    }

    /// The break opportunities inside the text of item `index` in `style`, from its start,
    /// ending with its end, and whether a line may break at its end: `word-break: keep-all`
    /// keeps letters together there, and `break-all` breaks between them at its end (inside,
    /// the line breaker finds those breaks where it needs them). For the min-content width
    /// (`measure`), Blink looks at the text of the item and the character after it alone.
    pub fn breaks(
        &self,
        index: usize,
        style: &ComputedStyle,
        measure: bool,
    ) -> (Vec<fagus_text::BreakOpportunity>, bool) {
        let range = self.ranges[index].clone();
        let strictness = strictness(style);
        let phrases = breaks_phrases(style) && strictness != Strictness::Anywhere;
        let local;
        let tailored;
        let (opportunities, base) = if measure {
            let end = self.text[range.end..]
                .chars()
                .next()
                .map_or(range.end, |next| range.end + next.len_utf8());
            local = opportunities_of(&self.text[range.start..end], (strictness, phrases));
            (&local[..], range.start)
        } else if strictness == Strictness::Auto && !phrases {
            (&self.opportunities[..], 0)
        } else {
            tailored = self.opportunities_with((strictness, phrases));
            (&tailored[..], 0)
        };
        let first = opportunities.partition_point(|opportunity| base + opportunity.offset <= range.start);
        let mut inside: Vec<fagus_text::BreakOpportunity> = opportunities[first..]
            .iter()
            .take_while(|opportunity| base + opportunity.offset < range.end)
            .map(|opportunity| fagus_text::BreakOpportunity {
                offset: base + opportunity.offset - range.start,
                mandatory: opportunity.mandatory,
            })
            .collect();
        let at_end = opportunities[first + inside.len()..]
            .first()
            .filter(|opportunity| base + opportunity.offset == range.end);
        let mandatory = at_end.is_some_and(|opportunity| opportunity.mandatory);
        let mut breaks_at_end = at_end.is_some();
        match style.word_break {
            // `line-break: anywhere` breaks between letters whatever `word-break` says.
            _ if strictness == Strictness::Anywhere => {}
            WordBreak::KeepAll => {
                inside.retain(|opportunity| {
                    opportunity.mandatory
                        || !fagus_text::keeps_all_at(&self.text, range.start + opportunity.offset)
                });
                breaks_at_end &= mandatory || !fagus_text::keeps_all_at(&self.text, range.end);
            }
            WordBreak::BreakAll if range.end < self.text.len() => {
                breaks_at_end |= fagus_text::breaks_all_at(&self.text, range.end);
            }
            _ => {}
        }
        // `white-space: break-spaces` breaks after every preserved space, also between spaces
        // (CSS Text 3, §3), as Blink's `kAfterEverySpace` does after other space separators too.
        if style.white_space.collapse == WhiteSpaceCollapse::BreakSpaces && style.white_space.wraps() {
            let own = &self.text[range.clone()];
            let before = inside.len();
            for (at, character) in own.char_indices() {
                if matches!(
                    character,
                    ' ' | '\t'
                        | '\u{1680}'
                        | '\u{2000}'..='\u{2006}'
                        | '\u{2008}'..='\u{200A}'
                        | '\u{205F}'
                        | '\u{3000}'
                ) {
                    let after = at + character.len_utf8();
                    if after < own.len() {
                        inside.push(fagus_text::BreakOpportunity {
                            offset: after,
                            mandatory: false,
                        });
                    } else {
                        breaks_at_end = true;
                    }
                }
            }
            if inside.len() > before {
                // A forced break where there is one at the same place.
                inside.sort_by_key(|opportunity| (opportunity.offset, !opportunity.mandatory));
                inside.dedup_by_key(|opportunity| opportunity.offset);
            }
        }
        inside.push(fagus_text::BreakOpportunity {
            offset: range.len(),
            mandatory,
        });
        (inside, breaks_at_end)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn text(text: &str, style: &Arc<ComputedStyle>) -> InlineItem {
        InlineItem::Text {
            text: text.to_owned(),
            style: Arc::clone(style),
        }
    }

    #[test]
    fn breaks_after_every_space_of_break_spaces() {
        let style = Arc::new(ComputedStyle::from(fagus_style::ComputedValues {
            white_space: fagus_style::values::WhiteSpace::BREAK_SPACES,
            ..fagus_style::ComputedValues::default()
        }));
        let items = [text(" ab  c\u{3000}\u{3000}d ", &style)];
        let paragraph = Paragraph::new(&BoxTree::default(), &items, &[None]);
        let (inside, at_end) = paragraph.breaks(0, &style, false);
        // After each space and ideographic space, also between them.
        assert_eq!(
            inside.iter().map(|b| b.offset).collect::<Vec<_>>(),
            [1, 4, 5, 9, 12, 14]
        );
        assert!(at_end);
    }

    #[test]
    fn finds_breaks_across_items() {
        let style = Arc::new(ComputedStyle::default());
        let items = [
            text("[a]", &style),
            InlineItem::Open {
                style: Arc::clone(&style),
                node: None,
            },
            text("[b] c", &style),
            InlineItem::Close,
            text("d", &style),
        ];
        let paragraph = Paragraph::new(&BoxTree::default(), &items, &[None, None, None, None, None]);
        assert_eq!(paragraph.text, "[a][b] cd");
        // Between the brackets, but not between "c" and "d".
        assert!(paragraph.breaks(0, &style, false).1);
        let (inside, at_end) = paragraph.breaks(2, &style, false);
        assert_eq!(inside.iter().map(|b| b.offset).collect::<Vec<_>>(), [4, 5]);
        assert!(!at_end);
        // With `keep-all`, letters stay together at the end too.
        let keep = Arc::new(ComputedStyle::from(fagus_style::ComputedValues {
            word_break: WordBreak::KeepAll,
            ..fagus_style::ComputedValues::default()
        }));
        let items = [text("日本", &keep), text("語", &style)];
        let paragraph = Paragraph::new(&BoxTree::default(), &items, &[None, None]);
        assert_eq!(
            paragraph.breaks(0, &keep, false),
            (
                vec![fagus_text::BreakOpportunity {
                    offset: 6,
                    mandatory: false
                }],
                false
            )
        );
    }
}

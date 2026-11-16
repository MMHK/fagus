//! Ruby (CSS Ruby 1), laid out as Chrome lays it out: each base with its annotation is a ruby
//! column, placed on the line like an atomic inline as wide as the wider of the two. The
//! narrower one is spread over that width by `ruby-align`, the annotation sits on the em box of
//! the base (over it, or under it by `ruby-position`), and an annotation wider than its base
//! may reach over the text beside the column by up to half its font size.

use std::sync::Arc;

use fagus_style::ComputedStyle;
use fagus_style::values::{AnnotationSide, Display, RubyAlign, RubyDisplay};
use fagus_text::FontDb;

use crate::block::{Atomic, Layouter};
use crate::emphasis::em_box;
use crate::fragment::{Children, Fragment, FragmentKind, VisibleSides};
use crate::inline::{intrinsic_widths, layout_lines, metrics};
use crate::tree::{BoxId, BoxTree, InlineItem};
use crate::units::{Au, Rect};

/// The annotation of a ruby column, whose base is the content of the column's box.
#[derive(Clone, Debug)]
pub(crate) struct RubyColumn {
    /// The style of the annotation (`rt`).
    pub style: Arc<ComputedStyle>,
    pub items: Vec<InlineItem>,
    /// The text items before and after the column that the annotation may reach over.
    pub overhang: (Option<InlineItem>, Option<InlineItem>),
}

impl BoxTree {
    /// Notes which ruby columns among the inline items of a box have text before or after them
    /// that a wider annotation may reach over, as Chrome lets it (Blink's
    /// `CanApplyStartOverhang` and `CommitPendingEndOverhang`): text no larger than the ruby's
    /// in whole pixels, without emphasis marks on the side of the annotation.
    pub(crate) fn link_ruby_columns(&mut self, items: &[InlineItem]) {
        if self.ruby_columns.is_empty() {
            return;
        }
        let is_edge = |item: &InlineItem| matches!(item, InlineItem::Open { .. } | InlineItem::Close);
        for (index, item) in items.iter().enumerate() {
            let InlineItem::Atomic(id) = item else {
                continue;
            };
            let ruby = &self.get(*id).style;
            let size = pixel_size(ruby.font_size);
            let side = ruby.ruby_position;
            let Some(column) = self.ruby_columns.get_mut(id) else {
                continue;
            };
            let beside = |item: Option<&InlineItem>| match item {
                Some(InlineItem::Text { text, style })
                    if !text.is_empty()
                        && pixel_size(style.font_size) <= size
                        && !(style.text_emphasis_style.is_some() && style.text_emphasis_position == side) =>
                {
                    item.cloned()
                }
                _ => None,
            };
            column.overhang = (
                beside(items[..index].iter().rev().find(|item| !is_edge(item))),
                beside(items[index + 1..].iter().find(|item| !is_edge(item))),
            );
        }
    }
}

/// A font size in whole pixels, rounded, as Blink's `ComputedStyle::FontSize` gives it.
fn pixel_size(font_size: f32) -> u32 {
    (font_size + 0.5).max(0.0) as u32
}

/// Whether a character is a CJK ideograph, kana or symbol, which justification may space out
/// on both sides (as Chrome's `IsCJKIdeographOrSymbol` tells).
pub(crate) fn is_cjk(character: char) -> bool {
    crate::cjk::is_cjk_ideograph_or_symbol(character)
}

/// Whether a fragment is the box of a ruby column.
fn is_column(fragment: &Fragment) -> bool {
    matches!(&fragment.kind, FragmentKind::Box { style, .. } if style.ruby_display == RubyDisplay::Column)
}

/// The text runs of a line fragment in order, with their x from the start of the line. The
/// bases of ruby columns nested in the line are text of the line, as in Chrome, which lays
/// their items out on it; their annotations are not.
fn runs(fragment: &Fragment, x: f32, out: &mut Vec<(f32, String)>) {
    let column = is_column(fragment);
    for child in &fragment.children {
        let left = x + child.rect.x.to_px();
        match &child.kind {
            FragmentKind::Text(run) => out.push((left, run.text.clone())),
            FragmentKind::Line { .. } if column => runs(child, left, out),
            FragmentKind::Box { .. } if column => {}
            FragmentKind::Box { .. } | FragmentKind::Anonymous => runs(child, left, out),
            _ => {}
        }
    }
}

/// Where justification may add space in a line: before and after each cluster of its text runs
/// (spaces have room after them, CJK characters before and after, but not twice between two),
/// in order. The last room is left out: lines are not spread past their last character.
fn opportunities(line: &Fragment) -> Vec<(bool, bool)> {
    let mut texts = Vec::new();
    runs(line, 0.0, &mut texts);
    let mut slots = Vec::new();
    let mut after_opportunity = true;
    for (_, text) in &texts {
        for character in text.chars() {
            let slot = if is_cjk(character) {
                let before = !after_opportunity;
                after_opportunity = true;
                (before, true)
            } else if character == ' ' || character == '\u{3000}' {
                after_opportunity = true;
                (false, true)
            } else {
                after_opportunity = false;
                (false, false)
            };
            slots.push(slot);
        }
    }
    if after_opportunity && let Some(last) = slots.iter_mut().rev().find(|slot| slot.0 || slot.1) {
        if last.1 {
            last.1 = false;
        } else {
            last.0 = false;
        }
    }
    slots
}

/// Spreads the text of a line by `each` at its opportunities: glyph advances grow, and what
/// follows moves along. Returns how far the content grew.
fn spread(fragment: &mut Fragment, slots: &[(bool, bool)], next: &mut usize, each: f32) -> f32 {
    let column = is_column(fragment);
    let mut moved = 0.0;
    for child in &mut fragment.children {
        // In a nested ruby column only the base takes part; its annotation stays.
        if column {
            if matches!(child.kind, FragmentKind::Line { .. }) {
                let grown = spread(child, slots, next, each);
                child.rect.width += Au::from_px(grown);
                moved += grown;
            }
            continue;
        }
        child.rect.x += Au::from_px(moved);
        match &mut child.kind {
            FragmentKind::Text(run) => {
                let mut grown = 0.0;
                let mut char_slots = Vec::new();
                for (offset, _) in run.text.char_indices() {
                    char_slots.push((offset as u32, slots.get(*next).copied().unwrap_or_default()));
                    *next += 1;
                }
                // Room before the first cluster moves the run itself.
                let mut lead = 0.0;
                let mut index = 0;
                while index < run.glyphs.len() {
                    let cluster = run.glyphs[index].cluster;
                    let last = run.glyphs[index..]
                        .iter()
                        .position(|glyph| glyph.cluster != cluster)
                        .map_or(run.glyphs.len(), |count| index + count)
                        - 1;
                    let (before, after) = char_slots
                        .iter()
                        .filter(|(offset, _)| (cluster.0..cluster.1.max(cluster.0 + 1)).contains(offset))
                        .fold((false, false), |(before, after), (_, slot)| {
                            (before || slot.0, after || slot.1)
                        });
                    if before {
                        if index == 0 {
                            lead += each;
                        } else {
                            run.glyphs[index - 1].advance += each;
                        }
                        grown += each;
                    }
                    if after {
                        run.glyphs[last].advance += each;
                        grown += each;
                    }
                    index = last + 1;
                }
                child.rect.x += Au::from_px(lead);
                child.rect.width += Au::from_px(grown - lead);
                moved += grown;
            }
            FragmentKind::Box { .. } | FragmentKind::Anonymous => {
                let grown = spread(child, slots, next, each);
                child.rect.width += Au::from_px(grown);
                moved += grown;
            }
            _ => {}
        }
    }
    moved
}

/// Spreads a line `width` wide over `space` more by `ruby-align`, as Chrome's `ApplyRubyAlign`
/// does, and returns how far its content starts from the start of the column. `annotation`
/// limits the room at the ends of an annotation to a full-width character (`font_size` twice).
fn align(line: &mut Fragment, space: f32, ruby_align: RubyAlign, annotation: Option<f32>) -> f32 {
    if space <= 0.0 {
        return 0.0;
    }
    match ruby_align {
        RubyAlign::Start => 0.0,
        RubyAlign::Center => space / 2.0,
        RubyAlign::SpaceAround | RubyAlign::SpaceBetween => {
            let slots = opportunities(line);
            let count = slots
                .iter()
                .map(|slot| usize::from(slot.0) + usize::from(slot.1))
                .sum::<usize>();
            if count == 0 {
                return space / 2.0;
            }
            let inset = if ruby_align == RubyAlign::SpaceBetween {
                0.0
            } else {
                let inset = space / (count + 1) as f32;
                annotation.map_or(inset, |font_size| inset.min(2.0 * font_size))
            };
            spread(line, &slots, &mut 0, (space - inset) / count as f32);
            inset / 2.0
        }
    }
}

/// Keeps the annotations of the ruby columns nested at the start of a line in place while the
/// line's content moves `by` along. Chrome adds the room before a ruby base or annotation as
/// leading expansion to its first glyph run or atomic inline (Blink's
/// `ApplyLeftAndRightExpansion`), inside the box of the nested column that run belongs to,
/// whose annotation stays at the column's start. Returns whether the line had such content.
fn keep_nested_annotations(fragment: &mut Fragment, by: f32) -> bool {
    for child in &mut fragment.children {
        match &child.kind {
            FragmentKind::Text(run) if run.glyphs.is_empty() => continue,
            FragmentKind::Text(_) => return true,
            FragmentKind::Box { .. } | FragmentKind::Anonymous => {}
            _ => continue,
        }
        if is_column(child) {
            for part in &mut child.children {
                if matches!(part.kind, FragmentKind::Line { .. }) {
                    keep_nested_annotations(part, by);
                } else {
                    part.rect.x -= Au::from_px(by);
                }
            }
            return true;
        }
        let inline = match &child.kind {
            FragmentKind::Box { style, .. } => crate::annotation::is_inline_box(style),
            _ => true,
        };
        if !inline || keep_nested_annotations(child, by) {
            return true;
        }
    }
    false
}

/// How far the start of a base `space` narrower than its column lies from the column's start:
/// what Chrome's `ComputeRubyBaseInset` gives, which bounds the overhang.
fn base_inset(line: &Fragment, space: f32) -> f32 {
    let count = opportunities(line)
        .iter()
        .map(|slot| usize::from(slot.0) + usize::from(slot.1))
        .sum::<usize>();
    if count == 0 {
        space / 2.0
    } else {
        space / (count + 1) as f32 / 2.0
    }
}

/// The em box of the text of a line, above and below its baseline: the em boxes of its text
/// runs (inside their ascent and descent, rounded up), or the line's ascent and descent where
/// it has no text.
fn em_extent(fonts: &FontDb, line: &Fragment, height: f32) -> (f32, f32) {
    fn walk(fonts: &FontDb, fragment: &Fragment, y: f32, extent: &mut Option<(f32, f32)>) {
        for child in &fragment.children {
            let top = y + child.rect.y.to_px();
            match &child.kind {
                FragmentKind::Text(run) => {
                    let primary = metrics(fonts, &run.style);
                    // In a vertical line the em box is centered on the line's central baseline,
                    // where the run's ascent ends; the glyphs hang from their alphabetic one.
                    let (ascent, descent, baseline) = if crate::inline::central_baseline(&run.style) {
                        (run.size / 2.0, run.size / 2.0, top + primary.ascent)
                    } else {
                        let (ascent, descent) = em_box(fonts, run.face, run.size);
                        (ascent, descent, top + run.baseline.to_px())
                    };
                    let (over, under) = (
                        baseline - ascent.ceil().min(primary.ascent),
                        baseline + descent.ceil().min(primary.descent),
                    );
                    *extent = Some(
                        extent.map_or((over, under), |(top, bottom)| (top.min(over), bottom.max(under))),
                    );
                }
                // The base of a nested ruby column is text of the line; its annotation is not.
                FragmentKind::Box { .. } if is_column(child) => {
                    for base in &child.children {
                        if matches!(base.kind, FragmentKind::Line { .. }) {
                            walk(fonts, base, top + base.rect.y.to_px(), extent);
                        }
                    }
                }
                FragmentKind::Box { .. } | FragmentKind::Anonymous => walk(fonts, child, top, extent),
                _ => {}
            }
        }
    }
    let FragmentKind::Line { baseline } = line.kind else {
        return (0.0, 0.0);
    };
    let baseline = baseline.to_px();
    let mut extent = None;
    walk(fonts, line, 0.0, &mut extent);
    let (top, bottom) = extent.unwrap_or((0.0, height));
    (baseline - top, bottom - baseline)
}

/// The baseline of a line fragment, from its top.
fn baseline_of(line: &Fragment) -> f32 {
    match line.kind {
        FragmentKind::Line { baseline } => baseline.to_px(),
        _ => 0.0,
    }
}

/// A line of inline content laid out on its own, as wide as its content: the line, its height
/// and width.
fn line_of(
    layouter: &Layouter<'_>,
    style: &ComputedStyle,
    items: &[InlineItem],
) -> Option<(Fragment, f32, f32)> {
    let width = intrinsic_widths(layouter, items).1;
    let mut lines = layout_lines(layouter, style, items, Au::from_px_ceil(width) + Au::EPSILON);
    let line = lines.drain(..).find(|line| !line.fragment.children.is_empty())?;
    Some((line.fragment, line.height.to_px(), width))
}

/// The widths of a ruby column: as wide as the wider of base and annotation, less what the
/// annotation reaches over the text beside it. `lead` is the space of `text-autospace` before
/// its base.
pub(crate) fn intrinsic(layouter: &Layouter<'_>, id: BoxId, column: &RubyColumn, lead: f32) -> Au {
    let layout_box = layouter.tree.get(id);
    let crate::tree::BoxKind::Inline { items } = &layout_box.kind else {
        return Au::ZERO;
    };
    Au::from_px_ceil(laid_out(layouter, &layout_box.style, items, column, lead).width)
}

/// The first and the last letter of the base of ruby column `id` (not marks), which take part
/// in `text-autospace` with the text around the column.
pub(crate) fn base_ends(tree: &BoxTree, id: BoxId) -> Option<(char, char)> {
    let crate::tree::BoxKind::Inline { items } = &tree.get(id).kind else {
        return None;
    };
    let mut letters = items
        .iter()
        .filter_map(|item| match item {
            InlineItem::Text { text, .. } => Some(text.chars()),
            _ => None,
        })
        .flatten()
        .filter(|character| {
            !matches!(
                unicode_properties::UnicodeGeneralCategory::general_category_group(*character),
                unicode_properties::GeneralCategoryGroup::Mark
            )
        });
    let first = letters.next()?;
    Some((first, letters.next_back().unwrap_or(first)))
}

/// The inline items of a ruby annotation as Chrome sets them: without `text-autospace`, which
/// leaves out the text of annotations (Blink's `MaskRubyAnnotations`).
pub(crate) fn annotation_items(items: Vec<InlineItem>) -> Vec<InlineItem> {
    let mut styles: Vec<(Arc<ComputedStyle>, Arc<ComputedStyle>)> = Vec::new();
    let mut plain = |style: &mut Arc<ComputedStyle>| {
        if !style.text_autospace {
            return;
        }
        if let Some((_, done)) = styles.iter().find(|(own, _)| Arc::ptr_eq(own, style)) {
            *style = Arc::clone(done);
            return;
        }
        let done = Arc::new(ComputedStyle {
            text_autospace: false,
            ..(**style).clone()
        });
        styles.push((Arc::clone(style), Arc::clone(&done)));
        *style = done;
    };
    items
        .into_iter()
        .map(|mut item| {
            match &mut item {
                InlineItem::Text { style, .. }
                | InlineItem::LineBreak { style }
                | InlineItem::Open { style, .. } => plain(style),
                _ => {}
            }
            item
        })
        .collect()
}

/// A ruby column laid out: its fragment's children, its width and the baseline and height of
/// its base.
struct Laid {
    children: Vec<Fragment>,
    width: f32,
    baseline: f32,
    height: f32,
}

fn laid_out(
    layouter: &Layouter<'_>,
    style: &ComputedStyle,
    items: &[InlineItem],
    column: &RubyColumn,
    lead: f32,
) -> Laid {
    let fonts = layouter.fonts;
    let strut = metrics(fonts, style);
    // An empty base is a line of the strut alone, whose em box is that of the first font.
    let line = line_of(layouter, style, items);
    let empty = line.is_none();
    // The space of `text-autospace` before the base belongs to it.
    let line = line.map(|(mut base, height, width)| {
        if lead > 0.0 {
            for child in &mut base.children {
                child.rect.x += Au::from_px(lead);
            }
        }
        (base, height, width + lead)
    });
    let (mut base, base_height, base_width) = line.unwrap_or_else(|| {
        let leading = crate::inline::half_leading(strut.line_height - strut.ascent - strut.descent);
        let line = Fragment {
            rect: Rect::default(),
            kind: FragmentKind::Line {
                baseline: Au::from_px(leading + strut.ascent),
            },
            children: Children::new(),
        };
        (line, strut.line_height, 0.0)
    });
    let baseline = baseline_of(&base);
    let Some((mut annotation, annotation_height, annotation_width)) =
        line_of(layouter, &column.style, &column.items)
    else {
        return Laid {
            width: base_width,
            baseline,
            height: base_height,
            children: vec![base],
        };
    };
    let width = base_width.max(annotation_width);
    let ruby_align = style.ruby_align;
    let base_space = width - base_width;
    let inset = base_inset(&base, base_space);
    let base_x = align(&mut base, base_space, ruby_align, None);
    let annotation_x = align(
        &mut annotation,
        width - annotation_width,
        ruby_align,
        Some(column.style.font_size),
    );
    keep_nested_annotations(&mut base, base_x);
    keep_nested_annotations(&mut annotation, annotation_x);
    // The box of the annotation spans the column, as Chrome's ruby text box does.
    for child in &mut annotation.children {
        if let FragmentKind::Box { style, .. } = &child.kind
            && style.ruby_display == RubyDisplay::Text
        {
            let shift = child.rect.x + Au::from_px(annotation_x);
            child.rect.x -= shift;
            child.rect.width = Au::from_px_ceil(width);
            for grandchild in &mut child.children {
                grandchild.rect.x += shift;
            }
        }
    }
    // A wider annotation reaches over the text beside it, by up to half its font size: half
    // its size in whole pixels, rounded down (Blink halves `ComputedStyle::FontSize`).
    let half = (pixel_size(column.style.font_size) / 2) as f32;
    let (mut start, mut end) = match ruby_align {
        _ if base_space <= 0.0 || empty => (0.0, 0.0),
        RubyAlign::SpaceBetween => (0.0, 0.0),
        RubyAlign::Start => (0.0, base_space.min(half)),
        RubyAlign::SpaceAround | RubyAlign::Center => (inset.min(half), inset.min(half)),
    };
    // It reaches over no more than half the text beside it.
    let limit = |beside: &Option<InlineItem>| {
        beside.as_ref().map_or(0.0, |item| {
            (Au::from_px(intrinsic_widths(layouter, std::slice::from_ref(item)).1) / 2).to_px()
        })
    };
    if start > 0.0 {
        start = start.min(limit(&column.overhang.0));
    }
    if end > 0.0 {
        end = end.min(limit(&column.overhang.1));
    }
    let (base_over, base_under) = if empty {
        crate::inline::face_for(fonts, style).map_or((strut.ascent, strut.descent), |face| {
            let (ascent, descent) = em_box(fonts, face, crate::inline::used_font_size(fonts, style));
            (ascent.ceil().min(strut.ascent), descent.ceil().min(strut.descent))
        })
    } else {
        em_extent(fonts, &base, base_height)
    };
    let (annotation_over, annotation_under) = em_extent(fonts, &annotation, annotation_height);
    let annotation_baseline = baseline_of(&annotation);
    // The annotation's baseline, from the top of the column.
    let annotation_at = match style.ruby_position {
        AnnotationSide::Over => baseline - base_over - annotation_under,
        AnnotationSide::Under => baseline + base_under + annotation_over,
    };
    let top = annotation_at - annotation_over;
    base.rect.x = Au::from_px(base_x - start);
    annotation.rect.x = Au::from_px(annotation_x);
    annotation.rect.y = Au::from_px(annotation_at - annotation_baseline - top);
    let wrapper = Fragment {
        rect: Rect::new(
            Au::from_px(-start),
            Au::from_px(top),
            Au::from_px(width),
            Au::from_px(annotation_over + annotation_under),
        ),
        kind: FragmentKind::Box {
            style: Arc::new(ComputedStyle {
                display: Display::BLOCK,
                ruby_display: RubyDisplay::Annotation,
                // The side is the ruby's, as the annotation goes there whatever its own says.
                ruby_position: style.ruby_position,
                ..column.style.inherit()
            }),
            node: None,
            sides: VisibleSides::ALL,
        },
        children: vec![annotation].into(),
    };
    Laid {
        children: vec![wrapper, base],
        width: width - start - end,
        baseline,
        height: base_height,
    }
}

/// An annotation of a ruby column in a line, found by [`share_annotation_baselines`].
struct Found {
    /// The path of child indexes from the line to the annotation's box.
    path: Vec<usize>,
    side: AnnotationSide,
    /// Its level: 1 for annotations next to text, one more for each nested ruby column whose
    /// annotations on the same side lie between them and the text.
    level: u32,
    /// The baseline of its line, from the top of the line, and its extent over and under it.
    baseline: f32,
    over: f32,
    under: f32,
}

/// Puts the annotations of the ruby columns of a line on shared baselines over and under the
/// line, as Chrome does (Blink's `RubyBlockPositionCalculator`, which places the annotations of a
/// line as lines of annotations): those next to the text over its highest em box and under its
/// lowest, and those of ruby columns around nested ones (double-sided or stacked ruby) a level
/// further out, beyond the annotations of the columns they hold.
pub(crate) fn share_annotation_baselines(line: &mut Fragment) {
    /// The annotation boxes of the columns among the children of `fragment` (`y` below the top
    /// of the line) into `out`. Returns how many levels of annotations over and under they
    /// stack, as Blink's `HandleRubyLine` counts them: a column's annotation goes one level
    /// beyond those of the columns nested in its base.
    fn find(fragment: &Fragment, y: f32, path: &mut Vec<usize>, out: &mut Vec<Found>) -> (u32, u32) {
        let mut depth = (0, 0);
        for (index, child) in fragment.children.iter().enumerate() {
            let top = y + child.rect.y.to_px();
            path.push(index);
            let FragmentKind::Box { style, .. } = &child.kind else {
                if matches!(child.kind, FragmentKind::Anonymous) {
                    let inner = find(child, top, path, out);
                    depth = (depth.0.max(inner.0), depth.1.max(inner.1));
                }
                path.pop();
                continue;
            };
            if style.ruby_display == RubyDisplay::Column {
                let mut nested = (0, 0);
                for (inner, base) in child.children.iter().enumerate() {
                    if matches!(base.kind, FragmentKind::Line { .. }) {
                        path.push(inner);
                        let levels = find(base, top + base.rect.y.to_px(), path, out);
                        nested = (nested.0.max(levels.0), nested.1.max(levels.1));
                        path.pop();
                    }
                }
                let mut own = nested;
                for (inner, wrapper) in child.children.iter().enumerate() {
                    if let FragmentKind::Box { style, .. } = &wrapper.kind
                        && style.ruby_display == RubyDisplay::Annotation
                        && let Some(annotation) = wrapper.children.first()
                    {
                        let over = annotation.rect.y.to_px() + baseline_of(annotation);
                        let level = match style.ruby_position {
                            AnnotationSide::Over => {
                                own.0 = nested.0 + 1;
                                own.0
                            }
                            AnnotationSide::Under => {
                                own.1 = nested.1 + 1;
                                own.1
                            }
                        };
                        let mut at = path.clone();
                        at.push(inner);
                        out.push(Found {
                            path: at,
                            side: style.ruby_position,
                            level,
                            baseline: top + wrapper.rect.y.to_px() + over,
                            over,
                            under: wrapper.rect.height.to_px() - over,
                        });
                    }
                }
                depth = (depth.0.max(own.0), depth.1.max(own.1));
            } else if crate::annotation::is_inline_box(style) {
                let inner = find(child, top, path, out);
                depth = (depth.0.max(inner.0), depth.1.max(inner.1));
            }
            path.pop();
        }
        depth
    }
    let mut annotations = Vec::new();
    let depth = find(line, 0.0, &mut Vec::new(), &mut annotations);
    if annotations.len() < 2 {
        return;
    }
    // The baseline of each level on each side: the first over the highest (under the lowest)
    // baseline its annotations would have alone, each further one beyond the one before.
    let targets = |side: AnnotationSide, levels: u32| {
        let of = |level: u32| {
            annotations
                .iter()
                .filter(move |found| found.side == side && found.level == level)
        };
        let mut targets: Vec<f32> = Vec::new();
        for level in 1..=levels {
            let target = match (side, targets.last()) {
                (AnnotationSide::Over, None) => of(level)
                    .map(|found| found.baseline)
                    .fold(f32::INFINITY, f32::min),
                (AnnotationSide::Under, None) => of(level)
                    .map(|found| found.baseline)
                    .fold(f32::NEG_INFINITY, f32::max),
                (AnnotationSide::Over, Some(before)) => {
                    before
                        - of(level - 1).map(|found| found.over).fold(0.0, f32::max)
                        - of(level).map(|found| found.under).fold(0.0, f32::max)
                }
                (AnnotationSide::Under, Some(before)) => {
                    before
                        + of(level - 1).map(|found| found.under).fold(0.0, f32::max)
                        + of(level).map(|found| found.over).fold(0.0, f32::max)
                }
            };
            targets.push(target);
        }
        targets
    };
    let (over, under) = (
        targets(AnnotationSide::Over, depth.0),
        targets(AnnotationSide::Under, depth.1),
    );
    for found in &annotations {
        let targets = match found.side {
            AnnotationSide::Over => &over,
            AnnotationSide::Under => &under,
        };
        let Some(target) = targets.get(found.level as usize - 1) else {
            continue;
        };
        let shift = target - found.baseline;
        if shift.abs() < 0.001 {
            continue;
        }
        let mut wrapper = &mut *line;
        for index in &found.path {
            wrapper = &mut wrapper.children[*index];
        }
        wrapper.rect.y += Au::from_px(shift);
    }
}

/// Lays out a ruby column as an atomic inline, with `lead` of `text-autospace` before its base.
pub(crate) fn layout(layouter: &Layouter<'_>, id: BoxId, column: &RubyColumn, lead: f32) -> Atomic {
    let layout_box = layouter.tree.get(id);
    let items: &[InlineItem] = match &layout_box.kind {
        crate::tree::BoxKind::Inline { items } => items,
        _ => &[],
    };
    layout_with(layouter, id, items, column, lead)
}

/// Lays out a ruby column with the base `items` as an atomic inline.
fn layout_with(
    layouter: &Layouter<'_>,
    id: BoxId,
    items: &[InlineItem],
    column: &RubyColumn,
    lead: f32,
) -> Atomic {
    let layout_box = layouter.tree.get(id);
    let laid = laid_out(layouter, &layout_box.style, items, column, lead);
    let (width, height) = (Au::from_px_ceil(laid.width), Au::from_px(laid.height));
    Atomic {
        fragment: Fragment {
            rect: Rect::new(Au::ZERO, Au::ZERO, width, height),
            kind: FragmentKind::Box {
                style: Arc::clone(&layout_box.style),
                node: None,
                sides: VisibleSides::ALL,
            },
            children: laid.children.into(),
        },
        width,
        height,
        baseline: Au::from_px(laid.baseline),
    }
}

/// A ruby column on a line, or the part of one that a line holds where it breaks across lines:
/// the content of its base and its annotation.
#[derive(Clone, Debug)]
pub(crate) struct Part {
    id: BoxId,
    base: Vec<InlineItem>,
    annotation: RubyColumn,
    /// The space of `text-autospace` before the base, which stays with its first part.
    pub(crate) lead: f32,
}

impl Part {
    /// The ruby column of box `id`, whole, if it is one.
    pub(crate) fn of(tree: &BoxTree, id: BoxId) -> Option<Self> {
        let column = tree.ruby_columns.get(&id)?;
        let crate::tree::BoxKind::Inline { items } = &tree.get(id).kind else {
            return None;
        };
        Some(Self {
            id,
            base: items.clone(),
            annotation: column.clone(),
            lead: 0.0,
        })
    }

    /// Whether the column may break across lines, as Chrome lets it (Blink's
    /// `IsMonolithicRuby`): where its base has more than four letters or its annotation more
    /// than eight.
    pub(crate) fn breaks(&self) -> bool {
        let letters = |items: &[InlineItem]| text_of(items).chars().filter(|c| !c.is_whitespace()).count();
        letters(&self.base) > 4 || letters(&self.annotation.items) > 8
    }

    /// Breaks the column where a line has `room` left, as Chrome does (Blink's `HandleRuby`):
    /// the base breaks at the last of its break opportunities where it takes no more than its
    /// share of the room (by the width of the base to that of the column), at least at the first
    /// one, and the annotation likewise by its share. The part that goes on the line and the
    /// rest, each laid out; `None` where the base cannot break.
    pub(crate) fn split(&self, layouter: &Layouter<'_>, room: f32) -> Option<[(Atomic, Self); 2]> {
        // The space of `text-autospace` before the base is part of it, as in Chrome.
        let lead = self.lead;
        let base_width = intrinsic_widths(layouter, &self.base).1 + lead;
        let annotation_width = intrinsic_widths(layouter, &self.annotation.items).1;
        let size = base_width.max(annotation_width);
        if size <= 0.0 {
            return None;
        }
        let room = room.max(0.0);
        let annotation_at = |width: f32| {
            split_at_width(layouter, &self.annotation.items, width)
                .unwrap_or_else(|| (self.annotation.items.clone(), Vec::new()))
        };
        let (mut base, mut base_rest) =
            split_at_width(layouter, &self.base, room * base_width / size - lead)?;
        let (mut annotation, mut annotation_rest) = annotation_at(room * annotation_width / size);
        // A part wider than the room is tried again with a shorter base, and the annotation
        // breaks by the share of the base that part has (Blink's retry in `HandleOverflow`).
        let width = |items: &[InlineItem]| intrinsic_widths(layouter, items).1;
        let base_part = width(&base) + lead;
        if base_part.max(width(&annotation)) > room + 0.01 {
            (base, base_rest) = split_at_width(layouter, &self.base, base_part - 1.0 - lead)?;
            (annotation, annotation_rest) =
                annotation_at(annotation_width * (width(&base) + lead) / base_width.max(0.01));
        }
        let part = |base: Vec<InlineItem>,
                    items: Vec<InlineItem>,
                    overhang: (Option<InlineItem>, Option<InlineItem>),
                    lead: f32| {
            let part = Self {
                id: self.id,
                base,
                annotation: RubyColumn {
                    items,
                    overhang,
                    ..self.annotation.clone()
                },
                lead,
            };
            (
                layout_with(layouter, self.id, &part.base, &part.annotation, lead),
                part,
            )
        };
        let (before, after) = self.annotation.overhang.clone();
        Some([
            part(base, annotation, (before, None), self.lead),
            part(base_rest, annotation_rest, (None, after), 0.0),
        ])
    }
}

/// The text of inline items.
fn text_of(items: &[InlineItem]) -> String {
    items
        .iter()
        .filter_map(|item| match item {
            InlineItem::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

/// Splits inline items at the last break opportunity of their text before which they are no
/// wider than `width`, or at the first one where none is: the items before and after, with
/// the inline boxes open there closed in the first part and opened again in the second.
/// `None` where the text has no break opportunity inside.
fn split_at_width(
    layouter: &Layouter<'_>,
    items: &[InlineItem],
    width: f32,
) -> Option<(Vec<InlineItem>, Vec<InlineItem>)> {
    let text = text_of(items);
    let mut best = None;
    for opportunity in fagus_text::break_opportunities(&text) {
        if opportunity.offset >= text.len() {
            break;
        }
        let mut parts = split_items(items, opportunity.offset);
        // The spaces the first part ends with hang at the end of its line: they are not drawn.
        if let Some(InlineItem::Text { text, style }) = parts
            .0
            .iter_mut()
            .rev()
            .find(|item| matches!(item, InlineItem::Text { .. }))
            && style.white_space.collapses_spaces()
        {
            text.truncate(text.trim_end_matches(' ').len());
        }
        let fits = intrinsic_widths(layouter, &parts.0).1 <= width + 0.01;
        if !fits && best.is_some() {
            break;
        }
        best = Some(parts);
        if !fits {
            break;
        }
    }
    best
}

/// Splits inline items at byte `at` of their text.
fn split_items(items: &[InlineItem], at: usize) -> (Vec<InlineItem>, Vec<InlineItem>) {
    let (mut first, mut rest) = (Vec::new(), Vec::new());
    let mut open = Vec::new();
    let mut offset = 0;
    for item in items {
        if offset >= at {
            rest.push(item.clone());
            continue;
        }
        match item {
            InlineItem::Text { text, style } if offset + text.len() > at => {
                let (before, after) = text.split_at(at - offset);
                first.push(InlineItem::Text {
                    text: before.to_owned(),
                    style: Arc::clone(style),
                });
                first.extend(open.iter().map(|_| InlineItem::Close));
                rest.extend(open.iter().cloned());
                rest.push(InlineItem::Text {
                    text: after.to_owned(),
                    style: Arc::clone(style),
                });
                offset = at;
                continue;
            }
            InlineItem::Text { text, .. } => offset += text.len(),
            InlineItem::Open { .. } => open.push(item.clone()),
            InlineItem::Close => {
                open.pop();
            }
            _ => {}
        }
        first.push(item.clone());
        if offset == at {
            first.extend(open.iter().map(|_| InlineItem::Close));
            rest.extend(open.iter().cloned());
        }
    }
    (first, rest)
}

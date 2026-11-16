//! The room that emphasis marks and ruby annotations take between lines, as Chrome gives it
//! (`ComputeAnnotationOverflow` in Blink): a line moves down by as far as its annotations
//! reach over its line box, less the room the line before leaves under its text (or, for the
//! first line, the block's top margin and padding), and what reaches under a line pushes the
//! next one down, less the room that one leaves over its text.

use fagus_style::ComputedStyle;
use fagus_style::values::{AnnotationSide, Display, DisplayInside, DisplayOutside, RubyDisplay};
use fagus_text::FontDb;

use crate::emphasis;
use crate::fragment::{Fragment, FragmentKind};
use crate::inline::Line;
use crate::units::Au;

/// How far the content of a line reaches over the top and under the bottom of its line box:
/// positive where its annotations reach past them, negative for the room it leaves.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Reach {
    pub over: f32,
    pub under: f32,
}

/// The extent of the content of a line, from the top of its line box, while it is measured.
struct Extent {
    over: f32,
    under: f32,
    /// Whether annotations reach over and under the text.
    annotated: (bool, bool),
}

impl Extent {
    fn add(&mut self, over: f32, under: f32) {
        self.over = self.over.min(over);
        self.under = self.under.max(under);
    }
}

/// Whether a fragment is an inline box (not an atomic inline), whose content belongs to the line.
pub(crate) fn is_inline_box(style: &ComputedStyle) -> bool {
    matches!(
        style.display,
        Display::Normal {
            outside: DisplayOutside::Inline,
            inside: DisplayInside::Flow,
            ..
        }
    )
}

/// Adds the text runs of a fragment's children (at `y` below the top of the line) to `extent`.
/// In the base of a ruby column, `annotation` is the side of its annotation and how far that
/// reaches (from the top of the line).
fn measure(
    fonts: &FontDb,
    fragment: &Fragment,
    y: f32,
    annotation: Option<(AnnotationSide, f32)>,
    extent: &mut Extent,
) {
    for child in &fragment.children {
        let top = y + child.rect.y.to_px();
        match &child.kind {
            FragmentKind::Text(run) => {
                let primary = crate::inline::metrics(fonts, &run.style);
                let (inset_over, inset_under) = emphasis::em_insets(fonts, run.face, run.size, &primary);
                let bottom = top + child.rect.height.to_px();
                let (mut over, mut under) = (top + inset_over, bottom - inset_under);
                let baseline = top + run.baseline.to_px();
                let edge = annotation
                    .filter(|(side, _)| *side == run.style.text_emphasis_position)
                    .map(|(_, edge)| (edge - baseline).abs());
                if let Some((side, reach)) = emphasis::reach(fonts, run, edge) {
                    match side {
                        AnnotationSide::Over => {
                            over = top - reach;
                            extent.annotated.0 = true;
                        }
                        AnnotationSide::Under => {
                            under = bottom + reach;
                            extent.annotated.1 = true;
                        }
                    }
                }
                extent.add(over, under);
            }
            // The annotation of a ruby column reaches as far as its em box.
            FragmentKind::Box { style, .. } if style.ruby_display == RubyDisplay::Annotation => {
                let bottom = top + child.rect.height.to_px();
                extent.add(top, bottom);
                nested_annotations(child, top, extent);
                match style.ruby_position {
                    AnnotationSide::Over => extent.annotated.0 = true,
                    AnnotationSide::Under => extent.annotated.1 = true,
                }
            }
            FragmentKind::Box { style, .. } if style.ruby_display == RubyDisplay::Column => {
                let annotation = child.children.iter().find_map(|wrapper| match &wrapper.kind {
                    FragmentKind::Box { style, .. } if style.ruby_display == RubyDisplay::Annotation => {
                        let edge = top + wrapper.rect.y.to_px();
                        Some(match style.ruby_position {
                            AnnotationSide::Over => (AnnotationSide::Over, edge),
                            AnnotationSide::Under => {
                                (AnnotationSide::Under, edge + wrapper.rect.height.to_px())
                            }
                        })
                    }
                    _ => None,
                });
                measure(fonts, child, top, annotation, extent);
            }
            FragmentKind::Box { style, .. } if is_inline_box(style) => {
                measure(fonts, child, top, annotation, extent);
            }
            FragmentKind::Anonymous | FragmentKind::Line { .. } => {
                measure(fonts, child, top, annotation, extent);
            }
            // An atomic inline reaches as far as its box, as Chrome counts every item of the
            // line (WPT css-break ruby-000).
            FragmentKind::Box { style, .. } if !style.is_out_of_flow() && !style.is_floating() => {
                extent.add(top, top + child.rect.height.to_px());
            }
            FragmentKind::Image(_) => extent.add(top, top + child.rect.height.to_px()),
            _ => {}
        }
    }
}

/// Adds the annotations of the ruby columns nested in an annotation (at `y` below the top of the
/// line) to `extent`: they reach beyond it, as Chrome counts them into its height.
fn nested_annotations(fragment: &Fragment, y: f32, extent: &mut Extent) {
    for child in &fragment.children {
        let top = y + child.rect.y.to_px();
        match &child.kind {
            FragmentKind::Box { style, .. } if style.ruby_display == RubyDisplay::Annotation => {
                extent.add(top, top + child.rect.height.to_px());
                nested_annotations(child, top, extent);
            }
            FragmentKind::Box { style, .. }
                if style.ruby_display == RubyDisplay::Column || is_inline_box(style) =>
            {
                nested_annotations(child, top, extent);
            }
            FragmentKind::Anonymous | FragmentKind::Line { .. } => nested_annotations(child, top, extent),
            _ => {}
        }
    }
}

/// How far the content of a line box `height` high reaches past it, in a block of font size
/// `font_size`, and whether it has annotations. A line without them reaches past it nowhere,
/// but leaves room for the annotations of the lines beside it.
pub(crate) fn reach(fonts: &FontDb, line: &Fragment, height: f32, font_size: f32) -> Option<(Reach, bool)> {
    let FragmentKind::Line { baseline } = line.kind else {
        return None;
    };
    let baseline = baseline.to_px();
    let mut extent = Extent {
        over: baseline,
        under: baseline,
        annotated: (false, false),
    };
    measure(fonts, line, 0.0, None, &mut extent);
    // A line with hardly any content keeps room for the font size.
    if extent.under - extent.over < font_size {
        let half_leading = ((height - font_size) / 2.0).max(0.0);
        extent.over = half_leading;
        extent.under = height - half_leading;
    }
    // Text taller than the line height reaches past it only with annotations.
    if extent.over < 0.0 && !extent.annotated.0 {
        extent.over = 0.0;
    }
    if extent.under > height && !extent.annotated.1 {
        extent.under = height;
    }
    Some((
        Reach {
            over: -extent.over,
            under: extent.under - height,
        },
        extent.annotated != (false, false),
    ))
}

/// Moves lines apart where their annotations need room while they are laid out, as Chrome
/// does (Blink's `SetAnnotationOverflow`): a line moves down by as far as its annotations reach
/// over its line box, less the room the line before leaves under its text (or, for the first
/// line, the room over it), and what reaches under a line moves the lines after it down, which
/// then go beside the floats there.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Spacer {
    /// The room under the line before (positive) or how far it reaches past it (negative).
    before: f32,
    /// The font size of the block, below which a line keeps room for the font size.
    font_size: f32,
    /// Whether the next line is the first, trimmed by `text-box-trim`: it does not move, as
    /// trimming cancels the room for annotations (Blink's `ApplyTextBoxTrim`).
    trimmed: bool,
    /// The lines that an initial letter raised above its first line still puts before the text:
    /// their height is room for the annotations of that line, which the letter may cover (Blink's
    /// `ComputeAnnotationBlockOffsetAdjustment`).
    raised: u32,
}

impl Spacer {
    /// Spaces the lines of a block with `room` over the first line (its top padding, with its top
    /// margin where it has no top border) and text of `font_size`; `trimmed` where
    /// `text-box-trim` trims the start of the first line.
    pub(crate) fn new(room: f32, font_size: f32, trimmed: bool) -> Self {
        Self {
            before: room,
            font_size,
            trimmed,
            raised: 0,
        }
    }

    /// Notes that an initial letter raised by `lines` lines starts on the next line: that line
    /// and the empty lines after it are room for the annotations of the text line after them.
    pub(crate) fn raise(&mut self, lines: u32) {
        self.raised = lines;
    }

    /// Whether the next line is one of those an initial letter raises above its text.
    pub(crate) fn raising(&self) -> bool {
        self.raised > 0
    }

    /// Places a line `height` high: returns how far it moves down and how far the lines after it
    /// move down besides (up, for the lines a raised initial letter makes where the first line
    /// is trimmed).
    pub(crate) fn place(&mut self, fonts: &FontDb, line: &Fragment, height: f32) -> (f32, f32) {
        let reach = if self.raised > 0 {
            Reach::default()
        } else {
            reach(fonts, line, height, self.font_size)
                .map(|(reach, _)| reach)
                .unwrap_or_default()
        };
        self.shift((reach.over, reach.under), height)
    }

    /// [`Spacer::place`] for a line whose content reaches `over` and `under` past it.
    fn shift(&mut self, (over, under): (f32, f32), height: f32) -> (f32, f32) {
        if self.raised > 0 {
            self.raised -= 1;
            // Trimming the first line cancels the room the letter makes over it: the text
            // goes up to where the first line is (Blink's `ApplyTextBoxTrim`).
            if self.trimmed {
                return (0.0, -height);
            }
            self.before = self.before.max(0.0) + height;
            return (0.0, 0.0);
        }
        let reach = Reach { over, under };
        let (overflow, space) = (reach.over.max(0.0), (-reach.over).max(0.0));
        let mut shift = overflow;
        if self.before < 0.0 && space > 0.0 {
            shift = -space.min(-self.before);
        }
        if overflow > 0.0 && self.before > 0.0 {
            shift = (overflow - self.before).max(0.0);
        }
        if std::mem::take(&mut self.trimmed) {
            shift = 0.0;
        }
        let (overflow, space) = (reach.under.max(0.0), (-reach.under).max(0.0));
        if overflow > 0.0 {
            self.before = -overflow;
            (shift, overflow)
        } else {
            self.before = space;
            (shift, 0.0)
        }
    }
}

/// Ends the lines of a block, spaced by [`Spacer`] as they were laid out: the annotations of
/// the last line reach past it into the block's bottom padding (`padding_bottom`), and the
/// block grows by the rest. Returns the room the last line leaves under its text, into which
/// the annotations of the next block may reach.
pub(crate) fn end_lines(
    fonts: &FontDb,
    lines: &mut [Line],
    font_size: f32,
    padding_bottom: f32,
) -> Option<f32> {
    let line = lines.last_mut()?;
    let (reach, _) = reach(fonts, &line.fragment, line.height.to_px(), font_size)?;
    let overflow = reach.under.max(0.0);
    if overflow > 0.0 {
        line.height += Au::from_px(overflow - padding_bottom.min(overflow));
    }
    Some((-reach.under).max(0.0))
}

/// Turns the room the last line of a box leaves under its text (`room`, set by
/// [`end_lines`]) into the room the box leaves for the annotations of the block after it, as
/// Chrome computes it (`BlockLayoutAlgorithm`, `SetBlockEndAnnotationSpace`): with what the box
/// is taller than its lines (`extra`) and its bottom padding, none past a bottom border, and,
/// as Chrome counts the box's top border and padding (`top`) twice, less them.
pub(crate) fn room_after_box(
    room: &std::cell::Cell<Au>,
    model: &crate::block::BoxModel,
    (top, extra): (Au, Au),
    broken: bool,
) {
    use crate::block::BOTTOM;
    let below = room.get();
    if below <= Au::ZERO {
        return;
    }
    room.set(if broken || model.border[BOTTOM] > Au::ZERO {
        Au::ZERO
    } else {
        (below + extra.clamp_non_negative() + model.padding[BOTTOM] - top).clamp_non_negative()
    });
}

#[cfg(test)]
mod tests {
    use super::Spacer;

    /// Places a line of no content `height` high whose content reaches `reach` (over, under)
    /// past it, as [`Spacer::place`] does with the reach it measures.
    fn place(spacer: &mut Spacer, (over, under): (f32, f32), height: f32) -> (f32, f32) {
        spacer.shift((over, under), height)
    }

    #[test]
    fn moves_lines_by_what_their_annotations_reach_past_the_room_for_them() {
        let mut spacer = Spacer::new(2.0, 20.0, false);
        // 8 pixels over the first line, 2 of them in the room over it.
        assert_eq!(place(&mut spacer, (8.0, -3.0), 24.0), (6.0, 0.0));
        // The next line reaches 2 into the 3 the line before leaves.
        assert_eq!(place(&mut spacer, (2.0, 5.0), 24.0), (0.0, 5.0));
        // After annotations under a line, one with room over its text moves up into theirs.
        assert_eq!(place(&mut spacer, (-4.0, 0.0), 24.0), (-4.0, 0.0));
    }

    #[test]
    fn lets_raised_lines_and_trimming_take_the_room_for_annotations() {
        // An initial letter raised by two lines of 24 pixels: 30 over its text line fit in them.
        let mut spacer = Spacer::new(0.0, 20.0, false);
        spacer.raise(2);
        assert!(spacer.raising());
        assert_eq!(place(&mut spacer, (0.0, 0.0), 24.0), (0.0, 0.0));
        assert_eq!(place(&mut spacer, (0.0, 0.0), 24.0), (0.0, 0.0));
        assert!(!spacer.raising());
        assert_eq!(place(&mut spacer, (50.0, 0.0), 24.0), (2.0, 0.0));
        // A first line trimmed by `text-box-trim` does not move, nor do the lines of a raised
        // letter move it down.
        let mut spacer = Spacer::new(0.0, 20.0, true);
        spacer.raise(1);
        assert_eq!(place(&mut spacer, (0.0, 0.0), 24.0), (0.0, -24.0));
        assert_eq!(place(&mut spacer, (8.0, 0.0), 24.0), (0.0, 0.0));
        assert_eq!(place(&mut spacer, (8.0, 0.0), 24.0), (8.0, 0.0));
    }
}

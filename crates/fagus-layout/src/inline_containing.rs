//! The containing blocks that inline boxes broken across lines form for absolutely positioned
//! boxes (CSS 2.1 §10.1, item 4): from the start of the box's fragments on its first line to the
//! end of those on its last, as Blink computes them (`InlineContainingBlockUtils`): the padding
//! edges of the fragments on the first line (united) at the start, of those on the last line at
//! the end, a size that would be negative clamped to zero. Lines in other columns or on other
//! pages lie where those are in the flow of fragmentainers laid end to end (see
//! [`crate::positioned::PageFlow`]).

use std::sync::Arc;

use fagus_style::ComputedStyle;
use fagus_style::values::{Display, DisplayInside, DisplayOutside};
use rustc_hash::FxHashMap;

use crate::fragment::{Fragment, FragmentKind};
use crate::positioned::ChainKey;
use crate::units::{Au, Rect};

/// The containing block of an inline box broken across lines, and the fragments of the box
/// that hold the absolutely positioned boxes placed in it.
pub(crate) struct InlineBlock {
    /// The padding box from its first line to its last, in the coordinates of the roots.
    pub(crate) rect: Rect,
    /// The last fragment of the box in each root that has one (by its index), with the path of
    /// child indices to it from the root and its border box origin. Positioned boxes go in the
    /// last, so that they are painted over all of its content, as in Chrome.
    pub(crate) lasts: Vec<(usize, Vec<usize>, (Au, Au))>,
}

/// The containing blocks of the inline boxes broken across lines, by box.
pub(crate) type InlineBlocks = FxHashMap<ChainKey, InlineBlock>;

/// Whether `style` is that of an inline box (not an atomic inline) which contains absolutely
/// positioned boxes.
pub(crate) fn is_inline_containing(style: &ComputedStyle) -> bool {
    is_inline_box(style) && style.contains_absolute()
}

/// Whether `style` is that of an inline box, whose fragments lie on the lines around them.
fn is_inline_box(style: &ComputedStyle) -> bool {
    matches!(
        style.display,
        Display::Normal {
            outside: DisplayOutside::Inline,
            inside: DisplayInside::Flow,
            ..
        }
    )
}

/// The fragments of an inline box on its first and on its last line so far.
struct Lines {
    /// The keys of its fragments: those of the parts of an element's box that blocks inside it
    /// split have styles of their own (without the start edges after a block).
    keys: Vec<ChainKey>,
    style: Arc<ComputedStyle>,
    first: (usize, Rect),
    last: (usize, Rect),
    lasts: Vec<(usize, Vec<usize>, (Au, Au))>,
}

fn unite(a: Rect, b: Rect) -> Rect {
    let (left, top) = (a.x.min(b.x), a.y.min(b.y));
    let (right, bottom) = (
        (a.x + a.width).max(b.x + b.width),
        (a.y + a.height).max(b.y + b.height),
    );
    Rect::new(left, top, right - left, bottom - top)
}

/// Where [`walk`] is: the root (by index) and the path to the fragment from it, and the line
/// the fragment lies on.
struct Walk {
    root: usize,
    path: Vec<usize>,
    lines: usize,
}

/// Collects the containing blocks of the inline boxes broken across lines below `roots`, each
/// with its border box origin in the coordinates the rectangles are given in: the fragments of
/// one box in several roots are taken as one, in order.
pub(crate) fn inline_blocks<'f>(roots: impl Iterator<Item = (&'f Fragment, (Au, Au))>) -> InlineBlocks {
    // By element, or by style for boxes without one.
    let mut found: FxHashMap<ChainKey, Lines> = FxHashMap::default();
    let mut at = Walk {
        root: 0,
        path: Vec::new(),
        lines: 0,
    };
    for (index, (root, origin)) in roots.enumerate() {
        at.root = index;
        walk(root, origin, None, &mut at, &mut found);
    }
    found
        .into_values()
        .filter(|lines| lines.first.0 != lines.last.0)
        .flat_map(|lines| {
            let rect = containing_block(&lines);
            lines.keys.into_iter().map(move |key| {
                (
                    key,
                    InlineBlock {
                        rect,
                        lasts: lines.lasts.clone(),
                    },
                )
            })
        })
        .collect()
}

fn walk(
    fragment: &Fragment,
    origin: (Au, Au),
    line: Option<usize>,
    at: &mut Walk,
    found: &mut FxHashMap<ChainKey, Lines>,
) {
    let line = match &fragment.kind {
        FragmentKind::Line { .. } => {
            at.lines += 1;
            Some(at.lines)
        }
        FragmentKind::Box { style, node, .. } => {
            if let Some(line) = line
                && is_inline_containing(style)
            {
                let rect = Rect::new(origin.0, origin.1, fragment.rect.width, fragment.rect.height);
                let key = (*node, Arc::as_ptr(style) as usize);
                let element = (*node, if node.is_some() { 0 } else { key.1 });
                let last = (at.root, at.path.clone(), origin);
                match found.get_mut(&element) {
                    Some(lines) => {
                        if !lines.keys.contains(&key) {
                            lines.keys.push(key);
                        }
                        if lines.first.0 == line {
                            lines.first.1 = unite(lines.first.1, rect);
                        }
                        lines.last = if lines.last.0 == line {
                            (line, unite(lines.last.1, rect))
                        } else {
                            (line, rect)
                        };
                        match lines.lasts.last_mut() {
                            Some(previous) if previous.0 == at.root => *previous = last,
                            _ => lines.lasts.push(last),
                        }
                    }
                    None => {
                        found.insert(
                            element,
                            Lines {
                                keys: vec![key],
                                style: Arc::clone(style),
                                first: (line, rect),
                                last: (line, rect),
                                lasts: vec![last],
                            },
                        );
                    }
                }
            }
            // The lines inside an atomic inline or a block are lines of their own.
            line.filter(|_| is_inline_box(style))
        }
        _ => line,
    };
    for (index, child) in fragment.children.iter().enumerate() {
        at.path.push(index);
        walk(
            child,
            (origin.0 + child.rect.x, origin.1 + child.rect.y),
            line,
            at,
            found,
        );
        at.path.pop();
    }
}

/// The padding box from the start of the fragments on the first line to the end of those on the
/// last (their starts on the right in right-to-left text).
fn containing_block(lines: &Lines) -> Rect {
    let style = &lines.style;
    let border = |width: f32| Au::from_px(width);
    let (first, last) = (lines.first.1, lines.last.1);
    let top = first.y + border(style.border_width.top);
    let bottom = last.y + last.height - border(style.border_width.bottom);
    let (left, right) = if style.is_rtl() {
        (
            last.x + border(style.border_width.left),
            first.x + first.width - border(style.border_width.right),
        )
    } else {
        (
            first.x + border(style.border_width.left),
            last.x + last.width - border(style.border_width.right),
        )
    };
    let width = (right - left).clamp_non_negative();
    let height = (bottom - top).clamp_non_negative();
    // A width clamped to zero keeps the start edge.
    let x = if style.is_rtl() { right - width } else { left };
    Rect::new(x, top, width, height)
}

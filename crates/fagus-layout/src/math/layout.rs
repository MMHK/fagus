//! The layout algorithms of `MathML` Core as Blink implements them (`math_*_layout_algorithm.cc`,
//! BSD licensed: see `CHROMIUM-LICENSE`): rows, tokens, operators, fractions, radicals,
//! scripts, limits, `<mpadded>`, `<mspace>` and tables.
//!
//! Boxes are laid out in pixels with their baselines at y = 0 and y growing down. Lengths are
//! cut to layout units (1/64 px) where Blink cuts them. A box's width is its max-content width,
//! as Blink computes it apart from layout (`ComputeMinMaxSizes`), even where its content reaches
//! past it.

use std::sync::Arc;

use fagus_style::ComputedStyle;
use fagus_style::values::Visibility;
use fagus_text::{FaceId, FontDb, MathConstant};

use super::stretch::{MathFont, Stretched, stretch, vertical_width};
use super::{Formula, Kind, MathNode, Operator, ScriptKind};
use crate::fragment::{Children, Fragment, FragmentKind, Glyph, GlyphOrientation, TextRun, VisibleSides};
use crate::units::{Au, Rect};

/// A length cut to layout units, as Blink's `LayoutUnit(float)` cuts it.
pub(crate) fn lu(px: f32) -> f32 {
    (px * 64.0).trunc() / 64.0
}

/// A length rounded to layout units (`LayoutUnit::FromFloatRound`).
pub(crate) fn lu_round(px: f32) -> f32 {
    (px * 64.0).round() / 64.0
}

/// A length floored to layout units (`LayoutUnit::FromFloatFloor`).
fn lu_floor(px: f32) -> f32 {
    (px * 64.0).floor() / 64.0
}

/// Half a length in layout units, as `LayoutUnit / 2` divides.
fn half(px: f32) -> f32 {
    lu(lu(px) / 2.0)
}

/// Where the content of a table cell goes in its column: a cell's content is a block, which
/// `text-align` moves only as `-webkit-center` (the user agent's) and its kin do; else it stays
/// at the start.
#[derive(Clone, Copy, Debug)]
enum CellAlign {
    Start,
    Center,
    End,
}

impl CellAlign {
    fn of(style: &ComputedStyle) -> Self {
        use fagus_style::values::TextAlign;
        if !style.align_blocks {
            return Self::Start;
        }
        match style.text_align {
            TextAlign::Center => Self::Center,
            TextAlign::Right | TextAlign::End => Self::End,
            _ => Self::Start,
        }
    }

    /// How far the content goes from the start of the room `free` it leaves.
    fn offset(self, free: f32) -> f32 {
        match self {
            Self::Start => 0.0,
            Self::Center => half(free),
            Self::End => free,
        }
    }
}

/// What a laid-out box draws.
#[derive(Clone, Debug)]
enum Item {
    /// Glyphs of one face on a baseline at `y`. The text of a token is set in a line box of
    /// its own, whose (rounded) ascent and descent `line` gives: Chrome draws its baseline on a
    /// whole pixel, as it does the text of lines. Stretched operators are drawn where they are.
    Run {
        x: f32,
        y: f32,
        face: FaceId,
        size: f32,
        style: Arc<ComputedStyle>,
        glyphs: Vec<Glyph>,
        text: String,
        width: f32,
        line: Option<(f32, f32)>,
    },
    /// A rule in the color of the text (a fraction bar, the bar of a radical) from `y`, which
    /// the painter moves up by half its snapped height (see `ComputedStyle::math_bar`).
    Bar {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        style: Arc<ComputedStyle>,
    },
    /// The background and border of an element.
    Frame {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        style: Arc<ComputedStyle>,
    },
}

impl Item {
    /// How wide the item is.
    fn width(&self) -> f32 {
        match self {
            Self::Run { width, .. } | Self::Bar { width, .. } | Self::Frame { width, .. } => *width,
        }
    }

    fn moved(mut self, dx: f32, dy: f32) -> Self {
        match &mut self {
            Self::Run { x, y, .. } | Self::Bar { x, y, .. } | Self::Frame { x, y, .. } => {
                *x += dx;
                *y += dy;
            }
        }
        self
    }
}

/// A laid-out box: its width, its ascent and descent from its baseline, its italic correction
/// and what it draws, from its left edge and baseline.
#[derive(Clone, Debug, Default)]
struct Laid {
    width: f32,
    ascent: f32,
    descent: f32,
    italic: f32,
    items: Vec<Item>,
}

impl Laid {
    /// Adds what `child` draws with its left edge at `x` and its baseline at `y`.
    fn place(&mut self, child: Laid, x: f32, y: f32) {
        self.items
            .extend(child.items.into_iter().map(|item| item.moved(x, y)));
    }

    /// Mirrors what the box draws from left to right: Blink lays out the children of a box
    /// right to left by turning their offsets from its start into offsets from its right edge.
    /// Items are laid out left to right and a box whose direction differs from its parent's
    /// is mirrored with all it holds, so that what stands right to left is mirrored once;
    /// glyphs keep their shape.
    fn mirror(&mut self) {
        let width = self.width;
        for item in &mut self.items {
            let item_width = item.width();
            match item {
                Item::Run { x, .. } | Item::Bar { x, .. } | Item::Frame { x, .. } => {
                    *x = width - *x - item_width;
                }
            }
        }
    }
}

/// The size a stretchy operator stretches to: along the block axis, an ascent and a descent;
/// along the inline axis, a width (Blink's target stretch sizes).
#[derive(Clone, Copy, Debug, Default)]
struct Target {
    block: Option<(f32, f32)>,
    inline: Option<f32>,
}

/// The properties of an embellished operator (Blink's
/// `MathMLEmbellishedOperatorProperties`).
#[derive(Clone, Copy, Debug)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "the flags of MathML Core's operators"
)]
struct Embellished {
    movablelimits: bool,
    largeop: bool,
    stretchy: bool,
    vertical: bool,
    lspace: f32,
    rspace: f32,
}

/// The padding and border widths of a box, in layout units: top, right, bottom, left.
fn frame(style: &ComputedStyle) -> [f32; 4] {
    let padding = |side: &fagus_style::values::LengthPercentage| lu(side.resolve(0.0).max(0.0));
    [
        padding(&style.padding.top) + lu(style.border_width.top),
        padding(&style.padding.right) + lu(style.border_width.right),
        padding(&style.padding.bottom) + lu(style.border_width.bottom),
        padding(&style.padding.left) + lu(style.border_width.left),
    ]
}

/// The width of a box's content: `intrinsic`, or what `width`, `min-width` and `max-width`
/// give where they are lengths (Blink's `ComputeInlineSizeForFragment` of a `MathML` box,
/// whose percentages resolve against no size).
fn content_width(style: &ComputedStyle, intrinsic: f32) -> f32 {
    let [_, right, _, left] = frame(style);
    content_size(
        style,
        intrinsic,
        left + right,
        (&style.width, &style.min_width, &style.max_width),
    )
}

/// The height of a box's content, as [`content_width`] finds its width
/// (`ComputeBlockSizeForFragment`).
fn content_height(style: &ComputedStyle, intrinsic: f32) -> f32 {
    let [top, _, bottom, _] = frame(style);
    content_size(
        style,
        intrinsic,
        top + bottom,
        (&style.height, &style.min_height, &style.max_height),
    )
}

/// A content size from a size, a minimum and a maximum that are lengths, of the border box
/// where `box-sizing` says so, around `frame` of padding and border.
fn content_size(
    style: &ComputedStyle,
    intrinsic: f32,
    frame: f32,
    (size, min, max): (
        &fagus_style::values::Size,
        &fagus_style::values::Size,
        &fagus_style::values::MaxSize,
    ),
) -> f32 {
    use fagus_style::values::{BoxSizing, MaxSize, Size};
    let content = |length: &fagus_style::values::LengthPercentage| {
        length.fixed().map(|value| {
            let value = if style.box_sizing == BoxSizing::BorderBox {
                value - frame
            } else {
                value
            };
            lu(value.max(0.0))
        })
    };
    let mut used = match size {
        Size::Length(length) => content(length).unwrap_or(intrinsic),
        _ => intrinsic,
    };
    if let MaxSize::Length(length) = max
        && let Some(max) = content(length)
    {
        used = used.min(max);
    }
    if let Size::Length(length) = min
        && let Some(min) = content(length)
    {
        used = used.max(min);
    }
    used
}

/// The margins of a box in layout units, `auto` and percentages as 0: top, right, bottom,
/// left.
fn margins(style: &ComputedStyle) -> [f32; 4] {
    use fagus_style::values::LengthPercentageAuto;
    let margin = |side: &LengthPercentageAuto| match side {
        LengthPercentageAuto::Length(length) => lu(length.fixed().unwrap_or(0.0)),
        LengthPercentageAuto::Auto => 0.0,
    };
    [
        margin(&style.margin.top),
        margin(&style.margin.right),
        margin(&style.margin.bottom),
        margin(&style.margin.left),
    ]
}

/// Whether a style draws a background or a border.
fn decorated(style: &ComputedStyle) -> bool {
    style.background_color.a > 0.0
        || [
            style.border_width.top,
            style.border_width.right,
            style.border_width.bottom,
            style.border_width.left,
        ]
        .iter()
        .any(|width| *width > 0.0)
}

/// A formula laid out: the fragments of its content in a line, with its size and baseline.
#[derive(Debug)]
pub(crate) struct LaidFormula {
    /// The line that holds the formula's content, at the content box's top left corner.
    pub(crate) line: Fragment,
    pub(crate) width: Au,
    pub(crate) height: Au,
}

/// Lays out a formula; `available` is the width a block formula is centered in (`display:
/// block`), `None` for an inline one.
pub(crate) fn layout(fonts: &FontDb, formula: &Formula, available: Option<Au>) -> LaidFormula {
    let layouter = Layouter {
        fonts,
        laid: std::cell::RefCell::default(),
    };
    let root = &formula.root;
    let mut laid = layouter.row(root, &root.children, Target::default(), true);
    // An inline formula takes the sizes CSS gives its box (its padding and border are the
    // box's).
    if available.is_none() {
        laid.width = content_width(&root.style, laid.width);
        laid.descent = content_height(&root.style, laid.ascent + laid.descent) - laid.ascent;
    }
    if root.mirrored {
        laid.mirror();
    }
    let mut shift = 0.0;
    if let Some(available) = available {
        // A block formula is centered in its box (Blink's
        // `InlineOffsetForDisplayMathCentering`).
        shift = half(available.to_px() - laid.width).max(0.0);
        laid.width = laid.width.max(available.to_px());
    }
    let ascent = laid.ascent;
    let height = laid.ascent + laid.descent;
    let mut children = Vec::with_capacity(laid.items.len());
    for item in laid.items {
        children.push(fragment(item.moved(shift, ascent)));
    }
    let line = Fragment {
        rect: Rect::new(Au::ZERO, Au::ZERO, Au::from_px(laid.width), Au::from_px(height)),
        kind: FragmentKind::Line {
            baseline: Au::from_px(ascent),
        },
        children: children.into(),
    };
    LaidFormula {
        line,
        width: Au::from_px(laid.width),
        height: Au::from_px(height),
    }
}

/// The fragment of an item placed in the line, whose top is at y = 0.
fn fragment(item: Item) -> Fragment {
    match item {
        Item::Run {
            x,
            y,
            face,
            size,
            style,
            glyphs,
            text,
            width,
            line,
        } => {
            // Glyphs out of a line of their own are those of stretched and large operators,
            // which Chrome draws where they are.
            let exact = line.is_none();
            let run = |top: f32, height: f32, baseline: f32| Fragment {
                rect: Rect::new(
                    Au::ZERO,
                    Au::from_px(top),
                    Au::from_px(width),
                    Au::from_px(height),
                ),
                kind: FragmentKind::Text(TextRun {
                    face,
                    size,
                    style,
                    glyphs,
                    text,
                    baseline: Au::from_px(baseline),
                    marker: false,
                    symbol: None,
                    orientation: GlyphOrientation::Horizontal,
                    exact,
                }),
                children: Children::new(),
            };
            if let Some((ascent, descent)) = line {
                Fragment {
                    rect: Rect::new(
                        Au::from_px(x),
                        Au::from_px(y - ascent),
                        Au::from_px(width),
                        Au::from_px(ascent + descent),
                    ),
                    kind: FragmentKind::Line {
                        baseline: Au::from_px(ascent),
                    },
                    children: vec![run(0.0, ascent + descent, ascent)].into(),
                }
            } else {
                // The run's box goes from its baseline up to the top of the formula's line.
                let mut fragment = run(0.0, y.max(0.0), y);
                fragment.rect.x = Au::from_px(x);
                fragment
            }
        }
        Item::Bar {
            x,
            y,
            width,
            height,
            style,
        } => {
            // A rule is a box filled with the color of the text.
            let rule = ComputedStyle {
                display: fagus_style::values::Display::BLOCK,
                background_color: style.color,
                visibility: style.visibility,
                math_bar: true,
                ..ComputedStyle::initial_values().clone()
            };
            Fragment {
                rect: Rect::new(
                    Au::from_px(x),
                    Au::from_px(y),
                    Au::from_px(width),
                    Au::from_px(height),
                ),
                kind: FragmentKind::Box {
                    style: Arc::new(rule),
                    node: None,
                    sides: VisibleSides::ALL,
                },
                children: Children::new(),
            }
        }
        Item::Frame {
            x,
            y,
            width,
            height,
            style,
        } => Fragment {
            rect: Rect::new(
                Au::from_px(x),
                Au::from_px(y),
                Au::from_px(width),
                Au::from_px(height),
            ),
            kind: FragmentKind::Box {
                style: Arc::new(ComputedStyle {
                    display: fagus_style::values::Display::BLOCK,
                    padding: ComputedStyle::initial_values().padding.clone(),
                    margin: ComputedStyle::initial_values().margin.clone(),
                    ..(*style).clone()
                }),
                node: None,
                sides: VisibleSides::ALL,
            },
            children: Children::new(),
        },
    }
}

/// A cell of a table as [`Layouter::table`] lays it out: its node, slot, content, padding and
/// border, and whether it has content.
struct TableCell<'n> {
    node: &'n MathNode,
    slot: super::table::Slot,
    content: Laid,
    frame: [f32; 4],
    filled: bool,
}

struct Layouter<'a> {
    fonts: &'a FontDb,
    /// The boxes laid out, by node (its address) and the size its operators stretch to (as
    /// bits): rows of stretchy operators lay out their children twice, which nesting would
    /// multiply.
    laid: std::cell::RefCell<rustc_hash::FxHashMap<(usize, TargetKey), Laid>>,
}

/// A [`Target`] as bits, to key layouts by.
type TargetKey = (Option<(u32, u32)>, Option<u32>);

impl Target {
    fn key(self) -> TargetKey {
        (
            self.block
                .map(|(ascent, descent)| (ascent.to_bits(), descent.to_bits())),
            self.inline.map(f32::to_bits),
        )
    }
}

/// The first in-flow child of a node: all are in flow.
fn children_count_is(node: &MathNode, count: usize) -> bool {
    node.children.len() == count
}

/// Whether a node is space-like (`MathML` Core, §3.2.4.1): an `<mtext>` or `<mspace>`, or a
/// grouping element or `<mpadded>` whose children are all space-like.
fn space_like(node: &MathNode) -> bool {
    match &node.kind {
        Kind::Token { space_like, .. } => *space_like,
        Kind::Space { .. } => true,
        Kind::Row { grouping: true } | Kind::Padded(_) => node.children.iter().all(space_like),
        _ => false,
    }
}

/// The core operator of an embellished operator (`MathML` Core, §3.2.4.1).
fn core_operator(node: &MathNode) -> Option<(&Operator, &MathNode)> {
    match &node.kind {
        Kind::Operator(operator) => Some((operator, node)),
        Kind::Scripts { .. } | Kind::Fraction { .. } => node.children.first().and_then(core_operator),
        Kind::Row { grouping: true } | Kind::Padded(_) => {
            let mut core = None;
            for child in &node.children {
                if space_like(child) {
                    continue;
                }
                if core.is_some() {
                    return None;
                }
                core = Some(core_operator(child)?);
            }
            core
        }
        _ => None,
    }
}

fn embellished(node: &MathNode) -> Option<Embellished> {
    let (operator, _) = core_operator(node)?;
    let character = single(&operator.text);
    Some(Embellished {
        movablelimits: operator.movablelimits,
        largeop: operator.largeop,
        stretchy: operator.stretchy,
        vertical: character.is_none_or(super::operators::is_vertical),
        lspace: lu(operator.lspace),
        rspace: lu(operator.rspace),
    })
}

/// Whether a node is an embellished operator that stretches along the block axis
/// (`vertical`) or the inline axis.
fn stretchy(node: &MathNode, vertical: bool) -> bool {
    embellished(node).is_some_and(|properties| properties.stretchy && properties.vertical == vertical)
}

/// The one character of a text, if it is one.
fn single(text: &str) -> Option<char> {
    let mut characters = text.chars();
    let character = characters.next()?;
    characters.next().is_none().then_some(character)
}

/// Whether math is in display style (`math-style: normal`).
fn display(style: &ComputedStyle) -> bool {
    !style.math_compact
}

/// Whether scripted elements have the children they need (Blink's `IsValidMathMLScript`).
fn valid_scripts(node: &MathNode, kind: ScriptKind) -> bool {
    match kind {
        ScriptKind::Sub | ScriptKind::Super | ScriptKind::Under | ScriptKind::Over => {
            children_count_is(node, 2)
        }
        ScriptKind::SubSuper | ScriptKind::UnderOver => children_count_is(node, 3),
        ScriptKind::Multiscripts => {
            let prescripts = |child: &MathNode| matches!(child.kind, Kind::Prescripts);
            let Some(first) = node.children.first() else {
                return false;
            };
            if prescripts(first) {
                return false;
            }
            let mut even = true;
            let mut found = false;
            for child in &node.children[1..] {
                if prescripts(child) {
                    if !even || found {
                        return false;
                    }
                    found = true;
                    continue;
                }
                even = !even;
            }
            even
        }
    }
}

impl Layouter<'_> {
    fn font(&self, style: &ComputedStyle) -> Option<MathFont<'_>> {
        MathFont::of(self.fonts, style)
    }

    /// A constant of the math font of a style in layout units, or `fallback`.
    fn constant(&self, style: &ComputedStyle, constant: MathConstant, fallback: f32) -> f32 {
        lu(self
            .font(style)
            .and_then(|font| font.constant(constant))
            .unwrap_or(fallback))
    }

    fn x_height(&self, style: &ComputedStyle) -> f32 {
        self.font(style).map_or(0.0, |font| font.x_height())
    }

    fn rule_fallback(&self, style: &ComputedStyle) -> f32 {
        self.font(style).map_or(0.0, |font| font.rule_thickness())
    }

    fn axis(&self, style: &ComputedStyle) -> f32 {
        let fallback = self.x_height(style) / 2.0;
        self.constant(style, MathConstant::AxisHeight, fallback)
    }

    /// Lays out a node with the size its stretchy operators stretch to.
    fn layout(&self, node: &MathNode, target: Target) -> Laid {
        let key = (std::ptr::from_ref(node) as usize, target.key());
        if let Some(laid) = self.laid.borrow().get(&key) {
            return laid.clone();
        }
        let laid = self.lay_out(node, target);
        self.laid.borrow_mut().insert(key, laid.clone());
        laid
    }

    /// Lays out a node (see [`Layouter::layout`]).
    fn lay_out(&self, node: &MathNode, target: Target) -> Laid {
        let style = &node.style;
        let [top, right, bottom, left] = frame(style);
        let mut laid = match &node.kind {
            Kind::Space { width, height, depth } => Laid {
                width: lu(width.unwrap_or(0.0)),
                ascent: lu(height.unwrap_or(0.0)),
                descent: lu(height.unwrap_or(0.0) + depth.unwrap_or(0.0)) - lu(height.unwrap_or(0.0)),
                ..Laid::default()
            },
            Kind::Fraction { thickness } if children_count_is(node, 2) => self.fraction(node, *thickness),
            Kind::Sqrt => self.radical(node, node.children.first(), None),
            Kind::Root if children_count_is(node, 2) => {
                self.radical(node, node.children.first(), node.children.get(1))
            }
            Kind::Padded(padding) => self.padded(node, padding),
            Kind::Token { text, source, .. } => self.token(style, text, source.as_deref()),
            Kind::Operator(operator) => {
                if self.special_shaping(style, operator) {
                    self.operator(style, operator, target)
                } else {
                    self.token(style, &operator.text, None)
                }
            }
            Kind::Scripts {
                kind,
                accent,
                accent_under,
            } if valid_scripts(node, *kind) => {
                if kind.is_under_over() && !self.under_over_as_scripts(node) {
                    self.under_over(node, *kind, (*accent, *accent_under), target)
                } else {
                    self.scripts(node, *kind)
                }
            }
            Kind::Table => self.table(node),
            _ => self.row(node, &node.children, target, false),
        };
        // The sizes CSS gives the box: content wider or narrower stays at its start, a box
        // taller or shorter keeps its baseline.
        laid.width = content_width(style, laid.width);
        laid.descent = content_height(style, laid.ascent + laid.descent) - laid.ascent;
        // The padding and border around the content. Content that stands right to left is
        // mirrored with them (see `Laid::mirror`): its offset from the start is that from the
        // right.
        let rtl = style.is_rtl();
        let start = if rtl { right } else { left };
        laid.width += left + right;
        laid.ascent += top;
        laid.descent += bottom;
        let items = std::mem::take(&mut laid.items);
        if decorated(style) && style.visibility == Visibility::Visible {
            laid.items.push(Item::Frame {
                x: 0.0,
                y: -laid.ascent,
                width: laid.width,
                height: laid.ascent + laid.descent,
                style: Arc::clone(style),
            });
        }
        laid.items
            .extend(items.into_iter().map(|item| item.moved(start, 0.0)));
        // The margins, which the parent lays the box out with.
        let [margin_top, margin_right, margin_bottom, margin_left] = margins(style);
        if [margin_top, margin_right, margin_bottom, margin_left]
            .iter()
            .any(|margin| *margin != 0.0)
        {
            let start = if rtl { margin_right } else { margin_left };
            for item in &mut laid.items {
                *item = item.clone().moved(start, 0.0);
            }
            laid.width = (laid.width + margin_left + margin_right).max(0.0);
            laid.ascent += margin_top;
            laid.descent += margin_bottom;
        }
        if node.mirrored {
            laid.mirror();
        }
        laid
    }

    /// `<mrow>` and the elements laid out like it (Blink's `MathRowLayoutAlgorithm`): the
    /// children side by side on their baselines, embellished operators with their spaces, and
    /// stretchy operators stretched to the height of the others.
    fn row(&self, node: &MathNode, children: &[MathNode], target: Target, root: bool) -> Laid {
        let spaced = root || embellished(node).is_none();
        let inherits_block = target.block.is_some();
        let inherits_inline = !inherits_block && target.inline.is_some();
        let mut measured: Vec<Option<Laid>> = vec![None; children.len()];
        let mut sizes = (0.0_f32, 0.0_f32);
        if !inherits_block && !inherits_inline {
            for (index, child) in children.iter().enumerate() {
                if stretchy(child, true) {
                    continue;
                }
                let laid = self.layout(child, Target::default());
                sizes = (sizes.0.max(laid.ascent), sizes.1.max(laid.descent));
                measured[index] = Some(laid);
            }
            if measured.iter().all(Option::is_none) {
                for child in children {
                    let laid = self.layout(
                        child,
                        Target {
                            block: Some((0.0, 0.0)),
                            inline: None,
                        },
                    );
                    sizes = (sizes.0.max(laid.ascent), sizes.1.max(laid.descent));
                }
            }
        }
        let mut laid = Laid::default();
        let mut x = 0.0;
        let (mut ascent, mut descent) = (0.0_f32, 0.0_f32);
        for (index, child) in children.iter().enumerate() {
            let child_target = if inherits_block && stretchy(child, true) {
                target
            } else if inherits_inline && stretchy(child, false) {
                Target {
                    block: None,
                    inline: target.inline,
                }
            } else if !inherits_block && !inherits_inline && stretchy(child, true) {
                Target {
                    block: Some(sizes),
                    inline: None,
                }
            } else {
                Target::default()
            };
            let child_laid = match measured[index].take() {
                Some(laid) if child_target.block.is_none() && child_target.inline.is_none() => laid,
                _ => self.layout(child, child_target),
            };
            let (lspace, rspace) = if spaced {
                embellished(child).map_or((0.0, 0.0), |properties| (properties.lspace, properties.rspace))
            } else {
                (0.0, 0.0)
            };
            x += lspace;
            ascent = ascent.max(child_laid.ascent);
            descent = descent.max(child_laid.descent);
            let width = child_laid.width;
            laid.place(child_laid, x, 0.0);
            x += width + rspace;
        }
        laid.width = x.max(0.0);
        laid.ascent = ascent;
        laid.descent = descent;
        laid
    }

    /// The top and bottom of the ink of a text from its baseline (y down), as Blink's
    /// `ComputeInkMetrics` measures it: empty (`top > bottom`) without ink.
    fn ink(&self, runs: &[crate::inline::ShapedRun]) -> (f32, f32) {
        let (mut top, mut bottom) = (f32::MAX, f32::MIN);
        for run in runs {
            let face = self.fonts.face(run.face);
            for glyph in &run.glyphs {
                if let Some([_, glyph_top, _, glyph_bottom]) =
                    super::stretch::glyph_box(face, glyph.id, run.size)
                {
                    top = top.min(glyph_top - glyph.y_offset);
                    bottom = bottom.max(glyph_bottom - glyph.y_offset);
                }
            }
        }
        (top, bottom)
    }

    /// Text of a token element (Blink's `MathTokenLayoutAlgorithm`): set on its baseline, as
    /// high and deep as the ink of its `source` text, the characters of the element before
    /// `<mi>` made them italic, else of the text.
    fn token(&self, style: &Arc<ComputedStyle>, text: &str, source: Option<&str>) -> Laid {
        if text.is_empty() {
            return Laid::default();
        }
        let runs = crate::inline::shape_line(self.fonts, style, text);
        let (top, bottom) = match source {
            Some(source) => self.ink(&crate::inline::shape_line(self.fonts, style, source)),
            None => self.ink(&runs),
        };
        let mut laid = Laid::default();
        let mut x = 0.0_f32;
        for run in runs {
            // The invisible operators (function application, invisible times, separator and
            // plus) take room but draw nothing, as in Chrome.
            if run
                .text
                .chars()
                .all(|character| ('\u{2061}'..='\u{2064}').contains(&character))
            {
                x += run.width;
                continue;
            }
            let metrics = crate::inline::metrics(self.fonts, style);
            laid.items.push(Item::Run {
                x,
                y: 0.0,
                face: run.face,
                size: run.size,
                style: Arc::clone(style),
                glyphs: run.glyphs,
                text: run.text,
                width: run.width,
                line: Some((metrics.ascent, metrics.descent)),
            });
            x += run.width;
        }
        // Text is as wide as its advances, rounded up to layout units.
        laid.width = (x * 64.0).ceil() / 64.0;
        // Runs stand in the order they go on the page, which mirroring the token right to
        // left must keep.
        if style.is_rtl() {
            laid.mirror();
        }
        if style.display != fagus_style::values::Display::BLOCK {
            // An element CSS gives another display than math's is a box of text, as high as
            // its line.
            let metrics = crate::inline::metrics(self.fonts, style);
            laid.ascent = metrics.ascent + metrics.leading_above();
            laid.descent = metrics.line_height - laid.ascent;
        } else if top <= bottom {
            laid.ascent = lu(-top);
            laid.descent = lu(bottom);
        }
        laid
    }

    /// Whether an `<mo>` is drawn with a stretched or large glyph (Blink's
    /// `IsOperatorWithSpecialShaping`).
    fn special_shaping(&self, style: &ComputedStyle, operator: &Operator) -> bool {
        let Some(character) = single(&operator.text) else {
            return false;
        };
        let has_glyph = self
            .font(style)
            .is_some_and(|font| font.glyph(character).is_some());
        has_glyph && (operator.stretchy || (operator.largeop && display(style)))
    }

    /// A stretchy or large operator (Blink's `MathOperatorLayoutAlgorithm`).
    fn operator(&self, style: &Arc<ComputedStyle>, operator: &Operator, target: Target) -> Laid {
        let (Some(character), Some(font)) = (single(&operator.text), self.font(style)) else {
            return Laid::default();
        };
        let vertical = super::operators::is_vertical(character);
        let axis = self.axis(style);
        let mut size = 0.0;
        let (mut target_ascent, mut target_descent) = (0.0_f32, 0.0_f32);
        if operator.stretchy {
            if !vertical {
                size = target.inline.unwrap_or(0.0);
            } else if let Some((ascent, descent)) = target.block {
                (target_ascent, target_descent) = (ascent, descent);
                if operator.symmetric {
                    let half_size = (target_ascent - axis).max(target_descent + axis);
                    target_ascent = half_size + axis;
                    target_descent = half_size - axis;
                }
                size = target_ascent + target_descent;
                let unstretched = font.glyph(character).map_or(0.0, |glyph| {
                    let [_, top, _, bottom] = font.bounds(glyph);
                    lu(bottom - top)
                });
                let min = operator
                    .minsize
                    .map_or(unstretched, |length| lu(length.resolve(unstretched)))
                    .max(0.0);
                let max = operator
                    .maxsize
                    .map_or(f32::MAX, |length| lu(length.resolve(unstretched)))
                    .max(min);
                if size <= 0.0 {
                    target_ascent = half(min) + axis;
                    target_descent = min - target_ascent;
                } else if size < min {
                    target_ascent = lu((target_ascent - axis).max(0.0) * min / size) + axis;
                    target_descent = min - target_ascent;
                } else if max < size {
                    target_ascent = lu((target_ascent - axis).max(0.0) * max / size) + axis;
                    target_descent = max - target_ascent;
                }
                size = target_ascent + target_descent;
            }
        } else {
            size = self.constant(style, MathConstant::DisplayOperatorMinHeight, 0.0);
        }
        let stretched = stretch(&font, character, vertical, size);
        let operator_ascent = lu_floor(stretched.ascent);
        let operator_descent = lu_floor(stretched.descent);
        let delta = if vertical && operator.stretchy {
            half((operator_ascent - operator_descent) - (target_ascent - target_descent))
        } else if operator.largeop && operator.symmetric {
            half((operator_ascent - operator_descent) - 2.0 * axis)
        } else {
            0.0
        };
        // The width comes from the operator's intrinsic sizes, not from the stretched glyph.
        let width = if operator.stretchy {
            if vertical {
                vertical_width(&font, character)
            } else {
                lu(stretch(&font, character, false, 0.0).advance)
            }
        } else {
            let display_size = self.constant(style, MathConstant::DisplayOperatorMinHeight, 0.0);
            lu(stretch(&font, character, true, display_size).advance)
        };
        // The glyph is centered in the width CSS gives the box.
        let width = content_width(style, width);
        let mut laid = Laid {
            width,
            ascent: operator_ascent - delta,
            descent: operator_descent + delta,
            italic: lu(stretched.italic),
            items: Vec::new(),
        };
        // The glyph is centered in the box and sits `delta` below its baseline.
        let x = half(width - lu(stretched.advance));
        self.push_glyphs(&mut laid, &font, style, &stretched, (x, delta), &operator.text);
        // The parts of an assembly keep their order right to left, where the box is mirrored
        // (see `Laid::mirror`): they are mirrored in their extent first.
        if style.is_rtl() && laid.items.len() > 1 {
            let extent = |item: &Item| match item {
                Item::Run { x, width, .. } => (*x, *x + *width),
                _ => (0.0, 0.0),
            };
            let left = laid
                .items
                .iter()
                .map(|item| extent(item).0)
                .fold(f32::MAX, f32::min);
            let right = laid
                .items
                .iter()
                .map(|item| extent(item).1)
                .fold(f32::MIN, f32::max);
            for item in &mut laid.items {
                if let Item::Run { x, width, .. } = item {
                    *x = left + right - *x - *width;
                }
            }
        }
        laid
    }

    /// Adds the glyphs of a stretched operator with its origin at `origin`.
    fn push_glyphs(
        &self,
        laid: &mut Laid,
        font: &MathFont<'_>,
        style: &Arc<ComputedStyle>,
        stretched: &Stretched,
        origin: (f32, f32),
        text: &str,
    ) {
        for (index, &(glyph, x, y)) in stretched.glyphs.iter().enumerate() {
            // The first glyph stands for the operator's text; the other parts of an assembly
            // for nothing.
            let text = if index == 0 {
                text.to_owned()
            } else {
                String::new()
            };
            let advance = font.advance(glyph);
            laid.items.push(Item::Run {
                x: origin.0 + x,
                y: origin.1 + y,
                face: font.id,
                size: font.size,
                style: Arc::clone(style),
                glyphs: vec![Glyph {
                    id: glyph,
                    advance,
                    x_offset: 0.0,
                    y_offset: 0.0,
                    cluster: (0, u32::try_from(text.len()).unwrap_or(0)),
                }],
                text,
                width: advance,
                line: None,
            });
        }
    }

    /// `<mfrac>` (Blink's `MathFractionLayoutAlgorithm`): the numerator over the denominator,
    /// with a bar on the math axis between them unless its thickness is 0.
    fn fraction(&self, node: &MathNode, thickness: Option<super::MathLength>) -> Laid {
        let style = &node.style;
        let numerator = self.layout(&node.children[0], Target::default());
        let denominator = self.layout(&node.children[1], Target::default());
        let display = display(style);
        let rule = self.rule_fallback(style);
        let default_thickness = self.constant(style, MathConstant::FractionRuleThickness, rule);
        let thickness =
            lu(thickness.map_or(default_thickness, |length| length.resolve(default_thickness))).max(0.0);
        let (numerator_shift, denominator_shift);
        if thickness > 0.0 {
            let axis = self.axis(style);
            let (gap_up, gap_down, shift_up, shift_down) = if display {
                (
                    MathConstant::FractionNumDisplayStyleGapMin,
                    MathConstant::FractionDenomDisplayStyleGapMin,
                    MathConstant::FractionNumeratorDisplayStyleShiftUp,
                    MathConstant::FractionDenominatorDisplayStyleShiftDown,
                )
            } else {
                (
                    MathConstant::FractionNumeratorGapMin,
                    MathConstant::FractionDenominatorGapMin,
                    MathConstant::FractionNumeratorShiftUp,
                    MathConstant::FractionDenominatorShiftDown,
                )
            };
            let numerator_gap = self.constant(style, gap_up, if display { 3.0 } else { 1.0 } * rule);
            let denominator_gap = self.constant(style, gap_down, numerator_gap);
            numerator_shift = self
                .constant(style, shift_up, 0.0)
                .max(axis + half(thickness) + numerator_gap + numerator.descent);
            denominator_shift = self
                .constant(style, shift_down, 0.0)
                .max(half(thickness) + denominator_gap + denominator.ascent - axis);
        } else {
            let (gap, top, bottom) = if display {
                (
                    MathConstant::StackDisplayStyleGapMin,
                    MathConstant::StackTopDisplayStyleShiftUp,
                    MathConstant::StackBottomDisplayStyleShiftDown,
                )
            } else {
                (
                    MathConstant::StackGapMin,
                    MathConstant::StackTopShiftUp,
                    MathConstant::StackBottomShiftDown,
                )
            };
            let gap_min = self.constant(style, gap, if display { 7.0 } else { 3.0 } * rule);
            let mut up = self.constant(style, top, 0.0);
            let mut down = self.constant(style, bottom, 0.0);
            let gap = down - denominator.ascent + up - numerator.descent;
            if gap < gap_min {
                let difference = gap_min - gap;
                let delta = half(difference);
                up += delta;
                down += difference - delta;
            }
            numerator_shift = up;
            denominator_shift = down;
        }
        let width = content_width(style, numerator.width.max(denominator.width));
        let mut laid = Laid {
            width,
            ascent: (numerator_shift + numerator.ascent)
                .max(-denominator_shift + denominator.ascent)
                .max(0.0),
            descent: (-numerator_shift + numerator.descent)
                .max(denominator_shift + denominator.descent)
                .max(0.0),
            ..Laid::default()
        };
        if thickness > 0.0 && style.visibility == Visibility::Visible {
            let axis = self.axis(style);
            laid.items.push(Item::Bar {
                x: 0.0,
                y: -axis,
                width,
                height: thickness,
                style: Arc::clone(style),
            });
        }
        let numerator_x = half(width - numerator.width);
        let denominator_x = half(width - denominator.width);
        laid.place(numerator, numerator_x, -numerator_shift);
        laid.place(denominator, denominator_x, denominator_shift);
        laid
    }

    /// `<msqrt>` and `<mroot>` (Blink's `MathRadicalLayoutAlgorithm`): the radical sign stretched
    /// over the base, a bar over it, and the index raised before the sign.
    fn radical(&self, node: &MathNode, base: Option<&MathNode>, index: Option<&MathNode>) -> Laid {
        let style = &node.style;
        let display = display(style);
        let rule = self.rule_fallback(style);
        let x_height = self.x_height(style);
        let rule_thickness = self.constant(style, MathConstant::RadicalRuleThickness, rule);
        let gap = if display {
            self.constant(
                style,
                MathConstant::RadicalDisplayStyleVerticalGap,
                rule + x_height / 4.0,
            )
        } else {
            self.constant(style, MathConstant::RadicalVerticalGap, 5.0 * rule / 4.0)
        };
        let extra_ascender = self.constant(style, MathConstant::RadicalExtraAscender, rule_thickness);
        let base = base.map(|base| self.layout(base, Target::default()));
        let index = index.map(|index| self.layout(index, Target::default()));
        let (base_ascent, base_descent, base_width) = base
            .as_ref()
            .map_or((0.0, 0.0, 0.0), |base| (base.ascent, base.descent, base.width));
        let (mut kern_before, mut kern_after) = (0.0, 0.0);
        let mut index_width = 0.0;
        let mut width = 0.0;
        if let Some(index) = &index {
            index_width = index.width;
            let font_size = (style.font_size + 0.5).floor();
            let before = self.constant(
                style,
                MathConstant::RadicalKernBeforeDegree,
                5.0 * font_size / 18.0,
            );
            let after = self.constant(
                style,
                MathConstant::RadicalKernAfterDegree,
                -10.0 * font_size / 18.0,
            );
            kern_before = before.max(0.0);
            kern_after = after.max(-index_width);
            width += kern_before + index_width + kern_after;
        }
        let font = self.font(style);
        let sign = font.as_ref().filter(|font| font.glyph('\u{221A}').is_some());
        let surd = sign.map(|font| {
            stretch(
                font,
                '\u{221A}',
                true,
                base_ascent + base_descent + gap + rule_thickness,
            )
        });
        if let Some(font) = sign {
            width += vertical_width(font, '\u{221A}');
        }
        width += base_width;
        let surd_ascent = surd.as_ref().map_or(0.0, |surd| lu(surd.ascent));
        let surd_advance = surd.as_ref().map_or(0.0, |surd| lu(surd.advance));
        let surd_size = surd.as_ref().map_or(0.0, |surd| lu(surd.ascent + surd.descent));
        let raise_percent = if index.is_some() {
            self.font(style)
                .and_then(|font| font.constant(MathConstant::RadicalDegreeBottomRaisePercent))
                .unwrap_or(0.6)
        } else {
            0.0
        };
        // Blink multiplies the percentage as a layout unit.
        let index_raise = lu(lu(raise_percent) * surd_size);
        let mut ascent = base_ascent + gap + rule_thickness + extra_ascender;
        let mut descent = base_descent.max(surd_size + extra_ascender - ascent);
        if let Some(index) = &index {
            ascent = ascent.max(-descent + index_raise + index.descent + index.ascent);
            descent = descent.max(descent - index_raise + index.descent + index.ascent);
        }
        let mut laid = Laid {
            width,
            ascent,
            descent,
            ..Laid::default()
        };
        // The sign starts after the index, its top where the gap over the base starts.
        let operator_x = index_width + kern_before + kern_after;
        let sign_top = -gap - base_ascent;
        if let (Some(font), Some(surd)) = (sign, &surd) {
            self.push_glyphs(
                &mut laid,
                font,
                style,
                surd,
                (operator_x, sign_top + surd_ascent),
                "\u{221A}",
            );
            if rule_thickness > 0.0 && style.visibility == Visibility::Visible {
                laid.items.push(Item::Bar {
                    x: operator_x + surd_advance,
                    y: sign_top,
                    width: base_width,
                    height: rule_thickness,
                    style: Arc::clone(style),
                });
            }
        }
        if let Some(base) = base {
            laid.place(base, surd_advance + index_width + kern_before + kern_after, 0.0);
        }
        if let Some(index) = index {
            let index_y = descent - index_raise - index.descent;
            laid.place(index, kern_before, index_y);
        }
        laid
    }

    /// `<mpadded>` (Blink's `MathPaddedLayoutAlgorithm`): its content with the width, height
    /// and depth its attributes give, moved by `lspace` and `voffset`.
    fn padded(&self, node: &MathNode, padding: &super::Padding) -> Laid {
        let content = node
            .children
            .first()
            .map(|child| self.layout(child, Target::default()))
            .unwrap_or_default();
        let ascent = padding.height.map_or(content.ascent, lu).max(0.0);
        let descent = padding.depth.map_or(content.descent, lu).max(0.0);
        let lspace = padding.lspace.map_or(0.0, lu).max(0.0);
        let voffset = padding.voffset.map_or(0.0, lu);
        let width = padding.width.map_or(content.width, lu);
        let mut laid = Laid {
            width,
            ascent,
            descent,
            ..Laid::default()
        };
        laid.place(content, lspace, -voffset);
        laid
    }

    /// Whether `<munder>`, `<mover>` or `<munderover>` puts its scripts beside its base, as
    /// limits do in inline formulas (Blink's `IsUnderOverLaidOutAsSubSup`).
    fn under_over_as_scripts(&self, node: &MathNode) -> bool {
        !display(&node.style)
            && node
                .children
                .first()
                .and_then(embellished)
                .is_some_and(|properties| properties.movablelimits)
    }

    /// Scripts beside a base (Blink's `MathScriptsLayoutAlgorithm`): subscripts shifted down,
    /// superscripts up, prescripts before the base.
    #[allow(clippy::too_many_lines, reason = "one algorithm, as in Blink")]
    fn scripts(&self, node: &MathNode, kind: ScriptKind) -> Laid {
        let style = &node.style;
        let x_height = self.x_height(style);
        let font_size = (style.font_size + 0.5).floor();
        let parameter = |constant, fallback| self.constant(style, constant, fallback);
        let subscript_shift_down = parameter(MathConstant::SubscriptShiftDown, x_height / 3.0);
        let superscript_shift_up = parameter(MathConstant::SuperscriptShiftUp, x_height);
        let superscript_shift_up_cramped = parameter(MathConstant::SuperscriptShiftUpCramped, x_height);
        let subscript_baseline_drop_min = parameter(MathConstant::SubscriptBaselineDropMin, x_height / 2.0);
        let superscript_baseline_drop_max =
            parameter(MathConstant::SuperscriptBaselineDropMax, x_height / 2.0);
        let sub_superscript_gap_min = parameter(MathConstant::SubSuperscriptGapMin, font_size / 5.0);
        let superscript_bottom_min = parameter(MathConstant::SuperscriptBottomMin, x_height / 4.0);
        let subscript_top_max = parameter(MathConstant::SubscriptTopMax, 4.0 * x_height / 5.0);
        let superscript_bottom_max_with_subscript = parameter(
            MathConstant::SuperscriptBottomMaxWithSubscript,
            4.0 * x_height / 5.0,
        );
        let space = parameter(MathConstant::SpaceAfterScript, font_size / 5.0);

        // The base, then pairs of a subscript and a superscript, prescripts after the
        // `<mprescripts>`.
        let base = self.layout(&node.children[0], Target::default());
        let mut pairs: Vec<(Option<Laid>, Option<Laid>)> = vec![(None, None)];
        let mut prescripts: Option<Laid> = None;
        let mut first_prescript: Option<usize> = None;
        let mut even = true;
        for child in &node.children[1..] {
            let is_prescripts = matches!(child.kind, Kind::Prescripts);
            let laid = self.layout(child, Target::default());
            match kind {
                ScriptKind::Sub | ScriptKind::Under => pairs[0].0 = Some(laid),
                ScriptKind::Super | ScriptKind::Over => pairs[0].1 = Some(laid),
                ScriptKind::SubSuper | ScriptKind::UnderOver => {
                    if pairs[0].0.is_none() {
                        pairs[0].0 = Some(laid);
                    } else {
                        pairs[0].1 = Some(laid);
                    }
                }
                ScriptKind::Multiscripts => {
                    if is_prescripts {
                        first_prescript = Some(pairs.len() - 1);
                        prescripts = Some(laid);
                        continue;
                    }
                    let last = pairs.len() - 1;
                    if pairs[last].0.is_none() {
                        pairs[last].0 = Some(laid);
                    } else {
                        pairs[last].1 = Some(laid);
                    }
                    even = !even;
                    if even {
                        pairs.push((None, None));
                    }
                }
            }
        }
        let metrics = |laid: &Option<Laid>| {
            laid.as_ref()
                .map_or((0.0, 0.0, 0.0), |laid| (laid.ascent, laid.descent, laid.width))
        };

        // The shifts (Blink's `GetVerticalMetrics`).
        let has_sub = matches!(
            kind,
            ScriptKind::Sub
                | ScriptKind::SubSuper
                | ScriptKind::Multiscripts
                | ScriptKind::Under
                | ScriptKind::UnderOver
        );
        let has_sup = matches!(
            kind,
            ScriptKind::Super
                | ScriptKind::SubSuper
                | ScriptKind::Multiscripts
                | ScriptKind::Over
                | ScriptKind::UnderOver
        );
        let (mut sub_shift, mut sup_shift) = (0.0_f32, 0.0_f32);
        let (mut scripts_ascent, mut scripts_descent) = (0.0_f32, 0.0_f32);
        if has_sub && !matches!(kind, ScriptKind::UnderOver) {
            sub_shift = subscript_shift_down.max(base.descent + subscript_baseline_drop_min);
        }
        let shift_up = if style.math_shift_compact {
            superscript_shift_up_cramped
        } else {
            superscript_shift_up
        };
        if has_sup && !matches!(kind, ScriptKind::UnderOver) {
            sup_shift = shift_up.max(base.ascent - superscript_baseline_drop_max);
        }
        match kind {
            ScriptKind::Sub | ScriptKind::Under => {
                let (sub_ascent, sub_descent, _) = metrics(&pairs[0].0);
                scripts_descent = sub_descent;
                sub_shift = sub_shift.max(sub_ascent - subscript_top_max);
            }
            ScriptKind::Super | ScriptKind::Over => {
                let (sup_ascent, sup_descent, _) = metrics(&pairs[0].1);
                scripts_ascent = sup_ascent;
                sup_shift = sup_shift.max(superscript_bottom_min + sup_descent);
            }
            ScriptKind::SubSuper | ScriptKind::UnderOver | ScriptKind::Multiscripts => {
                for (sub, sup) in &pairs {
                    if sub.is_none() && sup.is_none() {
                        continue;
                    }
                    let (sub_ascent, sub_descent, _) = metrics(sub);
                    let (sup_ascent, sup_descent, _) = metrics(sup);
                    scripts_ascent = scripts_ascent.max(sup_ascent);
                    scripts_descent = scripts_descent.max(sub_descent);
                    let mut sub_script_shift = subscript_shift_down
                        .max(base.descent + subscript_baseline_drop_min)
                        .max(sub_ascent - subscript_top_max);
                    let mut sup_script_shift = shift_up
                        .max(base.ascent - superscript_baseline_drop_max)
                        .max(superscript_bottom_min + sup_descent);
                    let mut gap = (sub_script_shift - sub_ascent) + (sup_script_shift - sup_descent);
                    if gap < sub_superscript_gap_min {
                        let mut delta =
                            superscript_bottom_max_with_subscript - (sup_script_shift - sup_descent);
                        if delta > 0.0 {
                            delta = delta.min(sub_superscript_gap_min - gap);
                            sup_script_shift += delta;
                            gap += delta;
                        }
                        if gap < sub_superscript_gap_min {
                            sub_script_shift += sub_superscript_gap_min - gap;
                        }
                    }
                    sub_shift = sub_shift.max(sub_script_shift);
                    sup_shift = sup_shift.max(sup_script_shift);
                }
            }
        }
        let ascent = base.ascent.max(scripts_ascent + sup_shift).max(0.0);
        let descent = base.descent.max(scripts_descent + sub_shift).max(0.0);

        // The width, from the intrinsic sizes (Blink's `ComputeMinMaxSizes`), which leave out
        // the italic correction.
        let mut width = base.width;
        match kind {
            ScriptKind::Sub | ScriptKind::Super | ScriptKind::Under | ScriptKind::Over => {
                let script = pairs[0].0.as_ref().or(pairs[0].1.as_ref());
                width += script.map_or(0.0, |script| script.width) + space;
            }
            _ => {
                let mut pair_width = 0.0_f32;
                for (sub, sup) in &pairs {
                    let Some(sub) = sub else {
                        continue;
                    };
                    pair_width = pair_width.max(sub.width);
                    let Some(sup) = sup else {
                        continue;
                    };
                    pair_width = pair_width.max(sup.width);
                    width += pair_width + space;
                }
            }
        }

        let base_italic = base.width.min(base.italic);
        let mut laid = Laid {
            width,
            ascent,
            descent,
            ..Laid::default()
        };
        let mut x = 0.0;
        let first_prescript = match first_prescript {
            Some(first) => {
                for (sub, sup) in pairs
                    .iter_mut()
                    .skip(first)
                    .filter(|(sub, sup)| sub.is_some() || sup.is_some())
                {
                    let (_, _, sub_width) = metrics(sub);
                    let (_, _, sup_width) = metrics(sup);
                    x += space + sub_width.max(sup_width);
                    if let Some(sub) = sub.take() {
                        let sub_x = x - sub.width;
                        laid.place(sub, sub_x, sub_shift);
                    }
                    if let Some(sup) = sup.take() {
                        let sup_x = x - sup.width;
                        laid.place(sup, sup_x, -sup_shift);
                    }
                }
                first
            }
            None => pairs.len(),
        };
        let base_width = base.width;
        laid.place(base, x, 0.0);
        if let Some(prescripts) = prescripts {
            laid.place(prescripts, x, 0.0);
        }
        x += base_width;
        for (sub, sup) in pairs
            .into_iter()
            .take(first_prescript)
            .filter(|(sub, sup)| sub.is_some() || sup.is_some())
        {
            let (_, _, sub_width) = metrics(&sub);
            let (_, _, sup_width) = metrics(&sup);
            if let Some(sub) = sub {
                laid.place(sub, (x - base_italic).max(0.0), sub_shift);
            }
            if let Some(sup) = sup {
                laid.place(sup, x, -sup_shift);
            }
            x += space + sub_width.max(sup_width);
        }
        laid
    }

    /// Limits under and over a base (Blink's `MathUnderOverLayoutAlgorithm`): stacked and
    /// centered, stretchy operators stretched to the widest of them.
    #[allow(clippy::too_many_lines, reason = "one algorithm, as in Blink")]
    fn under_over(
        &self,
        node: &MathNode,
        kind: ScriptKind,
        (accent, accent_under): (bool, bool),
        target: Target,
    ) -> Laid {
        let style = &node.style;
        let base_node = &node.children[0];
        let (under_node, over_node) = match kind {
            ScriptKind::Under => (node.children.get(1), None),
            ScriptKind::Over => (None, node.children.get(1)),
            _ => (node.children.get(1), node.children.get(2)),
        };
        let base_properties = embellished(base_node);
        let large = base_properties.is_some_and(|properties| properties.largeop);
        let stretchy_inline =
            base_properties.is_some_and(|properties| properties.stretchy && !properties.vertical);
        let rule = self.rule_fallback(style);
        let parameter = |constant, fallback| self.constant(style, constant, fallback);
        let (
            under_gap_min,
            over_gap_min,
            under_shift_min,
            over_shift_min,
            under_extra,
            over_extra,
            accent_base,
            bar,
        ) = if large {
            (
                parameter(MathConstant::LowerLimitGapMin, 0.0),
                parameter(MathConstant::UpperLimitGapMin, 0.0),
                parameter(MathConstant::LowerLimitBaselineDropMin, 0.0),
                parameter(MathConstant::UpperLimitBaselineRiseMin, 0.0),
                0.0,
                0.0,
                0.0,
                false,
            )
        } else if stretchy_inline {
            (
                parameter(MathConstant::StretchStackGapBelowMin, 0.0),
                parameter(MathConstant::StretchStackGapAboveMin, 0.0),
                parameter(MathConstant::StretchStackBottomShiftDown, 0.0),
                parameter(MathConstant::StretchStackTopShiftUp, 0.0),
                0.0,
                0.0,
                0.0,
                false,
            )
        } else {
            (
                parameter(MathConstant::UnderbarVerticalGap, 3.0 * rule),
                parameter(MathConstant::OverbarVerticalGap, 3.0 * rule),
                0.0,
                0.0,
                parameter(MathConstant::UnderbarExtraDescender, rule),
                parameter(MathConstant::OverbarExtraAscender, rule),
                parameter(MathConstant::AccentBaseHeight, self.x_height(style) / 2.0),
                true,
            )
        };
        let inherits_block = target.block.is_some();
        let inherits_inline = !inherits_block && target.inline.is_some();
        // The width stretchy operators stretch to: that of the widest other child.
        let mut stretch_width = 0.0_f32;
        let children: Vec<&MathNode> = std::iter::once(base_node)
            .chain(under_node)
            .chain(over_node)
            .collect();
        let mut any = false;
        for child in &children {
            if stretchy(child, false) {
                continue;
            }
            stretch_width = stretch_width.max(self.layout(child, Target::default()).width);
            any = true;
        }
        if !any {
            for child in &children {
                if std::ptr::eq(*child, base_node) && (inherits_block || inherits_inline) {
                    continue;
                }
                let laid = self.layout(
                    child,
                    Target {
                        block: None,
                        inline: Some(0.0),
                    },
                );
                stretch_width = stretch_width.max(laid.width);
            }
        }
        let child_target = |child: &MathNode, is_base: bool| {
            if is_base && inherits_block && stretchy(child, true) {
                target
            } else if is_base && inherits_inline && stretchy(child, false) {
                Target {
                    block: None,
                    inline: target.inline,
                }
            } else if (!is_base || (!inherits_block && !inherits_inline)) && stretchy(child, false) {
                Target {
                    block: None,
                    inline: Some(stretch_width),
                }
            } else {
                Target::default()
            }
        };
        let base = self.layout(base_node, child_target(base_node, true));
        let under = under_node.map(|child| self.layout(child, child_target(child, false)));
        let over = over_node.map(|child| self.layout(child, child_target(child, false)));
        // The width from the intrinsic sizes: the widest child, stretchy ones unstretched.
        let width = children
            .iter()
            .map(|child| {
                if stretchy(child, false) {
                    self.layout(child, Target::default()).width
                } else {
                    match (std::ptr::eq(*child, base_node), under_node, over_node) {
                        (true, ..) => base.width,
                        (false, Some(under_node), _) if std::ptr::eq(*child, under_node) => {
                            under.as_ref().map_or(0.0, |under| under.width)
                        }
                        _ => over.as_ref().map_or(0.0, |over| over.width),
                    }
                }
            })
            .fold(0.0, f32::max);
        // The scripts and base are centered in the width CSS gives the box.
        let width = content_width(style, width);

        let mut laid = Laid {
            width,
            ..Laid::default()
        };
        // From the top: the over script, the base, the under script.
        let mut ascent = 0.0_f32;
        let mut placed: Vec<(Laid, f32, f32)> = Vec::new();
        if let Some(over) = over {
            ascent += over_extra;
            let over_top = ascent;
            let over_height = over.ascent + over.descent;
            if bar {
                ascent += over_height;
                if accent {
                    if base.ascent < accent_base {
                        ascent += accent_base - base.ascent;
                    }
                } else {
                    ascent += over_gap_min;
                }
            } else {
                ascent += (over_height + over_gap_min).max(over.ascent + over_shift_min);
            }
            let x = half(width - over.width);
            let over_ascent = over.ascent;
            placed.push((over, x, over_top + over_ascent));
        }
        let base_top = ascent;
        ascent += base.ascent;
        ascent = ascent.max(0.0);
        let mut descent = base.descent;
        let base_x = half(width - base.width);
        let base_ascent = base.ascent;
        placed.push((base, base_x, base_top + base_ascent));
        if let Some(under) = under {
            if bar {
                if !accent_under {
                    descent += under_gap_min;
                }
            } else {
                descent += under_gap_min.max(under_shift_min - under.ascent);
            }
            let under_top = ascent + descent;
            descent += under.ascent + under.descent + under_extra;
            let x = half(width - under.width);
            let under_ascent = under.ascent;
            placed.push((under, x, under_top + under_ascent));
        }
        laid.ascent = ascent;
        laid.descent = descent.max(0.0);
        for (child, x, baseline_from_top) in placed {
            laid.place(child, x, baseline_from_top - ascent);
        }
        laid
    }

    /// `<mtable>`, which Chrome lays out as a CSS table (see [`super::table`]): cells in the
    /// slots of a grid, as wide as their columns and as tall as their rows, their content
    /// aligned on the baseline of their first row and placed in their columns as `text-align`
    /// says; the table centered on the math axis.
    #[allow(clippy::too_many_lines, reason = "one algorithm, as in Blink")]
    fn table(&self, node: &MathNode) -> Laid {
        use super::table::{CellBlock, Slot, column_widths, rows, slots};
        // The rows, each of its cells; children of the table that are no rows stand in rows of
        // their own, as do children of a row that are no cells in cells.
        let grid: Vec<Vec<&MathNode>> = node
            .children
            .iter()
            .map(|row| {
                if matches!(row.kind, Kind::TableRow) {
                    row.children.iter().collect()
                } else {
                    vec![row]
                }
            })
            .collect();
        let spans: Vec<Vec<(usize, usize)>> = grid
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| match cell.kind {
                        Kind::TableCell {
                            column_span,
                            row_span,
                        } => (column_span, row_span),
                        _ => (1, 1),
                    })
                    .collect()
            })
            .collect();
        let placed = slots(&spans);
        let cells: Vec<TableCell<'_>> = grid
            .iter()
            .flatten()
            .zip(placed)
            .map(|(&cell, slot)| {
                if matches!(cell.kind, Kind::TableCell { .. }) {
                    let row = cell.children.first();
                    TableCell {
                        node: cell,
                        slot,
                        content: row
                            .map(|row| self.layout(row, Target::default()))
                            .unwrap_or_default(),
                        frame: frame(&cell.style),
                        filled: row.is_some_and(|row| !row.children.is_empty()),
                    }
                } else {
                    TableCell {
                        node: cell,
                        slot,
                        content: self.layout(cell, Target::default()),
                        frame: [0.0; 4],
                        filled: true,
                    }
                }
            })
            .collect();
        let columns = column_widths(
            &cells
                .iter()
                .map(|cell| {
                    let [_, right, _, left] = cell.frame;
                    (cell.slot, cell.content.width + left + right)
                })
                .collect::<Vec<_>>(),
        );
        let blocks: Vec<CellBlock> = cells
            .iter()
            .map(|cell| {
                let [top, _, bottom, _] = cell.frame;
                CellBlock {
                    slot: cell.slot,
                    height: top + cell.content.ascent + cell.content.descent + bottom,
                    baseline: cell.filled.then_some(top + cell.content.ascent),
                    bottom,
                }
            })
            .collect();
        let rows = rows(grid.len(), &blocks);
        let lefts: Vec<f32> = columns
            .iter()
            .scan(0.0, |x, width| {
                let left = *x;
                *x += width;
                Some(left)
            })
            .collect();
        let tops: Vec<f32> = rows
            .iter()
            .scan(0.0, |y, (height, _)| {
                let top = *y;
                *y += height;
                Some(top)
            })
            .collect();
        let width: f32 = columns.iter().sum();
        let height: f32 = rows.iter().map(|(height, _)| height).sum();
        // The items, from the table's top.
        let mut laid = Laid {
            width,
            ..Laid::default()
        };
        for cell in cells {
            let Slot {
                row,
                column,
                rows: row_count,
                columns: column_count,
            } = cell.slot;
            let cell_width: f32 = columns[column..column + column_count].iter().sum();
            let cell_height: f32 = rows[row..row + row_count].iter().map(|(height, _)| height).sum();
            let [top, right, _, left] = cell.frame;
            let style = &cell.node.style;
            let mut own = Laid {
                width: cell_width,
                ..Laid::default()
            };
            if matches!(cell.node.kind, Kind::TableCell { .. })
                && decorated(style)
                && style.visibility == Visibility::Visible
            {
                own.items.push(Item::Frame {
                    x: 0.0,
                    y: 0.0,
                    width: cell_width,
                    height: cell_height,
                    style: Arc::clone(style),
                });
            }
            // The content on the baseline of the first row, moved along it in the room the
            // padding and border leave; right to left, from the right (see `Laid::mirror`).
            let start = if style.is_rtl() { right } else { left };
            let room = cell_width - left - right - cell.content.width;
            let x = start + CellAlign::of(style).offset(room);
            let baseline = if cell.filled {
                rows[row].1
            } else {
                top + cell.content.ascent
            };
            own.place(cell.content, x, baseline);
            if matches!(cell.node.kind, Kind::TableCell { .. }) && cell.node.mirrored {
                own.mirror();
            }
            laid.place(own, lefts[column], tops[row]);
        }
        let baseline = half(height) + self.axis(&node.style);
        laid.ascent = baseline;
        laid.descent = height - baseline;
        laid.items = std::mem::take(&mut laid.items)
            .into_iter()
            .map(|item| item.moved(0.0, -baseline))
            .collect();
        laid
    }
}

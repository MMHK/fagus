//! Computed values.
//!
//! Lengths are in CSS pixels. Percentages stay unresolved until layout knows what they refer to;
//! `calc()` with percentages is kept as an expression for the same reason.

use std::sync::Arc;

pub use fagus_css::hanging::{HangingEnd, HangingPunctuation};

/// A color in sRGB, each channel between 0 and 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    pub const TRANSPARENT: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    };
    pub const BLACK: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };
    pub const WHITE: Self = Self {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };

    pub fn from_u8(r: u8, g: u8, b: u8, a: f32) -> Self {
        Self {
            r: f32::from(r) / 255.0,
            g: f32::from(g) / 255.0,
            b: f32::from(b) / 255.0,
            a,
        }
    }

    pub fn is_transparent(&self) -> bool {
        self.a <= 0.0
    }
}

/// A node of a `calc()` expression that mixes lengths and percentages.
#[derive(Clone, Debug, PartialEq)]
pub enum CalcNode {
    Px(f32),
    /// A fraction of the percentage basis (0.5 = 50%).
    Percent(f32),
    Sum(Box<CalcNode>, Box<CalcNode>),
    Product(f32, Box<CalcNode>),
    Min(Vec<CalcNode>),
    Max(Vec<CalcNode>),
    Clamp(Box<CalcNode>, Box<CalcNode>, Box<CalcNode>),
    Abs(Box<CalcNode>),
    /// `round()`: the value to a multiple of the step.
    Round(Rounding, Box<CalcNode>, Box<CalcNode>),
    /// `mod()`: the remainder with the sign of the divisor.
    Mod(Box<CalcNode>, Box<CalcNode>),
    /// `rem()`: the remainder with the sign of the dividend.
    Rem(Box<CalcNode>, Box<CalcNode>),
    Hypot(Vec<CalcNode>),
}

/// How `round()` rounds (CSS Values 4, §10.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rounding {
    /// To the nearer multiple, the upper one where both are as near.
    Nearest,
    Up,
    Down,
    ToZero,
}

impl Rounding {
    /// `value` rounded to a multiple of `step`; NaN for a step of zero, as CSS says.
    pub fn round(self, value: f32, step: f32) -> f32 {
        if step.abs() < f32::MIN_POSITIVE {
            return f32::NAN;
        }
        let multiples = value / step.abs();
        step.abs()
            * match self {
                Self::Nearest => (multiples + 0.5).floor(),
                Self::Up => multiples.ceil(),
                Self::Down => multiples.floor(),
                Self::ToZero => multiples.trunc(),
            }
    }
}

/// A calculation's result, 0 for NaN (CSS Values 4, §10.9).
pub(crate) fn censored(value: f32) -> f32 {
    if value.is_nan() { 0.0 } else { value }
}

/// `mod(a, b)`: `a` minus the multiple of `b` below it, with the sign of `b`.
pub fn css_mod(a: f32, b: f32) -> f32 {
    a - b * (a / b).floor()
}

/// `rem(a, b)`: `a` minus the multiple of `b` toward zero, with the sign of `a`.
pub fn css_rem(a: f32, b: f32) -> f32 {
    a - b * (a / b).trunc()
}

impl CalcNode {
    pub fn resolve(&self, basis: f32) -> f32 {
        match self {
            Self::Px(px) => *px,
            Self::Percent(fraction) => fraction * basis,
            Self::Sum(a, b) => a.resolve(basis) + b.resolve(basis),
            Self::Product(factor, node) => factor * node.resolve(basis),
            Self::Min(nodes) => nodes
                .iter()
                .map(|node| node.resolve(basis))
                .fold(f32::INFINITY, f32::min),
            Self::Max(nodes) => nodes
                .iter()
                .map(|node| node.resolve(basis))
                .fold(f32::NEG_INFINITY, f32::max),
            Self::Clamp(min, value, max) => value
                .resolve(basis)
                .min(max.resolve(basis))
                .max(min.resolve(basis)),
            Self::Abs(node) => node.resolve(basis).abs(),
            Self::Round(rounding, value, step) => rounding.round(value.resolve(basis), step.resolve(basis)),
            Self::Mod(a, b) => css_mod(a.resolve(basis), b.resolve(basis)),
            Self::Rem(a, b) => css_rem(a.resolve(basis), b.resolve(basis)),
            Self::Hypot(nodes) => nodes
                .iter()
                .map(|node| node.resolve(basis).powi(2))
                .sum::<f32>()
                .sqrt(),
        }
    }

    fn has_percent(&self) -> bool {
        match self {
            Self::Px(_) => false,
            Self::Percent(_) => true,
            Self::Sum(a, b) => a.has_percent() || b.has_percent(),
            Self::Product(_, node) => node.has_percent(),
            Self::Min(nodes) | Self::Max(nodes) | Self::Hypot(nodes) => nodes.iter().any(Self::has_percent),
            Self::Clamp(a, b, c) => a.has_percent() || b.has_percent() || c.has_percent(),
            Self::Abs(node) => node.has_percent(),
            Self::Round(_, a, b) | Self::Mod(a, b) | Self::Rem(a, b) => a.has_percent() || b.has_percent(),
        }
    }
}

/// A `<length-percentage>`.
#[derive(Clone, Debug, PartialEq)]
pub enum LengthPercentage {
    Px(f32),
    /// A fraction of the percentage basis (0.5 = 50%).
    Percent(f32),
    Calc(Arc<CalcNode>),
}

impl LengthPercentage {
    pub const ZERO: Self = Self::Px(0.0);

    /// The used value, given what percentages refer to. A calculation that comes to NaN (a
    /// rounding to a step of zero) comes to 0, as CSS says.
    pub fn resolve(&self, basis: f32) -> f32 {
        match self {
            Self::Px(px) => *px,
            Self::Percent(fraction) => fraction * basis,
            Self::Calc(node) => censored(node.resolve(basis)),
        }
    }

    /// The value if it does not depend on a percentage basis.
    pub fn fixed(&self) -> Option<f32> {
        match self {
            Self::Px(px) => Some(*px),
            Self::Percent(_) => None,
            Self::Calc(node) => (!node.has_percent()).then(|| censored(node.resolve(0.0))),
        }
    }

    /// The value clamped at zero, as the used value of a property that takes no negative
    /// values (a padding) whose calculation comes out negative.
    #[must_use]
    pub fn non_negative(self) -> Self {
        match self {
            Self::Px(px) => Self::Px(px.max(0.0)),
            Self::Percent(fraction) => Self::Percent(fraction.max(0.0)),
            Self::Calc(node) => Self::Calc(Arc::new(CalcNode::Max(vec![
                Arc::unwrap_or_clone(node),
                CalcNode::Px(0.0),
            ]))),
        }
    }

    pub(crate) fn from_node(node: CalcNode) -> Self {
        match node {
            CalcNode::Px(px) => Self::Px(censored(px)),
            CalcNode::Percent(fraction) => Self::Percent(censored(fraction)),
            node if !node.has_percent() => Self::Px(censored(node.resolve(0.0))),
            node => Self::Calc(Arc::new(node)),
        }
    }
}

/// A `<length-percentage>` or `auto`.
#[derive(Clone, Debug, PartialEq)]
pub enum LengthPercentageAuto {
    Auto,
    Length(LengthPercentage),
}

impl LengthPercentageAuto {
    pub const ZERO: Self = Self::Length(LengthPercentage::ZERO);

    pub fn is_auto(&self) -> bool {
        matches!(self, Self::Auto)
    }

    /// The used value, with `auto` resolved to `auto_value`.
    pub fn resolve_or(&self, basis: f32, auto_value: f32) -> f32 {
        match self {
            Self::Auto => auto_value,
            Self::Length(length) => length.resolve(basis),
        }
    }
}

/// A preferred or minimum size (`width`, `min-height`, …).
#[derive(Clone, Debug, PartialEq)]
pub enum Size {
    Auto,
    Length(LengthPercentage),
    MinContent,
    MaxContent,
    FitContent(Option<LengthPercentage>),
    /// `stretch` (`-webkit-fill-available`): the margin box fills the containing block.
    Stretch,
}

/// A maximum size (`max-width`, …).
#[derive(Clone, Debug, PartialEq)]
pub enum MaxSize {
    None,
    Length(LengthPercentage),
    MinContent,
    MaxContent,
    FitContent(Option<LengthPercentage>),
    /// `stretch` (`-webkit-fill-available`): at most what fills the containing block.
    Stretch,
}

/// Four values for the sides of a box.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Sides<T> {
    pub top: T,
    pub right: T,
    pub bottom: T,
    pub left: T,
}

impl<T: Clone> Sides<T> {
    pub fn all(value: T) -> Self {
        Self {
            top: value.clone(),
            right: value.clone(),
            bottom: value.clone(),
            left: value,
        }
    }
}

/// Four corner radii, each with a horizontal and a vertical length.
#[derive(Clone, Debug, PartialEq)]
pub struct Corners {
    pub top_left: (LengthPercentage, LengthPercentage),
    pub top_right: (LengthPercentage, LengthPercentage),
    pub bottom_right: (LengthPercentage, LengthPercentage),
    pub bottom_left: (LengthPercentage, LengthPercentage),
}

impl Default for Corners {
    fn default() -> Self {
        let zero = (LengthPercentage::ZERO, LengthPercentage::ZERO);
        Self {
            top_left: zero.clone(),
            top_right: zero.clone(),
            bottom_right: zero.clone(),
            bottom_left: zero,
        }
    }
}

impl Corners {
    pub fn is_zero(&self) -> bool {
        [
            &self.top_left,
            &self.top_right,
            &self.bottom_right,
            &self.bottom_left,
        ]
        .iter()
        .all(|(x, y)| x.fixed() == Some(0.0) && y.fixed() == Some(0.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DisplayOutside {
    #[default]
    Inline,
    Block,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DisplayInside {
    #[default]
    Flow,
    FlowRoot,
    Table,
    Flex,
    Grid,
}

/// Layout-internal display types of tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InternalDisplay {
    TableRowGroup,
    TableHeaderGroup,
    TableFooterGroup,
    TableRow,
    TableCell,
    TableColumnGroup,
    TableColumn,
    TableCaption,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Display {
    None,
    Contents,
    Normal {
        outside: DisplayOutside,
        inside: DisplayInside,
        list_item: bool,
    },
    Internal(InternalDisplay),
}

impl Default for Display {
    fn default() -> Self {
        Self::INLINE
    }
}

impl Display {
    pub const INLINE: Self = Self::Normal {
        outside: DisplayOutside::Inline,
        inside: DisplayInside::Flow,
        list_item: false,
    };
    pub const BLOCK: Self = Self::Normal {
        outside: DisplayOutside::Block,
        inside: DisplayInside::Flow,
        list_item: false,
    };

    pub fn is_block_level(self) -> bool {
        matches!(
            self,
            Self::Normal {
                outside: DisplayOutside::Block,
                ..
            } | Self::Internal(_)
        )
    }

    /// The blockified display of an element that becomes block-level (floats, absolutely
    /// positioned boxes, flex and grid items).
    #[must_use]
    pub fn blockify(self) -> Self {
        match self {
            Self::Normal {
                inside, list_item, ..
            } => Self::Normal {
                outside: DisplayOutside::Block,
                inside,
                list_item,
            },
            Self::Internal(_) => Self::BLOCK,
            other => other,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Position {
    #[default]
    Static,
    Relative,
    Absolute,
    Fixed,
    /// Positioned, but on paper nothing scrolls, so the box stays where it is.
    Sticky,
}

/// `footnote-display` (CSS Generated Content for Paged Media 3, §2.3): how a footnote is laid
/// out in the footnote area.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FootnoteDisplay {
    /// A block of its own.
    #[default]
    Block,
    /// Inline, on the lines of the footnotes before and after it.
    Inline,
    /// Inline where it fits on one line with the footnotes beside it, else a block.
    Compact,
}

/// `footnote-policy` (CSS Generated Content for Paged Media 3, §2.6): what happens where a
/// footnote does not fit on the page of its call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FootnotePolicy {
    /// The footnote moves to the next page; the call stays.
    #[default]
    Auto,
    /// The line of the call moves to the next page with the footnote.
    Line,
    /// The block of the call moves to the next page with the footnote.
    Block,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Float {
    #[default]
    None,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Clear {
    #[default]
    None,
    Left,
    Right,
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BoxSizing {
    #[default]
    ContentBox,
    BorderBox,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BorderStyle {
    #[default]
    None,
    Hidden,
    Solid,
    Dashed,
    Dotted,
    Double,
    Groove,
    Ridge,
    Inset,
    Outset,
}

impl BorderStyle {
    /// Whether a border with this style has a width (`none` and `hidden` compute to 0).
    pub fn is_visible(self) -> bool {
        !matches!(self, Self::None | Self::Hidden)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Overflow {
    #[default]
    Visible,
    Hidden,
    Clip,
}

/// The scroll bar `overflow` gives a box along an axis: none for `visible`, `hidden` and `clip`,
/// one where needed for `auto`, always one for `scroll` (all three of the latter are
/// [`Overflow::Hidden`] otherwise). Chrome prints no scroll bars, but a `<textarea>` keeps
/// room for them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Scrollbar {
    #[default]
    None,
    Auto,
    Scroll,
}

/// A shadow of text (`text-shadow`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextShadow {
    pub x: f32,
    pub y: f32,
    /// The blur radius; the standard deviation of the Gaussian is half of it.
    pub blur: f32,
    /// `None` means `currentColor`.
    pub color: Option<Rgba>,
}

/// `outline-style`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutlineStyle {
    /// A line in one of the styles of borders.
    Line(BorderStyle),
    /// The focus ring of the browser: in Chrome, a black ring with a white one around it.
    Auto,
}

impl Default for OutlineStyle {
    fn default() -> Self {
        Self::Line(BorderStyle::None)
    }
}

/// `font-palette`: which palette of a color font its glyphs take (CSS Fonts 4, §9.1).
#[derive(Clone, Debug, PartialEq, Default)]
pub enum FontPalette {
    /// The font's first palette.
    #[default]
    Normal,
    /// The first palette for light backgrounds.
    Light,
    /// The first palette for dark backgrounds.
    Dark,
    /// The palette of the `@font-palette-values` rules of this name.
    Named(Arc<str>),
    /// `palette-mix()`: two palettes mixed color by color, with the share of the second from
    /// 0 to 1.
    Mix(Arc<(FontPalette, FontPalette, f32)>),
}

/// `text-overflow`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum TextOverflow {
    #[default]
    Clip,
    /// A line that overflows its box ends with an ellipsis, in place of what does not fit.
    Ellipsis,
    /// As `Ellipsis`, with this text in place of the ellipsis (CSS Overflow 4, §5.1), its line
    /// breaks made spaces.
    String(Arc<str>),
}

impl TextOverflow {
    /// Whether a line that overflows its box is cut to end with an ellipsis or a string.
    pub fn ellipsizes(&self) -> bool {
        !matches!(self, Self::Clip)
    }
}

/// `appearance`: a form control with its native look, or styled by CSS alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Appearance {
    #[default]
    Auto,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Visibility {
    #[default]
    Visible,
    Hidden,
    Collapse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum FontStyle {
    #[default]
    Normal,
    Italic,
    /// Slanted by an angle in tenths of a degree (`oblique` alone is 14 degrees).
    Oblique(i16),
}

impl FontStyle {
    /// `oblique` by `degrees`, within the -90 to 90 degrees that CSS allows.
    pub fn oblique(degrees: f32) -> Self {
        let tenths = (degrees.clamp(-90.0, 90.0) * 10.0).round();
        Self::Oblique(if tenths.is_finite() { tenths as i16 } else { 140 })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum LineHeight {
    #[default]
    Normal,
    /// A multiple of the font size.
    Number(f32),
    Px(f32),
}

/// `direction`: the inline base direction of a box's text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Direction {
    #[default]
    Ltr,
    Rtl,
}

/// `writing-mode`: whether lines are horizontal or vertical and which way they stack (CSS
/// Writing Modes 4, §3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
pub enum WritingMode {
    /// Lines run left to right and stack top to bottom.
    #[default]
    HorizontalTb,
    /// Lines run top to bottom and stack right to left.
    VerticalRl,
    /// Lines run top to bottom and stack left to right.
    VerticalLr,
    /// Like `vertical-rl`, with all text set sideways, turned a quarter clockwise.
    SidewaysRl,
    /// Lines run bottom to top and stack left to right, with all text set sideways, turned a
    /// quarter counterclockwise.
    SidewaysLr,
}

impl WritingMode {
    /// Whether lines are vertical.
    pub fn is_vertical(self) -> bool {
        self != Self::HorizontalTb
    }

    /// Whether all text is set sideways, whatever its `text-orientation`.
    pub fn is_sideways(self) -> bool {
        matches!(self, Self::SidewaysRl | Self::SidewaysLr)
    }

    /// Whether two writing modes have their lines in the same direction: both horizontal or
    /// both vertical. Boxes whose are not are orthogonal (CSS Writing Modes 4, §7).
    pub fn is_parallel(self, other: Self) -> bool {
        self.is_vertical() == other.is_vertical()
    }
}

/// `text-orientation`: how characters stand in vertical lines (CSS Writing Modes 4, §5.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextOrientation {
    /// Characters of horizontal scripts are turned sideways, those of vertical scripts stand
    /// upright.
    #[default]
    Mixed,
    /// All characters stand upright.
    Upright,
    /// All characters are turned sideways, like a horizontal line turned a quarter clockwise.
    Sideways,
}

/// `unicode-bidi`: how a box takes part in the bidirectional algorithm (CSS Writing Modes 3,
/// §2.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum UnicodeBidi {
    #[default]
    Normal,
    Embed,
    Isolate,
    BidiOverride,
    IsolateOverride,
    Plaintext,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextAlign {
    #[default]
    Start,
    End,
    Left,
    Right,
    Center,
    Justify,
}

/// `tab-size`: the distance between tab stops.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TabSize {
    /// A number of spaces of the block's font.
    Spaces(f32),
    Px(f32),
}

impl Default for TabSize {
    fn default() -> Self {
        Self::Spaces(8.0)
    }
}

/// `bookmark-level` (CSS GCPM 3, §5): whether an element makes an entry in the PDF outline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BookmarkLevel {
    /// Not declared: no entry, except for an element with the ARIA role `heading`, which makes
    /// one at its `aria-level` as in Chrome's outline.
    #[default]
    Auto,
    /// `none`: no entry, whatever the element's role.
    None,
    /// An entry at this level, 1 for the top.
    Level(u8),
}

/// `empty-cells`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum EmptyCells {
    #[default]
    Show,
    /// Cells without content draw no borders and backgrounds (with separate borders).
    Hide,
}

/// `font-variant-caps`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FontVariantCaps {
    #[default]
    Normal,
    SmallCaps,
    AllSmallCaps,
    PetiteCaps,
    AllPetiteCaps,
    Unicase,
    TitlingCaps,
}

/// `font-variant-position`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FontVariantPosition {
    #[default]
    Normal,
    Sub,
    Super,
}

/// `font-kerning`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FontKerning {
    /// As the browser sees fit: kerned, as in Chrome.
    #[default]
    Auto,
    Normal,
    None,
}

/// Whether a kind of ligature is asked for, turned off, or left to the font.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Ligatures {
    #[default]
    Normal,
    On,
    Off,
}

/// The `font-variant-numeric` keywords, in the order of [`FontVariant::numeric`]'s bits, with
/// their OpenType features.
pub const NUMERIC_FEATURES: [(&str, [u8; 4]); 8] = [
    ("lining-nums", *b"lnum"),
    ("oldstyle-nums", *b"onum"),
    ("proportional-nums", *b"pnum"),
    ("tabular-nums", *b"tnum"),
    ("diagonal-fractions", *b"frac"),
    ("stacked-fractions", *b"afrc"),
    ("ordinal", *b"ordn"),
    ("slashed-zero", *b"zero"),
];

/// The `font-variant-east-asian` keywords, in the order of [`FontVariant::east_asian`]'s bits,
/// with their OpenType features.
pub const EAST_ASIAN_FEATURES: [(&str, [u8; 4]); 9] = [
    ("jis78", *b"jp78"),
    ("jis83", *b"jp83"),
    ("jis90", *b"jp90"),
    ("jis04", *b"jp04"),
    ("simplified", *b"smpl"),
    ("traditional", *b"trad"),
    ("full-width", *b"fwid"),
    ("proportional-width", *b"pwid"),
    ("ruby", *b"ruby"),
];

/// Where a computed font size comes from, for the size Chrome gives text in the generic
/// `monospace` family alone: its default fixed font size, 13px where the default size is 16px.
/// A size from a keyword or from em or percentages of one is taken again from the fixed sizes,
/// or scaled by 13/16, where the family turns to or from `monospace`; a size from an absolute
/// length is kept (Blink's `FontBuilder::CheckForGenericFamilyChange`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontSizeBasis {
    /// The keyword the size is, from `xx-small` (0) to `xxx-large` (7).
    pub keyword: Option<u8>,
    /// Whether the size comes from a length that does not depend on the parent's size.
    pub absolute: bool,
    /// Whether the font family is the generic `monospace` alone.
    pub monospace: bool,
}

impl Default for FontSizeBasis {
    /// The initial size: `medium`, in a family other than `monospace`.
    fn default() -> Self {
        Self {
            keyword: Some(3),
            absolute: false,
            monospace: false,
        }
    }
}

/// The longhands of `font-variant` (CSS Fonts 4, §6), each one inherited on its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct FontVariant {
    /// `font-variant-ligatures`: common (`liga`, `clig`), discretionary (`dlig`), historical
    /// (`hlig`) and contextual ligatures (`calt`).
    pub ligatures: [Ligatures; 4],
    pub caps: FontVariantCaps,
    /// `font-variant-numeric`: a bit for each of [`NUMERIC_FEATURES`].
    pub numeric: u8,
    /// `font-variant-east-asian`: a bit for each of [`EAST_ASIAN_FEATURES`].
    pub east_asian: u16,
    /// `font-variant-alternates: historical-forms`; the other alternates need
    /// `@font-feature-values`.
    pub historical_forms: bool,
    pub position: FontVariantPosition,
}

/// `font-synthesis`: which faces browsers may make up where a family lacks them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools, reason = "one switch per longhand")]
pub struct FontSynthesis {
    pub weight: bool,
    pub style: bool,
    pub small_caps: bool,
}

impl Default for FontSynthesis {
    fn default() -> Self {
        Self {
            weight: true,
            style: true,
            small_caps: true,
        }
    }
}

/// The metric of a font that `font-size-adjust` keeps in proportion to the font size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FontMetric {
    #[default]
    ExHeight,
    CapHeight,
    ChWidth,
    IcWidth,
    IcHeight,
}

/// The metrics of a font that the font-relative lengths `ex`, `ch`, `cap` and `ic` refer to, in
/// ems (CSS Values 4, §6.1.1): its x-height, the advance of its "0", its cap height and the
/// advance of its water ideograph. Without a font they take the values the specification
/// suggests: half an em for `ex` and `ch`, an em for `ic`; Chrome takes 0.7 em for `cap`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontUnits {
    pub ex: f32,
    pub ch: f32,
    pub cap: f32,
    pub ic: f32,
    /// `scriptPercentScaleDown` and `scriptScriptPercentScaleDown` of the font's `MATH` table
    /// as fractions, which `font-size: math` scales by; `None` for a font without one.
    pub math_scale: Option<(f32, f32)>,
    /// The advance of the "0" set upright in vertical text, where the font has vertical
    /// metrics; without them, the font's ascent and descent (`ascent`, `descent`) together,
    /// each rounded to whole pixels, as Chrome advances upright glyphs.
    pub ch_upright: Option<f32>,
    pub ascent: f32,
    pub descent: f32,
    /// Whether Chrome makes the rounded ascent larger by 15 % of the rounded ascent and descent
    /// together, rounded (the installed Times, Helvetica and Courier of macOS), for `ch` in
    /// upright text too.
    pub ascent_hack: bool,
}

impl FontUnits {
    /// What `ch` is in pixels at `font_size` in vertical text set upright, where it refers to
    /// the advance of the "0" in the vertical axis (CSS Values 4, §6.1.1).
    pub fn upright_ch(&self, font_size: f32) -> f32 {
        self.ch_upright.map_or_else(
            || {
                let ascent = (self.ascent * font_size).round();
                let descent = (self.descent * font_size).round();
                let adjustment = if self.ascent_hack {
                    ((ascent + descent) * 0.15 + 0.5).floor()
                } else {
                    0.0
                };
                ascent + adjustment + descent
            },
            |ch| ch * font_size,
        )
    }
}

impl Default for FontUnits {
    fn default() -> Self {
        Self {
            ex: 0.5,
            ch: 0.5,
            cap: 0.7,
            ic: 1.0,
            math_scale: None,
            ch_upright: Some(1.0),
            ascent: 0.8,
            descent: 0.2,
            ascent_hack: false,
        }
    }
}

/// The fonts of a document as the cascade sees them: the metrics of the first available font
/// for an element's font properties, which its font-relative lengths refer to.
pub trait FontMetrics {
    fn units(&self, request: &FontRequest) -> FontUnits;

    /// The line spacing of the first available font at `size` pixels, which `line-height:
    /// normal` is (and `lh` refers to); `None` without fonts.
    fn line_spacing(&self, request: &FontRequest, size: f32) -> Option<f32> {
        let _ = (request, size);
        None
    }
}

/// No fonts: font-relative lengths take the values of [`FontUnits::default`].
pub struct NoFontMetrics;

impl FontMetrics for NoFontMetrics {
    fn units(&self, _: &FontRequest) -> FontUnits {
        FontUnits::default()
    }
}

/// The font properties of an element, which select its first available font.
#[derive(Clone, Debug, PartialEq)]
pub struct FontRequest {
    pub families: Arc<[String]>,
    pub weight: f32,
    pub style: FontStyle,
    pub stretch: f32,
    /// The font size, on the `opsz` axis of a variable font with `font-optical-sizing: auto`:
    /// Chrome takes the metrics of the instance at that size (the "0" of SF Pro at 16px is that
    /// of its optical size 17, wider than at its default 28). `None` keeps the axis's default.
    pub optical_size: Option<f32>,
    /// The language of the element (its `lang`), which picks the family that a generic family
    /// stands for: Chrome's standard font for `lang="ja"` on macOS is Hiragino Kaku Gothic,
    /// whose "0" is 0.657 em wide.
    pub lang: Option<Arc<str>>,
}

/// `font-size-adjust` (CSS Fonts 5, §2.6).
#[derive(Clone, Debug, PartialEq, Default)]
pub enum FontSizeAdjust {
    #[default]
    None,
    /// The aspect value to keep: the metric divided by the font size.
    Aspect(FontMetric, f32),
    /// `from-font`: the aspect value of the first available font of the element that declared
    /// it, whose font properties the cascade fills in.
    FromFont(FontMetric, Option<Arc<FontRequest>>),
}

/// `font-optical-sizing`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OpticalSizing {
    /// The `opsz` axis of a variable font follows the font size.
    #[default]
    Auto,
    None,
}

/// `text-justify`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextJustify {
    /// Between words.
    #[default]
    Auto,
    /// Not at all: justified text is aligned at the start.
    None,
    /// Between all characters.
    InterCharacter,
}

/// `white-space-collapse`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum WhiteSpaceCollapse {
    #[default]
    Collapse,
    Preserve,
    PreserveBreaks,
    BreakSpaces,
}

/// `white-space` as its longhands (CSS Text 4, §3): `white-space-collapse` and
/// `text-wrap-mode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct WhiteSpace {
    pub collapse: WhiteSpaceCollapse,
    /// `text-wrap-mode: nowrap`.
    pub nowrap: bool,
}

impl WhiteSpace {
    pub const NORMAL: Self = Self::of(WhiteSpaceCollapse::Collapse, false);
    pub const NO_WRAP: Self = Self::of(WhiteSpaceCollapse::Collapse, true);
    pub const PRE: Self = Self::of(WhiteSpaceCollapse::Preserve, true);
    pub const PRE_WRAP: Self = Self::of(WhiteSpaceCollapse::Preserve, false);
    pub const PRE_LINE: Self = Self::of(WhiteSpaceCollapse::PreserveBreaks, false);
    pub const BREAK_SPACES: Self = Self::of(WhiteSpaceCollapse::BreakSpaces, false);

    const fn of(collapse: WhiteSpaceCollapse, nowrap: bool) -> Self {
        Self { collapse, nowrap }
    }

    pub fn collapses_spaces(self) -> bool {
        matches!(
            self.collapse,
            WhiteSpaceCollapse::Collapse | WhiteSpaceCollapse::PreserveBreaks
        )
    }

    pub fn preserves_newlines(self) -> bool {
        self.collapse != WhiteSpaceCollapse::Collapse
    }

    pub fn wraps(self) -> bool {
        !self.nowrap
    }
}

/// `text-wrap-style`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextWrapStyle {
    #[default]
    Auto,
    /// Lines of about the same length, for paragraphs of up to six lines.
    Balance,
    /// No single word on a short last line.
    Pretty,
    Stable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextTransform {
    #[default]
    None,
    Uppercase,
    Lowercase,
    Capitalize,
    /// `math-auto`: a text of a single letter in its mathematical italic form.
    MathAuto,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub enum VerticalAlign {
    #[default]
    Baseline,
    Sub,
    Super,
    Top,
    TextTop,
    Middle,
    /// `-webkit-baseline-middle`: the middle of the box on the parent's baseline (from
    /// `align=middle` on images, as in Chrome).
    BaselineMiddle,
    Bottom,
    TextBottom,
    Length(LengthPercentage),
}

/// Lines drawn by `text-decoration-line`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct TextDecorationLine {
    pub underline: bool,
    pub overline: bool,
    pub line_through: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextDecorationStyle {
    #[default]
    Solid,
    Double,
    Dotted,
    Dashed,
    Wavy,
}

/// `break-before` and `break-after`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BreakBetween {
    #[default]
    Auto,
    Avoid,
    AvoidPage,
    Page,
    Left,
    Right,
    Recto,
    Verso,
    Column,
    AvoidColumn,
}

impl BreakBetween {
    pub fn is_forced_page_break(self) -> bool {
        matches!(
            self,
            Self::Page | Self::Left | Self::Right | Self::Recto | Self::Verso
        )
    }

    pub fn avoids_page_break(self) -> bool {
        matches!(self, Self::Avoid | Self::AvoidPage)
    }
}

/// `break-inside`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BreakInside {
    #[default]
    Auto,
    Avoid,
    AvoidPage,
    AvoidColumn,
}

impl BreakInside {
    pub fn avoids_page_break(self) -> bool {
        matches!(self, Self::Avoid | Self::AvoidPage)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FlexDirection {
    #[default]
    Row,
    RowReverse,
    Column,
    ColumnReverse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FlexWrap {
    #[default]
    NoWrap,
    Wrap,
    WrapReverse,
}

/// `justify-content` and `align-content`. `Start` and `End` go by the inline or block axis,
/// `FlexStart` and `FlexEnd` by the main or cross axis of a flex container, which reversed
/// directions turn around; elsewhere they are `Start` and `End`. `Left` and `Right` (of
/// `justify-content`) go by the physical sides of an axis that runs from left to right or right
/// to left; along another axis they are `Start`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ContentAlignment {
    #[default]
    Normal,
    Start,
    End,
    Left,
    Right,
    FlexStart,
    FlexEnd,
    Center,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
    Stretch,
}

impl ContentAlignment {
    /// The value along a horizontal axis that starts on the left (`left_starts`) or on the
    /// right: `left` and `right` become `start` or `end`, other values stay as they are.
    #[must_use]
    pub fn sided(self, left_starts: bool) -> Self {
        match self {
            Self::Left if left_starts => Self::Start,
            Self::Right if !left_starts => Self::Start,
            Self::Left | Self::Right => Self::End,
            other => other,
        }
    }
}

/// `align-items` and `align-self` (`Auto` only for `align-self`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ItemAlignment {
    Auto,
    #[default]
    Normal,
    Stretch,
    Start,
    End,
    Center,
    Baseline,
    /// `last baseline`: items share their last baselines, at the end of the line where none
    /// is further from it (CSS Box Alignment 3, §9.1).
    LastBaseline,
}

/// What a self-alignment value (`align-self`, `align-items`) says beyond its [`ItemAlignment`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SelfFlags {
    /// `safe`: an item that overflows its alignment container starts at its start (CSS Box
    /// Alignment 3, §4.4).
    pub safe: bool,
    /// `flex-start` or `flex-end`: the start of a flex line is its cross-start, which
    /// `flex-wrap: wrap-reverse` turns over, unlike the `start` of the writing mode.
    pub flex: bool,
}

/// `justify-self` or `justify-items` as block layout uses them for block-level boxes (CSS Box
/// Alignment 3, §6.1): the position with the physical and `legacy` forms that [`ItemAlignment`]
/// folds together, and the overflow alignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockJustify {
    pub position: JustifyPosition,
    /// `safe` (`Some(true)`), `unsafe` (`Some(false)`) or neither.
    pub safe: Option<bool>,
    /// Whether this is `justify-items: legacy …`, which the children's `justify-items` take on.
    pub legacy: bool,
}

impl BlockJustify {
    /// `justify-self: auto`, the initial value.
    pub const AUTO: Self = Self::of(JustifyPosition::Auto);
    /// `justify-items: normal` (the initial `legacy` without a `legacy` value to take on).
    pub const NORMAL: Self = Self::of(JustifyPosition::Normal);

    /// A position without an overflow alignment.
    #[must_use]
    pub const fn of(position: JustifyPosition) -> Self {
        Self {
            position,
            safe: None,
            legacy: false,
        }
    }
}

/// The position of a [`BlockJustify`]. Baseline positions are `Start`, as in Blink.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JustifyPosition {
    Auto,
    Normal,
    Stretch,
    Start,
    End,
    Center,
    Left,
    Right,
    SelfStart,
    SelfEnd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TableLayout {
    #[default]
    Auto,
    Fixed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BorderCollapse {
    #[default]
    Separate,
    Collapse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CaptionSide {
    #[default]
    Top,
    Bottom,
}

/// A grid track size (simplified: the breadths Fagus supports in its first grid layout).
#[derive(Clone, Debug, PartialEq)]
pub enum TrackSize {
    Length(LengthPercentage),
    /// A flexible fraction (`fr`).
    Fraction(f32),
    Auto,
    MinContent,
    MaxContent,
    MinMax(Box<TrackSize>, Box<TrackSize>),
    /// `fit-content(limit)`.
    FitContent(LengthPercentage),
}

/// `hyphens`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Hyphens {
    None,
    /// Only at soft hyphens (`&shy;`).
    #[default]
    Manual,
    /// Also where the hyphenation patterns of the text's language allow it.
    Auto,
}

/// `overflow-wrap` (and its alias `word-wrap`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OverflowWrap {
    #[default]
    Normal,
    /// A word that does not fit on a line of its own breaks anywhere; its minimum width stays
    /// that of the word.
    BreakWord,
    /// Like `break-word`, and the minimum width is that of a single character.
    Anywhere,
}

/// `line-break`: how strictly lines break around punctuation and symbols of Chinese and
/// Japanese text, as Chrome tailors the Unicode line breaking algorithm.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LineBreak {
    /// Small kana and the prolonged sound mark may start a line.
    #[default]
    Auto,
    /// Like `normal`; iteration marks, hyphens, centered punctuation, exclamation and
    /// question marks, some postfix symbols may start a line too, some prefix symbols end one,
    /// and lines break between inseparable characters (`…`).
    Loose,
    /// Like `auto`; the wave dash and the katakana-hiragana double hyphen may start a line.
    Normal,
    /// Small kana and the prolonged sound mark may not start a line.
    Strict,
    /// Lines may break between any two characters (grapheme clusters).
    Anywhere,
}

/// `word-break`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum WordBreak {
    #[default]
    Normal,
    /// Lines may break between the letters and digits of words.
    BreakAll,
    /// No breaks inside words of ideographic scripts: for Latin scripts the same as `normal`.
    KeepAll,
    /// Deprecated: `normal` with the effect of `overflow-wrap: anywhere`.
    BreakWord,
    /// Japanese text breaks between phrases (where a phrase does not fit on a line, as with
    /// `normal`), and nothing is hyphenated.
    AutoPhrase,
}

/// The tracks of `grid-template-columns` or `grid-template-rows`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct TrackList {
    pub tracks: Arc<[TrackSize]>,
    /// The names of lines (`[name]`): the index of the line, from zero, and a name.
    pub names: Arc<[(usize, Arc<str>)]>,
    /// `repeat(auto-fill | auto-fit, …)`: the tracks repeated as often as they fit, inserted
    /// before track `at`.
    pub auto_repeat: Option<AutoRepeat>,
    /// `subgrid`: the tracks are those of the parent grid that the box spans, and `names` are
    /// extra names of their lines (CSS Grid 2, §9).
    pub subgrid: bool,
}

impl TrackList {
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty() && self.auto_repeat.is_none()
    }
}

/// An automatic repetition of grid tracks.
#[derive(Clone, Debug, PartialEq)]
pub struct AutoRepeat {
    pub at: usize,
    pub tracks: Arc<[TrackSize]>,
    /// `auto-fit`: repeated tracks without items collapse.
    pub fit: bool,
    /// The names of the lines inside one repetition: the index of the line from its start (up
    /// to the number of its tracks), and a name.
    pub names: Arc<[(usize, Arc<str>)]>,
    /// The names of the lines after the repeated tracks, the index of the line from the one
    /// right after them (`[a]` in `repeat(…) [a]` is at 0), and a name.
    pub after: Arc<[(usize, Arc<str>)]>,
    /// In a `subgrid` value, the number of lists of names after the repetition (empty ones
    /// too); `tracks` then has one `auto` per list repeated.
    pub after_lists: usize,
}

/// `column-fill`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ColumnFill {
    /// Columns of the same height, as far as the content allows.
    #[default]
    Balance,
    /// Columns filled one after the other.
    Auto,
}

/// `-fagus-repeat`, a Fagus extension: where a row of a table's header or footer shows when the
/// table breaks across pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Repeat {
    /// On every page of the table, as the header and footer are.
    #[default]
    Auto,
    /// Only where the table breaks: in the header after a break, in the footer before one. A
    /// carry-over row ("Übertrag") with the running total in `string()`.
    AtBreak,
}

/// `column-span`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ColumnSpan {
    #[default]
    None,
    /// Across all columns of the multi-column container.
    All,
}

/// How many columns a multi-column container has at most.
pub const MAX_COLUMNS: u32 = 1000;

/// `object-fit`: how the content of a replaced element (an image) fills its box.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ObjectFit {
    /// Stretched to the box.
    #[default]
    Fill,
    /// As large as fits in the box, keeping its aspect ratio.
    Contain,
    /// As small as covers the box, keeping its aspect ratio; clipped to the box.
    Cover,
    /// At its intrinsic size; clipped to the box.
    None,
    /// `none` or `contain`, whichever is smaller.
    ScaleDown,
}

/// How many explicit tracks a grid has at most, and the largest line number and span of an item.
/// Browsers clamp grids too (Firefox at 10,000 lines); the limit keeps the work of placing items
/// bounded.
pub const MAX_GRID_TRACKS: u16 = 1000;

/// A grid line of `grid-row-start`, `grid-column-end` and the like.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum GridLine {
    #[default]
    Auto,
    /// A line number; negative numbers count from the end of the explicit grid.
    Line(i32),
    Span(u32),
    /// A name alone: the line of an area (`name-start` or `name-end`), else a line of the name.
    Area(Arc<str>),
    /// The `n`th line of a name; negative numbers count from the end.
    Named(Arc<str>, i32),
    /// A span to the `n`th line of a name.
    SpanNamed(Arc<str>, u32),
}

/// The named areas of `grid-template-areas`, in a grid of `rows` by `columns` tracks.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct GridAreas {
    pub rows: usize,
    pub columns: usize,
    pub areas: Vec<GridArea>,
}

/// A named grid area: the rows and columns it takes, as start and end lines from zero.
#[derive(Clone, Debug, PartialEq)]
pub struct GridArea {
    pub name: Arc<str>,
    pub rows: (usize, usize),
    pub columns: (usize, usize),
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum ListStyleType {
    #[default]
    Disc,
    Circle,
    Square,
    Decimal,
    DecimalLeadingZero,
    LowerAlpha,
    UpperAlpha,
    LowerRoman,
    UpperRoman,
    None,
    String(String),
    /// Another counter style: a predefined one or one of an `@counter-style` rule, which
    /// layout finds by its name.
    Named(String),
}

impl ListStyleType {
    /// The name of the counter style, for the values that are one.
    pub fn counter_style(&self) -> Option<&str> {
        Some(match self {
            Self::Disc => "disc",
            Self::Circle => "circle",
            Self::Square => "square",
            Self::Decimal => "decimal",
            Self::DecimalLeadingZero => "decimal-leading-zero",
            Self::LowerAlpha => "lower-alpha",
            Self::UpperAlpha => "upper-alpha",
            Self::LowerRoman => "lower-roman",
            Self::UpperRoman => "upper-roman",
            Self::Named(name) => name,
            Self::None | Self::String(_) => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ListStylePosition {
    #[default]
    Outside,
    Inside,
}

/// One item of the `content` property.
#[derive(Clone, Debug, PartialEq)]
pub enum ContentItem {
    String(String),
    Counter {
        name: String,
        style: ListStyleType,
    },
    Counters {
        name: String,
        separator: String,
        style: ListStyleType,
    },
    Attr(String),
    OpenQuote,
    CloseQuote,
    /// A quote that is not shown, but counts for the depth of those that are.
    NoOpenQuote,
    NoCloseQuote,
    /// `string(name, which)` in margin boxes.
    NamedString {
        name: String,
        which: StringWhich,
    },
    /// `element(name)` in margin boxes.
    Element {
        name: String,
        which: StringWhich,
    },
    /// `content()` in `string-set`: text of the element or of its pseudo-elements.
    Content(ContentTarget),
    Leader(String),
    /// `target-counter(url, name, style)` (CSS GCPM 3, §2.1): the value of a counter at the
    /// element a link points to; for `page`, the number of the page it is on.
    TargetCounter {
        target: TargetUrl,
        name: String,
        style: ListStyleType,
    },
    /// `target-counters(url, name, separator, style)`: the values of all counters of the name.
    TargetCounters {
        target: TargetUrl,
        name: String,
        separator: String,
        style: ListStyleType,
    },
    /// `target-text(url, which)`: the text of the element a link points to.
    TargetText {
        target: TargetUrl,
        which: ContentTarget,
    },
    /// `url()`: an image, shown as an anonymous replaced element in the content (CSS Generated
    /// Content 3, §1.1); nothing where it does not load.
    Image(String),
}

/// Where a `target-*()` function points: the URL in an attribute of the element (`attr(href)`),
/// or a URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TargetUrl {
    Attr(String),
    Url(String),
}

/// What `content()` refers to (CSS GCPM 3, §1.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ContentTarget {
    #[default]
    Text,
    Before,
    After,
    FirstLetter,
    Marker,
}

/// Which assignment of a named string or running element a margin box shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum StringWhich {
    #[default]
    First,
    Start,
    Last,
    FirstExcept,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub enum Content {
    #[default]
    Normal,
    None,
    Items(Vec<ContentItem>),
}

/// An `<image>`, as used by `background-image`.
#[derive(Clone, Debug, PartialEq)]
pub enum Image {
    None,
    Gradient(Arc<Gradient>),
    /// An image file, resolved later by the caller's resource loader, and its resolution: image
    /// pixels per CSS pixel, 1 but for a candidate of `image-set()`.
    Url(Arc<str>, f32),
}

/// A gradient (CSS Images 3 and 4).
#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    pub kind: GradientKind,
    pub repeating: bool,
    pub stops: Vec<GradientStop>,
    /// The color interpolation method; `None` means sRGB if all colors are legacy sRGB colors and
    /// Oklab otherwise (CSS Color 4, §12.1).
    pub interpolation: Option<Interpolation>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum GradientKind {
    Linear(LinearDirection),
    Radial {
        shape: RadialShape,
        position: (LengthPercentage, LengthPercentage),
    },
    Conic {
        /// The start angle in degrees, clockwise from the top.
        from: f32,
        position: (LengthPercentage, LengthPercentage),
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LinearDirection {
    /// Degrees, clockwise from "to top".
    Angle(f32),
    /// Towards a corner: the angle depends on the box's aspect ratio.
    Corner { right: bool, bottom: bool },
}

#[derive(Clone, Debug, PartialEq)]
pub enum RadialShape {
    Circle(RadialSize),
    Ellipse(RadialSize),
}

#[derive(Clone, Debug, PartialEq)]
pub enum RadialSize {
    ClosestSide,
    FarthestSide,
    ClosestCorner,
    FarthestCorner,
    /// Explicit radii; a circle has the same length twice.
    Radii(LengthPercentage, LengthPercentage),
}

/// Where a color stop or hint sits on the gradient line.
#[derive(Clone, Debug, PartialEq)]
pub enum StopPosition {
    /// A length or percentage of the gradient line (for conic gradients: a fraction of a turn).
    Length(LengthPercentage),
    /// Degrees (conic gradients).
    Angle(f32),
}

#[derive(Clone, Debug, PartialEq)]
pub enum GradientStop {
    Color {
        /// `None` means `currentColor`.
        color: Option<Rgba>,
        /// Whether the color was given in a legacy sRGB syntax (hex, `rgb()`, `hsl()`, keywords).
        legacy: bool,
        position: Option<StopPosition>,
        /// The color the gradient goes on from at this stop where it differs from `color`: a
        /// color that misses components (`none`) takes them from the stop before up to here
        /// and from the stop after beyond.
        after: Option<Rgba>,
    },
    /// A transition hint: where the middle between two colors lies.
    Hint(StopPosition),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Interpolation {
    pub space: ColorSpace,
    pub hue: HueInterpolation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorSpace {
    Srgb,
    SrgbLinear,
    Lab,
    Oklab,
    Lch,
    Oklch,
    Hsl,
    Hwb,
    Xyz,
}

impl ColorSpace {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name.to_ascii_lowercase().as_str() {
            "srgb" | "display-p3" | "a98-rgb" | "prophoto-rgb" | "rec2020" => Self::Srgb,
            "srgb-linear" => Self::SrgbLinear,
            "lab" => Self::Lab,
            "oklab" => Self::Oklab,
            "lch" => Self::Lch,
            "oklch" => Self::Oklch,
            "hsl" => Self::Hsl,
            "hwb" => Self::Hwb,
            "xyz" | "xyz-d65" | "xyz-d50" => Self::Xyz,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HueInterpolation {
    #[default]
    Shorter,
    Longer,
    Increasing,
    Decreasing,
}

/// `background-size` of one layer.
#[derive(Clone, Debug, PartialEq)]
pub enum BackgroundSize {
    /// Width and height; `auto` keeps the image's size or ratio.
    Explicit(LengthPercentageAuto, LengthPercentageAuto),
    Cover,
    Contain,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackgroundRepeat {
    Repeat,
    Space,
    Round,
    NoRepeat,
}

/// A box of the box model, for `background-origin` and `background-clip`, and the reference
/// box of `clip-path` (which may be the margin box too).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxArea {
    BorderBox,
    PaddingBox,
    ContentBox,
    MarginBox,
}

/// A transform function (CSS Transforms 1 and 2), in 2D or 3D; a page shows the box flattened
/// onto it.
#[derive(Clone, Debug, PartialEq)]
pub enum TransformFunction {
    Translate(LengthPercentage, LengthPercentage),
    /// Pixels toward the viewer.
    TranslateZ(f32),
    Scale(f32, f32),
    /// Degrees, clockwise.
    Rotate(f32),
    /// Degrees about the axis `(x, y, z)`.
    Rotate3d(f32, f32, f32, f32),
    /// Degrees along x and y.
    Skew(f32, f32),
    /// `matrix(a, b, c, d, e, f)`.
    Matrix([f32; 6]),
    /// `matrix3d()`, column by column.
    Matrix3d([f32; 16]),
    /// `perspective()`: the distance to the viewer in pixels.
    Perspective(f32),
}

/// One `box-shadow`, in pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct BoxShadow {
    /// `None` means `currentColor`.
    pub color: Option<Rgba>,
    pub x: f32,
    pub y: f32,
    pub blur: f32,
    pub spread: f32,
    pub inset: bool,
}

/// A text decoration that a box draws over the text of its in-flow descendants (CSS Text
/// Decoration 3, §2: decorations propagate, they are not inherited).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Decoration {
    pub line: TextDecorationLine,
    pub style: TextDecorationStyle,
    pub color: Rgba,
    pub thickness: DecorationThickness,
    /// `text-underline-offset` in pixels, `None` for `auto`.
    pub underline_offset: Option<f32>,
    /// `text-underline-position: under`: below the text's descenders, not below its baseline.
    pub under: bool,
    /// The font size of the box that draws the decoration: its `auto` thickness and its
    /// place follow that box's font in the text of all its descendants, as in Chrome.
    pub font_size: f32,
}

/// A function of `filter` (Filter Effects 1, §13): amounts as fractions (`50%` is 0.5), angles
/// in degrees, lengths in pixels.
#[derive(Clone, Debug, PartialEq)]
pub enum Filter {
    /// The standard deviation of a Gaussian blur.
    Blur(f32),
    Brightness(f32),
    Contrast(f32),
    Grayscale(f32),
    HueRotate(f32),
    Invert(f32),
    Opacity(f32),
    Saturate(f32),
    Sepia(f32),
    /// A shadow of what the element draws: offsets, the standard deviation of its blur, and
    /// its color (`None` for `currentColor`).
    DropShadow {
        x: f32,
        y: f32,
        blur: f32,
        color: Option<Rgba>,
    },
    /// An SVG color matrix (`feColorMatrix`): rows of five numbers that map red, green, blue,
    /// alpha and 1 to each channel of a color with unpremultiplied alpha, in linear light
    /// (`color-interpolation-filters: linearRGB`, the default of SVG filters) or in sRGB.
    ColorMatrix {
        matrix: [f32; 20],
        linear: bool,
    },
    /// An SVG `<filter>` element (`url(#id)`), resolved to the functions above where it is
    /// drawn; one that is not resolved filters nothing.
    Url(Arc<str>),
}

impl Filter {
    /// A color after this filter, in sRGB as browsers filter, clamped to the gamut; filters
    /// other than those of color leave it as it is.
    pub fn apply(&self, color: Rgba) -> Rgba {
        let Rgba { r, g, b, a } = color;
        // The color matrices of Filter Effects 1, §13.
        let matrix = |m: [f32; 9]| Rgba {
            r: m[0] * r + m[1] * g + m[2] * b,
            g: m[3] * r + m[4] * g + m[5] * b,
            b: m[6] * r + m[7] * g + m[8] * b,
            a,
        };
        let each = |f: &dyn Fn(f32) -> f32| Rgba {
            r: f(r),
            g: f(g),
            b: f(b),
            a,
        };
        let filtered = match *self {
            Self::Grayscale(amount) => {
                let s = 1.0 - amount.clamp(0.0, 1.0);
                matrix([
                    0.2126 + 0.7874 * s,
                    0.7152 - 0.7152 * s,
                    0.0722 - 0.0722 * s,
                    0.2126 - 0.2126 * s,
                    0.7152 + 0.2848 * s,
                    0.0722 - 0.0722 * s,
                    0.2126 - 0.2126 * s,
                    0.7152 - 0.7152 * s,
                    0.0722 + 0.9278 * s,
                ])
            }
            Self::Sepia(amount) => {
                let s = 1.0 - amount.clamp(0.0, 1.0);
                matrix([
                    0.393 + 0.607 * s,
                    0.769 - 0.769 * s,
                    0.189 - 0.189 * s,
                    0.349 - 0.349 * s,
                    0.686 + 0.314 * s,
                    0.168 - 0.168 * s,
                    0.272 - 0.272 * s,
                    0.534 - 0.534 * s,
                    0.131 + 0.869 * s,
                ])
            }
            Self::Saturate(s) => {
                let s = s.max(0.0);
                matrix([
                    0.213 + 0.787 * s,
                    0.715 - 0.715 * s,
                    0.072 - 0.072 * s,
                    0.213 - 0.213 * s,
                    0.715 + 0.285 * s,
                    0.072 - 0.072 * s,
                    0.213 - 0.213 * s,
                    0.715 - 0.715 * s,
                    0.072 + 0.928 * s,
                ])
            }
            Self::HueRotate(degrees) => {
                let (sin, cos) = degrees.to_radians().sin_cos();
                matrix([
                    0.213 + cos * 0.787 - sin * 0.213,
                    0.715 - cos * 0.715 - sin * 0.715,
                    0.072 - cos * 0.072 + sin * 0.928,
                    0.213 - cos * 0.213 + sin * 0.143,
                    0.715 + cos * 0.285 + sin * 0.140,
                    0.072 - cos * 0.072 - sin * 0.283,
                    0.213 - cos * 0.213 - sin * 0.787,
                    0.715 - cos * 0.715 + sin * 0.715,
                    0.072 + cos * 0.928 + sin * 0.072,
                ])
            }
            Self::Invert(amount) => {
                let amount = amount.clamp(0.0, 1.0);
                each(&|c| amount * (1.0 - c) + (1.0 - amount) * c)
            }
            Self::Brightness(amount) => each(&|c| c * amount.max(0.0)),
            Self::Contrast(amount) => {
                let amount = amount.max(0.0);
                each(&|c| amount * c + 0.5 - 0.5 * amount)
            }
            Self::Opacity(amount) => Rgba {
                a: a * amount.clamp(0.0, 1.0),
                ..color
            },
            Self::ColorMatrix { matrix, linear } => {
                let decode = |c: f32| if linear { to_linear(c.clamp(0.0, 1.0)) } else { c };
                let input = [decode(r), decode(g), decode(b), a];
                let row = |index: usize| {
                    let m = &matrix[index * 5..index * 5 + 5];
                    m[0] * input[0] + m[1] * input[1] + m[2] * input[2] + m[3] * input[3] + m[4]
                };
                let encode = |c: f32| if linear { from_linear(c.clamp(0.0, 1.0)) } else { c };
                Rgba {
                    r: encode(row(0)),
                    g: encode(row(1)),
                    b: encode(row(2)),
                    a: row(3),
                }
            }
            Self::Blur(_) | Self::DropShadow { .. } | Self::Url(_) => return color,
        };
        Rgba {
            r: filtered.r.clamp(0.0, 1.0),
            g: filtered.g.clamp(0.0, 1.0),
            b: filtered.b.clamp(0.0, 1.0),
            a: filtered.a.clamp(0.0, 1.0),
        }
    }

    /// Whether the filter only maps colors, pixel by pixel.
    pub fn is_color(&self) -> bool {
        !matches!(
            self,
            Self::Blur(_) | Self::DropShadow { .. } | Self::Opacity(_) | Self::Url(_)
        )
    }
}

/// An sRGB channel in linear light.
fn to_linear(c: f32) -> f32 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// A channel in linear light in sRGB.
fn from_linear(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Where a scroll container has its `::scroll-marker-group` (`scroll-marker-group`, CSS
/// Overflow 5, §3.1): none, or a box before or after it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScrollMarkerGroup {
    #[default]
    None,
    Before,
    After,
}

/// Whether a box is a query container for `@container` (`container-type`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ContainerType {
    #[default]
    Normal,
    /// Its width can be queried.
    InlineSize,
    /// Its width and its height can be queried.
    Size,
}

/// How a box broken across lines, columns or pages is decorated (`box-decoration-break`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BoxDecorationBreak {
    /// Drawn whole and sliced at the breaks.
    #[default]
    Slice,
    /// Each fragment has its own border, padding and margin.
    Clone,
}

/// `border-image` (CSS Backgrounds 3, §6): an image cut into nine parts, drawn over the border
/// area in place of the border's styles.
#[derive(Clone, Debug, PartialEq)]
pub struct BorderImage {
    pub source: Image,
    /// Where the image is cut, in from its top, right, bottom and left edge.
    pub slice: [ImageSlice; 4],
    /// Whether the middle part is drawn too.
    pub fill: bool,
    /// How wide the parts are drawn at the top, right, bottom and left.
    pub width: [BorderImageWidth; 4],
    /// How far the image reaches out of the border box, at the top, right, bottom and left.
    pub outset: [BorderImageOutset; 4],
    /// How the edges and the middle fill their space, across and down.
    pub repeat: (BorderImageRepeat, BorderImageRepeat),
}

impl Default for BorderImage {
    fn default() -> Self {
        Self {
            source: Image::None,
            slice: [ImageSlice::Percent(1.0); 4],
            fill: false,
            width: std::array::from_fn(|_| BorderImageWidth::Number(1.0)),
            outset: [BorderImageOutset::Number(0.0); 4],
            repeat: (BorderImageRepeat::Stretch, BorderImageRepeat::Stretch),
        }
    }
}

/// Where `border-image-slice` cuts: pixels of the image, or a fraction of its size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ImageSlice {
    Number(f32),
    Percent(f32),
}

/// A side of `border-image-width`.
#[derive(Clone, Debug, PartialEq)]
pub enum BorderImageWidth {
    /// A multiple of the border width.
    Number(f32),
    Length(LengthPercentage),
    /// The size of the part of the image.
    Auto,
}

/// A side of `border-image-outset`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BorderImageOutset {
    /// A multiple of the border width.
    Number(f32),
    /// Pixels.
    Length(f32),
}

/// How a part of a border image fills its space (`border-image-repeat`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BorderImageRepeat {
    Stretch,
    Repeat,
    Round,
    Space,
}

/// `mask` (CSS Masking 1, §7): image layers, painted as background layers are, whose alpha or
/// luminance masks what an element draws. The lists repeat to match the images.
#[derive(Clone, Debug, PartialEq)]
pub struct MaskLayers {
    pub images: Arc<[Image]>,
    pub modes: Arc<[MaskMode]>,
    pub position_x: Arc<[LengthPercentage]>,
    pub position_y: Arc<[LengthPercentage]>,
    pub sizes: Arc<[BackgroundSize]>,
    pub repeats: Arc<[(BackgroundRepeat, BackgroundRepeat)]>,
    pub origins: Arc<[BoxArea]>,
    /// `None` for `no-clip`.
    pub clips: Arc<[Option<BoxArea>]>,
    pub composites: Arc<[MaskComposite]>,
    /// The boxes of `mask-origin` as an SVG element (one without a CSS box) has them: `fill-box`
    /// for `content-box`, `padding-box` and `fill-box`, `view-box`, and `stroke-box` for the
    /// others (CSS Masking 1, §6.1).
    pub svg_origins: Arc<[TransformBox]>,
    /// The boxes of `mask-clip` as an SVG element has them; `None` for `no-clip`.
    pub svg_clips: Arc<[Option<TransformBox>]>,
}

impl Default for MaskLayers {
    fn default() -> Self {
        Self {
            images: Arc::from([Image::None]),
            modes: Arc::from([MaskMode::MatchSource]),
            position_x: Arc::from([LengthPercentage::ZERO]),
            position_y: Arc::from([LengthPercentage::ZERO]),
            sizes: Arc::from([BackgroundSize::Explicit(
                LengthPercentageAuto::Auto,
                LengthPercentageAuto::Auto,
            )]),
            repeats: Arc::from([(BackgroundRepeat::Repeat, BackgroundRepeat::Repeat)]),
            origins: Arc::from([BoxArea::BorderBox]),
            clips: Arc::from([Some(BoxArea::BorderBox)]),
            composites: Arc::from([MaskComposite::Add]),
            svg_origins: Arc::from([TransformBox::StrokeBox]),
            svg_clips: Arc::from([Some(TransformBox::StrokeBox)]),
        }
    }
}

impl MaskLayers {
    /// Whether a layer has an image: a mask of none masks nothing.
    pub fn is_none(&self) -> bool {
        self.images.iter().all(|image| matches!(image, Image::None))
    }
}

/// What of a mask image masks (`mask-mode`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MaskMode {
    Alpha,
    Luminance,
    /// The alpha of images; the luminance of SVG `<mask>` elements, which are not supported.
    #[default]
    MatchSource,
}

/// How a mask layer combines with those below it (`mask-composite`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MaskComposite {
    #[default]
    Add,
    Subtract,
    Intersect,
    Exclude,
}

/// `clip-path` (CSS Masking 1, §6): a basic shape in a box of the element, or the box itself.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipPath {
    pub shape: ClipShape,
    pub reference: BoxArea,
    /// The reference box for an SVG element, which has no CSS boxes: `fill-box` for
    /// `content-box`, `padding-box` and `fill-box`, `view-box`, and `stroke-box` for the others
    /// (CSS Masking 1, §6.1).
    pub svg_reference: TransformBox,
}

/// A basic shape of `clip-path` (CSS Shapes 1, §3).
#[derive(Clone, Debug, PartialEq)]
pub enum ClipShape {
    /// The reference box with the element's rounded corners.
    Box,
    /// `inset()`: the box less its insets (top, right, bottom, left), with rounded corners.
    Inset {
        insets: [LengthPercentage; 4],
        radii: Corners,
    },
    /// `circle()` and `ellipse()`: radii (the same for a circle) about a center.
    Ellipse {
        radii: (ShapeRadius, ShapeRadius),
        circle: bool,
        center: (LengthPercentage, LengthPercentage),
    },
    /// `polygon()`: its points, filled by the even-odd rule or not.
    Polygon {
        even_odd: bool,
        points: Vec<(LengthPercentage, LengthPercentage)>,
    },
    /// An SVG `<clipPath>` element (`url(#id)`) of `clip-path`, by its URL.
    Url(Arc<str>),
    /// `path()`: an outline in SVG path data, in px from the reference box's corner, filled by
    /// the even-odd rule or not.
    Path { even_odd: bool, data: Arc<str> },
}

/// `border-shape` (CSS Borders 4, §7): the border drawn along a basic shape (a stroke as wide
/// as the border at the block start, in its color), or between two (filled in that color),
/// where the box's background and content are clipped to the shape (the outer one).
#[derive(Clone, Debug, PartialEq)]
pub struct BorderShape {
    pub outer: (ClipShape, ShapeReference),
    pub inner: Option<(ClipShape, ShapeReference)>,
}

/// The box a shape of `border-shape` is laid out in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeReference {
    Box(BoxArea),
    /// Halfway between the border box and the padding box: the default of a single shape.
    HalfBorderBox,
}

/// `shape-outside` (CSS Shapes 1, §6.1): the float area that the lines beside a float wrap
/// around.
#[derive(Clone, Debug, PartialEq)]
pub enum ShapeOutside {
    /// A basic shape in a box of the float, or the box itself ([`ClipShape::Box`]) with its
    /// rounded corners.
    Shape { shape: ClipShape, reference: ShapeBox },
    /// Where an image is more opaque than `shape-image-threshold`, laid out in the content box.
    Image(Image),
}

/// The reference box of `shape-outside`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeBox {
    MarginBox,
    BorderBox,
    PaddingBox,
    ContentBox,
}

/// The radius of `circle()` or `ellipse()`.
#[derive(Clone, Debug, PartialEq)]
pub enum ShapeRadius {
    Length(LengthPercentage),
    ClosestSide,
    FarthestSide,
}

/// A blend mode of `mix-blend-mode` and `background-blend-mode` (Compositing 1, §10).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl BlendMode {
    /// The blend mode of a keyword.
    pub fn of(keyword: &str) -> Option<Self> {
        Some(match keyword.to_ascii_lowercase().as_str() {
            "normal" => Self::Normal,
            "multiply" => Self::Multiply,
            "screen" => Self::Screen,
            "overlay" => Self::Overlay,
            "darken" => Self::Darken,
            "lighten" => Self::Lighten,
            "color-dodge" => Self::ColorDodge,
            "color-burn" => Self::ColorBurn,
            "hard-light" => Self::HardLight,
            "soft-light" => Self::SoftLight,
            "difference" => Self::Difference,
            "exclusion" => Self::Exclusion,
            "hue" => Self::Hue,
            "saturation" => Self::Saturation,
            "color" => Self::Color,
            "luminosity" => Self::Luminosity,
            _ => return None,
        })
    }
}

/// `transform-box` (CSS Transforms 1, §6.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TransformBox {
    ContentBox,
    BorderBox,
    /// The object bounding box of an SVG element.
    FillBox,
    /// The stroke bounding box of an SVG element.
    StrokeBox,
    /// The nearest SVG viewport.
    #[default]
    ViewBox,
}

/// `isolation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Isolation {
    #[default]
    Auto,
    Isolate,
}

/// `image-orientation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ImageOrientation {
    /// As the image's EXIF data says.
    #[default]
    FromImage,
    None,
}

/// `image-rendering`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ImageRendering {
    /// Smoothed where scaled.
    #[default]
    Auto,
    /// Not smoothed: `pixelated`, `crisp-edges`, `optimizeSpeed`.
    Pixelated,
}

/// `-webkit-box-orient`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BoxOrient {
    #[default]
    Horizontal,
    Vertical,
}

/// `text-decoration-skip-ink`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SkipInk {
    /// Underlines and overlines leave gaps where they would cross glyphs.
    #[default]
    Auto,
    None,
    All,
}

/// `text-underline-position`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum UnderlinePosition {
    #[default]
    Auto,
    /// Below the text's descenders.
    Under,
}

/// The mark of `text-emphasis-style` (CSS Text Decoration 3, §3.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EmphasisMark {
    /// One of the shapes, filled or open.
    Shape { shape: EmphasisShape, filled: bool },
    /// A string, of which the first character is drawn.
    String(Arc<str>),
}

/// The shapes of `text-emphasis-style`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmphasisShape {
    Dot,
    Circle,
    DoubleCircle,
    Triangle,
    Sesame,
}

impl EmphasisMark {
    /// The text drawn as the mark: the character of the shape, as Chrome draws it.
    pub fn text(&self) -> &str {
        match self {
            Self::String(text) => text,
            Self::Shape { shape, filled } => match (shape, filled) {
                (EmphasisShape::Dot, true) => "\u{2022}",
                (EmphasisShape::Dot, false) => "\u{25E6}",
                (EmphasisShape::Circle, true) => "\u{25CF}",
                (EmphasisShape::Circle, false) => "\u{25CB}",
                (EmphasisShape::DoubleCircle, true) => "\u{25C9}",
                (EmphasisShape::DoubleCircle, false) => "\u{25CE}",
                (EmphasisShape::Triangle, true) => "\u{25B2}",
                (EmphasisShape::Triangle, false) => "\u{25B3}",
                (EmphasisShape::Sesame, true) => "\u{FE45}",
                (EmphasisShape::Sesame, false) => "\u{FE46}",
            },
        }
    }
}

/// Which side of the text a mark or an annotation goes on: `text-emphasis-position`,
/// `ruby-position`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AnnotationSide {
    #[default]
    Over,
    Under,
}

/// The part a box plays in ruby layout (CSS Ruby 1, §2.1), apart from its `display` type, which
/// is `inline` for all of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RubyDisplay {
    #[default]
    None,
    /// `display: ruby`: a ruby container, whose content pairs bases with annotations.
    Container,
    /// `display: ruby-text`: an annotation.
    Text,
    /// A ruby column, a base with its annotation, laid out by Fagus as one atomic inline.
    Column,
    /// The box around an annotation in a column, which reaches as far as the annotation's em box.
    Annotation,
}

/// `text-spacing-trim` (CSS Text 4, §8.3): where fullwidth punctuation loses the blank half of
/// its glyph, with the values Chrome takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextSpacingTrim {
    /// Collapses adjacent punctuation and lets closing punctuation at the end of a line lose its
    /// blank half where that makes it fit.
    #[default]
    Normal,
    /// Keeps the full width of all punctuation.
    SpaceAll,
    /// As `normal`, and opening punctuation at the start of every line but the first of a
    /// paragraph loses its blank half.
    SpaceFirst,
    /// As `normal`, and opening punctuation at the start of every line loses its blank half.
    TrimStart,
}

/// `text-fit` (CSS Text 5, §6.1): whether the text of a block grows or shrinks to fill its lines,
/// as Chrome 157 takes it (`[none | grow | shrink] [consistent | per-line | per-line-all]?
/// <percentage>?`).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct TextFit {
    pub kind: TextFitKind,
    pub target: TextFitTarget,
    /// The percentage as a factor (1 for `100%`): how far the text may grow or shrink at most.
    pub limit: Option<f32>,
}

/// Whether `text-fit` grows or shrinks text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextFitKind {
    #[default]
    None,
    Grow,
    Shrink,
}

/// Which lines `text-fit` fits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextFitTarget {
    /// All lines by the same factor, the one that fits the line that needs the least.
    #[default]
    Consistent,
    /// Each line but the last by its own factor.
    PerLine,
    /// Each line by its own factor.
    PerLineAll,
}

/// `ruby-align` (CSS Ruby 1, §4.1): how ruby text and its base share the width of the wider.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RubyAlign {
    #[default]
    SpaceAround,
    Start,
    Center,
    SpaceBetween,
}

/// The edges of `text-box-edge` (CSS Inline 3, §5.2) a block's first and last lines are trimmed
/// to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextEdge {
    /// The font's ascent or descent (`auto`, `text`).
    #[default]
    Text,
    /// The cap height (over only).
    Cap,
    /// The x-height (over only).
    Ex,
    /// The alphabetic baseline (under only).
    Alphabetic,
}

/// `text-box-trim` (CSS Inline 3, §5.1): which sides of a block are trimmed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct TextBoxTrim {
    pub start: bool,
    pub end: bool,
}

/// `text-decoration-thickness`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum DecorationThickness {
    #[default]
    Auto,
    /// The underline thickness of the font.
    FromFont,
    /// A length in pixels.
    Px(f32),
}

impl TextDecorationLine {
    pub fn is_none(self) -> bool {
        !(self.underline || self.overline || self.line_through)
    }
}

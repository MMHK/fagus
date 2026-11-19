//! CSS Anchor Positioning 1: the properties that name anchors and place absolutely positioned
//! boxes against them (`anchor-name`, `anchor-scope`, `position-anchor`, `position-area`,
//! `position-try-fallbacks`), and the insets, sizes and margins with `anchor()` and
//! `anchor-size()`, which keep their tokens (lengths made pixels) until layout knows where the
//! anchors are.

use std::sync::Arc;

use lightningcss::properties::PropertyId;
use lightningcss::properties::custom::{Function, Token, TokenList, TokenOrValue};
use lightningcss::values::length::LengthValue;

use crate::compute::Context;
use crate::style::{ComputedStyle, ComputedValues};

/// The tokens of a value with anchor functions.
pub type Tokens = TokenList<'static>;

/// The anchor positioning of an element (not inherited).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Anchoring {
    /// `anchor-name`: the names the element's box is an anchor by.
    pub names: Vec<String>,
    /// `anchor-scope`: the anchor names that only the element's descendants find inside it.
    pub scope: AnchorScope,
    /// `position-anchor`: the anchor of `anchor()` without a name and of `position-area`
    /// (`None` for `auto`, the implicit anchor, which printed documents do not have).
    pub default: Option<String>,
    /// `top`, `right`, `bottom` and `left` with `anchor()` or `anchor-size()`.
    pub insets: [Option<TokenList<'static>>; 4],
    /// `width`, `height`, `min-width`, `min-height`, `max-width` and `max-height` with
    /// `anchor-size()`.
    pub sizes: [Option<TokenList<'static>>; 6],
    /// `margin-top`, `-right`, `-bottom` and `-left` with `anchor-size()`.
    pub margins: [Option<TokenList<'static>>; 4],
    /// `position-area`.
    pub area: Option<PositionArea>,
    /// `position-try-fallbacks`: the options tried in order where the box overflows.
    pub fallbacks: Vec<TryOption>,
    /// `position-try-order`.
    pub order: TryOrder,
    /// `position-visibility`.
    pub visibility: PositionVisibility,
    /// `justify-self: anchor-center` and `align-self: anchor-center`.
    pub center: [bool; 2],
}

impl Anchoring {
    /// Whether the element's box is placed against an anchor (rather than only being one).
    pub fn is_positioned(&self) -> bool {
        self.insets.iter().any(Option::is_some)
            || self.sizes.iter().any(Option::is_some)
            || self.margins.iter().any(Option::is_some)
            || self.area.is_some()
            || self.center.iter().any(|center| *center)
    }
}

/// `anchor-scope`.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum AnchorScope {
    #[default]
    None,
    All,
    Names(Vec<String>),
}

impl AnchorScope {
    /// Whether the scope covers an anchor name.
    pub fn covers(&self, name: &str) -> bool {
        match self {
            Self::None => false,
            Self::All => true,
            Self::Names(names) => names.iter().any(|scoped| scoped == name),
        }
    }
}

/// A region of the 3×3 grid that an anchor's edges make in the containing block: the columns
/// and rows it spans, each from 0 (before the anchor) to 2 (after it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PositionArea {
    pub columns: (u8, u8),
    pub rows: (u8, u8),
}

impl PositionArea {
    /// The area with the rows (`block`) or the columns mirrored.
    #[must_use]
    pub fn flipped(self, block: bool, inline: bool) -> Self {
        let mirror = |(from, to): (u8, u8)| (2 - to, 2 - from);
        Self {
            columns: if inline {
                mirror(self.columns)
            } else {
                self.columns
            },
            rows: if block { mirror(self.rows) } else { self.rows },
        }
    }

    /// The area with rows and columns swapped (`flip-start`).
    #[must_use]
    pub fn transposed(self) -> Self {
        Self {
            columns: self.rows,
            rows: self.columns,
        }
    }
}

/// An option of `position-try-fallbacks`.
#[derive(Clone, Debug)]
pub enum TryOption {
    /// `flip-block`, `flip-inline` and `flip-start`, in any combination.
    Flip { block: bool, inline: bool, start: bool },
    /// A `position-area` instead of the element's own.
    Area(PositionArea),
    /// A `@position-try` rule, with flips after it.
    Rule {
        name: String,
        flip: (bool, bool, bool),
        /// The element's style with the rule's declarations, once the cascade has applied
        /// them (`None` where no rule has the name: the option is skipped).
        style: Option<Arc<ComputedStyle>>,
    },
}

impl PartialEq for TryOption {
    /// Options are equal where they say the same; the styles of rules, built from the same
    /// declarations, are the same style.
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::Flip { block, inline, start },
                Self::Flip {
                    block: other_block,
                    inline: other_inline,
                    start: other_start,
                },
            ) => (block, inline, start) == (other_block, other_inline, other_start),
            (Self::Area(area), Self::Area(other)) => area == other,
            (
                Self::Rule { name, flip, style },
                Self::Rule {
                    name: other_name,
                    flip: other_flip,
                    style: other_style,
                },
            ) => {
                name == other_name
                    && flip == other_flip
                    && match (style, other_style) {
                        (Some(style), Some(other)) => Arc::ptr_eq(style, other),
                        (None, None) => true,
                        _ => false,
                    }
            }
            _ => false,
        }
    }
}

/// `position-try-order`: the options (the element's own placement among them) sorted by the
/// size of their inset-modified containing blocks along an axis, largest first, before they
/// are tried; `Normal` keeps them in order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TryOrder {
    #[default]
    Normal,
    MostWidth,
    MostHeight,
}

/// `position-visibility`: when a box placed against an anchor is not drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PositionVisibility {
    /// `anchors-visible` (the initial value): where its anchor is clipped away by the boxes
    /// around it.
    pub anchors_visible: bool,
    /// `no-overflow`: where it overflows its inset-modified containing block in every option.
    pub no_overflow: bool,
}

impl Default for PositionVisibility {
    fn default() -> Self {
        Self {
            anchors_visible: true,
            no_overflow: false,
        }
    }
}

/// The properties a `@position-try` rule may set (CSS Anchor Positioning 1, §5.1).
pub fn applies_in_position_try(name: &str) -> bool {
    matches!(
        name,
        "top"
            | "right"
            | "bottom"
            | "left"
            | "inset"
            | "inset-block"
            | "inset-inline"
            | "inset-block-start"
            | "inset-block-end"
            | "inset-inline-start"
            | "inset-inline-end"
            | "margin"
            | "margin-top"
            | "margin-right"
            | "margin-bottom"
            | "margin-left"
            | "margin-block"
            | "margin-inline"
            | "margin-block-start"
            | "margin-block-end"
            | "margin-inline-start"
            | "margin-inline-end"
            | "width"
            | "height"
            | "min-width"
            | "min-height"
            | "max-width"
            | "max-height"
            | "block-size"
            | "inline-size"
            | "min-block-size"
            | "min-inline-size"
            | "max-block-size"
            | "max-inline-size"
            | "justify-self"
            | "align-self"
            | "place-self"
            | "position-anchor"
            | "position-area"
    )
}

fn significant<'t>(tokens: &'t [TokenOrValue<'static>]) -> Vec<&'t TokenOrValue<'static>> {
    tokens.iter().filter(|token| !token.is_whitespace()).collect()
}

fn ident(token: &TokenOrValue<'_>) -> Option<String> {
    match token {
        TokenOrValue::Token(Token::Ident(ident)) => Some(ident.to_ascii_lowercase()),
        _ => None,
    }
}

/// A dashed identifier (an anchor name).
fn dashed(token: &TokenOrValue<'_>) -> Option<String> {
    match token {
        TokenOrValue::DashedIdent(name) => Some(name.0.to_string()),
        TokenOrValue::Token(Token::Ident(name)) if name.starts_with("--") => Some(name.to_string()),
        _ => None,
    }
}

fn anchoring(style: &mut ComputedValues) -> &mut Anchoring {
    Arc::make_mut(style.anchoring.get_or_insert_with(Default::default))
}

/// Applies a declaration of the anchor properties that lightningcss does not know; `false` for
/// other properties.
pub(crate) fn apply_property(name: &str, tokens: &TokenList<'static>, style: &mut ComputedValues) -> bool {
    let tokens = significant(&tokens.0);
    let keyword = tokens.first().and_then(|token| ident(token));
    match name {
        "anchor-name" => {
            let names = if keyword.as_deref() == Some("none") {
                Some(Vec::new())
            } else {
                names(&tokens)
            };
            if let Some(names) = names {
                anchoring(style).names = names;
            }
        }
        "anchor-scope" => {
            let scope = match (keyword.as_deref(), tokens.len()) {
                (Some("none"), 1) => Some(AnchorScope::None),
                (Some("all"), 1) => Some(AnchorScope::All),
                _ => names(&tokens).map(AnchorScope::Names),
            };
            if let Some(scope) = scope {
                anchoring(style).scope = scope;
            }
        }
        "position-anchor" => match (tokens.as_slice(), keyword.as_deref()) {
            ([_], Some("auto")) => anchoring(style).default = None,
            ([token], _) if let Some(name) = dashed(token) => anchoring(style).default = Some(name),
            _ => {}
        },
        "position-area" => {
            if keyword.as_deref() == Some("none") && tokens.len() == 1 {
                anchoring(style).area = None;
            } else if let Some(area) = position_area(&tokens) {
                anchoring(style).area = Some(area);
            }
        }
        "position-try-fallbacks" => {
            if let Some(fallbacks) = fallbacks(&tokens) {
                anchoring(style).fallbacks = fallbacks;
            }
        }
        // The shorthand: an order, then the fallbacks.
        "position-try" => {
            let order = keyword.as_deref().and_then(try_order);
            let rest = if order.is_some() {
                &tokens[1..]
            } else {
                &tokens[..]
            };
            if let Some(fallbacks) = fallbacks(rest) {
                let anchoring = anchoring(style);
                anchoring.fallbacks = fallbacks;
                anchoring.order = order.unwrap_or_default();
            }
        }
        "position-try-order" => {
            if let (Some(order), 1) = (keyword.as_deref().and_then(try_order), tokens.len()) {
                anchoring(style).order = order;
            }
        }
        "position-visibility" => {
            let words: Option<Vec<String>> = tokens.iter().map(|token| ident(token)).collect();
            let visibility = match words.as_deref() {
                Some([word]) if word == "always" => Some(PositionVisibility {
                    anchors_visible: false,
                    no_overflow: false,
                }),
                Some(words) if !words.is_empty() && words.len() <= 2 => {
                    let anchors_visible = words.iter().any(|word| word == "anchors-visible");
                    let no_overflow = words.iter().any(|word| word == "no-overflow");
                    (usize::from(anchors_visible) + usize::from(no_overflow) == words.len()).then_some(
                        PositionVisibility {
                            anchors_visible,
                            no_overflow,
                        },
                    )
                }
                _ => None,
            };
            if let Some(visibility) = visibility {
                anchoring(style).visibility = visibility;
            }
        }
        _ => return false,
    }
    true
}

/// A keyword of `position-try-order` (the logical ones for horizontal writing).
fn try_order(word: &str) -> Option<TryOrder> {
    match word {
        "normal" => Some(TryOrder::Normal),
        "most-width" | "most-inline-size" => Some(TryOrder::MostWidth),
        "most-height" | "most-block-size" => Some(TryOrder::MostHeight),
        _ => None,
    }
}

/// A comma-separated list of dashed identifiers.
fn names(tokens: &[&TokenOrValue<'static>]) -> Option<Vec<String>> {
    let mut names = Vec::new();
    for group in tokens.split(|token| matches!(token, TokenOrValue::Token(Token::Comma))) {
        match group {
            [token] => names.push(dashed(token)?),
            _ => return None,
        }
    }
    Some(names)
}

/// `none` or a comma-separated list of try options.
fn fallbacks(tokens: &[&TokenOrValue<'static>]) -> Option<Vec<TryOption>> {
    if let [token] = tokens
        && ident(token).as_deref() == Some("none")
    {
        return Some(Vec::new());
    }
    let mut options = Vec::new();
    for group in tokens.split(|token| matches!(token, TokenOrValue::Token(Token::Comma))) {
        if group.is_empty() {
            return None;
        }
        let (mut block, mut inline, mut start) = (false, false, false);
        let mut named = None;
        let mut tactics = true;
        for token in group {
            match ident(token).as_deref() {
                Some("flip-block") if !block => block = true,
                Some("flip-inline") if !inline => inline = true,
                Some("flip-start") if !start => start = true,
                _ if named.is_none()
                    && let Some(name) = dashed(token) =>
                {
                    named = Some(name);
                }
                _ => tactics = false,
            }
        }
        if tactics {
            options.push(match named {
                // A `@position-try` rule, which the cascade applies.
                Some(name) => TryOption::Rule {
                    name,
                    flip: (block, inline, start),
                    style: None,
                },
                None => TryOption::Flip { block, inline, start },
            });
        } else {
            options.push(TryOption::Area(position_area(group)?));
        }
    }
    Some(options)
}

/// The track range a keyword of `position-area` stands for, and its axis: `Some(true)` for the
/// columns (horizontal), `Some(false)` for the rows, `None` for either (`center`, `start`,
/// `span-all`, …).
fn area_keyword(word: &str) -> Option<((u8, u8), Option<bool>)> {
    let range = |name: &str| match name {
        "start" => Some((0, 0)),
        "end" => Some((2, 2)),
        "span-start" => Some((0, 1)),
        "span-end" => Some((1, 2)),
        _ => None,
    };
    Some(match word {
        "left" => ((0, 0), Some(true)),
        "right" => ((2, 2), Some(true)),
        "span-left" => ((0, 1), Some(true)),
        "span-right" => ((1, 2), Some(true)),
        "top" => ((0, 0), Some(false)),
        "bottom" => ((2, 2), Some(false)),
        "span-top" => ((0, 1), Some(false)),
        "span-bottom" => ((1, 2), Some(false)),
        "center" => ((1, 1), None),
        "span-all" => ((0, 2), None),
        "start" | "end" | "span-start" | "span-end" | "self-start" | "self-end" | "span-self-start"
        | "span-self-end" => (
            range(word.trim_start_matches("self-").replace("-self-", "-").as_str())?,
            None,
        ),
        _ => {
            // `x-start`, `block-end`, `span-inline-start`, `span-y-self-end`, …
            let (span, rest) = match word.strip_prefix("span-") {
                Some(rest) => (true, rest),
                None => (false, word),
            };
            let (axis, side) = rest.split_once('-')?;
            let horizontal = match axis {
                "x" | "inline" => true,
                "y" | "block" => false,
                _ => return None,
            };
            let side = side.trim_start_matches("self-");
            let side = if span {
                format!("span-{side}")
            } else {
                side.to_owned()
            };
            (range(&side)?, Some(horizontal))
        }
    })
}

/// `position-area`: one keyword (the other axis spans all, or both take it), or two.
fn position_area(tokens: &[&TokenOrValue<'static>]) -> Option<PositionArea> {
    let words: Vec<String> = tokens.iter().map(|token| ident(token)).collect::<Option<_>>()?;
    let keywords: Vec<((u8, u8), Option<bool>)> = words
        .iter()
        .map(|word| area_keyword(word))
        .collect::<Option<_>>()?;
    match keywords.as_slice() {
        [(range, Some(true))] => Some(PositionArea {
            columns: *range,
            rows: (0, 2),
        }),
        [(range, Some(false))] => Some(PositionArea {
            columns: (0, 2),
            rows: *range,
        }),
        [(range, None)] => Some(PositionArea {
            columns: *range,
            rows: *range,
        }),
        [(first, first_axis), (second, second_axis)] => match (first_axis, second_axis) {
            (Some(true), Some(false) | None) | (None, Some(false)) => Some(PositionArea {
                columns: *first,
                rows: *second,
            }),
            (Some(false), Some(true) | None) | (None, Some(true)) => Some(PositionArea {
                columns: *second,
                rows: *first,
            }),
            // Two generic keywords: the block axis first.
            (None, None) => Some(PositionArea {
                columns: *second,
                rows: *first,
            }),
            _ => None,
        },
        _ => None,
    }
}

/// The physical property a longhand with anchor functions sets, for logical ones in the
/// element's writing mode.
fn physical(id: &PropertyId<'static>, context: &Context) -> PropertyId<'static> {
    crate::compute::physical_id(id, context.writing_mode, context.rtl).unwrap_or_else(|| id.clone())
}

/// Applies a longhand with `anchor()` or `anchor-size()`, or a self-alignment of
/// `anchor-center` (see [`fagus_css::is_anchored`]); `false` for others.
pub(crate) fn apply_unparsed(
    id: &PropertyId<'static>,
    tokens: &TokenList<'static>,
    style: &mut ComputedValues,
    context: &Context,
) -> bool {
    let id = match id {
        PropertyId::BlockSize => {
            if context.writing_mode.is_vertical() {
                PropertyId::Width
            } else {
                PropertyId::Height
            }
        }
        PropertyId::InlineSize => {
            if context.writing_mode.is_vertical() {
                PropertyId::Height
            } else {
                PropertyId::Width
            }
        }
        other => physical(other, context),
    };
    let center = || {
        tokens
            .0
            .iter()
            .any(|token| ident(token).as_deref() == Some("anchor-center"))
    };
    let value = || Some(pixels(tokens, context));
    match id {
        PropertyId::JustifySelf => anchoring(style).center[0] = center(),
        PropertyId::AlignSelf(_) => anchoring(style).center[1] = center(),
        PropertyId::Top => anchoring(style).insets[0] = value(),
        PropertyId::Right => anchoring(style).insets[1] = value(),
        PropertyId::Bottom => anchoring(style).insets[2] = value(),
        PropertyId::Left => anchoring(style).insets[3] = value(),
        PropertyId::Width => anchoring(style).sizes[0] = value(),
        PropertyId::Height => anchoring(style).sizes[1] = value(),
        PropertyId::MinWidth => anchoring(style).sizes[2] = value(),
        PropertyId::MinHeight => anchoring(style).sizes[3] = value(),
        PropertyId::MaxWidth => anchoring(style).sizes[4] = value(),
        PropertyId::MaxHeight => anchoring(style).sizes[5] = value(),
        PropertyId::MarginTop => anchoring(style).margins[0] = value(),
        PropertyId::MarginRight => anchoring(style).margins[1] = value(),
        PropertyId::MarginBottom => anchoring(style).margins[2] = value(),
        PropertyId::MarginLeft => anchoring(style).margins[3] = value(),
        _ => return false,
    }
    true
}

/// The value of one longhand from the tokens of a shorthand with anchor functions: the `index`th
/// of its `count` longhands (`inset` and `margin` give their sides as `margin` does, two-sided
/// ones start and end, `place-self` both).
pub(crate) fn longhand_value(
    tokens: &TokenList<'static>,
    index: usize,
    count: usize,
) -> Option<TokenList<'static>> {
    let values: Vec<&TokenOrValue<'static>> = significant(&tokens.0);
    let pick = match (count, values.len()) {
        (_, 1) => 0,
        (2, 2) => index,
        (4, 2) => index % 2,
        (4, 3) => [0, 1, 2, 1][index],
        (4, 4) => index,
        _ => return None,
    };
    Some(TokenList(vec![values.get(pick).copied()?.clone()]))
}

/// Tokens with their lengths in pixels (zoomed, as computed values are), so that layout can
/// evaluate them without the element's fonts.
fn pixels(tokens: &TokenList<'static>, context: &Context) -> TokenList<'static> {
    TokenList(
        tokens
            .0
            .iter()
            .map(|token| match token {
                TokenOrValue::Length(length) => {
                    TokenOrValue::Length(LengthValue::Px(context.length_value(length)))
                }
                TokenOrValue::Function(function) => TokenOrValue::Function(Function {
                    name: function.name.clone(),
                    arguments: pixels(&function.arguments, context),
                }),
                other => other.clone(),
            })
            .collect(),
    )
}

/// An anchor's border box in the coordinates of the containing block, in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnchorRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// The property an anchor function is evaluated for: an inset of a side (0 top, 1 right, 2
/// bottom, 3 left), or a size or margin along an axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Inset(usize),
    Size { horizontal: bool },
}

/// What evaluating anchor functions needs from layout.
pub struct Environment<'a> {
    /// The anchor of a name, `None` for the default anchor (`position-anchor`).
    pub anchor: &'a dyn Fn(Option<&str>) -> Option<AnchorRect>,
    /// The containing block's width and height.
    pub containing: (f32, f32),
}

/// Evaluates a value with anchor functions for a slot to pixels; `None` where an anchor is
/// missing and no fallback is given (the value is then `auto`).
pub fn evaluate(tokens: &TokenList<'static>, slot: Slot, environment: &Environment<'_>) -> Option<f32> {
    let basis = match slot {
        Slot::Inset(side) if side % 2 == 0 => environment.containing.1,
        Slot::Inset(_) | Slot::Size { .. } => environment.containing.0,
    };
    let resolved = resolve_functions(tokens, slot, basis, environment)?;
    let quantity = crate::math::evaluate(&resolved.0, None)?;
    if quantity.powers != [1, 0, 0, 0] && !(quantity.powers == [0; 4] && quantity.value == 0.0) {
        return None;
    }
    #[expect(clippy::cast_possible_truncation, reason = "lengths are single precision")]
    Some(quantity.value as f32)
}

/// Tokens with `anchor()` and `anchor-size()` replaced by pixels and percentages by their part
/// of `basis`.
fn resolve_functions(
    tokens: &TokenList<'static>,
    slot: Slot,
    basis: f32,
    environment: &Environment<'_>,
) -> Option<TokenList<'static>> {
    let mut result = Vec::with_capacity(tokens.0.len());
    for token in &tokens.0 {
        result.push(match token {
            TokenOrValue::Function(function) if function.name.0.eq_ignore_ascii_case("anchor") => {
                TokenOrValue::Length(LengthValue::Px(anchor(&function.arguments, slot, environment)?))
            }
            TokenOrValue::Function(function) if function.name.0.eq_ignore_ascii_case("anchor-size") => {
                TokenOrValue::Length(LengthValue::Px(anchor_size(
                    &function.arguments,
                    slot,
                    environment,
                )?))
            }
            TokenOrValue::Function(function) => TokenOrValue::Function(Function {
                name: function.name.clone(),
                arguments: resolve_functions(&function.arguments, slot, basis, environment)?,
            }),
            TokenOrValue::Token(Token::Percentage { unit_value, .. }) => {
                TokenOrValue::Length(LengthValue::Px(unit_value * basis))
            }
            other => other.clone(),
        });
    }
    Some(TokenList(result))
}

/// The name and the rest of the first argument of an anchor function, and its fallback.
fn anchor_arguments<'t>(
    arguments: &'t TokenList<'static>,
) -> (
    Option<String>,
    Vec<&'t TokenOrValue<'static>>,
    Option<TokenList<'static>>,
) {
    let position = arguments
        .0
        .iter()
        .position(|token| matches!(token, TokenOrValue::Token(Token::Comma)));
    let (head, fallback) = match position {
        Some(index) => (
            &arguments.0[..index],
            Some(TokenList(arguments.0[index + 1..].to_vec())),
        ),
        None => (&arguments.0[..], None),
    };
    let mut head = significant(head);
    let mut name = None;
    if let Some(index) = head.iter().position(|token| dashed(token).is_some()) {
        name = dashed(head[index]);
        head.remove(index);
    }
    (name, head, fallback)
}

/// `anchor(<name>? <side>, <fallback>?)` for an inset: where the anchor's side lies, measured
/// from the containing block's edge of the inset.
fn anchor(arguments: &TokenList<'static>, slot: Slot, environment: &Environment<'_>) -> Option<f32> {
    let (name, head, fallback) = anchor_arguments(arguments);
    let fallback = || {
        fallback
            .as_ref()
            .and_then(|fallback| evaluate(fallback, slot, environment))
    };
    let Slot::Inset(side) = slot else {
        return None;
    };
    let Some(rect) = (environment.anchor)(name.as_deref()) else {
        return fallback();
    };
    let vertical = side % 2 == 0;
    let (start, size, extent) = if vertical {
        (rect.y, rect.height, environment.containing.1)
    } else {
        (rect.x, rect.width, environment.containing.0)
    };
    // How far along the anchor (0 its start, 1 its end) the side lies.
    let fraction = match head.as_slice() {
        [TokenOrValue::Token(Token::Percentage { unit_value, .. })] => *unit_value,
        [token] => match (ident(token).as_deref(), vertical) {
            (Some("top"), true) | (Some("left"), false) => 0.0,
            (Some("bottom"), true) | (Some("right"), false) => 1.0,
            (Some("start" | "self-start"), _) => 0.0,
            (Some("end" | "self-end"), _) => 1.0,
            (Some("center"), _) => 0.5,
            // The side of the inset itself, or the one opposite.
            (Some("inside"), _) => {
                if side == 0 || side == 3 {
                    0.0
                } else {
                    1.0
                }
            }
            (Some("outside"), _) => {
                if side == 0 || side == 3 {
                    1.0
                } else {
                    0.0
                }
            }
            _ => return fallback(),
        },
        _ => return fallback(),
    };
    let position = start + fraction * size;
    // `bottom` and `right` measure from the far edge.
    Some(if side == 1 || side == 2 {
        extent - position
    } else {
        position
    })
}

/// `anchor-size(<name>? <size>?, <fallback>?)`: the anchor's width or height.
fn anchor_size(arguments: &TokenList<'static>, slot: Slot, environment: &Environment<'_>) -> Option<f32> {
    let (name, head, fallback) = anchor_arguments(arguments);
    let fallback = || {
        fallback
            .as_ref()
            .and_then(|fallback| evaluate(fallback, slot, environment))
    };
    let Some(rect) = (environment.anchor)(name.as_deref()) else {
        return fallback();
    };
    let own_horizontal = match slot {
        Slot::Inset(side) => side % 2 == 1,
        Slot::Size { horizontal } => horizontal,
    };
    let horizontal = match head.as_slice() {
        [] => own_horizontal,
        [token] => match ident(token).as_deref() {
            Some("width" | "inline" | "self-inline") => true,
            Some("height" | "block" | "self-block") => false,
            _ => return fallback(),
        },
        _ => return fallback(),
    };
    Some(if horizontal { rect.width } else { rect.height })
}

/// Tokens for a flipped placement: the sides of `anchor()` mirrored in the block axis (`top` ↔
/// `bottom`), the inline axis (`left` ↔ `right`), or swapped between them (`start`).
#[must_use]
pub fn flip(tokens: &TokenList<'static>, block: bool, inline: bool, start: bool) -> TokenList<'static> {
    TokenList(
        tokens
            .0
            .iter()
            .map(|token| match token {
                TokenOrValue::Function(function) if function.name.0.eq_ignore_ascii_case("anchor") => {
                    let arguments = function
                        .arguments
                        .0
                        .iter()
                        .map(|argument| flip_side(argument, block, inline, start))
                        .collect();
                    TokenOrValue::Function(Function {
                        name: function.name.clone(),
                        arguments: TokenList(arguments),
                    })
                }
                TokenOrValue::Function(function) => TokenOrValue::Function(Function {
                    name: function.name.clone(),
                    arguments: flip(&function.arguments, block, inline, start),
                }),
                other => other.clone(),
            })
            .collect(),
    )
}

fn flip_side(token: &TokenOrValue<'static>, block: bool, inline: bool, start: bool) -> TokenOrValue<'static> {
    let word = |word: &str| TokenOrValue::Token(Token::Ident(word.to_owned().into()));
    match token {
        TokenOrValue::Token(Token::Ident(side)) => {
            let side = side.to_ascii_lowercase();
            let mut side = side.as_str();
            if start {
                side = match side {
                    "top" => "left",
                    "left" => "top",
                    "bottom" => "right",
                    "right" => "bottom",
                    other => other,
                };
            }
            let swapped = match side {
                "top" if block => "bottom",
                "bottom" if block => "top",
                "left" if inline => "right",
                "right" if inline => "left",
                // Logical sides are on either axis: mirrored by either flip.
                "start" if block || inline => "end",
                "end" if block || inline => "start",
                "self-start" if block || inline => "self-end",
                "self-end" if block || inline => "self-start",
                other => other,
            };
            word(swapped)
        }
        TokenOrValue::Token(Token::Percentage { unit_value, .. }) if block || inline => {
            TokenOrValue::Token(Token::Percentage {
                has_sign: false,
                unit_value: 1.0 - unit_value,
                int_value: None,
            })
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(css: &str) -> TokenList<'static> {
        crate::variables::value_tokens(css).unwrap_or_else(|| TokenList(Vec::new()))
    }

    fn environment(anchor: &dyn Fn(Option<&str>) -> Option<AnchorRect>) -> Environment<'_> {
        Environment {
            anchor,
            containing: (400.0, 300.0),
        }
    }

    #[test]
    fn resolves_anchor_sides_for_insets() {
        let rect = AnchorRect {
            x: 100.0,
            y: 40.0,
            width: 80.0,
            height: 30.0,
        };
        let find = |name: Option<&str>| (name.is_none() || name == Some("--a")).then_some(rect);
        let environment = environment(&find);
        let at = |css: &str, side: usize| evaluate(&tokens(css), Slot::Inset(side), &environment);
        assert_eq!(at("anchor(bottom)", 0), Some(70.0));
        assert_eq!(at("anchor(top)", 2), Some(260.0));
        assert_eq!(at("anchor(--a right)", 3), Some(180.0));
        assert_eq!(at("anchor(left)", 1), Some(300.0));
        assert_eq!(at("anchor(center)", 0), Some(55.0));
        assert_eq!(at("anchor(50%)", 3), Some(140.0));
        assert_eq!(at("calc(anchor(right) + 50px)", 3), Some(230.0));
        assert_eq!(at("anchor(outside)", 3), Some(180.0));
        assert_eq!(at("anchor(inside)", 0), Some(40.0));
        // A missing anchor takes the fallback, or makes the inset auto.
        assert_eq!(at("anchor(--b left, 25%)", 3), Some(100.0));
        assert_eq!(at("anchor(--b left)", 3), None);
        // A side of the other axis is invalid.
        assert_eq!(at("anchor(left, 7px)", 0), Some(7.0));
        assert_eq!(
            evaluate(
                &tokens("anchor-size(width)"),
                Slot::Size { horizontal: false },
                &environment
            ),
            Some(80.0)
        );
        assert_eq!(
            evaluate(
                &tokens("calc(anchor-size() * 2)"),
                Slot::Size { horizontal: false },
                &environment
            ),
            Some(60.0)
        );
    }

    #[test]
    fn parses_position_area() {
        let area = |css: &str| position_area(&significant(&tokens(css).0));
        assert_eq!(
            area("top center"),
            Some(PositionArea {
                columns: (1, 1),
                rows: (0, 0)
            })
        );
        assert_eq!(
            area("right span-bottom"),
            Some(PositionArea {
                columns: (2, 2),
                rows: (1, 2)
            })
        );
        assert_eq!(
            area("bottom"),
            Some(PositionArea {
                columns: (0, 2),
                rows: (2, 2)
            })
        );
        assert_eq!(
            area("span-x-start block-end"),
            Some(PositionArea {
                columns: (0, 1),
                rows: (2, 2)
            })
        );
        assert_eq!(
            area("center"),
            Some(PositionArea {
                columns: (1, 1),
                rows: (1, 1)
            })
        );
        assert_eq!(area("left right"), None);
        assert_eq!(area("top").map(|area| area.flipped(true, false)), area("bottom"));
    }

    #[test]
    fn flips_anchor_sides() {
        let flipped = flip(&tokens("calc(anchor(--a top) + 25%)"), true, false, false);
        assert_eq!(
            crate::variables::to_css(&flipped).as_deref().map(str::trim),
            Some("calc(anchor(--a bottom) + 25%)")
        );
        let options = fallbacks(&significant(
            &tokens("flip-block, flip-inline flip-block, --custom flip-start, bottom").0,
        ));
        assert_eq!(
            options,
            Some(vec![
                TryOption::Flip {
                    block: true,
                    inline: false,
                    start: false
                },
                TryOption::Flip {
                    block: true,
                    inline: true,
                    start: false
                },
                // A `@position-try` rule, which the cascade applies, with flips after it.
                TryOption::Rule {
                    name: "--custom".to_owned(),
                    flip: (false, false, true),
                    style: None,
                },
                TryOption::Area(PositionArea {
                    columns: (0, 2),
                    rows: (2, 2)
                }),
            ])
        );
        // Two rules in one option are not one.
        assert_eq!(fallbacks(&significant(&tokens("--a --b").0)), None);
    }

    #[test]
    fn reads_the_try_order_and_the_visibility() {
        let anchoring = |declarations: &[(&str, &str)]| {
            let mut style = ComputedStyle::default();
            for (name, value) in declarations {
                assert!(apply_property(name, &tokens(value), &mut style));
            }
            style.anchoring.as_deref().cloned().unwrap_or_default()
        };
        assert_eq!(anchoring(&[]).visibility, PositionVisibility::default());
        assert!(PositionVisibility::default().anchors_visible);
        let set = anchoring(&[
            ("position-try-order", "most-block-size"),
            ("position-visibility", "no-overflow anchors-visible"),
        ]);
        assert_eq!(set.order, TryOrder::MostHeight);
        assert_eq!(
            set.visibility,
            PositionVisibility {
                anchors_visible: true,
                no_overflow: true
            }
        );
        let always = anchoring(&[("position-visibility", "always")]);
        assert!(!always.visibility.anchors_visible && !always.visibility.no_overflow);
        // Invalid values leave the initial ones.
        let invalid = anchoring(&[
            ("position-visibility", "always no-overflow"),
            ("position-try-order", "most-depth"),
        ]);
        assert_eq!(invalid.visibility, PositionVisibility::default());
        assert_eq!(invalid.order, TryOrder::Normal);
        // The shorthand sets the order with the fallbacks.
        let shorthand = anchoring(&[("position-try", "most-width flip-inline")]);
        assert_eq!(shorthand.order, TryOrder::MostWidth);
        assert_eq!(shorthand.fallbacks.len(), 1);
    }
}

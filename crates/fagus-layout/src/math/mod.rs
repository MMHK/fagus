//! `MathML` Core: formulas in `<math>` laid out as Chrome lays them out
//! (`third_party/blink/renderer/core/layout/mathml`, BSD licensed: see `CHROMIUM-LICENSE`).
//!
//! A `<math>` element is one box of the box tree, an atomic inline or a block. Its content is
//! a tree of [`MathNode`]s built from the `MathML` elements inside it, which [`layout`] lays out
//! with the constants and stretchy glyphs of the math font's OpenType `MATH` table into text
//! runs and rules on one line.

mod layout;
pub(crate) mod operators;
mod stretch;
mod table;

use std::sync::Arc;

use fagus_dom::{Document, ElementData, NodeId};
use fagus_style::values::Display;
use fagus_style::{ComputedStyle, StyledDocument};

use crate::units::Au;
pub(crate) use layout::LaidFormula;

/// The formula of a `<math>` box, with its layouts by the width a block formula is centered in.
#[derive(Debug)]
pub(crate) struct MathBox {
    formula: Formula,
    /// Whether the formula is a block (`display="block"`), centered in its box.
    pub(crate) block: bool,
    laid: std::sync::Mutex<Vec<(Option<Au>, Arc<LaidFormula>)>>,
}

impl MathBox {
    pub(crate) fn new(formula: Formula, block: bool) -> Self {
        Self {
            formula,
            block,
            laid: std::sync::Mutex::default(),
        }
    }

    /// The formula laid out, centered in `available` if it is a block.
    pub(crate) fn laid(&self, fonts: &fagus_text::FontDb, available: Option<Au>) -> Arc<LaidFormula> {
        let available = available.filter(|_| self.block);
        if let Ok(laid) = self.laid.lock()
            && let Some((_, laid)) = laid.iter().find(|(width, _)| *width == available)
        {
            return Arc::clone(laid);
        }
        let laid = Arc::new(layout::layout(fonts, &self.formula, available));
        if let Ok(mut cache) = self.laid.lock() {
            cache.push((available, Arc::clone(&laid)));
        }
        laid
    }
}

/// The namespace of `MathML`.
pub(crate) const MATHML: &str = "http://www.w3.org/1998/Math/MathML";

/// Whether an element is a `MathML` `<math>` element.
pub(crate) fn is_math(data: &ElementData) -> bool {
    is_mathml(data) && &*data.name.local == "math"
}

/// Whether an element is in the `MathML` namespace.
fn is_mathml(data: &ElementData) -> bool {
    &*data.name.ns == MATHML
}

/// A formula: the content of a `<math>` element.
#[derive(Debug)]
pub(crate) struct Formula {
    /// The `<math>` element, laid out as a row.
    pub(crate) root: MathNode,
}

/// A `MathML` element in a formula, or an anonymous row.
#[derive(Debug)]
pub(crate) struct MathNode {
    pub(crate) style: Arc<ComputedStyle>,
    pub(crate) kind: Kind,
    pub(crate) children: Vec<MathNode>,
    /// Whether the node's direction differs from its parent's (the root's from left to right):
    /// laid out left to right, it is mirrored (see `layout::Laid::mirror`).
    pub(crate) mirrored: bool,
}

/// What a `MathML` element is, for layout.
#[derive(Debug)]
pub(crate) enum Kind {
    /// `<mrow>` and the elements laid out like it: `<math>`, `<mstyle>`, `<merror>`,
    /// `<mphantom>`, `<semantics>`, unknown elements, and the anonymous rows of `<msqrt>`,
    /// `<mpadded>` and `<mtd>`. `phantom` rows draw nothing.
    Row { grouping: bool },
    /// `<mi>`, `<mn>`, `<mtext>` and `<ms>`: text, and the text it was before `<mi>` made it
    /// italic where it differs: Chrome measures the ink of the token's own characters.
    Token {
        text: String,
        source: Option<String>,
        space_like: bool,
    },
    /// `<mo>`.
    Operator(Box<Operator>),
    /// `<mspace>`, with its `width`, `height` and `depth`.
    Space {
        width: Option<f32>,
        height: Option<f32>,
        depth: Option<f32>,
    },
    /// `<mfrac>`, with its `linethickness`.
    Fraction { thickness: Option<MathLength> },
    /// `<msqrt>`: its one child is the anonymous row of its content.
    Sqrt,
    /// `<mroot>`: base and index.
    Root,
    /// `<msub>`, `<msup>`, `<msubsup>`, `<mmultiscripts>`, `<munder>`, `<mover>` and
    /// `<munderover>`.
    Scripts {
        kind: ScriptKind,
        accent: bool,
        accent_under: bool,
    },
    /// `<mprescripts>` in `<mmultiscripts>`.
    Prescripts,
    /// `<mpadded>`: its one child is the anonymous row of its content.
    Padded(Box<Padding>),
    /// `<mtable>`, whose children are rows.
    Table,
    /// `<mtr>`, whose children are cells.
    TableRow,
    /// `<mtd>`: its one child is the anonymous row of its content. A `row_span` of 0 spans the
    /// rows to the end of the table, as in HTML.
    TableCell { column_span: usize, row_span: usize },
}

/// The kinds of scripted elements.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScriptKind {
    Sub,
    Super,
    SubSuper,
    Multiscripts,
    Under,
    Over,
    UnderOver,
}

impl ScriptKind {
    fn is_under_over(self) -> bool {
        matches!(self, Self::Under | Self::Over | Self::UnderOver)
    }
}

/// An `<mo>` element: its text and the properties of the operator.
#[derive(Debug)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "the flags of MathML Core's operators"
)]
pub(crate) struct Operator {
    pub(crate) text: String,
    pub(crate) stretchy: bool,
    pub(crate) symmetric: bool,
    pub(crate) largeop: bool,
    pub(crate) movablelimits: bool,
    /// The space before and after it in pixels.
    pub(crate) lspace: f32,
    pub(crate) rspace: f32,
    pub(crate) minsize: Option<MathLength>,
    pub(crate) maxsize: Option<MathLength>,
}

/// The attributes of an `<mpadded>`, in pixels.
#[derive(Debug, Default)]
pub(crate) struct Padding {
    pub(crate) width: Option<f32>,
    pub(crate) height: Option<f32>,
    pub(crate) depth: Option<f32>,
    pub(crate) lspace: Option<f32>,
    pub(crate) voffset: Option<f32>,
}

/// A length of a `MathML` attribute: in pixels, or a percentage of a default.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum MathLength {
    Px(f32),
    Percent(f32),
}

impl MathLength {
    /// The length in pixels, a percentage of `default`.
    pub(crate) fn resolve(self, default: f32) -> f32 {
        match self {
            Self::Px(px) => px,
            Self::Percent(percent) => default * percent / 100.0,
        }
    }
}

/// Reads a length as Chrome reads `MathML` attributes: a CSS length (a number with a unit, or 0)
/// or, where `percentages`, a percentage; units relative to the font refer to `style`.
pub(crate) fn parse_length(value: &str, style: &ComputedStyle, percentages: bool) -> Option<MathLength> {
    let value = value.trim();
    let split = value
        .find(|c: char| c.is_ascii_alphabetic() || c == '%')
        .unwrap_or(value.len());
    let (number, unit) = value.split_at(split);
    if number.is_empty() || number.ends_with('.') || number.contains(['e', 'E']) {
        return None;
    }
    let number: f32 = number.parse().ok().filter(|number: &f32| number.is_finite())?;
    let em = style.font_size;
    let px = match unit.to_ascii_lowercase().as_str() {
        "" if number == 0.0 => 0.0,
        "%" if percentages => return Some(MathLength::Percent(number)),
        "px" => number,
        "em" => number * em,
        "ex" => number * em * style.font_units.ex,
        "ch" => number * em * style.font_units.ch,
        "pt" => number * 4.0 / 3.0,
        "pc" => number * 16.0,
        "in" => number * 96.0,
        "cm" => number * 96.0 / 2.54,
        "mm" => number * 96.0 / 25.4,
        "q" => number * 96.0 / 101.6,
        _ => return None,
    };
    Some(MathLength::Px(px * style.zoom))
}

/// A non-negative length in pixels, without percentages.
fn parse_size(data: &ElementData, attribute: &str, style: &ComputedStyle) -> Option<f32> {
    match parse_length(data.attribute(attribute)?, style, false)? {
        MathLength::Px(px) if px >= 0.0 => Some(px),
        _ => None,
    }
}

/// A non-negative integer as HTML reads one: digits after white space and an optional `+`,
/// whatever follows them left out (Blink's `ParseHTMLClampedNonNegativeInteger`).
fn html_integer(value: &str) -> Option<usize> {
    let value = value.trim_start_matches(|c: char| c.is_ascii_whitespace());
    let value = value.strip_prefix('+').unwrap_or(value);
    let digits = value.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    // Too many digits clamp to the maximum.
    Some(value[..digits].parse::<usize>().unwrap_or(usize::MAX))
}

/// `true` or `false` in any case; `None` for anything else.
fn boolean(data: &ElementData, attribute: &str) -> Option<bool> {
    let value = data.attribute(attribute)?.trim();
    if value.eq_ignore_ascii_case("true") {
        Some(true)
    } else if value.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

/// `scriptPercentScaleDown` and `scriptScriptPercentScaleDown` of a face's `MATH` table, which
/// `font-size: math` scales by; zero takes the defaults, as in Blink.
pub(crate) fn script_scales(face: &fagus_text::Face) -> Option<(f32, f32)> {
    let script = face.math_constant(fagus_text::MathConstant::ScriptPercentScaleDown)?;
    let script_script = face
        .math_constant(fagus_text::MathConstant::ScriptScriptPercentScaleDown)
        .unwrap_or(0.0);
    let script = if script > 0.0 { script } else { 0.71 };
    let script_script = if script_script > 0.0 {
        script_script
    } else {
        0.71 * 0.71
    };
    Some((script, script_script))
}

/// The alternative text of a formula for readers: its `alttext`, else the text of its TeX
/// annotation (`<annotation encoding="application/x-tex">`), else the text of its tokens;
/// `None` if all are empty.
pub fn alternative_text(document: &Document, math: NodeId) -> Option<String> {
    let data = document.element(math)?;
    let normalize = |text: &str| {
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        (!text.is_empty()).then_some(text)
    };
    if let Some(alt) = data.attribute("alttext").and_then(normalize) {
        return Some(alt);
    }
    let is_annotation = |node: NodeId| {
        document.element(node).is_some_and(|data| {
            is_mathml(data) && matches!(&*data.name.local, "annotation" | "annotation-xml")
        })
    };
    let tex = document.descendants(math).find(|&node| {
        is_annotation(node)
            && document.element(node).is_some_and(|data| {
                data.attribute("encoding").is_some_and(|encoding| {
                    let encoding = encoding.trim();
                    encoding.eq_ignore_ascii_case("application/x-tex")
                        || encoding.eq_ignore_ascii_case("text/x-tex")
                })
            })
    });
    if let Some(text) = tex.and_then(|tex| normalize(&document.text_content(tex))) {
        return Some(text);
    }
    // The text of the tokens, leaving out annotations.
    let mut text = String::new();
    let mut stack: Vec<NodeId> = document.children(math).collect();
    stack.reverse();
    while let Some(node) = stack.pop() {
        if is_annotation(node) {
            continue;
        }
        if let Some(own) = document.text(node) {
            text.push_str(own);
            text.push(' ');
        }
        let children: Vec<NodeId> = document.children(node).collect();
        stack.extend(children.into_iter().rev());
    }
    normalize(&text)
}

/// Builds the formula of a `<math>` element from its `MathML` elements and their styles.
pub(crate) fn build(document: &Document, styled: &StyledDocument, math: NodeId) -> Option<Formula> {
    // Decorations of the text around do not reach into the formula, an atomic inline.
    let outer = document
        .parent(math)
        .and_then(|parent| styled.style(parent))
        .map_or(0, |parent| parent.decorations.len());
    let builder = Builder {
        document,
        styled,
        depth: std::cell::Cell::new(0),
        outer,
    };
    let mut root = builder.node(math)?;
    mark_mirrored(&mut root, false);
    Some(Formula { root })
}

/// Marks the nodes whose direction differs from that of their parent, `rtl`.
fn mark_mirrored(node: &mut MathNode, rtl: bool) {
    let own = node.style.is_rtl();
    node.mirrored = own != rtl;
    for child in &mut node.children {
        mark_mirrored(child, own);
    }
}

/// How deep elements in a formula are laid out; deeper ones are left out, so that no input
/// exhausts the stack.
const MAX_DEPTH: usize = 128;

struct Builder<'a> {
    document: &'a Document,
    styled: &'a StyledDocument,
    /// How deep in the formula the element being built is.
    depth: std::cell::Cell<usize>,
    /// How many decorations of an element's style come from outside the formula.
    outer: usize,
}

impl Builder<'_> {
    /// The nodes of the children of an element that are laid out: its `MathML` elements that are
    /// displayed. Text and other elements have no boxes in Chrome.
    fn children(&self, element: NodeId) -> Vec<MathNode> {
        self.document
            .child_elements(element)
            .filter_map(|child| self.node(child))
            .collect()
    }

    /// The content of an element as one anonymous row, as Chrome wraps that of `<msqrt>`,
    /// `<mpadded>` and `<mtd>`.
    fn anonymous_row(&self, element: NodeId, style: &Arc<ComputedStyle>) -> Vec<MathNode> {
        // The row is anonymous: it inherits the element's style, without its box.
        vec![MathNode {
            style: Arc::new(style.inherit()),
            kind: Kind::Row { grouping: true },
            children: self.children(element),
            mirrored: false,
        }]
    }

    fn node(&self, element: NodeId) -> Option<MathNode> {
        if self.depth.get() >= MAX_DEPTH {
            return None;
        }
        self.depth.set(self.depth.get() + 1);
        let node = self.element(element);
        self.depth.set(self.depth.get() - 1);
        node
    }

    /// The node of an element (see [`Builder::node`]).
    fn element(&self, element: NodeId) -> Option<MathNode> {
        let data = self.document.element(element)?;
        if !is_mathml(data) {
            return None;
        }
        let style = self.styled.style(element)?;
        if style.display == Display::None || style.is_out_of_flow() {
            return None;
        }
        let style = if self.outer > 0 && style.decorations.len() >= self.outer {
            Arc::new(ComputedStyle {
                decorations: style.decorations[self.outer..].into(),
                ..(**style).clone()
            })
        } else {
            Arc::clone(style)
        };
        let name = &*data.name.local;
        let (kind, children) = match name {
            "mi" | "mn" | "mtext" | "ms" => {
                let source = token_text(self.document, element);
                let text = if name == "mi"
                    && !data
                        .attribute("mathvariant")
                        .is_some_and(|variant| variant.trim().eq_ignore_ascii_case("normal"))
                {
                    italic(&source)
                } else {
                    source.clone()
                };
                (
                    Kind::Token {
                        source: (source != text).then_some(source),
                        text,
                        space_like: name == "mtext",
                    },
                    Vec::new(),
                )
            }
            "mo" => (
                Kind::Operator(Box::new(self.operator(element, data, &style))),
                Vec::new(),
            ),
            "mspace" => (
                Kind::Space {
                    width: parse_size(data, "width", &style),
                    height: parse_size(data, "height", &style),
                    depth: parse_size(data, "depth", &style),
                },
                Vec::new(),
            ),
            "mfrac" => (
                Kind::Fraction {
                    thickness: data
                        .attribute("linethickness")
                        .and_then(|value| parse_length(value, &style, true))
                        .filter(|length| !matches!(length, MathLength::Px(px) if *px < 0.0)),
                },
                self.children(element),
            ),
            "msqrt" => (Kind::Sqrt, self.anonymous_row(element, &style)),
            "mroot" => (Kind::Root, self.children(element)),
            "msub" | "msup" | "msubsup" | "mmultiscripts" | "munder" | "mover" | "munderover" => {
                let kind = match name {
                    "msub" => ScriptKind::Sub,
                    "msup" => ScriptKind::Super,
                    "msubsup" => ScriptKind::SubSuper,
                    "mmultiscripts" => ScriptKind::Multiscripts,
                    "munder" => ScriptKind::Under,
                    "mover" => ScriptKind::Over,
                    _ => ScriptKind::UnderOver,
                };
                (
                    Kind::Scripts {
                        kind,
                        accent: boolean(data, "accent") == Some(true),
                        accent_under: boolean(data, "accentunder") == Some(true),
                    },
                    self.children(element),
                )
            }
            "mprescripts" => (Kind::Prescripts, Vec::new()),
            "mpadded" => (
                Kind::Padded(Box::new(Padding {
                    width: parse_size(data, "width", &style),
                    height: parse_size(data, "height", &style),
                    depth: parse_size(data, "depth", &style),
                    lspace: parse_size(data, "lspace", &style),
                    voffset: data
                        .attribute("voffset")
                        .and_then(|value| parse_length(value, &style, false))
                        .map(|length| length.resolve(0.0)),
                })),
                self.anonymous_row(element, &style),
            ),
            "mtable" => (Kind::Table, self.children(element)),
            "mtr" => (Kind::TableRow, self.children(element)),
            "mtd" => (
                Kind::TableCell {
                    column_span: data
                        .attribute("columnspan")
                        .and_then(html_integer)
                        .map_or(1, |span| span.clamp(1, 1000)),
                    row_span: data
                        .attribute("rowspan")
                        .and_then(html_integer)
                        .map_or(1, |span| span.min(65534)),
                },
                self.anonymous_row(element, &style),
            ),
            "mrow" | "mstyle" | "merror" | "mphantom" | "math" | "semantics" | "maction" => {
                (Kind::Row { grouping: true }, self.children(element))
            }
            _ => (Kind::Row { grouping: false }, self.children(element)),
        };
        Some(MathNode {
            style,
            kind,
            children,
            mirrored: false,
        })
    }

    /// The operator of an `<mo>`: its properties from its attributes and the dictionary, by its
    /// form (`MathML` Core, §3.2.4).
    fn operator(&self, element: NodeId, data: &ElementData, style: &Arc<ComputedStyle>) -> Operator {
        use operators::Form;
        let text = token_text(self.document, element);
        let form = data.attribute("form").and_then(|form| {
            let form = form.trim();
            if form.eq_ignore_ascii_case("prefix") {
                Some(Form::Prefix)
            } else if form.eq_ignore_ascii_case("infix") {
                Some(Form::Infix)
            } else if form.eq_ignore_ascii_case("postfix") {
                Some(Form::Postfix)
            } else {
                None
            }
        });
        let previous = self.document.previous_sibling_element(element).is_some();
        let next = self.document.next_sibling_element(element).is_some();
        let position = match (previous, next) {
            (false, true) => Form::Prefix,
            (true, false) => Form::Postfix,
            _ => Form::Infix,
        };
        let entry = operators::lookup(&text, form, position);
        let flag = |attribute: &str, default: bool| boolean(data, attribute).unwrap_or(default);
        // The spaces of the dictionary are in eighteenths of an em, as layout units, of the
        // font size rounded to whole pixels (Blink's `ComputedStyle::FontSize()`).
        let space = |eighteenths: u8, attribute: &str| {
            let em = (style.font_size + 0.5).floor();
            let default = layout::lu(f32::from(eighteenths) / 18.0 * em);
            data.attribute(attribute)
                .and_then(|value| parse_length(value, style, true))
                .map_or(default, |length| length.resolve(default))
                .max(0.0)
        };
        Operator {
            stretchy: flag("stretchy", entry.stretchy),
            symmetric: flag("symmetric", entry.symmetric),
            largeop: flag("largeop", entry.largeop),
            movablelimits: flag("movablelimits", entry.movablelimits),
            lspace: space(entry.lspace, "lspace"),
            rspace: space(entry.rspace, "rspace"),
            minsize: data
                .attribute("minsize")
                .and_then(|value| parse_length(value, style, true)),
            maxsize: data
                .attribute("maxsize")
                .and_then(|value| parse_length(value, style, true)),
            text,
        }
    }
}

/// The text of a token element, with its white space collapsed and trimmed as text on a line
/// of its own is.
fn token_text(document: &Document, element: NodeId) -> String {
    let text = document.text_content(element);
    text.split(|c: char| c.is_ascii_whitespace())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// `text-transform: math-auto`: a single letter in its mathematical italic form (Blink's
/// `ItalicMathVariant`, `MathML` Core §C.1); other text as it is.
pub(crate) fn italic(text: &str) -> String {
    let mut characters = text.chars();
    let (Some(character), None) = (characters.next(), characters.next()) else {
        return text.to_owned();
    };
    italic_variant(character).to_string()
}

fn italic_variant(character: char) -> char {
    const BOLD_UPPER_A: u32 = 0x1D400;
    const ITALIC_UPPER_A: u32 = 0x1D434;
    const BOLD_UPPER_ALPHA: u32 = 0x1D6A8;
    const ITALIC_UPPER_ALPHA: u32 = 0x1D6E2;
    let code = character as u32;
    let greek = |offset: u32| char::from_u32(ITALIC_UPPER_ALPHA + offset).unwrap_or(character);
    match character {
        // Reserved, digammas: nothing to map to.
        '\u{03A2}' | '\u{03DC}' | '\u{03DD}' => character,
        '\u{0131}' => '\u{1D6A4}',
        '\u{0237}' => '\u{1D6A5}',
        'A'..='Z' | 'a'..='z' => {
            let offset = if character.is_ascii_uppercase() {
                code - u32::from('A')
            } else {
                0x1D41A - BOLD_UPPER_A + code - u32::from('a')
            };
            let mapped = ITALIC_UPPER_A + offset;
            // The italic small h is Planck's constant, outside the block.
            if mapped == 0x1D455 {
                '\u{210E}'
            } else {
                char::from_u32(mapped).unwrap_or(character)
            }
        }
        '\u{0391}'..='\u{03A9}' => greek(code - 0x0391),
        '\u{03B1}'..='\u{03C9}' => greek(0x1D6C2 - BOLD_UPPER_ALPHA + code - 0x03B1),
        '\u{03F4}' => greek(0x1D6B9 - BOLD_UPPER_ALPHA),
        '\u{2207}' => greek(0x1D6C1 - BOLD_UPPER_ALPHA),
        '\u{2202}' => greek(0x1D6DB - BOLD_UPPER_ALPHA),
        '\u{03F5}' => greek(0x1D6DC - BOLD_UPPER_ALPHA),
        '\u{03D1}' => greek(0x1D6DD - BOLD_UPPER_ALPHA),
        '\u{03F0}' => greek(0x1D6DE - BOLD_UPPER_ALPHA),
        '\u{03D5}' => greek(0x1D6DF - BOLD_UPPER_ALPHA),
        '\u{03F1}' => greek(0x1D6E0 - BOLD_UPPER_ALPHA),
        '\u{03D6}' => greek(0x1D6E1 - BOLD_UPPER_ALPHA),
        _ => character,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_single_letters_to_italics() {
        assert_eq!(italic("x"), "𝑥");
        assert_eq!(italic("A"), "𝐴");
        assert_eq!(italic("h"), "ℎ");
        assert_eq!(italic("α"), "𝛼");
        assert_eq!(italic("Ω"), "𝛺");
        assert_eq!(italic("∂"), "𝜕");
        assert_eq!(italic("ϕ"), "𝜙");
        assert_eq!(italic("ı"), "𝚤");
        assert_eq!(italic("sin"), "sin");
        assert_eq!(italic("2"), "2");
    }

    #[test]
    fn reads_spans_as_html_reads_integers() {
        assert_eq!(html_integer(" +3px"), Some(3));
        assert_eq!(html_integer("0"), Some(0));
        assert_eq!(html_integer("-1"), None);
        assert_eq!(html_integer("two"), None);
        assert_eq!(html_integer("99999999999999999999999"), Some(usize::MAX));
    }

    #[test]
    fn reads_lengths_of_attributes() {
        let style = ComputedStyle {
            font_size: 20.0,
            ..ComputedStyle::initial_values().clone()
        };
        assert_eq!(parse_length("2em", &style, false), Some(MathLength::Px(40.0)));
        assert_eq!(parse_length(" 3px ", &style, false), Some(MathLength::Px(3.0)));
        assert_eq!(parse_length("0", &style, false), Some(MathLength::Px(0.0)));
        assert_eq!(parse_length("1", &style, false), None);
        assert_eq!(parse_length("50%", &style, true), Some(MathLength::Percent(50.0)));
        assert_eq!(parse_length("50%", &style, false), None);
        assert_eq!(parse_length("thinmathspace", &style, false), None);
        assert_eq!(parse_length("-0.5em", &style, false), Some(MathLength::Px(-10.0)));
    }
}

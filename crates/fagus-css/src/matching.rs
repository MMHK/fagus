//! Selector matching on the Fagus DOM.
//!
//! Selectors are stored by `parcel_selectors` in match order: the rightmost compound comes first,
//! combinators are separate components. Matching walks that order, moving from the element to its
//! ancestors or previous siblings. Interaction states (`:hover`, `:focus`, …) never match because a
//! printed document has no interaction.

use fagus_dom::{Document, NodeId, ns};
use lightningcss::selector::{PseudoClass, PseudoElement};
use lightningcss::vendor_prefix::VendorPrefix;
use parcel_selectors::attr::{
    AttrSelectorOperator, NamespaceConstraint, ParsedAttrSelectorOperation, ParsedCaseSensitivity,
};
use parcel_selectors::parser::{Combinator, NthSelectorData, NthType};

use crate::ruleset::Scope;
use crate::{Component, Selector, SelectorList};

/// The pseudo-elements Fagus generates boxes for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PseudoElementKind {
    Before,
    After,
    Marker,
    FirstLine,
    FirstLetter,
    /// `::-webkit-progress-bar`: the track of a `<progress>` without its native look.
    ProgressBar,
    /// `::-webkit-progress-value`: the filled part of that track.
    ProgressValue,
    /// `::-webkit-meter-bar`: the track of a `<meter>`.
    MeterBar,
    /// `::-webkit-meter-optimum-value`: the filled part of a meter whose value is optimum.
    MeterOptimumValue,
    /// `::-webkit-meter-suboptimum-value`: the filled part of a meter whose value is suboptimal.
    MeterSuboptimumValue,
    /// `::-webkit-meter-even-less-good-value`: the filled part of a meter whose value is even
    /// less good.
    MeterEvenLessGoodValue,
    /// `::placeholder` (`::-webkit-input-placeholder`): the placeholder text of an empty text
    /// field or text area.
    Placeholder,
    /// `::-webkit-slider-runnable-track`: the track of a slider.
    SliderTrack,
    /// `::-webkit-slider-thumb`: the thumb of a slider, in its track.
    SliderThumb,
    /// `::scroll-marker`: the marker of an element in the scroll marker group of the scroll
    /// container it is in (CSS Overflow 5, §3).
    ScrollMarker,
    /// `::scroll-marker-group`: the box that holds the scroll markers of a scroll container,
    /// before or after it.
    ScrollMarkerGroup,
    /// `::scroll-button(<direction>)`: a button that scrolls the element; `None` for
    /// `::scroll-button(*)`, which styles all four.
    ScrollButton(Option<ScrollDirection>),
    /// `::column`: a column of a multi-column scroll container, which only has a
    /// `::scroll-marker` in print.
    Column,
    /// `::column::scroll-marker`: the scroll marker of a column.
    ColumnScrollMarker,
    /// Pseudo-elements without effect on print (`::selection`, `::backdrop`, …).
    Other,
}

/// The direction a `::scroll-button()` scrolls in, in its generation order (CSS Overflow 5,
/// §4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScrollDirection {
    BlockStart,
    InlineStart,
    InlineEnd,
    BlockEnd,
}

impl ScrollDirection {
    /// The four, in the order their buttons follow each other, as in Chrome.
    pub const ALL: [Self; 4] = [
        Self::BlockStart,
        Self::InlineStart,
        Self::InlineEnd,
        Self::BlockEnd,
    ];

    fn of(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "block-start" => Some(Self::BlockStart),
            "inline-start" => Some(Self::InlineStart),
            "inline-end" => Some(Self::InlineEnd),
            "block-end" => Some(Self::BlockEnd),
            // The physical directions as the logical ones of horizontal left-to-right text.
            "up" => Some(Self::BlockStart),
            "left" => Some(Self::InlineStart),
            "right" => Some(Self::InlineEnd),
            "down" => Some(Self::BlockEnd),
            _ => None,
        }
    }
}

impl PseudoElementKind {
    pub fn of(pseudo_element: &PseudoElement<'_>) -> Self {
        match pseudo_element {
            PseudoElement::Before => Self::Before,
            PseudoElement::After => Self::After,
            PseudoElement::Marker => Self::Marker,
            PseudoElement::FirstLine => Self::FirstLine,
            PseudoElement::FirstLetter => Self::FirstLetter,
            // Chrome knows neither `::-moz-placeholder` nor `::-ms-input-placeholder`.
            PseudoElement::Placeholder(VendorPrefix::None | VendorPrefix::WebKit) => Self::Placeholder,
            PseudoElement::Custom { name } if name.eq_ignore_ascii_case("-webkit-progress-bar") => {
                Self::ProgressBar
            }
            PseudoElement::Custom { name } if name.eq_ignore_ascii_case("-webkit-progress-value") => {
                Self::ProgressValue
            }
            PseudoElement::Custom { name } if name.eq_ignore_ascii_case("-webkit-slider-runnable-track") => {
                Self::SliderTrack
            }
            PseudoElement::Custom { name } if name.eq_ignore_ascii_case("-webkit-slider-thumb") => {
                Self::SliderThumb
            }
            PseudoElement::Custom { name } if name.eq_ignore_ascii_case("-webkit-meter-bar") => {
                Self::MeterBar
            }
            PseudoElement::Custom { name } if name.eq_ignore_ascii_case("-webkit-meter-optimum-value") => {
                Self::MeterOptimumValue
            }
            PseudoElement::Custom { name } if name.eq_ignore_ascii_case("-webkit-meter-suboptimum-value") => {
                Self::MeterSuboptimumValue
            }
            PseudoElement::Custom { name }
                if name.eq_ignore_ascii_case("-webkit-meter-even-less-good-value") =>
            {
                Self::MeterEvenLessGoodValue
            }
            PseudoElement::Custom { name } if name.eq_ignore_ascii_case("scroll-marker") => {
                Self::ScrollMarker
            }
            PseudoElement::Custom { name } if name.eq_ignore_ascii_case("scroll-marker-group") => {
                Self::ScrollMarkerGroup
            }
            PseudoElement::Custom { name } if name.eq_ignore_ascii_case("column") => Self::Column,
            PseudoElement::CustomFunction { name, arguments }
                if name.eq_ignore_ascii_case("scroll-button") =>
            {
                use lightningcss::properties::custom::{Token, TokenOrValue};
                let tokens: Vec<&TokenOrValue<'_>> = arguments
                    .0
                    .iter()
                    .filter(|token| !matches!(token, TokenOrValue::Token(Token::WhiteSpace(_))))
                    .collect();
                match tokens.as_slice() {
                    [TokenOrValue::Token(Token::Delim('*'))] => Self::ScrollButton(None),
                    [TokenOrValue::Token(Token::Ident(name))] => ScrollDirection::of(name)
                        .map_or(Self::Other, |direction| Self::ScrollButton(Some(direction))),
                    _ => Self::Other,
                }
            }
            _ => Self::Other,
        }
    }

    /// The kind of the pseudo-element a selector (`selector.pseudo_element()` being one)
    /// selects: `::column::scroll-marker` chains two.
    pub fn of_selector(selector: &Selector) -> Option<Self> {
        let first = Self::of(selector.pseudo_element()?);
        let kinds = || {
            selector
                .iter_raw_match_order()
                .filter_map(|component| match component {
                    Component::PseudoElement(pseudo_element) => Some(Self::of(pseudo_element)),
                    _ => None,
                })
        };
        if kinds().any(|kind| kind == Self::Column) && kinds().any(|kind| kind == Self::ScrollMarker) {
            return Some(Self::ColumnScrollMarker);
        }
        Some(first)
    }

    /// Whether a rule for pseudo-elements of kind `self` styles those of kind `requested`:
    /// `::scroll-button(*)` styles every scroll button.
    pub fn styles(self, requested: Self) -> bool {
        self == requested || (self == Self::ScrollButton(None) && matches!(requested, Self::ScrollButton(_)))
    }
}

/// Whether a selector refers to the root of its scope with `:scope`.
fn contains_scope(selector: &Selector) -> bool {
    selector
        .iter_raw_match_order()
        .any(|component| matches!(component, Component::Scope))
}

/// Whether a selector refers to the rule around it with `&`.
fn contains_nesting(selector: &Selector) -> bool {
    selector
        .iter_raw_match_order()
        .any(|component| matches!(component, Component::Nesting))
}

/// The selector list that `&` refers to inside a nested rule, with its own parent.
#[derive(Debug)]
pub struct Nesting {
    pub selectors: SelectorList,
    pub parent: Option<std::sync::Arc<Nesting>>,
}

/// Matches selectors against the elements of one document.
#[derive(Clone, Copy, Debug)]
pub struct Matcher<'a> {
    document: &'a Document,
    /// What the ancestors of the elements are, where known.
    ancestors: Option<&'a Ancestors>,
}

/// A Bloom filter of names, IDs and classes: 256 bits, two for each.
type Bloom = [u64; 4];

/// What kind of name a [`Bloom`] holds.
#[derive(Clone, Copy)]
enum Feature {
    Tag,
    Id,
    Class,
}

/// The bits of a name in a [`Bloom`], case aside: names match without regard to case in quirks
/// mode and HTML, and a filter that holds more names only tells less.
fn feature_bits(kind: Feature, name: &str) -> Bloom {
    use std::hash::Hasher;
    let mut hasher = rustc_hash::FxHasher::default();
    hasher.write_u8(kind as u8);
    for byte in name.bytes() {
        hasher.write_u8(byte.to_ascii_lowercase());
    }
    let hash = hasher.finish();
    let mut bloom = [0; 4];
    for bit in [hash >> 56, (hash >> 48) & 0xff] {
        bloom[(bit / 64) as usize] |= 1 << (bit % 64);
    }
    bloom
}

/// The names, IDs and classes of the ancestors of each element of a document, as a Bloom filter
/// (as Blink's `SelectorFilter` keeps them): a descendant combinator need not look for an
/// ancestor that the filter says none of them can be.
#[derive(Debug)]
pub struct Ancestors {
    /// By node; all bits set for nodes outside the tree, which are not filtered.
    filters: Vec<Bloom>,
    /// Where each element stands among its siblings, counted when a selector first asks: without
    /// it, `:nth-child()`, `:first-child` or `:last-child` on each row of a long table would
    /// count all rows again.
    positions: std::sync::OnceLock<Vec<Position>>,
}

/// Where an element stands among the elements of its parent, as `:nth-child()` and
/// `:nth-of-type()` count; [`Position::UNKNOWN`] for nodes outside the tree.
#[derive(Clone, Copy, Debug)]
struct Position {
    /// The sibling elements before it.
    before: u32,
    /// The elements of its parent, itself included.
    siblings: u32,
    /// The sibling elements with its name before it.
    before_of_type: u32,
    /// The elements of its parent with its name, itself included.
    of_type: u32,
}

impl Position {
    const UNKNOWN: Self = Self {
        before: 0,
        siblings: 0,
        before_of_type: 0,
        of_type: 0,
    };

    /// The elements before and after it that count for `ty`.
    fn counts(self, ty: NthType) -> (u32, u32) {
        let (before, all) = match ty {
            NthType::OfType | NthType::LastOfType | NthType::OnlyOfType => {
                (self.before_of_type, self.of_type)
            }
            _ => (self.before, self.siblings),
        };
        (before, all - before - 1)
    }
}

/// The positions of the elements of `document` among their siblings, by node.
fn positions(document: &Document) -> Vec<Position> {
    let mut positions = vec![Position::UNKNOWN; document.len()];
    let mut stack = vec![NodeId::DOCUMENT];
    let mut names: Vec<(&fagus_dom::QualName, u32)> = Vec::new();
    while let Some(parent) = stack.pop() {
        names.clear();
        let mut siblings = 0;
        for child in document.child_elements(parent) {
            let Some(data) = document.element(child) else {
                continue;
            };
            let before_of_type =
                if let Some((_, count)) = names.iter_mut().find(|(name, _)| **name == data.name) {
                    *count += 1;
                    *count - 1
                } else {
                    names.push((&data.name, 1));
                    0
                };
            positions[child.index()] = Position {
                before: siblings,
                siblings: 0,
                before_of_type,
                of_type: 0,
            };
            siblings += 1;
            stack.push(child);
        }
        for child in document.child_elements(parent) {
            let Some(data) = document.element(child) else {
                continue;
            };
            let position = &mut positions[child.index()];
            position.siblings = siblings;
            position.of_type = names
                .iter()
                .find(|(name, _)| **name == data.name)
                .map_or(1, |&(_, count)| count);
        }
    }
    positions
}

impl Ancestors {
    pub fn new(document: &Document) -> Self {
        let mut filters = vec![[u64::MAX; 4]; document.len()];
        let mut stack = vec![(NodeId::DOCUMENT, [0; 4])];
        while let Some((node, filter)) = stack.pop() {
            filters[node.index()] = filter;
            // The ancestors of an element end at a parent that is not one.
            let mut inner = [0; 4];
            if let Some(data) = document.element(node) {
                let features = std::iter::once(feature_bits(Feature::Tag, data.local_name()))
                    .chain(data.id.as_deref().map(|id| feature_bits(Feature::Id, id)))
                    .chain(
                        data.classes
                            .iter()
                            .map(|class| feature_bits(Feature::Class, class)),
                    );
                inner = filter;
                for bits in features {
                    for (word, bit) in inner.iter_mut().zip(bits) {
                        *word |= bit;
                    }
                }
            }
            stack.extend(document.children(node).map(|child| (child, inner)));
        }
        Self {
            filters,
            positions: std::sync::OnceLock::new(),
        }
    }

    /// Where `element` (of `document`, the one these are the ancestors in) stands among its
    /// siblings; `None` outside the tree.
    fn position(&self, document: &Document, element: NodeId) -> Option<Position> {
        self.positions
            .get_or_init(|| positions(document))
            .get(element.index())
            .copied()
            .filter(|position| position.siblings > 0)
    }

    /// Whether the compounds of `rest` (in match order) up to a sibling combinator may match
    /// ancestors of `element`: each of them has to match one, and the names, IDs and classes they
    /// require have to be among those of the ancestors.
    fn may_match(&self, element: NodeId, rest: &[Component]) -> bool {
        let Some(filter) = self.filters.get(element.index()) else {
            return true;
        };
        for component in rest {
            let bits = match component {
                Component::Combinator(Combinator::Child | Combinator::Descendant) => continue,
                Component::Combinator(_) => break,
                Component::LocalName(name) => feature_bits(Feature::Tag, name.name.as_ref()),
                Component::ID(id) => feature_bits(Feature::Id, id.as_ref()),
                Component::Class(class) => feature_bits(Feature::Class, class.as_ref()),
                _ => continue,
            };
            if filter.iter().zip(bits).any(|(word, bit)| word & bit != bit) {
                return false;
            }
        }
        true
    }
}

#[derive(Clone, Copy)]
struct Context<'r> {
    /// The pseudo-element whose style is computed, if any.
    pseudo: Option<PseudoElementKind>,
    /// The selectors `&` refers to.
    nesting: Option<&'r Nesting>,
    /// The element `:scope` refers to.
    scope: Option<NodeId>,
    /// For relative selectors in `:has()` without a leading combinator: the leftmost compound
    /// has to match a descendant of this element.
    within: Option<NodeId>,
}

impl<'a> Matcher<'a> {
    pub fn new(document: &'a Document) -> Self {
        Self {
            document,
            ancestors: None,
        }
    }

    /// A matcher that skips looking for ancestors that `ancestors` (of `document`) rules out.
    pub fn with_ancestors(document: &'a Document, ancestors: &'a Ancestors) -> Self {
        Self {
            document,
            ancestors: Some(ancestors),
        }
    }

    /// Whether `selector` matches `element`, or its pseudo-element `pseudo`.
    pub fn matches(
        &self,
        selector: &Selector,
        element: NodeId,
        pseudo: Option<PseudoElementKind>,
        nesting: Option<&Nesting>,
    ) -> bool {
        let context = Context {
            pseudo,
            nesting,
            scope: None,
            within: None,
        };
        if pseudo.is_none() && selector.has_pseudo_element() {
            return false;
        }
        self.complex(components(selector), element, context)
    }

    /// How many generations up from `element` the nearest root of `scope` lies for which
    /// `selector` matches the element in the scope (CSS Cascade 6, §2.5); `None` where it
    /// matches in no scope. A selector without `:scope` matches below the root alone, as if
    /// it started with `:scope` and a descendant combinator.
    pub fn scoped(
        &self,
        selector: &Selector,
        element: NodeId,
        pseudo: Option<PseudoElementKind>,
        nesting: Option<&Nesting>,
        scope: &Scope,
    ) -> Option<u32> {
        if pseudo.is_none() && selector.has_pseudo_element() {
            return None;
        }
        let document = self.document;
        let owner_parent = scope.owner.and_then(|owner| document.parent_element(owner));
        let mut candidate = Some(element);
        let mut generations = 0;
        while let Some(root) = candidate {
            // The roots of a nested scope match in the scope it is nested in.
            let is_root = match (&scope.start, &scope.parent) {
                (Some(start), Some(parent)) => start
                    .0
                    .iter()
                    .any(|selector| self.scoped(selector, root, None, None, parent).is_some()),
                (Some(start), None) => self.matches_any(&start.0, root),
                (None, _) => owner_parent == Some(root),
            };
            if is_root && !self.limited(scope, root, element) {
                // `&` outside a nested rule is `:scope` too.
                let explicit = contains_scope(selector) || (nesting.is_none() && contains_nesting(selector));
                let context = Context {
                    pseudo,
                    nesting,
                    scope: Some(root),
                    within: (!explicit).then_some(root),
                };
                if self.complex(components(selector), element, context) {
                    return Some(generations);
                }
            }
            candidate = document.parent_element(root);
            generations += 1;
        }
        None
    }

    /// Whether `element` lies at or below a limit of the scope of `root`, below the root.
    fn limited(&self, scope: &Scope, root: NodeId, element: NodeId) -> bool {
        let Some(end) = &scope.end else {
            return false;
        };
        let mut current = Some(element);
        while let Some(node) = current.filter(|&node| node != root) {
            let limit = end.0.iter().any(|selector| {
                let context = Context {
                    pseudo: None,
                    nesting: None,
                    scope: Some(root),
                    within: (!contains_scope(selector)).then_some(root),
                };
                self.complex(components(selector), node, context)
            });
            if limit {
                return true;
            }
            current = self.document.parent_element(node);
        }
        false
    }

    /// Whether any selector of the list matches `element`.
    pub fn matches_any(&self, selectors: &[Selector], element: NodeId) -> bool {
        let context = Context {
            pseudo: None,
            nesting: None,
            scope: None,
            within: None,
        };
        self.any(selectors, element, context)
    }

    fn any(&self, selectors: &[Selector], element: NodeId, context: Context<'_>) -> bool {
        let context = Context {
            pseudo: None,
            ..context
        };
        selectors
            .iter()
            .any(|selector| self.complex(components(selector), element, context))
    }

    fn complex(&self, components: &[Component], element: NodeId, context: Context<'_>) -> bool {
        let end = components
            .iter()
            .position(|component| matches!(component, Component::Combinator(_)))
            .unwrap_or(components.len());
        let (compound, rest) = components.split_at(end);
        if !compound
            .iter()
            .all(|component| self.simple(component, element, context))
        {
            return false;
        }
        let Some((Component::Combinator(combinator), rest)) = rest.split_first() else {
            return context
                .within
                .is_none_or(|within| self.is_inclusive_descendant(element, within, false));
        };
        let document = self.document;
        match combinator {
            Combinator::Child => document
                .parent_element(element)
                .is_some_and(|parent| self.complex(rest, parent, context)),
            Combinator::Descendant => {
                if self
                    .ancestors
                    .is_some_and(|ancestors| !ancestors.may_match(element, rest))
                {
                    return false;
                }
                let mut ancestor = document.parent_element(element);
                while let Some(current) = ancestor {
                    if self.complex(rest, current, context) {
                        return true;
                    }
                    ancestor = document.parent_element(current);
                }
                false
            }
            Combinator::NextSibling => document
                .previous_sibling_element(element)
                .is_some_and(|sibling| self.complex(rest, sibling, context)),
            Combinator::LaterSibling => {
                let mut sibling = document.previous_sibling_element(element);
                while let Some(current) = sibling {
                    if self.complex(rest, current, context) {
                        return true;
                    }
                    sibling = document.previous_sibling_element(current);
                }
                false
            }
            // The pseudo-element was matched in the rightmost compound; the rest describes its
            // originating element, which is `element` itself.
            Combinator::PseudoElement => self.complex(
                rest,
                element,
                Context {
                    pseudo: None,
                    ..context
                },
            ),
            Combinator::SlotAssignment | Combinator::Part | Combinator::DeepDescendant | Combinator::Deep => {
                false
            }
        }
    }

    fn simple(&self, component: &Component, element: NodeId, context: Context<'_>) -> bool {
        let document = self.document;
        let Some(data) = document.element(element) else {
            return false;
        };
        match component {
            Component::Combinator(_) => unreachable!("compounds are split at combinators"),
            Component::ExplicitAnyNamespace | Component::ExplicitUniversalType => true,
            Component::ExplicitNoNamespace => data.namespace().is_empty(),
            Component::DefaultNamespace(url) | Component::Namespace(_, url) => **data.namespace() == **url,
            Component::LocalName(name) => {
                let expected: &str = if data.is_html() {
                    name.lower_name.as_ref()
                } else {
                    name.name.as_ref()
                };
                &**data.local_name() == expected
            }
            Component::ID(id) => data.id.as_deref().is_some_and(|value| {
                if document.quirks_mode() {
                    value.eq_ignore_ascii_case(id.as_ref())
                } else {
                    value == id.as_ref()
                }
            }),
            Component::Class(class) => data.classes.iter().any(|value| {
                if document.quirks_mode() {
                    value.eq_ignore_ascii_case(class.as_ref())
                } else {
                    value == class.as_ref()
                }
            }),
            Component::AttributeInNoNamespaceExists {
                local_name,
                local_name_lower,
            } => {
                let name = if data.is_html() {
                    local_name_lower.as_ref()
                } else {
                    local_name.as_ref()
                };
                data.attribute(name).is_some()
            }
            Component::AttributeInNoNamespace {
                local_name,
                operator,
                value,
                case_sensitivity,
                never_matches,
            } => {
                !never_matches
                    && data.attribute(local_name.as_ref()).is_some_and(|actual| {
                        attribute_matches(
                            actual,
                            *operator,
                            value.as_ref(),
                            *case_sensitivity,
                            data.is_html(),
                        )
                    })
            }
            Component::AttributeOther(selector) => {
                if selector.never_matches {
                    return false;
                }
                let name = if data.is_html() {
                    selector.local_name_lower.as_ref()
                } else {
                    selector.local_name.as_ref()
                };
                data.attributes.iter().any(|attribute| {
                    let namespace_matches = match &selector.namespace {
                        None => attribute.name.ns == ns!(),
                        Some(NamespaceConstraint::Any) => true,
                        Some(NamespaceConstraint::Specific((_, url))) => *attribute.name.ns == **url,
                    };
                    namespace_matches
                        && &*attribute.name.local == name
                        && match &selector.operation {
                            ParsedAttrSelectorOperation::Exists => true,
                            ParsedAttrSelectorOperation::WithValue {
                                operator,
                                case_sensitivity,
                                expected_value,
                            } => attribute_matches(
                                &attribute.value,
                                *operator,
                                expected_value.as_ref(),
                                *case_sensitivity,
                                data.is_html(),
                            ),
                        }
                })
            }
            Component::Negation(selectors) => !self.any(selectors, element, context),
            Component::Is(selectors) | Component::Where(selectors) | Component::Any(_, selectors) => {
                self.any(selectors, element, context)
            }
            Component::Has(selectors) => selectors
                .iter()
                .any(|selector| self.has(selector, element, context)),
            Component::Root => document.document_element() == Some(element),
            Component::Empty => document.children(element).all(|child| {
                document.element(child).is_none() && document.text(child).is_none_or(str::is_empty)
            }),
            Component::Scope => match context.scope {
                Some(scope) => scope == element,
                None => document.document_element() == Some(element),
            },
            Component::Nth(nth) => self.nth(nth, element, None, context),
            Component::NthOf(nth_of) => {
                self.any(nth_of.selectors(), element, context)
                    && self.nth(nth_of.nth_data(), element, Some(nth_of.selectors()), context)
            }
            Component::NonTSPseudoClass(pseudo_class) => self.pseudo_class(pseudo_class, element),
            Component::PseudoElement(pseudo_element) => {
                let kind = PseudoElementKind::of(pseudo_element);
                context.pseudo.is_some_and(|requested| {
                    kind.styles(requested)
                        || (requested == PseudoElementKind::ColumnScrollMarker
                            && matches!(kind, PseudoElementKind::Column | PseudoElementKind::ScrollMarker))
                })
            }
            Component::Nesting => match context.nesting {
                Some(nesting) => {
                    let parent_context = Context {
                        nesting: nesting.parent.as_deref(),
                        pseudo: None,
                        ..context
                    };
                    nesting
                        .selectors
                        .0
                        .iter()
                        .any(|selector| self.complex(components(selector), element, parent_context))
                }
                // Outside a nested rule, `&` is `:scope`: the root of the scope, or of the document.
                None => match context.scope {
                    Some(scope) => scope == element,
                    None => document.document_element() == Some(element),
                },
            },
            Component::Slotted(_) | Component::Part(_) | Component::Host(_) => false,
        }
    }

    /// `:has()` with a relative selector, anchored at `anchor`.
    fn has(&self, selector: &Selector, anchor: NodeId, context: Context<'_>) -> bool {
        let components = components(selector);
        let document = self.document;
        // An explicit leading combinator is stored as `… <combinator> :scope` at the end.
        let leading = match components {
            [.., Component::Combinator(combinator), Component::Scope] => Some(*combinator),
            _ => None,
        };
        let context = Context {
            scope: Some(anchor),
            within: if leading.is_none() { Some(anchor) } else { None },
            pseudo: None,
            ..context
        };
        // After the leading combinator, a descendant or child combinator (`:has(+ tr .x)`,
        // `:has(> li a)`) puts the subject below the sibling or child it reaches.
        let goes_down = components.len() > 2
            && components[..components.len() - 2].iter().any(|component| {
                matches!(
                    component,
                    Component::Combinator(Combinator::Descendant | Combinator::Child)
                )
            });
        // The element, and its descendants where the subject can lie below it.
        let matches_at = |element: NodeId| {
            self.complex(components, element, context)
                || (goes_down
                    && document
                        .descendants(element)
                        .filter(|&node| document.element(node).is_some())
                        .any(|descendant| self.complex(components, descendant, context)))
        };
        match leading {
            Some(Combinator::NextSibling) => document.next_sibling_element(anchor).is_some_and(matches_at),
            Some(Combinator::LaterSibling) => {
                let mut sibling = document.next_sibling_element(anchor);
                while let Some(current) = sibling {
                    if matches_at(current) {
                        return true;
                    }
                    sibling = document.next_sibling_element(current);
                }
                false
            }
            Some(Combinator::Child) => document.child_elements(anchor).any(matches_at),
            _ => document
                .descendants(anchor)
                .filter(|&node| document.element(node).is_some())
                .any(|descendant| self.complex(components, descendant, context)),
        }
    }

    fn nth(
        &self,
        nth: &NthSelectorData,
        element: NodeId,
        of: Option<&[Selector]>,
        context: Context<'_>,
    ) -> bool {
        let document = self.document;
        let Some(data) = document.element(element) else {
            return false;
        };
        let counts = |sibling: NodeId| -> bool {
            match nth.ty {
                NthType::OfType | NthType::LastOfType | NthType::OnlyOfType => document
                    .element(sibling)
                    .is_some_and(|other| other.name == data.name),
                _ => of.is_none_or(|selectors| self.any(selectors, sibling, context)),
            }
        };
        // Counted once for all elements, unless only siblings that match selectors count.
        let is_of_type = matches!(
            nth.ty,
            NthType::OfType | NthType::LastOfType | NthType::OnlyOfType
        );
        let position = self
            .ancestors
            .filter(|_| of.is_none() || is_of_type)
            .and_then(|ancestors| ancestors.position(document, element));
        let (before, after) = if let Some(position) = position {
            let (before, after) = position.counts(nth.ty);
            (
                i32::try_from(before).unwrap_or(i32::MAX),
                i32::try_from(after).unwrap_or(i32::MAX),
            )
        } else {
            // Only the siblings on the side that counts.
            let mut before = 0;
            let mut sibling = document.previous_sibling_element(element);
            if matches!(
                nth.ty,
                NthType::LastChild | NthType::LastOfType | NthType::LastCol
            ) {
                sibling = None;
            }
            while let Some(current) = sibling {
                before += i32::from(counts(current));
                sibling = document.previous_sibling_element(current);
            }
            let mut after = 0;
            let mut sibling = document.next_sibling_element(element);
            if matches!(nth.ty, NthType::Child | NthType::OfType | NthType::Col) {
                sibling = None;
            }
            while let Some(current) = sibling {
                after += i32::from(counts(current));
                sibling = document.next_sibling_element(current);
            }
            (before, after)
        };
        let index = match nth.ty {
            NthType::Child | NthType::OfType | NthType::Col => before + 1,
            NthType::LastChild | NthType::LastOfType | NthType::LastCol => after + 1,
            NthType::OnlyChild | NthType::OnlyOfType => return before == 0 && after == 0,
        };
        let (a, b) = (nth.a, nth.b);
        if a == 0 {
            index == b
        } else {
            let difference = index - b;
            difference % a == 0 && difference / a >= 0
        }
    }

    fn pseudo_class(&self, pseudo_class: &PseudoClass<'_>, element: NodeId) -> bool {
        let document = self.document;
        let Some(data) = document.element(element) else {
            return false;
        };
        let is_form_control =
            data.is("input") || data.is("select") || data.is("textarea") || data.is("button");
        match pseudo_class {
            PseudoClass::Lang { languages } => {
                let language = self.language(element).to_ascii_lowercase();
                languages.iter().any(|wanted| {
                    let wanted = wanted.to_ascii_lowercase();
                    wanted == "*"
                        || language == wanted
                        || language
                            .strip_prefix(wanted.as_str())
                            .is_some_and(|rest| rest.starts_with('-'))
                })
            }
            PseudoClass::Dir { direction } => {
                let is_rtl = self.is_rtl(element);
                matches!(direction, lightningcss::selector::Direction::Rtl) == is_rtl
            }
            PseudoClass::AnyLink(_) | PseudoClass::Link => {
                (data.is("a") || data.is("area") || data.is("link")) && data.attribute("href").is_some()
            }
            PseudoClass::Defined => true,
            PseudoClass::Enabled => is_form_control && data.attribute("disabled").is_none(),
            PseudoClass::Disabled => is_form_control && data.attribute("disabled").is_some(),
            PseudoClass::Checked => {
                (data.is("input") && crate::validity::checked(document, element))
                    || (data.is("option") && data.attribute("selected").is_some())
            }
            PseudoClass::Default => {
                data.attribute("checked").is_some() || data.attribute("selected").is_some()
            }
            PseudoClass::Required => is_form_control && data.attribute("required").is_some(),
            PseudoClass::Optional => is_form_control && data.attribute("required").is_none(),
            PseudoClass::ReadOnly(_) => {
                !(data.is("input") || data.is("textarea"))
                    || data.attribute("readonly").is_some()
                    || data.attribute("disabled").is_some()
            }
            PseudoClass::ReadWrite(_) => {
                (data.is("input") || data.is("textarea"))
                    && data.attribute("readonly").is_none()
                    && data.attribute("disabled").is_none()
            }
            PseudoClass::PlaceholderShown(_) => {
                (data.is("input") || data.is("textarea"))
                    && data.attribute("placeholder").is_some()
                    && data.attribute("value").is_none_or(str::is_empty)
            }
            PseudoClass::Valid => crate::validity::validity(document, element) == Some(true),
            PseudoClass::Invalid => crate::validity::validity(document, element) == Some(false),
            PseudoClass::InRange => crate::validity::in_range(document, element) == Some(true),
            PseudoClass::OutOfRange => crate::validity::in_range(document, element) == Some(false),
            PseudoClass::Indeterminate => crate::validity::indeterminate(document, element),
            PseudoClass::Open => {
                (data.is("details") || data.is("dialog")) && data.attribute("open").is_some()
            }
            PseudoClass::Closed => {
                (data.is("details") || data.is("dialog")) && data.attribute("open").is_none()
            }
            _ => false,
        }
    }

    /// The language of an element: the nearest `lang` attribute.
    fn language(&self, element: NodeId) -> &'a str {
        let mut current = Some(element);
        while let Some(node) = current {
            if let Some(language) = self
                .document
                .element(node)
                .and_then(fagus_dom::ElementData::language)
            {
                return language;
            }
            current = self.document.parent_element(node);
        }
        ""
    }

    fn is_rtl(&self, element: NodeId) -> bool {
        is_rtl(self.document, element)
    }

    fn is_inclusive_descendant(&self, node: NodeId, ancestor: NodeId, inclusive: bool) -> bool {
        if inclusive && node == ancestor {
            return true;
        }
        let mut current = self.document.parent(node);
        while let Some(parent) = current {
            if parent == ancestor {
                return true;
            }
            current = self.document.parent(parent);
        }
        false
    }
}

fn components(selector: &Selector) -> &[Component] {
    selector.iter_raw_match_order().as_slice()
}

fn attribute_matches(
    actual: &str,
    operator: AttrSelectorOperator,
    expected: &str,
    case_sensitivity: ParsedCaseSensitivity,
    is_html: bool,
) -> bool {
    let insensitive = match case_sensitivity {
        ParsedCaseSensitivity::AsciiCaseInsensitive => true,
        ParsedCaseSensitivity::AsciiCaseInsensitiveIfInHtmlElementInHtmlDocument => is_html,
        ParsedCaseSensitivity::CaseSensitive | ParsedCaseSensitivity::ExplicitCaseSensitive => false,
    };
    let (actual, expected) = if insensitive {
        (actual.to_ascii_lowercase(), expected.to_ascii_lowercase())
    } else {
        (actual.to_owned(), expected.to_owned())
    };
    match operator {
        AttrSelectorOperator::Equal => actual == expected,
        AttrSelectorOperator::Includes => {
            !expected.is_empty() && actual.split_ascii_whitespace().any(|word| word == expected)
        }
        AttrSelectorOperator::DashMatch => {
            actual == expected
                || actual
                    .strip_prefix(expected.as_str())
                    .is_some_and(|rest| rest.starts_with('-'))
        }
        AttrSelectorOperator::Prefix => !expected.is_empty() && actual.starts_with(&expected),
        AttrSelectorOperator::Substring => !expected.is_empty() && actual.contains(&expected),
        AttrSelectorOperator::Suffix => !expected.is_empty() && actual.ends_with(&expected),
    }
}

/// Whether the directionality of an element is right to left (HTML, §3.2.6.4): that of its
/// `dir` attribute; for `dir=auto`, and for a `<bdi>` without one, that of the first character
/// of its text with a strong direction (left to right without one); else that of its parent,
/// left to right at the root.
pub fn is_rtl(document: &Document, element: NodeId) -> bool {
    let mut current = Some(element);
    while let Some(node) = current {
        if let Some(data) = document.element(node) {
            match data.attribute("dir").map(str::to_ascii_lowercase).as_deref() {
                Some("rtl") => return true,
                Some("ltr") => return false,
                Some("auto") => return auto_direction(document, node) == Some(true),
                _ if data.is_html() && data.is("bdi") => return auto_direction(document, node) == Some(true),
                _ => {}
            }
        }
        current = document.parent_element(node);
    }
    false
}

/// The direction of the first character with a strong direction in the text of an element,
/// `true` for right to left: leaving out `<bdi>`, `<script>`, `<style>` and `<textarea>`
/// elements and elements with a `dir` attribute of their own, with their content.
fn auto_direction(document: &Document, element: NodeId) -> Option<bool> {
    for child in document.children(element) {
        if let Some(text) = document.text(child) {
            if let Some(rtl) = first_strong(text) {
                return Some(rtl);
            }
            continue;
        }
        let Some(data) = document.element(child) else {
            continue;
        };
        let has_dir = data.attribute("dir").is_some_and(|dir| {
            ["ltr", "rtl", "auto"]
                .iter()
                .any(|valid| dir.eq_ignore_ascii_case(valid))
        });
        let skipped = data.is_html()
            && ["bdi", "script", "style", "textarea"]
                .iter()
                .any(|name| data.is(name));
        if has_dir || skipped {
            continue;
        }
        if let Some(rtl) = auto_direction(document, child) {
            return Some(rtl);
        }
    }
    None
}

/// The direction of the first character of `text` with a strong direction (bidi class L, R
/// or AL), `true` for right to left.
pub fn first_strong(text: &str) -> Option<bool> {
    use unicode_bidi::BidiClass;
    text.chars()
        .find_map(|character| match unicode_bidi::bidi_class(character) {
            BidiClass::L => Some(false),
            BidiClass::R | BidiClass::AL => Some(true),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Origin, Stylesheet};
    use lightningcss::rules::CssRule;

    /// The elements of `html` matched by `selector`, as `tag#id` strings.
    fn select(html: &str, selector: &str) -> Vec<String> {
        let document = Document::parse_html(html);
        let sheet = Stylesheet::parse(&format!("{selector} {{ color: red }}"), Origin::Author, "test");
        assert!(
            sheet.diagnostics().is_empty(),
            "{selector}: {:?}",
            sheet.diagnostics()
        );
        let CssRule::Style(rule) = &sheet.sheet.rules.0[0] else {
            panic!("not a style rule")
        };
        let matcher = Matcher::new(&document);
        document
            .descendants(NodeId::DOCUMENT)
            .filter(|&node| document.element(node).is_some())
            .filter(|&node| {
                rule.selectors
                    .0
                    .iter()
                    .any(|selector| matcher.matches(selector, node, None, None))
            })
            .map(|node| {
                let data = document.element(node).unwrap();
                format!("{}#{}", data.local_name(), data.id.as_deref().unwrap_or(""))
            })
            .collect()
    }

    const HTML: &str = r#"<!doctype html><html lang="de"><body>
        <div id="a" class="card ok"><p id="p1">x</p><p id="p2" class="note">y</p><span id="s1"></span></div>
        <div id="b" class="card"><a id="l" href="/x">link</a></div>
        <ul id="u"><li id="i1"></li><li id="i2" class="x"></li><li id="i3"></li><li id="i4" class="x"></li></ul>
        <input id="c" type="checkbox" checked data-kind="Alpha Beta"></body></html>"#;

    #[test]
    fn matches_simple_and_compound_selectors() {
        assert_eq!(select(HTML, "p"), ["p#p1", "p#p2"]);
        assert_eq!(select(HTML, "#b"), ["div#b"]);
        assert_eq!(select(HTML, "div.card.ok"), ["div#a"]);
        assert_eq!(select(HTML, "*:root"), ["html#"]);
        assert_eq!(select(HTML, "span:empty"), ["span#s1"]);
    }

    #[test]
    fn skips_ancestors_that_none_can_be() {
        let html = r#"<!doctype html><body class="Page"><main id="Top"><ul><li><ul><li><p id="deep">x</p>
            </li></ul></li></ul></main><template><ul><p id="kept">y</p></ul></template>
            <svg><foreignObject><p id="svg">z</p></foreignObject></svg></body>"#;
        let document = Document::parse_html(html);
        let ancestors = Ancestors::new(&document);
        let plain = Matcher::new(&document);
        let filtered = Matcher::with_ancestors(&document, &ancestors);
        for selector in [
            "ul p",
            "ol p",
            "ul ul p",
            "ol ul p",
            "ul > li p",
            ".page p",
            ".Page main p",
            "#top p",
            "#Top p",
            "li + li p",
            "main ~ p",
            "foreignobject p",
            "svg foreignObject > p",
            ":is(ul, ol) p",
            "table p",
        ] {
            let sheet = Stylesheet::parse(&format!("{selector} {{ color: red }}"), Origin::Author, "test");
            let CssRule::Style(rule) = &sheet.sheet.rules.0[0] else {
                panic!("not a style rule")
            };
            for node in document.descendants(NodeId::DOCUMENT) {
                for selector in &rule.selectors.0 {
                    assert_eq!(
                        plain.matches(selector, node, None, None),
                        filtered.matches(selector, node, None, None),
                    );
                }
            }
        }
        // No ancestor of the paragraph is an `ol` or has the class `missing`.
        let deep = document.descendants(NodeId::DOCUMENT).find(|&node| {
            document
                .element(node)
                .is_some_and(|data| data.id.as_deref() == Some("deep"))
        });
        let deep = deep.expect("the paragraph");
        let rule = |css: &str| {
            let sheet = Stylesheet::parse(&format!("{css} {{ color: red }}"), Origin::Author, "test");
            let CssRule::Style(rule) = &sheet.sheet.rules.0[0] else {
                panic!("not a style rule")
            };
            rule.selectors.0[0].clone()
        };
        let components = |selector: &Selector| components(selector).to_vec();
        let ol = components(&rule("ol p"));
        let missing = components(&rule("ul.missing p"));
        assert!(!ancestors.may_match(deep, &ol[2..]));
        assert!(ancestors.may_match(deep, &components(&rule("ul p"))[2..]));
        assert!(!ancestors.may_match(deep, &missing[2..]));
    }

    #[test]
    fn counts_siblings_once_for_structural_pseudo_classes() {
        let html = r#"<!doctype html><body><h1 id="h">t</h1><p id="p1">a</p><div id="d1"></div>
            <p id="p2" class="x">b</p>text<!-- c --><p id="p3">c</p><div id="d2"><p id="only">d</p></div>
            <section><b id="b1"></b><i id="i1"></i><b id="b2" class="x"></b></section>
            </body>"#;
        let document = Document::parse_html(html);
        let ancestors = Ancestors::new(&document);
        let plain = Matcher::new(&document);
        let counted = Matcher::with_ancestors(&document, &ancestors);
        let mut checked = 0;
        for selector in [
            ":first-child",
            ":last-child",
            ":only-child",
            ":nth-child(2n+1)",
            ":nth-child(-n+2)",
            ":nth-last-child(2)",
            ":first-of-type",
            ":last-of-type",
            ":only-of-type",
            ":nth-of-type(2)",
            ":nth-last-of-type(odd)",
            ":nth-child(2 of .x)",
            ":nth-last-child(1 of p)",
            "p:nth-child(even) + *",
        ] {
            let sheet = Stylesheet::parse(&format!("{selector} {{ color: red }}"), Origin::Author, "test");
            let CssRule::Style(rule) = &sheet.sheet.rules.0[0] else {
                panic!("not a style rule")
            };
            for node in document.descendants(NodeId::DOCUMENT) {
                for selector in &rule.selectors.0 {
                    let matches = plain.matches(selector, node, None, None);
                    assert_eq!(matches, counted.matches(selector, node, None, None));
                    checked += usize::from(matches);
                }
            }
        }
        assert!(checked > 20);
        let id = |id: &str| {
            document
                .descendants(NodeId::DOCUMENT)
                .find(|&node| {
                    document
                        .element(node)
                        .is_some_and(|data| data.id.as_deref() == Some(id))
                })
                .expect("the element")
        };
        let p2 = ancestors.position(&document, id("p2")).expect("in the tree");
        assert_eq!(
            (p2.before, p2.siblings, p2.before_of_type, p2.of_type),
            (3, 7, 1, 3)
        );
        assert_eq!(select(html, "section > :nth-of-type(2)"), ["b#b2"]);
        assert_eq!(select(html, "p:nth-last-of-type(1)"), ["p#p3", "p#only"]);
    }

    #[test]
    fn matches_combinators() {
        assert_eq!(select(HTML, "div > p"), ["p#p1", "p#p2"]);
        assert_eq!(select(HTML, "body p.note"), ["p#p2"]);
        assert_eq!(select(HTML, "p + p"), ["p#p2"]);
        assert_eq!(select(HTML, "#p1 ~ span"), ["span#s1"]);
    }

    #[test]
    fn matches_logical_pseudo_classes() {
        assert_eq!(
            select(HTML, ":is(#a, #b) > :where(p, a)"),
            ["p#p1", "p#p2", "a#l"]
        );
        assert_eq!(select(HTML, "div:not(.ok)"), ["div#b"]);
        assert_eq!(select(HTML, "div:has(> a)"), ["div#b"]);
        assert_eq!(select(HTML, "div:has(.note)"), ["div#a"]);
        assert_eq!(select(HTML, "p:has(+ span)"), ["p#p2"]);
        assert_eq!(select(HTML, "body:has(div p)"), ["body#"]);
    }

    #[test]
    fn matches_has_whose_subject_lies_below_a_sibling_or_child() {
        // `tr:has(+ tr .blocked)`: the row before one that holds a blocked badge.
        let rows = r#"<table><tr id="r1"><td>a</td></tr><tr id="r2"><td><b class="blocked">x</b></td></tr>
            <tr id="r3"><td>c</td></tr><tr id="r4"><td><b>d</b></td></tr></table>"#;
        assert_eq!(select(rows, "tr:has(+ tr .blocked)"), ["tr#r1"]);
        assert_eq!(select(rows, "tr:has(~ tr > td > .blocked)"), ["tr#r1"]);
        assert_eq!(select(rows, "tr:has(+ .blocked)"), Vec::<String>::new());
        assert_eq!(select(HTML, "body:has(> div p.note)"), ["body#"]);
        assert_eq!(select(HTML, "body:has(> div > a)"), ["body#"]);
        assert_eq!(select(HTML, "body:has(> ul > p)"), Vec::<String>::new());
    }

    #[test]
    fn matches_structural_pseudo_classes() {
        assert_eq!(select(HTML, "li:nth-child(2n)"), ["li#i2", "li#i4"]);
        assert_eq!(select(HTML, "li:first-child"), ["li#i1"]);
        assert_eq!(select(HTML, "li:last-child"), ["li#i4"]);
        assert_eq!(select(HTML, "li:nth-last-child(-n + 2)"), ["li#i3", "li#i4"]);
        assert_eq!(select(HTML, "li:nth-child(2 of .x)"), ["li#i4"]);
        assert_eq!(select(HTML, "p:first-of-type"), ["p#p1"]);
    }

    #[test]
    fn matches_directionality() {
        const DIRS: &str = r"<div id=a dir=RTL><p id=b>x</p><p id=c dir=auto>1 <i dir=rtl>א</i>
            <bdi id=d>abc</bdi> b</p><p id=e dir=auto><bdi>x</bdi> שלום</p><bdi id=f>שלום</bdi></div>";
        assert_eq!(select(DIRS, ":dir(rtl)"), ["div#a", "p#b", "i#", "p#e", "bdi#f"]);
        assert_eq!(select(DIRS, "p:dir(ltr)"), ["p#c"]);
        assert_eq!(select(DIRS, "bdi:dir(ltr)"), ["bdi#d", "bdi#"]);
    }

    #[test]
    fn matches_attributes_and_states() {
        assert_eq!(select(HTML, "[data-kind~=beta i]"), ["input#c"]);
        assert_eq!(select(HTML, "[data-kind^=Alpha]"), ["input#c"]);
        assert_eq!(select(HTML, "a[href$='/x']"), ["a#l"]);
        assert_eq!(select(HTML, ":checked"), ["input#c"]);
        assert_eq!(select(HTML, "a:link"), ["a#l"]);
        assert_eq!(select(HTML, "a:hover"), [] as [String; 0]);
        assert_eq!(select(HTML, "div:lang(de)").len(), 2);
    }
}

//! Custom properties: `var()` substitution (CSS Variables 1) and registered properties
//! (`@property`, CSS Properties and Values API 1).

use lightningcss::properties::custom::{
    CustomProperty, CustomPropertyName, Function, Token, TokenList, TokenOrValue, UnresolvedColor,
};
use lightningcss::properties::{Property, PropertyId};
use lightningcss::rules::property::PropertyRule;
use lightningcss::stylesheet::{ParserOptions, PrinterOptions};
use lightningcss::traits::{IntoOwned, ToCss};
use lightningcss::values::ident::DashedIdent;
use rustc_hash::FxHashMap;

use crate::functions::Functions;

/// How deep `var()` references may nest; deeper chains are treated as cycles.
const MAX_DEPTH: u8 = 32;

/// How many tokens one substitution may produce. References that repeat other references grow
/// exponentially; browsers cap them in the same way.
const MAX_TOKENS: usize = 1 << 16;

/// A custom property registered with `@property`.
#[derive(Debug)]
pub struct Registered {
    pub inherits: bool,
    pub initial: Option<TokenList<'static>>,
    /// The syntax of its values, unless it takes any (`*`).
    pub syntax: Option<lightningcss::values::syntax::SyntaxString>,
}

/// The registered custom properties of a document.
#[derive(Debug, Default)]
pub struct Registry {
    properties: FxHashMap<String, Registered>,
}

impl Registry {
    /// Collects `@property` rules; a later rule for the same name wins.
    pub fn new(rules: &[PropertyRule<'static>]) -> Self {
        let mut properties = FxHashMap::default();
        for rule in rules {
            let name = rule.name.0.to_string();
            let initial = rule.initial_value.as_ref().and_then(|value| {
                let css = value.to_css_string(PrinterOptions::default()).ok()?;
                tokens(&name, &css)
            });
            properties.insert(
                name,
                Registered {
                    inherits: rule.inherits,
                    initial,
                    syntax: match &rule.syntax {
                        lightningcss::values::syntax::SyntaxString::Universal => None,
                        syntax @ lightningcss::values::syntax::SyntaxString::Components(_) => {
                            Some(syntax.clone())
                        }
                    },
                },
            );
        }
        Self { properties }
    }

    pub fn is_empty(&self) -> bool {
        self.properties.is_empty()
    }

    /// Names of registered properties that are not inherited.
    pub fn non_inherited(&self) -> impl Iterator<Item = &str> {
        self.properties
            .iter()
            .filter(|(_, registered)| !registered.inherits)
            .map(|(name, _)| name.as_str())
    }

    /// Whether a custom property inherits: unless it is registered as one that does not.
    pub fn inherits(&self, name: &str) -> bool {
        self.properties
            .get(name)
            .is_none_or(|registered| registered.inherits)
    }

    /// Whether a property is registered as a single color (`syntax: "<color>"`): its computed
    /// value is an absolute color.
    pub fn is_color(&self, name: &str) -> bool {
        use lightningcss::values::syntax::{Multiplier, SyntaxComponentKind, SyntaxString};
        matches!(
            self.properties.get(name).and_then(|registered| registered.syntax.as_ref()),
            Some(SyntaxString::Components(components))
                if matches!(components.as_slice(), [component]
                    if component.kind == SyntaxComponentKind::Color
                        && component.multiplier == Multiplier::None)
        )
    }

    /// Whether a property is registered as a single number, percentage, length, angle or time,
    /// whose values interpolate in animations.
    pub fn is_numeric(&self, name: &str) -> bool {
        use lightningcss::values::syntax::{Multiplier, SyntaxComponentKind, SyntaxString};
        matches!(
            self.properties.get(name).and_then(|registered| registered.syntax.as_ref()),
            Some(SyntaxString::Components(components))
                if matches!(components.as_slice(), [component]
                    if component.multiplier == Multiplier::None
                        && matches!(
                            component.kind,
                            SyntaxComponentKind::Number
                                | SyntaxComponentKind::Percentage
                                | SyntaxComponentKind::Length
                                | SyntaxComponentKind::LengthPercentage
                                | SyntaxComponentKind::Angle
                                | SyntaxComponentKind::Time
                        ))
        )
    }

    /// Whether a property is registered with a syntax that its values must have.
    pub fn has_syntax(&self, name: &str) -> bool {
        self.properties
            .get(name)
            .is_some_and(|registered| registered.syntax.is_some())
    }

    /// Whether a value (with its references substituted) is of the syntax its property is
    /// registered with, if any.
    pub fn accepts(&self, name: &str, value: &TokenList<'static>) -> bool {
        match self
            .properties
            .get(name)
            .and_then(|registered| registered.syntax.as_ref())
        {
            Some(syntax) => to_css(value).is_some_and(|css| fagus_css::matches_syntax(syntax, &css)),
            None => true,
        }
    }

    /// The initial value of a registered property.
    pub fn initial(&self, name: &str) -> Option<&TokenList<'static>> {
        self.properties
            .get(name)
            .and_then(|registered| registered.initial.as_ref())
    }
}

/// Parses `css` as the value of the custom property `name`.
fn tokens(name: &str, css: &str) -> Option<TokenList<'static>> {
    let css = &*fagus_css::flat_math(css);
    if fagus_css::nests_too_deep(css) {
        return None;
    }
    let id = PropertyId::from(name).into_owned();
    match Property::parse_string(id, css, ParserOptions::default()).ok()? {
        Property::Custom(custom) => Some(custom.value.into_owned()),
        _ => None,
    }
}

/// The tokens of a value (of an attribute, for `attr()`).
pub(crate) fn value_tokens(css: &str) -> Option<TokenList<'static>> {
    tokens("--fagus-value", css)
}

/// Whether a token list contains `var()`, `env()` or another function resolved at computed-value
/// time ([`fagus_css::is_computed_function`]).
pub fn has_variables(tokens: &TokenList<'_>) -> bool {
    tokens.0.iter().any(|token| match token {
        TokenOrValue::Var(_) | TokenOrValue::Env(_) => true,
        TokenOrValue::Function(function) => {
            fagus_css::is_computed_function(function) || has_variables(&function.arguments)
        }
        TokenOrValue::UnresolvedColor(color) => match color {
            UnresolvedColor::RGB { alpha, .. } | UnresolvedColor::HSL { alpha, .. } => has_variables(alpha),
            UnresolvedColor::LightDark { light, dark } => has_variables(light) || has_variables(dark),
        },
        _ => false,
    })
}

/// Replaces the `var()` and `env()` references in `tokens`, looking up custom properties with
/// `lookup`. Returns `None` if a reference has neither a value nor a fallback, or the references
/// form a cycle: the declaration is then invalid at computed-value time.
pub fn substitute<'v>(
    tokens: &TokenList<'static>,
    lookup: &impl Fn(&str) -> Option<&'v TokenList<'static>>,
) -> Option<TokenList<'static>> {
    substitute_with(tokens, lookup, &Functions::default())
}

/// Replaces the `var()` and `env()` references in `tokens` as [`substitute`] does, and the other
/// functions resolved at computed-value time with what `functions` tells of the element.
pub fn substitute_with<'v>(
    tokens: &TokenList<'static>,
    lookup: &impl Fn(&str) -> Option<&'v TokenList<'static>>,
    functions: &Functions<'_>,
) -> Option<TokenList<'static>> {
    let mut budget = MAX_TOKENS;
    substitute_at(tokens, lookup, functions, 0, &mut budget)
}

/// Resolves a function of [`fagus_css::is_computed_function`] into `result`; `None` where it
/// makes the declaration invalid at computed-value time.
fn resolve_function<'v>(
    function: &Function<'static>,
    lookup: &impl Fn(&str) -> Option<&'v TokenList<'static>>,
    functions: &Functions<'_>,
    depth: u8,
    budget: &mut usize,
    result: &mut Vec<TokenOrValue<'static>>,
) -> Option<()> {
    let name = function.name.0.to_ascii_lowercase();
    match name.as_str() {
        // Substitution functions, which custom properties resolve too.
        "if" => {
            let test =
                |test: crate::functions::Test<'_>| crate::functions::test(test, lookup, functions.media);
            let value = crate::functions::choose(&function.arguments, &test)?;
            result.extend(substitute_at(&TokenList(value.to_vec()), lookup, functions, depth + 1, budget)?.0);
        }
        "attr" => match crate::functions::attr(&function.arguments, functions.element) {
            crate::functions::Attr::Value(value) => result.extend(value.0),
            crate::functions::Attr::Fallback(fallback) => {
                result.extend(
                    substitute_at(
                        &TokenList(fallback.to_vec()),
                        lookup,
                        functions,
                        depth + 1,
                        budget,
                    )?
                    .0,
                );
            }
            crate::functions::Attr::Invalid => return None,
        },
        _ => {
            let arguments = substitute_at(&function.arguments, lookup, functions, depth, budget)?;
            if !functions.math {
                result.push(TokenOrValue::Function(Function {
                    name: function.name.clone(),
                    arguments,
                }));
                return Some(());
            }
            match name.as_str() {
                "sibling-index" | "sibling-count" => {
                    let (document, node) = functions.element?;
                    let (index, count) = crate::functions::sibling(document, node);
                    let value = if name == "sibling-index" { index } else { count };
                    result.push(TokenOrValue::Token(Token::Number {
                        has_sign: false,
                        #[expect(clippy::cast_precision_loss, reason = "sibling counts are small")]
                        value: value as f32,
                        int_value: i32::try_from(value).ok(),
                    }));
                }
                "progress" => result.push(crate::functions::progress(&arguments, functions.lengths)?),
                "contrast-color" => result.push(crate::functions::contrast_color(&arguments)?),
                "image" => result.push(crate::functions::image(&arguments)?),
                _ if fagus_css::is_unparsed_color(function) => {
                    let function = Function {
                        name: function.name.clone(),
                        arguments,
                    };
                    let color = crate::color_functions::resolve(&function, functions.lengths)?;
                    result.push(TokenOrValue::Color(color));
                }
                _ if fagus_css::divides_by_type(function) => {
                    let function = Function {
                        name: function.name.clone(),
                        arguments,
                    };
                    result.push(crate::math::evaluate_function(&function, functions.lengths)?.token()?);
                }
                _ => result.push(TokenOrValue::Function(Function {
                    name: function.name.clone(),
                    arguments: crate::functions::round_arguments(&arguments),
                })),
            }
        }
    }
    Some(())
}

fn substitute_at<'v>(
    tokens: &TokenList<'static>,
    lookup: &impl Fn(&str) -> Option<&'v TokenList<'static>>,
    functions: &Functions<'_>,
    depth: u8,
    budget: &mut usize,
) -> Option<TokenList<'static>> {
    if depth > MAX_DEPTH {
        return None;
    }
    let mut result = Vec::with_capacity(tokens.0.len());
    for token in &tokens.0 {
        *budget = budget.checked_sub(1)?;
        match token {
            TokenOrValue::Var(variable) => {
                let value = match lookup(variable.name.ident.0.as_ref()) {
                    Some(value) => substitute_at(value, lookup, functions, depth + 1, budget),
                    None => None,
                };
                let value = match (value, &variable.fallback) {
                    (Some(value), _) => value,
                    (None, Some(fallback)) => substitute_at(fallback, lookup, functions, depth + 1, budget)?,
                    (None, None) => return None,
                };
                *budget = budget.checked_sub(value.0.len())?;
                result.extend(value.0);
            }
            // The variables Chrome defines, or the fallback.
            TokenOrValue::Env(environment) => {
                if let Some(value) = crate::functions::environment(environment) {
                    result.push(value);
                } else {
                    let fallback = environment.fallback.as_ref()?;
                    result.extend(substitute_at(fallback, lookup, functions, depth + 1, budget)?.0);
                }
            }
            TokenOrValue::Function(function) if fagus_css::is_computed_function(function) => {
                resolve_function(function, lookup, functions, depth, budget, &mut result)?;
            }
            TokenOrValue::Function(function) if has_variables(&function.arguments) => {
                let function = Function {
                    name: function.name.clone(),
                    arguments: substitute_at(&function.arguments, lookup, functions, depth, budget)?,
                };
                // What the substituted values make a function Fagus computes.
                if functions.math && fagus_css::is_computed_function(&function) {
                    resolve_function(&function, lookup, functions, depth, budget, &mut result)?;
                } else {
                    result.push(TokenOrValue::Function(function));
                }
            }
            // `light-dark()` of images: the element's color scheme picks one.
            TokenOrValue::UnresolvedColor(UnresolvedColor::LightDark { light, dark })
                if functions.math && crate::functions::is_image_pair(light, dark) =>
            {
                let chosen = if crate::compute::is_dark() { dark } else { light };
                result.extend(substitute_at(chosen, lookup, functions, depth + 1, budget)?.0);
            }
            TokenOrValue::UnresolvedColor(color) if has_variables(&TokenList(vec![token.clone()])) => {
                *budget = budget.checked_sub(1)?;
                let color = match color {
                    UnresolvedColor::RGB { r, g, b, alpha } => UnresolvedColor::RGB {
                        r: *r,
                        g: *g,
                        b: *b,
                        alpha: substitute_at(alpha, lookup, functions, depth, budget)?,
                    },
                    UnresolvedColor::HSL { h, s, l, alpha } => UnresolvedColor::HSL {
                        h: *h,
                        s: *s,
                        l: *l,
                        alpha: substitute_at(alpha, lookup, functions, depth, budget)?,
                    },
                    // Kept as a function: the element that uses the color picks one by its
                    // color scheme.
                    UnresolvedColor::LightDark { light, dark } => {
                        let mut arguments = substitute_at(light, lookup, functions, depth, budget)?.0;
                        arguments.push(TokenOrValue::Token(Token::Comma));
                        arguments.extend(substitute_at(dark, lookup, functions, depth, budget)?.0);
                        result.push(TokenOrValue::Function(Function {
                            name: lightningcss::values::ident::Ident("light-dark".into()),
                            arguments: TokenList(arguments),
                        }));
                        continue;
                    }
                };
                result.push(TokenOrValue::UnresolvedColor(color));
            }
            other => result.push(other.clone()),
        }
    }
    Some(TokenList(result))
}

/// Parses a property from substituted tokens.
pub fn parse_property(id: &PropertyId<'static>, tokens: &TokenList<'static>) -> Option<Property<'static>> {
    // A negative value where none is allowed makes the declaration invalid.
    if fagus_css::has_forbidden_negative(id, tokens) {
        return None;
    }
    let css = to_css(tokens)
        .map(|css| match fagus_css::flat_math(&css) {
            std::borrow::Cow::Borrowed(_) => css,
            std::borrow::Cow::Owned(flat) => flat,
        })
        .filter(|css| !fagus_css::nests_too_deep(css))?;
    if let PropertyId::Custom(CustomPropertyName::Unknown(_)) = id {
        // Properties lightningcss does not know stay token lists; Fagus parses them itself.
        let Property::Custom(mut custom) = Property::parse_string(
            PropertyId::from("--fagus").into_owned(),
            &css,
            ParserOptions::default(),
        )
        .ok()?
        .into_owned() else {
            return None;
        };
        custom.name = match id {
            PropertyId::Custom(name) => name.clone(),
            _ => return None,
        };
        return Some(Property::Custom(custom));
    }
    // Values from variables reach every corner of the parser, including those where it panics.
    fagus_css::guarded(|| {
        Property::parse_string(id.clone(), &css, ParserOptions::default())
            .ok()
            .map(|property| {
                let mut property = property.into_owned();
                fagus_css::repair_property(&mut property);
                property
            })
    })
    .flatten()
}

pub(crate) fn to_css(tokens: &TokenList<'static>) -> Option<String> {
    // Token lists only serialize as the value of a property.
    Property::Custom(CustomProperty {
        name: CustomPropertyName::Custom(DashedIdent("--fagus".into())),
        value: tokens.clone(),
    })
    .value_to_css_string(PrinterOptions::default())
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(css: &str) -> TokenList<'static> {
        tokens("--test", css).expect("tokens")
    }

    #[test]
    fn substitutes_references_and_fallbacks() {
        let variables: FxHashMap<&str, TokenList<'static>> =
            [("--a", list("10px")), ("--b", list("var(--a) solid"))]
                .into_iter()
                .collect();
        let lookup = |name: &str| variables.get(name);
        let substituted = substitute(&list("var(--b) var(--missing, red)"), &lookup).expect("valid");
        assert_eq!(to_css(&substituted).as_deref(), Some("10px solid red"));
        assert!(substitute(&list("var(--missing)"), &lookup).is_none());
    }

    #[test]
    fn stops_exponential_growth() {
        let mut variables: FxHashMap<String, TokenList<'static>> = FxHashMap::default();
        variables.insert("--v0".to_owned(), list("x"));
        for level in 1..40 {
            let previous = level - 1;
            variables.insert(
                format!("--v{level}"),
                list(&format!("var(--v{previous}) var(--v{previous})")),
            );
        }
        let lookup = |name: &str| variables.get(name);
        assert!(substitute(&list("var(--v39)"), &lookup).is_none());
        assert!(substitute(&list("var(--v8)"), &lookup).is_some());
    }

    #[test]
    fn detects_cycles() {
        let variables: FxHashMap<&str, TokenList<'static>> =
            [("--a", list("var(--b)")), ("--b", list("var(--a)"))]
                .into_iter()
                .collect();
        let lookup = |name: &str| variables.get(name);
        assert!(substitute(&list("var(--a)"), &lookup).is_none());
        assert_eq!(
            substitute(&list("var(--a, 1px)"), &lookup)
                .as_ref()
                .and_then(to_css)
                .as_deref(),
            Some("1px")
        );
    }
}

use std::sync::{Arc, RwLock};

use lightningcss::declaration::DeclarationBlock;
use lightningcss::properties::Property;
use lightningcss::properties::custom::{CustomPropertyName, TokenList, TokenOrValue};
use lightningcss::rules::{CssRule, CssRuleList, Location};
use lightningcss::stylesheet::{ParserOptions, StyleSheet};
use lightningcss::traits::IntoOwned;

use crate::properties::PropertySupport;
use crate::{Diagnostic, Severity};

/// Where a stylesheet comes from. The cascade ranks origins differently.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Origin {
    /// The built-in stylesheet of Fagus.
    UserAgent,
    /// Stylesheets of the document.
    Author,
}

/// A parsed stylesheet. Parsing is independent of the document, so a stylesheet can be parsed once
/// and reused for many documents.
#[derive(Debug)]
pub struct Stylesheet {
    pub(crate) sheet: StyleSheet<'static>,
    pub(crate) origin: Origin,
    pub(crate) source: String,
    /// The `<style>` element the stylesheet comes from.
    pub(crate) owner: Option<fagus_dom::NodeId>,
    /// The address the stylesheet was loaded from, which its relative URLs refer to.
    base: Option<String>,
    diagnostics: Vec<Diagnostic>,
}

impl Stylesheet {
    /// Parses a stylesheet. Invalid rules and declarations are skipped like in browsers and
    /// reported as diagnostics.
    pub fn parse(css: &str, origin: Origin, source: &str) -> Self {
        let css = &*crate::flat_math(css);
        let css = &*crate::encode_nth_pages(css);
        if crate::nests_too_deep(css) {
            return Self {
                sheet: empty_sheet(source),
                origin,
                source: source.to_owned(),
                owner: None,
                base: None,
                diagnostics: vec![Diagnostic {
                    severity: Severity::Error,
                    code: "css.too-deep",
                    message: format!(
                        "blocks or functions nest more than {} levels deep; the stylesheet is ignored",
                        crate::MAX_NESTING
                    ),
                    source: source.to_owned(),
                    line: 0,
                    column: 0,
                }],
            };
        }
        let (sheet, mut diagnostics) = parse_sheet(css, source).unwrap_or_else(|| {
            // The parser panicked: parse again without the rules and declarations it panics on.
            let parses = |css: &str| {
                crate::guard::guarded(|| {
                    StyleSheet::parse(
                        css,
                        ParserOptions {
                            error_recovery: true,
                            ..ParserOptions::default()
                        },
                    )
                    .is_ok()
                })
                .is_some()
            };
            let css = crate::guard::without_panics(css, &parses);
            let (sheet, mut diagnostics) =
                parse_sheet(&css, source).unwrap_or_else(|| (empty_sheet(source), Vec::new()));
            diagnostics.push(Diagnostic {
                severity: Severity::Warning,
                code: "css.unparsable",
                message: "rules or declarations that the CSS parser cannot handle are left out".to_owned(),
                source: source.to_owned(),
                line: 0,
                column: 0,
            });
            (sheet, diagnostics)
        });
        let mut sheet = sheet;
        drop_invalid_declarations(&mut sheet.rules, source, &mut diagnostics);
        diagnostics.sort_by_key(|diagnostic| (diagnostic.line, diagnostic.column));
        Self {
            sheet,
            origin,
            source: source.to_owned(),
            owner: None,
            base: None,
            diagnostics,
        }
    }

    /// Parses a stylesheet loaded from `url`: its relative URLs refer to that address, as in
    /// browsers, and are resolved against it.
    pub fn parse_from(css: &str, origin: Origin, url: &str) -> Self {
        let mut sheet = Self::parse(css, origin, url);
        rebase(&mut sheet.sheet, url);
        sheet.base = Some(url.to_owned());
        sheet
    }

    /// Replaces the `@import` rules at the start of the stylesheet by the stylesheets they
    /// import, which `load` loads by their URL, inside the conditions and the layer the rules
    /// name. Imported stylesheets import in turn, 8 levels deep and each address once.
    #[must_use]
    pub fn with_imports(mut self, load: &mut dyn FnMut(&str) -> Option<String>) -> Self {
        let mut seen = vec![self.source.clone()];
        self.import(load, &mut seen, 0);
        self
    }

    fn import(&mut self, load: &mut dyn FnMut(&str) -> Option<String>, seen: &mut Vec<String>, depth: usize) {
        use lightningcss::rules::layer::LayerBlockRule;
        use lightningcss::rules::media::MediaRule;
        use lightningcss::rules::supports::SupportsRule;
        let rules = std::mem::take(&mut self.sheet.rules.0);
        let mut kept = Vec::with_capacity(rules.len());
        for rule in rules {
            let CssRule::Import(import) = rule else {
                kept.push(rule);
                continue;
            };
            let url = match &self.base {
                Some(base) => crate::join_url(base, &import.url),
                None => import.url.to_string(),
            };
            if depth >= 8 || seen.contains(&url) {
                continue;
            }
            seen.push(url.clone());
            let Some(css) = load(&url) else {
                self.diagnostics.push(Diagnostic {
                    severity: Severity::Warning,
                    code: "resource.not-loaded",
                    message: format!("stylesheet `{url}` could not be imported"),
                    source: self.source.clone(),
                    line: import.loc.line + 1,
                    column: import.loc.column,
                });
                continue;
            };
            let mut imported = Self::parse_from(&css, self.origin, &url);
            imported.import(load, seen, depth + 1);
            self.diagnostics.append(&mut imported.diagnostics);
            let loc = import.loc;
            // Inside out: the layer, the supports condition, the media queries.
            let mut inner = imported.sheet.rules;
            if let Some(name) = import.layer {
                inner = CssRuleList(vec![CssRule::LayerBlock(LayerBlockRule {
                    name,
                    rules: inner,
                    loc,
                })]);
            }
            if let Some(condition) = import.supports {
                inner = CssRuleList(vec![CssRule::Supports(SupportsRule {
                    condition,
                    rules: inner,
                    loc,
                })]);
            }
            if !import.media.media_queries.is_empty() {
                inner = CssRuleList(vec![CssRule::Media(MediaRule {
                    query: import.media,
                    rules: inner,
                    loc,
                })]);
            }
            kept.extend(inner.0);
        }
        self.sheet.rules.0 = kept;
    }

    /// The stylesheet of the `<style>` or `<link>` element `owner`, whose parent is the root of its
    /// `@scope` rules without roots of their own.
    #[must_use]
    pub fn owned_by(mut self, owner: fagus_dom::NodeId) -> Self {
        self.owner = Some(owner);
        self
    }

    /// Whether the stylesheet starts with `@import` rules, which [`Self::with_imports`] replaces.
    pub fn has_imports(&self) -> bool {
        self.sheet
            .rules
            .0
            .iter()
            .any(|rule| matches!(rule, CssRule::Import(_)))
    }

    pub fn origin(&self) -> Origin {
        self.origin
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    /// Problems found while parsing.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Number of top-level rules.
    pub fn len(&self) -> usize {
        self.sheet.rules.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sheet.rules.0.is_empty()
    }
}

/// Resolves the relative URLs of a stylesheet loaded from `base` against it.
fn rebase(sheet: &mut StyleSheet<'static>, base: &str) {
    use lightningcss::values::url::Url;
    use lightningcss::visit_types;
    use lightningcss::visitor::{Visit, VisitTypes, Visitor};
    struct Rebase<'a>(&'a str);
    impl<'i> Visitor<'i> for Rebase<'_> {
        type Error = std::convert::Infallible;

        fn visit_types(&self) -> VisitTypes {
            visit_types!(URLS)
        }

        fn visit_url(&mut self, url: &mut Url<'i>) -> Result<(), Self::Error> {
            url.url = crate::join_url(self.0, &url.url).into();
            Ok(())
        }
    }
    let _ = sheet.visit(&mut Rebase(base));
}

/// lightningcss keeps declarations it cannot parse (as unparsed or unknown properties) so it can
/// print them again. A renderer has to drop them, like browsers do: a declaration with an invalid
/// value or an unknown property is ignored. Unparsed values that contain `var()` stay, because they
/// can only be checked after substitution.
fn drop_invalid_declarations(
    rules: &mut CssRuleList<'static>,
    source: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for rule in &mut rules.0 {
        match rule {
            CssRule::Style(style) => {
                check_block(&mut style.declarations, style.loc, source, diagnostics);
                drop_invalid_declarations(&mut style.rules, source, diagnostics);
            }
            CssRule::Nesting(nesting) => {
                check_block(
                    &mut nesting.style.declarations,
                    nesting.style.loc,
                    source,
                    diagnostics,
                );
                drop_invalid_declarations(&mut nesting.style.rules, source, diagnostics);
            }
            CssRule::NestedDeclarations(nested) => {
                check_block(&mut nested.declarations, nested.loc, source, diagnostics);
            }
            CssRule::Media(media) => drop_invalid_declarations(&mut media.rules, source, diagnostics),
            CssRule::Supports(supports) => {
                drop_invalid_declarations(&mut supports.rules, source, diagnostics);
            }
            CssRule::LayerBlock(layer) => drop_invalid_declarations(&mut layer.rules, source, diagnostics),
            CssRule::Page(page) => {
                check_page_size(&mut page.declarations, page.loc, source, diagnostics);
                check_block(&mut page.declarations, page.loc, source, diagnostics);
                for margin in &mut page.rules {
                    check_block(&mut margin.declarations, margin.loc, source, diagnostics);
                }
            }
            _ => {}
        }
    }
}

/// Drops the declarations of `size` in an `@page` rule that Chrome does not take (a value with
/// `var()` is checked once it is substituted), as Chrome drops them, and reports what is wrong
/// with each: the page keeps the size that applied before. So it does with the declarations of
/// `-fagus-page-height` that are not `auto` or `fit-content`.
fn check_page_size(
    block: &mut DeclarationBlock<'static>,
    location: Location,
    source: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for declarations in [&mut block.declarations, &mut block.important_declarations] {
        declarations.retain(|property| {
            let Property::Custom(custom) = property else {
                return true;
            };
            let CustomPropertyName::Unknown(name) = &custom.name else {
                return true;
            };
            if has_variables(&custom.value) || is_wide_keyword(&custom.value) {
                return true;
            }
            let message = if name.0.eq_ignore_ascii_case("size") {
                let Some(problem) = crate::page_size::page_size_problem(&custom.value) else {
                    return true;
                };
                format!(
                    "invalid value for `size`: {problem}; the declaration is ignored and the page keeps the size that applied before"
                )
            } else if name.0.eq_ignore_ascii_case("-fagus-page-height") {
                if crate::page_size::page_height_fits(&custom.value.0).is_some() {
                    return true;
                }
                "invalid value for `-fagus-page-height`: it takes `auto` or `fit-content`; the declaration is ignored"
                    .to_owned()
            } else {
                return true;
            };
            diagnostics.push(Diagnostic {
                severity: Severity::Warning,
                code: "css.invalid-value",
                message,
                source: source.to_owned(),
                line: location.line + 1,
                column: location.column,
            });
            false
        });
    }
}

/// A background with a gradient that lightningcss rejects but browsers accept; Fagus repairs it
/// (see [`crate::gradient`]).
fn is_interpolated_background(unparsed: &lightningcss::properties::custom::UnparsedProperty<'_>) -> bool {
    use lightningcss::properties::PropertyId;
    matches!(
        unparsed.property_id,
        PropertyId::Background | PropertyId::BackgroundImage
    ) && crate::gradient::needs_repair(&unparsed.value)
}

/// `subgrid` in `grid-template-columns`, `grid-template-rows` or their shorthands, which
/// lightningcss does not know.
fn is_subgrid(unparsed: &lightningcss::properties::custom::UnparsedProperty<'_>) -> bool {
    use lightningcss::properties::PropertyId;
    match unparsed.property_id {
        PropertyId::GridTemplateColumns | PropertyId::GridTemplateRows => {
            crate::paged::subgrid_line_names(&unparsed.value).is_some()
        }
        // `grid` and `grid-template` with `subgrid` on either side.
        PropertyId::Grid | PropertyId::GridTemplate => {
            crate::paged::subgrid_shorthand_sides(&unparsed.value).is_some()
        }
        _ => false,
    }
}

/// `image-rendering: pixelated`, `crisp-edges`, `smooth` or `high-quality`, which lightningcss
/// knows only by the names of SVG 1.1 and the style keeps unparsed.
/// Whether a declaration is `transform-style: preserve-3d`, which lightningcss does not parse
/// (it expects `preserve3d`).
fn is_preserve_3d(unparsed: &lightningcss::properties::custom::UnparsedProperty<'_>) -> bool {
    use lightningcss::properties::custom::{Token, TokenOrValue};
    let mut significant = unparsed
        .value
        .0
        .iter()
        .filter(|token| !matches!(token, TokenOrValue::Token(Token::WhiteSpace(_))));
    matches!(
        unparsed.property_id,
        lightningcss::properties::PropertyId::TransformStyle(_)
    ) && matches!(
        (significant.next(), significant.next()),
        (Some(TokenOrValue::Token(Token::Ident(ident))), None) if ident.eq_ignore_ascii_case("preserve-3d")
    )
}

fn is_image_rendering_keyword(unparsed: &lightningcss::properties::custom::UnparsedProperty<'_>) -> bool {
    use lightningcss::properties::custom::{Token, TokenOrValue};
    let mut significant = unparsed
        .value
        .0
        .iter()
        .filter(|token| !matches!(token, TokenOrValue::Token(Token::WhiteSpace(_))));
    unparsed.property_id == lightningcss::properties::PropertyId::ImageRendering
        && matches!(
            (significant.next(), significant.next()),
            (Some(TokenOrValue::Token(Token::Ident(ident))), None)
                if ["pixelated", "crisp-edges", "smooth", "high-quality"]
                    .iter()
                    .any(|keyword| ident.eq_ignore_ascii_case(keyword))
        )
}

/// `text-align: -webkit-match-parent` (as in Bootstrap's reboot), `-internal-center` (the
/// alignment of `th` in Chrome's user agent stylesheet) or `-webkit-left`, `-webkit-center` and
/// `-webkit-right` (of `align` attributes and `<center>`), and `vertical-align:
/// -webkit-baseline-middle`, which lightningcss does not know.
/// Whether a declaration is `word-break: auto-phrase`, which lightningcss does not know.
fn is_auto_phrase(unparsed: &lightningcss::properties::custom::UnparsedProperty<'_>) -> bool {
    use lightningcss::properties::custom::{Token, TokenOrValue};
    let mut significant = unparsed
        .value
        .0
        .iter()
        .filter(|token| !matches!(token, TokenOrValue::Token(Token::WhiteSpace(_))));
    unparsed.property_id == lightningcss::properties::PropertyId::WordBreak
        && matches!(
            (significant.next(), significant.next()),
            (Some(TokenOrValue::Token(Token::Ident(ident))), None) if ident.eq_ignore_ascii_case("auto-phrase")
        )
}

/// Whether a declaration is `text-transform: math-auto`, which lightningcss does not know.
fn is_math_auto(unparsed: &lightningcss::properties::custom::UnparsedProperty<'_>) -> bool {
    use lightningcss::properties::custom::{Token, TokenOrValue};
    let mut significant = unparsed
        .value
        .0
        .iter()
        .filter(|token| !matches!(token, TokenOrValue::Token(Token::WhiteSpace(_))));
    unparsed.property_id == lightningcss::properties::PropertyId::TextTransform
        && matches!(
            (significant.next(), significant.next()),
            (Some(TokenOrValue::Token(Token::Ident(ident))), None) if ident.eq_ignore_ascii_case("math-auto")
        )
}

/// Whether a declaration is `flex-wrap: balance` (CSS Flexible Box Layout 2), which lightningcss
/// does not know.
fn is_flex_wrap_balance(unparsed: &lightningcss::properties::custom::UnparsedProperty<'_>) -> bool {
    matches!(
        unparsed.property_id,
        lightningcss::properties::PropertyId::FlexWrap(_)
    ) && flex_wrap_balance(&unparsed.value).is_some()
}

/// `flex-wrap: balance`, alone or with `wrap` or `wrap-reverse` (CSS Flexible Box Layout 2):
/// whether the lines go in reverse, if the tokens are such a value.
pub fn flex_wrap_balance(tokens: &TokenList<'_>) -> Option<bool> {
    use lightningcss::properties::custom::{Token, TokenOrValue};
    let mut balance = false;
    let mut wrap = None;
    for token in &tokens.0 {
        match token {
            TokenOrValue::Token(Token::WhiteSpace(_)) => {}
            TokenOrValue::Token(Token::Ident(ident)) if ident.eq_ignore_ascii_case("balance") && !balance => {
                balance = true;
            }
            TokenOrValue::Token(Token::Ident(ident))
                if wrap.is_none() && ident.eq_ignore_ascii_case("wrap") =>
            {
                wrap = Some(false);
            }
            TokenOrValue::Token(Token::Ident(ident))
                if wrap.is_none() && ident.eq_ignore_ascii_case("wrap-reverse") =>
            {
                wrap = Some(true);
            }
            _ => return None,
        }
    }
    balance.then_some(wrap.unwrap_or(false))
}

/// `normal`, `light`, `dark` or `palette-mix()` in `font-palette`, which lightningcss takes as a
/// dashed identifier alone.
fn is_font_palette_value(unparsed: &lightningcss::properties::custom::UnparsedProperty<'_>) -> bool {
    use lightningcss::properties::custom::{Token, TokenOrValue};
    let mut significant = unparsed
        .value
        .0
        .iter()
        .filter(|token| !matches!(token, TokenOrValue::Token(Token::WhiteSpace(_))));
    unparsed.property_id == lightningcss::properties::PropertyId::FontPalette
        && match (significant.next(), significant.next()) {
            (Some(TokenOrValue::Token(Token::Ident(ident))), None) => ["normal", "light", "dark"]
                .iter()
                .any(|keyword| ident.eq_ignore_ascii_case(keyword)),
            (Some(TokenOrValue::Function(function)), None) => {
                function.name.0.eq_ignore_ascii_case("palette-mix")
            }
            _ => false,
        }
}

/// A string in `text-overflow` (CSS Overflow 4), which lightningcss does not know.
fn is_text_overflow_string(unparsed: &lightningcss::properties::custom::UnparsedProperty<'_>) -> bool {
    use lightningcss::properties::custom::{Token, TokenOrValue};
    let mut significant = unparsed
        .value
        .0
        .iter()
        .filter(|token| !matches!(token, TokenOrValue::Token(Token::WhiteSpace(_))));
    matches!(
        unparsed.property_id,
        lightningcss::properties::PropertyId::TextOverflow(_)
    ) && matches!(
        (significant.next(), significant.next()),
        (Some(TokenOrValue::Token(Token::String(_))), None)
    )
}

fn is_prefixed_text_align(unparsed: &lightningcss::properties::custom::UnparsedProperty<'_>) -> bool {
    use lightningcss::properties::custom::{Token, TokenOrValue};
    let mut significant = unparsed
        .value
        .0
        .iter()
        .filter(|token| !matches!(token, TokenOrValue::Token(Token::WhiteSpace(_))));
    if unparsed.property_id == lightningcss::properties::PropertyId::VerticalAlign {
        let mut significant = unparsed
            .value
            .0
            .iter()
            .filter(|token| !matches!(token, TokenOrValue::Token(Token::WhiteSpace(_))));
        return matches!(
            (significant.next(), significant.next()),
            (Some(TokenOrValue::Token(Token::Ident(ident))), None)
                if ident.eq_ignore_ascii_case("-webkit-baseline-middle")
        );
    }
    unparsed.property_id == lightningcss::properties::PropertyId::TextAlign
        && matches!(
            (significant.next(), significant.next()),
            (Some(TokenOrValue::Token(Token::Ident(ident))), None)
                if ["-webkit-match-parent", "-internal-center", "-webkit-left", "-webkit-center", "-webkit-right"]
                    .iter()
                    .any(|keyword| ident.eq_ignore_ascii_case(keyword))
        )
}

/// `letter-spacing` or `word-spacing` with a percentage (CSS Text 4, §7), which lightningcss
/// does not parse for them; Fagus resolves it against the font size.
fn is_spacing_percentage(unparsed: &lightningcss::properties::custom::UnparsedProperty<'_>) -> bool {
    use lightningcss::properties::PropertyId;
    use lightningcss::stylesheet::PrinterOptions;
    use lightningcss::traits::Parse;
    use lightningcss::values::length::LengthPercentage;
    if !matches!(
        unparsed.property_id,
        PropertyId::LetterSpacing | PropertyId::WordSpacing
    ) {
        return false;
    }
    Property::Unparsed(unparsed.clone())
        .value_to_css_string(PrinterOptions::default())
        .is_ok_and(|text| LengthPercentage::parse_string(text.trim()).is_ok())
}

/// `flex-basis` or `flex` with a sizing keyword as the basis (`min-content`, `content`, …),
/// which lightningcss does not know; Fagus parses it from the tokens.
fn is_keyword_flex_basis(unparsed: &lightningcss::properties::custom::UnparsedProperty<'_>) -> bool {
    use lightningcss::properties::PropertyId;
    use lightningcss::properties::custom::{Token, TokenOrValue};
    const KEYWORDS: [&str; 7] = [
        "content",
        "min-content",
        "max-content",
        "fit-content",
        "stretch",
        "-webkit-fill-available",
        "-moz-available",
    ];
    let shorthand = match unparsed.property_id {
        PropertyId::Flex(_) => true,
        PropertyId::FlexBasis(_) => false,
        _ => return false,
    };
    let (mut keywords, mut numbers) = (0, 0);
    for token in &unparsed.value.0 {
        match token {
            TokenOrValue::Token(Token::WhiteSpace(_)) => {}
            TokenOrValue::Token(Token::Ident(ident))
                if keywords == 0 && KEYWORDS.iter().any(|known| ident.eq_ignore_ascii_case(known)) =>
            {
                keywords += 1;
            }
            TokenOrValue::Token(Token::Number { .. }) if shorthand && keywords == 0 && numbers < 2 => {
                numbers += 1;
            }
            _ => return false,
        }
    }
    keywords == 1
}

/// `position: running(name)`, which takes an element out of the flow for the page margins.
fn is_running_position(unparsed: &lightningcss::properties::custom::UnparsedProperty<'_>) -> bool {
    unparsed.property_id == lightningcss::properties::PropertyId::Position
        && crate::paged::running_name(&unparsed.value).is_some()
}

/// `font-size: math`, which scales the font size of `MathML` scripts by their depth.
fn is_math_font_size(unparsed: &lightningcss::properties::custom::UnparsedProperty<'_>) -> bool {
    use lightningcss::properties::custom::Token;
    unparsed.property_id == lightningcss::properties::PropertyId::FontSize
        && matches!(
            unparsed
                .value
                .0
                .iter()
                .filter(|token| !matches!(token, TokenOrValue::Token(Token::WhiteSpace(_))))
                .collect::<Vec<_>>()
                .as_slice(),
            [TokenOrValue::Token(Token::Ident(ident))] if ident.eq_ignore_ascii_case("math")
        )
}

/// Repairs the declarations of a `style` attribute and drops those that are not valid, as
/// [`check_block`] does for the rules of a style sheet (without diagnostics): so an invalid
/// declaration leaves an earlier one of the same property in force, as in Chrome.
pub(crate) fn check_attribute(block: &mut DeclarationBlock<'static>) {
    check_block(
        block,
        Location {
            source_index: 0,
            line: 0,
            column: 0,
        },
        "",
        &mut Vec::new(),
    );
}

/// The diagnostics of the declarations of a `style` attribute on line `line` of `source` (the
/// document): unknown properties and invalid values, as [`Stylesheet::parse`] reports those of
/// a style sheet, and declarations that do not parse.
pub fn style_attribute_diagnostics(css: &str, source: &str, line: u32) -> Vec<Diagnostic> {
    use lightningcss::stylesheet::StyleAttribute;
    let at = |code, message: String| Diagnostic {
        severity: Severity::Warning,
        code,
        message,
        source: source.to_owned(),
        line,
        column: 0,
    };
    if crate::nests_too_deep(css) {
        return vec![at(
            "css.too-deep",
            format!(
                "blocks or functions nest more than {} levels deep; the attribute is ignored",
                crate::MAX_NESTING
            ),
        )];
    }
    let css = &*crate::flat_math(css);
    let warnings = Arc::new(RwLock::new(Vec::new()));
    let options = ParserOptions {
        error_recovery: true,
        warnings: Some(Arc::clone(&warnings)),
        ..ParserOptions::default()
    };
    let parsed = crate::guard::guarded(|| {
        StyleAttribute::parse(css, options)
            .ok()
            .map(IntoOwned::into_owned)
    });
    let Some(Some(parsed)) = parsed else {
        return vec![at(
            "css.unparsable",
            "declarations that the CSS parser cannot handle are left out".to_owned(),
        )];
    };
    let mut diagnostics: Vec<Diagnostic> = warnings.read().map_or_else(
        |_| Vec::new(),
        |warnings| {
            warnings
                .iter()
                .map(|warning| {
                    at(
                        "css.parse-error",
                        format!("{}; the declaration is ignored", warning.kind),
                    )
                })
                .collect()
        },
    );
    let mut block = parsed.declarations;
    check_block(
        &mut block,
        Location {
            source_index: 0,
            line: line.saturating_sub(1),
            column: 0,
        },
        source,
        &mut diagnostics,
    );
    diagnostics
}

fn check_block(
    block: &mut DeclarationBlock<'static>,
    location: Location,
    source: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    crate::repair_block(block);
    for declarations in [&mut block.declarations, &mut block.important_declarations] {
        declarations.retain(|property| {
            let problem = match property {
                Property::Unparsed(unparsed)
                    if !has_variables(&unparsed.value)
                        && !is_wide_keyword(&unparsed.value)
                        && !is_interpolated_background(unparsed)
                        && !is_running_position(unparsed)
                        && !is_keyword_flex_basis(unparsed)
                        && !is_subgrid(unparsed)
                        && !is_prefixed_text_align(unparsed)
                        && !is_auto_phrase(unparsed)
                        && !is_math_auto(unparsed)
                        && !is_flex_wrap_balance(unparsed)
                        && !is_text_overflow_string(unparsed)
                        && !is_font_palette_value(unparsed)
                        && crate::flow_position(unparsed).is_none()
                        && !crate::is_anchored(unparsed)
                        && !crate::is_calc_size(unparsed)
                        && crate::clip_path_function(unparsed).is_none()
                        && !crate::is_newer_clip_shape(unparsed)
                        && !is_image_rendering_keyword(unparsed)
                        && !is_preserve_3d(unparsed)
                        && !is_math_font_size(unparsed)
                        && !crate::background_clip::is_border_area(unparsed)
                        && !is_spacing_percentage(unparsed) =>
                {
                    Some((
                        "css.invalid-value",
                        format!("invalid value for `{}`", unparsed.property_id.name()),
                    ))
                }
                // Chrome 157 does not support `display: run-in` and drops it.
                Property::Display(lightningcss::properties::display::Display::Pair(pair))
                    if pair.outside == lightningcss::properties::display::DisplayOutside::RunIn =>
                {
                    Some(("css.invalid-value", "invalid value for `display`".to_owned()))
                }
                // Chrome does not know `text-align: justify-all` and drops it.
                Property::TextAlign(lightningcss::properties::text::TextAlign::JustifyAll) => {
                    Some(("css.invalid-value", "invalid value for `text-align`".to_owned()))
                }
                // Nor does Chrome 157 take `full-width` and `full-size-kana` in `text-transform`.
                Property::TextTransform(transform) if !transform.other.is_empty() => Some((
                    "css.invalid-value",
                    "invalid value for `text-transform`".to_owned(),
                )),
                Property::Custom(custom) => match &custom.name {
                    CustomPropertyName::Unknown(name) => match crate::properties::classify(&name.0) {
                        // Chrome does not take `float: footnote` (nor other values it does not
                        // know) and drops the declaration.
                        PropertySupport::Known
                            if name.0.eq_ignore_ascii_case("float")
                                && !has_variables(&custom.value)
                                && !is_wide_keyword(&custom.value)
                                && !is_float_keyword(&custom.value) =>
                        {
                            Some(("css.invalid-value", "invalid value for `float`".to_owned()))
                        }
                        // `element()` stands alone (CSS Generated Content for Paged Media 3).
                        PropertySupport::Known
                            if name.0.eq_ignore_ascii_case("content")
                                && !has_variables(&custom.value)
                                && crate::paged::content_misuses_element(&custom.value) =>
                        {
                            Some((
                                "css.invalid-value",
                                "invalid value for `content`: `element()` cannot be combined with other values"
                                    .to_owned(),
                            ))
                        }
                        PropertySupport::Known
                            if name.0.eq_ignore_ascii_case("hanging-punctuation")
                                && !has_variables(&custom.value)
                                && !is_wide_keyword(&custom.value)
                                && crate::hanging::HangingPunctuation::from_tokens(&custom.value).is_none() =>
                        {
                            Some((
                                "css.invalid-value",
                                "invalid value for `hanging-punctuation`: it takes `none` or `first`, `last` and one of `force-end` and `allow-end`, each at most once"
                                    .to_owned(),
                            ))
                        }
                        PropertySupport::Known => None,
                        PropertySupport::Ignored => return false,
                        PropertySupport::Unknown => {
                            Some(("css.unknown-property", format!("unknown property `{}`", name.0)))
                        }
                    },
                    CustomPropertyName::Custom(_) => None,
                },
                _ => None,
            };
            let Some((code, message)) = problem else {
                return true;
            };
            diagnostics.push(Diagnostic {
                severity: Severity::Warning,
                code,
                message: format!("{message}; the declaration is ignored"),
                source: source.to_owned(),
                line: location.line + 1,
                column: location.column,
            });
            false
        });
    }
}

/// Whether the value is a CSS-wide keyword (`inherit`, `initial`, …), which lightningcss keeps
/// unparsed for most properties.
fn is_wide_keyword(tokens: &TokenList<'_>) -> bool {
    use lightningcss::properties::custom::Token;
    let mut significant = tokens
        .0
        .iter()
        .filter(|token| !matches!(token, TokenOrValue::Token(Token::WhiteSpace(_))));
    matches!(
        (significant.next(), significant.next()),
        (Some(TokenOrValue::Token(Token::Ident(ident))), None)
            if matches!(ident.to_ascii_lowercase().as_str(), "inherit" | "initial" | "unset" | "revert" | "revert-layer")
    )
}

/// Whether the value is one of the keywords of `float` that Chrome takes.
fn is_float_keyword(tokens: &TokenList<'_>) -> bool {
    use lightningcss::properties::custom::Token;
    let mut significant = tokens
        .0
        .iter()
        .filter(|token| !matches!(token, TokenOrValue::Token(Token::WhiteSpace(_))));
    matches!(
        (significant.next(), significant.next()),
        (Some(TokenOrValue::Token(Token::Ident(ident))), None)
            if matches!(ident.to_ascii_lowercase().as_str(), "left" | "right" | "none" | "inline-start" | "inline-end")
    )
}

fn has_variables(tokens: &TokenList<'_>) -> bool {
    crate::has_computed_values(tokens)
}

/// Parses a stylesheet with lightningcss, with the diagnostics of what it skips; `None` if
/// lightningcss panics.
fn parse_sheet(css: &str, source: &str) -> Option<(StyleSheet<'static>, Vec<Diagnostic>)> {
    let warnings = Arc::new(RwLock::new(Vec::new()));
    let options = ParserOptions {
        filename: source.to_owned(),
        error_recovery: true,
        warnings: Some(Arc::clone(&warnings)),
        ..ParserOptions::default()
    };
    let (sheet, mut diagnostics) = crate::guard::guarded(|| match StyleSheet::parse(css, options) {
        Ok(sheet) => (sheet.into_owned(), Vec::new()),
        Err(error) => {
            let diagnostic = diagnostic(source, &error.kind.to_string(), error.loc.as_ref());
            (empty_sheet(source), vec![diagnostic])
        }
    })?;
    if let Ok(warnings) = warnings.read() {
        diagnostics.extend(
            warnings
                .iter()
                .filter(|warning| !known_pseudo_element(&warning.kind))
                .map(|warning| {
                    let message = if in_nth_page_selector(css, warning.loc.as_ref()) {
                        "invalid page selector with `:nth()`".to_owned()
                    } else {
                        warning.kind.to_string()
                    };
                    diagnostic(source, &message, warning.loc.as_ref())
                }),
        );
    }
    Some((sheet, diagnostics))
}

/// Whether a parse error at `location` in `css` lies in the selector of an `@page` rule with
/// `:nth()` (CSS Generated Content for Paged Media 3, §3.1), which is left as it is (see
/// [`crate::encode_nth_pages`]) where it is not valid.
fn in_nth_page_selector(css: &str, location: Option<&lightningcss::error::ErrorLocation>) -> bool {
    let Some(location) = location else {
        return false;
    };
    let Some(line) = css.lines().nth(location.line as usize) else {
        return false;
    };
    let line = line.to_ascii_lowercase();
    let column = (location.column as usize).min(line.len());
    let Some(rule) = line.get(..column).and_then(|before| before.rfind("@page")) else {
        return false;
    };
    let prelude = line[rule..].split('{').next().unwrap_or_default();
    prelude.contains(":nth(")
}

/// Whether a warning is about a pseudo-element that lightningcss does not know but Fagus does
/// (CSS Overflow 5's scroll markers and buttons), whose rules it keeps.
fn known_pseudo_element(kind: &lightningcss::error::ParserError<'_>) -> bool {
    use lightningcss::error::{ParserError, SelectorError};
    matches!(
        kind,
        ParserError::SelectorError(SelectorError::UnsupportedPseudoElement(name))
            if ["scroll-marker", "scroll-marker-group", "scroll-button", "column"]
                .iter()
                .any(|known| name.eq_ignore_ascii_case(known))
    )
}

fn empty_sheet(source: &str) -> StyleSheet<'static> {
    let options = ParserOptions {
        filename: source.to_owned(),
        ..ParserOptions::default()
    };
    StyleSheet::parse("", options)
        .expect("empty stylesheet parses")
        .into_owned()
}

fn diagnostic(
    source: &str,
    message: &str,
    location: Option<&lightningcss::error::ErrorLocation>,
) -> Diagnostic {
    Diagnostic {
        severity: Severity::Warning,
        code: "css.parse-error",
        message: format!("{message}; the rule or declaration is ignored"),
        source: source.to_owned(),
        line: location.map_or(0, |location| location.line + 1),
        column: location.map_or(0, |location| location.column),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_image_rendering_keywords_of_css_images() {
        // lightningcss parses only the SVG 1.1 names; the others stay unparsed for the style.
        let sheet = Stylesheet::parse(
            "img { image-rendering: pixelated } i { image-rendering: crisp-edges }
             b { image-rendering: smooth } s { image-rendering: blocky }",
            Origin::Author,
            "<style> 1",
        );
        let kept: Vec<_> = sheet
            .sheet
            .rules
            .0
            .iter()
            .filter_map(|rule| match rule {
                CssRule::Style(rule) => Some(rule.declarations.declarations.len()),
                _ => None,
            })
            .collect();
        assert_eq!(kept, vec![1, 1, 1, 0]);
        assert_eq!(sheet.diagnostics().len(), 1, "{:?}", sheet.diagnostics());
    }

    #[test]
    fn leaves_out_only_what_makes_the_parser_panic() {
        // `calc()` of a number where lightningcss expects a percentage reaches an
        // `unreachable!()` in its parser.
        let sheet = Stylesheet::parse(
            "p { color: red; font-stretch: calc(5) } @keyframes k { calc(5) { color: red } }
             a { color: blue } @media print { b { color: green } }",
            Origin::Author,
            "<style> 1",
        );
        assert!(
            sheet
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.code == "css.unparsable")
        );
        let style_rules: Vec<_> = sheet
            .sheet
            .rules
            .0
            .iter()
            .filter_map(|rule| match rule {
                CssRule::Style(rule) => Some(rule.declarations.declarations.len()),
                _ => None,
            })
            .collect();
        // `p` keeps its color, `a` stays whole.
        assert_eq!(style_rules, vec![1, 1]);
        assert!(
            sheet
                .sheet
                .rules
                .0
                .iter()
                .any(|rule| matches!(rule, CssRule::Media(_)))
        );
    }

    #[test]
    fn resolves_urls_against_their_stylesheet() {
        use crate::join_url;
        assert_eq!(join_url("css/main.css", "img/logo.png"), "css/img/logo.png");
        assert_eq!(join_url("css/main.css", "../fonts/a.woff2"), "fonts/a.woff2");
        assert_eq!(join_url("a/b/c.css", "./d.png?v=1#x"), "a/b/d.png?v=1#x");
        assert_eq!(join_url("../shared/s.css", "../x.png"), "../x.png");
        assert_eq!(join_url("/css/main.css", "../img.png"), "/img.png");
        assert_eq!(
            join_url("https://cdn.example.com/lib/css/a.css", "../img/b.png"),
            "https://cdn.example.com/lib/img/b.png"
        );
        for kept in [
            "/root.png",
            "https://x.y/z.png",
            "data:image/png;base64,AAAA",
            "#filter",
        ] {
            assert_eq!(join_url("css/main.css", kept), kept);
        }
    }

    #[test]
    fn imports_stylesheets_inside_their_conditions() {
        let files = [
            (
                "css/a.css",
                "@import 'b.css' print; .a { background: url(a.png) }",
            ),
            ("css/b.css", "@import url(c.css) layer(base); .b { color: red }"),
            ("css/c.css", ".c { color: blue }"),
        ];
        let sheet = Stylesheet::parse(
            "@import url(css/a.css); @import 'missing.css'; .x { color: green }",
            Origin::Author,
            "<style> 1",
        )
        .with_imports(&mut |url| {
            files
                .iter()
                .find(|(name, _)| *name == url)
                .map(|(_, css)| (*css).to_owned())
        });
        let printed = format!("{:?}", sheet.sheet.rules);
        // The imported rules come first, inside the media query and the layer of their rule, with
        // their URLs resolved against their own stylesheet.
        assert!(printed.find(".c").unwrap() < printed.find(".b").unwrap());
        assert!(printed.find(".b").unwrap() < printed.find(".x").unwrap());
        assert!(printed.contains("css/a.png"), "{printed}");
        assert!(
            printed.contains("Media(") && printed.contains("LayerBlock("),
            "{printed}"
        );
        assert_eq!(
            sheet.diagnostics().len(),
            1,
            "the missing one: {:?}",
            sheet.diagnostics()
        );
    }

    #[test]
    fn ignores_stylesheets_that_nest_too_deep() {
        let deep = format!("p {{ width: {}1px{} }}", "calc(".repeat(1000), ")".repeat(1000));
        let sheet = Stylesheet::parse(&deep, Origin::Author, "deep.css");
        assert_eq!(sheet.diagnostics()[0].code, "css.too-deep");
        // Strings and comments do not count.
        let fine = format!(
            "p::before {{ content: \"{}\" }} /* {} */ p {{ width: calc(1px + (2px * 3)) }}",
            "(".repeat(1000),
            "{".repeat(1000)
        );
        assert!(!crate::nests_too_deep(&fine));
        assert_eq!(
            Stylesheet::parse(&fine, Origin::Author, "fine.css").diagnostics(),
            []
        );
    }

    #[test]
    fn parses_modern_css() {
        let sheet = Stylesheet::parse(
            "@layer base, utilities;
             @layer utilities { .a { color: oklch(0.7 0.15 150) } }
             .card { color: red; &.ok { color: green } }
             :is(.x, .y):has(> .z) { margin: 1px }",
            Origin::Author,
            "test.css",
        );
        assert!(sheet.diagnostics().is_empty(), "{:?}", sheet.diagnostics());
        assert_eq!(sheet.len(), 4);
    }

    #[test]
    fn keeps_what_chrome_keeps_of_text_declarations() {
        // Percentages of `letter-spacing` and `word-spacing`, which lightningcss does not parse,
        // stay; `text-align: justify-all` and `text-transform: full-width`, which Chrome does not
        // know, go.
        let sheet = Stylesheet::parse(
            "p { letter-spacing: 10%; word-spacing: calc(1em + 50%); text-align: justify-all;
                 text-transform: uppercase; text-transform: uppercase full-width }",
            Origin::Author,
            "test.css",
        );
        let codes: Vec<_> = sheet
            .diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code)
            .collect();
        assert_eq!(codes, ["css.invalid-value", "css.invalid-value"]);
        let CssRule::Style(rule) = &sheet.sheet.rules.0[0] else {
            panic!("style rule")
        };
        assert_eq!(rule.declarations.declarations.len(), 3);
    }

    #[test]
    fn drops_content_that_combines_element_with_other_values() {
        let sheet = Stylesheet::parse(
            "@page {
               @top-left { content: element(a) element(b) }
               @top-center { content: element(a) ' – ' counter(page) }
               @top-right { content: element(a, other) }
               @bottom-left { content: element(a, last) }
               @bottom-center { content: ELEMENT( a ) }
               @bottom-right { content: element(a) var(--more) }
             }",
            Origin::Author,
            "pages.css",
        );
        let found: Vec<(u32, &str)> = sheet
            .diagnostics()
            .iter()
            .map(|diagnostic| (diagnostic.line, diagnostic.message.as_str()))
            .collect();
        let message = "invalid value for `content`: `element()` cannot be combined with other values; the declaration is ignored";
        assert_eq!(found, [(2, message), (3, message), (4, message)]);
        let CssRule::Page(page) = &sheet.sheet.rules.0[0] else {
            panic!("page rule")
        };
        let kept: Vec<usize> = page
            .rules
            .iter()
            .map(|margin| margin.declarations.declarations.len())
            .collect();
        assert_eq!(kept, [0, 0, 0, 1, 1, 1]);
    }

    #[test]
    fn drops_and_reports_page_heights_that_are_not_auto_or_fit_content() {
        let sheet = Stylesheet::parse(
            "@page { size: 80mm 100mm; -fagus-page-height: fit-content }\n@page { -fagus-page-height: AUTO; -fagus-page-height: 200mm }",
            Origin::Author,
            "pages.css",
        );
        let found: Vec<(&str, u32, &str)> = sheet
            .diagnostics()
            .iter()
            .map(|diagnostic| (diagnostic.code, diagnostic.line, diagnostic.message.as_str()))
            .collect();
        assert_eq!(
            found,
            [(
                "css.invalid-value",
                2,
                "invalid value for `-fagus-page-height`: it takes `auto` or `fit-content`; the declaration is ignored"
            )]
        );
        let kept = |rule: &CssRule<'static>| match rule {
            CssRule::Page(page) => page.declarations.declarations.len(),
            _ => panic!("page rule"),
        };
        assert_eq!(kept(&sheet.sheet.rules.0[0]), 2);
        assert_eq!(kept(&sheet.sheet.rules.0[1]), 1);
    }

    #[test]
    fn drops_and_reports_page_sizes_that_chrome_does_not_take() {
        let sheet = Stylesheet::parse(
            "@page { size: A5 }\n@page receipt {\n  size: 80mm auto;\n}\n@media print { @page :first { size: A6 landscape } }\n@page { size: 1px 2px 3px; size: var(--size); size: inherit; margin: 1cm }",
            Origin::Author,
            "pages.css",
        );
        let found: Vec<(&str, u32, &str)> = sheet
            .diagnostics()
            .iter()
            .map(|diagnostic| (diagnostic.code, diagnostic.line, diagnostic.message.as_str()))
            .collect();
        let ignored = "the declaration is ignored and the page keeps the size that applied before";
        assert_eq!(
            found,
            [
                (
                    "css.invalid-value",
                    2,
                    &*format!(
                        "invalid value for `size`: a page's height cannot follow its content in CSS; give both lengths, as in `80mm 100mm`, and `-fagus-page-height: fit-content` to let the page grow; {ignored}"
                    )
                ),
                (
                    "css.invalid-value",
                    5,
                    &*format!(
                        "invalid value for `size`: Chrome does not know the page size `A6`; write `148mm 105mm`; {ignored}"
                    )
                ),
                (
                    "css.invalid-value",
                    6,
                    &*format!(
                        "invalid value for `size`: a page takes one length (a square) or two (width and height); {ignored}"
                    )
                ),
            ]
        );
        assert_eq!(sheet.diagnostics()[0].source, "pages.css");
        // The valid declarations stay, and those with `var()` or a CSS-wide keyword.
        let kept = |rule: &CssRule<'static>| match rule {
            CssRule::Page(page) => page.declarations.declarations.len(),
            _ => panic!("page rule"),
        };
        assert_eq!(kept(&sheet.sheet.rules.0[0]), 1);
        assert_eq!(kept(&sheet.sheet.rules.0[1]), 0);
        assert_eq!(kept(&sheet.sheet.rules.0[3]), 3);
    }

    #[test]
    fn keeps_page_selectors_with_nth_and_reports_invalid_ones() {
        let sheet = Stylesheet::parse(
            "@page :first { margin: 1cm }\n@page :NTH(2n+1) { margin: 2cm }\n@page chapter:first, :nth(1 of chapter) { margin: 3cm }\n@page :nth(x) { margin: 4cm }\n@page :blank { margin: 5cm }",
            Origin::Author,
            "pages.css",
        );
        let found: Vec<String> = sheet.diagnostics().iter().map(ToString::to_string).collect();
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].starts_with("pages.css:4:"), "{found:?}");
        assert!(
            found[0].ends_with(
                "warning[css.parse-error]: invalid page selector with `:nth()`; the rule or declaration is ignored"
            ),
            "{found:?}"
        );
        assert_eq!(sheet.len(), 4);
    }

    #[test]
    fn reports_invalid_declarations_of_style_attributes() {
        let diagnostics =
            style_attribute_diagnostics("colr: red; color: blu; margin: 2px; color: ;", "html", 7);
        let found: Vec<_> = diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code, diagnostic.source.as_str(), diagnostic.line))
            .collect();
        assert_eq!(
            found,
            [
                ("css.unknown-property", "html", 7),
                ("css.invalid-value", "html", 7),
                ("css.invalid-value", "html", 7),
            ],
            "{diagnostics:?}"
        );
        assert_eq!(
            style_attribute_diagnostics(
                "color: red; width: var(--w); interpolate-size: allow-keywords",
                "html",
                1
            ),
            []
        );
    }

    #[test]
    fn reports_footnotes_as_unsupported_and_invalid_hanging_punctuation() {
        let sheet = Stylesheet::parse(
            "p { float: left; float: footnote; footnote-display: block; footnote-policy: line;\n  float-reference: page; hanging-punctuation: first; hanging-punctuation: first first; float: inline-end }",
            Origin::Author,
            "notes.css",
        );
        let messages: Vec<_> = sheet
            .diagnostics()
            .iter()
            .map(|diagnostic| (diagnostic.code, diagnostic.message.as_str()))
            .collect();
        assert_eq!(
            messages,
            [
                (
                    "css.invalid-value",
                    "invalid value for `float`; the declaration is ignored"
                ),
                (
                    "css.unknown-property",
                    "unknown property `footnote-display`; the declaration is ignored"
                ),
                (
                    "css.unknown-property",
                    "unknown property `footnote-policy`; the declaration is ignored"
                ),
                (
                    "css.unknown-property",
                    "unknown property `float-reference`; the declaration is ignored"
                ),
                (
                    "css.invalid-value",
                    "invalid value for `hanging-punctuation`: it takes `none` or `first`, `last` and one of `force-end` and `allow-end`, each at most once; the declaration is ignored"
                ),
            ]
        );
        let CssRule::Style(rule) = &sheet.sheet.rules.0[0] else {
            panic!("style rule")
        };
        // The earlier `float: left` stays in force where `float: footnote` is dropped, the
        // earlier `hanging-punctuation` where the second is.
        assert_eq!(rule.declarations.declarations.len(), 3);
    }

    #[test]
    fn drops_and_reports_invalid_declarations() {
        let sheet = Stylesheet::parse(
            "p { margin: 1px }\nh1 {\n  color: banana;\n  colr: red;\n  width: var(--w);\n  margin-top: inherit;\n  break-inside: avoid;\n  interpolate-size: allow-keywords;\n}",
            Origin::Author,
            "invoice.css",
        );
        let codes: Vec<_> = sheet
            .diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code)
            .collect();
        assert_eq!(codes, ["css.invalid-value", "css.unknown-property"]);
        assert_eq!(sheet.diagnostics()[0].source, "invoice.css");
        assert_eq!(sheet.diagnostics()[0].line, 2);
        let CssRule::Style(rule) = &sheet.sheet.rules.0[1] else {
            panic!("style rule")
        };
        let kept: Vec<String> = rule
            .declarations
            .declarations
            .iter()
            .map(|property| property.property_id().name().to_owned())
            .collect();
        assert_eq!(
            kept,
            ["width", "margin-top", "break-inside"],
            "var(), keywords and print properties stay, interaction properties go silently"
        );
    }
}

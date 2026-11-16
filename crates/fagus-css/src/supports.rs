//! The functions of `@supports` besides declarations: `selector()`, `font-tech()`,
//! `font-format()` and `at-rule()` (CSS Conditional 4 and 5), answered as Chrome 157 answers
//! them, so that a stylesheet takes the branch that Chrome takes.

use cssparser::{Parser, ParserInput, Token};
use lightningcss::selector::SelectorList;
use lightningcss::stylesheet::ParserOptions;
use lightningcss::traits::ParseWithOptions;

/// The font technologies Chrome supports in `font-tech()`.
const FONT_TECHNOLOGIES: &[&str] = &[
    "features-opentype",
    "features-aat",
    "color-colrv0",
    "color-colrv1",
    "color-sbix",
    "color-cbdt",
    "variations",
    "palettes",
];

/// The font formats Chrome supports in `font-format()`.
const FONT_FORMATS: &[&str] = &["opentype", "truetype", "woff", "woff2", "collection"];

/// The at-rules Chrome knows in `at-rule()`: not `@charset`, which is no rule.
const AT_RULES: &[&str] = &[
    "media",
    "supports",
    "font-face",
    "page",
    "container",
    "layer",
    "scope",
    "starting-style",
    "property",
    "counter-style",
    "font-feature-values",
    "font-palette-values",
    "keyframes",
    "-webkit-keyframes",
    "import",
    "namespace",
    "position-try",
    "view-transition",
    "function",
    // Inside `@page`.
    "top-left-corner",
    "top-left",
    "top-center",
    "top-right",
    "top-right-corner",
    "bottom-left-corner",
    "bottom-left",
    "bottom-center",
    "bottom-right",
    "bottom-right-corner",
    "left-top",
    "left-middle",
    "left-bottom",
    "right-top",
    "right-middle",
    "right-bottom",
    // Inside `@font-feature-values`.
    "stylistic",
    "styleset",
    "character-variant",
    "swash",
    "ornaments",
    "annotation",
];

/// The pseudo-classes Chrome knows without arguments, with the four pseudo-elements that may
/// be written with one colon.
const PSEUDO_CLASSES: &[&str] = &[
    "-internal-autofill-previewed",
    "-internal-autofill-selected",
    "-internal-dialog-in-top-layer",
    "-internal-menulist-popover-with-menulist-anchor",
    "-internal-popover-in-top-layer",
    "-internal-relative-anchor",
    "-internal-select-has-slotted-button",
    "-internal-text-field",
    "-webkit-any-link",
    "-webkit-autofill",
    "-webkit-drag",
    "-webkit-full-page-media",
    "-webkit-full-screen",
    "-webkit-full-screen-ancestor",
    "active",
    "active-view-transition",
    "after",
    "any-link",
    "autofill",
    "before",
    "checked",
    "corner-present",
    "current",
    "decrement",
    "default",
    "defined",
    "disabled",
    "double-button",
    "empty",
    "enabled",
    "end",
    "first-child",
    "first-letter",
    "first-line",
    "first-of-type",
    "focus",
    "focus-visible",
    "focus-within",
    "fullscreen",
    "future",
    "granted",
    "horizontal",
    "host",
    "hover",
    "in-range",
    "increment",
    "indeterminate",
    "interest-source",
    "interest-target",
    "invalid",
    "last-child",
    "last-of-type",
    "link",
    "modal",
    "navigation-source",
    "no-button",
    "only-child",
    "only-of-type",
    "open",
    "optional",
    "out-of-range",
    "past",
    "picture-in-picture",
    "placeholder-shown",
    "popover-open",
    "read-only",
    "read-write",
    "required",
    "root",
    "scope",
    "single-button",
    "start",
    "target",
    "target-after",
    "target-before",
    "target-current",
    "unbounded",
    "user-invalid",
    "user-valid",
    "valid",
    "vertical",
    "visited",
    "window-inactive",
    "xr-overlay",
];

/// The pseudo-classes of scrollbar parts.
const SCROLLBAR_PSEUDO_CLASSES: &[&str] = &[
    "horizontal",
    "vertical",
    "decrement",
    "increment",
    "start",
    "end",
    "double-button",
    "single-button",
    "no-button",
    "corner-present",
    "window-inactive",
];

/// The pseudo-classes Chrome knows with arguments.
const FUNCTIONAL_PSEUDO_CLASSES: &[&str] = &[
    "-webkit-any",
    "active-view-transition-type",
    "dir",
    "has",
    "host",
    "host-context",
    "is",
    "lang",
    "not",
    "nth-child",
    "nth-last-child",
    "nth-last-of-type",
    "nth-of-type",
    "state",
    "where",
];

/// The pseudo-elements Chrome knows without arguments (besides the parts of form controls that
/// [`WEBKIT_PSEUDO_ELEMENTS`] lists).
const PSEUDO_ELEMENTS: &[&str] = &[
    "-internal-media-controls-overlay-cast-button",
    "-webkit-resizer",
    "-webkit-scrollbar",
    "-webkit-scrollbar-button",
    "-webkit-scrollbar-corner",
    "-webkit-scrollbar-thumb",
    "-webkit-scrollbar-track",
    "-webkit-scrollbar-track-piece",
    "after",
    "backdrop",
    "before",
    "checkmark",
    "column",
    "cue",
    "details-content",
    "file-selector-button",
    "first-letter",
    "first-line",
    "grammar-error",
    "interest-button",
    "marker",
    "permission-icon",
    "picker-icon",
    "placeholder",
    "scroll-marker",
    "scroll-marker-group",
    "search-text",
    "select-listbox",
    "selection",
    "spelling-error",
    "target-text",
    "view-transition",
];

/// The parts of form and media controls that Chrome lets authors select with `::-webkit-…`.
const WEBKIT_PSEUDO_ELEMENTS: &[&str] = &[
    "-webkit-input-placeholder",
    "-webkit-progress-bar",
    "-webkit-progress-value",
    "-webkit-progress-inner-element",
    "-webkit-meter-bar",
    "-webkit-meter-inner-element",
    "-webkit-meter-optimum-value",
    "-webkit-meter-suboptimum-value",
    "-webkit-meter-even-less-good-value",
    "-webkit-inner-spin-button",
    "-webkit-search-cancel-button",
    "-webkit-file-upload-button",
    "-webkit-calendar-picker-indicator",
    "-webkit-datetime-edit",
    "-webkit-datetime-edit-fields-wrapper",
    "-webkit-datetime-edit-text",
    "-webkit-datetime-edit-year-field",
    "-webkit-datetime-edit-month-field",
    "-webkit-datetime-edit-day-field",
    "-webkit-datetime-edit-hour-field",
    "-webkit-datetime-edit-minute-field",
    "-webkit-datetime-edit-second-field",
    "-webkit-datetime-edit-millisecond-field",
    "-webkit-datetime-edit-ampm-field",
    "-webkit-datetime-edit-week-field",
    "-webkit-slider-runnable-track",
    "-webkit-slider-thumb",
    "-webkit-slider-container",
    "-webkit-color-swatch",
    "-webkit-color-swatch-wrapper",
    "-webkit-textfield-decoration-container",
    "-webkit-media-controls",
    "-webkit-media-controls-panel",
    "-webkit-media-controls-play-button",
    "-webkit-media-controls-enclosure",
    "-webkit-media-controls-timeline",
    "-webkit-media-controls-current-time-display",
    "-webkit-media-controls-time-remaining-display",
    "-webkit-media-controls-fullscreen-button",
    "-webkit-media-controls-mute-button",
    "-webkit-media-controls-volume-slider",
    "-webkit-media-controls-toggle-closed-captions-button",
    "-webkit-media-controls-overlay-enclosure",
    "-webkit-media-controls-overlay-play-button",
    "-webkit-media-slider-container",
    "-webkit-media-slider-thumb",
    "-webkit-media-text-track-container",
    "-webkit-media-text-track-display",
];

/// The pseudo-elements Chrome knows with arguments.
const FUNCTIONAL_PSEUDO_ELEMENTS: &[&str] = &[
    "cue",
    "highlight",
    "part",
    "picker",
    "scroll-button",
    "slotted",
    "view-transition-group",
    "view-transition-group-children",
    "view-transition-image-pair",
    "view-transition-new",
    "view-transition-old",
];

fn listed(list: &[&str], name: &str) -> bool {
    list.iter().any(|entry| entry.eq_ignore_ascii_case(name))
}

/// Whether Chrome supports a function of `@supports` that lightningcss leaves unknown:
/// `font-tech(…)`, `font-format(…)` or `at-rule(…)`, each of one keyword. Anything else
/// (`foo(bar)`, a general enclosed condition) is false.
pub(crate) fn feature(css: &str) -> bool {
    let mut input = ParserInput::new(css);
    let mut parser = Parser::new(&mut input);
    let Ok(Token::Function(name)) = parser.next().cloned() else {
        return false;
    };
    let kind = name.to_ascii_lowercase();
    let answer = parser.parse_nested_block(|arguments| {
        let token = arguments.next()?.clone();
        arguments.expect_exhausted()?;
        let supported = match (kind.as_str(), &token) {
            ("font-tech", Token::Ident(tech)) => listed(FONT_TECHNOLOGIES, tech),
            ("font-format", Token::Ident(format)) => listed(FONT_FORMATS, format),
            ("at-rule", Token::AtKeyword(rule)) => listed(AT_RULES, rule),
            _ => false,
        };
        Ok::<_, cssparser::ParseError<'_, ()>>(supported)
    });
    answer.unwrap_or(false) && parser.is_exhausted()
}

/// Whether Chrome supports `selector(<complex-selector>)`: one selector that parses, whose
/// pseudo-classes and pseudo-elements Chrome knows, where they may stand.
pub(crate) fn selector(css: &str) -> bool {
    let mut input = ParserInput::new(css);
    let mut parser = Parser::new(&mut input);
    let mut chained = Vec::new();
    if scan(&mut parser, Context::Top, &mut chained).is_err() {
        return false;
    }
    // lightningcss takes no pseudo-class or pseudo-element after a pseudo-element (but
    // `::before::marker`, `::selection:window-inactive` are valid): it parses the selector
    // without those, which the scan has checked.
    let mut plain = String::with_capacity(css.len());
    let mut from = 0;
    for (start, end, replacement) in chained {
        plain.push_str(css.get(from..start).unwrap_or_default());
        plain.push_str(replacement);
        from = end;
    }
    plain.push_str(css.get(from..).unwrap_or_default());
    crate::guarded(|| {
        SelectorList::parse_string_with_options(&plain, ParserOptions::default())
            .ok()
            .map(|list| list.0.len() == 1)
    })
    .flatten()
        == Some(true)
}

/// What a list of tokens being scanned is the argument of.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Context {
    /// The selector itself.
    Top,
    /// `:has()`, in which `:has()` may not appear again.
    Has,
    /// A compound selector alone (`:host()`), without combinators.
    Compound,
    /// Selectors in another function, or an argument not checked further.
    Other,
}

type Scan<'i> = Result<(), cssparser::ParseError<'i, ()>>;

fn reject<'i>(parser: &Parser<'i, '_>) -> cssparser::ParseError<'i, ()> {
    parser.new_custom_error(())
}

/// Checks the pseudo-classes and pseudo-elements of a selector, recursing into the selectors of
/// functional pseudo-classes, and the case flags of attribute selectors.
fn scan<'i>(
    parser: &mut Parser<'i, '_>,
    context: Context,
    chained: &mut Vec<(usize, usize, &'static str)>,
) -> Scan<'i> {
    // The pseudo-element the current compound selector ends in, which limits what may follow.
    let mut element: Option<String> = None;
    let mut previous: Option<Token<'i>> = None;
    loop {
        let start = parser.position().byte_index();
        let token = match parser.next_including_whitespace() {
            Ok(token) => token.clone(),
            Err(_) => return Ok(()),
        };
        // A namespace prefix: none, or any (`*|a`); Chrome knows no other without
        // `@namespace`.
        if matches!(token, Token::Delim('|')) && matches!(previous, Some(Token::Ident(_))) {
            return Err(reject(parser));
        }
        previous = Some(token.clone());
        match token {
            Token::Colon => {
                let pseudo_element = parser.try_parse(Parser::expect_colon).is_ok();
                let token = parser.next_including_whitespace()?.clone();
                let (name, functional) = match &token {
                    Token::Ident(name) => (name.to_ascii_lowercase(), false),
                    Token::Function(name) => (name.to_ascii_lowercase(), true),
                    _ => return Err(reject(parser)),
                };
                let known = match (pseudo_element, functional) {
                    (false, false) => listed(PSEUDO_CLASSES, &name),
                    (false, true) => listed(FUNCTIONAL_PSEUDO_CLASSES, &name),
                    (true, false) => listed(PSEUDO_ELEMENTS, &name) || listed(WEBKIT_PSEUDO_ELEMENTS, &name),
                    (true, true) => listed(FUNCTIONAL_PSEUDO_ELEMENTS, &name),
                };
                if !known {
                    return Err(reject(parser));
                }
                let legacy = !pseudo_element
                    && matches!(name.as_str(), "before" | "after" | "first-letter" | "first-line");
                if let Some(previous) = &element
                    && !may_follow(previous, &name, pseudo_element || legacy)
                {
                    return Err(reject(parser));
                }
                let follows = element.is_some();
                if pseudo_element || legacy {
                    element = Some(name.clone());
                }
                if functional {
                    let inner = match name.as_str() {
                        "has" if context == Context::Has => return Err(reject(parser)),
                        "has" => Context::Has,
                        "host" | "host-context" => Context::Compound,
                        _ if context == Context::Has => Context::Has,
                        _ => Context::Other,
                    };
                    parser.parse_nested_block(|arguments| {
                        if matches!(name.as_str(), "is" | "where" | "not" | "has" | "-webkit-any") {
                            // No empty list of selectors.
                            arguments.skip_whitespace();
                            if arguments.is_exhausted() {
                                return Err(reject(arguments));
                            }
                        }
                        if name == "dir" {
                            // Any direction, not only `ltr` and `rtl` as lightningcss.
                            arguments.expect_ident()?;
                            return arguments.expect_exhausted().map_err(Into::into);
                        }
                        if name == "lang" {
                            // One language range, an identifier: Chrome takes neither strings
                            // nor lists.
                            arguments.expect_ident()?;
                            return arguments.expect_exhausted().map_err(Into::into);
                        }
                        if name == "picker" {
                            arguments.expect_ident_matching("select")?;
                            return arguments.expect_exhausted().map_err(Into::into);
                        }
                        scan(arguments, inner, &mut Vec::new())
                    })?;
                }
                if context == Context::Top {
                    // What lightningcss does not parse: replaced by nothing where it follows a
                    // pseudo-element, by a pseudo-class it knows elsewhere.
                    let end = parser.position().byte_index();
                    if follows {
                        chained.push((start, end, ""));
                    } else if !pseudo_element && (name == "dir" || listed(SCROLLBAR_PSEUDO_CLASSES, &name)) {
                        chained.push((start, end, ":root"));
                    }
                }
            }
            Token::SquareBracketBlock => {
                parser.parse_nested_block(|attribute| {
                    // The flag after the value of an attribute selector: only `i`.
                    let mut tokens = Vec::new();
                    while let Ok(token) = attribute.next_including_whitespace() {
                        tokens.push(token.clone());
                    }
                    while matches!(tokens.last(), Some(Token::WhiteSpace(_))) {
                        tokens.pop();
                    }
                    let matcher = tokens.iter().any(|token| {
                        matches!(
                            token,
                            Token::Delim('=')
                                | Token::IncludeMatch
                                | Token::DashMatch
                                | Token::PrefixMatch
                                | Token::SuffixMatch
                                | Token::SubstringMatch
                        )
                    });
                    match tokens.as_slice() {
                        [.., Token::WhiteSpace(_), Token::Ident(flag)]
                            if matcher && !flag.eq_ignore_ascii_case("i") =>
                        {
                            Err(reject(attribute))
                        }
                        _ => Ok(()),
                    }
                })?;
            }
            Token::WhiteSpace(_) | Token::Delim('>' | '+' | '~') => {
                if context == Context::Compound {
                    // Whitespace at the ends of the argument is no combinator.
                    let rest = parser.try_parse(|parser| {
                        parser.skip_whitespace();
                        parser.expect_exhausted()
                    });
                    if rest.is_err() {
                        return Err(reject(parser));
                    }
                }
                element = None;
            }
            Token::Comma => element = None,
            Token::Function(_) | Token::ParenthesisBlock => {
                parser.parse_nested_block(|inner| scan(inner, Context::Other, &mut Vec::new()))?;
            }
            _ => {}
        }
    }
}

/// Whether a pseudo-class or pseudo-element may follow the pseudo-element `previous` in a
/// compound selector, as Chrome allows it.
fn may_follow(previous: &str, name: &str, is_element: bool) -> bool {
    match previous {
        "before" | "after" => is_element && name == "marker",
        "part" => true,
        "slotted" | "details-content" => {
            is_element
                && matches!(
                    name,
                    "before" | "after" | "marker" | "placeholder" | "file-selector-button"
                )
        }
        "selection" => !is_element && name == "window-inactive",
        _ if previous.starts_with("-webkit-scrollbar") || previous == "-webkit-resizer" => {
            !is_element
                && (listed(SCROLLBAR_PSEUDO_CLASSES, name)
                    || matches!(name, "hover" | "active" | "enabled" | "disabled"))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{feature, selector};

    #[test]
    fn answers_font_functions_as_chrome() {
        for supported in [
            "font-tech(features-opentype)",
            "font-tech(color-COLRv1)",
            "Font-Tech( variations )",
            "font-tech(palettes)",
            "font-format(woff2)",
            "font-format(TrueType)",
            "font-format(collection)",
        ] {
            assert!(feature(supported), "{supported}");
        }
        for unsupported in [
            "font-tech(features-graphite)",
            "font-tech(color-SVG)",
            "font-tech(incremental)",
            "font-tech(variations, palettes)",
            "font-tech(features-opentype features-aat)",
            "font-format(embedded-opentype)",
            "font-format(svg)",
            "font-format(\"woff2\")",
            "foo(bar)",
        ] {
            assert!(!feature(unsupported), "{unsupported}");
        }
    }

    #[test]
    fn answers_at_rule_as_chrome() {
        for supported in [
            "at-rule(@media)",
            "AT-RULE(@MEDIA)",
            "at-rule( @layer )",
            "at-rule(@position-try)",
            "at-rule(@top-left)",
            "at-rule(@swash)",
            "at-rule(@-webkit-keyframes)",
        ] {
            assert!(feature(supported), "{supported}");
        }
        for unsupported in [
            "at-rule(@charset)",
            "at-rule(@custom-media)",
            "at-rule(@nonsense)",
            "at-rule(media)",
            "at-rule(@media screen)",
            "at-rule(@media, @page)",
            "at-rule()",
            "at-rule(@-moz-keyframes)",
        ] {
            assert!(!feature(unsupported), "{unsupported}");
        }
    }

    #[test]
    fn answers_selector_as_chrome() {
        for supported in [
            "a > b",
            ":has(> a)",
            ":is(a, b)",
            "::before",
            ":before",
            "::before::marker",
            "::-webkit-scrollbar:horizontal",
            "::selection:window-inactive",
            "::part(x)::before",
            "::slotted(a)::before",
            ":host::before",
            ":state(foo):hover",
            ":lang(\\*-CH)",
            ":nth-child(odd of a, b)",
            "::picker(select)",
            "::-webkit-progress-bar",
            "[a=b i]",
            "*|a",
            "&.a",
            ":scope > a",
            "a b > c + d ~ e",
            ":host(.a)",
            ":horizontal",
            ":dir(foo)",
            "[a|=b]",
        ] {
            assert!(selector(supported), "{supported}");
        }
        for unsupported in [
            ":nonsense",
            "a, b",
            ":has-slotted",
            ":heading",
            ":muted",
            ":-moz-any(a)",
            "::-moz-selection",
            ":lang(\"de\")",
            ":lang(de, en)",
            ":is(a, :nonsense)",
            ":not(:nonsense)",
            "a:has(b:has(c))",
            ":host(a b)",
            "::after:hover",
            "::marker::before",
            "a::before b",
            "::first-line:hover",
            "::placeholder::before",
            "::picker(foo)",
            "::-webkit-details-marker",
            "::-webkit-foo",
            "[a=b s]",
            "a|b",
            ":first",
            "::hover",
            ":marker",
            "#1a",
            ":is()",
            ":where( )",
        ] {
            assert!(!selector(unsupported), "{unsupported}");
        }
    }
}

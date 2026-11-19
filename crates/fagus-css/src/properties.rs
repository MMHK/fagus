//! Properties that lightningcss does not parse itself.
//!
//! lightningcss stores declarations of properties it does not know as raw tokens. Many of them
//! matter for print (`content`, `break-inside`, `orphans`, `page`, `string-set`, …); Fagus parses
//! those from the tokens. Properties that only affect interaction or animation are dropped
//! silently, like browsers ignore them when printing. Everything else is an unknown property.

/// How Fagus treats a property that lightningcss does not know.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropertySupport {
    /// Parsed by Fagus from the raw tokens.
    Known,
    /// Without effect on printed output; dropped without a diagnostic.
    Ignored,
    /// Not a CSS property Fagus knows of; dropped with a diagnostic.
    Unknown,
}

/// Properties parsed by Fagus from tokens because lightningcss does not know them.
const KNOWN: &[&str] = &[
    "anchor-name",
    "anchor-scope",
    "background-blend-mode",
    "bleed",
    "bookmark-label",
    "bookmark-level",
    "bookmark-state",
    "border-collapse",
    "border-shape",
    "break-after",
    "break-before",
    "break-inside",
    "caption-side",
    "clear",
    "clip",
    "column-count",
    "column-fill",
    "column-height",
    "column-rule",
    "column-rule-color",
    "column-rule-style",
    "column-rule-width",
    "column-span",
    "column-width",
    "column-wrap",
    "columns",
    "contain",
    "contain-intrinsic-block-size",
    "contain-intrinsic-height",
    "contain-intrinsic-inline-size",
    "contain-intrinsic-size",
    "contain-intrinsic-width",
    "content",
    "content-visibility",
    "corner-bottom-left-shape",
    "corner-bottom-right-shape",
    "corner-bottom-shape",
    "corner-end-end-shape",
    "corner-end-start-shape",
    "corner-left-shape",
    "corner-right-shape",
    "corner-shape",
    "corner-start-end-shape",
    "corner-start-start-shape",
    "corner-top-left-shape",
    "corner-top-right-shape",
    "corner-top-shape",
    "counter-increment",
    "counter-reset",
    "counter-set",
    "cx",
    "cy",
    "d",
    "empty-cells",
    "field-sizing",
    "flex-line-count",
    "float",
    "float-reference",
    "font-feature-settings",
    "font-kerning",
    "font-optical-sizing",
    "font-size-adjust",
    "font-synthesis",
    "font-synthesis-small-caps",
    "font-synthesis-style",
    "font-synthesis-weight",
    "font-variant",
    "font-variant-alternates",
    "font-variant-east-asian",
    "font-variant-ligatures",
    "font-variant-numeric",
    "font-variant-position",
    "font-variation-settings",
    "footnote-display",
    "footnote-policy",
    "frame-sizing",
    "hanging-punctuation",
    "hyphenate-character",
    "hyphenate-limit-chars",
    "image-orientation",
    "initial-letter",
    "isolation",
    "line-break",
    "margin-trim",
    "marks",
    "math-depth",
    "math-shift",
    "math-style",
    "object-fit",
    "object-position",
    "orphans",
    "outline-offset",
    "overflow-clip-margin",
    "page",
    "page-break-after",
    "page-break-before",
    "page-break-inside",
    "page-orientation",
    "position-anchor",
    "position-area",
    "position-try",
    "position-try-fallbacks",
    "position-try-order",
    "position-visibility",
    "quotes",
    "r",
    "ruby-align",
    "ruby-position",
    "rx",
    "ry",
    "scroll-marker-group",
    "shape-image-threshold",
    "shape-margin",
    "shape-outside",
    "size",
    "string-set",
    "table-layout",
    "text-autospace",
    "text-box",
    "text-box-edge",
    "text-box-trim",
    "text-combine-upright",
    "text-fit",
    "text-orientation",
    "text-spacing-trim",
    "text-underline-offset",
    "text-underline-position",
    "text-wrap",
    "text-wrap-mode",
    "text-wrap-style",
    "vector-effect",
    "white-space-collapse",
    "widows",
    "will-change",
    "writing-mode",
    "x",
    "y",
    "zoom",
];

/// Properties without effect on printed output. (Those that lightningcss parses itself, such as
/// `cursor`, `user-select` or `appearance`, never come here and are dropped by the cascade.)
const IGNORED: &[&str] = &[
    "forced-color-adjust",
    "interpolate-size",
    // Chrome 157 does not take it yet (only `-webkit-line-clamp`).
    "line-clamp",
    "pointer-events",
    "touch-action",
];

const IGNORED_PREFIXES: &[&str] = &[
    "-webkit-",
    "-moz-",
    "-ms-",
    "-o-",
    "animation",
    // Microsoft Office's own properties, in all HTML that Word, Excel and Outlook write.
    "mso-",
    "overscroll-",
    "scroll-",
    "scrollbar-",
    "transition",
    "view-transition",
];

/// How Fagus treats the property `name` if lightningcss does not know it.
pub fn classify(name: &str) -> PropertySupport {
    let name = name.to_ascii_lowercase();
    if KNOWN.binary_search(&name.as_str()).is_ok()
        || name == "-webkit-line-clamp"
        || name == "-webkit-text-fill-color"
        || name == "-webkit-writing-mode"
        || name.starts_with("-fagus-")
    {
        PropertySupport::Known
    } else if IGNORED.binary_search(&name.as_str()).is_ok()
        || IGNORED_PREFIXES.iter().any(|prefix| name.starts_with(prefix))
    {
        PropertySupport::Ignored
    } else {
        PropertySupport::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_are_sorted_for_binary_search() {
        assert!(KNOWN.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(IGNORED.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn classifies_properties() {
        assert_eq!(classify("break-inside"), PropertySupport::Known);
        assert_eq!(classify("Content"), PropertySupport::Known);
        assert_eq!(classify("-webkit-line-clamp"), PropertySupport::Known);
        assert_eq!(classify("-webkit-writing-mode"), PropertySupport::Known);
        assert_eq!(classify("-webkit-text-fill-color"), PropertySupport::Known);
        assert_eq!(classify("-webkit-font-smoothing"), PropertySupport::Ignored);
        assert_eq!(classify("transition-duration"), PropertySupport::Ignored);
        assert_eq!(classify("mso-number-format"), PropertySupport::Ignored);
        assert_eq!(classify("colr"), PropertySupport::Unknown);
        assert_eq!(classify("hanging-punctuation"), PropertySupport::Known);
        assert_eq!(classify("footnote-display"), PropertySupport::Known);
        assert_eq!(classify("footnote-policy"), PropertySupport::Known);
        assert_eq!(classify("float-reference"), PropertySupport::Known);
    }

    #[test]
    fn ignores_only_properties_that_lightningcss_does_not_parse() {
        use lightningcss::properties::PropertyId;
        for name in IGNORED {
            assert!(
                matches!(PropertyId::from(*name), PropertyId::Custom(_)),
                "lightningcss parses `{name}` itself"
            );
        }
    }
}

//! The properties that apply in `@page` rules and their margin boxes.
//!
//! Chrome keeps only these longhands of a declaration in a page context (its
//! `valid_for_page_context` list in `css_properties.json5`) and drops the others, the
//! longhands of a shorthand one by one: `font: 9pt Serif` in a margin box sets the size and the
//! family but not `font-variant-numeric`, which keeps the value inherited from the root element.
//! `font-feature-settings`, the `font-variant-*` longhands, `font-variation-settings`,
//! `opacity` and `transform` are among those dropped.

/// The longhands that apply in a page context, sorted for binary search.
const PAGE_CONTEXT: &[&str] = &[
    "background-attachment",
    "background-blend-mode",
    "background-clip",
    "background-color",
    "background-image",
    "background-origin",
    "background-position-x",
    "background-position-y",
    "background-repeat",
    "background-size",
    "block-size",
    "border-block-end-color",
    "border-block-end-style",
    "border-block-end-width",
    "border-block-start-color",
    "border-block-start-style",
    "border-block-start-width",
    "border-bottom-color",
    "border-bottom-left-radius",
    "border-bottom-right-radius",
    "border-bottom-style",
    "border-bottom-width",
    "border-end-end-radius",
    "border-end-start-radius",
    "border-inline-end-color",
    "border-inline-end-style",
    "border-inline-end-width",
    "border-inline-start-color",
    "border-inline-start-style",
    "border-inline-start-width",
    "border-left-color",
    "border-left-style",
    "border-left-width",
    "border-right-color",
    "border-right-style",
    "border-right-width",
    "border-start-end-radius",
    "border-start-start-radius",
    "border-top-color",
    "border-top-left-radius",
    "border-top-right-radius",
    "border-top-style",
    "border-top-width",
    "box-sizing",
    "color",
    "content",
    "counter-increment",
    "counter-reset",
    "counter-set",
    "direction",
    "font-family",
    "font-kerning",
    "font-optical-sizing",
    "font-palette",
    "font-size",
    "font-size-adjust",
    "font-stretch",
    "font-style",
    "font-weight",
    "height",
    "inline-size",
    "inset-block-end",
    "letter-spacing",
    "line-height",
    "margin-block-end",
    "margin-block-start",
    "margin-bottom",
    "margin-inline-end",
    "margin-inline-start",
    "margin-left",
    "margin-right",
    "margin-top",
    "max-block-size",
    "max-height",
    "max-inline-size",
    "max-width",
    "min-block-size",
    "min-height",
    "min-inline-size",
    "min-width",
    "outline-color",
    "outline-offset",
    "outline-style",
    "outline-width",
    "padding-block-end",
    "padding-block-start",
    "padding-bottom",
    "padding-inline-end",
    "padding-inline-start",
    "padding-left",
    "padding-right",
    "padding-top",
    "page-orientation",
    "quotes",
    "size",
    "text-align",
    "text-align-last",
    "text-decoration-color",
    "text-decoration-line",
    "text-decoration-skip-ink",
    "text-decoration-skip-spaces",
    "text-decoration-style",
    "text-decoration-thickness",
    "text-indent",
    "text-overflow",
    "text-shadow",
    "text-transform",
    "text-underline-offset",
    "text-underline-position",
    "text-wrap",
    "text-wrap-mode",
    "text-wrap-style",
    "vertical-align",
    "visibility",
    "white-space",
    "white-space-collapse",
    "width",
    "word-spacing",
    "writing-mode",
    "z-index",
];

/// Whether a longhand (by its name, without a vendor prefix) applies in `@page` rules and margin
/// boxes. Custom properties always apply.
pub(crate) fn applies(name: &str) -> bool {
    name.starts_with("--") || PAGE_CONTEXT.binary_search(&name).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_context_list_is_sorted() {
        assert!(PAGE_CONTEXT.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn page_context_keeps_box_and_font_basics_only() {
        for name in [
            "font-size",
            "font-family",
            "color",
            "margin-top",
            "--x",
            "content",
        ] {
            assert!(applies(name), "{name}");
        }
        for name in [
            "font-feature-settings",
            "font-variant-numeric",
            "font-variant-caps",
            "font-variant-ligatures",
            "font-variation-settings",
            "opacity",
            "transform",
            "display",
        ] {
            assert!(!applies(name), "{name}");
        }
    }

    #[test]
    fn margin_box_drops_font_features_and_keeps_inherited_ones() {
        use crate::ComputedStyle;
        // The root element: `font-variant-numeric: oldstyle-nums; letter-spacing: 3px`.
        let mut root = ComputedStyle::default();
        root.font_variant.numeric = 0b10;
        root.letter_spacing = 3.0;
        let margin_css = "font: 12px Serif; font-feature-settings: \"smcp\"; \
                          font-variant-caps: small-caps; opacity: 0.5; color: red";
        let margin_block = fagus_css::parse_declarations(margin_css).expect("parses");
        let style = crate::style_from_declarations(&[&margin_block.declarations], &root, (0.0, 0.0));
        assert!((style.font_size - 12.0).abs() < 1e-3);
        assert!(style.font_feature_settings.is_empty());
        assert!((style.opacity - 1.0).abs() < 1e-6);
        assert!((style.letter_spacing - 3.0).abs() < 1e-3);
        // `font` does not reset `font-variant-numeric` here: it stays inherited (onum).
        assert!(
            style
                .font_features()
                .iter()
                .any(|(tag, value)| tag == b"onum" && *value == 1)
        );
        assert!(
            !style
                .font_features()
                .iter()
                .any(|(tag, _)| tag == b"smcp" || tag == b"c2sc")
        );
    }
}

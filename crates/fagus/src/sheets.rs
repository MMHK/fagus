//! Stylesheets parsed for earlier documents. An engine renders document after document from
//! the same templates, whose `<style>` elements and stylesheets are the same each time: they are
//! parsed once and kept, as long as they were used recently.
//!
//! A stylesheet is found by its text, whether the document is in quirks mode (which changes how
//! colors and lengths read), and where it comes from: the number and the node of its `<style>`
//! element, the URL of its `<link>`, or its name. Stylesheets with `@import` rules are not kept:
//! what they import is loaded again for each document.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

use fagus_css::Stylesheet;
use rustc_hash::FxHashMap;

/// The most stylesheets kept.
const MAX_SHEETS: usize = 64;

/// The most stylesheet text kept, in bytes.
const MAX_BYTES: usize = 16 << 20;

/// Where a stylesheet comes from.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Origin {
    /// The `<style>` element with this number (counted from 1) and node.
    Style(usize, fagus_dom::NodeId),
    /// A `<link>` element (the node) to this URL: its parent is the root of the `@scope` rules
    /// without a prelude in the stylesheet.
    Link(String, fagus_dom::NodeId),
    /// A stylesheet given with the document, by its name.
    Given(String),
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    css: Arc<str>,
    quirks: bool,
    origin: Origin,
}

#[derive(Default)]
struct Kept {
    sheets: FxHashMap<Key, Arc<Stylesheet>>,
    /// The keys, the most recently used last.
    order: VecDeque<Key>,
    bytes: usize,
}

/// Stylesheets parsed before, shared by the clones of an engine.
#[derive(Clone, Default)]
pub(crate) struct Sheets(Arc<Mutex<Kept>>);

impl Sheets {
    /// The stylesheet that `parse` makes of `css`, which then gets its imports from `import`:
    /// the one kept from an earlier document, or the one parsed now, which is kept unless it
    /// imports.
    pub(crate) fn get_or_parse(
        &self,
        css: &str,
        quirks: bool,
        origin: Origin,
        parse: impl FnOnce(&str) -> Stylesheet,
        import: &mut dyn FnMut(&str) -> Option<String>,
    ) -> Arc<Stylesheet> {
        let key = Key {
            css: Arc::from(css),
            quirks,
            origin,
        };
        {
            let mut kept = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(sheet) = kept.sheets.get(&key).cloned() {
                if let Some(position) = kept.order.iter().position(|known| *known == key) {
                    let known = kept.order.remove(position);
                    kept.order.extend(known);
                }
                return sheet;
            }
        }
        let sheet = parse(css);
        if sheet.has_imports() {
            return Arc::new(sheet.with_imports(import));
        }
        let sheet = Arc::new(sheet);
        let mut kept = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if kept.sheets.insert(key.clone(), Arc::clone(&sheet)).is_none() {
            kept.bytes += key.css.len();
            kept.order.push_back(key);
        }
        while kept.order.len() > MAX_SHEETS || (kept.bytes > MAX_BYTES && kept.order.len() > 1) {
            let Some(oldest) = kept.order.pop_front() else {
                break;
            };
            kept.bytes -= oldest.css.len();
            kept.sheets.remove(&oldest);
        }
        sheet
    }

    /// The number of stylesheets kept.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).sheets.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fagus_css::Origin as CssOrigin;

    fn parse(sheets: &Sheets, css: &str, origin: Origin, parsed: &mut usize) -> Arc<Stylesheet> {
        sheets.get_or_parse(
            css,
            false,
            origin,
            |css| {
                *parsed += 1;
                Stylesheet::parse(css, CssOrigin::Author, "test")
            },
            &mut |_| Some("p { color: red }".to_owned()),
        )
    }

    #[test]
    fn keeps_stylesheets_by_text_and_origin() {
        let sheets = Sheets::default();
        let mut parsed = 0;
        let given = || Origin::Given("app.css".to_owned());
        let first = parse(&sheets, "p { margin: 0 }", given(), &mut parsed);
        let again = parse(&sheets, "p { margin: 0 }", given(), &mut parsed);
        assert!(Arc::ptr_eq(&first, &again));
        assert_eq!(parsed, 1);
        parse(&sheets, "p { margin: 1px }", given(), &mut parsed);
        parse(
            &sheets,
            "p { margin: 0 }",
            Origin::Link("app.css".to_owned(), fagus_dom::NodeId::DOCUMENT),
            &mut parsed,
        );
        assert_eq!(parsed, 3);
        // In quirks mode the same text reads otherwise.
        sheets.get_or_parse(
            "p { margin: 0 }",
            true,
            given(),
            |css| {
                parsed += 1;
                Stylesheet::parse(css, CssOrigin::Author, "test")
            },
            &mut |_| None,
        );
        assert_eq!(parsed, 4);
        assert_eq!(sheets.len(), 4);
    }

    #[test]
    fn imports_again_for_each_document() {
        let sheets = Sheets::default();
        let mut parsed = 0;
        let css = "@import 'more.css'; p { margin: 0 }";
        for _ in 0..2 {
            let sheet = parse(&sheets, css, Origin::Given("app.css".to_owned()), &mut parsed);
            assert!(!sheet.has_imports());
            assert_eq!(sheet.len(), 2, "the imported rule and the own one");
        }
        assert_eq!(parsed, 2);
        assert_eq!(sheets.len(), 0);
    }

    #[test]
    fn keeps_the_stylesheets_used_last() {
        let sheets = Sheets::default();
        let mut parsed = 0;
        let css = |number: usize| format!("p {{ margin: {number}px }}");
        for number in 0..=MAX_SHEETS {
            parse(&sheets, &css(number), Origin::Given(String::new()), &mut parsed);
        }
        assert_eq!(sheets.len(), MAX_SHEETS);
        parse(
            &sheets,
            &css(MAX_SHEETS),
            Origin::Given(String::new()),
            &mut parsed,
        );
        assert_eq!(parsed, MAX_SHEETS + 1);
        parse(&sheets, &css(0), Origin::Given(String::new()), &mut parsed);
        assert_eq!(parsed, MAX_SHEETS + 2);
    }
}

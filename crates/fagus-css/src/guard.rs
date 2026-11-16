//! Guards against panics of the CSS parser.
//!
//! lightningcss panics on a few inputs (an `unreachable!()` where `calc()` of a plain number stands
//! for a percentage, as in `font-stretch: calc(5)` or a keyframe selector `calc(5)`). A document
//! must not stop rendering for that: [`guarded`] turns such a panic into `None`, and the parsers
//! here then leave out only the rules or declarations that make lightningcss panic, as browsers
//! leave out what they cannot parse.

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Once;

use lightningcss::stylesheet::{ParserOptions, StyleAttribute};
use lightningcss::traits::IntoOwned;

thread_local! {
    /// Whether this thread runs a guarded call, whose panics are expected and not reported.
    static QUIET: Cell<bool> = const { Cell::new(false) };
}

static HOOK: Once = Once::new();

/// Runs `parse`, and returns `None` if it panics. The panic is not printed: the panic hook (that
/// of the program, which this wraps once) is skipped while a guarded call runs on the thread.
pub fn guarded<T>(parse: impl FnOnce() -> T) -> Option<T> {
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !QUIET.with(Cell::get) {
                previous(info);
            }
        }));
    });
    let outer = QUIET.with(|quiet| quiet.replace(true));
    let result = catch_unwind(AssertUnwindSafe(parse));
    QUIET.with(|quiet| quiet.set(outer));
    result.ok()
}

/// Parses the declarations of a `style` attribute (or any declaration list), leaving out those
/// that make the parser panic. `None` if they do not parse at all.
pub fn parse_declarations(css: &str) -> Option<StyleAttribute<'static>> {
    let css = &*crate::flat_math(css);
    let parse = |css: &str| {
        guarded(|| {
            StyleAttribute::parse(css, ParserOptions::default())
                .ok()
                .map(|parsed| {
                    let mut parsed = parsed.into_owned();
                    crate::stylesheet::check_attribute(&mut parsed.declarations);
                    parsed
                })
        })
    };
    if let Some(parsed) = parse(css) {
        return parsed;
    }
    let kept: Vec<&str> = split_top_level(css)
        .into_iter()
        .filter(|declaration| parse(declaration).is_some())
        .collect();
    parse(&kept.join(";")).flatten()
}

/// Rewrites a stylesheet that makes the parser panic (`parses` says whether a piece of CSS parses
/// without panicking) so that it does not: top-level rules that panic are left out, and of a rule
/// with a block only the parts of the block (declarations and nested rules) that panic.
pub(crate) fn without_panics(css: &str, parses: &dyn Fn(&str) -> bool) -> String {
    let mut kept = String::with_capacity(css.len());
    for item in split_top_level(css) {
        if parses(item) {
            kept.push_str(item);
        } else if let Some((prelude, body)) = split_block(item) {
            kept.push_str(prelude);
            kept.push('{');
            for part in split_top_level(body) {
                if parses(&format!("{prelude}{{{part}}}")) {
                    kept.push_str(part);
                }
            }
            kept.push('}');
        }
        // A statement without a block that panics is left out.
    }
    kept
}

/// Splits CSS into its top-level items: statements and declarations end after a `;`, rules
/// after the `}` of their block. Strings, comments and escapes are skipped over.
fn split_top_level(css: &str) -> Vec<&str> {
    let bytes = css.as_bytes();
    let mut items = Vec::new();
    let mut depth = 0_usize;
    let mut start = 0;
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        match byte {
            b'\\' => index += 1,
            b'"' | b'\'' => {
                index += 1;
                while let Some(&inner) = bytes.get(index) {
                    match inner {
                        b'\\' => index += 1,
                        b'\n' => break,
                        _ if inner == byte => break,
                        _ => {}
                    }
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index = css[index + 2..]
                    .find("*/")
                    .map_or(bytes.len(), |end| index + 2 + end + 1);
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' => depth = depth.saturating_sub(1),
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    items.push(&css[start..=index]);
                    start = index + 1;
                }
            }
            b';' if depth == 0 => {
                items.push(&css[start..=index]);
                start = index + 1;
            }
            _ => {}
        }
        index += 1;
    }
    if start < css.len() {
        items.push(&css[start..]);
    }
    items
}

/// The prelude and the inside of the block of a rule (`prelude { body }`), if it has one.
fn split_block(item: &str) -> Option<(&str, &str)> {
    let open = item.find('{')?;
    let body = &item[open + 1..];
    let body = body.strip_suffix('}').unwrap_or(body);
    Some((&item[..open], body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guarded_turns_a_panic_into_none() {
        assert_eq!(guarded(|| 1), Some(1));
        assert_eq!(guarded(|| -> i32 { panic!("expected") }), None);
    }

    #[test]
    fn declarations_that_panic_are_left_out() {
        // `calc()` of a number where lightningcss expects a percentage reaches an
        // `unreachable!()` in its parser.
        let parsed = parse_declarations("color: red; font-stretch: calc(5); width: 3px").expect("parses");
        assert_eq!(parsed.declarations.declarations.len(), 2);
        let parsed = parse_declarations("font-stretch:calc(infinity").expect("parses");
        assert_eq!(parsed.declarations.declarations.len(), 0);
    }

    #[test]
    fn items_split_at_the_top_level() {
        assert_eq!(
            split_top_level("a { b: c; d: e } @import 'x;y'; f: g(h; i)"),
            vec!["a { b: c; d: e }", " @import 'x;y';", " f: g(h; i)"]
        );
    }
}

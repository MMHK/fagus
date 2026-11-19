//! Limits on what an SVG document can make the SVG parser (usvg, roxmltree) and renderer (resvg,
//! krilla-svg) do, checked before the document reaches them. usvg itself refuses documents of
//! more than a million elements (also those that `<use>` copies), but inflates SVGZ without a
//! limit, expands internal XML entities to as much text as they reference, and keeps the result
//! of every primitive of a filter until the filter ends.

use std::io::Read as _;

/// The most data an SVGZ document inflates to (as much as an SVG image may take uncompressed).
pub(crate) const MAX_INFLATED: usize = 128 << 20;

/// The most text that the internal entities of an SVG add where they are referenced: the
/// namespace entities of illustration programs take a few kilobytes.
pub(crate) const MAX_ENTITY_TEXT: usize = 16 << 20;

/// The most primitives of one `<filter>`. resvg keeps the image of every primitive (as large as
/// the filter's region) until the last one; real filters have a few dozen at most.
pub(crate) const MAX_PRIMITIVES: usize = 64;

/// The data of an SVGZ document (gzip-compressed SVG), unless it inflates to more than
/// [`MAX_INFLATED`] or is not gzip.
pub(crate) fn inflate(data: &[u8]) -> Option<Vec<u8>> {
    let mut inflated = Vec::new();
    flate2::read::GzDecoder::new(data)
        .take(MAX_INFLATED as u64 + 1)
        .read_to_end(&mut inflated)
        .ok()?;
    (inflated.len() <= MAX_INFLATED).then_some(inflated)
}

/// Whether an SVG document (not compressed) stays within the limits of its entities and filters.
pub(crate) fn acceptable(data: &[u8]) -> bool {
    let text = String::from_utf8_lossy(data);
    entities_within(&text) && primitives_within(&text)
}

/// Whether the internal entities of a document add at most [`MAX_ENTITY_TEXT`] where they are
/// referenced (counting every reference, also those in other entities), and none holds markup:
/// elements from entities would escape the count of [`primitives_within`].
fn entities_within(text: &str) -> bool {
    if !text.contains("<!ENTITY") {
        return true;
    }
    let mut values: rustc_hash::FxHashMap<&str, &str> = rustc_hash::FxHashMap::default();
    for declaration in text.split("<!ENTITY").skip(1) {
        let declaration = declaration.trim_start();
        // Parameter entities only serve the DTD.
        if declaration.starts_with('%') {
            continue;
        }
        let Some(name_end) = declaration.find(char::is_whitespace) else {
            continue;
        };
        let (name, rest) = declaration.split_at(name_end);
        let rest = rest.trim_start();
        // External entities (SYSTEM, PUBLIC) are not read.
        let Some(quote) = rest.chars().next().filter(|quote| matches!(quote, '"' | '\'')) else {
            continue;
        };
        let value = rest[1..].split(quote).next().unwrap_or_default();
        if value.contains('<') {
            return false;
        }
        // The first declaration of a name holds, as in XML.
        values.entry(name).or_insert(value);
    }
    let mut sizes = rustc_hash::FxHashMap::default();
    let added = references(text, &values)
        .map(|name| expanded(name, &values, &mut sizes, 0))
        .fold(0usize, usize::saturating_add);
    added <= MAX_ENTITY_TEXT
}

/// The names of the declared entities that `text` references, once per reference.
fn references<'t, 'v>(
    text: &'t str,
    values: &'v rustc_hash::FxHashMap<&str, &str>,
) -> impl Iterator<Item = &'t str> + use<'t, 'v> {
    text.split('&').skip(1).filter_map(|after| {
        let name = &after[..after.find(';')?];
        values.contains_key(name).then_some(name)
    })
}

/// The length of the text an entity expands to, with the entities it references expanded.
fn expanded<'a>(
    name: &'a str,
    values: &rustc_hash::FxHashMap<&'a str, &'a str>,
    sizes: &mut rustc_hash::FxHashMap<&'a str, usize>,
    depth: usize,
) -> usize {
    if let Some(&size) = sizes.get(name) {
        return size;
    }
    // Deeper nesting (and a cycle) is an error to the parser anyway.
    if depth > 16 {
        return usize::MAX;
    }
    let value = values[name];
    let size = references(value, values)
        .map(|inner| expanded(inner, values, sizes, depth + 1))
        .fold(value.len(), usize::saturating_add);
    sizes.insert(name, size);
    size
}

/// Whether every `<filter>` of a document has at most [`MAX_PRIMITIVES`] primitives (the
/// elements of light sources, transfer functions and merge nodes inside primitives not counted).
fn primitives_within(text: &str) -> bool {
    text.split("<filter").skip(1).all(|filter| {
        let filter = filter.split("</filter").next().unwrap_or_default();
        let primitives = filter
            .split("<fe")
            .skip(1)
            .filter(|element| {
                !["Func", "MergeNode", "DistantLight", "PointLight", "SpotLight"]
                    .iter()
                    .any(|part| element.starts_with(part))
            })
            .count();
        primitives <= MAX_PRIMITIVES
    })
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;
    use std::io::Write as _;

    use super::*;

    fn gzip(data: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        encoder.write_all(data).expect("compressed");
        encoder.finish().expect("compressed")
    }

    #[test]
    fn inflates_up_to_the_limit() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"/>"#;
        assert_eq!(inflate(&gzip(svg)).as_deref(), Some(&svg[..]));
        // About 130 KiB that inflate to more than the limit.
        let bomb = gzip(&vec![b' '; MAX_INFLATED + 1]);
        assert!(bomb.len() < 1 << 20);
        assert_eq!(inflate(&bomb), None);
        assert_eq!(inflate(b"<svg/>"), None, "not gzip");
    }

    #[test]
    fn counts_what_entities_expand_to() {
        // The namespace entities of Illustrator.
        let illustrator = r#"<!DOCTYPE svg [<!ENTITY ns_svg "http://www.w3.org/2000/svg"><!ENTITY ns_xlink "http://www.w3.org/1999/xlink">]><svg xmlns="&ns_svg;" xmlns:xlink="&ns_xlink;"/>"#;
        assert!(entities_within(illustrator));
        // Ten levels of ten references: 10^10 bytes from a few hundred.
        let mut laughs = String::from(r#"<!DOCTYPE svg [<!ENTITY l0 "lol">"#);
        for level in 1..10 {
            let previous = format!("&l{};", level - 1).repeat(10);
            write!(laughs, r#"<!ENTITY l{level} "{previous}">"#).expect("written");
        }
        laughs.push_str(r#"]><svg xmlns="http://www.w3.org/2000/svg"><text>&l9;</text></svg>"#);
        assert!(laughs.len() < 1000);
        assert!(!entities_within(&laughs));
        // Many references of one flat entity add up as well.
        let flat = format!(
            r#"<!DOCTYPE svg [<!ENTITY e "{}">]><svg><text>{}</text></svg>"#,
            "x".repeat(4096),
            "&e;".repeat(5000)
        );
        assert!(!entities_within(&flat));
        // A cycle is refused.
        let cycle = r#"<!DOCTYPE svg [<!ENTITY a "&b;"><!ENTITY b "&a;">]><svg><text>&a;</text></svg>"#;
        assert!(!entities_within(cycle));
        // Entities with markup are refused.
        let markup = r#"<!DOCTYPE svg [<!ENTITY f "<feFlood/>">]><svg><filter id="f">&f;</filter></svg>"#;
        assert!(!entities_within(markup));
    }

    #[test]
    fn counts_the_primitives_of_each_filter() {
        let filter = |primitives: usize| {
            format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg"><filter id="f">{}<feComponentTransfer><feFuncR type="identity"/><feFuncG type="identity"/></feComponentTransfer><feMerge><feMergeNode/><feMergeNode/></feMerge></filter><filter id="g"><feFlood/></filter></svg>"#,
                "<feFlood/>".repeat(primitives)
            )
        };
        assert!(primitives_within(&filter(MAX_PRIMITIVES - 2)));
        assert!(!primitives_within(&filter(MAX_PRIMITIVES - 1)));
        assert!(!primitives_within(&filter(1000)));
    }
}

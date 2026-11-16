//! Characters that no font has: they are drawn as the `.notdef` glyph of the first font, usually
//! an empty box. Outside PDF/A and PDF/UA, which refuse such documents, a warning
//! says which characters these are and where the first one is.

use fagus_css::{Diagnostic, Severity};
use fagus_dom::{Document, NodeId};
use fagus_layout::{Fragment, FragmentKind, Page};
use fagus_style::values::Visibility;
use fagus_text::FontDb;

/// How many characters the warning names.
const NAMED: usize = 8;

/// The characters without a glyph on the pages, in the order they appear.
#[derive(Default)]
struct Missing {
    characters: Vec<char>,
    /// How many glyphs stand for them.
    count: usize,
    /// The line of the element of the first one; 0 where unknown.
    line: u32,
}

/// The warning `font.missing-glyph` for the characters on `pages` that no font has a glyph for,
/// if there are any.
pub(crate) fn warning(document: &Document, pages: &[Page], fonts: &FontDb) -> Option<Diagnostic> {
    let mut missing = Missing::default();
    for page in pages {
        for fragment in &page.fragments {
            collect(document, fragment, None, fonts, &mut missing);
        }
    }
    let (first, rest) = missing.characters.split_first()?;
    let named = |character: &char| format!("`{character}` (U+{:04X})", u32::from(*character));
    let places = if missing.count > 1 {
        format!(" in {} places", missing.count)
    } else {
        String::new()
    };
    let message = if rest.is_empty() {
        format!(
            "no font has a glyph for {}{places}; it is drawn as an empty box (add a font that has it)",
            named(first)
        )
    } else {
        let mut list: Vec<String> = missing.characters.iter().take(NAMED).map(named).collect();
        if missing.characters.len() > NAMED {
            list.push(format!("and {} more", missing.characters.len() - NAMED));
        }
        format!(
            "no font has glyphs for {} characters{places}: {}; they are drawn as empty boxes (add \
             a font that has them)",
            missing.characters.len(),
            list.join(", ")
        )
    };
    Some(Diagnostic {
        severity: Severity::Warning,
        code: "font.missing-glyph",
        message,
        source: "html".to_owned(),
        line: missing.line,
        column: 0,
    })
}

/// Collects the characters of the `.notdef` glyphs in a fragment and its children; `node` is the
/// closest element around it.
fn collect(
    document: &Document,
    fragment: &Fragment,
    node: Option<NodeId>,
    fonts: &FontDb,
    missing: &mut Missing,
) {
    let node = match &fragment.kind {
        FragmentKind::Box { node: Some(node), .. } => Some(*node),
        _ => node,
    };
    if let FragmentKind::Text(run) = &fragment.kind
        && run.style.visibility == Visibility::Visible
        && run.symbol.is_none()
    {
        let face = fonts.face(run.face);
        for glyph in run.glyphs.iter().filter(|glyph| glyph.id == 0) {
            let cluster = run
                .text
                .get(glyph.cluster.0 as usize..glyph.cluster.1 as usize)
                .unwrap_or_default();
            let characters: Vec<char> = cluster
                .chars()
                .filter(|character| !invisible(*character) && !face.has_glyph(*character))
                .collect();
            if characters.is_empty() {
                continue;
            }
            if missing.count == 0 {
                missing.line = node
                    .and_then(|node| document.element(node))
                    .map_or(0, |element| element.line);
            }
            missing.count += 1;
            for character in characters {
                if !missing.characters.contains(&character) {
                    missing.characters.push(character);
                }
            }
        }
    }
    for child in &*fragment.children {
        collect(document, child, node, fonts, missing);
    }
}

/// Whether a character shows nothing of its own: white space, controls and the default
/// ignorable code points of Unicode, which shaping hides where the font has no glyph for them.
fn invisible(character: char) -> bool {
    character.is_whitespace()
        || character.is_control()
        || matches!(
            u32::from(character),
            0xAD | 0x34F
                | 0x61C
                | 0x115F..=0x1160
                | 0x17B4..=0x17B5
                | 0x180B..=0x180F
                | 0x200B..=0x200F
                | 0x202A..=0x202E
                | 0x2060..=0x206F
                | 0x3164
                | 0xFE00..=0xFE0F
                | 0xFEFF
                | 0xFFA0
                | 0xFFF0..=0xFFF8
                | 0x1BCA0..=0x1BCA3
                | 0x1D173..=0x1D17A
                | 0xE0000..=0xE0FFF
        )
}

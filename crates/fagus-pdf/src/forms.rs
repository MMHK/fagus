//! Interactive forms (`AcroForm`, PDF 1.7 §12.7): fields a reader fills in, with widget annotations
//! where the controls lie on the pages.
//!
//! krilla writes no fields, so each widget starts as a link annotation, which krilla puts where
//! it belongs on its page and, in a tagged PDF, into the structure tree. What a widget shows is
//! drawn on a page of its own after the document's pages, with the fonts and images krilla
//! embeds for the document. Once the PDF is written, each such page becomes a form `XObject`, the
//! appearance stream of its widget, and leaves the page tree; each link annotation becomes a
//! widget, and the document gets its fields. Every widget has its appearance, so viewers need
//! not draw one (`NeedAppearances` stays unset, as PDF/A requires).

use std::fmt::Write as _;
use std::sync::Arc;

use fagus_dom::NodeId;
use fagus_paint::Item;
use fagus_style::values::{Layer, Rgba};
use rustc_hash::FxHashMap;

use crate::update::{self, Update};

/// The scheme of the link annotations that stand for widgets until the PDF is written.
pub(crate) const SCHEME: &str = "fagus-field:";

/// A field of an interactive form.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    /// The fully qualified name: the names of the fields it lies in and its own, separated by
    /// periods. No two fields have the same name, and none has the name of another and more.
    pub name: String,
    pub kind: FieldKind,
    pub read_only: bool,
    pub required: bool,
    /// What the field is, for a reader who does not see it (`TU`).
    pub description: Option<String>,
    /// The size of the text the reader types in, in pixels, and its color.
    pub font_size: f32,
    pub color: Rgba,
    /// How the text lies across the field: 0 left, 1 centered, 2 right.
    pub alignment: u8,
    /// Where the field lies: one widget for each place, several for a group of radio buttons
    /// or a field repeated on pages.
    pub widgets: Vec<FieldWidget>,
}

/// What kind of field.
#[derive(Clone, Debug, PartialEq)]
pub enum FieldKind {
    /// A text field with its value.
    Text {
        value: Option<String>,
        multiline: bool,
        /// The value is not shown and not stored.
        password: bool,
        max_length: Option<u32>,
    },
    /// A checkbox, on where a widget is checked.
    Check,
    /// A group of radio buttons; one widget is on at most.
    Radio,
    /// A drop-down list: the options with their values and what they show, and the value
    /// chosen.
    Choice {
        options: Vec<(String, String)>,
        value: Option<String>,
    },
}

/// Where a field lies on a page and what it shows.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldWidget {
    pub page: usize,
    /// The area in CSS pixels from the page's top left corner: x, y, width and height.
    pub rect: [f32; 4],
    /// The size of the area [`FieldWidget::appearance`] is drawn in, in CSS pixels; larger than the
    /// area on the page where the content is shrunk.
    pub size: (f32, f32),
    /// The element of the control, for a tagged PDF.
    pub node: Option<NodeId>,
    /// What the widget shows: of a checkbox or radio button, how it looks when on.
    pub appearance: Vec<Item>,
    /// How a checkbox or radio button looks when off.
    pub off: Vec<Item>,
    /// The name of the state of a checkbox or radio button when on: its value.
    pub on: String,
    /// Whether a checkbox or radio button is on.
    pub checked: bool,
    /// The layer (`-fagus-layer`) the widget is on.
    pub layer: Option<Arc<Layer>>,
}

/// An appearance drawn on a page after the document's pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Appearance {
    pub(crate) field: usize,
    pub(crate) widget: usize,
    /// Whether it is how a checkbox or radio button looks when off.
    pub(crate) off: bool,
    /// How its page is turned on the sheet: the turn the appearance takes too.
    pub(crate) turn: Option<&'static str>,
}

/// The URI of the link annotation that stands for a widget.
pub(crate) fn placeholder(field: usize, widget: usize) -> String {
    format!("{SCHEME}{field}:{widget}")
}

/// The flags of a field (`Ff`).
fn flags(field: &Field) -> u32 {
    let mut flags = 0;
    if field.read_only {
        flags |= 1;
    }
    if field.required {
        flags |= 1 << 1;
    }
    match &field.kind {
        FieldKind::Text {
            multiline, password, ..
        } => {
            if *multiline {
                flags |= 1 << 12;
            }
            if *password {
                flags |= 1 << 13;
            }
        }
        FieldKind::Check => {}
        // Radio, and one stays on.
        FieldKind::Radio => flags |= (1 << 15) | (1 << 14),
        // A drop-down list.
        FieldKind::Choice { .. } => flags |= 1 << 17,
    }
    flags
}

/// The name of the state of a checkbox or radio button when on; never `Off`, the state off.
fn on_state(value: &str) -> String {
    let value = if value.is_empty() { "on" } else { value };
    if value == "Off" {
        update::name("Off_")
    } else {
        update::name(value)
    }
}

/// The entries of a field's dictionary: its kind, value, flags and what the reader types with.
fn field_entries(field: &Field, partial: &str, scale: f32) -> String {
    let mut entries = format!("/T {}", update::text(partial));
    if let Some(description) = field.description.as_deref().filter(|text| !text.is_empty()) {
        let _ = write!(entries, "/TU {}", update::text(description));
    }
    let flags = flags(field);
    if flags != 0 {
        let _ = write!(entries, "/Ff {flags}");
    }
    let text_entries = |entries: &mut String| {
        let [r, g, b] = [field.color.r, field.color.g, field.color.b]
            .map(|channel| update::number(channel.clamp(0.0, 1.0)));
        let size = update::number(field.font_size * crate::SCALE * scale);
        let _ = write!(entries, "/DA (/Helv {size} Tf {r} {g} {b} rg)");
        if field.alignment > 0 {
            let _ = write!(entries, "/Q {}", field.alignment.min(2));
        }
    };
    match &field.kind {
        FieldKind::Text {
            value,
            password,
            max_length,
            ..
        } => {
            entries.push_str("/FT/Tx");
            if let Some(value) = value.as_deref().filter(|_| !password) {
                let _ = write!(entries, "/V {0}/DV {0}", update::text(value));
            }
            if let Some(length) = max_length {
                let _ = write!(entries, "/MaxLen {length}");
            }
            text_entries(&mut entries);
        }
        FieldKind::Check | FieldKind::Radio => {
            entries.push_str("/FT/Btn");
            let value = field
                .widgets
                .iter()
                .find(|widget| widget.checked)
                .map_or_else(|| "/Off".to_owned(), |widget| on_state(&widget.on));
            let _ = write!(entries, "/V {value}/DV {value}");
        }
        FieldKind::Choice { options, value } => {
            entries.push_str("/FT/Ch/Opt[");
            for (export, shown) in options {
                if export == shown {
                    entries.push_str(&update::text(export));
                } else {
                    let _ = write!(entries, "[{}{}]", update::text(export), update::text(shown));
                }
            }
            entries.push(']');
            if let Some(value) = value {
                let _ = write!(entries, "/V {0}/DV {0}", update::text(value));
                if let Some(index) = options.iter().position(|(export, _)| export == value) {
                    let _ = write!(entries, "/I[{index}]");
                }
            }
            text_entries(&mut entries);
        }
    }
    entries
}

/// Makes the fields of the PDF: the pages after the first `pages` become the appearances of
/// `appearances`, the placeholders of the widgets become widgets, and the document gets its
/// form. `groups` are the objects of the layers, by name.
pub(crate) fn write(
    update: &mut Update<'_>,
    fields: &[Field],
    appearances: &[Appearance],
    pages: usize,
    groups: &FxHashMap<Arc<str>, u32>,
) -> Option<()> {
    if fields.is_empty() {
        return Some(());
    }
    let root = update.root()?;
    let catalog = update.dictionary(root)?;
    let tree = update::reference(crate::parse::entry(&catalog, "/Pages")?)?;
    let tree_dictionary = update.dictionary(tree)?;
    let kids = update::references(crate::parse::entry(&tree_dictionary, "/Kids")?);
    if kids.len() != pages + appearances.len() {
        return None;
    }

    // The appearances: form XObjects made of the content of their pages, which go.
    let mut streams: FxHashMap<(usize, usize, bool), u32> = FxHashMap::default();
    for (&page, appearance) in kids[pages..].iter().zip(appearances) {
        let dictionary = update.dictionary(page)?;
        let contents = update::reference(crate::parse::entry(&dictionary, "/Contents")?)?;
        let resources = crate::parse::entry(&dictionary, "/Resources")?.trim().to_owned();
        let media = crate::parse::entry(&dictionary, "/MediaBox")?.trim().to_owned();
        let mut stream = update.dictionary(contents)?;
        for (key, value) in [
            ("/Type", "/XObject"),
            ("/Subtype", "/Form"),
            ("/BBox", media.as_str()),
            ("/Resources", resources.as_str()),
        ] {
            stream = update::with_entry(&stream, key, value);
        }
        if let Some(group) = crate::parse::entry(&dictionary, "/Group") {
            stream = update::with_entry(&stream, "/Group", group.trim());
        }
        if let Some(turn) = appearance.turn {
            stream = update::with_entry(&stream, "/Matrix", turn);
        }
        update.set_dictionary(contents, &stream)?;
        update.set(page, b"null".to_vec());
        streams.insert((appearance.field, appearance.widget, appearance.off), contents);
    }
    let mut tree_dictionary = update::with_entry(
        &tree_dictionary,
        "/Kids",
        &format!(
            "[{}]",
            kids[..pages]
                .iter()
                .map(|kid| format!("{kid} 0 R"))
                .collect::<Vec<_>>()
                .join(" ")
        ),
    );
    tree_dictionary = update::with_entry(&tree_dictionary, "/Count", &pages.to_string());
    update.set(tree, tree_dictionary.into_bytes());

    // The placeholders of the widgets on the pages, with their pages.
    let mut placed: FxHashMap<(usize, usize), (u32, u32)> = FxHashMap::default();
    for &page in &kids[..pages] {
        let dictionary = update.dictionary(page)?;
        let Some(annotations) = crate::parse::entry(&dictionary, "/Annots") else {
            continue;
        };
        for annotation in update::references(annotations) {
            let Some(widget) = update
                .dictionary(annotation)
                .and_then(|dictionary| placeholder_of(&dictionary))
            else {
                continue;
            };
            placed.insert(widget, (annotation, page));
        }
    }

    // The fields, in a tree of the parts of their names.
    let mut top: Vec<u32> = Vec::new();
    let mut parents: FxHashMap<String, (u32, Vec<u32>)> = FxHashMap::default();
    let mut order: Vec<String> = Vec::new();
    for (index, field) in fields.iter().enumerate() {
        let widgets: Vec<(usize, &FieldWidget, u32, u32)> = field
            .widgets
            .iter()
            .enumerate()
            .filter_map(|(position, widget)| {
                let (annotation, page) = placed.get(&(index, position))?;
                Some((position, widget, *annotation, *page))
            })
            .collect();
        if widgets.is_empty() {
            continue;
        }
        let (path, partial) = match field.name.rsplit_once('.') {
            Some((path, partial)) => (Some(path), partial),
            None => (None, field.name.as_str()),
        };
        let scale = widgets.first().map_or(1.0, |(_, widget, ..)| scaled(widget));
        let entries = field_entries(field, partial, scale);
        let parent = path.map(|path| parent_of(update, &mut parents, &mut order, &mut top, path));
        let radio = matches!(field.kind, FieldKind::Radio);
        // A field with one widget is that widget; others hold their widgets.
        let (own, merged) = if widgets.len() == 1 && !radio {
            (widgets[0].2, true)
        } else {
            (update.reserve(), false)
        };
        let mut kids = Vec::new();
        for &(position, widget, annotation, page) in &widgets {
            let mut dictionary = update.dictionary(annotation)?;
            for key in ["/A", "/Border", "/Subtype", "/F", "/C", "/H"] {
                dictionary = update::without_entry(&dictionary, key);
            }
            let mut extra = format!("/Subtype/Widget/F 4/P {page} 0 R");
            let normal = streams.get(&(index, position, false));
            match &field.kind {
                FieldKind::Check | FieldKind::Radio => {
                    let on = on_state(&widget.on);
                    let off = streams.get(&(index, position, true));
                    let mut states = String::new();
                    if let Some(normal) = normal {
                        let _ = write!(states, "{on} {normal} 0 R");
                    }
                    if let Some(off) = off {
                        let _ = write!(states, "/Off {off} 0 R");
                    }
                    let _ = write!(extra, "/AP<</N<<{states}>>>>");
                    let _ = write!(extra, "/AS {}", if widget.checked { on.as_str() } else { "/Off" });
                }
                _ => {
                    if let Some(normal) = normal {
                        let _ = write!(extra, "/AP<</N {normal} 0 R>>");
                    }
                }
            }
            if let Some(group) = widget.layer.as_ref().and_then(|layer| groups.get(&layer.name)) {
                let _ = write!(extra, "/OC {group} 0 R");
            }
            if merged {
                extra.push_str(&entries);
                if let Some(parent) = parent {
                    let _ = write!(extra, "/Parent {parent} 0 R");
                }
            } else {
                let _ = write!(extra, "/Parent {own} 0 R");
            }
            let open = dictionary.trim_end().strip_suffix(">>")?.to_owned();
            update.set(annotation, format!("{open}{extra}>>").into_bytes());
            kids.push(annotation);
        }
        if !merged {
            let mut dictionary = format!(
                "<<{entries}/Kids[{}]",
                kids.iter()
                    .map(|kid| format!("{kid} 0 R"))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            if let Some(parent) = parent {
                let _ = write!(dictionary, "/Parent {parent} 0 R");
            }
            dictionary.push_str(">>");
            update.set(own, dictionary.into_bytes());
        }
        match path {
            Some(path) => parents.get_mut(path)?.1.push(own),
            None => top.push(own),
        }
    }
    // The fields that hold others, with their kids.
    for path in order {
        let (number, kids) = parents.get(&path)?;
        let partial = path
            .rsplit_once('.')
            .map_or(path.as_str(), |(_, partial)| partial);
        let mut dictionary = format!(
            "<</T {}/Kids[{}]",
            update::text(partial),
            kids.iter()
                .map(|kid| format!("{kid} 0 R"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        if let Some((outer, _)) = path.rsplit_once('.') {
            let _ = write!(dictionary, "/Parent {} 0 R", parents.get(outer)?.0);
        }
        dictionary.push_str(">>");
        update.set(*number, dictionary.into_bytes());
    }
    if top.is_empty() {
        return Some(());
    }

    // The form: its fields, and the font for what readers type in.
    let font =
        update.add(b"<</Type/Font/Subtype/Type1/BaseFont/Helvetica/Encoding/WinAnsiEncoding>>".to_vec());
    let form = format!(
        "<</Fields[{}]/DA(/Helv 0 Tf 0 g)/DR<</Font<</Helv {font} 0 R>>>>>>",
        top.iter()
            .map(|field| format!("{field} 0 R"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    let catalog = update.dictionary(root)?;
    update.set(
        root,
        update::with_entry(&catalog, "/AcroForm", &form).into_bytes(),
    );
    Some(())
}

/// The scale of a widget on its page: below 1 where the content is shrunk.
fn scaled(widget: &FieldWidget) -> f32 {
    if widget.size.0 > 0.0 {
        widget.rect[2] / widget.size.0
    } else {
        1.0
    }
}

/// The field (by its number) that holds the fields under `path`, made with those that hold it
/// where they are not yet there.
fn parent_of(
    update: &mut Update<'_>,
    parents: &mut FxHashMap<String, (u32, Vec<u32>)>,
    order: &mut Vec<String>,
    top: &mut Vec<u32>,
    path: &str,
) -> u32 {
    if let Some((number, _)) = parents.get(path) {
        return *number;
    }
    let number = update.reserve();
    parents.insert(path.to_owned(), (number, Vec::new()));
    order.push(path.to_owned());
    match path.rsplit_once('.') {
        Some((outer, _)) => {
            parent_of(update, parents, order, top, outer);
            if let Some((_, kids)) = parents.get_mut(outer) {
                kids.push(number);
            }
        }
        None => top.push(number),
    }
    number
}

/// The field and widget a placeholder annotation stands for.
fn placeholder_of(dictionary: &str) -> Option<(usize, usize)> {
    let action = crate::parse::entry(dictionary, "/A")?;
    let uri = crate::parse::entry(action.trim(), "/URI")?.trim();
    let uri = uri.strip_prefix('(')?.strip_suffix(')')?;
    let (field, widget) = uri.strip_prefix(SCHEME)?.split_once(':')?;
    Some((field.parse().ok()?, widget.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(kind: FieldKind) -> Field {
        Field {
            name: "customer.name".to_owned(),
            kind,
            read_only: false,
            required: true,
            description: Some("Name".to_owned()),
            font_size: 16.0,
            color: Rgba::BLACK,
            alignment: 0,
            widgets: Vec::new(),
        }
    }

    #[test]
    fn writes_the_entries_of_fields() {
        let text = field(FieldKind::Text {
            value: Some("Ada (L.)".to_owned()),
            multiline: true,
            password: false,
            max_length: Some(40),
        });
        assert_eq!(
            field_entries(&text, "name", 1.0),
            "/T (name)/TU (Name)/Ff 4098/FT/Tx/V (Ada \\(L.\\))/DV (Ada \\(L.\\))/MaxLen 40\
             /DA (/Helv 12 Tf 0 0 0 rg)"
        );
        let choice = field(FieldKind::Choice {
            options: vec![
                ("de".to_owned(), "Deutsch".to_owned()),
                ("en".to_owned(), "en".to_owned()),
            ],
            value: Some("en".to_owned()),
        });
        assert_eq!(
            field_entries(&choice, "language", 0.5),
            "/T (language)/TU (Name)/Ff 131074/FT/Ch/Opt[[(de)(Deutsch)](en)]/V (en)/DV (en)/I[1]\
             /DA (/Helv 6 Tf 0 0 0 rg)"
        );
        let password = field(FieldKind::Text {
            value: Some("secret".to_owned()),
            multiline: false,
            password: true,
            max_length: None,
        });
        assert!(!field_entries(&password, "pin", 1.0).contains("secret"));
    }

    #[test]
    fn names_the_states_of_buttons() {
        assert_eq!(on_state(""), "/on");
        assert_eq!(on_state("Off"), "/Off_");
        assert_eq!(on_state("Ja, gern"), "/Ja,#20gern");
        assert_eq!(
            placeholder_of("<</A<</S/URI/URI(fagus-field:3:1)>>/F 4>>"),
            Some((3, 1))
        );
        assert_eq!(placeholder_of("<</A<</S/URI/URI(https://x.dev/)>>>>"), None);
    }
}

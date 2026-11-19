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
    /// A drop-down list or a list box: the options with their values and what they show, and
    /// the options chosen, by their index.
    Choice {
        options: Vec<(String, String)>,
        selected: Vec<usize>,
        /// A list box, which shows several options at once, rather than a drop-down list.
        list: bool,
        /// Whether several options may be chosen (in a list box).
        multiple: bool,
        /// The option a list box shows first, scrolled to it.
        top: usize,
    },
    /// A push button, which submits the form or resets its fields; `caption` is its label.
    Button {
        action: ButtonAction,
        caption: Option<String>,
    },
    /// A signature field, empty until the PDF is signed in it.
    Signature,
}

/// What a push button does.
#[derive(Clone, Debug, PartialEq)]
pub enum ButtonAction {
    /// Sends the fields named in `fields` (fully qualified names) to `url`, as HTML sends a
    /// form encoded as `application/x-www-form-urlencoded`: in the URL where `get`, else in the
    /// body of a POST request.
    Submit {
        url: String,
        get: bool,
        fields: Vec<String>,
    },
    /// Sets the fields named back to their default values.
    Reset { fields: Vec<String> },
}

impl ButtonAction {
    /// The action dictionary.
    fn dictionary(&self) -> String {
        let names = |fields: &[String]| fields.iter().map(|name| update::text(name)).collect::<String>();
        match self {
            // Flags: `ExportFormat` (3), the HTML form format, and `GetMethod` (4).
            Self::Submit { url, get, fields } => format!(
                "<</S/SubmitForm/F<</FS/URL/F {}>>/Fields[{}]/Flags {}>>",
                update::text(url),
                names(fields),
                if *get { 4 | 8 } else { 4 }
            ),
            Self::Reset { fields } => format!("<</S/ResetForm/Fields[{}]>>", names(fields)),
        }
    }
}

/// Where a field lies on a page and what it shows.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldWidget {
    pub page: usize,
    /// The area in CSS pixels from the page's top left corner: x, y, width and height; for a
    /// control in a transformed box, the smallest rectangle around it.
    pub rect: [f32; 4],
    /// How the control's box is turned, skewed and scaled onto the page, `[a, b, c, d]` of a
    /// matrix in CSS pixels (`(x, y)` maps to `(a·x + c·y, b·x + d·y)`); `[s, 0, 0, s]` where
    /// it is only shrunk by `s`.
    pub transform: [f32; 4],
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
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Appearance {
    pub(crate) field: usize,
    pub(crate) widget: usize,
    /// Whether it is how a checkbox or radio button looks when off.
    pub(crate) off: bool,
    /// How the appearance is turned or skewed onto the sheet (`Matrix`, without translation, in
    /// PDF's coordinates): by the control's transform and its page's turn. `None` for upright.
    pub(crate) matrix: Option<[f32; 4]>,
}

/// The `Matrix` of a widget's appearance, from the transform of its control (in CSS pixels, see
/// [`FieldWidget::transform`]) and the turn of its page (in PDF's coordinates); `None` where the
/// widget is upright. Scaling is left out: the viewer fits the appearance into the widget's
/// rectangle.
pub(crate) fn matrix(transform: [f32; 4], turn: [f32; 4]) -> Option<[f32; 4]> {
    let [a, b, c, d] = transform;
    let scale = libm::sqrtf((a * d - b * c).abs());
    if scale <= 0.0 || !scale.is_finite() {
        return None;
    }
    // From y downwards to y upwards.
    let [a, b, c, d] = [a / scale, -b / scale, -c / scale, d / scale];
    let [ta, tb, tc, td] = turn;
    let matrix = [ta * a + tc * b, tb * a + td * b, ta * c + tc * d, tb * c + td * d];
    let upright = [1.0, 0.0, 0.0, 1.0];
    (!matrix
        .iter()
        .zip(upright)
        .all(|(value, upright)| (value - upright).abs() < 1e-4))
    .then_some(matrix)
}

/// The rotation of a widget (`R` of `MK`, counterclockwise in degrees) that a viewer gives what
/// it draws when the field is filled in: a quarter, a half or three quarters, where the
/// appearance is turned by one of those and not skewed; `None` for other turns, which viewers
/// cannot draw.
fn rotation(matrix: [f32; 4]) -> Option<u32> {
    let close = |a: f32, b: f32| (a - b).abs() < 1e-3;
    let [a, b, c, d] = matrix;
    // A rotation keeps its shape: `[cos, sin, -sin, cos]`.
    if !close(a, d) || !close(b, -c) || !close(a * a + b * b, 1.0) {
        return None;
    }
    [(1.0, 0.0, 0), (0.0, 1.0, 90), (-1.0, 0.0, 180), (0.0, -1.0, 270)]
        .into_iter()
        .find(|&(cos, sin, _)| close(a, cos) && close(b, sin))
        .map(|(.., degrees)| degrees)
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
        FieldKind::Choice { list, multiple, .. } => {
            // A drop-down list, or a list box of which several options may be chosen.
            if !list {
                flags |= 1 << 17;
            }
            if *multiple {
                flags |= 1 << 21;
            }
        }
        FieldKind::Button { .. } => flags |= 1 << 16,
        FieldKind::Signature => {}
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
        FieldKind::Choice {
            options,
            selected,
            top,
            ..
        } => {
            entries.push_str("/FT/Ch/Opt[");
            for (export, shown) in options {
                if export == shown {
                    entries.push_str(&update::text(export));
                } else {
                    let _ = write!(entries, "[{}{}]", update::text(export), update::text(shown));
                }
            }
            entries.push(']');
            let mut selected: Vec<usize> = selected
                .iter()
                .copied()
                .filter(|&index| index < options.len())
                .collect();
            selected.sort_unstable();
            selected.dedup();
            let values: Vec<String> = selected
                .iter()
                .map(|&index| update::text(&options[index].0))
                .collect();
            let value = match values.as_slice() {
                [] => None,
                [one] => Some(one.clone()),
                several => Some(format!("[{}]", several.concat())),
            };
            if let Some(value) = value {
                let _ = write!(entries, "/V {value}/DV {value}");
                let indices: Vec<String> = selected.iter().map(usize::to_string).collect();
                let _ = write!(entries, "/I[{}]", indices.join(" "));
            }
            if *top > 0 {
                let _ = write!(entries, "/TI {top}");
            }
            text_entries(&mut entries);
        }
        FieldKind::Button { .. } => entries.push_str("/FT/Btn"),
        FieldKind::Signature => entries.push_str("/FT/Sig"),
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
    let mut matrices: FxHashMap<(usize, usize), [f32; 4]> = FxHashMap::default();
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
        if let Some(matrix) = appearance.matrix {
            let [a, b, c, d] = matrix.map(update::number);
            stream = update::with_entry(&stream, "/Matrix", &format!("[{a} {b} {c} {d} 0 0]"));
            matrices.insert((appearance.field, appearance.widget), matrix);
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
            // How a viewer draws the widget anew: turned as it is, with the button's label.
            let mut characteristics = String::new();
            if let Some(degrees) = matrices
                .get(&(index, position))
                .and_then(|&matrix| rotation(matrix))
                .filter(|&degrees| degrees != 0)
            {
                let _ = write!(characteristics, "/R {degrees}");
            }
            if let FieldKind::Button { action, caption } = &field.kind {
                if let Some(caption) = caption.as_deref().filter(|caption| !caption.is_empty()) {
                    let _ = write!(characteristics, "/CA {}", update::text(caption));
                }
                let _ = write!(extra, "/A {}", action.dictionary());
            }
            if !characteristics.is_empty() {
                let _ = write!(extra, "/MK<<{characteristics}>>");
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

/// The scale of a widget's text on its page: below 1 where the content is shrunk.
fn scaled(widget: &FieldWidget) -> f32 {
    let [.., c, d] = widget.transform;
    let scale = libm::hypotf(c, d);
    if scale > 0.0 && scale.is_finite() {
        scale
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
            selected: vec![1],
            list: false,
            multiple: false,
            top: 0,
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

        let options: Vec<(String, String)> = ["a", "b", "c", "d"]
            .iter()
            .map(|option| ((*option).to_owned(), option.to_uppercase()))
            .collect();
        let list = field(FieldKind::Choice {
            options: options.clone(),
            selected: vec![3, 1],
            list: true,
            multiple: true,
            top: 2,
        });
        assert_eq!(
            field_entries(&list, "letters", 1.0),
            "/T (letters)/TU (Name)/Ff 2097154/FT/Ch/Opt[[(a)(A)][(b)(B)][(c)(C)][(d)(D)]]\
             /V [(b)(d)]/DV [(b)(d)]/I[1 3]/TI 2/DA (/Helv 12 Tf 0 0 0 rg)"
        );
        let single = field(FieldKind::Choice {
            options,
            selected: Vec::new(),
            list: true,
            multiple: false,
            top: 0,
        });
        assert_eq!(
            field_entries(&single, "letter", 1.0),
            "/T (letter)/TU (Name)/Ff 2/FT/Ch/Opt[[(a)(A)][(b)(B)][(c)(C)][(d)(D)]]\
             /DA (/Helv 12 Tf 0 0 0 rg)"
        );

        let submit = field(FieldKind::Button {
            action: ButtonAction::Submit {
                url: "https://example.com/apply?x=1".to_owned(),
                get: false,
                fields: vec!["customer.name".to_owned(), "ok".to_owned()],
            },
            caption: Some("Send".to_owned()),
        });
        assert_eq!(
            field_entries(&submit, "send", 1.0),
            "/T (send)/TU (Name)/Ff 65538/FT/Btn"
        );
        let FieldKind::Button { action, .. } = &submit.kind else {
            unreachable!()
        };
        assert_eq!(
            action.dictionary(),
            "<</S/SubmitForm/F<</FS/URL/F (https://example.com/apply?x=1)>>/Fields[(customer.name)(ok)]/Flags 4>>"
        );
        let get = ButtonAction::Submit {
            url: "https://example.com/".to_owned(),
            get: true,
            fields: Vec::new(),
        };
        assert!(get.dictionary().ends_with("/Fields[]/Flags 12>>"));
        assert_eq!(
            ButtonAction::Reset {
                fields: vec!["a".to_owned()]
            }
            .dictionary(),
            "<</S/ResetForm/Fields[(a)]>>"
        );
    }

    #[test]
    fn turns_appearances_with_their_controls_and_pages() {
        let upright = [1.0, 0.0, 0.0, 1.0];
        assert_eq!(matrix([1.0, 0.0, 0.0, 1.0], upright), None);
        assert_eq!(matrix([0.5, 0.0, 0.0, 0.5], upright), None, "only shrunk");
        // `rotate(90deg)` turns clockwise on the page: a quarter clockwise in PDF's coordinates.
        let quarter = matrix([0.0, 1.0, -1.0, 0.0], upright).expect("turned");
        assert_eq!(quarter, [0.0, -1.0, 1.0, 0.0]);
        assert_eq!(rotation(quarter), Some(270));
        // On a page turned to the left, the turns add up.
        assert_eq!(matrix([0.0, 1.0, -1.0, 0.0], [0.0, 1.0, -1.0, 0.0]), None);
        let left = matrix(upright, [0.0, 1.0, -1.0, 0.0]).expect("turned");
        assert_eq!(rotation(left), Some(90));
        assert_eq!(
            rotation(matrix([-2.0, 0.0, 0.0, -2.0], upright).expect("turned")),
            Some(180)
        );
        // 30° and skews have no rotation that viewers can draw.
        let (sin, cos) = (0.5, libm::sqrtf(3.0) / 2.0);
        let thirty = matrix([cos, sin, -sin, cos], upright).expect("turned");
        assert_eq!(rotation(thirty), None);
        assert_eq!(
            rotation(matrix([1.0, 0.0, 0.5, 1.0], upright).expect("skewed")),
            None
        );
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

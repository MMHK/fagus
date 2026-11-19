//! The fields of an interactive form, made of the form controls of a document
//! ([`crate::Document::forms`]).
//!
//! Text fields (`<input>` of the types that take text, numbers or dates), text areas,
//! checkboxes, radio buttons, drop-down lists, list boxes and the buttons that submit or reset
//! a form become fields, named after their `name`. Each lies where its control is laid out and
//! shows what the control shows; other buttons, files, sliders and the like stay as they are
//! printed, and so does every control in an `inert` element.

use std::sync::Arc;

use fagus_dom::{Document, ElementData, NodeId};
use fagus_layout::{Fragment, FragmentKind, Page};
use fagus_paint::{Item, Lift, Widget};
use fagus_pdf::{ButtonAction, Field, FieldKind, FieldWidget};
use fagus_style::StyledDocument;
use fagus_style::values::{Appearance, Direction, Display, TextAlign};
use rustc_hash::{FxHashMap, FxHashSet};

/// The `type`s of `<input>` that become text fields.
const TEXT_TYPES: &[&str] = &[
    "text",
    "search",
    "email",
    "url",
    "tel",
    "password",
    "number",
    "date",
    "time",
    "datetime-local",
    "month",
    "week",
];

/// What a widget of a control shows: when on (or always) and when off.
type States<'a> = dyn Fn(&Widget) -> (Vec<Item>, Vec<Item>) + 'a;

/// What kind of control an element is, for its field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Text,
    TextArea,
    Select,
    /// A `<select>` that shows several options at once (`multiple` or `size` above 1).
    ListBox,
    Check,
    Radio,
    /// A button that submits its form to the form's `action`.
    Submit,
    /// A button that resets the fields of its form.
    Reset,
}

/// The controls of a document that become fields.
#[derive(Debug, Default)]
pub(crate) struct Controls {
    /// What of each control its field shows.
    pub(crate) lifts: FxHashMap<NodeId, Lift>,
    /// The controls in document order, with their kinds.
    order: Vec<(NodeId, Kind)>,
}

/// The controls of `document` that become fields; the buttons that submit or reset a form only
/// with `buttons` (PDF/A forbids fields that act).
pub(crate) fn controls(document: &Document, styled: &StyledDocument, buttons: bool) -> Controls {
    let mut controls = Controls::default();
    let Some(root) = document.document_element() else {
        return controls;
    };
    let mut inert: FxHashSet<NodeId> = FxHashSet::default();
    for node in std::iter::once(root).chain(document.descendants(root)) {
        let Some(data) = document.element(node) else {
            continue;
        };
        let parent_inert = document
            .parent(node)
            .is_some_and(|parent| inert.contains(&parent));
        if parent_inert || data.attribute("inert").is_some() {
            inert.insert(node);
            continue;
        }
        let Some(kind) = kind(data) else {
            continue;
        };
        // A button does something only in a form, and is sent only where the PDF can send it.
        let usable = match kind {
            Kind::Submit => buttons && submission(document, node).is_some(),
            Kind::Reset => buttons && owner(document, node).is_some(),
            _ => true,
        };
        if !usable {
            continue;
        }
        if styled
            .style(node)
            .is_none_or(|style| style.display == Display::None)
        {
            continue;
        }
        // A list box shows which options are chosen, which its field draws anew.
        let lift = match kind {
            Kind::Check | Kind::Radio | Kind::ListBox => Lift::Whole,
            _ => Lift::Text,
        };
        controls.lifts.insert(node, lift);
        controls.order.push((node, kind));
    }
    controls
}

/// What kind of field an element becomes, if any.
fn kind(data: &ElementData) -> Option<Kind> {
    if data.is("textarea") {
        return Some(Kind::TextArea);
    }
    if data.is("select") {
        // A drop-down list or a list box; a drop-down list of which several options may be
        // chosen (`multiple` with `size=1`) stays as it is printed.
        let multiple = data.attribute("multiple").is_some();
        return match (list_size(data) > 1, multiple) {
            (true, _) => Some(Kind::ListBox),
            (false, false) => Some(Kind::Select),
            (false, true) => None,
        };
    }
    if data.is("button") {
        let kind = data
            .attribute("type")
            .map(|kind| kind.trim().to_ascii_lowercase());
        // A button without a valid `type` submits.
        return match kind.as_deref() {
            Some("reset") => Some(Kind::Reset),
            Some("button" | "menu") => None,
            _ => Some(Kind::Submit),
        };
    }
    if !data.is("input") {
        return None;
    }
    let kind = data
        .attribute("type")
        .map_or_else(|| "text".to_owned(), |kind| kind.trim().to_ascii_lowercase());
    match kind.as_str() {
        "checkbox" => Some(Kind::Check),
        "radio" => Some(Kind::Radio),
        "submit" => Some(Kind::Submit),
        "reset" => Some(Kind::Reset),
        kind if TEXT_TYPES.contains(&kind) => Some(Kind::Text),
        // An unknown type is a text field, as in HTML.
        "button" | "image" | "file" | "range" | "color" | "hidden" => None,
        _ => Some(Kind::Text),
    }
}

/// The number of options a `<select>` shows at once, as it is laid out: its `size`, or 4 with
/// `multiple` and 1 without.
fn list_size(data: &ElementData) -> u32 {
    data.attribute("size")
        .and_then(|size| size.trim().parse::<u32>().ok())
        .filter(|&size| size > 0)
        .unwrap_or(if data.attribute("multiple").is_some() {
            4
        } else {
            1
        })
}

/// The form a control belongs to: the one its `form` attribute names, else the one it is in.
fn owner(document: &Document, node: NodeId) -> Option<NodeId> {
    if let Some(id) = document.element(node).and_then(|data| data.attribute("form")) {
        let root = document.document_element()?;
        return std::iter::once(root)
            .chain(document.descendants(root))
            .find(|&candidate| {
                document
                    .element(candidate)
                    .is_some_and(|data| data.is("form") && data.id.as_deref() == Some(id))
            });
    }
    let mut current = document.parent(node);
    while let Some(parent) = current {
        if document.element(parent).is_some_and(|data| data.is("form")) {
            return Some(parent);
        }
        current = document.parent(parent);
    }
    None
}

/// Where a submit button sends its form, and whether with GET: the button's `formaction` and
/// `formmethod`, else the form's `action` and `method` (GET by default, as in HTML). `None`
/// without a form, for `method=dialog` and where the URL is relative or not one to send to.
fn submission(document: &Document, button: NodeId) -> Option<(String, bool)> {
    let form = document.element(owner(document, button)?)?;
    let data = document.element(button)?;
    let action = data
        .attribute("formaction")
        .or_else(|| form.attribute("action"))?;
    let method = data
        .attribute("formmethod")
        .or_else(|| form.attribute("method"))
        .unwrap_or("get")
        .trim()
        .to_ascii_lowercase();
    if method == "dialog" {
        return None;
    }
    Some((crate::interactive::form_action(action)?, method != "post"))
}

/// The fields of the controls, from the widgets painted for them.
pub(crate) fn fields(
    document: &Document,
    styled: &StyledDocument,
    controls: &Controls,
    widgets: Vec<Widget>,
    pages: &[Page],
) -> Vec<Field> {
    let mut by_node: FxHashMap<NodeId, Vec<Widget>> = FxHashMap::default();
    for widget in widgets {
        by_node.entry(widget.node).or_default().push(widget);
    }
    let marks = check_marks(controls, pages);
    let mut names = Names::default();
    let mut fields: Vec<Field> = Vec::new();
    // The field of each group of radio buttons, by name.
    let mut radios: FxHashMap<String, usize> = FxHashMap::default();
    let mut unnamed = 0;
    // The form of each field, to name the fields a button submits or resets.
    let mut forms: Vec<Option<NodeId>> = Vec::new();
    for &(node, kind) in &controls.order {
        let Some(painted) = by_node.remove(&node) else {
            continue;
        };
        let (Some(data), Some(style)) = (document.element(node), styled.style(node)) else {
            continue;
        };
        let given = data
            .attribute("name")
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .or_else(|| data.id.as_deref().map(str::trim).filter(|id| !id.is_empty()))
            .map(str::to_owned);
        let checked = fagus_css::checked(document, node);
        let on = data.attribute("value").unwrap_or("on").to_owned();
        let disabled = disabled(document, node);
        let make_widgets = |off: &States<'_>| -> Vec<FieldWidget> {
            painted
                .iter()
                .map(|widget| {
                    let (appearance, off) = off(widget);
                    FieldWidget {
                        page: widget.page,
                        rect: widget.rect,
                        transform: widget.transform,
                        size: widget.size,
                        clip: widget.clip,
                        node: Some(node),
                        appearance,
                        off,
                        on: on.clone(),
                        checked,
                        layer: widget.layer.clone(),
                    }
                })
                .collect()
        };
        let buttons = |widget: &Widget| {
            let other = toggled(
                &widget.items,
                marks.get(&node),
                style,
                kind,
                checked,
                disabled,
                widget,
            );
            if checked {
                (widget.items.clone(), other)
            } else {
                (other, widget.items.clone())
            }
        };
        if kind == Kind::Radio
            && let Some(name) = &given
            && let Some(&index) = radios.get(name)
        {
            let mut more = make_widgets(&buttons);
            fields[index].widgets.append(&mut more);
            fields[index].read_only &= disabled;
            fields[index].required |= data.attribute("required").is_some();
            continue;
        }
        let name = names.unique(given.clone().unwrap_or_else(|| {
            unnamed += 1;
            format!("field{unnamed}")
        }));
        if kind == Kind::Radio
            && let Some(given) = given
        {
            radios.insert(given, fields.len());
        }
        let caption = painted.first().map(|widget| {
            normalize(
                &widget
                    .items
                    .iter()
                    .filter_map(|item| match item {
                        Item::Text { text, .. } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<String>(),
            )
        });
        let button = |action: ButtonAction| FieldKind::Button {
            action,
            caption: caption.clone().filter(|caption| !caption.is_empty()),
        };
        let field_kind = match kind {
            Kind::Text => text_field(data),
            Kind::TextArea => FieldKind::Text {
                value: Some(document.text_content(node).replace("\r\n", "\n"))
                    .filter(|value| !value.is_empty()),
                multiline: true,
                password: false,
                max_length: max_length(data),
            },
            Kind::Select => choice(document, node),
            Kind::ListBox => list_box(document, node, data),
            Kind::Check => FieldKind::Check,
            Kind::Radio => FieldKind::Radio,
            Kind::Submit => {
                let Some((url, get)) = submission(document, node) else {
                    continue;
                };
                button(ButtonAction::Submit {
                    url,
                    get,
                    fields: Vec::new(),
                })
            }
            Kind::Reset => button(ButtonAction::Reset { fields: Vec::new() }),
        };
        let widgets = match kind {
            Kind::Check | Kind::Radio => make_widgets(&buttons),
            _ => make_widgets(&|widget: &Widget| (widget.items.clone(), Vec::new())),
        };
        let rtl = style.direction == Direction::Rtl;
        let alignment = match style.text_align {
            TextAlign::Center => 1,
            TextAlign::Right => 2,
            TextAlign::End if !rtl => 2,
            TextAlign::Start if rtl => 2,
            _ => 0,
        };
        let buttons = matches!(kind, Kind::Submit | Kind::Reset);
        let selects = matches!(kind, Kind::Select | Kind::ListBox);
        let description = if buttons {
            button_description(data, caption.as_deref())
        } else {
            description(document, node, data)
        };
        forms.push(owner(document, node));
        fields.push(Field {
            name,
            kind: field_kind,
            read_only: disabled || (!selects && !buttons && data.attribute("readonly").is_some()),
            required: !buttons && data.attribute("required").is_some(),
            description,
            font_size: style.font_size,
            color: style.color,
            alignment,
            widgets,
        });
    }
    // The fields a button submits or resets: the others of its form.
    let names: Vec<(Option<NodeId>, String)> = fields
        .iter()
        .zip(&forms)
        .filter(|(field, _)| !matches!(field.kind, FieldKind::Button { .. }))
        .map(|(field, form)| (*form, field.name.clone()))
        .collect();
    for (field, form) in fields.iter_mut().zip(&forms) {
        if let FieldKind::Button {
            action: ButtonAction::Submit { fields, .. } | ButtonAction::Reset { fields },
            ..
        } = &mut field.kind
        {
            *fields = names
                .iter()
                .filter(|(owner, _)| owner == form)
                .map(|(_, name)| name.clone())
                .collect();
        }
    }
    fields
}

/// What a button is to a reader who does not see it: its `aria-label`, its `title`, its label
/// or its name.
fn button_description(data: &ElementData, caption: Option<&str>) -> Option<String> {
    let attribute = |name: &str| {
        data.attribute(name)
            .map(normalize)
            .filter(|text| !text.is_empty())
    };
    attribute("aria-label")
        .or_else(|| attribute("title"))
        .or_else(|| caption.map(str::to_owned).filter(|text| !text.is_empty()))
        .or_else(|| attribute("name"))
}

/// The first element of `document` with the `id`, as `getElementById` finds it.
pub(crate) fn element_by_id(document: &Document, id: &str) -> Option<NodeId> {
    let root = document.document_element()?;
    std::iter::once(root)
        .chain(document.descendants(root))
        .find(|&node| {
            document
                .element(node)
                .is_some_and(|data| data.id.as_deref() == Some(id))
        })
}

/// The empty signature field of a visible signature, with its widgets: named `Signature1`, or
/// with the first number that no field of `fields` has.
pub(crate) fn signature_field(
    description: Option<String>,
    widgets: Vec<FieldWidget>,
    fields: &[Field],
) -> Field {
    let taken = |name: &str| {
        fields
            .iter()
            .any(|field| field.name == name || field.name.starts_with(&format!("{name}.")))
    };
    // One of the first numbers beyond the count of the fields is free.
    let name = (1..=fields.len() + 1)
        .map(|number| format!("Signature{number}"))
        .find(|name| !taken(name))
        .unwrap_or_default();
    Field {
        name,
        kind: FieldKind::Signature,
        read_only: false,
        required: false,
        description: description.or_else(|| Some("Signature".to_owned())),
        font_size: 0.0,
        color: fagus_style::values::Rgba::BLACK,
        alignment: 0,
        widgets,
    }
}

/// What the element of a signature field says it is, for a reader who does not see it: its
/// `aria-label` or `title`, else its text.
pub(crate) fn signature_description(document: &Document, node: NodeId) -> Option<String> {
    let data = document.element(node)?;
    data.attribute("aria-label")
        .or_else(|| data.attribute("title"))
        .map(normalize)
        .filter(|text| !text.is_empty())
        .or_else(|| Some(normalize(&document.text_content(node))).filter(|text| !text.is_empty()))
}

/// The widgets of a signature field from the widgets painted for its element.
pub(crate) fn signature_widgets(painted: Vec<Widget>) -> Vec<FieldWidget> {
    painted
        .into_iter()
        .map(|widget| FieldWidget {
            page: widget.page,
            rect: widget.rect,
            transform: widget.transform,
            size: widget.size,
            clip: widget.clip,
            node: Some(widget.node),
            appearance: widget.items,
            off: Vec::new(),
            on: String::new(),
            checked: false,
            layer: widget.layer,
        })
        .collect()
}

/// The field of an `<input>` that takes text, with the value Chrome shows.
fn text_field(data: &ElementData) -> FieldKind {
    let kind = data
        .attribute("type")
        .map_or_else(|| "text".to_owned(), |kind| kind.trim().to_ascii_lowercase());
    let value = data
        .attribute("value")
        .map(|value| value.replace(['\r', '\n'], ""));
    let value = match kind.as_str() {
        "number" => value.filter(|value| value.trim().parse::<f64>().is_ok_and(f64::is_finite)),
        "email" | "url" => value.map(|value| value.trim().to_owned()),
        _ => value,
    }
    .filter(|value| !value.is_empty());
    FieldKind::Text {
        value,
        multiline: false,
        password: kind == "password",
        max_length: max_length(data),
    }
}

/// `maxlength`, if it is a number.
fn max_length(data: &ElementData) -> Option<u32> {
    data.attribute("maxlength")?.trim().parse().ok()
}

/// The options of a `<select>` with their values and what they show, and which have
/// `selected`.
fn options(document: &Document, select: NodeId) -> Vec<((String, String), bool, bool)> {
    let mut options = Vec::new();
    for node in document.descendants(select) {
        let Some(option) = document.element(node).filter(|data| data.is("option")) else {
            continue;
        };
        let text = normalize(&document.text_content(node));
        let shown = option
            .attribute("label")
            .map(normalize)
            .filter(|label| !label.is_empty())
            .unwrap_or_else(|| text.clone());
        let value = option.attribute("value").map_or(text, str::to_owned);
        options.push((
            (value, shown),
            option.attribute("selected").is_some(),
            option.attribute("disabled").is_some(),
        ));
    }
    options
}

/// The drop-down list of a `<select>`, with the option chosen: the last with `selected`, else
/// the first that is not disabled.
fn choice(document: &Document, select: NodeId) -> FieldKind {
    let options = options(document, select);
    let chosen = options
        .iter()
        .rposition(|(_, selected, _)| *selected)
        .or_else(|| options.iter().position(|(_, _, disabled)| !disabled));
    FieldKind::Choice {
        options: options.into_iter().map(|(option, ..)| option).collect(),
        selected: chosen.into_iter().collect(),
        list: false,
        multiple: false,
        top: 0,
    }
}

/// The list box of a `<select>`: with `multiple`, every option with `selected` is chosen,
/// without it the last; none if none has it. It shows the first chosen option at its bottom
/// where that is further down than it reaches, as Chrome scrolls it.
fn list_box(document: &Document, select: NodeId, data: &ElementData) -> FieldKind {
    let options = options(document, select);
    let multiple = data.attribute("multiple").is_some();
    let selected: Vec<usize> = if multiple {
        options
            .iter()
            .enumerate()
            .filter_map(|(index, (_, selected, _))| selected.then_some(index))
            .collect()
    } else {
        options
            .iter()
            .rposition(|(_, selected, _)| *selected)
            .into_iter()
            .collect()
    };
    let size = list_size(data) as usize;
    let top = selected
        .first()
        .map_or(0, |&first| (first + 1).saturating_sub(size));
    FieldKind::Choice {
        options: options.into_iter().map(|(option, ..)| option).collect(),
        selected,
        list: true,
        multiple,
        top,
    }
}

/// Whether a control is disabled: by its attribute, or in a disabled `<fieldset>` (but not in
/// its first `<legend>`).
fn disabled(document: &Document, node: NodeId) -> bool {
    if document
        .element(node)
        .is_some_and(|data| data.attribute("disabled").is_some())
    {
        return true;
    }
    let mut child = node;
    while let Some(parent) = document.parent(child) {
        if let Some(data) = document.element(parent)
            && data.is("fieldset")
            && data.attribute("disabled").is_some()
        {
            let legend = document
                .children(parent)
                .find(|&kid| document.element(kid).is_some_and(|data| data.is("legend")));
            if legend != Some(child) {
                return true;
            }
        }
        child = parent;
    }
    false
}

/// What a control is to a reader who does not see it: its `aria-label`, its `title`, the text
/// of its `<label>`, its placeholder or its name.
fn description(document: &Document, node: NodeId, data: &ElementData) -> Option<String> {
    let attribute = |name: &str| {
        data.attribute(name)
            .map(normalize)
            .filter(|text| !text.is_empty())
    };
    attribute("aria-label")
        .or_else(|| attribute("title"))
        .or_else(|| label(document, node, data))
        .or_else(|| attribute("placeholder"))
        .or_else(|| attribute("name"))
}

/// The text of the `<label>` of a control: the one whose `for` names it, else the one around it,
/// without the text of controls in it.
fn label(document: &Document, node: NodeId, data: &ElementData) -> Option<String> {
    let root = document.document_element()?;
    let by_id = data.id.as_deref().and_then(|id| {
        document.descendants(root).find(|&candidate| {
            document
                .element(candidate)
                .is_some_and(|element| element.is("label") && element.attribute("for") == Some(id))
        })
    });
    let around = || {
        let mut current = document.parent(node);
        while let Some(parent) = current {
            if document
                .element(parent)
                .is_some_and(|element| element.is("label"))
            {
                return Some(parent);
            }
            current = document.parent(parent);
        }
        None
    };
    let label = by_id.or_else(around)?;
    let mut text = String::new();
    let mut skipped: FxHashSet<NodeId> = FxHashSet::default();
    for descendant in document.descendants(label) {
        let parent_skipped = document
            .parent(descendant)
            .is_some_and(|parent| skipped.contains(&parent));
        let control = document
            .element(descendant)
            .is_some_and(|element| element.is("select") || element.is("textarea") || element.is("script"));
        if parent_skipped || control {
            skipped.insert(descendant);
            continue;
        }
        if document.element(descendant).is_none() {
            text.push_str(&document.text_content(descendant));
        }
    }
    Some(normalize(&text)).filter(|text| !text.is_empty())
}

/// Text with white space collapsed.
fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The image each checkbox and radio button with its native look shows, with the size of its
/// content box, by the control's element.
fn check_marks(
    controls: &Controls,
    pages: &[Page],
) -> FxHashMap<NodeId, (Arc<fagus_layout::Image>, (f32, f32))> {
    fn walk(
        fragment: &Fragment,
        controls: &Controls,
        marks: &mut FxHashMap<NodeId, (Arc<fagus_layout::Image>, (f32, f32))>,
    ) {
        if let FragmentKind::Box {
            node: Some(node),
            style,
            ..
        } = &fragment.kind
            && controls.lifts.get(node) == Some(&Lift::Whole)
            && style.appearance != Appearance::None
        {
            for child in &fragment.children {
                if let FragmentKind::Image(image) = &child.kind {
                    marks.entry(*node).or_insert_with(|| {
                        (
                            Arc::clone(image),
                            (child.rect.width.to_px(), child.rect.height.to_px()),
                        )
                    });
                }
            }
            return;
        }
        for child in &fragment.children {
            walk(child, controls, marks);
        }
    }
    let mut marks = FxHashMap::default();
    if !controls.lifts.values().any(|lift| *lift == Lift::Whole) {
        return marks;
    }
    for page in pages {
        for fragment in &page.fragments {
            walk(fragment, controls, &mut marks);
        }
    }
    marks
}

/// What a checkbox or radio button shows in the state it is not in: its mark in that state
/// where it has its native look, else a mark of the native look over its box where that state
/// is on, and nothing where it is off.
fn toggled(
    items: &[Item],
    mark: Option<&(Arc<fagus_layout::Image>, (f32, f32))>,
    style: &fagus_style::ComputedStyle,
    kind: Kind,
    checked: bool,
    disabled: bool,
    widget: &Widget,
) -> Vec<Item> {
    let radio = kind == Kind::Radio;
    if let Some((image, size)) = mark {
        let other = fagus_layout::control_mark(style, radio, !checked, disabled, *size);
        return items
            .iter()
            .map(|item| match item {
                Item::Image { rect, image: shown } if Arc::ptr_eq(shown, image) => Item::Image {
                    rect: *rect,
                    image: Arc::clone(&other),
                },
                item => item.clone(),
            })
            .collect();
    }
    if checked {
        return Vec::new();
    }
    let (width, height) = widget.size;
    let side = width.min(height);
    vec![Item::Image {
        rect: fagus_paint::RectF {
            x: (width - side) / 2.0,
            y: (height - side) / 2.0,
            width: side,
            height: side,
        },
        image: fagus_layout::control_mark(style, radio, true, disabled, (side, side)),
    }]
}

/// Field names, unique and such that none is the name of another with more after a period.
#[derive(Default)]
struct Names {
    /// The names given.
    taken: FxHashSet<String>,
    /// The names of the fields that hold others: the parts of the names given before a period.
    holders: FxHashSet<String>,
}

impl Names {
    /// `wanted`, or a name made of it that is free: without periods where a part of it is
    /// empty or it clashes with the fields that hold others, and with `_2`, `_3`, … where it is
    /// taken.
    fn unique(&mut self, wanted: String) -> String {
        let fits = |names: &Self, name: &str| {
            !names.taken.contains(name)
                && !names.holders.contains(name)
                && !holders(name).any(|holder| names.taken.contains(holder))
        };
        // Periods go where a part is empty or the name clashes with the tree of fields.
        let clashes = |names: &Self, name: &str| {
            names.holders.contains(name) || holders(name).any(|holder| names.taken.contains(holder))
        };
        let mut base = wanted;
        if base.split('.').any(str::is_empty) || clashes(self, &base) {
            base = base.replace('.', "_");
        }
        let mut name = base.clone();
        let mut count = 1;
        while !fits(self, &name) {
            count += 1;
            name = format!("{base}_{count}");
        }
        for holder in holders(&name) {
            self.holders.insert(holder.to_owned());
        }
        self.taken.insert(name.clone());
        name
    }
}

/// The names of the fields that hold a field of `name`: `a` and `a.b` for `a.b.c`.
fn holders(name: &str) -> impl Iterator<Item = &str> {
    name.match_indices('.').map(|(index, _)| &name[..index])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_names_unique_and_apart() {
        let mut names = Names::default();
        assert_eq!(names.unique("customer.name".to_owned()), "customer.name");
        assert_eq!(names.unique("customer.name".to_owned()), "customer.name_2");
        // `customer` holds fields already; `customer.name.first` would hold one.
        assert_eq!(names.unique("customer".to_owned()), "customer_2");
        assert_eq!(
            names.unique("customer.name.first".to_owned()),
            "customer_name_first"
        );
        assert_eq!(names.unique("a..b".to_owned()), "a__b");
        assert_eq!(names.unique("items[]".to_owned()), "items[]");
        assert_eq!(names.unique("items[]".to_owned()), "items[]_2");
    }

    #[test]
    fn finds_the_controls_that_become_fields() {
        let document = Document::parse_html(
            "<!DOCTYPE html><form><input name=a><input type=submit><input type=checkbox name=b>\
             <select name=c><option>x</select><select multiple name=d></select><textarea name=e></textarea>\
             <input type=hidden name=f><input type=range name=g><input type=date name=h>\
             <div inert><input name=i></div><input name=j style='display: none'></form>",
        );
        let user_agent = fagus_css::Stylesheet::parse(
            fagus_style::USER_AGENT_CSS,
            fagus_css::Origin::UserAgent,
            "ua.css",
        );
        let author = fagus_css::Stylesheet::parse("", fagus_css::Origin::Author, "doc.css");
        let rules = fagus_css::RuleSet::new(&[&user_agent, &author], &fagus_css::MediaEnvironment::default());
        let styled = fagus_style::compute_styles(&document, &rules, (300.0, 300.0));
        let found = controls(&document, &styled, true);
        let names: Vec<&str> = found
            .order
            .iter()
            .filter_map(|(node, _)| document.element(*node)?.attribute("name"))
            .collect();
        assert_eq!(names, ["a", "b", "c", "d", "e", "h"]);
        assert_eq!(found.order[3].1, Kind::ListBox);
        assert_eq!(found.order[1].1, Kind::Check);
        assert_eq!(found.lifts[&found.order[1].0], Lift::Whole);
    }

    #[test]
    fn chooses_options_and_labels_as_browsers() {
        let document = Document::parse_html(
            "<!DOCTYPE html><label for=l>Country</label>\
             <select id=l><option value=de>Germany<option disabled>-<option selected value=fr>France</select>\
             <label>Notes <textarea id=t>ignored</textarea></label>\
             <fieldset disabled><legend><input id=in></legend><input id=out></fieldset>",
        );
        let select = document.find("select").expect("select");
        assert_eq!(
            choice(&document, select),
            FieldKind::Choice {
                options: vec![
                    ("de".to_owned(), "Germany".to_owned()),
                    ("-".to_owned(), "-".to_owned()),
                    ("fr".to_owned(), "France".to_owned()),
                ],
                selected: vec![2],
                list: false,
                multiple: false,
                top: 0,
            }
        );
        let element = |id: &str| {
            let root = document.document_element().expect("root");
            document
                .descendants(root)
                .find(|&node| {
                    document
                        .element(node)
                        .is_some_and(|data| data.id.as_deref() == Some(id))
                })
                .expect("element")
        };
        let data = |id: &str| document.element(element(id)).expect("data");
        assert_eq!(
            description(&document, select, data("l")).as_deref(),
            Some("Country")
        );
        assert_eq!(
            description(&document, element("t"), data("t")).as_deref(),
            Some("Notes")
        );
        assert!(!disabled(&document, element("in")));
        assert!(disabled(&document, element("out")));
    }
}

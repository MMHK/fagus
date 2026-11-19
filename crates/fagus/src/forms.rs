//! The fields of an interactive form, made of the form controls of a document
//! ([`crate::Document::forms`]).
//!
//! Text fields (`<input>` of the types that take text, numbers or dates), text areas,
//! checkboxes, radio buttons and drop-down lists become fields, named after their `name`. Each
//! lies where its control is laid out and shows what the control shows; buttons, files, sliders
//! and the like stay as they are printed, and so does every control in an `inert` element.

use std::sync::Arc;

use fagus_dom::{Document, ElementData, NodeId};
use fagus_layout::{Fragment, FragmentKind, Page};
use fagus_paint::{Item, Lift, Widget};
use fagus_pdf::{Field, FieldKind, FieldWidget};
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
    Check,
    Radio,
}

/// The controls of a document that become fields.
#[derive(Debug, Default)]
pub(crate) struct Controls {
    /// What of each control its field shows.
    pub(crate) lifts: FxHashMap<NodeId, Lift>,
    /// The controls in document order, with their kinds.
    order: Vec<(NodeId, Kind)>,
}

/// The controls of `document` that become fields.
pub(crate) fn controls(document: &Document, styled: &StyledDocument) -> Controls {
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
        if styled
            .style(node)
            .is_none_or(|style| style.display == Display::None)
        {
            continue;
        }
        let lift = match kind {
            Kind::Check | Kind::Radio => Lift::Whole,
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
        // A drop-down list; list boxes stay as they are printed.
        let size = data
            .attribute("size")
            .and_then(|size| size.trim().parse::<u32>().ok())
            .unwrap_or(0);
        return (data.attribute("multiple").is_none() && size <= 1).then_some(Kind::Select);
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
        kind if TEXT_TYPES.contains(&kind) => Some(Kind::Text),
        // An unknown type is a text field, as in HTML.
        "submit" | "reset" | "button" | "image" | "file" | "range" | "color" | "hidden" => None,
        _ => Some(Kind::Text),
    }
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
                        size: widget.size,
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
            Kind::Check => FieldKind::Check,
            Kind::Radio => FieldKind::Radio,
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
        fields.push(Field {
            name,
            kind: field_kind,
            read_only: disabled || (kind != Kind::Select && data.attribute("readonly").is_some()),
            required: data.attribute("required").is_some(),
            description: description(document, node, data),
            font_size: style.font_size,
            color: style.color,
            alignment,
            widgets,
        });
    }
    fields
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

/// The options of a `<select>` and the one chosen: the last with `selected`, else the first
/// that is not disabled.
fn choice(document: &Document, select: NodeId) -> FieldKind {
    let mut options = Vec::new();
    let mut chosen = None;
    let mut first = None;
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
        if option.attribute("selected").is_some() {
            chosen = Some(value.clone());
        }
        if first.is_none() && option.attribute("disabled").is_none() {
            first = Some(value.clone());
        }
        options.push((value, shown));
    }
    FieldKind::Choice {
        options,
        value: chosen.or(first),
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
        let found = controls(&document, &styled);
        let names: Vec<&str> = found
            .order
            .iter()
            .filter_map(|(node, _)| document.element(*node)?.attribute("name"))
            .collect();
        assert_eq!(names, ["a", "b", "c", "e", "h"]);
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
                value: Some("fr".to_owned()),
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

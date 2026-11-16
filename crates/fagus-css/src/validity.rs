//! Constraint validation of form controls (HTML §4.10.20) for `:valid`, `:invalid`,
//! `:in-range`, `:out-of-range` and `:indeterminate`, as a document prints before anyone edits
//! it: from the values, states and constraints in its attributes. Constraints that only user
//! edits break (`maxlength`, `minlength`) are left out, as Chrome leaves them out.

use fagus_dom::{Document, ElementData, NodeId};

use crate::pattern::Pattern;

/// The type of an `<input>`, lowercase; `text` where it is missing or unknown.
fn input_type(data: &ElementData) -> String {
    let kind = data
        .attribute("type")
        .unwrap_or("text")
        .trim()
        .to_ascii_lowercase();
    match kind.as_str() {
        "hidden" | "text" | "search" | "tel" | "url" | "email" | "password" | "date" | "month" | "week"
        | "time" | "datetime-local" | "number" | "range" | "color" | "checkbox" | "radio" | "file"
        | "submit" | "image" | "reset" | "button" => kind,
        _ => "text".to_owned(),
    }
}

/// The element ancestors of `element`, nearest first.
fn ancestors(document: &Document, element: NodeId) -> impl Iterator<Item = NodeId> + '_ {
    std::iter::successors(document.parent_element(element), |&node| {
        document.parent_element(node)
    })
}

/// The types whose text `readonly` and `pattern` and `required` apply to.
fn is_text_like(kind: &str) -> bool {
    matches!(kind, "text" | "search" | "tel" | "url" | "email" | "password")
}

fn is_date_like(kind: &str) -> bool {
    matches!(kind, "date" | "month" | "week" | "time" | "datetime-local")
}

/// Whether `element` is disabled: by its own `disabled`, or inside a disabled `<fieldset>`
/// but not inside its first `<legend>`.
fn disabled(document: &Document, element: NodeId) -> bool {
    if document
        .element(element)
        .is_some_and(|data| data.attribute("disabled").is_some())
    {
        return true;
    }
    let mut child = element;
    let mut current = document.parent_element(element);
    while let Some(ancestor) = current {
        if let Some(data) = document.element(ancestor)
            && data.is("fieldset")
            && data.attribute("disabled").is_some()
        {
            let first_legend = document
                .child_elements(ancestor)
                .find(|&node| document.element(node).is_some_and(|data| data.is("legend")));
            if first_legend != Some(child) {
                return true;
            }
        }
        child = ancestor;
        current = document.parent_element(ancestor);
    }
    false
}

/// Whether `element` is a candidate for constraint validation: a submittable control that is
/// not barred from it (HTML §4.10.20.1).
fn candidate(document: &Document, element: NodeId) -> bool {
    let Some(data) = document.element(element) else {
        return false;
    };
    let listed = if data.is("input") {
        let kind = input_type(data);
        !matches!(kind.as_str(), "hidden" | "reset" | "button")
            && !(data.attribute("readonly").is_some()
                && (is_text_like(&kind) || is_date_like(&kind) || kind == "number"))
    } else if data.is("button") {
        !matches!(
            data.attribute("type").map(str::to_ascii_lowercase).as_deref(),
            Some("reset" | "button")
        )
    } else if data.is("textarea") {
        data.attribute("readonly").is_none()
    } else {
        data.is("select")
    };
    listed
        && !disabled(document, element)
        && !ancestors(document, element)
            .any(|node| document.element(node).is_some_and(|data| data.is("datalist")))
}

/// The value of an `<input>` after its type sanitizes it (HTML §4.10.5.1).
fn input_value(data: &ElementData, kind: &str) -> String {
    let raw: String = data
        .attribute("value")
        .unwrap_or_default()
        .chars()
        .filter(|&c| c != '\n' && c != '\r')
        .collect();
    match kind {
        "email" | "url" => raw.trim_matches(is_html_space).to_owned(),
        "number" => {
            if parse_number(&raw).is_some() {
                raw
            } else {
                String::new()
            }
        }
        _ => raw,
    }
}

fn is_html_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{c}' | '\r')
}

/// A valid floating-point number (HTML §2.3.4.3).
fn parse_number(text: &str) -> Option<f64> {
    let valid = !text.is_empty()
        && !text.starts_with('+')
        && !text.starts_with('.')
        && !text.starts_with("-.")
        && !text.ends_with('.')
        && text
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '-' | '.' | 'e' | 'E' | '+'));
    valid
        .then(|| text.parse::<f64>().ok())
        .flatten()
        .filter(|value| value.is_finite())
}

/// A date, time or the like as a number that orders them: `None` where it is not one.
fn parse_temporal(kind: &str, text: &str) -> Option<f64> {
    let digits = |part: &str, length: usize| -> Option<f64> {
        (part.len() == length && part.chars().all(|c| c.is_ascii_digit()))
            .then(|| part.parse::<f64>().ok())
            .flatten()
    };
    let date = |text: &str| -> Option<f64> {
        let mut parts = text.split('-');
        let (year, month, day) = (parts.next()?, parts.next()?, parts.next()?);
        let (month, day) = (digits(month, 2)?, digits(day, 2)?);
        let year = (year.len() >= 4 && year.chars().all(|c| c.is_ascii_digit()))
            .then(|| year.parse::<f64>().ok())
            .flatten()?;
        ((1.0..=12.0).contains(&month) && (1.0..=31.0).contains(&day) && parts.next().is_none())
            .then_some((year * 12.0 + month) * 31.0 + day)
    };
    let time = |text: &str| -> Option<f64> {
        let mut parts = text.splitn(3, ':');
        let (hours, minutes) = (digits(parts.next()?, 2)?, digits(parts.next()?, 2)?);
        let seconds = match parts.next() {
            Some(seconds) => {
                let (whole, fraction) = seconds.split_once('.').unwrap_or((seconds, ""));
                digits(whole, 2)?
                    + if fraction.is_empty() {
                        0.0
                    } else {
                        format!("0.{fraction}").parse::<f64>().ok()?
                    }
            }
            None => 0.0,
        };
        (hours < 24.0 && minutes < 60.0 && seconds < 60.0)
            .then_some((hours * 60.0 + minutes) * 60.0 + seconds)
    };
    match kind {
        "date" => date(text),
        "month" => {
            let (year, month) = text.rsplit_once('-')?;
            let month = digits(month, 2)?;
            let year = (year.len() >= 4).then(|| year.parse::<f64>().ok()).flatten()?;
            (1.0..=12.0).contains(&month).then_some(year * 12.0 + month)
        }
        "week" => {
            let (year, week) = text.split_once("-W")?;
            let week = digits(week, 2)?;
            let year = (year.len() >= 4).then(|| year.parse::<f64>().ok()).flatten()?;
            (1.0..=53.0).contains(&week).then_some(year * 53.0 + week)
        }
        "time" => time(text),
        "datetime-local" => {
            let (day, clock) = text.split_once(['T', ' '])?;
            Some(date(day)? * 86_400.0 + time(clock)?)
        }
        _ => None,
    }
}

/// A valid e-mail address (HTML §4.10.5.1.5), its domain also in other scripts (Chrome turns
/// them into Punycode).
fn valid_email(address: &str) -> bool {
    let Some((local, domain)) = address.split_once('@') else {
        return false;
    };
    let local_ok = !local.is_empty()
        && local
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".!#$%&'*+/=?^_`{|}~-".contains(c));
    let label_ok = |label: &str| {
        !label.is_empty()
            && label.chars().count() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.chars().all(|c| c.is_alphanumeric() || c == '-')
    };
    local_ok && domain.split('.').all(label_ok)
}

/// An absolute URL, roughly as Chrome's parser takes one: a scheme, and a host for the schemes
/// that need one.
fn valid_url(url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once(':') else {
        return false;
    };
    let scheme_ok = scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if !scheme_ok || url.chars().any(char::is_whitespace) {
        return false;
    }
    match scheme.to_ascii_lowercase().as_str() {
        "http" | "https" | "ws" | "wss" | "ftp" => {
            let host = rest.trim_start_matches(['/', '\\']);
            host.split(['/', '?', '#', '\\'])
                .next()
                .is_some_and(|authority| !authority.rsplit('@').next().unwrap_or_default().is_empty())
        }
        _ => true,
    }
}

/// The selected option of a `<select>` that shows one (HTML §4.10.7): the last with
/// `selected`, else the first that is not disabled.
fn selected_options(document: &Document, select: NodeId) -> Vec<NodeId> {
    let options: Vec<NodeId> = document
        .descendants(select)
        .filter(|&node| document.element(node).is_some_and(|data| data.is("option")))
        .collect();
    let selected: Vec<NodeId> = options
        .iter()
        .copied()
        .filter(|&node| {
            document
                .element(node)
                .is_some_and(|data| data.attribute("selected").is_some())
        })
        .collect();
    let multiple = document
        .element(select)
        .is_some_and(|data| data.attribute("multiple").is_some());
    if multiple {
        return selected;
    }
    let size = document
        .element(select)
        .and_then(|data| data.attribute("size"))
        .and_then(|size| size.trim().parse::<u32>().ok())
        .unwrap_or(1);
    match selected.last() {
        Some(&last) => vec![last],
        None if size <= 1 => options
            .iter()
            .copied()
            .find(|&node| {
                document
                    .element(node)
                    .is_some_and(|data| data.attribute("disabled").is_none())
            })
            .into_iter()
            .collect(),
        None => Vec::new(),
    }
}

/// The value of an `<option>`: its `value`, else its text with collapsed white space.
fn option_value(document: &Document, option: NodeId) -> String {
    match document.element(option).and_then(|data| data.attribute("value")) {
        Some(value) => value.to_owned(),
        None => document
            .text_content(option)
            .split(is_html_space)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" "),
    }
}

/// The form an element belongs to, by its ancestors.
fn form_of(document: &Document, element: NodeId) -> Option<NodeId> {
    ancestors(document, element).find(|&node| document.element(node).is_some_and(|data| data.is("form")))
}

/// The radio buttons in the group of `radio` (HTML §4.10.5.1.18): those of its name in its
/// form, or in the document outside forms.
fn radio_group(document: &Document, radio: NodeId) -> Vec<NodeId> {
    let Some(name) = document
        .element(radio)
        .and_then(|data| data.attribute("name"))
        .filter(|name| !name.is_empty())
    else {
        return vec![radio];
    };
    let form = form_of(document, radio);
    document
        .descendants(NodeId::DOCUMENT)
        .filter(|&node| {
            document.element(node).is_some_and(|data| {
                data.is("input") && input_type(data) == "radio" && data.attribute("name") == Some(name)
            }) && form_of(document, node) == form
        })
        .collect()
}

/// Whether a checkbox or radio button is checked: by its `checked`, a radio button only where
/// no radio button after it in its group has one too (each unchecks the others of its group as
/// the parser inserts it).
pub fn checked(document: &Document, node: NodeId) -> bool {
    let Some(data) = document.element(node) else {
        return false;
    };
    if data.attribute("checked").is_none() {
        return false;
    }
    if !data.is("input") || input_type(data) != "radio" {
        return true;
    }
    let group = radio_group(document, node);
    group
        .iter()
        .rev()
        .find(|&&other| {
            document
                .element(other)
                .is_some_and(|data| data.attribute("checked").is_some())
        })
        .is_none_or(|&last| last == node)
}

/// Whether a candidate control satisfies its constraints.
fn satisfies(document: &Document, element: NodeId) -> bool {
    let Some(data) = document.element(element) else {
        return true;
    };
    let required = data.attribute("required").is_some();
    if data.is("select") {
        if !required {
            return true;
        }
        let selected = selected_options(document, element);
        let multiple = data.attribute("multiple").is_some();
        let size = data
            .attribute("size")
            .and_then(|size| size.trim().parse::<u32>().ok())
            .unwrap_or(1);
        // The placeholder label option: a first option child without a value.
        let placeholder = (!multiple && size <= 1)
            .then(|| {
                document.child_elements(element).next().filter(|&first| {
                    document.element(first).is_some_and(|data| data.is("option"))
                        && option_value(document, first).is_empty()
                })
            })
            .flatten();
        return !selected.is_empty() && selected.first().copied() != placeholder;
    }
    if data.is("textarea") {
        let text = document.text_content(element);
        let text = text.strip_prefix('\n').unwrap_or(&text);
        return !(required && text.is_empty());
    }
    if !data.is("input") {
        return true;
    }
    let kind = input_type(data);
    match kind.as_str() {
        "checkbox" => return !required || checked(document, element),
        "radio" => {
            let group = radio_group(document, element);
            let required = group.iter().any(|&node| {
                document
                    .element(node)
                    .is_some_and(|data| data.attribute("required").is_some())
            });
            return !required || group.iter().any(|&node| checked(document, node));
        }
        "file" => return !required,
        "submit" | "image" | "color" | "range" => return true,
        _ => {}
    }
    let value = input_value(data, &kind);
    if value.is_empty() {
        return !required;
    }
    let multiple = data.attribute("multiple").is_some();
    let type_ok = match kind.as_str() {
        "email" if multiple => value
            .split(',')
            .map(|part| part.trim_matches(is_html_space))
            .all(valid_email),
        "email" => valid_email(&value),
        "url" => valid_url(&value),
        _ => true,
    };
    if !type_ok {
        return false;
    }
    if is_text_like(&kind)
        && let Some(pattern) = data.attribute("pattern").and_then(Pattern::parse)
    {
        let values: Vec<&str> = if kind == "email" && multiple {
            value
                .split(',')
                .map(|part| part.trim_matches(is_html_space))
                .collect()
        } else {
            vec![value.as_str()]
        };
        if values
            .iter()
            .any(|value| pattern.matches_whole(value) == Some(false))
        {
            return false;
        }
    }
    if range(data, &kind, &value) == Some(false) {
        return false;
    }
    // A number off the steps from its minimum (its default value is the base otherwise, which
    // the value is).
    if kind == "number"
        && let (Some(number), Some(min)) =
            (parse_number(&value), data.attribute("min").and_then(parse_number))
        && data
            .attribute("step")
            .is_none_or(|step| !step.trim().eq_ignore_ascii_case("any"))
    {
        let step = data
            .attribute("step")
            .and_then(parse_number)
            .filter(|&step| step > 0.0)
            .unwrap_or(1.0);
        let steps = (number - min) / step;
        if (steps - steps.round()).abs() > 1e-7 {
            return false;
        }
    }
    true
}

/// Whether the value of an input with range limitations is within them (`None` without
/// limitations or a value).
fn range(data: &ElementData, kind: &str, value: &str) -> Option<bool> {
    let parse = |text: &str| -> Option<f64> {
        match kind {
            "number" | "range" => parse_number(text),
            _ if is_date_like(kind) => parse_temporal(kind, text),
            _ => None,
        }
    };
    let (min, max) = (
        data.attribute("min").and_then(parse),
        data.attribute("max").and_then(parse),
    );
    if min.is_none() && max.is_none() {
        return None;
    }
    let value = parse(value)?;
    // A time whose maximum is before its minimum wraps around midnight.
    if kind == "time"
        && let (Some(min), Some(max)) = (min, max)
        && max < min
    {
        return Some(value >= min || value <= max);
    }
    Some(min.is_none_or(|min| value >= min) && max.is_none_or(|max| value <= max))
}

/// Whether `element` matches `:valid` (`Some(true)`), `:invalid` (`Some(false)`) or neither:
/// candidates by their constraints, forms and fieldsets by the candidates in them.
pub(crate) fn validity(document: &Document, element: NodeId) -> Option<bool> {
    let data = document.element(element)?;
    if data.is("form") || data.is("fieldset") {
        return Some(
            !document
                .descendants(element)
                .any(|node| node != element && candidate(document, node) && !satisfies(document, node)),
        );
    }
    candidate(document, element).then(|| satisfies(document, element))
}

/// Whether `element` matches `:in-range` (`Some(true)`), `:out-of-range` (`Some(false)`) or
/// neither: inputs with a minimum or maximum (a slider always has them) and a value.
pub(crate) fn in_range(document: &Document, element: NodeId) -> Option<bool> {
    let data = document.element(element)?;
    if !data.is("input") || !candidate(document, element) {
        return None;
    }
    let kind = input_type(data);
    match kind.as_str() {
        // A slider clamps its value into its range.
        "range" => Some(true),
        "number" => range(data, &kind, &input_value(data, &kind))
            .or_else(|| (data.attribute("min").is_some() || data.attribute("max").is_some()).then_some(true)),
        _ if is_date_like(&kind) => range(data, &kind, &input_value(data, &kind))
            .or_else(|| (data.attribute("min").is_some() || data.attribute("max").is_some()).then_some(true)),
        _ => None,
    }
}

/// Whether `element` matches `:indeterminate`: a radio button whose group has none checked,
/// or a progress bar without a value.
pub(crate) fn indeterminate(document: &Document, element: NodeId) -> bool {
    let Some(data) = document.element(element) else {
        return false;
    };
    if data.is("progress") {
        return data.attribute("value").and_then(parse_number).is_none();
    }
    data.is("input")
        && input_type(data) == "radio"
        && !radio_group(document, element)
            .iter()
            .any(|&node| checked(document, node))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(document: &Document, id: &str) -> NodeId {
        document
            .descendants(NodeId::DOCUMENT)
            .find(|&node| {
                document
                    .element(node)
                    .is_some_and(|data| data.id.as_deref() == Some(id))
            })
            .expect("element")
    }

    #[test]
    fn validates_values_from_their_attributes() {
        let document = Document::parse_html(
            "<form id=f>\
             <input id=a type=email value='not-an-email'>\
             <input id=b type=email value=' sam@example.org '>\
             <input id=c required>\
             <input id=d required value=x>\
             <input id=e type=number min=1 max=5 value=7>\
             <input id=g pattern='[A-Z]{4}' value=ab12>\
             <input id=h pattern='[A-Z]{4}' value=ABCD>\
             <input id=i type=url value='example.org'>\
             <input id=j type=checkbox required>\
             <input id=k type=number min=0 value=0.5>\
             <input id=l type=date min=2026-01-01 value=2025-12-31>\
             <select id=m required><option value=''>Choose</option><option>A</option></select>\
             <textarea id=n required>\n</textarea>\
             <input id=o disabled required>\
             <input id=p readonly required>\
             <button id=q>Send</button><button id=r type=button>No</button>\
             </form><fieldset id=s><input id=t required value=y></fieldset>",
        );
        let validity = |id: &str| validity(&document, find(&document, id));
        for (id, expected) in [
            ("a", Some(false)),
            ("b", Some(true)),
            ("c", Some(false)),
            ("d", Some(true)),
            ("e", Some(false)),
            ("g", Some(false)),
            ("h", Some(true)),
            ("i", Some(false)),
            ("j", Some(false)),
            ("k", Some(false)),
            ("l", Some(false)),
            ("m", Some(false)),
            ("n", Some(false)),
            // Disabled and read-only fields are barred from validation.
            ("o", None),
            ("p", None),
            ("q", Some(true)),
            ("r", None),
            ("f", Some(false)),
            ("s", Some(true)),
        ] {
            assert_eq!(validity(id), expected, "{id}");
        }
    }

    #[test]
    fn tells_ranges_and_indeterminate_radio_groups() {
        let document = Document::parse_html(
            "<input id=a type=number min=1 max=5 value=7><input id=b type=number min=1 max=5 value=3>\
             <input id=c type=number value=3><input id=d type=range><input id=e type=time min=22:00 max=06:00 value=23:30>\
             <input id=f type=radio name=x><input id=g type=radio name=x><input id=h type=radio name=y checked>\
             <progress id=i></progress><progress id=j value=3></progress>",
        );
        let range = |id: &str| in_range(&document, find(&document, id));
        assert_eq!(range("a"), Some(false));
        assert_eq!(range("b"), Some(true));
        assert_eq!(range("c"), None);
        assert_eq!(range("d"), Some(true));
        assert_eq!(range("e"), Some(true));
        let indeterminate = |id: &str| indeterminate(&document, find(&document, id));
        assert!(indeterminate("f") && indeterminate("g") && !indeterminate("h"));
        assert!(indeterminate("i") && !indeterminate("j"));
    }

    #[test]
    fn checks_one_radio_button_of_a_group() {
        let document = Document::parse_html(
            "<input id=a type=radio name=q checked><input id=b type=radio name=q checked>\
             <form><input id=c type=radio name=q checked></form><input id=d type=checkbox checked>",
        );
        let checked = |id: &str| checked(&document, find(&document, id));
        // The last checked one of a group wins; a form makes a group of its own.
        assert!(!checked("a") && checked("b") && checked("c") && checked("d"));
    }
}

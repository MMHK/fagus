//! CSS counters (CSS Lists 3, §4) and their text: the `list-item` counter of list items,
//! `counter-reset`, `counter-increment` and `counter-set`, and `counter()` in generated content.

use fagus_style::values::ListStyleType;

use crate::counter_style::CounterStyles;

/// One counter instance.
#[derive(Clone, Debug)]
struct Counter {
    name: String,
    value: i32,
    /// The depth of the element that created it: it is visible to that element, its following
    /// siblings and their descendants; one deeper if it is visible to the element and its
    /// descendants only.
    depth: usize,
    /// The implicit step of `list-item` (-1 in a reversed list).
    step: i32,
    /// For `list-item`: the ordinal that list item markers show minus the counter's value.
    /// Chrome numbers markers from the list's `start` (or its item count when reversed) and an
    /// item's `value` attribute, while `counter(list-item)` counts on from `counter-reset`.
    ordinal: i32,
}

/// The counters in scope while the box tree is built in document order.
#[derive(Debug, Default)]
pub(crate) struct Counters {
    stack: Vec<Counter>,
}

impl Counters {
    /// `counter-reset` on an element at `depth`: a new counter that replaces one of the same name
    /// created by a preceding sibling. Where the parent has a counter of that name, the new one
    /// is for the element and its descendants only: its following siblings inherit the parent's
    /// (CSS Lists 3, §4.4.1, as in Chrome).
    pub fn reset(&mut self, name: &str, value: i32, depth: usize) {
        // So does one the element made itself already (an `<ol>` resets `list-item` beside its
        // style): at its depth, or one deeper where it is the element's own.
        self.stack.retain(|counter| {
            !(counter.name == name && (counter.depth == depth || counter.depth == depth + 1))
        });
        let nested = self
            .stack
            .iter()
            .any(|counter| counter.name == name && counter.depth < depth);
        self.stack.push(Counter {
            name: name.to_owned(),
            value,
            depth: if nested { depth + 1 } else { depth },
            step: 1,
            ordinal: 0,
        });
    }

    /// The innermost counter `name`. Where none is in scope, the element at `depth` that
    /// increments or sets it instantiates one at 0, visible to it, its following siblings and
    /// their descendants (CSS Lists 3, §4.4, as in Chrome: `h2::before { counter-increment: h }`
    /// shows 1 on every heading).
    fn innermost(&mut self, name: &str, depth: usize) -> &mut Counter {
        if let Some(index) = self.stack.iter().rposition(|counter| counter.name == name) {
            return &mut self.stack[index];
        }
        self.stack.push(Counter {
            name: name.to_owned(),
            value: 0,
            depth,
            step: 1,
            ordinal: 0,
        });
        let last = self.stack.len() - 1;
        &mut self.stack[last]
    }

    /// `counter-increment` on an element at `depth`.
    pub fn increment(&mut self, name: &str, by: i32, depth: usize) {
        let counter = self.innermost(name, depth);
        counter.value = counter.value.saturating_add(by);
    }

    /// The implicit increment of a list item at `depth`: by the list's step.
    pub fn increment_list_item(&mut self, depth: usize) {
        let counter = self.innermost("list-item", depth);
        counter.value = counter.value.saturating_add(counter.step);
    }

    /// `counter-set` on an element at `depth`. A list item's marker shows the value set.
    pub fn set(&mut self, name: &str, value: i32, depth: usize) {
        let counter = self.innermost(name, depth);
        counter.value = value;
        counter.ordinal = 0;
    }

    /// Numbers the markers of a list whose `list-item` counter the element at `depth` has just
    /// reset: its first item shows `base` plus the step.
    pub fn number_list(&mut self, base: i32, depth: usize) {
        let counter = self.innermost("list-item", depth);
        counter.ordinal = base.saturating_sub(counter.value);
    }

    /// The `value` attribute of a list item at `depth`: its marker shows `value`, and the next
    /// items count on from there, while `counter(list-item)` does not change (as in Chrome).
    pub fn number_item(&mut self, value: i32, depth: usize) {
        let counter = self.innermost("list-item", depth);
        counter.ordinal = value.saturating_sub(counter.value);
    }

    /// The number of the current list item's marker.
    pub fn ordinal(&self) -> i32 {
        self.stack
            .iter()
            .rev()
            .find(|counter| counter.name == "list-item")
            .map_or(0, |counter| counter.value.saturating_add(counter.ordinal))
    }

    /// Makes the innermost `list-item` counter count down (`<ol reversed>`).
    pub fn reverse_list_item(&mut self, depth: usize) {
        self.innermost("list-item", depth).step = -1;
    }

    pub fn value(&self, name: &str) -> i32 {
        self.stack
            .iter()
            .rev()
            .find(|counter| counter.name == name)
            .map_or(0, |counter| counter.value)
    }

    /// All counters in scope, by name in the order they were made, each with its values from
    /// the outermost in.
    pub fn snapshot(&self) -> Vec<(String, Vec<i32>)> {
        let mut names: Vec<&str> = Vec::new();
        for counter in &self.stack {
            if !names.contains(&counter.name.as_str()) {
                names.push(&counter.name);
            }
        }
        names
            .into_iter()
            .map(|name| (name.to_owned(), self.values(name)))
            .collect()
    }

    /// All counters `name` in scope, outermost first (for `counters()`); a counter that is not
    /// in scope reads as one at 0, as Chrome prints it.
    pub fn values(&self, name: &str) -> Vec<i32> {
        let values: Vec<i32> = self
            .stack
            .iter()
            .filter(|counter| counter.name == name)
            .map(|counter| counter.value)
            .collect();
        if values.is_empty() { vec![0] } else { values }
    }

    /// Ends the scope of the counters that the children of an element at `depth` created.
    pub fn leave(&mut self, depth: usize) {
        self.stack.retain(|counter| counter.depth <= depth);
    }
}

/// The text of a counter value in a counter style.
pub fn counter_text(value: i32, style: &ListStyleType, styles: &CounterStyles) -> String {
    match style {
        ListStyleType::String(text) => text.clone(),
        style => style
            .counter_style()
            .map(|name| styles.text(name, value))
            .unwrap_or_default(),
    }
}

/// The text of a list item's marker: the counter between the style's prefix and suffix (". "
/// for numbers).
pub fn marker_text(value: i32, style: &ListStyleType, styles: &CounterStyles) -> Option<String> {
    match style {
        ListStyleType::String(text) => Some(text.clone()),
        style => style.counter_style().map(|name| styles.marker(name, value)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_counters() {
        let styles = CounterStyles::default();
        assert_eq!(counter_text(4, &ListStyleType::UpperRoman, &styles), "IV");
        assert_eq!(counter_text(28, &ListStyleType::LowerAlpha, &styles), "ab");
        assert_eq!(counter_text(7, &ListStyleType::Decimal, &styles), "7");
        assert_eq!(counter_text(-3, &ListStyleType::LowerRoman, &styles), "-3");
        assert_eq!(counter_text(5, &ListStyleType::DecimalLeadingZero, &styles), "05");
        assert_eq!(counter_text(5, &ListStyleType::None, &styles), "");
        assert_eq!(
            marker_text(2, &ListStyleType::Decimal, &styles).as_deref(),
            Some("2. ")
        );
        assert_eq!(
            marker_text(2, &ListStyleType::Disc, &styles).as_deref(),
            Some("\u{2022} ")
        );
        assert_eq!(marker_text(2, &ListStyleType::None, &styles), None);
    }

    #[test]
    fn scopes_counters_like_css() {
        let mut counters = Counters::default();
        counters.reset("list-item", 0, 1);
        counters.increment_list_item(2);
        counters.increment_list_item(2);
        // A nested list.
        counters.reset("list-item", 0, 3);
        counters.increment_list_item(4);
        assert_eq!(counters.values("list-item"), [2, 1]);
        counters.leave(2);
        assert_eq!(
            counters.value("list-item"),
            2,
            "the nested counter ends with its list"
        );
    }

    #[test]
    fn sibling_resets_nest_only_inside_the_element() {
        let mut counters = Counters::default();
        counters.reset("c", 10, 1);
        // A child resets `c`, which its parent has: the new counter is its own.
        counters.reset("c", 1, 2);
        counters.increment("c", 1, 2);
        assert_eq!(counters.values("c"), [10, 2]);
        counters.leave(2);
        // Its next sibling counts on with the parent's counter.
        counters.increment("c", 1, 2);
        assert_eq!(counters.values("c"), [11]);
        // Without one in the parent, a reset reaches the following siblings.
        counters.leave(0);
        counters.reset("d", 5, 2);
        counters.leave(2);
        assert_eq!(counters.values("d"), [5]);
        // A second reset by the same element replaces its first.
        counters.reset("e", 1, 1);
        counters.reset("e", 0, 2);
        counters.reset("e", 0, 2);
        assert_eq!(counters.values("e"), [1, 0]);
    }

    #[test]
    fn increments_instantiate_counters_at_the_element() {
        let mut counters = Counters::default();
        // A counter that is not in scope reads as 0.
        assert_eq!(counters.values("h"), [0]);
        // `h2::before { counter-increment: h }`: each pseudo-element (depth 3) makes its own.
        counters.increment("h", 1, 3);
        assert_eq!(counters.value("h"), 1);
        counters.leave(2);
        counters.increment("h", 1, 3);
        assert_eq!(counters.value("h"), 1);
        counters.leave(2);
        // `p { counter-increment: q }`: siblings at depth 2 share the first one's counter, which
        // ends with their parent.
        counters.increment("q", 1, 2);
        counters.leave(2);
        counters.increment("q", 1, 2);
        assert_eq!(counters.values("q"), [2]);
        counters.leave(1);
        assert_eq!(counters.values("q"), [0]);
        counters.set("q", 7, 2);
        assert_eq!(counters.values("q"), [7]);
    }
}

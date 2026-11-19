//! Form controls painted for the fields of an interactive form: what a control shows of its value
//! goes into the appearance of its field instead of onto the page, so that the viewer can draw
//! another value in its place.

use std::sync::Arc;

use fagus_dom::NodeId;
use fagus_layout::{Fragment, FragmentKind};
use fagus_style::values::{Layer, Visibility};

use crate::{Item, Painter};

/// What of a control its field shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lift {
    /// The text of its value (a text field, a `<select>`): its frame and anything else it draws,
    /// such as the arrow of a `<select>`, stay on the page.
    Text,
    /// Everything inside its box: the mark of a checkbox or radio button.
    Whole,
}

/// A control painted for a field: where it lies on its page and what it shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Widget {
    pub node: NodeId,
    pub page: usize,
    /// The area of the control's border box on the page, in CSS pixels: x, y, width and height,
    /// shrunk with the content where the content is shrunk.
    pub rect: [f32; 4],
    /// The size of the border box as laid out, the area [`Widget::items`] are drawn in.
    pub size: (f32, f32),
    /// What the field shows, with the top left corner of the border box at the origin.
    pub items: Vec<Item>,
    /// The innermost layer (`-fagus-layer`) the control is on.
    pub layer: Option<Arc<Layer>>,
}

/// How the content of a page is placed on it: shrunk by a scale around a point, after moving
/// right by a fraction of a pixel (see `paint_pages`).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Placement {
    pub(crate) shrink: Option<(f32, f32, f32)>,
    pub(crate) shift: f32,
}

impl Placement {
    fn on_page(self, x: f32, y: f32) -> (f32, f32) {
        let x = x + self.shift;
        match self.shrink {
            Some((scale, left, top)) => (left + (x - left) * scale, top + (y - top) * scale),
            None => (x, y),
        }
    }

    fn scale(self) -> f32 {
        self.shrink.map_or(1.0, |(scale, ..)| scale)
    }
}

impl Painter<'_> {
    /// What of `fragment` its field shows, if it is the box of a control that becomes a field
    /// and the field can lie where the box is: not in a transformed box or a text mask, and not
    /// a copy of content that reaches onto the page from the page before.
    fn lift_of(&self, fragment: &Fragment) -> Option<Lift> {
        let FragmentKind::Box {
            style,
            node: Some(node),
            ..
        } = &fragment.kind
        else {
            return None;
        };
        let lift = *self.fields?.get(node)?;
        let copied = self
            .aliases
            .is_some_and(|aliases| aliases.contains_key(&std::ptr::from_ref(fragment)));
        (style.visibility == Visibility::Visible
            && self.transforms == 0
            && self.space.is_none()
            && !self.text_mask
            && !copied)
            .then_some(lift)
    }

    /// Paints the content of `fragment` (at `x`, `y`) with `paint`; where the box is a control
    /// that becomes a field, what the field shows goes into its widget instead of onto the page.
    pub(crate) fn lifted(&mut self, fragment: &Fragment, x: f32, y: f32, paint: impl FnOnce(&mut Self)) {
        let Some(lift) = self.lift_of(fragment) else {
            paint(self);
            return;
        };
        let outer_items = std::mem::take(&mut self.items);
        let outer_tagged = std::mem::replace(&mut self.tagged, false);
        let outer_tag = self.tag.take();
        paint(self);
        let captured = std::mem::replace(&mut self.items, outer_items);
        self.tagged = outer_tagged;
        self.tag = outer_tag;
        let (mut shown, mut kept) = match lift {
            Lift::Whole => (captured, Vec::new()),
            Lift::Text => split(captured),
        };
        crate::cull::drop_empty_groups(&mut kept);
        crate::cull::drop_empty_groups(&mut shown);
        // What stays on the page is the control's design, as its frame is.
        if !kept.is_empty() {
            self.design(|painter| painter.items.extend(kept));
        }
        let FragmentKind::Box { node: Some(node), .. } = &fragment.kind else {
            return;
        };
        let key = std::ptr::from_ref(fragment);
        let index = if let Some(index) = self.lifted.iter().position(|(known, _)| *known == key) {
            index
        } else {
            let size = (fragment.rect.width.to_px(), fragment.rect.height.to_px());
            let (left, top) = self.placement.on_page(x, y);
            let scale = self.placement.scale();
            self.lifted.push((
                key,
                Widget {
                    node: *node,
                    page: self.page_index,
                    rect: [left, top, size.0 * scale, size.1 * scale],
                    size,
                    items: Vec::new(),
                    layer: self.layers.last().cloned(),
                },
            ));
            self.lifted.len() - 1
        };
        if !shown.is_empty() {
            let items = &mut self.lifted[index].1.items;
            items.push(Item::PushTransform([1.0, 0.0, 0.0, 1.0, -x, -y]));
            items.append(&mut shown);
            items.push(Item::Pop);
        }
    }
}

/// The items of a control's content split into the text, which the field shows, and the rest,
/// which stays on the page. Both keep the groups around what they hold.
fn split(items: Vec<Item>) -> (Vec<Item>, Vec<Item>) {
    let mut shown = Vec::new();
    let mut kept = Vec::new();
    for item in items {
        match item {
            Item::Text { .. } | Item::TextShadow { .. } | Item::Wave { .. } | Item::DashedLine { .. } => {
                shown.push(item);
            }
            Item::Tag(_) => {}
            _ if crate::is_group(&item) => {
                shown.push(item.clone());
                kept.push(item);
            }
            _ => kept.push(item),
        }
    }
    (shown, kept)
}

#[cfg(test)]
mod tests {
    use rustc_hash::FxHashMap;

    use super::*;

    fn painted(html: &str, lift: Lift) -> (Vec<crate::PageList>, Vec<Widget>) {
        let document = fagus_dom::Document::parse_html(html);
        let control = document
            .find("input")
            .or_else(|| document.find("select"))
            .expect("a control");
        let user_agent = fagus_css::Stylesheet::parse(
            fagus_style::USER_AGENT_CSS,
            fagus_css::Origin::UserAgent,
            "ua.css",
        );
        let rules = fagus_css::RuleSet::new(&[&user_agent], &fagus_css::MediaEnvironment::default());
        let styled = fagus_style::compute_styles(&document, &rules, (794.0, 1123.0));
        let mut fonts = fagus_text::FontDb::new();
        fonts.add_default_fonts();
        let images = fagus_layout::Images::default();
        let pages = fagus_layout::layout(&document, &styled, &rules, &fonts, &images);
        let mut fields = FxHashMap::default();
        fields.insert(control, lift);
        crate::paint_with_fields(&pages, &fonts, &images, true, &fields)
    }

    fn texts(items: &[Item]) -> Vec<&str> {
        items
            .iter()
            .filter_map(|item| match item {
                Item::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn moves_the_value_of_a_field_into_its_widget() {
        let (pages, widgets) = painted(
            "<!DOCTYPE html><p>Name <input name=name value=Ada style='margin-left: 20px'>",
            Lift::Text,
        );
        assert_eq!(widgets.len(), 1);
        let widget = &widgets[0];
        assert_eq!(texts(&widget.items), ["Ada"]);
        assert_eq!(widget.page, 0);
        assert!(
            widget.rect[0] > 20.0 && widget.rect[2] > 100.0,
            "{:?}",
            widget.rect
        );
        assert_eq!((widget.rect[2], widget.rect[3]), widget.size);
        // The label stays on the page, the value does not; the frame is still drawn there.
        assert_eq!(texts(&pages[0].items), ["Name "]);
        assert!(
            pages[0]
                .items
                .iter()
                .any(|item| matches!(item, Item::RoundedBorder { .. }))
        );
        // The value is drawn from the corner of the box.
        assert!(matches!(widget.items[0], Item::PushTransform([1.0, 0.0, 0.0, 1.0, x, _]) if x < -20.0));
    }

    #[test]
    fn moves_a_whole_checkbox_into_its_widget() {
        let (pages, widgets) = painted(
            "<!DOCTYPE html><input type=checkbox name=ok checked>",
            Lift::Whole,
        );
        assert_eq!(widgets.len(), 1);
        assert!(
            widgets[0]
                .items
                .iter()
                .any(|item| matches!(item, Item::Image { .. }))
        );
        assert!(
            !pages[0]
                .items
                .iter()
                .any(|item| matches!(item, Item::Image { .. }))
        );
    }

    #[test]
    fn leaves_hidden_controls_on_the_page() {
        let (_, widgets) = painted(
            "<!DOCTYPE html><input name=a value=x style='visibility: hidden'>",
            Lift::Text,
        );
        assert_eq!(widgets, []);
        let (pages, widgets) = painted(
            "<!DOCTYPE html><input name=a value=x style='transform: rotate(10deg)'>",
            Lift::Text,
        );
        assert_eq!(widgets, []);
        assert_eq!(texts(&pages[0].items), ["x"]);
    }
}

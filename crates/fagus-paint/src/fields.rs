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
    /// shrunk with the content where the content is shrunk. In a transformed box, the smallest
    /// rectangle around the border box as it is transformed. Where the page area or a box around
    /// with `overflow` other than `visible` cuts the control, only the part that shows.
    pub rect: [f32; 4],
    /// How the border box is turned, skewed and scaled onto the page: `[a, b, c, d]` of a
    /// matrix as in [`Item::PushTransform`], `[1, 0, 0, 1]` where it is not.
    pub transform: [f32; 4],
    /// The size of the border box as laid out, the area [`Widget::items`] are drawn in.
    pub size: (f32, f32),
    /// What the field shows, with the top left corner of the border box at the origin.
    pub items: Vec<Item>,
    /// The part of the border box that shows where clipping cuts the control (see
    /// [`Widget::rect`]): x, y, width and height from its top left corner, in the size it is
    /// laid out in. `None` where all of it shows.
    pub clip: Option<[f32; 4]>,
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

/// Where a box of `size` at `(x, y)` lies on the page with the transform `ctm` around it: the
/// smallest rectangle around its corners, and the transform without its translation.
#[allow(clippy::many_single_char_names, reason = "the entries of the matrix")]
fn placed(placement: Placement, ctm: [f32; 6], (x, y): (f32, f32), size: (f32, f32)) -> ([f32; 4], [f32; 4]) {
    let [a, b, c, d, e, f] = ctm;
    let corners = [(x, y), (x + size.0, y), (x, y + size.1), (x + size.0, y + size.1)]
        .map(|(x, y)| placement.on_page(a * x + c * y + e, b * x + d * y + f));
    let (mut left, mut top) = corners[0];
    let (mut right, mut bottom) = corners[0];
    for (x, y) in corners {
        left = left.min(x);
        top = top.min(y);
        right = right.max(x);
        bottom = bottom.max(y);
    }
    let scale = placement.scale();
    (
        [left, top, right - left, bottom - top],
        [a * scale, b * scale, c * scale, d * scale],
    )
}

/// Where the widget of a control lies on the page: its rectangle, how its box is transformed
/// (see [`Widget::transform`]), the size it is laid out in and the part that shows (see
/// [`Widget::clip`]).
struct Place {
    rect: [f32; 4],
    transform: [f32; 4],
    size: (f32, f32),
    clip: Option<[f32; 4]>,
}

/// What of a cut control shows (see [`part_shown`]).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Part {
    /// This part: x, y, width and height from the top left corner of its box.
    Shown([f32; 4]),
    /// Nothing.
    Hidden,
    /// A part that is no rectangle in the box: the box is turned other than by quarter turns,
    /// or skewed.
    Skewed,
}

/// The part of a box of `size` that shows where the rectangle `shown` (left, top, right, bottom
/// on the page) of it does, its top left corner lying at `origin` on the page with the box
/// turned by `transform`.
fn part_shown(transform: [f32; 4], origin: (f32, f32), shown: [f32; 4], size: (f32, f32)) -> Part {
    let [xx, yx, xy, yy] = transform;
    let upright = yx.abs() < 1e-4 && xy.abs() < 1e-4;
    let turned = xx.abs() < 1e-4 && yy.abs() < 1e-4;
    let determinant = xx * yy - yx * xy;
    if !(upright || turned) || determinant.abs() < 1e-6 {
        return Part::Skewed;
    }
    let back = |(left, top): (f32, f32)| {
        let (along, down) = (left - origin.0, top - origin.1);
        (
            (yy * along - xy * down) / determinant,
            (xx * down - yx * along) / determinant,
        )
    };
    let corners = [
        (shown[0], shown[1]),
        (shown[2], shown[1]),
        (shown[0], shown[3]),
        (shown[2], shown[3]),
    ]
    .map(back);
    let fold = |pick: fn(&(f32, f32)) -> f32, start: f32, choose: fn(f32, f32) -> f32| {
        corners.iter().map(pick).fold(start, choose)
    };
    let left = fold(|corner| corner.0, f32::INFINITY, f32::min).max(0.0);
    let top = fold(|corner| corner.1, f32::INFINITY, f32::min).max(0.0);
    let right = fold(|corner| corner.0, f32::NEG_INFINITY, f32::max).min(size.0);
    let bottom = fold(|corner| corner.1, f32::NEG_INFINITY, f32::max).min(size.1);
    if left < right && top < bottom {
        Part::Shown([left, top, right - left, bottom - top])
    } else {
        Part::Hidden
    }
}

impl Painter<'_> {
    /// What of `fragment` its field shows, if it is the box of a control that becomes a field
    /// and the field can lie where the box is: not in a 3D space or a text mask, and not a copy
    /// of content that reaches onto the page from the page before.
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
            && self.ctm.is_some()
            && self.space.is_none()
            && !self.text_mask
            && !copied)
            .then_some(lift)
    }

    /// Where the widget of the control `fragment` (at `x`, `y`) lies on the page (see
    /// [`Place`]); `None` where the clips around it hide all of it. The clips are those of the
    /// page and of the boxes around the control, not its own, which the painter may have opened
    /// just before what it holds.
    fn place(&self, fragment: &Fragment, x: f32, y: f32) -> Option<Place> {
        let size = (fragment.rect.width.to_px(), fragment.rect.height.to_px());
        let identity = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
        let ctm = self.ctm.unwrap_or(identity);
        let (rect, transform) = placed(self.placement, ctm, (x, y), size);
        let own = crate::clip_rect(fragment, x, y).map(|clip| clip.item());
        let clip = crate::cull::open_clip(&self.items, self.page_bounds, own.as_ref())?;
        let bounds = [rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]];
        let shown = crate::cull::overlap(bounds, clip)?;
        let whole = Place {
            rect,
            transform,
            size,
            clip: None,
        };
        if crate::cull::holds(clip, bounds) {
            return Some(whole);
        }
        // The part of the box that shows: the visible rectangle taken back into the box. A box
        // turned other than by quarter turns or skewed shows a part that is no rectangle in it:
        // its field stays whole.
        let origin = self
            .placement
            .on_page(ctm[0] * x + ctm[2] * y + ctm[4], ctm[1] * x + ctm[3] * y + ctm[5]);
        let part = part_shown(transform, origin, shown, size);
        match part {
            Part::Shown(part) => Some(Place {
                rect: [shown[0], shown[1], shown[2] - shown[0], shown[3] - shown[1]],
                clip: Some(part),
                ..whole
            }),
            Part::Hidden => None,
            Part::Skewed => Some(whole),
        }
    }

    /// Paints the content of `fragment` (at `x`, `y`) with `paint`; where the box is a control
    /// that becomes a field, what the field shows goes into its widget instead of onto the page.
    pub(crate) fn lifted(&mut self, fragment: &Fragment, x: f32, y: f32, paint: impl FnOnce(&mut Self)) {
        let Some(lift) = self.lift_of(fragment) else {
            paint(self);
            return;
        };
        let key = std::ptr::from_ref(fragment);
        // Where the field lies and what of the control shows, found where the control is
        // painted first. A control that clipping hides entirely stays as it is printed.
        let known = self.lifted.iter().any(|(known, _)| *known == key);
        let place = if known || self.hidden_controls.contains(&key) {
            None
        } else {
            let place = self.place(fragment, x, y);
            if place.is_none() {
                self.hidden_controls.push(key);
            }
            place
        };
        if !known && place.is_none() {
            paint(self);
            return;
        }
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
        let index = if let Some(index) = self.lifted.iter().position(|(known, _)| *known == key) {
            index
        } else {
            let Some(Place {
                rect,
                transform,
                size,
                clip,
            }) = place
            else {
                return;
            };
            self.lifted.push((
                key,
                Widget {
                    node: *node,
                    page: self.page_index,
                    rect,
                    transform,
                    size,
                    items: Vec::new(),
                    clip,
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
        // In a 3D space a field cannot lie where the control is drawn.
        let (pages, widgets) = painted(
            "<!DOCTYPE html><div style='transform-style: preserve-3d; transform: rotateY(20deg)'>\
             <input name=a value=x style='transform: rotateX(10deg)'></div>",
            Lift::Text,
        );
        assert_eq!(widgets, []);
        assert_eq!(texts(&pages[0].items), ["x"]);
    }

    #[test]
    fn finds_the_part_of_a_cut_control_that_shows() {
        // Upright at (10, 20), 100 × 20, cut at 50 and 110 on the page.
        let upright = [1.0, 0.0, 0.0, 1.0];
        assert_eq!(
            part_shown(upright, (10.0, 20.0), [50.0, 20.0, 110.0, 40.0], (100.0, 20.0)),
            Part::Shown([40.0, 0.0, 60.0, 20.0])
        );
        // Turned by a quarter, its top left corner at (50, 10): it lies from 30 to 50 across and
        // 10 to 110 down; the upper half shows.
        assert_eq!(
            part_shown(
                [0.0, 1.0, -1.0, 0.0],
                (50.0, 10.0),
                [30.0, 10.0, 50.0, 60.0],
                (100.0, 20.0)
            ),
            Part::Shown([0.0, 0.0, 50.0, 20.0])
        );
        // Turned by 30°, the part is no rectangle.
        let (sin, cos) = (0.5, 0.866);
        assert_eq!(
            part_shown(
                [cos, sin, -sin, cos],
                (50.0, 10.0),
                [30.0, 10.0, 50.0, 60.0],
                (100.0, 20.0)
            ),
            Part::Skewed
        );
    }

    #[test]
    #[allow(clippy::many_single_char_names, reason = "the entries of the matrix")]
    fn places_transformed_fields_around_their_boxes() {
        let close = |a: f32, b: f32| (a - b).abs() < 0.01;
        // (Away from the top of the page area, which would cut the turned box off.)
        let (_, upright) = painted(
            "<!DOCTYPE html><input name=a value=x style='margin-top: 100px'>",
            Lift::Text,
        );
        let (width, height) = upright[0].size;
        assert_eq!(upright[0].transform, [1.0, 0.0, 0.0, 1.0]);

        // Turned by a quarter around its center: as wide as it was high.
        let (pages, widgets) = painted(
            "<!DOCTYPE html><input name=a value=x style='transform: rotate(90deg); margin-top: 100px'>",
            Lift::Text,
        );
        assert_eq!(texts(&pages[0].items), Vec::<&str>::new());
        let widget = &widgets[0];
        assert_eq!(widget.size, (width, height));
        let [a, b, c, d] = widget.transform;
        assert!(
            close(a, 0.0) && close(b, 1.0) && close(c, -1.0) && close(d, 0.0),
            "{a} {b} {c} {d}"
        );
        assert!(
            close(widget.rect[2], height) && close(widget.rect[3], width),
            "{:?}",
            widget.rect
        );
        let center = |rect: [f32; 4]| (rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0);
        let (x, y) = center(widget.rect);
        let (upright_x, upright_y) = center(upright[0].rect);
        assert!(close(x, upright_x) && close(y, upright_y), "{x} {y}");

        // Inside a scaled and rotated box, by 30°: the rectangle around the turned box.
        let (_, widgets) = painted(
            "<!DOCTYPE html><div style='transform: scale(2)'>\
             <input name=a value=x style='transform: rotate(30deg)'></div>",
            Lift::Text,
        );
        let [a, b, ..] = widgets[0].transform;
        assert!(
            close(a, 2.0 * libm::cosf(std::f32::consts::FRAC_PI_6))
                && close(b, 2.0 * libm::sinf(std::f32::consts::FRAC_PI_6))
        );
        let expected = 2.0
            * (width * libm::cosf(std::f32::consts::FRAC_PI_6)
                + height * libm::sinf(std::f32::consts::FRAC_PI_6));
        assert!(close(widgets[0].rect[2], expected), "{:?}", widgets[0].rect);
    }
}

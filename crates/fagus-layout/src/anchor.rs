//! Anchor positioning (CSS Anchor Positioning 1) of absolutely positioned boxes: insets, sizes
//! and margins with `anchor()` and `anchor-size()`, `position-area`, `anchor-center` and
//! `position-try-fallbacks`, against the boxes of the page laid out so far.
//!
//! An anchor is the last box named so (`anchor-name`) before the positioned element in tree
//! order, laid out already on the page, and not hidden from it by `anchor-scope`; its border box
//! (the union of its fragments, for an inline box across lines) is what the functions measure.
//! Where the box overflows its inset-modified containing block, the options of
//! `position-try-fallbacks` (`@position-try` rules among them) are tried in order, or in the
//! order of `position-try-order`, and the first that fits is used, else the box's own
//! placement. `position-visibility` hides the box where its anchor is clipped away or where no
//! option fits. Anchors on earlier pages count, where the box is placed in the flow of pages,
//! as in Chrome.

use fagus_dom::NodeId;
use fagus_style::ComputedStyle;
use rustc_hash::FxHashMap;
use std::sync::Arc;

use fagus_style::anchor::{
    AnchorRect, AnchorScope, Anchoring, Environment, PositionArea, Slot, Tokens, TryOption, TryOrder,
};
use fagus_style::values::{
    ItemAlignment, LengthPercentage, LengthPercentageAuto, MaxSize, Overflow, Size, Visibility,
};

use crate::block::Layouter;
use crate::fragment::{Fragment, FragmentKind};
use crate::tree::BoxId;
use crate::units::{Au, Rect};

/// A box that is an anchor, found on the page.
#[derive(Clone, Debug)]
pub(crate) struct Found {
    /// The style that names it (`anchor-name`), shared rather than copied: a page's anchors
    /// are found again for each box placed against them.
    anchoring: Arc<Anchoring>,
    node: Option<NodeId>,
    /// Its border box in page coordinates.
    rect: Rect,
    /// The elements with an `anchor-scope` around it (itself included), and their scopes.
    scopes: Vec<(Option<NodeId>, AnchorScope)>,
    /// The elements around it that clip what overflows them, and their padding boxes.
    clips: Vec<(Option<NodeId>, Rect)>,
}

impl Found {
    /// The anchor moved down by `by`.
    pub(crate) fn moved(&self, by: Au) -> Self {
        let mut moved = self.clone();
        moved.rect.y += by;
        for (_, clip) in &mut moved.clips {
            clip.y += by;
        }
        moved
    }
}

/// The anchors on a page whose content is `content`, in page coordinates.
pub(crate) fn anchors_on(content: &Fragment) -> Vec<Found> {
    let mut found = Vec::new();
    collect(
        content,
        (content.rect.x, content.rect.y),
        &mut Around::default(),
        &mut found,
    );
    found
}

/// The scopes and the clips of the boxes around a fragment, and where in the anchors found the
/// first of each element is.
#[derive(Default)]
struct Around {
    scopes: Vec<(Option<NodeId>, AnchorScope)>,
    clips: Vec<(Option<NodeId>, Rect)>,
    /// The index of the first anchor of each element among those found, so that the fragments
    /// of one box are found in constant time (thousands of anchors on a page took minutes).
    first: FxHashMap<NodeId, usize>,
}

impl Around {
    /// Nothing around yet, with `found` the anchors known already.
    fn with(found: &[Found]) -> Self {
        let mut around = Self::default();
        for (index, anchor) in found.iter().enumerate() {
            if let Some(node) = anchor.node {
                around.first.entry(node).or_insert(index);
            }
        }
        around
    }
}

/// Collects the anchors below `fragment`, whose border box origin is at `origin`; `around`
/// holds the scopes and the clips of the boxes around it.
fn collect(fragment: &Fragment, origin: (Au, Au), around: &mut Around, found: &mut Vec<Found>) {
    let mut pushed = false;
    let mut clipped = false;
    if let FragmentKind::Box { style, node, .. } = &fragment.kind
        && let Some(anchoring) = &style.anchoring
    {
        if anchoring.scope != AnchorScope::None {
            around.scopes.push((*node, anchoring.scope.clone()));
            pushed = true;
        }
        if !anchoring.names.is_empty() {
            let rect = Rect::new(origin.0, origin.1, fragment.rect.width, fragment.rect.height);
            // The fragments of one box (an inline box across lines) make one anchor.
            if let Some(&index) = node.and_then(|node| around.first.get(&node)) {
                found[index].rect = union(found[index].rect, rect);
            } else {
                if let Some(node) = *node {
                    around.first.insert(node, found.len());
                }
                found.push(Found {
                    anchoring: Arc::clone(anchoring),
                    node: *node,
                    rect,
                    scopes: around.scopes.clone(),
                    clips: around.clips.clone(),
                });
            }
        }
    }
    if let FragmentKind::Box { style, node, .. } = &fragment.kind
        && (style.overflow_x != Overflow::Visible || style.overflow_y != Overflow::Visible)
    {
        let border = &style.border_width;
        let (left, top) = (Au::from_px(border.left), Au::from_px(border.top));
        // An axis that does not clip reaches far.
        let far = Au::from_px(1e6);
        let mut padding = Rect::new(
            origin.0 + left,
            origin.1 + top,
            (fragment.rect.width - left - Au::from_px(border.right)).clamp_non_negative(),
            (fragment.rect.height - top - Au::from_px(border.bottom)).clamp_non_negative(),
        );
        if style.overflow_x == Overflow::Visible {
            padding.x = -far;
            padding.width = far * 2;
        }
        if style.overflow_y == Overflow::Visible {
            padding.y = -far;
            padding.height = far * 2;
        }
        around.clips.push((*node, padding));
        clipped = true;
    }
    for child in &fragment.children {
        collect(
            child,
            (origin.0 + child.rect.x, origin.1 + child.rect.y),
            around,
            found,
        );
    }
    if pushed {
        around.scopes.pop();
    }
    if clipped {
        around.clips.pop();
    }
}

fn union(a: Rect, b: Rect) -> Rect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    Rect::new(
        x,
        y,
        (a.x + a.width).max(b.x + b.width) - x,
        (a.y + a.height).max(b.y + b.height) - y,
    )
}

/// The elements of the fragments along `path` from `root`: the ancestors of what is there.
fn ancestors(root: &Fragment, path: &[usize]) -> Vec<NodeId> {
    let mut nodes = Vec::new();
    let mut fragment = root;
    for &index in path {
        if let FragmentKind::Box { node: Some(node), .. } = &fragment.kind {
            nodes.push(*node);
        }
        match fragment.children.get(index) {
            Some(child) => fragment = child,
            None => break,
        }
    }
    nodes
}

/// The insets (top, right, bottom, left), margins and sizes of one placement of a box.
#[derive(Clone)]
struct Placement {
    insets: [Value<LengthPercentageAuto>; 4],
    margins: [Value<LengthPercentageAuto>; 4],
    /// Width, height, min-width, min-height, max-width, max-height.
    sizes: [Option<Tokens>; 6],
    area: Option<PositionArea>,
    /// Whether the box is centered on its anchor horizontally and vertically.
    center: [bool; 2],
    /// `justify-self` and `align-self`.
    align: [ItemAlignment; 2],
    /// Whether the box's own sizes are swapped between the axes (`flip-start`).
    transposed: bool,
    /// `position-anchor`.
    default: Option<String>,
    /// The style a `@position-try` rule makes of the box's, which the placement starts from.
    style: Option<Arc<ComputedStyle>>,
}

/// A property's own value, or one with anchor functions.
#[derive(Clone)]
enum Value<T> {
    Own(T),
    Anchored(Tokens),
}

impl Placement {
    fn of(style: &ComputedStyle, anchoring: &Anchoring) -> Self {
        let side = |own: &LengthPercentageAuto, anchored: &Option<Tokens>| match anchored {
            Some(tokens) => Value::Anchored(tokens.clone()),
            None => Value::Own(own.clone()),
        };
        let inset = &style.inset;
        let margin = &style.margin;
        Placement {
            insets: [
                side(&inset.top, &anchoring.insets[0]),
                side(&inset.right, &anchoring.insets[1]),
                side(&inset.bottom, &anchoring.insets[2]),
                side(&inset.left, &anchoring.insets[3]),
            ],
            margins: [
                side(&margin.top, &anchoring.margins[0]),
                side(&margin.right, &anchoring.margins[1]),
                side(&margin.bottom, &anchoring.margins[2]),
                side(&margin.left, &anchoring.margins[3]),
            ],
            sizes: anchoring.sizes.clone(),
            area: anchoring.area,
            center: anchoring.center,
            align: [style.justify_self, style.align_self],
            transposed: false,
            default: anchoring.default.clone(),
            style: None,
        }
    }

    /// The placement an option of `position-try-fallbacks` makes of this one; `None` for a
    /// `@position-try` rule that is not there.
    fn tried(&self, option: &TryOption) -> Option<Self> {
        let mut tried = self.clone();
        match *option {
            TryOption::Area(area) => tried.area = Some(area),
            TryOption::Rule {
                ref style,
                flip: (block, inline, start),
                ..
            } => {
                let style = style.as_ref()?;
                let anchoring = style.anchoring.as_deref().cloned().unwrap_or_default();
                let mut placement = Self::of(style, &anchoring);
                placement.style = Some(Arc::clone(style));
                if block || inline || start {
                    placement = placement.tried(&TryOption::Flip { block, inline, start })?;
                }
                return Some(placement);
            }
            TryOption::Flip { block, inline, start } => {
                let flip = |value: &mut Value<LengthPercentageAuto>| {
                    if let Value::Anchored(tokens) = value {
                        *tokens = fagus_style::anchor::flip(tokens, block, inline, start);
                    }
                };
                if start {
                    // Top with left, bottom with right, and the sizes of the axes.
                    tried.insets.swap(0, 3);
                    tried.insets.swap(1, 2);
                    tried.margins.swap(0, 3);
                    tried.margins.swap(1, 2);
                    tried.sizes.swap(0, 1);
                    tried.sizes.swap(2, 3);
                    tried.sizes.swap(4, 5);
                    tried.area = tried.area.map(PositionArea::transposed);
                    tried.center.swap(0, 1);
                    tried.align.swap(0, 1);
                    tried.transposed = !tried.transposed;
                }
                if block {
                    tried.insets.swap(0, 2);
                    tried.margins.swap(0, 2);
                    tried.align[1] = mirrored(tried.align[1]);
                }
                if inline {
                    tried.insets.swap(1, 3);
                    tried.margins.swap(1, 3);
                    tried.align[0] = mirrored(tried.align[0]);
                }
                tried.area = tried.area.map(|area| area.flipped(block, inline));
                tried.insets.iter_mut().for_each(flip);
                tried.margins.iter_mut().for_each(flip);
            }
        }
        Some(tried)
    }
}

fn mirrored(alignment: ItemAlignment) -> ItemAlignment {
    match alignment {
        ItemAlignment::Start => ItemAlignment::End,
        ItemAlignment::End => ItemAlignment::Start,
        other => other,
    }
}

/// How a box is aligned in an axis of its inset-modified containing block.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Align {
    Start,
    End,
    Center,
    Stretch,
    /// Centered on the anchor.
    Anchor,
}

/// The alignment in a `position-area` that spans `range` of an axis, for `normal`: towards the
/// anchor, centered on it where the area spans all three tracks (CSS Anchor Positioning 1,
/// §3.1.3).
fn area_alignment(range: (u8, u8), own: ItemAlignment, centered: bool) -> Align {
    if centered {
        return Align::Anchor;
    }
    match own {
        ItemAlignment::Start => return Align::Start,
        ItemAlignment::End => return Align::End,
        ItemAlignment::Center => return Align::Center,
        ItemAlignment::Stretch => return Align::Stretch,
        _ => {}
    }
    match range {
        (0, 2) => Align::Anchor,
        (1, 1) => Align::Center,
        (0, _) => Align::End,
        _ => Align::Start,
    }
}

impl Layouter<'_> {
    /// Lays out an absolutely positioned box placed against an anchor, in a containing block
    /// `width` × `height` whose origin is at `origin` in the page coordinates of `root`.
    /// `placeholder` is the path from `root` to where the box would be in flow. `None` for boxes
    /// without anchor positioning, which [`Layouter::absolute`] lays out.
    /// `elsewhere` are the anchors on earlier pages, in the coordinates of this one.
    #[expect(
        clippy::too_many_arguments,
        reason = "what placing a box against anchors takes"
    )]
    pub(crate) fn anchored(
        &self,
        id: BoxId,
        root: &Fragment,
        placeholder: &[usize],
        origin: (Au, Au),
        (width, height): (Au, Au),
        static_position: (Au, Au),
        elsewhere: &[Found],
    ) -> Option<Fragment> {
        let layout_box = self.tree.get(id);
        let style = &layout_box.style;
        let anchoring = style
            .anchoring
            .as_deref()
            .filter(|anchoring| anchoring.is_positioned())?;
        let mut found = elsewhere.to_vec();
        collect(
            root,
            (root.rect.x, root.rect.y),
            &mut Around::with(&found),
            &mut found,
        );
        let own_ancestors = ancestors(root, placeholder);
        let own_node = layout_box.node;
        let find = |name: Option<&str>| -> Option<&Found> {
            let name = name?;
            // The last in tree order of those laid out (boxes in flow are laid out before the
            // positioned boxes of their containing block, which are placed in tree order).
            found
                .iter()
                .filter(|anchor| {
                    anchor.anchoring.names.iter().any(|known| known == name)
                        && (anchor.node.is_none() || anchor.node != own_node)
                        // Hidden by a scope that the positioned element is not inside.
                        && anchor.scopes.iter().all(|(scope_node, scope)| {
                            !scope.covers(name)
                                || scope_node.is_some_and(|scope_node| own_ancestors.contains(&scope_node))
                        })
                })
                .enumerate()
                .max_by_key(|(index, anchor)| (anchor.node, *index))
                .map(|(_, anchor)| anchor)
        };
        let lookup = |name: Option<&str>| -> Option<AnchorRect> {
            let anchor = find(name)?;
            Some(AnchorRect {
                x: (anchor.rect.x - origin.0).to_px(),
                y: (anchor.rect.y - origin.1).to_px(),
                width: anchor.rect.width.to_px(),
                height: anchor.rect.height.to_px(),
            })
        };
        let base = Placement::of(style, anchoring);
        let options: Vec<Placement> = std::iter::once(base.clone())
            .chain(anchoring.fallbacks.iter().filter_map(|option| base.tried(option)))
            .collect();
        let place = |placement: &Placement| {
            self.place_anchored(id, style, placement, &lookup, (width, height), static_position)
        };
        let (mut fragment, fits) = if anchoring.order == TryOrder::Normal {
            let mut first = None;
            let mut chosen = None;
            for placement in &options {
                let (fragment, fits, _) = place(placement);
                if fits {
                    chosen = Some((fragment, true));
                    break;
                }
                first.get_or_insert(fragment);
            }
            chosen.or_else(|| first.map(|fragment| (fragment, false)))?
        } else {
            // All options, the largest inset-modified containing blocks along the axis first.
            let mut placed: Vec<(Fragment, bool, Rect)> = options.iter().map(place).collect();
            let first = placed.first().map(|(fragment, ..)| fragment.clone())?;
            let along = |rect: &Rect| match anchoring.order {
                TryOrder::MostWidth => rect.width,
                _ => rect.height,
            };
            placed.sort_by_key(|(_, _, imcb)| std::cmp::Reverse(along(imcb)));
            placed
                .into_iter()
                .find(|(_, fits, _)| *fits)
                .map_or((first, false), |(fragment, ..)| (fragment, true))
        };
        let visibility = anchoring.visibility;
        let anchor_hidden = visibility.anchors_visible
            && find(anchoring.default.as_deref()).is_some_and(|anchor| {
                anchor
                    .clips
                    .iter()
                    .filter(|(node, _)| node.is_none_or(|node| !own_ancestors.contains(&node)))
                    .any(|(_, clip)| !overlaps(clip, &anchor.rect))
            });
        if anchor_hidden || (visibility.no_overflow && !fits) {
            hide(&mut fragment);
        }
        Some(fragment)
    }

    /// Lays out a box with one placement; also tells whether its margin box fits in its
    /// inset-modified containing block, and that block.
    fn place_anchored(
        &self,
        id: BoxId,
        style: &ComputedStyle,
        placement: &Placement,
        lookup: &dyn Fn(Option<&str>) -> Option<AnchorRect>,
        (cb_width, cb_height): (Au, Au),
        static_position: (Au, Au),
    ) -> (Fragment, bool, Rect) {
        let style = placement.style.as_deref().unwrap_or(style);
        let lookup = |name: Option<&str>| lookup(name.or(placement.default.as_deref()));
        let environment = Environment {
            anchor: &lookup,
            containing: (cb_width.to_px(), cb_height.to_px()),
        };
        let resolve = |value: &Value<LengthPercentageAuto>, slot: Slot| match value {
            Value::Own(own) => own.clone(),
            Value::Anchored(tokens) => fagus_style::anchor::evaluate(tokens, slot, &environment)
                .map_or(LengthPercentageAuto::Auto, |px| {
                    LengthPercentageAuto::Length(LengthPercentage::Px(px))
                }),
        };
        let mut used = style.clone();
        used.inset.top = resolve(&placement.insets[0], Slot::Inset(0));
        used.inset.right = resolve(&placement.insets[1], Slot::Inset(1));
        used.inset.bottom = resolve(&placement.insets[2], Slot::Inset(2));
        used.inset.left = resolve(&placement.insets[3], Slot::Inset(3));
        let margin_slot = Slot::Size { horizontal: true };
        used.margin.top = resolve(&placement.margins[0], margin_slot);
        used.margin.right = resolve(&placement.margins[1], margin_slot);
        used.margin.bottom = resolve(&placement.margins[2], margin_slot);
        used.margin.left = resolve(&placement.margins[3], margin_slot);
        let size = |index: usize| {
            placement.sizes[index].as_ref().map(|tokens| {
                fagus_style::anchor::evaluate(
                    tokens,
                    Slot::Size {
                        horizontal: index.is_multiple_of(2),
                    },
                    &environment,
                )
            })
        };
        if placement.transposed {
            std::mem::swap(&mut used.width, &mut used.height);
            std::mem::swap(&mut used.min_width, &mut used.min_height);
            std::mem::swap(&mut used.max_width, &mut used.max_height);
        }
        let length = |px: f32| LengthPercentage::Px(px);
        for (index, target) in [&mut used.width, &mut used.height].into_iter().enumerate() {
            if let Some(value) = size(index) {
                *target = value.map_or(Size::Auto, |px| Size::Length(length(px)));
            }
        }
        for (index, target) in [&mut used.min_width, &mut used.min_height]
            .into_iter()
            .enumerate()
        {
            if let Some(value) = size(index + 2) {
                *target = value.map_or(Size::Auto, |px| Size::Length(length(px)));
            }
        }
        for (index, target) in [&mut used.max_width, &mut used.max_height]
            .into_iter()
            .enumerate()
        {
            if let Some(value) = size(index + 4) {
                *target = value.map_or(MaxSize::None, |px| MaxSize::Length(length(px)));
            }
        }
        let inset = |value: &LengthPercentageAuto, basis: Au| match value {
            LengthPercentageAuto::Auto => None,
            LengthPercentageAuto::Length(length) => Some(Au::from_px(length.resolve(basis.to_px()))),
        };
        let anchor = lookup(None);
        if let (Some(area), Some(anchor)) = (placement.area, anchor) {
            {
                // The area's region of the grid the anchor's edges make, less the insets.
                let edges = |start: f32, size: f32, extent: Au| {
                    let extent = extent.to_px();
                    [
                        0.0,
                        start.clamp(0.0, extent),
                        (start + size).clamp(0.0, extent),
                        extent,
                    ]
                };
                let columns = edges(anchor.x, anchor.width, cb_width);
                let rows = edges(anchor.y, anchor.height, cb_height);
                let region_x = Au::from_px(columns[usize::from(area.columns.0)]);
                let region_right = Au::from_px(columns[usize::from(area.columns.1) + 1]);
                let region_y = Au::from_px(rows[usize::from(area.rows.0)]);
                let region_bottom = Au::from_px(rows[usize::from(area.rows.1) + 1]);
                let region_width = (region_right - region_x).clamp_non_negative();
                let region_height = (region_bottom - region_y).clamp_non_negative();
                let top = inset(&used.inset.top, region_height).unwrap_or(Au::ZERO);
                let right = inset(&used.inset.right, region_width).unwrap_or(Au::ZERO);
                let bottom = inset(&used.inset.bottom, region_height).unwrap_or(Au::ZERO);
                let left = inset(&used.inset.left, region_width).unwrap_or(Au::ZERO);
                let imcb = Rect::new(
                    region_x + left,
                    region_y + top,
                    (region_width - left - right).clamp_non_negative(),
                    (region_height - top - bottom).clamp_non_negative(),
                );
                let align = [
                    area_alignment(area.columns, placement.align[0], placement.center[0]),
                    area_alignment(area.rows, placement.align[1], placement.center[1]),
                ];
                let zero = || LengthPercentageAuto::Length(LengthPercentage::Px(0.0));
                let sides = |align: Align| match align {
                    Align::Start | Align::Center | Align::Anchor => (zero(), LengthPercentageAuto::Auto),
                    Align::End => (LengthPercentageAuto::Auto, zero()),
                    Align::Stretch => (zero(), zero()),
                };
                (used.inset.left, used.inset.right) = sides(align[0]);
                (used.inset.top, used.inset.bottom) = sides(align[1]);
                let mut fragment =
                    self.absolute_styled(id, imcb.width, imcb.height, (Au::ZERO, Au::ZERO), &used);
                let anchor_center = (
                    Au::from_px(anchor.x + anchor.width / 2.0) - imcb.x,
                    Au::from_px(anchor.y + anchor.height / 2.0) - imcb.y,
                );
                fragment.rect.x = aligned(
                    align[0],
                    fragment.rect.x,
                    fragment.rect.width,
                    imcb.width,
                    anchor_center.0,
                );
                fragment.rect.y = aligned(
                    align[1],
                    fragment.rect.y,
                    fragment.rect.height,
                    imcb.height,
                    anchor_center.1,
                );
                fragment.rect.x += imcb.x;
                fragment.rect.y += imcb.y;
                // Centered on the anchor, the box stays inside the containing block.
                if align[0] == Align::Anchor {
                    fragment.rect.x = kept_inside(fragment.rect.x, fragment.rect.width, cb_width);
                }
                if align[1] == Align::Anchor {
                    fragment.rect.y = kept_inside(fragment.rect.y, fragment.rect.height, cb_height);
                }
                let fits = fits(&fragment, imcb);
                (fragment, fits, imcb)
            }
        } else {
            {
                let mut fragment = self.absolute_styled(id, cb_width, cb_height, static_position, &used);
                let left = inset(&used.inset.left, cb_width);
                let right = inset(&used.inset.right, cb_width);
                let top = inset(&used.inset.top, cb_height);
                let bottom = inset(&used.inset.bottom, cb_height);
                if let Some(anchor) = anchor {
                    if placement.center[0] && !(left.is_some() && right.is_some()) {
                        let center = Au::from_px(anchor.x + anchor.width / 2.0);
                        fragment.rect.x =
                            kept_inside(center - fragment.rect.width / 2, fragment.rect.width, cb_width);
                    }
                    if placement.center[1] && !(top.is_some() && bottom.is_some()) {
                        let center = Au::from_px(anchor.y + anchor.height / 2.0);
                        fragment.rect.y =
                            kept_inside(center - fragment.rect.height / 2, fragment.rect.height, cb_height);
                    }
                }
                let imcb = Rect::new(
                    left.unwrap_or(Au::ZERO),
                    top.unwrap_or(Au::ZERO),
                    (cb_width - left.unwrap_or(Au::ZERO) - right.unwrap_or(Au::ZERO)).clamp_non_negative(),
                    (cb_height - top.unwrap_or(Au::ZERO) - bottom.unwrap_or(Au::ZERO)).clamp_non_negative(),
                );
                let fits = fits(&fragment, imcb);
                (fragment, fits, imcb)
            }
        }
    }
}

/// Whether two rectangles overlap.
fn overlaps(a: &Rect, b: &Rect) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

/// Hides a box and all inside it (`position-visibility`): it keeps its place, but nothing of
/// it is drawn.
fn hide(fragment: &mut Fragment) {
    fragment.children.clear();
    if let FragmentKind::Box { style, .. } = &mut fragment.kind {
        *style = Arc::new(ComputedStyle {
            visibility: Visibility::Hidden,
            ..(**style).clone()
        });
    }
}

/// The position of a box `size` long aligned in a space `room` long; `placed` is where layout
/// put it (at the start or end already).
fn aligned(align: Align, placed: Au, size: Au, room: Au, anchor_center: Au) -> Au {
    match align {
        Align::Center => (room - size) / 2,
        Align::Anchor => anchor_center - size / 2,
        _ => placed,
    }
}

/// A position moved so that a box `size` long stays in `0..extent` where it fits.
fn kept_inside(position: Au, size: Au, extent: Au) -> Au {
    if size > extent {
        return position;
    }
    position.max(Au::ZERO).min(extent - size)
}

/// Whether a fragment's border box lies inside a rectangle (with a little tolerance).
fn fits(fragment: &Fragment, area: Rect) -> bool {
    let slack = Au::from_px(0.5);
    fragment.rect.x >= area.x - slack
        && fragment.rect.y >= area.y - slack
        && fragment.rect.x + fragment.rect.width <= area.x + area.width + slack
        && fragment.rect.y + fragment.rect.height <= area.y + area.height + slack
}

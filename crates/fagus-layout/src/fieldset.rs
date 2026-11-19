//! The border of a fieldset with a rendered legend: as Chrome paints it, the top border runs
//! through the middle of the legend, with a gap for the legend (HTML, §15.3.13).

use std::sync::Arc;

use fagus_dom::NodeId;
use fagus_style::values::{Rgba, Sides};
use fagus_style::{ComputedStyle, ComputedValues};

use crate::fragment::{Children, Fragment, FragmentKind, VisibleSides};
use crate::units::{Au, Rect};

/// Redraws the border of a fieldset's fragment around its legend: the fragment keeps its place
/// and size but draws nothing itself; a fragment of its background and its other borders starts
/// where the top border is pushed down to, the middle of the legend, and two fragments draw the
/// top border to the left and right of the legend.
pub(crate) fn cut_legend_gap(fragment: &mut Fragment, legend: NodeId) {
    let FragmentKind::Box { style, node, sides } = &fragment.kind else {
        return;
    };
    let top = style.border_width.top;
    if !sides.top || top <= 0.0 {
        return;
    }
    let Some(found) = find(fragment, legend, Au::ZERO, Au::ZERO, 3) else {
        return;
    };
    let (style, node, sides) = (Arc::clone(style), *node, *sides);
    let width = fragment.rect.width;
    let center = found.y + found.height / 2;
    let offset = (center - Au::from_px(top) / 2).clamp_non_negative();
    let mut frame = (*style).clone();
    frame.outline_width = 0.0;
    frame.box_shadow = Arc::from([]);
    // The fieldset's fragment keeps what makes it a stacking context or clips: its frame,
    // painted first inside it, is a plain box under the legend and the content (as with
    // containment, WPT `contain-layout-baseline-005`).
    frame.position = fagus_style::values::Position::Static;
    frame.z_index = None;
    frame.opacity = 1.0;
    frame.transform = Arc::from([]);
    frame.translate = None;
    frame.rotate = None;
    frame.scale = None;
    frame.filter = Arc::from([]);
    frame.clip_path = None;
    frame.mask = None;
    frame.perspective = None;
    frame.mix_blend_mode = fagus_style::values::BlendMode::Normal;
    frame.isolation = fagus_style::values::Isolation::Auto;
    frame.contain_layout = false;
    frame.layout_containment = false;
    frame.will_change_contains = false;
    frame.backdrop_filter = false;
    frame.preserve_3d = false;
    frame.overflow_x = fagus_style::values::Overflow::Visible;
    frame.overflow_y = fagus_style::values::Overflow::Visible;
    let frame = Fragment {
        rect: Rect::new(
            Au::ZERO,
            offset,
            width,
            (fragment.rect.height - offset).clamp_non_negative(),
        ),
        kind: FragmentKind::Box {
            style: Arc::new(frame),
            node: None,
            sides: VisibleSides { top: false, ..sides },
        },
        children: Children::new(),
    };
    let mut border = ComputedStyle::from(ComputedValues {
        color: style.color,
        border_width: Sides::all(0.0),
        ..ComputedValues::default()
    });
    border.border_width.top = top;
    border.border_style.top = style.border_style.top;
    border.border_color.top = Some(style.border_color.top.unwrap_or(style.color));
    let border = Arc::new(border);
    let piece = |x: Au, width: Au| Fragment {
        rect: Rect::new(x, offset, width, Au::from_px(top)),
        kind: FragmentKind::Box {
            style: Arc::clone(&border),
            node: None,
            sides: VisibleSides::ALL,
        },
        children: Children::new(),
    };
    let mut pieces = vec![frame];
    if found.x > Au::ZERO {
        pieces.push(piece(Au::ZERO, found.x));
    }
    let end = found.x + found.width;
    if end < width {
        pieces.push(piece(end, width - end));
    }
    // The fieldset itself draws neither background nor border any more.
    let mut own = (*style).clone();
    own.border_width = Sides::all(0.0);
    own.background_color = Rgba::TRANSPARENT;
    own.background_image = Arc::from([]);
    own.box_shadow = Arc::from([]);
    fragment.kind = FragmentKind::Box {
        style: Arc::new(own),
        node,
        sides,
    };
    fragment.children.splice(0..0, pieces);
}

/// The border box of the legend's fragment among the first children of a fragment, relative to
/// it: in the box that holds it, in its line.
fn find(fragment: &Fragment, legend: NodeId, x: Au, y: Au, depth: usize) -> Option<Rect> {
    for child in &fragment.children {
        let (child_x, child_y) = (x + child.rect.x, y + child.rect.y);
        if let FragmentKind::Box { node: Some(node), .. } = &child.kind
            && *node == legend
        {
            return Some(Rect::new(child_x, child_y, child.rect.width, child.rect.height));
        }
        if depth > 0
            && matches!(
                child.kind,
                FragmentKind::Anonymous | FragmentKind::Line { .. } | FragmentKind::Box { node: None, .. }
            )
            && let Some(found) = find(child, legend, child_x, child_y, depth - 1)
        {
            return Some(found);
        }
    }
    None
}

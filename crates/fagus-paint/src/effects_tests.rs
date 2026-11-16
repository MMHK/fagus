//! Transforms in 3D, backdrop filters, backgrounds clipped to no text, hidden root elements,
//! zoomed clip paths and the order of nested outlines, as Chrome 157 prints them.

use std::sync::Arc;

use fagus_dom::NodeId;
use fagus_layout::units::{Au, Rect};
use fagus_layout::{Fragment, FragmentKind, Page, VisibleSides};
use fagus_style::ComputedStyle;
use fagus_style::values::{Display, Rgba, TransformFunction, Visibility};
use fagus_text::FontDb;

use crate::{Item, paint};

/// A page of 300 × 100 with `fragments`.
fn page(fragments: Vec<Fragment>) -> Page {
    Page {
        width: Au::from_px(300.0),
        height: Au::from_px(100.0),
        fragments,
        margin: [Au::ZERO; 4],
        style: Arc::new(ComputedStyle::default()),
        canvas: None,
        bleed: 0.0,
        marks: fagus_layout::PageMarks::default(),
        orientation: fagus_layout::PageOrientation::Upright,
        scale: 1.0,
        area: Rect::default(),
        shift: 0.0,
        carried: Vec::new(),
    }
}

/// A box of an element with `style` at `(x, 0)`, `size` × `size`.
fn element(style: ComputedStyle, x: f32, size: f32, children: Vec<Fragment>) -> Fragment {
    Fragment {
        rect: Rect::new(Au::from_px(x), Au::ZERO, Au::from_px(size), Au::from_px(size)),
        kind: FragmentKind::Box {
            style: Arc::new(style),
            node: Some(NodeId::DOCUMENT),
            sides: VisibleSides::ALL,
        },
        children: children.into(),
    }
}

fn items(page: Page) -> Vec<Item> {
    let fonts = FontDb::new();
    let images = fagus_layout::Images::default();
    paint(&[page], &fonts, &images, false).remove(0).items
}

fn green() -> ComputedStyle {
    ComputedStyle {
        display: Display::BLOCK,
        background_color: Rgba::from_u8(0, 128, 0, 1.0),
        ..ComputedStyle::default()
    }
}

/// The transforms pushed, in order.
fn transforms(items: &[Item]) -> Vec<[f32; 6]> {
    items
        .iter()
        .filter_map(|item| match item {
            Item::PushTransform(matrix) => Some(*matrix),
            _ => None,
        })
        .collect()
}

#[test]
fn composes_the_transforms_of_a_3d_space() {
    // A box turned a quarter about the x axis keeps the space for its child, turned a quarter
    // more: together half a turn, a box upside down rather than an edge.
    let turned = |preserve_3d: bool, parent_preserve_3d: bool| ComputedStyle {
        transform: Arc::from([TransformFunction::Rotate3d(1.0, 0.0, 0.0, 90.0)]),
        preserve_3d,
        parent_preserve_3d,
        ..green()
    };
    let scene = |child_in_space: bool| {
        page(vec![element(
            turned(true, false),
            0.0,
            50.0,
            vec![element(turned(false, child_in_space), 0.0, 50.0, Vec::new())],
        )])
    };
    let found = transforms(&items(scene(true)));
    assert!(
        found
            .iter()
            .any(|m| (m[3] + 1.0).abs() < 1e-4 && (m[0] - 1.0).abs() < 1e-4),
        "{found:?}"
    );
    // The child of another element (a box laid out in it from elsewhere) is flattened in
    // its plane, an edge on edge.
    let found = transforms(&items(scene(false)));
    assert!(found.iter().all(|m| m[3].abs() < 1e-4), "{found:?}");
}

#[test]
fn sees_only_children_of_the_element_in_its_perspective() {
    let viewer = ComputedStyle {
        perspective: Some(100.0),
        ..green()
    };
    let pushed_back = |parent_perspective: bool| ComputedStyle {
        transform: Arc::from([TransformFunction::TranslateZ(-100.0)]),
        parent_perspective,
        ..green()
    };
    let scale = |parent_perspective: bool| {
        let found = transforms(&items(page(vec![element(
            viewer.clone(),
            0.0,
            100.0,
            vec![element(pushed_back(parent_perspective), 0.0, 100.0, Vec::new())],
        )])));
        found.last().map(|m| m[0])
    };
    // Twice as far away, half as large.
    assert!(scale(true).is_some_and(|scale| (scale - 0.5).abs() < 1e-4));
    assert!(scale(false).is_some_and(|scale| (scale - 1.0).abs() < 1e-4));
}

#[test]
fn draws_a_box_with_a_backdrop_filter_as_a_group() {
    let filtered = ComputedStyle {
        backdrop_filter: true,
        ..green()
    };
    let found = items(page(vec![element(filtered, 0.0, 50.0, Vec::new())]));
    assert!(found.iter().any(|item| matches!(item, Item::PushIsolated)));
}

#[test]
fn draws_a_background_clipped_to_no_text_whole() {
    let clipped = ComputedStyle {
        background_clip_text: Arc::from([true]),
        ..green()
    };
    let found = items(page(vec![element(clipped, 0.0, 50.0, Vec::new())]));
    assert!(
        found
            .iter()
            .any(|item| matches!(item, Item::Rect { color, .. } if color.g > 0.4))
    );
    assert!(!found.iter().any(|item| matches!(item, Item::PushMask(_))));
}

#[test]
fn leaves_the_canvas_of_a_hidden_root_empty() {
    let canvas = |visibility: Visibility| {
        let mut page = page(vec![element(
            ComputedStyle {
                visibility,
                ..ComputedStyle::default()
            },
            0.0,
            50.0,
            Vec::new(),
        )]);
        page.canvas = Some(Arc::new(ComputedStyle {
            background_color: Rgba::from_u8(255, 165, 0, 1.0),
            ..ComputedStyle::default()
        }));
        items(page)
            .iter()
            .any(|item| matches!(item, Item::Rect { color, .. } if color.r > 0.9 && color.b < 0.1))
    };
    assert!(canvas(Visibility::Visible));
    assert!(!canvas(Visibility::Hidden));
}

#[test]
fn zooms_the_pixels_of_a_clip_path() {
    use fagus_layout::PathSegment;
    let segments = vec![PathSegment::Move(0.0, 0.0), PathSegment::Line(100.0, 50.0)];
    let zoomed = crate::zoomed_path(segments, 2.0, (10.0, 20.0));
    assert_eq!(zoomed[1], PathSegment::Line(210.0, 120.0));
}

#[test]
fn paints_the_outline_of_a_box_over_those_inside_it() {
    // Blink paints a box's descendant outlines before its own: where they meet, the outer
    // outline is on top (WPT css-multicol multicol-span-all-list-item-001; Chrome 157).
    let outlined = |red: u8| ComputedStyle {
        display: Display::BLOCK,
        outline_width: 4.0,
        outline_style: fagus_style::values::OutlineStyle::Line(fagus_style::values::BorderStyle::Solid),
        outline_color: Some(Rgba::from_u8(red, 0, 0, 1.0)),
        ..ComputedStyle::default()
    };
    let inner = element(outlined(100), 0.0, 50.0, Vec::new());
    let colors: Vec<u8> = items(page(vec![element(outlined(200), 0.0, 50.0, vec![inner])]))
        .iter()
        .filter_map(|item| match item {
            Item::BorderSide { color, .. } | Item::RoundedBorder { color, .. } => {
                Some((color.r * 255.0).round() as u8)
            }
            _ => None,
        })
        .collect();
    assert_eq!(colors.first(), Some(&100), "{colors:?}");
    assert_eq!(colors.last(), Some(&200), "{colors:?}");
}

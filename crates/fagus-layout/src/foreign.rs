//! SVG `<foreignObject>`: the HTML inside an inline `<svg>` is laid out as a box of the size the
//! element gives it, and drawn where the SVG puts the element, in the SVG's coordinates, as in
//! Chrome. The box is a child of the SVG's box, transformed into place and clipped to its size
//! and to the SVG. It is drawn in document order with the SVG's own content: where shapes
//! follow it, the SVG is drawn in layers (see [`ForeignObjects::layers`]) between the boxes.

use std::sync::Arc;

use fagus_dom::NodeId;
use fagus_style::values::TransformFunction;
use fagus_style::{ComputedStyle, ComputedValues};

use crate::block::Layouter;
use crate::fragment::{Children, Fragment, FragmentKind};
use crate::tree::BoxId;
use crate::units::Au;

/// How an SVG fills a box of another aspect ratio than its own: an SVG with a `viewBox` keeps
/// its aspect ratio as its root's `preserveAspectRatio` says, others stretch, but an inline SVG
/// without a `viewBox` keeps its scale, as in browsers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SvgFit {
    Stretch,
    /// Drawn at its own size from the top left corner of the box.
    Unscaled,
    /// Scaled to fit inside the box (`meet`) or to cover it (`slice`), aligned by the fractions
    /// of the free space left of and above it.
    Uniform {
        cover: bool,
        x: f32,
        y: f32,
    },
}

impl SvgFit {
    /// Where the SVG goes in a box of `width` × `height`, scaled from its size: the offset and
    /// the size it is drawn at.
    pub fn place(self, size: (f32, f32), width: f32, height: f32) -> (f32, f32, f32, f32) {
        let (cover, x, y) = match self {
            Self::Stretch => return (0.0, 0.0, width, height),
            Self::Unscaled => return (0.0, 0.0, size.0, size.1),
            Self::Uniform { cover, x, y } => (cover, x, y),
        };
        if size.0 <= 0.0 || size.1 <= 0.0 {
            return (0.0, 0.0, width, height);
        }
        let (across, down) = (width / size.0, height / size.1);
        let scale = if cover { across.max(down) } else { across.min(down) };
        let (drawn_width, drawn_height) = (size.0 * scale, size.1 * scale);
        (
            (width - drawn_width) * x,
            (height - drawn_height) * y,
            drawn_width,
            drawn_height,
        )
    }
}

/// The `<foreignObject>` elements of an inline `<svg>` and where the SVG puts them.
#[derive(Clone, Debug, PartialEq)]
pub struct ForeignObjects {
    /// The size of the SVG as it is drawn, which `fit` places in its box.
    pub size: (f32, f32),
    pub fit: SvgFit,
    /// The elements in the order the SVG draws them.
    pub objects: Vec<ForeignObject>,
    /// The SVG in layers: the first with what it draws before the first element, then one with
    /// what it draws after each element up to the next; empty where nothing follows an element
    /// (the whole SVG is drawn below them).
    pub layers: Vec<Arc<crate::Image>>,
}

/// A `<foreignObject>` element as the SVG places it.
#[derive(Clone, Debug, PartialEq)]
pub struct ForeignObject {
    pub node: NodeId,
    /// `[a, b, c, d, e, f]` from the element's user space to the SVG of [`ForeignObjects::size`].
    pub transform: [f32; 6],
    /// Its `x`, `y`, `width` and `height` in its user space.
    pub rect: [f32; 4],
}

impl ForeignObjects {
    /// The transform from the top left corner of the box of `object` to the content box of the
    /// SVG, `width` × `height`.
    pub fn placed(&self, object: &ForeignObject, width: f32, height: f32) -> [f32; 6] {
        let corner = [1.0, 0.0, 0.0, 1.0, object.rect[0], object.rect[1]];
        multiply(&self.around(object, width, height), &corner)
    }

    /// The transform around the box of `object` placed at its `x` and `y` in its user space:
    /// the SVG's fit and the transforms of the element and its ancestors.
    fn around(&self, object: &ForeignObject, width: f32, height: f32) -> [f32; 6] {
        let (x, y, drawn_width, drawn_height) = self.fit.place(self.size, width, height);
        let (sx, sy) = if self.size.0 > 0.0 && self.size.1 > 0.0 {
            (drawn_width / self.size.0, drawn_height / self.size.1)
        } else {
            (1.0, 1.0)
        };
        let place = [sx, 0.0, 0.0, sy, x, y];
        multiply(&place, &object.transform)
    }

    /// Where the box of `object` goes in the SVG's content box, `width` × `height`: the
    /// offset of its corner and the transform from there. Blink takes the element's `x` and
    /// `y` into the box's position in layout units, inside the SVG's fit and the transforms of
    /// the element and its ancestors: where these only move it, the box lies at its `x` and
    /// `y`, so that the lines inside snap to whole pixels with them but not with the
    /// transforms; otherwise at the corner of the content box, transformed.
    pub fn located(&self, object: &ForeignObject, width: f32, height: f32) -> ((Au, Au), [f32; 6]) {
        let corner = (Au::from_px(object.rect[0]), Au::from_px(object.rect[1]));
        let around = self.around(object, width, height);
        if [around[0] - 1.0, around[1], around[2], around[3] - 1.0]
            .iter()
            .all(|value| value.abs() < 1e-6)
        {
            return (corner, around);
        }
        let moved = [1.0, 0.0, 0.0, 1.0, corner.0.to_px(), corner.1.to_px()];
        ((Au::ZERO, Au::ZERO), multiply(&around, &moved))
    }
}

/// The transform `outer` after `inner`: `inner` applies to a point first.
fn multiply(outer: &[f32; 6], inner: &[f32; 6]) -> [f32; 6] {
    [
        outer[0] * inner[0] + outer[2] * inner[1],
        outer[1] * inner[0] + outer[3] * inner[1],
        outer[0] * inner[2] + outer[2] * inner[3],
        outer[1] * inner[2] + outer[3] * inner[3],
        outer[0] * inner[4] + outer[2] * inner[5] + outer[4],
        outer[1] * inner[4] + outer[3] * inner[5] + outer[5],
    ]
}

/// The boxes of the `<foreignObject>` elements of an inline `<svg>`'s box, with where the SVG
/// puts them.
#[derive(Debug)]
pub(crate) struct ForeignBoxes {
    pub(crate) objects: Arc<ForeignObjects>,
    /// Each box with the index of its element in [`ForeignObjects::objects`].
    pub(crate) boxes: Vec<(BoxId, usize)>,
}

impl Layouter<'_> {
    /// The SVG layer of an inline `<svg>` box (see [`ForeignObjects::layers`]) that goes
    /// over the box of an element: the content box `rect` drawn as a box of its own, transformed
    /// in place, so that it is painted in order with the elements' boxes. `svg` is the SVG
    /// box's style, whose zoom and fit it keeps.
    fn layer(svg: &ComputedStyle, image: &Arc<crate::Image>, rect: crate::units::Rect) -> Fragment {
        let style = ComputedStyle::from(ComputedValues {
            zoom: svg.zoom,
            object_fit: svg.object_fit,
            object_position: svg.object_position.clone(),
            visibility: svg.visibility,
            transform: Arc::from([TransformFunction::Matrix([1.0, 0.0, 0.0, 1.0, 0.0, 0.0])]),
            ..ComputedValues::default()
        });
        Fragment {
            rect,
            kind: FragmentKind::Box {
                style: Arc::new(style),
                node: None,
                sides: crate::fragment::VisibleSides::ALL,
            },
            children: vec![Fragment {
                rect: crate::units::Rect::new(Au::ZERO, Au::ZERO, rect.width, rect.height),
                kind: FragmentKind::Image(Arc::clone(image)),
                children: Children::new(),
            }]
            .into(),
        }
    }

    /// The SVG layer drawn below the `<foreignObject>` boxes of the inline `<svg>` box `id`, in
    /// place of the whole SVG, where it is drawn in layers.
    pub(crate) fn first_svg_layer(&self, id: BoxId) -> Option<Arc<crate::Image>> {
        let foreign = self.tree.foreign.get(&id)?;
        foreign.objects.layers.first().cloned()
    }

    /// The fragments of the `<foreignObject>` boxes of the inline `<svg>` box `id`, whose
    /// content box lies at `origin` in its fragment and is `width` × `height`: each laid out at
    /// the size of its element, from the content box's corner, and transformed into place,
    /// with the layers of the SVG after each where it is drawn in layers.
    pub(crate) fn foreign_objects(
        &self,
        id: BoxId,
        origin: (Au, Au),
        width: Au,
        height: Au,
    ) -> Vec<Fragment> {
        let Some(foreign) = self.tree.foreign.get(&id) else {
            return Vec::new();
        };
        let layered = foreign.objects.layers.len() == foreign.objects.objects.len() + 1;
        let svg = &self.tree.get(id).style;
        let content = crate::units::Rect::new(origin.0, origin.1, width, height);
        let mut boxes = foreign.boxes.clone();
        boxes.sort_by_key(|&(_, index)| index);
        let mut fragments = Vec::with_capacity(boxes.len() * 2);
        let mut next_layer = 1;
        for (object_box, index) in boxes {
            // The layers of the elements before this one that have no box.
            while layered && next_layer <= index {
                fragments.push(Self::layer(svg, &foreign.objects.layers[next_layer], content));
                next_layer += 1;
            }
            let Some(object) = foreign.objects.objects.get(index) else {
                continue;
            };
            let [_, _, object_width, object_height] = object.rect;
            let mut fragment = self.absolute(
                object_box,
                Au::from_px(object_width),
                Au::from_px(object_height),
                (Au::ZERO, Au::ZERO),
            );
            let (corner, matrix) = foreign.objects.located(object, width.to_px(), height.to_px());
            fragment.rect.x = origin.0 + corner.0;
            fragment.rect.y = origin.1 + corner.1;
            if let FragmentKind::Box { style, .. } = &mut fragment.kind {
                *style = Arc::new(ComputedStyle::from(ComputedValues {
                    transform: Arc::from([TransformFunction::Matrix(matrix)]),
                    ..ComputedValues::clone(style)
                }));
            }
            fragments.push(fragment);
            if layered {
                fragments.push(Self::layer(svg, &foreign.objects.layers[index + 1], content));
                next_layer = index + 2;
            }
        }
        while layered && next_layer < foreign.objects.layers.len() {
            fragments.push(Self::layer(svg, &foreign.objects.layers[next_layer], content));
            next_layer += 1;
        }
        fragments
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_foreign_objects_in_the_svg_box() {
        let objects = ForeignObjects {
            size: (200.0, 100.0),
            fit: SvgFit::Uniform {
                cover: false,
                x: 0.5,
                y: 0.5,
            },
            objects: vec![ForeignObject {
                node: NodeId::DOCUMENT,
                transform: [1.0, 0.0, 0.0, 1.0, 20.0, 10.0],
                rect: [5.0, 6.0, 50.0, 20.0],
            }],
            layers: Vec::new(),
        };
        // Half the size, centered in a box twice as high as the SVG drawn: 25 pixels down.
        let placed = objects.placed(&objects.objects[0], 100.0, 100.0);
        assert_eq!(placed, [0.5, 0.0, 0.0, 0.5, 12.5, 33.0]);
        let unscaled = ForeignObjects {
            fit: SvgFit::Unscaled,
            ..objects.clone()
        };
        assert_eq!(
            unscaled.placed(&unscaled.objects[0], 10.0, 10.0),
            [1.0, 0.0, 0.0, 1.0, 25.0, 16.0]
        );
    }

    #[test]
    fn takes_x_and_y_into_the_position_in_layout_units() {
        // Chrome moves the box of `<foreignObject x="20.4" y="0.6">` by layout units (20.4
        // becomes 20.390625), so that its lines snap with it; the transform around it stays.
        let objects = ForeignObjects {
            size: (100.0, 50.0),
            fit: SvgFit::Unscaled,
            objects: vec![ForeignObject {
                node: NodeId::DOCUMENT,
                transform: [1.0, 0.0, 0.0, 1.0, 5.0, 0.25],
                rect: [20.4, 0.6, 50.0, 20.0],
            }],
            layers: Vec::new(),
        };
        let (corner, matrix) = objects.located(&objects.objects[0], 100.0, 50.0);
        assert_eq!((corner.0.to_px(), corner.1.to_px()), (20.390_625, 0.593_75));
        assert_eq!(matrix, [1.0, 0.0, 0.0, 1.0, 5.0, 0.25]);
        // Scaled by the SVG's fit: at the corner of the content box, `x` and `y` scaled too.
        let scaled = ForeignObjects {
            fit: SvgFit::Stretch,
            objects: vec![ForeignObject {
                transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                rect: [10.0, 4.0, 50.0, 20.0],
                ..objects.objects[0].clone()
            }],
            ..objects
        };
        let (corner, matrix) = scaled.located(&scaled.objects[0], 200.0, 100.0);
        assert_eq!(corner, (Au::ZERO, Au::ZERO));
        assert_eq!(matrix, [2.0, 0.0, 0.0, 2.0, 20.0, 8.0]);
    }
}

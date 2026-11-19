//! Transform matrices (CSS Transforms 1 and 2), composed in 3D and flattened onto the page.

use fagus_style::ComputedStyle;
use fagus_style::values::TransformFunction;

/// `[a, b, c, d, e, f]`: `(x, y)` maps to `(a·x + c·y + e, b·x + d·y + f)`.
type Matrix = [f32; 6];

/// A 3D transform, column by column as `matrix3d()` writes it: `(x, y, z, w)` maps to
/// `(m[0]·x + m[4]·y + m[8]·z + m[12]·w, …)`.
pub(crate) type Matrix3d = [f32; 16];

/// The perspective a box's parent gives it (`perspective`): the distance to the viewer and the
/// point on the page the viewer looks from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Perspective {
    pub(crate) distance: f32,
    pub(crate) origin: (f32, f32),
}

/// The transform of a box whose border box is at `(x, y)` with the given size: the individual
/// `translate`, `rotate` and `scale`, then `transform`, all around `transform-origin`, seen
/// with the perspective of its parent (CSS Transforms 2, §6), flattened onto the page. Without
/// perspective a flattened 3D transform is exact; with it, the box is drawn in the parallelogram
/// closest to the shape its corners take. `None` where the box lies behind the viewer.
///
/// The reference box is the border box, or the content box for `transform-box: content-box`
/// (and `fill-box`, which is the content box of a CSS box).
pub(crate) fn matrix(
    style: &ComputedStyle,
    rect: (f32, f32, f32, f32),
    perspective: Option<Perspective>,
) -> Option<Matrix> {
    let (matrix, reference) = matrix3d(style, rect, perspective);
    flatten(&matrix, reference)
}

/// The transform that applies `inner`, then `outer`.
#[allow(clippy::many_single_char_names, reason = "the entries of the matrices")]
pub(crate) fn then(outer: Matrix, inner: Matrix) -> Matrix {
    let [a, b, c, d, e, f] = outer;
    let [ia, ib, ic, id, ie, iff] = inner;
    [
        a * ia + c * ib,
        b * ia + d * ib,
        a * ic + c * id,
        b * ic + d * id,
        a * ie + c * iff + e,
        b * ie + d * iff + f,
    ]
}

/// [`matrix`] before it is flattened, with its reference box: the transform in 3D that the
/// boxes of a 3D rendering context (`transform-style: preserve-3d`) compose.
pub(crate) fn matrix3d(
    style: &ComputedStyle,
    (x, y, width, height): (f32, f32, f32, f32),
    perspective: Option<Perspective>,
) -> (Matrix3d, (f32, f32, f32, f32)) {
    use fagus_style::values::{BoxArea, TransformBox};
    let reference = if matches!(
        style.transform_box,
        TransformBox::ContentBox | TransformBox::FillBox
    ) {
        let border_box = crate::RectF { x, y, width, height };
        let (content, _) = crate::background::area(style, border_box, None, BoxArea::ContentBox);
        (content.x, content.y, content.width, content.height)
    } else {
        (x, y, width, height)
    };
    (in_box3d(style, reference, perspective), reference)
}

/// [`matrix`] with the reference box `(x, y, width, height)`.
fn in_box(
    style: &ComputedStyle,
    reference: (f32, f32, f32, f32),
    perspective: Option<Perspective>,
) -> Option<Matrix> {
    flatten(&in_box3d(style, reference, perspective), reference)
}

/// [`in_box`] before it is flattened.
fn in_box3d(
    style: &ComputedStyle,
    (x, y, width, height): (f32, f32, f32, f32),
    perspective: Option<Perspective>,
) -> Matrix3d {
    let origin = (
        x + style.transform_origin.0.resolve(width),
        y + style.transform_origin.1.resolve(height),
    );
    let mut matrix = IDENTITY;
    if let Some(Perspective { distance, origin }) = perspective {
        matrix = multiply(&translate(origin.0, origin.1, 0.0), &project(distance));
        matrix = multiply(&matrix, &translate(-origin.0, -origin.1, 0.0));
    }
    matrix = multiply(&matrix, &translate(origin.0, origin.1, 0.0));
    if let Some((tx, ty, tz)) = &style.translate {
        matrix = multiply(&matrix, &translate(tx.resolve(width), ty.resolve(height), *tz));
    }
    if let Some((axis_x, axis_y, axis_z, angle)) = style.rotate {
        matrix = multiply(&matrix, &rotate(axis_x, axis_y, axis_z, angle));
    }
    if let Some((sx, sy)) = style.scale {
        matrix = multiply(&matrix, &flat([sx, 0.0, 0.0, sy, 0.0, 0.0]));
    }
    for function in style.transform.iter() {
        let step = match function {
            TransformFunction::Translate(tx, ty) => translate(tx.resolve(width), ty.resolve(height), 0.0),
            TransformFunction::TranslateZ(tz) => translate(0.0, 0.0, *tz),
            TransformFunction::Scale(sx, sy) => flat([*sx, 0.0, 0.0, *sy, 0.0, 0.0]),
            TransformFunction::Rotate(angle) => rotate(0.0, 0.0, 1.0, *angle),
            TransformFunction::Rotate3d(axis_x, axis_y, axis_z, angle) => {
                rotate(*axis_x, *axis_y, *axis_z, *angle)
            }
            TransformFunction::Skew(ax, ay) => flat([
                1.0,
                libm::tanf(ay.to_radians()),
                libm::tanf(ax.to_radians()),
                1.0,
                0.0,
                0.0,
            ]),
            TransformFunction::Matrix(values) => flat(*values),
            TransformFunction::Matrix3d(values) => *values,
            TransformFunction::Perspective(distance) => project(*distance),
        };
        matrix = multiply(&matrix, &step);
    }
    multiply(&matrix, &translate(-origin.0, -origin.1, 0.0))
}

/// A 3D transform composed of `outer` (the space of a 3D rendering context) and `inner`.
pub(crate) fn compose(outer: &Matrix3d, inner: &Matrix3d) -> Matrix3d {
    multiply(outer, inner)
}

/// A 3D transform flattened onto the page in the box `reference` (see [`flatten`]).
pub(crate) fn flattened(matrix: &Matrix3d, reference: (f32, f32, f32, f32)) -> Option<Matrix> {
    flatten(matrix, reference)
}

/// The transform of an SVG element whose reference box (`transform-box`) is `(x, y, width,
/// height)` in its user space, flattened (see [`matrix`]); `None` where it lies behind the
/// viewer.
pub fn svg_matrix(style: &ComputedStyle, reference: (f32, f32, f32, f32)) -> Option<Matrix> {
    in_box(style, reference, None)
}

const IDENTITY: Matrix3d = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

/// A 2D transform in 3D.
fn flat(matrix: Matrix) -> Matrix3d {
    let mut flat = IDENTITY;
    flat[0..2].copy_from_slice(&matrix[0..2]);
    flat[4..6].copy_from_slice(&matrix[2..4]);
    flat[12..14].copy_from_slice(&matrix[4..6]);
    flat
}

fn translate(x: f32, y: f32, z: f32) -> Matrix3d {
    let mut matrix = IDENTITY;
    matrix[12..15].copy_from_slice(&[x, y, z]);
    matrix
}

/// `perspective()`: a viewer `distance` away, at least a pixel (CSS Transforms 2, §13.1).
fn project(distance: f32) -> Matrix3d {
    let mut matrix = IDENTITY;
    matrix[11] = -1.0 / distance.max(1.0);
    matrix
}

/// `rotate3d()` by `degrees` about the axis `(x, y, z)`, clockwise about z on a page whose y axis
/// points down (CSS Transforms 2, §13.1); about no axis, none.
fn rotate(x: f32, y: f32, z: f32, degrees: f32) -> Matrix3d {
    let length = (x * x + y * y + z * z).sqrt();
    if length <= f32::EPSILON {
        return IDENTITY;
    }
    let (x, y, z) = (x / length, y / length, z / length);
    let (sin, cos) = libm::sincosf(degrees.to_radians() / 2.0);
    let (sc, sq) = (sin * cos, sin * sin);
    [
        1.0 - 2.0 * (y * y + z * z) * sq,
        2.0 * (x * y * sq + z * sc),
        2.0 * (x * z * sq - y * sc),
        0.0,
        2.0 * (x * y * sq - z * sc),
        1.0 - 2.0 * (x * x + z * z) * sq,
        2.0 * (y * z * sq + x * sc),
        0.0,
        2.0 * (x * z * sq + y * sc),
        2.0 * (y * z * sq - x * sc),
        1.0 - 2.0 * (x * x + y * y) * sq,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
    ]
}

/// `first · second`: applies `second`, then `first`.
fn multiply(first: &Matrix3d, second: &Matrix3d) -> Matrix3d {
    let mut product = [0.0; 16];
    for column in 0..4 {
        for row in 0..4 {
            product[column * 4 + row] = (0..4).map(|k| first[k * 4 + row] * second[column * 4 + k]).sum();
        }
    }
    product
}

/// A 3D transform of the plane of a box with border box `(x, y, width, height)`, flattened onto
/// the page: where the box's corners land, divided by their depth, matched as closely as a 2D
/// transform can, exactly where no perspective makes their depths differ.
fn flatten(matrix: &Matrix3d, (x, y, width, height): (f32, f32, f32, f32)) -> Option<Matrix> {
    let m = matrix;
    if m[3].abs() <= f32::EPSILON && m[7].abs() <= f32::EPSILON {
        let depth = m[15];
        return (depth > f32::EPSILON).then(|| {
            [
                m[0] / depth,
                m[1] / depth,
                m[4] / depth,
                m[5] / depth,
                m[12] / depth,
                m[13] / depth,
            ]
        });
    }
    let project = |(px, py): (f32, f32)| {
        let depth = m[3] * px + m[7] * py + m[15];
        (depth > f32::EPSILON).then(|| {
            (
                (m[0] * px + m[4] * py + m[12]) / depth,
                (m[1] * px + m[5] * py + m[13]) / depth,
            )
        })
    };
    let (width, height) = (width.max(1.0), height.max(1.0));
    let top_left = project((x, y))?;
    let top_right = project((x + width, y))?;
    let bottom_left = project((x, y + height))?;
    let bottom_right = project((x + width, y + height))?;
    // Across: the mean of the top and the bottom edge; down: of the left and the right one.
    let across = (
        (top_right.0 - top_left.0 + bottom_right.0 - bottom_left.0) / (2.0 * width),
        (top_right.1 - top_left.1 + bottom_right.1 - bottom_left.1) / (2.0 * width),
    );
    let down = (
        (bottom_left.0 - top_left.0 + bottom_right.0 - top_right.0) / (2.0 * height),
        (bottom_left.1 - top_left.1 + bottom_right.1 - top_right.1) / (2.0 * height),
    );
    // The center lands where the corners do on average.
    let center = (x + width / 2.0, y + height / 2.0);
    let landed = (
        (top_left.0 + top_right.0 + bottom_left.0 + bottom_right.0) / 4.0,
        (top_left.1 + top_right.1 + bottom_left.1 + bottom_right.1) / 4.0,
    );
    Some([
        across.0,
        across.1,
        down.0,
        down.1,
        landed.0 - across.0 * center.0 - down.0 * center.1,
        landed.1 - across.1 * center.0 - down.1 * center.1,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use fagus_style::values::LengthPercentage;

    fn apply(matrix: Matrix, (x, y): (f32, f32)) -> (f32, f32) {
        (
            matrix[0] * x + matrix[2] * y + matrix[4],
            matrix[1] * x + matrix[3] * y + matrix[5],
        )
    }

    fn close(found: (f32, f32), expected: (f32, f32)) -> bool {
        (found.0 - expected.0).abs() < 1e-3 && (found.1 - expected.1).abs() < 1e-3
    }

    #[test]
    fn rotates_around_the_origin_and_translates_first() {
        let style = ComputedStyle::from(fagus_style::ComputedValues {
            rotate: Some((0.0, 0.0, 1.0, 90.0)),
            translate: Some((LengthPercentage::Px(10.0), LengthPercentage::ZERO, 0.0)),
            ..fagus_style::ComputedValues::default()
        });
        // A 20 × 20 box at (100, 100): its center stays put by the rotation, then moves 10px right.
        let matrix = matrix(&style, (100.0, 100.0, 20.0, 20.0), None).expect("drawn");
        assert!(close(apply(matrix, (110.0, 110.0)), (120.0, 110.0)));
        // The top left corner turns clockwise to the top right.
        assert!(close(apply(matrix, (100.0, 100.0)), (130.0, 100.0)));
    }

    #[test]
    fn flattens_transforms_in_depth() {
        let transformed = |function: TransformFunction| {
            let style = ComputedStyle::from(fagus_style::ComputedValues {
                transform: std::sync::Arc::from([function]),
                ..fagus_style::ComputedValues::default()
            });
            matrix(&style, (0.0, 0.0, 100.0, 50.0), None)
        };
        // Turned 60° about the vertical axis, a box is half as wide, about its center.
        let turned = transformed(TransformFunction::Rotate3d(0.0, 1.0, 0.0, 60.0)).expect("drawn");
        assert!(close(apply(turned, (0.0, 0.0)), (25.0, 0.0)));
        assert!(close(apply(turned, (100.0, 50.0)), (75.0, 50.0)));
        // Turned all the way, it is mirrored.
        let flipped = transformed(TransformFunction::Rotate3d(0.0, 1.0, 0.0, 180.0)).expect("drawn");
        assert!(close(apply(flipped, (0.0, 0.0)), (100.0, 0.0)));
        // Moving halfway toward a viewer 100px away makes it twice as large, about its center.
        let closer = multiply(&project(100.0), &translate(0.0, 0.0, 50.0));
        let closer = transformed(TransformFunction::Matrix3d(closer)).expect("drawn");
        assert!(close(apply(closer, (0.0, 0.0)), (-50.0, -25.0)));
        // Behind the viewer, it is not drawn.
        let behind = multiply(&project(100.0), &translate(0.0, 0.0, 150.0));
        assert_eq!(transformed(TransformFunction::Matrix3d(behind)), None);
    }
}

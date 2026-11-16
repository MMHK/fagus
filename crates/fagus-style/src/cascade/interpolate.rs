//! Values of `filter`, `transform`, `translate`, `rotate` and `scale` between two keyframes, as
//! Chrome prints an animation paused or delayed into its middle (Filter Effects 1, §12; CSS
//! Transforms 1, §9 and 2, §10). `None` stands for values that do not interpolate, which flip
//! at the middle as discrete values do.

use std::sync::Arc;

use super::animation::{mix, mix_lengths};
use crate::values::{Filter, LengthPercentage, Rgba, TransformFunction};

/// The value of a filter function that leaves what it filters unchanged, of the kind of
/// `function` (the "initial value for interpolation").
fn neutral_filter(function: &Filter) -> Option<Filter> {
    Some(match function {
        Filter::Blur(_) => Filter::Blur(0.0),
        Filter::Brightness(_) => Filter::Brightness(1.0),
        Filter::Contrast(_) => Filter::Contrast(1.0),
        Filter::Grayscale(_) => Filter::Grayscale(0.0),
        Filter::HueRotate(_) => Filter::HueRotate(0.0),
        Filter::Invert(_) => Filter::Invert(0.0),
        Filter::Opacity(_) => Filter::Opacity(1.0),
        Filter::Saturate(_) => Filter::Saturate(1.0),
        Filter::Sepia(_) => Filter::Sepia(0.0),
        Filter::DropShadow { .. } => Filter::DropShadow {
            x: 0.0,
            y: 0.0,
            blur: 0.0,
            color: Some(Rgba::TRANSPARENT),
        },
        Filter::ColorMatrix { .. } | Filter::Url(_) => return None,
    })
}

/// Two filter functions of one kind interpolated; `colors` are the elements' colors that a
/// shadow without one of its own has.
fn mix_filter(a: &Filter, b: &Filter, progress: f32, colors: (Rgba, Rgba)) -> Option<Filter> {
    let lerp = |a: f32, b: f32| a + (b - a) * progress;
    // Amounts below zero are not valid, and those of these functions not above one either.
    let amount = |a: f32, b: f32| lerp(a, b).max(0.0);
    let fraction = |a: f32, b: f32| lerp(a, b).clamp(0.0, 1.0);
    Some(match (a, b) {
        (Filter::Blur(a), Filter::Blur(b)) => Filter::Blur(amount(*a, *b)),
        (Filter::Brightness(a), Filter::Brightness(b)) => Filter::Brightness(amount(*a, *b)),
        (Filter::Contrast(a), Filter::Contrast(b)) => Filter::Contrast(amount(*a, *b)),
        (Filter::Grayscale(a), Filter::Grayscale(b)) => Filter::Grayscale(fraction(*a, *b)),
        (Filter::HueRotate(a), Filter::HueRotate(b)) => Filter::HueRotate(lerp(*a, *b)),
        (Filter::Invert(a), Filter::Invert(b)) => Filter::Invert(fraction(*a, *b)),
        (Filter::Opacity(a), Filter::Opacity(b)) => Filter::Opacity(fraction(*a, *b)),
        (Filter::Saturate(a), Filter::Saturate(b)) => Filter::Saturate(amount(*a, *b)),
        (Filter::Sepia(a), Filter::Sepia(b)) => Filter::Sepia(fraction(*a, *b)),
        (
            Filter::DropShadow {
                x: ax,
                y: ay,
                blur: ablur,
                color: acolor,
            },
            Filter::DropShadow {
                x: bx,
                y: by,
                blur: bblur,
                color: bcolor,
            },
        ) => Filter::DropShadow {
            x: lerp(*ax, *bx),
            y: lerp(*ay, *by),
            blur: amount(*ablur, *bblur),
            color: Some(mix(
                acolor.unwrap_or(colors.0),
                bcolor.unwrap_or(colors.1),
                progress,
            )),
        },
        _ if a == b => a.clone(),
        _ => return None,
    })
}

/// Two filter lists interpolated function by function: where one is shorter (or `none`), it
/// goes on with functions that change nothing; `None` where the functions at a place differ in
/// kind. `colors` are the elements' colors at both ends.
pub(super) fn filters(
    from: &Arc<[Filter]>,
    to: &Arc<[Filter]>,
    progress: f32,
    colors: (Rgba, Rgba),
) -> Option<Arc<[Filter]>> {
    let length = from.len().max(to.len());
    (0..length)
        .map(|index| {
            let (a, b) = match (from.get(index), to.get(index)) {
                (Some(a), Some(b)) => (a.clone(), b.clone()),
                (Some(a), None) => (a.clone(), neutral_filter(a)?),
                (None, Some(b)) => (neutral_filter(b)?, b.clone()),
                (None, None) => return None,
            };
            if std::mem::discriminant(&a) != std::mem::discriminant(&b) {
                return None;
            }
            mix_filter(&a, &b, progress, colors)
        })
        .collect()
}

/// The transform function that changes nothing, of the kind of `function`.
fn neutral_transform(function: &TransformFunction) -> Option<TransformFunction> {
    Some(match function {
        TransformFunction::Translate(..) => {
            TransformFunction::Translate(LengthPercentage::Px(0.0), LengthPercentage::Px(0.0))
        }
        TransformFunction::TranslateZ(_) => TransformFunction::TranslateZ(0.0),
        TransformFunction::Scale(..) => TransformFunction::Scale(1.0, 1.0),
        TransformFunction::Rotate(_) => TransformFunction::Rotate(0.0),
        TransformFunction::Rotate3d(_, x, y, z) => TransformFunction::Rotate3d(0.0, *x, *y, *z),
        TransformFunction::Skew(..) => TransformFunction::Skew(0.0, 0.0),
        TransformFunction::Matrix(_) => TransformFunction::Matrix([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
        TransformFunction::Matrix3d(_) | TransformFunction::Perspective(_) => return None,
    })
}

/// Two transform functions of one kind interpolated; `None` for other pairs.
fn mix_transform(a: &TransformFunction, b: &TransformFunction, progress: f32) -> Option<TransformFunction> {
    use TransformFunction as T;
    let lerp = |a: f32, b: f32| a + (b - a) * progress;
    Some(match (a, b) {
        (T::Translate(ax, ay), T::Translate(bx, by)) => {
            T::Translate(mix_lengths(ax, bx, progress), mix_lengths(ay, by, progress))
        }
        (T::TranslateZ(a), T::TranslateZ(b)) => T::TranslateZ(lerp(*a, *b)),
        (T::Scale(ax, ay), T::Scale(bx, by)) => T::Scale(lerp(*ax, *bx), lerp(*ay, *by)),
        (T::Rotate(a), T::Rotate(b)) => T::Rotate(lerp(*a, *b)),
        (T::Rotate3d(a, ax, ay, az), T::Rotate3d(b, bx, by, bz))
            if same_axis((*ax, *ay, *az), (*bx, *by, *bz)) =>
        {
            T::Rotate3d(lerp(*a, *b), *bx, *by, *bz)
        }
        (T::Skew(ax, ay), T::Skew(bx, by)) => T::Skew(lerp(*ax, *bx), lerp(*ay, *by)),
        (T::Matrix(a), T::Matrix(b)) => T::Matrix(mix_matrices(*a, *b, progress)?),
        _ if a == b => a.clone(),
        _ => return None,
    })
}

/// Whether two rotation axes point the same way.
fn same_axis(a: (f32, f32, f32), b: (f32, f32, f32)) -> bool {
    let length = |(x, y, z): (f32, f32, f32)| (x * x + y * y + z * z).sqrt();
    let (la, lb) = (length(a), length(b));
    if la == 0.0 || lb == 0.0 {
        return false;
    }
    let dot = (a.0 * b.0 + a.1 * b.1 + a.2 * b.2) / (la * lb);
    dot > 1.0 - 1e-5
}

/// Two transform lists interpolated: function by function where they are alike (the shorter,
/// or `none`, going on with functions that change nothing), else as the 2D matrices they make,
/// decomposed. `None` where neither works (3D matrices, percentages in a mix of others).
pub(super) fn transforms(
    from: &Arc<[TransformFunction]>,
    to: &Arc<[TransformFunction]>,
    progress: f32,
) -> Option<Arc<[TransformFunction]>> {
    let length = from.len().max(to.len());
    let pairwise: Option<Arc<[TransformFunction]>> = (0..length)
        .map(|index| {
            let (a, b) = match (from.get(index), to.get(index)) {
                (Some(a), Some(b)) => (a.clone(), b.clone()),
                (Some(a), None) => (a.clone(), neutral_transform(a)?),
                (None, Some(b)) => (neutral_transform(b)?, b.clone()),
                (None, None) => return None,
            };
            mix_transform(&a, &b, progress)
        })
        .collect();
    if pairwise.is_some() {
        return pairwise;
    }
    let matrix = mix_matrices(matrix_of(from)?, matrix_of(to)?, progress)?;
    Some(Arc::from([TransformFunction::Matrix(matrix)]))
}

/// The 2D matrix `[a, b, c, d, e, f]` of a transform list without percentages or 3D functions.
fn matrix_of(functions: &[TransformFunction]) -> Option<[f32; 6]> {
    let multiply = |m: [f32; 6], n: [f32; 6]| {
        [
            m[0] * n[0] + m[2] * n[1],
            m[1] * n[0] + m[3] * n[1],
            m[0] * n[2] + m[2] * n[3],
            m[1] * n[2] + m[3] * n[3],
            m[0] * n[4] + m[2] * n[5] + m[4],
            m[1] * n[4] + m[3] * n[5] + m[5],
        ]
    };
    let mut matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    for function in functions {
        let next = match function {
            TransformFunction::Translate(LengthPercentage::Px(x), LengthPercentage::Px(y)) => {
                [1.0, 0.0, 0.0, 1.0, *x, *y]
            }
            TransformFunction::Scale(x, y) => [*x, 0.0, 0.0, *y, 0.0, 0.0],
            TransformFunction::Rotate(degrees) => {
                let (sin, cos) = degrees.to_radians().sin_cos();
                [cos, sin, -sin, cos, 0.0, 0.0]
            }
            TransformFunction::Skew(x, y) => [1.0, y.to_radians().tan(), x.to_radians().tan(), 1.0, 0.0, 0.0],
            TransformFunction::Matrix(m) => *m,
            _ => return None,
        };
        matrix = multiply(matrix, next);
    }
    Some(matrix)
}

/// A 2D matrix taken apart into a translation, a rotation (degrees), a shear and scales:
/// `translate · rotate · [[1, shear], [0, 1]] · scale`.
struct Decomposed {
    translate: (f32, f32),
    angle: f32,
    shear: f32,
    scale: (f32, f32),
}

fn decompose(matrix: [f32; 6]) -> Option<Decomposed> {
    let [a, b, c, d, tx, ty] = matrix;
    let sx = a.hypot(b);
    if sx == 0.0 {
        return None;
    }
    let determinant = a * d - b * c;
    let sy = determinant / sx;
    if sy == 0.0 {
        return None;
    }
    // The part of the second column along the first, which the shear makes.
    let along = (a * c + b * d) / sx;
    Some(Decomposed {
        translate: (tx, ty),
        angle: b.atan2(a).to_degrees(),
        shear: along / sy,
        scale: (sx, sy),
    })
}

fn compose(m: &Decomposed) -> [f32; 6] {
    let (sin, cos) = m.angle.to_radians().sin_cos();
    let (sx, sy) = m.scale;
    // rotate · [[sx, shear · sy], [0, sy]]
    let (u0, u1) = (sx, 0.0);
    let (v0, v1) = (m.shear * sy, sy);
    [
        cos * u0 - sin * u1,
        sin * u0 + cos * u1,
        cos * v0 - sin * v1,
        sin * v0 + cos * v1,
        m.translate.0,
        m.translate.1,
    ]
}

/// Two 2D matrices interpolated by their decompositions, rotating the shorter way round.
fn mix_matrices(a: [f32; 6], b: [f32; 6], progress: f32) -> Option<[f32; 6]> {
    let (from, mut to) = (decompose(a)?, decompose(b)?);
    if (to.angle - from.angle).abs() > 180.0 {
        to.angle += if to.angle > from.angle { -360.0 } else { 360.0 };
    }
    let lerp = |a: f32, b: f32| a + (b - a) * progress;
    Some(compose(&Decomposed {
        translate: (
            lerp(from.translate.0, to.translate.0),
            lerp(from.translate.1, to.translate.1),
        ),
        angle: lerp(from.angle, to.angle),
        shear: lerp(from.shear, to.shear),
        scale: (lerp(from.scale.0, to.scale.0), lerp(from.scale.1, to.scale.1)),
    }))
}

/// `translate` interpolated, `none` being no move.
pub(super) fn translate(
    from: Option<&(LengthPercentage, LengthPercentage, f32)>,
    to: Option<&(LengthPercentage, LengthPercentage, f32)>,
    progress: f32,
) -> Option<(LengthPercentage, LengthPercentage, f32)> {
    let none = (LengthPercentage::Px(0.0), LengthPercentage::Px(0.0), 0.0);
    if from.is_none() && to.is_none() {
        return None;
    }
    let (a, b) = (from.unwrap_or(&none), to.unwrap_or(&none));
    Some((
        mix_lengths(&a.0, &b.0, progress),
        mix_lengths(&a.1, &b.1, progress),
        a.2 + (b.2 - a.2) * progress,
    ))
}

/// `scale` interpolated, `none` being a scale of one.
pub(super) fn scale(from: Option<(f32, f32)>, to: Option<(f32, f32)>, progress: f32) -> Option<(f32, f32)> {
    if from.is_none() && to.is_none() {
        return None;
    }
    let (a, b) = (from.unwrap_or((1.0, 1.0)), to.unwrap_or((1.0, 1.0)));
    Some((a.0 + (b.0 - a.0) * progress, a.1 + (b.1 - a.1) * progress))
}

/// `rotate` (`(x, y, z, degrees)`) interpolated about a common axis, `none` being no turn
/// about the other's axis;
/// `None` for rotations about different axes (outer `None`), which do not interpolate here.
#[allow(clippy::option_option, reason = "`rotate: none` is a value that interpolates")]
pub(super) fn rotate(
    from: Option<(f32, f32, f32, f32)>,
    to: Option<(f32, f32, f32, f32)>,
    progress: f32,
) -> Option<Option<(f32, f32, f32, f32)>> {
    let (a, b) = match (from, to) {
        (None, None) => return Some(None),
        (Some(a), None) => (a, (a.0, a.1, a.2, 0.0)),
        (None, Some(b)) => ((b.0, b.1, b.2, 0.0), b),
        (Some(a), Some(b)) => (a, b),
    };
    same_axis((a.0, a.1, a.2), (b.0, b.1, b.2)).then_some(Some((b.0, b.1, b.2, a.3 + (b.3 - a.3) * progress)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolates_filters_toward_none() {
        let from: Arc<[Filter]> = Arc::from([Filter::Contrast(0.0), Filter::Blur(20.0)]);
        let none: Arc<[Filter]> = Arc::from([]);
        let colors = (Rgba::TRANSPARENT, Rgba::TRANSPARENT);
        let half = filters(&from, &none, 0.5, colors).unwrap();
        assert_eq!(&*half, &[Filter::Contrast(0.5), Filter::Blur(10.0)]);
        // A shadow fades toward a transparent one of no offset.
        let shadow: Arc<[Filter]> = Arc::from([Filter::DropShadow {
            x: 30.0,
            y: 30.0,
            blur: 0.0,
            color: Some(Rgba::from_u8(0, 0, 0, 1.0)),
        }]);
        let Filter::DropShadow { x, color, .. } = filters(&shadow, &none, 0.5, colors).unwrap()[0].clone()
        else {
            panic!("a drop shadow");
        };
        assert!((x - 15.0).abs() < 1e-5 && (color.unwrap().a - 0.5).abs() < 1e-5);
        // Functions of other kinds at one place do not interpolate.
        let other: Arc<[Filter]> = Arc::from([Filter::Blur(1.0)]);
        assert!(filters(&from, &other, 0.5, colors).is_none());
    }

    #[test]
    fn interpolates_transforms() {
        let none: Arc<[TransformFunction]> = Arc::from([]);
        let to: Arc<[TransformFunction]> = Arc::from([
            TransformFunction::Rotate(90.0),
            TransformFunction::Scale(3.0, 1.0),
        ]);
        let half = transforms(&none, &to, 0.5).unwrap();
        assert_eq!(
            &*half,
            &[
                TransformFunction::Rotate(45.0),
                TransformFunction::Scale(2.0, 1.0)
            ]
        );
        // Unlike lists go through their matrices: a turn of 90° and a scale of 3 meet halfway.
        let rotated: Arc<[TransformFunction]> = Arc::from([TransformFunction::Rotate(90.0)]);
        let scaled: Arc<[TransformFunction]> = Arc::from([TransformFunction::Scale(3.0, 3.0)]);
        let Some(TransformFunction::Matrix(m)) = transforms(&rotated, &scaled, 0.5).map(|t| t[0].clone())
        else {
            panic!("a matrix");
        };
        let (sin, cos) = 45_f32.to_radians().sin_cos();
        let expected = [2.0 * cos, 2.0 * sin, -2.0 * sin, 2.0 * cos, 0.0, 0.0];
        assert!(m.iter().zip(expected).all(|(a, b)| (a - b).abs() < 1e-4), "{m:?}");
        // Individual transforms: `none` is the identity.
        assert_eq!(scale(None, Some((3.0, 1.0)), 0.5), Some((2.0, 1.0)));
        assert_eq!(
            rotate(Some((0.0, 0.0, 1.0, 90.0)), None, 0.5),
            Some(Some((0.0, 0.0, 1.0, 45.0)))
        );
        assert_eq!(
            rotate(Some((1.0, 0.0, 0.0, 90.0)), Some((0.0, 0.0, 1.0, 90.0)), 0.5),
            None
        );
    }
}

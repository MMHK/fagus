//! Gradient stops whose colors miss components (`none`, CSS Color 4, §4.4): interpolated with
//! a neighbor, a missing component takes the neighbor's value (§12.2), so such a stop stands for
//! two colors at its position, each filled in from the stop on its side, as Chrome draws it.
//! Missing components carry over to the gradient's interpolation space where it has an
//! analogous component (§12.2.1: reds, greens, blues, lightness, colorfulness, hue, opponent a
//! and b).

use lightningcss::values::color::{
    CssColor, FloatColor, HSL, HWB, LAB, LABColor, LCH, OKLAB, OKLCH, PredefinedColor, SRGB, SRGBLinear,
    XYZd65,
};

use crate::values::{ColorSpace, Rgba};

/// The kinds of analogous components.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Red,
    Green,
    Blue,
    Lightness,
    Colorfulness,
    Hue,
    OpponentA,
    OpponentB,
    X,
    Y,
    Z,
}

const RGB: [Option<Kind>; 3] = [Some(Kind::Red), Some(Kind::Green), Some(Kind::Blue)];
const XYZ: [Option<Kind>; 3] = [Some(Kind::X), Some(Kind::Y), Some(Kind::Z)];
const RECTANGULAR: [Option<Kind>; 3] = [
    Some(Kind::Lightness),
    Some(Kind::OpponentA),
    Some(Kind::OpponentB),
];
const POLAR: [Option<Kind>; 3] = [Some(Kind::Lightness), Some(Kind::Colorfulness), Some(Kind::Hue)];

/// The kinds of the components a color misses (and whether it misses its alpha), in the space
/// it is given in.
fn missing(color: &CssColor) -> (Vec<Kind>, bool) {
    let (components, kinds) = match color {
        CssColor::LAB(lab) => match **lab {
            LABColor::LAB(c) => ([c.l, c.a, c.b, c.alpha], RECTANGULAR),
            LABColor::OKLAB(c) => ([c.l, c.a, c.b, c.alpha], RECTANGULAR),
            LABColor::LCH(c) => ([c.l, c.c, c.h, c.alpha], POLAR),
            LABColor::OKLCH(c) => ([c.l, c.c, c.h, c.alpha], POLAR),
        },
        CssColor::Predefined(predefined) => match **predefined {
            PredefinedColor::SRGB(c) => ([c.r, c.g, c.b, c.alpha], RGB),
            PredefinedColor::SRGBLinear(c) => ([c.r, c.g, c.b, c.alpha], RGB),
            PredefinedColor::DisplayP3(c) => ([c.r, c.g, c.b, c.alpha], RGB),
            PredefinedColor::A98(c) => ([c.r, c.g, c.b, c.alpha], RGB),
            PredefinedColor::ProPhoto(c) => ([c.r, c.g, c.b, c.alpha], RGB),
            PredefinedColor::Rec2020(c) => ([c.r, c.g, c.b, c.alpha], RGB),
            PredefinedColor::XYZd50(c) => ([c.x, c.y, c.z, c.alpha], XYZ),
            PredefinedColor::XYZd65(c) => ([c.x, c.y, c.z, c.alpha], XYZ),
        },
        CssColor::Float(float) => match **float {
            FloatColor::RGB(c) => ([c.r, c.g, c.b, c.alpha], RGB),
            // The lightness of HSL and the whiteness and blackness of HWB have no analogs.
            FloatColor::HSL(c) => (
                [c.h, c.s, c.l, c.alpha],
                [Some(Kind::Hue), Some(Kind::Colorfulness), None],
            ),
            FloatColor::HWB(c) => ([c.h, c.w, c.b, c.alpha], [Some(Kind::Hue), None, None]),
        },
        _ => return (Vec::new(), false),
    };
    let kinds = (0..3)
        .filter(|&index| components[index].is_nan())
        .filter_map(|index| kinds[index])
        .collect();
    (kinds, components[3].is_nan())
}

/// The kinds of a space's components.
fn kinds(space: ColorSpace) -> [Option<Kind>; 3] {
    match space {
        ColorSpace::Srgb | ColorSpace::SrgbLinear => RGB,
        ColorSpace::Xyz => XYZ,
        ColorSpace::Lab | ColorSpace::Oklab => RECTANGULAR,
        ColorSpace::Lch | ColorSpace::Oklch => POLAR,
        ColorSpace::Hsl => [Some(Kind::Hue), Some(Kind::Colorfulness), None],
        ColorSpace::Hwb => [Some(Kind::Hue), None, None],
    }
}

/// A color's components in a space, its missing ones (that have analogs there) as NaN.
fn components(color: &CssColor, space: ColorSpace) -> Option<[f32; 4]> {
    fn of<T>(color: &CssColor) -> Option<[f32; 4]>
    where
        T: for<'a> TryFrom<&'a CssColor> + lightningcss::values::color::ColorSpace,
    {
        let (a, b, c, alpha) = T::try_from(color).ok()?.resolve_missing().components();
        Some([a, b, c, alpha])
    }
    let mut values = match space {
        ColorSpace::Srgb => of::<SRGB>(color)?,
        ColorSpace::SrgbLinear => of::<SRGBLinear>(color)?,
        ColorSpace::Xyz => of::<XYZd65>(color)?,
        ColorSpace::Lab => of::<LAB>(color)?,
        ColorSpace::Oklab => of::<OKLAB>(color)?,
        ColorSpace::Lch => of::<LCH>(color)?,
        ColorSpace::Oklch => of::<OKLCH>(color)?,
        ColorSpace::Hsl => of::<HSL>(color)?,
        ColorSpace::Hwb => of::<HWB>(color)?,
    };
    let (absent, alpha) = missing(color);
    for (value, kind) in values.iter_mut().zip(kinds(space)) {
        if kind.is_some_and(|kind| absent.contains(&kind)) {
            *value = f32::NAN;
        }
    }
    if alpha {
        values[3] = f32::NAN;
    }
    Some(values)
}

/// The sRGB color of components in a space.
fn color([a, b, c, alpha]: [f32; 4], space: ColorSpace) -> Option<Rgba> {
    let css = match space {
        ColorSpace::Srgb => CssColor::from(SRGB {
            r: a,
            g: b,
            b: c,
            alpha,
        }),
        ColorSpace::SrgbLinear => CssColor::from(SRGBLinear {
            r: a,
            g: b,
            b: c,
            alpha,
        }),
        ColorSpace::Xyz => CssColor::from(XYZd65 {
            x: a,
            y: b,
            z: c,
            alpha,
        }),
        ColorSpace::Lab => CssColor::from(LAB {
            l: a,
            a: b,
            b: c,
            alpha,
        }),
        ColorSpace::Oklab => CssColor::from(OKLAB {
            l: a,
            a: b,
            b: c,
            alpha,
        }),
        ColorSpace::Lch => CssColor::from(LCH {
            l: a,
            c: b,
            h: c,
            alpha,
        }),
        ColorSpace::Oklch => CssColor::from(OKLCH {
            l: a,
            c: b,
            h: c,
            alpha,
        }),
        ColorSpace::Hsl => CssColor::from(HSL {
            h: a,
            s: b,
            l: c,
            alpha,
        }),
        ColorSpace::Hwb => CssColor::from(HWB {
            h: a,
            w: b,
            b: c,
            alpha,
        }),
    };
    crate::compute::color(&css)
}

/// For each color of a gradient's stops (`None` for `currentcolor`, which misses nothing),
/// the colors it stands for toward the previous stop and toward the next, if it misses
/// components in `space`: each missing component filled in from the neighbor on that side
/// (zero where the neighbor misses it too, or there is none).
pub fn split(colors: &[Option<&CssColor>], space: ColorSpace) -> Vec<Option<(Rgba, Rgba)>> {
    let values: Vec<Option<[f32; 4]>> = colors
        .iter()
        .map(|color| color.and_then(|color| components(color, space)))
        .collect();
    let fill = |own: [f32; 4], neighbor: Option<[f32; 4]>| {
        let mut filled = own;
        for (index, value) in filled.iter_mut().enumerate() {
            if value.is_nan() {
                let from = neighbor.map_or(f32::NAN, |neighbor| neighbor[index]);
                *value = if from.is_nan() { 0.0 } else { from };
            }
        }
        filled
    };
    (0..values.len())
        .map(|index| {
            let own = values[index]?;
            if !own.iter().any(|value| value.is_nan()) {
                return None;
            }
            let before = index.checked_sub(1).and_then(|previous| values[previous]);
            let after = values.get(index + 1).copied().flatten();
            // The first and last stops stand for one color: the one toward their neighbor.
            let (before, after) = match (before.is_some(), after.is_some()) {
                (false, true) => (after, after),
                (true, false) => (before, before),
                _ => (before, after),
            };
            Some((color(fill(own, before), space)?, color(fill(own, after), space)?))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightningcss::traits::Parse;

    fn parse(css: &str) -> CssColor {
        CssColor::parse_string(css).expect("color")
    }

    fn bytes(color: Rgba) -> [u8; 3] {
        #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "0 to 255")]
        [color.r, color.g, color.b].map(|channel| (channel * 255.0).round() as u8)
    }

    #[test]
    fn fills_missing_components_from_each_neighbor() {
        let colors = [
            parse("color(srgb 0.5 0 0)"),
            parse("color(srgb none 0.5 0.5)"),
            parse("color(srgb 1 1 1)"),
        ];
        let split = split(&colors.iter().map(Some).collect::<Vec<_>>(), ColorSpace::Srgb);
        assert_eq!(split[0], None);
        let (before, after) = split[1].expect("split");
        assert_eq!(bytes(before), [128, 128, 128]);
        assert_eq!(bytes(after), [255, 128, 128]);
        assert_eq!(split[2], None);
    }

    #[test]
    fn carries_missing_hues_over_to_polar_spaces() {
        let colors = [
            parse("oklch(0.5 0.1 90)"),
            parse("oklch(0.5 0.1 none)"),
            parse("oklch(0.5 0.1 180)"),
        ];
        let split = split(&colors.iter().map(Some).collect::<Vec<_>>(), ColorSpace::Oklch);
        let (before, after) = split[1].expect("split");
        assert_eq!(
            before,
            crate::compute::color(&colors[0]).expect("color"),
            "takes the hue before"
        );
        assert_eq!(
            after,
            crate::compute::color(&colors[2]).expect("color"),
            "and after"
        );
        // No missing components in sRGB: an `oklch()` hue has no analog there.
        assert!(split_none(&colors, ColorSpace::Srgb));
    }

    fn split_none(colors: &[CssColor], space: ColorSpace) -> bool {
        split(&colors.iter().map(Some).collect::<Vec<_>>(), space)
            .iter()
            .all(Option::is_none)
    }
}

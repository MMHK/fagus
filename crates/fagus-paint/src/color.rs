//! Color interpolation (CSS Color 4, §12): mixing two colors in a color space, with premultiplied
//! alpha and hue interpolation methods.
//!
//! The conversions use the letters of the formulas in the specifications (r, g, b, l, a, …).
#![allow(clippy::many_single_char_names)]

use fagus_style::values::{ColorSpace, HueInterpolation, Interpolation, Rgba};

/// Mixes `a` and `b` at `t` (0 = `a`, 1 = `b`).
pub fn mix(a: Rgba, b: Rgba, t: f32, interpolation: Interpolation) -> Rgba {
    let space = interpolation.space;
    let mut from = to_space(a, space);
    let mut to = to_space(b, space);
    let alpha = a.a + (b.a - a.a) * t;
    let polar = hue_index(space);
    // An achromatic color has no hue: it takes the other color's (CSS Color 4, §4.4).
    if let Some(hue) = polar {
        let chroma = chroma_index(space);
        match (from[chroma] < 1e-4, to[chroma] < 1e-4) {
            (true, false) => from[hue] = to[hue],
            (false, true) => to[hue] = from[hue],
            _ => {}
        }
        (from[hue], to[hue]) = fix_hues(from[hue], to[hue], interpolation.hue);
    }
    let mut mixed = [0.0; 3];
    for index in 0..3 {
        mixed[index] = if Some(index) == polar {
            from[index] + (to[index] - from[index]) * t
        } else {
            // Premultiplied: transparent colors do not tint the result.
            let start = from[index] * a.a;
            let end = to[index] * b.a;
            let value = start + (end - start) * t;
            if alpha > 0.0 { value / alpha } else { 0.0 }
        };
    }
    let mut color = from_space(mixed, space);
    color.a = alpha;
    color
}

/// The default interpolation: sRGB when all colors are legacy sRGB colors, Oklab otherwise.
pub fn default_interpolation(all_legacy: bool) -> Interpolation {
    Interpolation {
        space: if all_legacy {
            ColorSpace::Srgb
        } else {
            ColorSpace::Oklab
        },
        hue: HueInterpolation::Shorter,
    }
}

fn hue_index(space: ColorSpace) -> Option<usize> {
    match space {
        ColorSpace::Lch | ColorSpace::Oklch => Some(2),
        ColorSpace::Hsl | ColorSpace::Hwb => Some(0),
        _ => None,
    }
}

/// The component that is zero for achromatic colors in a polar space.
fn chroma_index(space: ColorSpace) -> usize {
    match space {
        ColorSpace::Lch | ColorSpace::Oklch | ColorSpace::Hsl => 1,
        // HWB: gray when whiteness + blackness >= 1; approximated by the hue of a zero range.
        _ => 1,
    }
}

fn fix_hues(mut a: f32, mut b: f32, method: HueInterpolation) -> (f32, f32) {
    a = a.rem_euclid(360.0);
    b = b.rem_euclid(360.0);
    let difference = b - a;
    match method {
        HueInterpolation::Shorter => {
            if difference > 180.0 {
                a += 360.0;
            } else if difference < -180.0 {
                b += 360.0;
            }
        }
        HueInterpolation::Longer => {
            if difference > 0.0 && difference < 180.0 {
                a += 360.0;
            } else if difference > -180.0 && difference <= 0.0 {
                b += 360.0;
            }
        }
        HueInterpolation::Increasing => {
            if b < a {
                b += 360.0;
            }
        }
        HueInterpolation::Decreasing => {
            if a < b {
                a += 360.0;
            }
        }
    }
    (a, b)
}

fn to_space(color: Rgba, space: ColorSpace) -> [f32; 3] {
    let rgb = [color.r, color.g, color.b];
    match space {
        ColorSpace::Srgb => rgb,
        ColorSpace::SrgbLinear => rgb.map(to_linear),
        ColorSpace::Xyz => multiply(&SRGB_TO_XYZ, rgb.map(to_linear)),
        ColorSpace::Oklab => oklab(rgb.map(to_linear)),
        ColorSpace::Oklch => polar(oklab(rgb.map(to_linear))),
        ColorSpace::Lab => lab(rgb.map(to_linear)),
        ColorSpace::Lch => polar(lab(rgb.map(to_linear))),
        ColorSpace::Hsl => hsl(rgb),
        ColorSpace::Hwb => hwb(rgb),
    }
}

fn from_space(value: [f32; 3], space: ColorSpace) -> Rgba {
    let rgb = match space {
        ColorSpace::Srgb => value,
        ColorSpace::SrgbLinear => value.map(from_linear),
        ColorSpace::Xyz => multiply(&XYZ_TO_SRGB, value).map(from_linear),
        ColorSpace::Oklab => from_oklab(value).map(from_linear),
        ColorSpace::Oklch => from_oklab(rectangular(value)).map(from_linear),
        ColorSpace::Lab => from_lab(value).map(from_linear),
        ColorSpace::Lch => from_lab(rectangular(value)).map(from_linear),
        ColorSpace::Hsl => from_hsl(value),
        ColorSpace::Hwb => from_hwb(value),
    };
    // Out-of-gamut results are clipped.
    Rgba {
        r: rgb[0].clamp(0.0, 1.0),
        g: rgb[1].clamp(0.0, 1.0),
        b: rgb[2].clamp(0.0, 1.0),
        a: 1.0,
    }
}

fn to_linear(channel: f32) -> f32 {
    let magnitude = channel.abs();
    let linear = if magnitude <= 0.04045 {
        magnitude / 12.92
    } else {
        ((magnitude + 0.055) / 1.055).powf(2.4)
    };
    linear.copysign(channel)
}

fn from_linear(channel: f32) -> f32 {
    let magnitude = channel.abs();
    let encoded = if magnitude <= 0.003_130_8 {
        magnitude * 12.92
    } else {
        1.055 * magnitude.powf(1.0 / 2.4) - 0.055
    };
    encoded.copysign(channel)
}

type Matrix = [[f32; 3]; 3];

fn multiply(matrix: &Matrix, vector: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|row| matrix[row][0] * vector[0] + matrix[row][1] * vector[1] + matrix[row][2] * vector[2])
}

const SRGB_TO_XYZ: Matrix = [
    [0.412_390_8, 0.357_584_33, 0.180_480_8],
    [0.212_639, 0.715_168_7, 0.072_192_32],
    [0.019_330_818, 0.119_194_78, 0.950_532_15],
];
const XYZ_TO_SRGB: Matrix = [
    [3.240_97, -1.537_383_2, -0.498_610_76],
    [-0.969_243_65, 1.875_967_5, 0.041_555_06],
    [0.055_630_08, -0.203_976_96, 1.056_971_5],
];
const D65_TO_D50: Matrix = [
    [1.047_929_8, 0.022_946_794, -0.050_192_23],
    [0.029_627_815, 0.990_434_5, -0.017_073_825],
    [-0.009_243_058, 0.015_055_145, 0.751_874_3],
];
const D50_TO_D65: Matrix = [
    [0.955_473_4, -0.023_098_537, 0.063_259_31],
    [-0.028_369_706, 1.009_995_5, 0.021_041_399],
    [0.012_314_002, -0.020_507_697, 1.330_366],
];
const D50_WHITE: [f32; 3] = [0.964_295_7, 1.0, 0.825_104_6];

fn oklab(linear: [f32; 3]) -> [f32; 3] {
    let [r, g, b] = linear;
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

fn from_oklab(lab: [f32; 3]) -> [f32; 3] {
    let [l, a, b] = lab;
    let l_ = (l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m_ = (l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let s_ = (l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    [
        4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_,
        -1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_38 * s_,
        -0.004_196_086_3 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_,
    ]
}

const EPSILON: f32 = 216.0 / 24389.0;
const KAPPA: f32 = 24389.0 / 27.0;

fn lab(linear: [f32; 3]) -> [f32; 3] {
    let xyz = multiply(&D65_TO_D50, multiply(&SRGB_TO_XYZ, linear));
    let f = |index: usize| {
        let value = xyz[index] / D50_WHITE[index];
        if value > EPSILON {
            value.cbrt()
        } else {
            (KAPPA * value + 16.0) / 116.0
        }
    };
    let (fx, fy, fz) = (f(0), f(1), f(2));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

fn from_lab(lab: [f32; 3]) -> [f32; 3] {
    let [l, a, b] = lab;
    let fy = (l + 16.0) / 116.0;
    let fx = a / 500.0 + fy;
    let fz = fy - b / 200.0;
    let inverse = |f: f32| {
        let cube = f * f * f;
        if cube > EPSILON {
            cube
        } else {
            (116.0 * f - 16.0) / KAPPA
        }
    };
    let y = if l > KAPPA * EPSILON {
        fy * fy * fy
    } else {
        l / KAPPA
    };
    let xyz = [
        inverse(fx) * D50_WHITE[0],
        y * D50_WHITE[1],
        inverse(fz) * D50_WHITE[2],
    ];
    multiply(&XYZ_TO_SRGB, multiply(&D50_TO_D65, xyz))
}

fn polar([l, a, b]: [f32; 3]) -> [f32; 3] {
    [l, a.hypot(b), b.atan2(a).to_degrees().rem_euclid(360.0)]
}

fn rectangular([l, c, h]: [f32; 3]) -> [f32; 3] {
    let (sin, cos) = h.to_radians().sin_cos();
    [l, c * cos, c * sin]
}

fn hsl([r, g, b]: [f32; 3]) -> [f32; 3] {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let lightness = f32::midpoint(max, min);
    let delta = max - min;
    if delta <= 0.0 {
        return [0.0, 0.0, lightness];
    }
    let saturation = if lightness <= 0.0 || lightness >= 1.0 {
        0.0
    } else {
        (max - lightness) / lightness.min(1.0 - lightness)
    };
    [hue(r, g, b, max, delta), saturation, lightness]
}

fn hue(r: f32, g: f32, b: f32, max: f32, delta: f32) -> f32 {
    let hue = if (max - r).abs() < f32::EPSILON {
        (g - b) / delta + if g < b { 6.0 } else { 0.0 }
    } else if (max - g).abs() < f32::EPSILON {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    };
    hue * 60.0
}

fn from_hsl([h, s, l]: [f32; 3]) -> [f32; 3] {
    let channel = |n: f32| {
        let k = (n + h / 30.0).rem_euclid(12.0);
        let a = s * l.min(1.0 - l);
        l - a * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
    };
    [channel(0.0), channel(8.0), channel(4.0)]
}

fn hwb([r, g, b]: [f32; 3]) -> [f32; 3] {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let hue = if delta > 0.0 {
        hue(r, g, b, max, delta)
    } else {
        0.0
    };
    [hue, min, 1.0 - max]
}

fn from_hwb([h, w, b]: [f32; 3]) -> [f32; 3] {
    if w + b >= 1.0 {
        let gray = w / (w + b);
        return [gray; 3];
    }
    from_hsl([h, 1.0, 0.5]).map(|channel| channel * (1.0 - w - b) + w)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Rgba, b: Rgba) -> bool {
        (a.r - b.r).abs() < 0.01
            && (a.g - b.g).abs() < 0.01
            && (a.b - b.b).abs() < 0.01
            && (a.a - b.a).abs() < 0.01
    }

    #[test]
    fn round_trips_through_every_space() {
        let color = Rgba::from_u8(22, 163, 74, 1.0);
        for space in [
            ColorSpace::Srgb,
            ColorSpace::SrgbLinear,
            ColorSpace::Xyz,
            ColorSpace::Oklab,
            ColorSpace::Oklch,
            ColorSpace::Lab,
            ColorSpace::Lch,
            ColorSpace::Hsl,
            ColorSpace::Hwb,
        ] {
            let back = from_space(to_space(color, space), space);
            assert!(close(back, color), "{space:?}: {back:?}");
        }
    }

    #[test]
    fn mixes_in_oklab_and_with_premultiplied_alpha() {
        let oklab = Interpolation {
            space: ColorSpace::Oklab,
            hue: HueInterpolation::Shorter,
        };
        // Black and white meet in a lighter gray in Oklab than in sRGB.
        let middle = mix(Rgba::BLACK, Rgba::WHITE, 0.5, oklab);
        assert!(middle.r > 0.38 && middle.r < 0.40, "{middle:?}");
        // Mixing with transparent keeps the color and only fades it.
        let srgb = default_interpolation(true);
        let faded = mix(Rgba::from_u8(255, 0, 0, 1.0), Rgba::TRANSPARENT, 0.5, srgb);
        assert!(close(faded, Rgba::from_u8(255, 0, 0, 0.5)), "{faded:?}");
    }

    #[test]
    fn interpolates_hues_the_short_way() {
        assert_eq!(fix_hues(350.0, 10.0, HueInterpolation::Shorter), (350.0, 370.0));
        assert_eq!(fix_hues(10.0, 350.0, HueInterpolation::Shorter), (370.0, 350.0));
        assert_eq!(fix_hues(10.0, 30.0, HueInterpolation::Longer), (370.0, 30.0));
    }
}

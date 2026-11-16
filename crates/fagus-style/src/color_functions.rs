//! Colors that lightningcss leaves unparsed and Fagus computes as Chrome does (CSS Color 5):
//! relative colors whose channels use `calc()` on the channel keywords as numbers
//! (`hsl(from red calc(h + 120) s l)`), and `color-mix()` in the predefined RGB spaces
//! (`display-p3`, `a98-rgb`, `prophoto-rgb`, `rec2020`, `display-p3-linear`) or of such colors.

use lightningcss::properties::custom::{Function, Token, TokenList, TokenOrValue};
use lightningcss::stylesheet::PrinterOptions;
use lightningcss::traits::{Parse, ToCss};
use lightningcss::values::color::{
    A98, ColorSpace, CssColor, HSL, HWB, LAB, LCH, OKLAB, OKLCH, P3, ProPhoto, Rec2020, SRGB, SRGBLinear,
    XYZd50, XYZd65,
};

use crate::math::{Lengths, Quantity};

/// The color a function gives, `None` where it is invalid.
pub fn resolve(function: &Function<'static>, lengths: Lengths<'_>) -> Option<CssColor> {
    let name = function.name.0.to_ascii_lowercase();
    if name == "color-mix" {
        mix(&function.arguments, lengths)
    } else {
        relative(&name, &function.arguments, lengths)
    }
}

/// The color of tokens (a color, or a color function Fagus resolves), with `currentcolor`
/// resolved; `None` where they are not a color.
pub fn token_color(tokens: &[TokenOrValue<'static>]) -> Option<CssColor> {
    color(tokens, None)
}

/// A color token: parsed by lightningcss, or a function Fagus resolves.
fn color(tokens: &[TokenOrValue<'static>], lengths: Lengths<'_>) -> Option<CssColor> {
    let significant: Vec<&TokenOrValue<'static>> =
        tokens.iter().filter(|token| !token.is_whitespace()).collect();
    let color = match significant.as_slice() {
        [TokenOrValue::Color(color)] => color.clone(),
        [TokenOrValue::Function(function)] if fagus_css::is_unparsed_color(function) => {
            resolve(function, lengths)?
        }
        _ => {
            let css = crate::variables::to_css(&TokenList(tokens.to_vec()))?;
            fagus_css::guarded(|| CssColor::parse_string(css.trim()).ok()).flatten()?
        }
    };
    match color {
        // `currentcolor` is the element's color (its parent's in `color`), as in Chrome.
        CssColor::CurrentColor => {
            let current = crate::compute::current_color()?;
            Some(CssColor::from(SRGB {
                r: current.r,
                g: current.g,
                b: current.b,
                alpha: current.a,
            }))
        }
        other => Some(other),
    }
}

/// A space's channels as CSS Color 5 numbers (`hsl` saturation from 0 to 100, `rgb` from 0 to
/// 255, …), their reference ranges for percentages (the value of 100%) and whether each is a
/// hue.
struct Space {
    channels: [&'static str; 3],
    values: [f32; 4],
    percent: [f32; 3],
    hue: [bool; 3],
}

/// The origin color in the space of a relative color function, as its channels.
fn space(name: &str, predefined: Option<&str>, origin: &CssColor) -> Option<Space> {
    let rgb = |color: SRGB| Space {
        channels: ["r", "g", "b"],
        values: [color.r * 255.0, color.g * 255.0, color.b * 255.0, color.alpha],
        percent: [255.0; 3],
        hue: [false; 3],
    };
    let unit = |channels: [&'static str; 3], (a, b, c, alpha): (f32, f32, f32, f32)| Space {
        channels,
        values: [a, b, c, alpha],
        percent: [1.0; 3],
        hue: [false; 3],
    };
    Some(match name {
        "rgb" | "rgba" => rgb(SRGB::try_from(origin).ok()?.resolve_missing()),
        "hsl" | "hsla" => {
            let color = HSL::try_from(origin).ok()?.resolve_missing();
            Space {
                channels: ["h", "s", "l"],
                values: [color.h, color.s, color.l, color.alpha],
                percent: [1.0, 100.0, 100.0],
                hue: [true, false, false],
            }
        }
        "hwb" => {
            let color = HWB::try_from(origin).ok()?.resolve_missing();
            Space {
                channels: ["h", "w", "b"],
                values: [color.h, color.w, color.b, color.alpha],
                percent: [1.0, 100.0, 100.0],
                hue: [true, false, false],
            }
        }
        "lab" => {
            let color = LAB::try_from(origin).ok()?.resolve_missing();
            Space {
                channels: ["l", "a", "b"],
                values: [color.l, color.a, color.b, color.alpha],
                percent: [100.0, 125.0, 125.0],
                hue: [false; 3],
            }
        }
        "lch" => {
            let color = LCH::try_from(origin).ok()?.resolve_missing();
            Space {
                channels: ["l", "c", "h"],
                values: [color.l, color.c, color.h, color.alpha],
                percent: [100.0, 150.0, 1.0],
                hue: [false, false, true],
            }
        }
        "oklab" => {
            let color = OKLAB::try_from(origin).ok()?.resolve_missing();
            Space {
                channels: ["l", "a", "b"],
                values: [color.l, color.a, color.b, color.alpha],
                percent: [1.0, 0.4, 0.4],
                hue: [false; 3],
            }
        }
        "oklch" => {
            let color = OKLCH::try_from(origin).ok()?.resolve_missing();
            Space {
                channels: ["l", "c", "h"],
                values: [color.l, color.c, color.h, color.alpha],
                percent: [1.0, 0.4, 1.0],
                hue: [false, false, true],
            }
        }
        "color" => match predefined? {
            "srgb" => {
                let color = SRGB::try_from(origin).ok()?.resolve_missing();
                unit(["r", "g", "b"], color.components())
            }
            "srgb-linear" => unit(
                ["r", "g", "b"],
                SRGBLinear::try_from(origin).ok()?.resolve_missing().components(),
            ),
            "display-p3" => unit(
                ["r", "g", "b"],
                P3::try_from(origin).ok()?.resolve_missing().components(),
            ),
            "display-p3-linear" => {
                let (r, g, b, alpha) = P3::try_from(origin).ok()?.resolve_missing().components();
                unit(["r", "g", "b"], (to_linear(r), to_linear(g), to_linear(b), alpha))
            }
            "a98-rgb" => unit(
                ["r", "g", "b"],
                A98::try_from(origin).ok()?.resolve_missing().components(),
            ),
            "prophoto-rgb" => unit(
                ["r", "g", "b"],
                ProPhoto::try_from(origin).ok()?.resolve_missing().components(),
            ),
            "rec2020" => unit(
                ["r", "g", "b"],
                Rec2020::try_from(origin).ok()?.resolve_missing().components(),
            ),
            "xyz" | "xyz-d65" => unit(
                ["x", "y", "z"],
                XYZd65::try_from(origin).ok()?.resolve_missing().components(),
            ),
            "xyz-d50" => unit(
                ["x", "y", "z"],
                XYZd50::try_from(origin).ok()?.resolve_missing().components(),
            ),
            _ => return None,
        },
        _ => return None,
    })
}

/// The color of a space from its channels as CSS Color 5 numbers.
fn build(name: &str, predefined: Option<&str>, [a, b, c, alpha]: [f32; 4]) -> Option<CssColor> {
    Some(match name {
        "rgb" | "rgba" => CssColor::from(SRGB {
            r: a / 255.0,
            g: b / 255.0,
            b: c / 255.0,
            alpha,
        }),
        "hsl" | "hsla" => CssColor::from(HSL {
            h: a,
            s: b,
            l: c,
            alpha,
        }),
        "hwb" => CssColor::from(HWB {
            h: a,
            w: b,
            b: c,
            alpha,
        }),
        "lab" => CssColor::from(LAB {
            l: a,
            a: b,
            b: c,
            alpha,
        }),
        "lch" => CssColor::from(LCH {
            l: a,
            c: b,
            h: c,
            alpha,
        }),
        "oklab" => CssColor::from(OKLAB {
            l: a,
            a: b,
            b: c,
            alpha,
        }),
        "oklch" => CssColor::from(OKLCH {
            l: a,
            c: b,
            h: c,
            alpha,
        }),
        "color" => match predefined? {
            "srgb" => CssColor::from(SRGB {
                r: a,
                g: b,
                b: c,
                alpha,
            }),
            "srgb-linear" => CssColor::from(SRGBLinear {
                r: a,
                g: b,
                b: c,
                alpha,
            }),
            "display-p3" => CssColor::from(P3 {
                r: a,
                g: b,
                b: c,
                alpha,
            }),
            // Display P3 with linear light channels: Display P3 with the sRGB transfer function.
            "display-p3-linear" => CssColor::from(P3 {
                r: from_linear(a),
                g: from_linear(b),
                b: from_linear(c),
                alpha,
            }),
            "a98-rgb" => CssColor::from(A98 {
                r: a,
                g: b,
                b: c,
                alpha,
            }),
            "prophoto-rgb" => CssColor::from(ProPhoto {
                r: a,
                g: b,
                b: c,
                alpha,
            }),
            "rec2020" => CssColor::from(Rec2020 {
                r: a,
                g: b,
                b: c,
                alpha,
            }),
            "xyz" | "xyz-d65" => CssColor::from(XYZd65 {
                x: a,
                y: b,
                z: c,
                alpha,
            }),
            "xyz-d50" => CssColor::from(XYZd50 {
                x: a,
                y: b,
                z: c,
                alpha,
            }),
            _ => return None,
        },
        _ => return None,
    })
}

/// `<name>(from <color> <channel> <channel> <channel> [/ <alpha>]?)`, and `color(from <color>
/// <space> …)`: each channel a value or a math expression in which the channel keywords are
/// the origin's channels, as numbers.
fn relative(name: &str, arguments: &TokenList<'static>, lengths: Lengths<'_>) -> Option<CssColor> {
    let significant: Vec<&TokenOrValue<'static>> = arguments
        .0
        .iter()
        .filter(|token| !token.is_whitespace())
        .collect();
    let (from, after_from) = significant.split_first()?;
    // An absolute color of a space lightningcss does not know (`color(display-p3-linear 0 1 0)`)
    // has no origin: its channels are numbers alone.
    let (origin, mut rest) = if matches!(from, TokenOrValue::Token(Token::Ident(ident)) if ident.eq_ignore_ascii_case("from"))
    {
        let (origin, rest) = after_from.split_first()?;
        (Some(color(std::slice::from_ref(origin), lengths)?), rest)
    } else if name == "color" {
        (None, significant.as_slice())
    } else {
        return None;
    };
    let predefined = if name == "color" {
        let (space, after) = rest.split_first()?;
        rest = after;
        match space {
            TokenOrValue::Token(Token::Ident(space)) => Some(space.to_ascii_lowercase()),
            _ => return None,
        }
    } else {
        None
    };
    let space = match &origin {
        Some(origin) => space(name, predefined.as_deref(), origin)?,
        None if predefined.as_deref() == Some("display-p3-linear") => Space {
            channels: [""; 3],
            values: [0.0, 0.0, 0.0, 1.0],
            percent: [1.0; 3],
            hue: [false; 3],
        },
        None => return None,
    };
    // The channels, and the alpha after a slash.
    let (channels, alpha) = match rest
        .iter()
        .position(|token| matches!(token, TokenOrValue::Token(Token::Delim('/'))))
    {
        Some(slash) => (&rest[..slash], Some(&rest[slash + 1..])),
        None => (rest, None),
    };
    if channels.len() != 3 {
        return None;
    }
    let keyword = |token: &TokenOrValue<'static>| -> TokenOrValue<'static> { substitute(token, &space) };
    let evaluate = |token: &TokenOrValue<'static>| -> Option<Quantity> {
        if matches!(token, TokenOrValue::Token(Token::Ident(ident)) if ident.eq_ignore_ascii_case("none")) {
            return Some(Quantity::number(0.0));
        }
        crate::math::evaluate(&[keyword(token)], lengths)
    };
    let mut values = [0.0_f32; 4];
    for (index, token) in channels.iter().enumerate() {
        let quantity = evaluate(token)?;
        #[expect(clippy::cast_possible_truncation, reason = "colors are single precision")]
        let value = quantity.value as f32;
        values[index] = match quantity.powers {
            [0, 0, 0, 0] => value,
            [0, 0, 0, 1] => value / 100.0 * space.percent[index],
            [0, 1, 0, 0] if space.hue[index] => value,
            _ => return None,
        };
    }
    values[3] = match alpha {
        Some([token]) => {
            let quantity = evaluate(token)?;
            #[expect(clippy::cast_possible_truncation, reason = "colors are single precision")]
            let value = quantity.value as f32;
            match quantity.powers {
                [0, 0, 0, 0] => value,
                [0, 0, 0, 1] => value / 100.0,
                _ => return None,
            }
        }
        Some(_) => return None,
        None => space.values[3],
    }
    .clamp(0.0, 1.0);
    build(name, predefined.as_deref(), values)
}

/// A token with the channel keywords of a relative color (and `alpha`) replaced by their
/// numbers, inside math functions and parentheses too.
fn substitute(token: &TokenOrValue<'static>, space: &Space) -> TokenOrValue<'static> {
    match token {
        TokenOrValue::Token(Token::Ident(ident)) => {
            let value = if ident.eq_ignore_ascii_case("alpha") {
                Some(space.values[3])
            } else {
                space
                    .channels
                    .iter()
                    .position(|channel| ident.eq_ignore_ascii_case(channel))
                    .map(|index| space.values[index])
            };
            value.map_or_else(
                || token.clone(),
                |value| {
                    TokenOrValue::Token(Token::Number {
                        has_sign: value < 0.0,
                        value,
                        int_value: None,
                    })
                },
            )
        }
        TokenOrValue::Function(function) => TokenOrValue::Function(Function {
            name: function.name.clone(),
            arguments: TokenList(
                function
                    .arguments
                    .0
                    .iter()
                    .map(|token| substitute(token, space))
                    .collect(),
            ),
        }),
        other => other.clone(),
    }
}

/// `color-mix(in <space> [<hue> hue]?, <color> <percentage>?, <color> <percentage>?)`.
fn mix(arguments: &TokenList<'static>, lengths: Lengths<'_>) -> Option<CssColor> {
    let parts = split_commas(&arguments.0);
    let [method, first, second] = parts.as_slice() else {
        return None;
    };
    let method: Vec<&TokenOrValue<'static>> = method.iter().filter(|token| !token.is_whitespace()).collect();
    let space = match method.as_slice() {
        [
            TokenOrValue::Token(Token::Ident(word)),
            TokenOrValue::Token(Token::Ident(space)),
            ..,
        ] if word.eq_ignore_ascii_case("in") => space.to_ascii_lowercase(),
        _ => return None,
    };
    let (first, first_percent) = mix_component(first, lengths)?;
    let (second, second_percent) = mix_component(second, lengths)?;
    // Percentages normalized (CSS Color 5, §2.1).
    let (p1, p2) = match (first_percent, second_percent) {
        (None, None) => (0.5, 0.5),
        (Some(p1), None) => (p1, 1.0 - p1),
        (None, Some(p2)) => (1.0 - p2, p2),
        (Some(p1), Some(p2)) => (p1, p2),
    };
    if !(0.0..=1.0).contains(&p1) || !(0.0..=1.0).contains(&p2) || p1 + p2 <= 0.0 {
        return None;
    }
    match space.as_str() {
        "display-p3" => mix_rectangular::<P3>(&first, p1, &second, p2, false),
        "display-p3-linear" => mix_rectangular::<P3>(&first, p1, &second, p2, true),
        "a98-rgb" => mix_rectangular::<A98>(&first, p1, &second, p2, false),
        "prophoto-rgb" => mix_rectangular::<ProPhoto>(&first, p1, &second, p2, false),
        "rec2020" => mix_rectangular::<Rec2020>(&first, p1, &second, p2, false),
        _ => {
            // The other spaces lightningcss mixes itself, once the colors are resolved.
            let text = |color: &CssColor| color.to_css_string(PrinterOptions::default()).ok();
            let method_css = crate::variables::to_css(&TokenList(parts[0].to_vec()))?;
            let css = format!(
                "color-mix({}, {} {}%, {} {}%)",
                method_css.trim(),
                text(&first)?,
                p1 * 100.0,
                text(&second)?,
                p2 * 100.0
            );
            fagus_css::guarded(|| CssColor::parse_string(&css).ok()).flatten()
        }
    }
}

/// A color of `color-mix()` and its percentage, as a fraction.
fn mix_component(tokens: &[TokenOrValue<'static>], lengths: Lengths<'_>) -> Option<(CssColor, Option<f32>)> {
    let significant: Vec<&TokenOrValue<'static>> =
        tokens.iter().filter(|token| !token.is_whitespace()).collect();
    let percent = |token: &TokenOrValue<'static>| -> Option<f32> {
        let quantity = crate::math::evaluate(std::slice::from_ref(token), lengths)?;
        #[expect(clippy::cast_possible_truncation, reason = "percentages are single precision")]
        (quantity.powers == [0, 0, 0, 1]).then_some(quantity.value as f32 / 100.0)
    };
    match significant.as_slice() {
        [color_token] => Some((color(&[(*color_token).clone()], lengths)?, None)),
        [first, second] => match percent(first) {
            Some(p) => Some((color(&[(*second).clone()], lengths)?, Some(p))),
            None => Some((color(&[(*first).clone()], lengths)?, Some(percent(second)?))),
        },
        _ => None,
    }
}

/// Two colors mixed in a rectangular RGB space with premultiplied alpha; `linear` mixes the
/// channels with the sRGB transfer function undone (for `display-p3-linear`).
fn mix_rectangular<T>(first: &CssColor, p1: f32, second: &CssColor, p2: f32, linear: bool) -> Option<CssColor>
where
    T: for<'a> TryFrom<&'a CssColor> + ColorSpace + Into<CssColor> + Copy + FromComponents,
{
    let components = |color: &CssColor| -> Option<[f32; 4]> {
        let (a, b, c, alpha) = T::try_from(color).ok()?.resolve_missing().components();
        let channels = [a, b, c].map(|channel| if linear { to_linear(channel) } else { channel });
        Some([channels[0], channels[1], channels[2], alpha])
    };
    let one = components(first)?;
    let two = components(second)?;
    let sum = p1 + p2;
    let (w1, w2) = (p1 / sum, p2 / sum);
    let alpha = one[3] * w1 + two[3] * w2;
    let mut mixed = [0.0; 3];
    for (index, channel) in mixed.iter_mut().enumerate() {
        let premultiplied = one[index] * one[3] * w1 + two[index] * two[3] * w2;
        *channel = if alpha > 0.0 { premultiplied / alpha } else { 0.0 };
        if linear {
            *channel = from_linear(*channel);
        }
    }
    // Percentages that sum to less than 100% make the result transparent by as much.
    let alpha = alpha * sum.min(1.0);
    Some(T::from_components([mixed[0], mixed[1], mixed[2], alpha]).into())
}

/// Builds a color of a space from its components.
trait FromComponents {
    fn from_components(components: [f32; 4]) -> Self;
}

macro_rules! rgb_space {
    ($($space:ty),*) => {
        $(impl FromComponents for $space {
            fn from_components([r, g, b, alpha]: [f32; 4]) -> Self {
                Self { r, g, b, alpha }
            }
        })*
    };
}

rgb_space!(P3, A98, ProPhoto, Rec2020);

fn to_linear(channel: f32) -> f32 {
    let magnitude = channel.abs();
    let linear = if magnitude <= 0.040_45 {
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

/// Tokens split at the commas outside parentheses.
fn split_commas<'t>(tokens: &'t [TokenOrValue<'static>]) -> Vec<&'t [TokenOrValue<'static>]> {
    let mut parts = Vec::new();
    let mut depth = 0_i32;
    let mut start = 0;
    for (index, token) in tokens.iter().enumerate() {
        match token {
            TokenOrValue::Token(Token::ParenthesisBlock) => depth += 1,
            TokenOrValue::Token(Token::CloseParenthesis) => depth -= 1,
            TokenOrValue::Token(Token::Comma) if depth == 0 => {
                parts.push(&tokens[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(&tokens[start..]);
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn color_of(css: &str) -> Option<crate::values::Rgba> {
        let tokens = crate::variables::value_tokens(css)?;
        let function = tokens.0.into_iter().find_map(|token| match token {
            TokenOrValue::Function(function) => Some(function),
            _ => None,
        })?;
        assert!(fagus_css::is_unparsed_color(&function), "{css}");
        crate::compute::color(&resolve(&function, None)?)
    }

    fn bytes(color: crate::values::Rgba) -> [u8; 3] {
        #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "0 to 255")]
        [color.r, color.g, color.b].map(|channel| (channel * 255.0).round() as u8)
    }

    #[test]
    fn computes_relative_colors_with_calculated_channels() {
        // Channel keywords are numbers: the hue of red plus 120 is green.
        assert_eq!(
            color_of("hsl(from red calc(h + 120) s l)").map(bytes),
            Some([0, 255, 0])
        );
        assert_eq!(
            color_of("hsl(from #36c calc(h + 180) calc(s - 20) calc(l + 10))").map(bytes),
            Some([194, 167, 112])
        );
        assert_eq!(
            color_of("hwb(from teal calc(h + 60) w b)").map(bytes),
            Some([0, 0, 128])
        );
        assert_eq!(
            color_of("oklch(from #c33 l c calc(h + 90))").map(bytes),
            Some([113, 127, 0])
        );
    }

    #[test]
    fn mixes_in_predefined_spaces() {
        // As Chrome 157 prints them.
        assert_eq!(
            color_of("color-mix(in display-p3, red, blue)").map(bytes),
            Some([128, 10, 145])
        );
        assert_eq!(
            color_of("color-mix(in rec2020, orange, blue)").map(bytes),
            Some([161, 95, 159])
        );
        assert_eq!(
            color_of("color-mix(in display-p3-linear, red, blue)").map(bytes),
            Some([188, 0, 188])
        );
        // Of a color lightningcss leaves unparsed, in a space it mixes itself.
        assert_eq!(
            color_of("color-mix(in srgb, hsl(from red calc(h + 120) s l), blue)").map(bytes),
            Some([0, 128, 128])
        );
    }
}

//! Math functions evaluated over tokens at computed-value time (CSS Values 4, §10), for what
//! lightningcss does not parse: the division of typed values (`calc(50px / 10px)`, `(100vw -
//! 2rem) / 1px`, `atan2(1px, 1px) / 1deg`) and the arguments of `progress()`, which may be
//! relative lengths. Values carry their type as powers of the base types, so a length divided
//! by a length is a number. A percentage added to a length is a length too (it resolves against
//! one): such a sum keeps its percentage apart, and stays linear in it through products and
//! divisions by values without one (`(100% - 20px) / 2px * 1px` is `calc(50% - 10px)`); what
//! else needs the size it is a percentage of waits for layout.

use lightningcss::properties::custom::{Function, Token, TokenOrValue};
use lightningcss::values::angle::Angle;
use lightningcss::values::length::LengthValue;
use lightningcss::values::time::Time;

/// The base types of CSS values that math functions combine: a value's type is the power of
/// each (a length is `[1, 0, 0, 0]`, a number all zeros).
const LENGTH: usize = 0;
const ANGLE: usize = 1;
const TIME: usize = 2;
const PERCENT: usize = 3;

/// A value of a math expression: a number of the canonical unit of its type (px, deg, ms, %),
/// and where a percentage was added to a length, the percents of the size it resolves against
/// that it holds besides (`powers` then count that size as a length).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quantity {
    pub value: f64,
    pub powers: [i8; 4],
    pub percent: f64,
}

impl Quantity {
    pub(crate) const fn number(value: f64) -> Self {
        Self {
            value,
            powers: [0; 4],
            percent: 0.0,
        }
    }

    const fn of(value: f64, base: usize) -> Self {
        let mut powers = [0; 4];
        powers[base] = 1;
        Self {
            value,
            powers,
            percent: 0.0,
        }
    }

    /// The value with a percentage of it counted as part of a length: a percentage alone (or
    /// over lengths, `100% / 1px`) becomes that many percents; `None` for a percentage
    /// squared.
    fn resolving(self) -> Option<Self> {
        match self.powers[PERCENT] {
            0 => Some(self),
            1 if self.percent == 0.0 => {
                let mut powers = self.powers;
                powers[PERCENT] = 0;
                powers[LENGTH] = powers[LENGTH].checked_add(1)?;
                Some(Self {
                    value: 0.0,
                    powers,
                    percent: self.value,
                })
            }
            _ => None,
        }
    }

    /// The value where it holds no percents apart (see [`Quantity::resolving`]).
    pub(crate) fn plain(self) -> Option<Self> {
        (self.percent == 0.0).then_some(self)
    }

    pub(crate) fn is_number(&self) -> bool {
        self.powers == [0; 4]
    }

    fn is(&self, base: usize) -> bool {
        self.powers
            .iter()
            .enumerate()
            .all(|(index, power)| *power == i8::from(index == base))
    }

    fn with(self, value: f64) -> Self {
        Self { value, ..self }
    }

    /// The token of the value, if it is of a type a property takes: a number, a length, an
    /// angle, a time or a percentage.
    pub fn token(&self) -> Option<TokenOrValue<'static>> {
        #[expect(clippy::cast_possible_truncation, reason = "CSS values are single precision")]
        let value = self.value as f32;
        if !value.is_finite() {
            return None;
        }
        if self.percent != 0.0 {
            // A length and a percentage: `calc()` of both, for layout to resolve.
            #[expect(clippy::cast_possible_truncation, reason = "CSS values are single precision")]
            let percent = self.percent as f32;
            if !self.is(LENGTH) || !percent.is_finite() {
                return None;
            }
            return Some(TokenOrValue::Function(Function {
                name: lightningcss::values::ident::Ident("calc".into()),
                arguments: lightningcss::properties::custom::TokenList(vec![
                    TokenOrValue::Length(LengthValue::Px(value)),
                    TokenOrValue::Token(Token::WhiteSpace(" ".into())),
                    TokenOrValue::Token(Token::Delim('+')),
                    TokenOrValue::Token(Token::WhiteSpace(" ".into())),
                    TokenOrValue::Token(Token::Percentage {
                        has_sign: percent < 0.0,
                        unit_value: percent / 100.0,
                        int_value: None,
                    }),
                ]),
            }));
        }
        Some(if self.is_number() {
            TokenOrValue::Token(Token::Number {
                has_sign: value < 0.0,
                value,
                int_value: None,
            })
        } else if self.is(LENGTH) {
            TokenOrValue::Length(LengthValue::Px(value))
        } else if self.is(ANGLE) {
            TokenOrValue::Angle(Angle::Deg(value))
        } else if self.is(TIME) {
            TokenOrValue::Time(Time::Milliseconds(value))
        } else if self.is(PERCENT) {
            TokenOrValue::Token(Token::Percentage {
                has_sign: value < 0.0,
                unit_value: value / 100.0,
                int_value: None,
            })
        } else {
            return None;
        })
    }
}

/// Converts a length to pixels (without the element's zoom): absolute lengths alone where
/// nothing else is known.
pub type Lengths<'a> = Option<&'a dyn Fn(&LengthValue) -> Option<f32>>;

fn nesting(token: &TokenOrValue<'_>) -> i32 {
    match token {
        TokenOrValue::Token(Token::ParenthesisBlock) => 1,
        TokenOrValue::Token(Token::CloseParenthesis) => -1,
        _ => 0,
    }
}

/// Evaluates a math function to a value; `None` where it is invalid or refers to what is not
/// known here (percentages of a size with lengths).
pub fn evaluate_function(function: &Function<'_>, lengths: Lengths<'_>) -> Option<Quantity> {
    Evaluator { lengths }.function(&function.name.0, &function.arguments.0)
}

/// The number a math function in a descriptor of an at-rule gives (`calc(2 + sign(100em -
/// 1px))` in `@counter-style`), lengths relative to the font taken against the initial font
/// size of 16 pixels, as Chrome does where no element is known; `None` where the function is
/// invalid or gives no number.
pub fn descriptor_number(function: &Function<'_>) -> Option<f64> {
    let lengths = |length: &LengthValue| match length {
        LengthValue::Em(value) | LengthValue::Rem(value) => Some(value * 16.0),
        other => other.to_px(),
    };
    let quantity = evaluate_function(function, Some(&lengths))?.plain()?;
    (quantity.is_number() && quantity.value.is_finite()).then_some(quantity.value)
}

/// Evaluates tokens as a math expression (the argument of `calc()`).
pub fn evaluate(tokens: &[TokenOrValue<'_>], lengths: Lengths<'_>) -> Option<Quantity> {
    Evaluator { lengths }.expression(tokens)
}

struct Evaluator<'a> {
    lengths: Lengths<'a>,
}

impl Evaluator<'_> {
    fn expression(&self, tokens: &[TokenOrValue<'_>]) -> Option<Quantity> {
        let significant: Vec<&TokenOrValue<'_>> =
            tokens.iter().filter(|token| !token.is_whitespace()).collect();
        let mut position = 0;
        let value = self.sum(&significant, &mut position)?;
        (position == significant.len()).then_some(value)
    }

    fn sum(&self, tokens: &[&TokenOrValue<'_>], position: &mut usize) -> Option<Quantity> {
        let mut value = self.product(tokens, position)?;
        loop {
            let sign = match tokens.get(*position) {
                Some(TokenOrValue::Token(Token::Delim('+'))) => 1.0,
                Some(TokenOrValue::Token(Token::Delim('-'))) => -1.0,
                _ => return Some(value),
            };
            *position += 1;
            let mut next = self.product(tokens, position)?;
            if next.powers != value.powers || next.percent != 0.0 || value.percent != 0.0 {
                // A percentage and a length: the percentage resolves against a length.
                value = value.resolving()?;
                next = next.resolving()?;
                if next.powers != value.powers {
                    return None;
                }
            }
            value.value += sign * next.value;
            value.percent += sign * next.percent;
        }
    }

    fn product(&self, tokens: &[&TokenOrValue<'_>], position: &mut usize) -> Option<Quantity> {
        let mut value = self.term(tokens, position)?;
        loop {
            let divide = match tokens.get(*position) {
                Some(TokenOrValue::Token(Token::Delim('*'))) => false,
                Some(TokenOrValue::Token(Token::Delim('/'))) => true,
                _ => return Some(value),
            };
            *position += 1;
            let mut next = self.term(tokens, position)?;
            // A sum with a percentage takes a factor (or a divisor) without one.
            if value.percent != 0.0 {
                next = next.resolving()?.plain()?;
            } else if next.percent != 0.0 {
                if divide {
                    return None;
                }
                value = value.resolving()?;
            }
            for (power, other) in value.powers.iter_mut().zip(next.powers) {
                *power = if divide {
                    power.checked_sub(other)?
                } else {
                    power.checked_add(other)?
                };
            }
            let (factor, percent) = if divide {
                (1.0 / next.value, 0.0)
            } else {
                (next.value, next.percent)
            };
            value.percent = value.percent * factor + value.value * percent;
            value.value *= factor;
        }
    }

    fn term(&self, tokens: &[&TokenOrValue<'_>], position: &mut usize) -> Option<Quantity> {
        let token = tokens.get(*position)?;
        *position += 1;
        Some(match token {
            TokenOrValue::Token(Token::Number { value, .. }) => Quantity::number(f64::from(*value)),
            TokenOrValue::Token(Token::Percentage { unit_value, .. }) => {
                Quantity::of(f64::from(*unit_value) * 100.0, PERCENT)
            }
            TokenOrValue::Length(length) => {
                let px = match self.lengths {
                    Some(lengths) => lengths(length)?,
                    None => length.to_px()?,
                };
                Quantity::of(f64::from(px), LENGTH)
            }
            TokenOrValue::Angle(angle) => Quantity::of(f64::from(angle.to_degrees()), ANGLE),
            TokenOrValue::Time(time) => Quantity::of(f64::from(time.to_ms()), TIME),
            TokenOrValue::Token(Token::Ident(ident)) => {
                Quantity::number(match ident.to_ascii_lowercase().as_str() {
                    "e" => std::f64::consts::E,
                    "pi" => std::f64::consts::PI,
                    "infinity" => f64::INFINITY,
                    "-infinity" => f64::NEG_INFINITY,
                    "nan" => f64::NAN,
                    _ => return None,
                })
            }
            TokenOrValue::Token(Token::ParenthesisBlock) => {
                let value = self.sum(tokens, position)?;
                match tokens.get(*position) {
                    Some(TokenOrValue::Token(Token::CloseParenthesis)) => *position += 1,
                    _ => return None,
                }
                value
            }
            TokenOrValue::Function(function) => self.function(&function.name.0, &function.arguments.0)?,
            _ => return None,
        })
    }

    /// The arguments of a function, separated by commas, evaluated; `None` where one holds a
    /// percentage apart, which only layout can compare or round.
    fn arguments(&self, tokens: &[TokenOrValue<'_>]) -> Option<Vec<Quantity>> {
        split_commas(tokens)
            .into_iter()
            .map(|argument| self.expression(argument)?.plain())
            .collect()
    }

    fn function(&self, name: &str, tokens: &[TokenOrValue<'_>]) -> Option<Quantity> {
        let name = name.to_ascii_lowercase();
        let same = |values: &[Quantity]| values.windows(2).all(|pair| pair[0].powers == pair[1].powers);
        match name.as_str() {
            "calc" => self.expression(tokens),
            "min" | "max" => {
                let values = self.arguments(tokens)?;
                if values.is_empty() || !same(&values) {
                    return None;
                }
                let pick = if name == "min" { f64::min } else { f64::max };
                let first = values[0];
                Some(first.with(values.iter().map(|value| value.value).reduce(pick)?))
            }
            "clamp" => {
                let values = self.arguments(tokens)?;
                let [low, value, high] = values.as_slice() else {
                    return None;
                };
                if !same(&values) {
                    return None;
                }
                Some(value.with(value.value.min(high.value).max(low.value)))
            }
            "round" => self.round(tokens),
            "mod" | "rem" => {
                let values = self.arguments(tokens)?;
                let [a, b] = values.as_slice() else {
                    return None;
                };
                if a.powers != b.powers {
                    return None;
                }
                let value = if name == "mod" {
                    a.value - b.value * (a.value / b.value).floor()
                } else {
                    a.value % b.value
                };
                Some(a.with(value))
            }
            "abs" => {
                let value = self.single(tokens)?;
                Some(value.with(value.value.abs()))
            }
            "sign" => {
                let value = self.single(tokens)?.value;
                Some(Quantity::number(if value == 0.0 || value.is_nan() {
                    value
                } else {
                    value.signum()
                }))
            }
            "pow" => {
                let values = self.arguments(tokens)?;
                let [base, exponent] = values.as_slice() else {
                    return None;
                };
                (base.is_number() && exponent.is_number())
                    .then(|| Quantity::number(base.value.powf(exponent.value)))
            }
            "sqrt" | "exp" => {
                let value = self.single(tokens)?;
                value.is_number().then(|| {
                    Quantity::number(if name == "sqrt" {
                        value.value.sqrt()
                    } else {
                        value.value.exp()
                    })
                })
            }
            "log" => {
                let values = self.arguments(tokens)?;
                match values.as_slice() {
                    [value] if value.is_number() => Some(Quantity::number(value.value.ln())),
                    [value, base] if value.is_number() && base.is_number() => {
                        Some(Quantity::number(value.value.ln() / base.value.ln()))
                    }
                    _ => None,
                }
            }
            "hypot" => {
                let values = self.arguments(tokens)?;
                if values.is_empty() || !same(&values) {
                    return None;
                }
                Some(
                    values[0].with(
                        values
                            .iter()
                            .map(|value| value.value * value.value)
                            .sum::<f64>()
                            .sqrt(),
                    ),
                )
            }
            "sin" | "cos" | "tan" => {
                let value = self.single(tokens)?;
                let radians = if value.is(ANGLE) {
                    value.value.to_radians()
                } else if value.is_number() {
                    value.value
                } else {
                    return None;
                };
                Some(Quantity::number(match name.as_str() {
                    "sin" => radians.sin(),
                    "cos" => radians.cos(),
                    _ => radians.tan(),
                }))
            }
            "asin" | "acos" | "atan" => {
                let value = self.single(tokens)?;
                value.is_number().then(|| {
                    Quantity::of(
                        match name.as_str() {
                            "asin" => value.value.asin(),
                            "acos" => value.value.acos(),
                            _ => value.value.atan(),
                        }
                        .to_degrees(),
                        ANGLE,
                    )
                })
            }
            "atan2" => {
                let values = self.arguments(tokens)?;
                let [y, x] = values.as_slice() else {
                    return None;
                };
                (y.powers == x.powers).then(|| Quantity::of(y.value.atan2(x.value).to_degrees(), ANGLE))
            }
            "progress" => {
                let values = self.arguments(tokens)?;
                let [value, start, end] = values.as_slice() else {
                    return None;
                };
                if !same(&values) {
                    return None;
                }
                Some(Quantity::number(progress(value.value, start.value, end.value)))
            }
            _ => None,
        }
    }

    fn single(&self, tokens: &[TokenOrValue<'_>]) -> Option<Quantity> {
        match self.arguments(tokens)?.as_slice() {
            [value] => Some(*value),
            _ => None,
        }
    }

    /// `round(<strategy>?, A, B?)`: B is 1 where A is a number.
    fn round(&self, tokens: &[TokenOrValue<'_>]) -> Option<Quantity> {
        let mut parts = split_commas(tokens);
        let first = significant(parts.first()?);
        let strategy = match first.as_slice() {
            [TokenOrValue::Token(Token::Ident(ident))]
                if ["nearest", "up", "down", "to-zero"]
                    .iter()
                    .any(|known| ident.eq_ignore_ascii_case(known)) =>
            {
                let strategy = ident.to_ascii_lowercase();
                parts.remove(0);
                strategy
            }
            _ => "nearest".to_owned(),
        };
        let values = parts
            .into_iter()
            .map(|part| self.expression(part)?.plain())
            .collect::<Option<Vec<_>>>()?;
        let (value, step) = match values.as_slice() {
            [value] if value.is_number() => (*value, 1.0),
            [value, step] if value.powers == step.powers => (*value, step.value),
            _ => return None,
        };
        if step == 0.0 {
            return Some(value.with(f64::NAN));
        }
        let ratio = value.value / step;
        let rounded = match strategy.as_str() {
            "up" => ratio.ceil(),
            "down" => ratio.floor(),
            "to-zero" => ratio.trunc(),
            // Halfway rounds up (towards positive infinity).
            _ => (ratio + 0.5).floor(),
        };
        Some(value.with(rounded * step))
    }
}

/// Where a value lies from a start to an end, from 0 to 1 (`progress()`).
pub fn progress(value: f64, start: f64, end: f64) -> f64 {
    if (end - start).abs() < f64::EPSILON {
        0.0
    } else {
        ((value - start) / (end - start)).clamp(0.0, 1.0)
    }
}

fn significant<'t>(tokens: &'t [TokenOrValue<'t>]) -> Vec<&'t TokenOrValue<'t>> {
    tokens.iter().filter(|token| !token.is_whitespace()).collect()
}

/// Tokens split at the commas outside parentheses.
fn split_commas<'t>(tokens: &'t [TokenOrValue<'t>]) -> Vec<&'t [TokenOrValue<'t>]> {
    let mut parts = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    for (index, token) in tokens.iter().enumerate() {
        depth += nesting(token);
        if depth == 0 && matches!(token, TokenOrValue::Token(Token::Comma)) {
            parts.push(&tokens[start..index]);
            start = index + 1;
        }
    }
    parts.push(&tokens[start..]);
    parts
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightningcss::properties::custom::TokenList;

    fn function(css: &str) -> Function<'static> {
        let tokens = crate::variables::value_tokens(css).unwrap_or_else(|| TokenList(Vec::new()));
        match tokens.0.into_iter().find(|token| !token.is_whitespace()) {
            Some(TokenOrValue::Function(function)) => function,
            other => panic!("no function: {other:?}"),
        }
    }

    fn value(css: &str, lengths: Lengths<'_>) -> Option<TokenOrValue<'static>> {
        evaluate_function(&function(css), lengths)?.token()
    }

    #[test]
    fn divides_typed_values() {
        assert!(fagus_css::divides_by_type(&function("calc(50px / 10px * 20px)")));
        assert!(!fagus_css::divides_by_type(&function("calc(50px / 2)")));
        assert_eq!(
            value("calc(50px / 10px * 20px)", None),
            Some(TokenOrValue::Length(LengthValue::Px(100.0)))
        );
        assert_eq!(
            value("calc(atan2(1px, 1px) / 1deg * 3px)", None),
            Some(TokenOrValue::Length(LengthValue::Px(135.0)))
        );
        assert_eq!(
            value("calc(180deg / 1turn * 300px)", None),
            Some(TokenOrValue::Length(LengthValue::Px(150.0)))
        );
        // Relative lengths as the element resolves them.
        let lengths = |length: &LengthValue| match length {
            LengthValue::Vw(vw) => Some(vw * 5.0),
            LengthValue::Rem(rem) => Some(rem * 16.0),
            other => other.to_px(),
        };
        assert_eq!(
            value("calc((100vw - 2rem) / 1px * 0.5px)", Some(&lengths)),
            Some(TokenOrValue::Length(LengthValue::Px(234.0)))
        );
        // A number alone stays a number; mixed types are invalid.
        assert!(matches!(
            value("calc(30px / 60px)", None),
            Some(TokenOrValue::Token(Token::Number { value, .. })) if (value - 0.5).abs() < 1e-6
        ));
        assert_eq!(value("calc(1px + 1deg)", None), None);
    }

    #[test]
    fn keeps_percentages_of_lengths_apart() {
        // A sum of a percentage and a length divided and multiplied by lengths: a `calc()` of
        // both for layout.
        let calc = |css: &str| match value(css, None) {
            Some(TokenOrValue::Function(function)) => {
                crate::variables::to_css(&function.arguments).map(|css| css.trim().to_owned())
            }
            other => panic!("a calc(): {other:?}"),
        };
        assert_eq!(
            calc("calc((100% - 20px) / 2px * 1px)").as_deref(),
            Some("-10px + 50.0%")
        );
        assert_eq!(
            calc("calc(1px * (100% - 40px) / 4px)").as_deref(),
            Some("-10px + 25.0%")
        );
        assert_eq!(
            calc("calc((100% / 2px) * 1px + 10px)").as_deref(),
            Some("10px + 50.0%")
        );
        // A percentage alone stays one.
        assert!(matches!(
            value("calc(100% / 1px * 0.25px)", None),
            Some(TokenOrValue::Token(Token::Percentage { unit_value, .. })) if (unit_value - 0.25).abs() < 1e-6
        ));
        // What would compare, round or multiply percentages needs layout.
        assert_eq!(value("calc(round(down, 100% / 3px) * 1px)", None), None);
        assert_eq!(value("calc((100% - 10px) * (100% - 10px) / 1px)", None), None);
        assert_eq!(value("calc(1px / (100% - 10px) * 1px)", None), None);
    }

    #[test]
    fn progress_takes_font_relative_lengths() {
        let lengths = |length: &LengthValue| match length {
            LengthValue::Em(em) => Some(em * 12.0),
            other => other.to_px(),
        };
        assert!(matches!(
            value("progress(1em, 0px, 24px)", Some(&lengths)),
            Some(TokenOrValue::Token(Token::Number { value, .. })) if (value - 0.5).abs() < 1e-6
        ));
        // Without the element's font, a relative length is unknown.
        assert_eq!(value("progress(1em, 0px, 24px)", None), None);
    }
}

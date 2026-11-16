//! `shape-outside` (CSS Shapes 1, §6.1), which lightningcss keeps as tokens.

use std::sync::Arc;

use lightningcss::properties::custom::{Token, TokenList, TokenOrValue};
use lightningcss::properties::masking::ClipPath as CssClipPath;
use lightningcss::traits::Parse;
use lightningcss::values::image::Image as CssImage;

use crate::compute::Context;
use crate::values::{ClipShape, ShapeBox, ShapeOutside};

/// The value of a `shape-outside` declaration: `Some(None)` for `none`, `None` if it is not
/// valid.
#[allow(
    clippy::option_option,
    reason = "`none` is a valid value, unlike one that does not parse"
)]
pub(crate) fn shape_outside(
    tokens: &TokenList<'static>,
    context: &Context,
) -> Option<Option<Arc<ShapeOutside>>> {
    let css = crate::background::css(&tokens.0)?;
    let css = css.trim();
    if css.eq_ignore_ascii_case("none") {
        return Some(None);
    }
    if let Ok(image) = CssImage::parse_string(css)
        && !matches!(image, CssImage::None)
    {
        let image = crate::background::image(&image, context, None);
        return Some(Some(Arc::new(ShapeOutside::Image(image))));
    }
    // The box keyword, which lightningcss does not tell apart from its default.
    let reference = tokens
        .0
        .iter()
        .find_map(|token| match token {
            TokenOrValue::Token(Token::Ident(ident)) => match ident.to_ascii_lowercase().as_str() {
                "margin-box" => Some(ShapeBox::MarginBox),
                "border-box" => Some(ShapeBox::BorderBox),
                "padding-box" => Some(ShapeBox::PaddingBox),
                "content-box" => Some(ShapeBox::ContentBox),
                _ => None,
            },
            _ => None,
        })
        .unwrap_or(ShapeBox::MarginBox);
    // `path()`, which lightningcss does not know either.
    if let Some(path) = tokens.0.iter().find(
        |token| matches!(token, TokenOrValue::Function(function) if function.name.0.eq_ignore_ascii_case("path")),
    ) {
        let (even_odd, data) = crate::extra::path_shape(&crate::background::css(std::slice::from_ref(path))?)?;
        let shape = ClipShape::Path {
            even_odd,
            data: Arc::from(data),
        };
        return Some(Some(Arc::new(ShapeOutside::Shape { shape, reference })));
    }
    // `xywh()`, `rect()` and `shape()`, which lightningcss does not know, as the `inset()` and
    // `polygon()` they come to.
    let rewritten;
    let css = if tokens
        .0
        .iter()
        .any(|token| matches!(token, TokenOrValue::Function(function) if newer_shape(&function.name.0)))
    {
        rewritten = rewrite(tokens, context)?;
        rewritten.as_str()
    } else {
        css
    };
    let shape = match CssClipPath::parse_string(css).ok()? {
        CssClipPath::Box(_) => ClipShape::Box,
        CssClipPath::Shape { shape, .. } => context.basic_shape(&shape)?,
        CssClipPath::None | CssClipPath::Url(_) => return None,
    };
    Some(Some(Arc::new(ShapeOutside::Shape { shape, reference })))
}

/// A `clip-path` with `xywh()`, `rect()` or `shape()` (CSS Shapes 2), which lightningcss does not
/// parse, as the `inset()` or `polygon()` it comes to, with its reference box; `None` if it is
/// not valid.
pub(crate) fn newer_clip_path(
    tokens: &TokenList<'static>,
    context: &Context,
) -> Option<Arc<crate::values::ClipPath>> {
    let css = rewrite(tokens, context)?;
    let parsed = CssClipPath::parse_string(css.trim()).ok()?;
    context.clip_path(&parsed)
}

/// Whether a function is a shape of CSS Shapes 2 that lightningcss does not parse.
fn newer_shape(name: &str) -> bool {
    ["xywh", "rect", "shape"]
        .iter()
        .any(|known| name.eq_ignore_ascii_case(known))
}

/// The CSS of a value with `xywh()`, `rect()` and `shape()` written as `inset()` and `polygon()`.
fn rewrite(tokens: &TokenList<'static>, context: &Context) -> Option<String> {
    use std::fmt::Write;
    let mut css = String::new();
    for token in &tokens.0 {
        match token {
            TokenOrValue::Function(function) if function.name.0.eq_ignore_ascii_case("xywh") => {
                let (values, round) = values_and_round(&function.arguments)?;
                let [x, y, width, height] = values.as_slice() else {
                    return None;
                };
                write!(
                    css,
                    "inset({y} calc(100% - {x} - {width}) calc(100% - {y} - {height}) {x}{round})"
                )
                .ok()?;
            }
            TokenOrValue::Function(function) if function.name.0.eq_ignore_ascii_case("rect") => {
                let (values, round) = values_and_round(&function.arguments)?;
                let [top, right, bottom, left] = values.as_slice() else {
                    return None;
                };
                // `auto` puts an edge on the box's own.
                let near = |value: &str| {
                    if value.eq_ignore_ascii_case("auto") {
                        "0px".to_owned()
                    } else {
                        value.to_owned()
                    }
                };
                let far = |value: &str| {
                    if value.eq_ignore_ascii_case("auto") {
                        "0px".to_owned()
                    } else {
                        format!("calc(100% - {value})")
                    }
                };
                write!(
                    css,
                    "inset({} {} {} {}{round})",
                    near(top),
                    far(right),
                    far(bottom),
                    near(left)
                )
                .ok()?;
            }
            TokenOrValue::Function(function) if function.name.0.eq_ignore_ascii_case("shape") => {
                css.push_str(&path::polygon(&function.arguments, context)?);
            }
            other => css.push_str(&crate::background::css(std::slice::from_ref(other))?),
        }
    }
    Some(css)
}

/// The values of `xywh()` or `rect()` apart, with the ` round …` that follows them.
fn values_and_round(arguments: &TokenList<'static>) -> Option<(Vec<String>, String)> {
    let mut values = Vec::new();
    let mut round = String::new();
    let mut tokens = arguments
        .0
        .iter()
        .filter(|token| !matches!(token, TokenOrValue::Token(Token::WhiteSpace(_))));
    while let Some(token) = tokens.next() {
        if matches!(token, TokenOrValue::Token(Token::Ident(ident)) if ident.eq_ignore_ascii_case("round")) {
            let rest: Vec<TokenOrValue<'static>> = tokens
                .flat_map(|token| [token.clone(), TokenOrValue::Token(Token::WhiteSpace(" ".into()))])
                .collect();
            round = format!(" round {}", crate::background::css(&rest)?);
            break;
        }
        values.push(crate::background::css(std::slice::from_ref(token))?);
    }
    Some((values, round))
}

/// `shape()` (CSS Shapes 2, §3.2) as the `polygon()` its path comes to: lines as they are,
/// curves and arcs cut into short lines.
mod path {
    use lightningcss::properties::custom::{Token, TokenList, TokenOrValue};
    use lightningcss::traits::Parse;
    use lightningcss::values::length::LengthPercentage as CssLengthPercentage;

    use crate::compute::Context;

    /// A coordinate: pixels and a fraction of the reference box, which a point of a curve
    /// mixes linearly.
    #[derive(Clone, Copy, Default)]
    struct Coordinate {
        px: f32,
        fraction: f32,
    }

    impl Coordinate {
        fn scale(self, factor: f32) -> Self {
            Self {
                px: self.px * factor,
                fraction: self.fraction * factor,
            }
        }

        fn plus(self, other: Self) -> Self {
            Self {
                px: self.px + other.px,
                fraction: self.fraction + other.fraction,
            }
        }

        fn css(self) -> String {
            if self.fraction == 0.0 {
                format!("{}px", self.px)
            } else if self.px == 0.0 {
                format!("{}%", self.fraction * 100.0)
            } else {
                format!("calc({}px + {}%)", self.px, self.fraction * 100.0)
            }
        }
    }

    type Point = (Coordinate, Coordinate);

    /// Whether two points are the same.
    fn same(a: Point, b: Point) -> bool {
        let close = |a: Coordinate, b: Coordinate| {
            (a.px - b.px).abs() < 1e-4 && (a.fraction - b.fraction).abs() < 1e-6
        };
        close(a.0, b.0) && close(a.1, b.1)
    }

    fn mix(points: &[Point], weights: &[f32]) -> Point {
        points
            .iter()
            .zip(weights)
            .fold(Point::default(), |sum, (point, weight)| {
                (
                    sum.0.plus(point.0.scale(*weight)),
                    sum.1.plus(point.1.scale(*weight)),
                )
            })
    }

    /// How many lines a curve is cut into.
    const STEPS: u16 = 16;

    /// A length or percentage, as a coordinate: a calculation of both is taken as the sum of a
    /// length and a percentage.
    fn coordinate(token: &TokenOrValue<'static>, context: &Context) -> Option<Coordinate> {
        let css = crate::background::css(std::slice::from_ref(token))?;
        let value = context.length_percentage(&CssLengthPercentage::parse_string(css.trim()).ok()?);
        let px = value.resolve(0.0);
        let fraction = (value.resolve(100.0) - px) / 100.0;
        (px.is_finite() && fraction.is_finite()).then_some(Coordinate { px, fraction })
    }

    /// The words and values of one command, without white space.
    fn words<'t>(tokens: &'t [TokenOrValue<'static>]) -> Vec<&'t TokenOrValue<'static>> {
        tokens
            .iter()
            .filter(|token| !matches!(token, TokenOrValue::Token(Token::WhiteSpace(_))))
            .collect()
    }

    fn is(token: Option<&&TokenOrValue<'static>>, word: &str) -> bool {
        matches!(token, Some(TokenOrValue::Token(Token::Ident(ident))) if ident.eq_ignore_ascii_case(word))
    }

    /// The keyword of a word, in lower case.
    fn keyword(token: Option<&&TokenOrValue<'static>>) -> Option<String> {
        match token {
            Some(TokenOrValue::Token(Token::Ident(ident))) => Some(ident.to_ascii_lowercase()),
            _ => None,
        }
    }

    /// The fraction of the box that a keyword of a position stands for, and whether it is
    /// horizontal (`Some(true)`), vertical or either (`center`).
    fn edge(keyword: &str) -> Option<(f32, Option<bool>)> {
        Some(match keyword {
            "left" => (0.0, Some(true)),
            "right" => (1.0, Some(true)),
            "top" => (0.0, Some(false)),
            "bottom" => (1.0, Some(false)),
            "center" => (0.5, None),
            _ => return None,
        })
    }

    /// A `<position>` of one or two values at the start of `words`, and how many words it
    /// takes; whether it has a keyword.
    fn position(words: &[&TokenOrValue<'static>], context: &Context) -> Option<(Point, usize, bool)> {
        enum Part {
            Edge(f32, Option<bool>),
            Length(Coordinate),
        }
        let part = |word: Option<&&TokenOrValue<'static>>| -> Option<Part> {
            if let Some(keyword) = keyword(word) {
                let (fraction, horizontal) = edge(&keyword)?;
                return Some(Part::Edge(fraction, horizontal));
            }
            coordinate(word?, context).map(Part::Length)
        };
        let fraction = |fraction: f32| Coordinate { px: 0.0, fraction };
        let first = part(words.first())?;
        let second = part(words.get(1));
        let keyword = matches!(first, Part::Edge(..)) || matches!(second, Some(Part::Edge(..)));
        let point = match (first, second) {
            (Part::Edge(at, horizontal), None) => match horizontal {
                Some(false) => (fraction(0.5), fraction(at)),
                _ => (fraction(at), fraction(0.5)),
            },
            (Part::Length(x), None) => (x, fraction(0.5)),
            (first, Some(second)) => {
                // A vertical keyword first, or a horizontal one second, swaps them.
                let swap = matches!(first, Part::Edge(_, Some(false)))
                    || matches!(second, Part::Edge(_, Some(true)));
                let (x, y) = if swap { (second, first) } else { (first, second) };
                let x = match x {
                    Part::Edge(at, Some(true) | None) => fraction(at),
                    Part::Length(length) => length,
                    Part::Edge(_, Some(false)) => return None,
                };
                let y = match y {
                    Part::Edge(at, Some(false) | None) => fraction(at),
                    Part::Length(length) => length,
                    Part::Edge(_, Some(true)) => return None,
                };
                return Some(((x, y), 2, keyword));
            }
        };
        Some((point, 1, keyword))
    }

    /// A point after `to` (a position, from the box's corner) or `by` (a pair of coordinates,
    /// from the current point), and how many words it takes with `to` or `by`; whether it is
    /// one of `to`.
    fn target(
        words: &[&TokenOrValue<'static>],
        current: Point,
        context: &Context,
    ) -> Option<(Point, usize, bool)> {
        if is(words.first(), "by") {
            let x = coordinate(words.get(1)?, context)?;
            let y = coordinate(words.get(2)?, context)?;
            Some(((current.0.plus(x), current.1.plus(y)), 3, false))
        } else if is(words.first(), "to") {
            let (point, used, _) = position(&words[1..], context)?;
            Some((point, used + 1, true))
        } else {
            None
        }
    }

    /// A control point of a curve at the start of `words`, from the end of a command `to` or
    /// `by`, and how many words it takes: a position (with keywords, from the box's corner) or a
    /// pair of coordinates from the box's corner (`origin`, by default after `to`), the start of
    /// the curve (`start`, by default after `by`) or its end (`end`).
    fn control_point(
        words: &[&TokenOrValue<'static>],
        to: bool,
        (start, end): (Point, Point),
        context: &Context,
    ) -> Option<(Point, usize)> {
        let (point, used) = if to {
            let (point, used, keyword) = position(words, context)?;
            if keyword {
                return Some((point, used));
            }
            (point, used)
        } else {
            (
                (
                    coordinate(words.first()?, context)?,
                    coordinate(words.get(1)?, context)?,
                ),
                2,
            )
        };
        let (base, used) = if is(words.get(used), "from") {
            let base = match keyword(words.get(used + 1))?.as_str() {
                "start" => start,
                "end" => end,
                "origin" => Point::default(),
                _ => return None,
            };
            (base, used + 2)
        } else if to {
            (Point::default(), used)
        } else {
            (start, used)
        };
        Some(((base.0.plus(point.0), base.1.plus(point.1)), used))
    }

    pub(super) fn polygon(arguments: &TokenList<'static>, context: &Context) -> Option<String> {
        let mut commands = Vec::new();
        let mut start = 0;
        for (index, token) in arguments.0.iter().enumerate() {
            if matches!(token, TokenOrValue::Token(Token::Comma)) {
                commands.push(&arguments.0[start..index]);
                start = index + 1;
            }
        }
        commands.push(&arguments.0[start..]);
        let first = words(commands.first()?);
        let (even_odd, from) = if is(first.first(), "evenodd") || is(first.first(), "nonzero") {
            (is(first.first(), "evenodd"), &first[1..])
        } else {
            (false, &first[..])
        };
        if !is(from.first(), "from") {
            return None;
        }
        let mut current = position(&from[1..], context)?.0;
        let mut points = vec![current];
        // The starts of the subpaths (`move`): the polygon goes from each to the next and back
        // the same way at its end, edges that cancel out, so that each subpath fills as its own.
        let mut starts = vec![current];
        // The second control point of the last curve, for `smooth`.
        let mut last_control: Option<Point> = None;
        for command in &commands[1..] {
            let words = words(command);
            let name = words.first()?;
            let rest = &words[1..];
            let mut control = None;
            if is(Some(name), "move") {
                let start = *starts.last()?;
                if !same(*points.last()?, start) {
                    // The subpath closes before the next starts.
                    points.push(start);
                }
                current = target(rest, current, context)?.0;
                points.push(current);
                starts.push(current);
            } else if is(Some(name), "line") {
                current = target(rest, current, context)?.0;
                points.push(current);
            } else if is(Some(name), "hline") || is(Some(name), "vline") {
                let horizontal = is(Some(name), "hline");
                let by = is(rest.first(), "by");
                let value = match keyword(rest.get(1)) {
                    Some(keyword) if !by => match edge(&keyword)? {
                        (at, None) => Coordinate {
                            px: 0.0,
                            fraction: at,
                        },
                        (at, Some(across)) if across == horizontal => Coordinate {
                            px: 0.0,
                            fraction: at,
                        },
                        _ => return None,
                    },
                    _ => coordinate(rest.get(1)?, context)?,
                };
                let axis = if horizontal {
                    &mut current.0
                } else {
                    &mut current.1
                };
                *axis = if by { axis.plus(value) } else { value };
                points.push(current);
            } else if is(Some(name), "curve") || is(Some(name), "smooth") {
                let (end, used, to) = target(rest, current, context)?;
                let after = &rest[used..];
                let mut controls = Vec::new();
                if is(Some(name), "smooth") {
                    // The reflection of the last control point, or the current point.
                    let reflected = last_control.map_or(current, |control: Point| {
                        (
                            current.0.scale(2.0).plus(control.0.scale(-1.0)),
                            current.1.scale(2.0).plus(control.1.scale(-1.0)),
                        )
                    });
                    controls.push(reflected);
                }
                if is(after.first(), "with") {
                    let mut at = 1;
                    loop {
                        let (point, used) = control_point(&after[at..], to, (current, end), context)?;
                        controls.push(point);
                        at += used;
                        if !matches!(after.get(at), Some(TokenOrValue::Token(Token::Delim('/')))) {
                            break;
                        }
                        at += 1;
                    }
                    if at != after.len() {
                        return None;
                    }
                }
                // A quadratic curve with one control point, a cubic one with two.
                if !(1..=2).contains(&controls.len()) {
                    return None;
                }
                let mut hull = vec![current];
                hull.extend(controls.iter().copied());
                hull.push(end);
                control = Some(hull[hull.len() - 2]);
                for step in 1..=STEPS {
                    let t = f32::from(step) / f32::from(STEPS);
                    let u = 1.0 - t;
                    let weights = if hull.len() == 3 {
                        vec![u * u, 2.0 * u * t, t * t]
                    } else {
                        vec![u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t]
                    };
                    points.push(mix(&hull, &weights));
                }
                current = end;
            } else if is(Some(name), "arc") {
                // Arcs are drawn as the line to their end.
                current = target(rest, current, context)?.0;
                points.push(current);
            } else if is(Some(name), "close") {
                current = *starts.last()?;
            } else {
                return None;
            }
            last_control = control;
        }
        if starts.len() > 1 {
            let start = *starts.last()?;
            if !same(*points.last()?, start) {
                points.push(start);
            }
            // Back through the starts of the subpaths before the last, to the first.
            points.extend(starts[1..starts.len() - 1].iter().rev().copied());
        }
        let list: Vec<String> = points
            .iter()
            .map(|(x, y)| format!("{} {}", x.css(), y.css()))
            .collect();
        Some(format!(
            "polygon({}{})",
            if even_odd { "evenodd, " } else { "" },
            list.join(", ")
        ))
    }
}

//! SVG path data (SVG 2, §9.3), as `clip-path: path()` and `shape-outside: path()` give it:
//! parsed into moves, lines, cubic curves and closes in absolute coordinates. Quadratic curves become cubic ones and
//! elliptical arcs cubic curves of at most a quarter turn each.

use crate::PathSegment;

/// The outline that path data `data` draws, moved by `origin`. Like browsers, it keeps what
/// comes before the first error; `None` where that is nothing.
pub fn parse(data: &str, origin: (f32, f32)) -> Option<Vec<PathSegment>> {
    let mut parser = Parser {
        bytes: data.as_bytes(),
        at: 0,
    };
    let mut segments = Vec::new();
    // The current point, the start of the subpath and the control point a smooth curve
    // reflects (with whether the last command was a cubic or a quadratic curve).
    let (mut x, mut y) = (0.0f32, 0.0f32);
    let mut start = (0.0f32, 0.0f32);
    let mut last_cubic: Option<(f32, f32)> = None;
    let mut last_quadratic: Option<(f32, f32)> = None;
    let mut command = None;
    loop {
        parser.skip_space();
        let Some(&byte) = parser.bytes.get(parser.at) else {
            break;
        };
        if byte.is_ascii_alphabetic() {
            // Path data starts with a move.
            if segments.is_empty() && !matches!(byte, b'M' | b'm') {
                break;
            }
            parser.at += 1;
            command = Some(byte);
        } else if command.is_none() || matches!(command, Some(b'Z' | b'z')) {
            break;
        }
        let Some(current) = command else {
            break;
        };
        let relative = current.is_ascii_lowercase();
        let (dx, dy) = if relative { (x, y) } else { (0.0, 0.0) };
        let mut cubic = None;
        let mut quadratic = None;
        match current.to_ascii_uppercase() {
            b'M' => {
                let Some([px, py]) = parser.numbers() else { break };
                (x, y) = (px + dx, py + dy);
                start = (x, y);
                segments.push(PathSegment::Move(x, y));
                // Further pairs after a move are lines.
                command = Some(if relative { b'l' } else { b'L' });
            }
            b'L' => {
                let Some([px, py]) = parser.numbers() else { break };
                (x, y) = (px + dx, py + dy);
                segments.push(PathSegment::Line(x, y));
            }
            b'H' => {
                let Some([px]) = parser.numbers() else { break };
                x = px + dx;
                segments.push(PathSegment::Line(x, y));
            }
            b'V' => {
                let Some([py]) = parser.numbers() else { break };
                y = py + dy;
                segments.push(PathSegment::Line(x, y));
            }
            b'C' => {
                let Some([x1, y1, x2, y2, px, py]) = parser.numbers() else {
                    break;
                };
                let control = (x2 + dx, y2 + dy);
                (x, y) = (px + dx, py + dy);
                segments.push(PathSegment::Cubic([x1 + dx, y1 + dy, control.0, control.1, x, y]));
                cubic = Some(control);
            }
            b'S' => {
                let Some([x2, y2, px, py]) = parser.numbers() else {
                    break;
                };
                let (x1, y1) = last_cubic.map_or((x, y), |(cx, cy)| (2.0 * x - cx, 2.0 * y - cy));
                let control = (x2 + dx, y2 + dy);
                (x, y) = (px + dx, py + dy);
                segments.push(PathSegment::Cubic([x1, y1, control.0, control.1, x, y]));
                cubic = Some(control);
            }
            b'Q' => {
                let Some([qx, qy, px, py]) = parser.numbers() else {
                    break;
                };
                let control = (qx + dx, qy + dy);
                let end = (px + dx, py + dy);
                segments.push(quadratic_to_cubic((x, y), control, end));
                (x, y) = end;
                quadratic = Some(control);
            }
            b'T' => {
                let Some([px, py]) = parser.numbers() else { break };
                let control = last_quadratic.map_or((x, y), |(qx, qy)| (2.0 * x - qx, 2.0 * y - qy));
                let end = (px + dx, py + dy);
                segments.push(quadratic_to_cubic((x, y), control, end));
                (x, y) = end;
                quadratic = Some(control);
            }
            b'A' => {
                let Some(arc) = parser.arc() else { break };
                let end = (arc.end.0 + dx, arc.end.1 + dy);
                arc_to_cubics((x, y), end, arc, &mut segments);
                (x, y) = end;
            }
            b'Z' => {
                segments.push(PathSegment::Close);
                (x, y) = start;
            }
            _ => break,
        }
        last_cubic = cubic;
        last_quadratic = quadratic;
        parser.skip_comma();
    }
    if segments.is_empty() {
        return None;
    }
    let moved = |(px, py): (f32, f32)| (px + origin.0, py + origin.1);
    Some(
        segments
            .into_iter()
            .map(|segment| match segment {
                PathSegment::Move(px, py) => {
                    let (px, py) = moved((px, py));
                    PathSegment::Move(px, py)
                }
                PathSegment::Line(px, py) => {
                    let (px, py) = moved((px, py));
                    PathSegment::Line(px, py)
                }
                PathSegment::Cubic([x1, y1, x2, y2, px, py]) => {
                    let (x1, y1) = moved((x1, y1));
                    let (x2, y2) = moved((x2, y2));
                    let (px, py) = moved((px, py));
                    PathSegment::Cubic([x1, y1, x2, y2, px, py])
                }
                PathSegment::Close => PathSegment::Close,
            })
            .collect(),
    )
}

/// The arguments of an elliptical arc command, its end point not yet made absolute.
#[derive(Clone, Copy)]
struct Arc {
    radii: (f32, f32),
    rotation: f32,
    large: bool,
    sweep: bool,
    end: (f32, f32),
}

struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn skip_space(&mut self) {
        while self.bytes.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
    }

    fn skip_comma(&mut self) {
        self.skip_space();
        if self.bytes.get(self.at) == Some(&b',') {
            self.at += 1;
        }
    }

    /// `N` numbers separated by whitespace or commas.
    fn numbers<const N: usize>(&mut self) -> Option<[f32; N]> {
        let mut values = [0.0; N];
        for (index, value) in values.iter_mut().enumerate() {
            if index > 0 {
                self.skip_comma();
            }
            *value = self.number()?;
        }
        Some(values)
    }

    fn arc(&mut self) -> Option<Arc> {
        let [rx, ry, rotation] = self.numbers()?;
        self.skip_comma();
        let large = self.flag()?;
        self.skip_comma();
        let sweep = self.flag()?;
        self.skip_comma();
        let [x, y] = self.numbers()?;
        Some(Arc {
            radii: (rx.abs(), ry.abs()),
            rotation,
            large,
            sweep,
            end: (x, y),
        })
    }

    /// A flag of an arc: a single `0` or `1`, which needs no separator after it.
    fn flag(&mut self) -> Option<bool> {
        self.skip_space();
        let flag = match self.bytes.get(self.at)? {
            b'0' => false,
            b'1' => true,
            _ => return None,
        };
        self.at += 1;
        Some(flag)
    }

    /// A number: sign, digits with an optional fraction, optional exponent.
    fn number(&mut self) -> Option<f32> {
        self.skip_space();
        let start = self.at;
        let digits = |parser: &mut Self| {
            let from = parser.at;
            while parser.bytes.get(parser.at).is_some_and(u8::is_ascii_digit) {
                parser.at += 1;
            }
            parser.at > from
        };
        if matches!(self.bytes.get(self.at), Some(b'+' | b'-')) {
            self.at += 1;
        }
        let mut any = digits(self);
        if self.bytes.get(self.at) == Some(&b'.') {
            self.at += 1;
            any |= digits(self);
        }
        if !any {
            self.at = start;
            return None;
        }
        if matches!(self.bytes.get(self.at), Some(b'e' | b'E')) {
            let mark = self.at;
            self.at += 1;
            if matches!(self.bytes.get(self.at), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if !digits(self) {
                self.at = mark;
            }
        }
        std::str::from_utf8(&self.bytes[start..self.at])
            .ok()?
            .parse()
            .ok()
    }
}

/// A quadratic curve from `from` over `control` to `to` as the cubic curve that draws it.
fn quadratic_to_cubic(from: (f32, f32), control: (f32, f32), to: (f32, f32)) -> PathSegment {
    let third = |a: f32, b: f32| a + (b - a) * 2.0 / 3.0;
    PathSegment::Cubic([
        third(from.0, control.0),
        third(from.1, control.1),
        third(to.0, control.0),
        third(to.1, control.1),
        to.0,
        to.1,
    ])
}

/// Appends an elliptical arc from `from` to `to` as cubic curves (SVG 2, Appendix B.2.4: the
/// center from the endpoints, radii scaled up where they are too small).
fn arc_to_cubics(from: (f32, f32), to: (f32, f32), arc: Arc, segments: &mut Vec<PathSegment>) {
    let (mut rx, mut ry) = arc.radii;
    if from == to {
        return;
    }
    if rx == 0.0 || ry == 0.0 {
        segments.push(PathSegment::Line(to.0, to.1));
        return;
    }
    let (sin, cos) = libm::sincosf(arc.rotation.to_radians());
    let (hx, hy) = ((from.0 - to.0) / 2.0, (from.1 - to.1) / 2.0);
    let x1 = cos * hx + sin * hy;
    let y1 = -sin * hx + cos * hy;
    let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lambda > 1.0 {
        let scale = lambda.sqrt();
        rx *= scale;
        ry *= scale;
    }
    let numerator = (rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1).max(0.0);
    let denominator = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut factor = if denominator > 0.0 {
        (numerator / denominator).sqrt()
    } else {
        0.0
    };
    if arc.large == arc.sweep {
        factor = -factor;
    }
    let cx1 = factor * rx * y1 / ry;
    let cy1 = -factor * ry * x1 / rx;
    let center = (
        cos * cx1 - sin * cy1 + f32::midpoint(from.0, to.0),
        sin * cx1 + cos * cy1 + f32::midpoint(from.1, to.1),
    );
    let angle = |ux: f32, uy: f32, vx: f32, vy: f32| {
        let sign = if ux * vy - uy * vx < 0.0 { -1.0 } else { 1.0 };
        let dot = (ux * vx + uy * vy) / (libm::hypotf(ux, uy) * libm::hypotf(vx, vy));
        sign * libm::acosf(dot.clamp(-1.0, 1.0))
    };
    let start = angle(1.0, 0.0, (x1 - cx1) / rx, (y1 - cy1) / ry);
    let mut sweep = angle(
        (x1 - cx1) / rx,
        (y1 - cy1) / ry,
        (-x1 - cx1) / rx,
        (-y1 - cy1) / ry,
    );
    let turn = std::f32::consts::TAU;
    if !arc.sweep && sweep > 0.0 {
        sweep -= turn;
    } else if arc.sweep && sweep < 0.0 {
        sweep += turn;
    }
    // At most a quarter turn per curve.
    let count = (sweep.abs() / std::f32::consts::FRAC_PI_2).ceil().max(1.0);
    let step = sweep / count;
    let handle = 4.0 / 3.0 * libm::tanf(step / 4.0);
    let point = |theta: f32| {
        let (s, c) = libm::sincosf(theta);
        let (px, py) = (rx * c, ry * s);
        (cos * px - sin * py + center.0, sin * px + cos * py + center.1)
    };
    let tangent = |theta: f32| {
        let (s, c) = libm::sincosf(theta);
        let (tx, ty) = (-rx * s, ry * c);
        (cos * tx - sin * ty, sin * tx + cos * ty)
    };
    let mut theta = start;
    // `count` is a small whole number of quarter turns.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    for index in 0..count as usize {
        let next = theta + step;
        let (p0, p3) = (point(theta), point(next));
        let (t0, t3) = (tangent(theta), tangent(next));
        let end = if index + 1 == count as usize { to } else { p3 };
        segments.push(PathSegment::Cubic([
            p0.0 + handle * t0.0,
            p0.1 + handle * t0.1,
            p3.0 - handle * t3.0,
            p3.1 - handle * t3.1,
            end.0,
            end.1,
        ]));
        theta = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: &[PathSegment], b: &[PathSegment]) -> bool {
        let values = |segment: &PathSegment| match *segment {
            PathSegment::Move(x, y) | PathSegment::Line(x, y) => vec![x, y],
            PathSegment::Cubic(points) => points.to_vec(),
            PathSegment::Close => Vec::new(),
        };
        a.len() == b.len()
            && a.iter().zip(b).all(|(a, b)| {
                std::mem::discriminant(a) == std::mem::discriminant(b)
                    && values(a).iter().zip(values(b)).all(|(x, y)| (x - y).abs() < 0.01)
            })
    }

    #[test]
    fn parses_absolute_and_relative_commands() {
        let segments = parse(
            "M0 60 C 120 0, 240 110, 400 50 S640 20 800 70 V 120 h-800 z",
            (10.0, 5.0),
        )
        .unwrap();
        let expected = [
            PathSegment::Move(10.0, 65.0),
            PathSegment::Cubic([130.0, 5.0, 250.0, 115.0, 410.0, 55.0]),
            // The first control point mirrors the last one of the curve before.
            PathSegment::Cubic([570.0, -5.0, 650.0, 25.0, 810.0, 75.0]),
            PathSegment::Line(810.0, 125.0),
            PathSegment::Line(10.0, 125.0),
            PathSegment::Close,
        ];
        assert!(close(&segments, &expected), "{segments:?}");
        // Pairs after a move are lines; numbers run together by signs and points.
        let segments = parse("m1-2 3.5.5-1e1,0", (0.0, 0.0)).unwrap();
        let expected = [
            PathSegment::Move(1.0, -2.0),
            PathSegment::Line(4.5, -1.5),
            PathSegment::Line(-5.5, -1.5),
        ];
        assert!(close(&segments, &expected), "{segments:?}");
    }

    #[test]
    fn turns_arcs_and_quadratic_curves_into_cubic_curves() {
        // A half circle of radius 10 in two quarters, ending exactly at its end point.
        let segments = parse("M0 0 A10 10 0 0 1 20 0", (0.0, 0.0)).unwrap();
        assert_eq!(segments.len(), 3, "{segments:?}");
        let PathSegment::Cubic([.., x, y]) = segments[1] else {
            panic!("{segments:?}");
        };
        // The sweep runs clockwise on screen: through the top of the circle.
        assert!((x - 10.0).abs() < 0.01 && (y + 10.0).abs() < 0.01, "{segments:?}");
        let PathSegment::Cubic([.., x, y]) = segments[2] else {
            panic!("{segments:?}");
        };
        assert!(x.to_bits() == 20f32.to_bits() && y.abs() < 1e-6, "{segments:?}");
        let segments = parse("M0 0 Q 30 30 60 0", (0.0, 0.0)).unwrap();
        assert!(close(
            &segments[1..],
            &[PathSegment::Cubic([20.0, 20.0, 40.0, 20.0, 60.0, 0.0])]
        ));
        // What follows an error is left out; nothing valid is nothing.
        assert_eq!(
            parse("M0 0 L10 10 X 5 5", (0.0, 0.0)).map(|segments| segments.len()),
            Some(2)
        );
        assert_eq!(parse("L 10 10", (0.0, 0.0)), None);
    }
}

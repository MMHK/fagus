//! `clip-path` basic shapes on SVG elements (CSS Masking 1, §6.1), written as the outline of an
//! SVG `<clipPath>` for the SVG renderer, which knows only clip path elements.

use std::fmt::Write as _;

use fagus_style::ComputedStyle;
use fagus_style::values::{ClipPath, ClipShape};

use crate::{ClipGeometry, Item, RectF};

/// The outline of the basic shape of `clip` in the reference box `(x, y, width, height)` of an
/// SVG element, as SVG path data, and whether it is filled by the even-odd rule; `None` for a
/// `url()`, which the SVG renders itself.
pub fn svg_clip_path(clip: &ClipPath, (x, y, width, height): (f32, f32, f32, f32)) -> Option<(String, bool)> {
    if matches!(clip.shape, ClipShape::Url(_)) {
        return None;
    }
    let border_box = RectF { x, y, width, height };
    let mut d = String::new();
    let mut even_odd = false;
    match crate::clip_item(&ComputedStyle::default(), clip, border_box) {
        Item::PushClip(rect, radii) => {
            let [top_left, top_right, bottom_right, bottom_left] = radii.unwrap_or_default();
            let (right, bottom) = (rect.x + rect.width, rect.y + rect.height);
            let _ = write!(d, "M{} {}", rect.x + top_left.0, rect.y);
            let _ = write!(d, "H{}", right - top_right.0);
            arc(&mut d, top_right, (right, rect.y + top_right.1));
            let _ = write!(d, "V{}", bottom - bottom_right.1);
            arc(&mut d, bottom_right, (right - bottom_right.0, bottom));
            let _ = write!(d, "H{}", rect.x + bottom_left.0);
            arc(&mut d, bottom_left, (rect.x, bottom - bottom_left.1));
            let _ = write!(d, "V{}", rect.y + top_left.1);
            arc(&mut d, top_left, (rect.x + top_left.0, rect.y));
            d.push('Z');
        }
        Item::PushClipShape(ClipGeometry::Ellipse { center, radii }) => {
            let (rx, ry) = radii;
            let _ = write!(
                d,
                "M{} {}A{rx} {ry} 0 1 1 {} {}A{rx} {ry} 0 1 1 {} {}Z",
                center.0 - rx,
                center.1,
                center.0 + rx,
                center.1,
                center.0 - rx,
                center.1
            );
        }
        Item::PushClipShape(ClipGeometry::Polygon {
            points,
            even_odd: rule,
        }) => {
            even_odd = rule;
            for (index, (px, py)) in points.iter().enumerate() {
                let _ = write!(d, "{}{px} {py}", if index == 0 { 'M' } else { 'L' });
            }
            d.push('Z');
        }
        Item::PushClipShape(ClipGeometry::Path {
            segments,
            even_odd: rule,
        }) => {
            even_odd = rule;
            for segment in segments {
                let _ = match segment {
                    fagus_layout::PathSegment::Move(px, py) => write!(d, "M{px} {py}"),
                    fagus_layout::PathSegment::Line(px, py) => write!(d, "L{px} {py}"),
                    fagus_layout::PathSegment::Cubic(values) => {
                        let values: Vec<String> = values.iter().map(f32::to_string).collect();
                        write!(d, "C{}", values.join(" "))
                    }
                    fagus_layout::PathSegment::Close => write!(d, "Z"),
                };
            }
        }
        _ => return None,
    }
    Some((d, even_odd))
}

/// An elliptical corner of radii `(rx, ry)` to `to`; nothing for a square corner.
fn arc(d: &mut String, (rx, ry): (f32, f32), to: (f32, f32)) {
    if rx > 0.0 && ry > 0.0 {
        let _ = write!(d, "A{rx} {ry} 0 0 1 {} {}", to.0, to.1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fagus_style::values::{BoxArea, Corners, LengthPercentage, ShapeRadius, TransformBox};

    fn clip(shape: ClipShape) -> ClipPath {
        ClipPath {
            shape,
            reference: BoxArea::BorderBox,
            svg_reference: TransformBox::FillBox,
        }
    }

    #[test]
    fn writes_basic_shapes_as_path_data() {
        let inset = clip(ClipShape::Inset {
            insets: [
                LengthPercentage::Px(10.0),
                LengthPercentage::Px(20.0),
                LengthPercentage::Px(30.0),
                LengthPercentage::Px(40.0),
            ],
            radii: Corners::default(),
        });
        assert_eq!(
            svg_clip_path(&inset, (0.0, 200.0, 100.0, 100.0)),
            Some(("M40 210H80V270H40V210Z".to_owned(), false))
        );
        let circle = clip(ClipShape::Ellipse {
            radii: (
                ShapeRadius::Length(LengthPercentage::Px(50.0)),
                ShapeRadius::Length(LengthPercentage::Px(50.0)),
            ),
            circle: true,
            center: (LengthPercentage::Percent(0.5), LengthPercentage::Percent(0.5)),
        });
        assert_eq!(
            svg_clip_path(&circle, (0.0, 0.0, 100.0, 100.0)),
            Some(("M0 50A50 50 0 1 1 100 50A50 50 0 1 1 0 50Z".to_owned(), false))
        );
        assert_eq!(
            svg_clip_path(&clip(ClipShape::Url("#c".into())), (0.0, 0.0, 1.0, 1.0)),
            None
        );
    }
}

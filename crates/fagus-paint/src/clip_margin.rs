//! Where boxes clip their overflow with `overflow-clip-margin` (CSS Overflow 4, §3.2), and where
//! replaced elements clip what they show (CSS Overflow 4, §3.1: by their `overflow`).

use fagus_style::ComputedStyle;
use fagus_style::values::{BoxArea, Overflow};

use crate::{Radii, RectF, UNCLIPPED};

/// The clip of a box that clips its overflow in both axes with `overflow: clip` (or paint
/// containment) where `overflow-clip-margin` moves it off the padding box: the box it names,
/// outset by the margin, with the border box's `radii` outset as much as the edges (Blink's
/// `BorderOutsetsForClipping` and `FloatRoundedRect::Radii::Outset`). `border` holds the widths
/// of the borders drawn and `shown` which sides are not cut off where the box is broken. `None`
/// where the clip stays at the padding box, and for a scroll container (`hidden`, `auto`,
/// `scroll`), which Chrome clips at its padding box whatever the margin.
pub(crate) fn box_clip(
    style: &ComputedStyle,
    border: [f32; 4],
    shown: [bool; 4],
    border_box: RectF,
    radii: Option<Radii>,
) -> Option<(RectF, Option<Radii>)> {
    let (area, margin) = style.overflow_clip_margin;
    if style.overflow_x != Overflow::Clip
        || style.overflow_y != Overflow::Clip
        || (area == BoxArea::PaddingBox && margin == 0.0)
    {
        return None;
    }
    // How far each edge (top, right, bottom, left) of the reference box lies inside the border box.
    let mut insets = match area {
        // `overflow-clip-margin` takes no margin box; the border box stands in for it.
        BoxArea::BorderBox | BoxArea::MarginBox => [0.0; 4],
        BoxArea::PaddingBox | BoxArea::ContentBox => border,
    };
    if area == BoxArea::ContentBox {
        // Percentages of padding refer to the containing block's width, which painting does not
        // know; the box's own width is close for most boxes (as for backgrounds).
        let basis = border_box.width;
        let padding = &style.padding;
        let sides = [&padding.top, &padding.right, &padding.bottom, &padding.left];
        for ((inset, side), shown) in insets.iter_mut().zip(sides).zip(shown) {
            // A side cut off where the box is broken has no padding either.
            if shown {
                *inset += side.resolve(basis).max(0.0);
            }
        }
    }
    Some(outset(border_box, insets, margin, radii))
}

/// The clip of what a replaced element shows: by its `overflow` (Chrome's user agent style
/// clips at the content box with `overflow-clip-margin: content-box`), where `hidden` clips as
/// `clip` does, since a replaced element is never a scroll container, and `visible` does not
/// clip in that axis (where the other clips, it clips at the padding box; where neither does,
/// only rounded corners cut it). `border_box` is the element's, `border` the widths of its
/// borders and `content` its content box. `None` where nothing is clipped.
pub(crate) fn replaced_clip(
    style: &ComputedStyle,
    border_box: RectF,
    border: [f32; 4],
    content: RectF,
    radii: Option<Radii>,
) -> Option<(RectF, Option<Radii>)> {
    let visible = (
        style.overflow_x == Overflow::Visible,
        style.overflow_y == Overflow::Visible,
    );
    let content_insets = [
        content.y - border_box.y,
        border_box.x + border_box.width - content.x - content.width,
        border_box.y + border_box.height - content.y - content.height,
        content.x - border_box.x,
    ];
    // Unclipped, it is not cut along the curves of its content edge either, as in Chrome (157,
    // measured; CSS Backgrounds 3, §5.3, would cut it there).
    if visible.0 && visible.1 {
        return None;
    }
    // Clipped in one axis only, it clips at its padding box (`overflow-clip-margin` applies
    // where both axes clip, as in Blink's `LayoutBox::OverflowClipRect`).
    let (area, margin) = if visible.0 || visible.1 {
        (BoxArea::PaddingBox, 0.0)
    } else {
        style.overflow_clip_margin
    };
    let insets = match area {
        // `overflow-clip-margin` takes no margin box; the border box stands in for it.
        BoxArea::BorderBox | BoxArea::MarginBox => [0.0; 4],
        BoxArea::PaddingBox => border,
        BoxArea::ContentBox => content_insets,
    };
    let (mut rect, radii) = outset(border_box, insets, margin, radii);
    if !visible.0 && !visible.1 {
        return Some((rect, radii));
    }
    // The clip reaches as far as anything may in the axis that does not clip.
    if visible.0 {
        rect.x -= UNCLIPPED;
        rect.width += 2.0 * UNCLIPPED;
    } else {
        rect.y -= UNCLIPPED;
        rect.height += 2.0 * UNCLIPPED;
    }
    Some((rect, None))
}

/// The box `insets` (top, right, bottom, left) inside `border_box`, outset by `margin` on every
/// side, and the border box's `radii` moved with its edges: a curved corner grows or shrinks by
/// as much as its edges move, a square one stays square.
fn outset(border_box: RectF, insets: [f32; 4], margin: f32, radii: Option<Radii>) -> (RectF, Option<Radii>) {
    let [top, right, bottom, left] = insets.map(|inset| margin - inset);
    let rect = RectF {
        x: border_box.x - left,
        y: border_box.y - top,
        width: (border_box.width + left + right).max(0.0),
        height: (border_box.height + top + bottom).max(0.0),
    };
    let grow = |radius: f32, outset: f32| {
        if radius > 0.0 {
            (radius + outset).max(0.0)
        } else {
            0.0
        }
    };
    let radii = radii
        .map(|[top_left, top_right, bottom_right, bottom_left]| {
            [
                (grow(top_left.0, left), grow(top_left.1, top)),
                (grow(top_right.0, right), grow(top_right.1, top)),
                (grow(bottom_right.0, right), grow(bottom_right.1, bottom)),
                (grow(bottom_left.0, left), grow(bottom_left.1, bottom)),
            ]
        })
        .filter(|radii| radii.iter().any(|(x, y)| *x > 0.0 && *y > 0.0));
    (rect, radii)
}

#[cfg(test)]
mod tests {
    use fagus_style::values::{LengthPercentage, Sides};

    use super::*;

    fn style(overflow: Overflow, area: BoxArea, margin: f32) -> ComputedStyle {
        ComputedStyle::from(fagus_style::ComputedValues {
            overflow_x: overflow,
            overflow_y: overflow,
            overflow_clip_margin: (area, margin),
            padding: Sides::all(LengthPercentage::Px(5.0)),
            ..fagus_style::ComputedValues::default()
        })
    }

    const BORDER_BOX: RectF = RectF {
        x: 10.0,
        y: 20.0,
        width: 100.0,
        height: 50.0,
    };

    #[test]
    fn moves_overflow_clips_by_their_margin() {
        let border = [2.0; 4];
        let shown = [true; 4];
        // The padding box, 2px inside the border box, outset by 10px.
        let (rect, radii) = box_clip(
            &style(Overflow::Clip, BoxArea::PaddingBox, 10.0),
            border,
            shown,
            BORDER_BOX,
            None,
        )
        .expect("clip");
        assert_eq!(
            (rect.x, rect.y, rect.width, rect.height),
            (2.0, 12.0, 116.0, 66.0)
        );
        assert!(radii.is_none());
        // The content box, also inside the padding; corners curved by 20px shrink by 7px.
        let (rect, radii) = box_clip(
            &style(Overflow::Clip, BoxArea::ContentBox, 0.0),
            border,
            shown,
            BORDER_BOX,
            Some([(20.0, 20.0); 4]),
        )
        .expect("clip");
        assert_eq!(
            (rect.x, rect.y, rect.width, rect.height),
            (17.0, 27.0, 86.0, 36.0)
        );
        assert_eq!(radii, Some([(13.0, 13.0); 4]));
        // Square corners stay square at the border box outset.
        let (rect, _) = box_clip(
            &style(Overflow::Clip, BoxArea::BorderBox, 4.0),
            border,
            shown,
            BORDER_BOX,
            None,
        )
        .expect("clip");
        assert_eq!((rect.x, rect.width), (6.0, 108.0));
        // A scroll container clips at its padding box whatever the margin, and so does a box
        // without one.
        assert!(
            box_clip(
                &style(Overflow::Hidden, BoxArea::BorderBox, 10.0),
                border,
                shown,
                BORDER_BOX,
                None
            )
            .is_none()
        );
        assert!(
            box_clip(
                &style(Overflow::Clip, BoxArea::PaddingBox, 0.0),
                border,
                shown,
                BORDER_BOX,
                None
            )
            .is_none()
        );
    }

    #[test]
    fn clips_replaced_content_as_its_overflow_says() {
        let border = [2.0; 4];
        let content = RectF {
            x: 17.0,
            y: 27.0,
            width: 86.0,
            height: 36.0,
        };
        // Chrome's default: the content box.
        let (rect, _) = replaced_clip(
            &style(Overflow::Clip, BoxArea::ContentBox, 0.0),
            BORDER_BOX,
            border,
            content,
            None,
        )
        .expect("clip");
        assert_eq!(rect, content);
        // `hidden` takes the margin as `clip` does: a replaced element does not scroll.
        let (rect, _) = replaced_clip(
            &style(Overflow::Hidden, BoxArea::PaddingBox, 10.0),
            BORDER_BOX,
            border,
            content,
            None,
        )
        .expect("clip");
        assert_eq!((rect.x, rect.width), (2.0, 116.0));
        assert!(
            replaced_clip(
                &style(Overflow::Visible, BoxArea::ContentBox, 0.0),
                BORDER_BOX,
                border,
                content,
                None
            )
            .is_none()
        );
        // Nor along rounded corners (WPT css-overflow overflow-img-border-radius).
        assert!(
            replaced_clip(
                &style(Overflow::Visible, BoxArea::ContentBox, 0.0),
                BORDER_BOX,
                border,
                content,
                Some([(20.0, 20.0); 4])
            )
            .is_none()
        );
        // One axis clips at the padding box, the other not at all.
        let one_axis = ComputedStyle::from(fagus_style::ComputedValues {
            overflow_y: Overflow::Visible,
            ..fagus_style::ComputedValues::from(style(Overflow::Clip, BoxArea::ContentBox, 0.0))
        });
        let (rect, _) = replaced_clip(&one_axis, BORDER_BOX, border, content, None).expect("clip");
        assert_eq!((rect.x, rect.width), (12.0, 96.0));
        assert!(rect.y < -1000.0);
    }
}

//! Transformed, relatively positioned and sticky boxes that show on other pages than their own.
//!
//! Chrome lays the page areas of a document end to end and paints the content of each page in
//! that flow: a box moved by a transform or a relative offset past the top or bottom of its page
//! area shows on the page it lands on, as far as it reaches there (WPT `css-break/transform-024`,
//! where a box rotated around its container's fragment on the first page lands on the third;
//! Chrome 157, measured with `translateY()`, `scale()`, `top` and sticky boxes, which it moves in
//! that flow as well: a box of a later page shows
//! over the content of the page it moves up onto, one of an earlier page under the content of the
//! page it moves down onto). Layout places such boxes by their untransformed boxes, so the pages
//! they reach onto are found here, from where they are painted (see `carried`).

use fagus_layout::units::Au;
use fagus_layout::{Fragment, FragmentKind, Page};
use fagus_style::values::Position;

/// A box of another page that shows on a page.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Stitch {
    /// The page whose content holds the box.
    pub(crate) source: usize,
    /// The path of child indices from that page's content fragment to the box.
    pub(crate) path: Vec<usize>,
    /// How far the content of that page is moved to its place on this page.
    pub(crate) shift: (Au, Au),
    /// Whether the box comes from a later page, and so goes over the page's own content.
    pub(crate) after: bool,
}

/// How many pages away from its own page a box is shown at most.
const REACH: usize = 16;

/// For each page, the boxes of other pages that show on it.
pub(crate) fn stitches(pages: &[Page]) -> Vec<Vec<Stitch>> {
    let mut found: Vec<Vec<Stitch>> = vec![Vec::new(); pages.len()];
    // Shrunk pages place their content otherwise; their boxes stay on their pages.
    if pages.len() < 2 || pages.iter().any(|page| (page.scale - 1.0).abs() > 1e-6) {
        return found;
    }
    // Where each page area starts in the flow of page areas laid end to end, as tall as layout
    // makes them (rounded up to whole pixels).
    let heights: Vec<Au> = pages
        .iter()
        .map(|page| Au::from_px(page.area.height.to_px().ceil()))
        .collect();
    let starts: Vec<Au> = heights
        .iter()
        .scan(Au::ZERO, |start, &height| {
            let this = *start;
            *start += height;
            Some(this)
        })
        .collect();
    for (source, page) in pages.iter().enumerate() {
        let Some(content) = page.fragments.first() else {
            continue;
        };
        let mut moved = Vec::new();
        collect(
            content,
            &mut Vec::new(),
            (content.rect.x, content.rect.y),
            &mut moved,
        );
        for (path, (top, bottom)) in moved {
            // The box's extent in the flow, against the page areas of the other pages.
            let area = page.area;
            let (top, bottom) = (starts[source] + top - area.y, starts[source] + bottom - area.y);
            let first = source.saturating_sub(REACH);
            let last = (source + REACH).min(pages.len() - 1);
            for target in (first..=last).filter(|&target| target != source) {
                // (Not where it reaches less than a pixel onto the page: the edges of a box
                // turned in place, which its own page area cuts off.)
                let overlap = bottom.min(starts[target] + heights[target]) - top.max(starts[target]);
                if overlap < Au::from_px(1.0) {
                    continue;
                }
                let to = pages[target].area;
                found[target].push(Stitch {
                    source,
                    path: path.clone(),
                    shift: (to.x - area.x, to.y - area.y - (starts[target] - starts[source])),
                    after: target < source,
                });
            }
        }
    }
    found
}

/// Collects the boxes below `fragment` (reached by `path`, its border box at `origin` on the
/// page) that a transform, a relative offset or a sticky one moves, with the top and bottom of what they
/// paint on the page (the outermost of such boxes, which carry what they hold).
fn collect(
    fragment: &Fragment,
    path: &mut Vec<usize>,
    origin: (Au, Au),
    out: &mut Vec<(Vec<usize>, (Au, Au))>,
) {
    for (index, child) in fragment.children.iter().enumerate() {
        let at = (origin.0 + child.rect.x, origin.1 + child.rect.y);
        path.push(index);
        match &child.kind {
            FragmentKind::Box { style, .. } if style.has_transform() => {
                let (_, top, _, bottom) = painted(child, origin);
                if top.is_finite() && bottom.is_finite() {
                    out.push((path.clone(), (Au::from_px(top), Au::from_px(bottom))));
                }
            }
            FragmentKind::Box { style, .. }
                if matches!(style.position, Position::Relative | Position::Sticky)
                    && !(style.inset.top.is_auto() && style.inset.bottom.is_auto()) =>
            {
                let (_, top, _, bottom) = painted(child, origin);
                if top.is_finite() && bottom.is_finite() {
                    out.push((path.clone(), (Au::from_px(top), Au::from_px(bottom))));
                }
            }
            FragmentKind::Box { .. } | FragmentKind::Anonymous | FragmentKind::Line { .. } => {
                collect(child, path, at, out);
            }
            _ => {}
        }
        path.pop();
    }
}

/// The rectangle that `fragment` (whose parent lies at `origin`) and all it holds paint in, in
/// the coordinates of the page, their transforms applied: left, top, right, bottom in pixels.
fn painted(fragment: &Fragment, origin: (Au, Au)) -> (f32, f32, f32, f32) {
    let (x, y) = (origin.0 + fragment.rect.x, origin.1 + fragment.rect.y);
    let own = (
        x.to_px(),
        y.to_px(),
        (x + fragment.rect.width).to_px(),
        (y + fragment.rect.height).to_px(),
    );
    let (x0, y0, x1, y1) = fragment.children.iter().fold(own, |(x0, y0, x1, y1), child| {
        let (cx0, cy0, cx1, cy1) = painted(child, (x, y));
        (x0.min(cx0), y0.min(cy0), x1.max(cx1), y1.max(cy1))
    });
    let FragmentKind::Box { style, .. } = &fragment.kind else {
        return (x0, y0, x1, y1);
    };
    if !style.has_transform() {
        return (x0, y0, x1, y1);
    }
    let reference = (own.0, own.1, own.2 - own.0, own.3 - own.1);
    let Some(matrix) = crate::transform::matrix(style, reference, None) else {
        return (x0, y0, x1, y1);
    };
    [(x0, y0), (x1, y0), (x0, y1), (x1, y1)]
        .map(|(x, y)| {
            (
                matrix[0] * x + matrix[2] * y + matrix[4],
                matrix[1] * x + matrix[3] * y + matrix[5],
            )
        })
        .iter()
        .fold(
            (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY),
            |(x0, y0, x1, y1), &(x, y)| (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
        )
}

/// A copy of the box at `path` below `fragment` with the boxes around it that lead there (each
/// without its other children), and what each part copies (see `carried::Shape`).
pub(crate) fn along(fragment: &Fragment, path: &[usize]) -> Option<(Fragment, crate::carried::Shape)> {
    let source = std::ptr::from_ref(fragment);
    match path.split_first() {
        None => Some((fragment.clone(), crate::carried::Shape::whole(source))),
        Some((&index, rest)) => {
            let (child, shape) = along(fragment.children.get(index)?, rest)?;
            Some((
                Fragment {
                    rect: fragment.rect,
                    kind: fragment.kind.clone(),
                    children: vec![child].into(),
                },
                crate::carried::Shape::around(source, shape),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use fagus_layout::units::Rect;
    use fagus_layout::{PageMarks, PageOrientation, VisibleSides};
    use fagus_style::ComputedStyle;
    use fagus_style::values::{LengthPercentage, LengthPercentageAuto, TransformFunction};

    use super::*;

    fn px(value: f32) -> Au {
        Au::from_px(value)
    }

    fn boxed(style: ComputedStyle, (y, height): (f32, f32), children: Vec<Fragment>) -> Fragment {
        Fragment {
            rect: Rect::new(Au::ZERO, px(y), px(100.0), px(height)),
            kind: FragmentKind::Box {
                style: Arc::new(style),
                node: None,
                sides: VisibleSides::ALL,
            },
            children: children.into(),
        }
    }

    /// A page 100px tall without margins, with `content`.
    fn page(content: Fragment) -> Page {
        Page {
            width: px(100.0),
            height: px(100.0),
            fragments: vec![content],
            margin: [Au::ZERO; 4],
            style: Arc::new(ComputedStyle::default()),
            canvas: None,
            bleed: 0.0,
            marks: PageMarks::default(),
            orientation: PageOrientation::Upright,
            scale: 1.0,
            area: Rect::new(Au::ZERO, Au::ZERO, px(100.0), px(100.0)),
            shift: 0.0,
            carried: Vec::new(),
        }
    }

    #[test]
    fn shows_moved_boxes_on_the_pages_they_reach() {
        // On the second page, a box moved 30px up by its relative offset, and one moved 60px down
        // by a transform from 70px down.
        let relative = ComputedStyle {
            position: Position::Relative,
            inset: fagus_style::values::Sides {
                top: LengthPercentageAuto::Length(LengthPercentage::Px(-30.0)),
                ..ComputedStyle::default().inset
            },
            ..ComputedStyle::default()
        };
        let transformed = ComputedStyle {
            transform: Arc::from([TransformFunction::Matrix([1.0, 0.0, 0.0, 1.0, 0.0, 60.0])]),
            ..ComputedStyle::default()
        };
        let root = |children| boxed(ComputedStyle::default(), (0.0, 100.0), children);
        let pages = [
            page(root(Vec::new())),
            page(root(vec![
                boxed(relative, (-30.0, 20.0), Vec::new()),
                boxed(transformed, (70.0, 20.0), Vec::new()),
            ])),
            page(root(Vec::new())),
        ];
        let found = stitches(&pages);
        // The first box shows over the content of the first page, 70px down there.
        assert_eq!(
            found[0],
            [Stitch {
                source: 1,
                path: vec![0],
                shift: (Au::ZERO, px(100.0)),
                after: true,
            }]
        );
        // The second shows under the content of the third, 30px down there.
        assert_eq!(
            found[2],
            [Stitch {
                source: 1,
                path: vec![1],
                shift: (Au::ZERO, px(-100.0)),
                after: false,
            }]
        );
        assert_eq!(found[1], []);
        // A copy of it with the boxes around it goes on the page.
        let (copy, _) = along(&pages[1].fragments[0], &found[2][0].path).expect("copy");
        assert_eq!(copy.children.len(), 1);
        assert_eq!(copy.children[0].rect.y, px(70.0));
    }
}

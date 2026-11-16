//! The content of earlier pages that reaches onto a page (see [`fagus_layout::Carried`]), put
//! beside the page's own content.
//!
//! Chrome paints what the content of a page overflows onto the pages after it (monolithic
//! content taller than the page area) as the fragments of the earlier page that it is: beside
//! the page's own, in the phases of the root element's stacking context. The backgrounds of its
//! boxes go under those of the page's own boxes, and its content (text, images, flex and grid
//! items) over them: an image of a table cell over the background of the table going on below
//! it, below the repeated header's background, the overflow of a grid item over the background
//! of its grid. So the children of the root element's fragments that reach onto a page are put
//! before those of the page's own root fragment, with that fragment of the root (without its
//! children) where it reaches onto the page itself.
//!
//! The painter looks things up by the address of the fragment (the slices of broken boxes, the
//! reading order of the tagged content), so the fragments copied have aliases: the fragments
//! they copy.

use fagus_layout::units::Au;
use fagus_layout::{Fragment, FragmentKind, Page};
use rustc_hash::FxHashMap;

use crate::reach::Extents;

/// The fragments of merged content that stand for others, by address: the fragments of the
/// page or the earlier pages they copy.
pub(crate) type Aliases = FxHashMap<*const Fragment, *const Fragment>;

/// What a fragment of the merged content copies.
pub(crate) struct Shape {
    /// The fragment it copies: of the page's own content, or of an earlier page's.
    source: *const Fragment,
    /// Whether it copies a fragment of an earlier page that reaches onto the page itself (not
    /// only through its descendants).
    reaches: bool,
    /// What its children copy, where they are not those of the fragment it copies.
    children: Option<Vec<Shape>>,
}

impl Shape {
    /// The shape of a fragment of the page's own content, with its children.
    fn own(fragment: &Fragment) -> Self {
        Self {
            source: std::ptr::from_ref(fragment),
            reaches: false,
            children: None,
        }
    }

    /// The shape of a whole copy of `source` (of another page) with its children.
    pub(crate) fn whole(source: *const Fragment) -> Self {
        Self {
            source,
            reaches: true,
            children: None,
        }
    }

    /// The shape of a copy of `source` (of another page) that holds only the copy of `child`.
    pub(crate) fn around(source: *const Fragment, child: Self) -> Self {
        Self {
            source,
            reaches: false,
            children: Some(vec![child]),
        }
    }
}

/// What the content of each page reaches onto the pages after it, for putting it on them (see
/// [`Carrier::merged`]). Content of one page may reach over thousands, each of which shows a
/// part of it: that part is cut from what reaches onto any of them (see [`Reaching`]), made once
/// and kept until the last of them is painted, rather than from all of the page's content.
pub(crate) struct Carrier<'p> {
    pages: &'p [Page],
    /// For each page, the band of the page areas its content reaches onto (in its coordinates)
    /// and the last page it reaches onto.
    reached: Vec<Option<(Au, Au, usize)>>,
    /// What reaches onto later pages, by page, while those are painted.
    reaching: FxHashMap<usize, Option<Reaching>>,
    /// The transformed and relatively positioned boxes of other pages that show on each page
    /// (see `stitched`).
    stitches: Vec<Vec<crate::stitched::Stitch>>,
}

impl<'p> Carrier<'p> {
    pub(crate) fn new(pages: &'p [Page]) -> Self {
        let mut reached: Vec<Option<(Au, Au, usize)>> = vec![None; pages.len()];
        for (index, page) in pages.iter().enumerate() {
            for carried in &page.carried {
                if let Some(reach) = reached.get_mut(carried.page) {
                    let (from, to, _) = reach.get_or_insert((carried.from, carried.to, index));
                    *reach = Some(((*from).min(carried.from), (*to).max(carried.to), index));
                }
            }
        }
        Self {
            pages,
            reached,
            reaching: FxHashMap::default(),
            stitches: crate::stitched::stitches(pages),
        }
    }

    /// [`merged`] for page `index`, painted after the pages before it: what reaches onto pages
    /// painted already is dropped.
    pub(crate) fn merged(&mut self, index: usize) -> Option<(Box<Fragment>, Aliases)> {
        let page = self.pages.get(index)?;
        let (pages, reached, reaching) = (self.pages, &self.reached, &mut self.reaching);
        let stitched: Vec<(Fragment, Shape, (Au, Au), bool)> = self
            .stitches
            .get(index)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(|stitch| {
                let content = pages.get(stitch.source)?.fragments.first()?;
                let (copy, shape) = crate::stitched::along(content, &stitch.path)?;
                Some((copy, shape, stitch.shift, stitch.after))
            })
            .collect();
        let merged = merge(page, stitched, |carried| {
            let reach = reaching
                .entry(carried.page)
                .or_insert_with(|| {
                    let source = pages.get(carried.page)?.fragments.first()?;
                    let (from, to, _) = reached.get(carried.page).copied().flatten()?;
                    Reaching::of(source, (from, to))
                })
                .as_ref()?;
            reaching_into(&reach.content, Au::ZERO, (carried.from, carried.to), Some(reach))
        });
        self.reaching.retain(|&source, _| {
            reached
                .get(source)
                .copied()
                .flatten()
                .is_some_and(|(.., last)| last > index)
        });
        merged
    }
}

/// What of the content of a page reaches onto the pages after it: a copy of the content with
/// only what lies in the band of those page areas, with the extents of its boxes, which each of
/// those pages cuts its part from, and the fragments of the page that its fragments copy.
pub(crate) struct Reaching {
    /// The copy, boxed to keep the addresses `extents` and `sources` know its fragments by.
    content: Box<Fragment>,
    extents: Extents,
    sources: Aliases,
}

impl Reaching {
    /// What of `content` (a page's content) lies between `from` and `to`; `None` where nothing
    /// does.
    fn of(content: &Fragment, band: (Au, Au)) -> Option<Self> {
        let (copy, shape) = reaching_into(content, Au::ZERO, band, None)?;
        let content = Box::new(copy);
        let mut sources = Aliases::default();
        alias(&content, &shape, &mut sources);
        Some(Self {
            extents: Extents::of(&content),
            content,
            sources,
        })
    }
}

/// The content of `page` with what the content of the earlier `pages` reaches onto it put in
/// its place, and the aliases of its fragments; `None` where nothing reaches onto it.
#[cfg(test)]
pub(crate) fn merged(page: &Page, pages: &[Page]) -> Option<(Box<Fragment>, Aliases)> {
    merge(page, Vec::new(), |carried| {
        let source = pages.get(carried.page)?.fragments.first()?;
        reaching_into(source, Au::ZERO, (carried.from, carried.to), None)
    })
}

/// The content of `page` with the parts that `reach` cuts for what reaches onto it put in its
/// place (see [`merged`]), and the boxes of other pages `stitched` there (copies with their
/// shapes, how far they move onto the page, and whether they go over its own content).
fn merge(
    page: &Page,
    stitched: Vec<(Fragment, Shape, (Au, Au), bool)>,
    mut reach: impl FnMut(&fagus_layout::Carried) -> Option<(Fragment, Shape)>,
) -> Option<(Box<Fragment>, Aliases)> {
    let own = page.fragments.first()?;
    if page.carried.is_empty() && stitched.is_empty() {
        return None;
    }
    let mut content = own.clone();
    let mut shape = Shape::own(own);
    let shapes = shape
        .children
        .get_or_insert_with(|| own.children.iter().map(Shape::own).collect());
    let here = (own.rect.x, own.rect.y);
    let mut before = 0;
    for carried in &page.carried {
        let Some((mut copy, copy_shape)) = reach(carried) else {
            continue;
        };
        copy.rect.x += carried.shift.0;
        copy.rect.y += carried.shift.1;
        let at = (copy.rect.x, copy.rect.y);
        put(&mut content, shapes, here, (copy, copy_shape, at), &mut before);
    }
    for (mut copy, copy_shape, shift, after) in stitched {
        copy.rect.x += shift.0;
        copy.rect.y += shift.1;
        let at = (copy.rect.x, copy.rect.y);
        if after {
            let mut end = content.children.len();
            put(&mut content, shapes, here, (copy, copy_shape, at), &mut end);
        } else {
            put(&mut content, shapes, here, (copy, copy_shape, at), &mut before);
        }
    }
    // The fragments are where they stay: their addresses are known.
    let content = Box::new(content);
    let mut aliases = Aliases::default();
    alias(&content, &shape, &mut aliases);
    Some((content, aliases))
}

/// A copy of `fragment` (lying `origin` below the top of its page) with only its descendants that
/// lie between `from` and `to`, or `None` if none does. Where `fragment` is part of what reaches
/// onto later pages (`reach`), the subtrees that do not reach that far are left alone and the
/// copies copy the fragments of the page that it copies.
fn reaching_into(
    fragment: &Fragment,
    origin: Au,
    (from, to): (Au, Au),
    reach: Option<&Reaching>,
) -> Option<(Fragment, Shape)> {
    if reach.is_some_and(|reach| !reach.extents.may_reach(fragment, origin, (from, to))) {
        return None;
    }
    let top = origin + fragment.rect.y;
    let (children, shapes): (Vec<Fragment>, Vec<Shape>) = fragment
        .children
        .iter()
        .filter_map(|child| reaching_into(child, top, (from, to), reach))
        .unzip();
    let reaches = top < to && top + fragment.rect.height > from;
    let own = std::ptr::from_ref(fragment);
    let source = reach.map_or(own, |reach| reach.sources.get(&own).copied().unwrap_or(own));
    (reaches || !children.is_empty()).then(|| {
        (
            Fragment {
                rect: fragment.rect,
                kind: fragment.kind.clone(),
                children: children.into(),
            },
            Shape {
                source,
                reaches,
                children: Some(shapes),
            },
        )
    })
}

/// Puts `copy` (of an earlier page, with `copy_shape`), which lies at `at` on the page, among
/// the children of `content` (with `shapes`), the page's own content, which lies at `here`:
/// after the `before` children copied there from earlier pages, before its own. Where both are
/// fragments of the root element (that neither clips its content nor paints it in a space of
/// its own), the copy's children go there, and the copy itself without them first where it
/// reaches onto the page itself; otherwise the copy goes there whole.
fn put(
    content: &mut Fragment,
    shapes: &mut Vec<Shape>,
    here: (Au, Au),
    (copy, copy_shape, at): (Fragment, Shape, (Au, Au)),
    before: &mut usize,
) {
    let mergeable = mergeable(content, &copy);
    let mut place = |child: Fragment, shape: Shape, child_at: (Au, Au)| {
        let mut child = child;
        child.rect.x = child_at.0 - here.0;
        child.rect.y = child_at.1 - here.1;
        content.children.insert(*before, child);
        shapes.insert(*before, shape);
        *before += 1;
    };
    if !mergeable {
        place(copy, copy_shape, at);
        return;
    }
    if copy_shape.reaches {
        let (alone, alone_shape) = without_children(&copy, &copy_shape);
        place(alone, alone_shape, at);
    }
    let copy_shapes = copy_shape.children.unwrap_or_default();
    for (child, child_shape) in copy.children.into_iter().zip(copy_shapes) {
        let child_at = (at.0 + child.rect.x, at.1 + child.rect.y);
        place(child, child_shape, child_at);
    }
}

/// A copy of `copy` (with `shape`) without its children.
fn without_children(copy: &Fragment, shape: &Shape) -> (Fragment, Shape) {
    (
        Fragment {
            rect: copy.rect,
            kind: copy.kind.clone(),
            children: fagus_layout::Children::new(),
        },
        Shape {
            source: shape.source,
            reaches: true,
            children: Some(Vec::new()),
        },
    )
}

/// Whether the children of `copy` may go among those of `fragment`: both are fragments of one
/// element's box, which neither clips them nor paints them in a space or shape of its own.
fn mergeable(fragment: &Fragment, copy: &Fragment) -> bool {
    let same = match (&fragment.kind, &copy.kind) {
        (FragmentKind::Box { node: Some(a), .. }, FragmentKind::Box { node: Some(b), .. }) => a == b,
        _ => false,
    };
    same && takes_children(fragment) && takes_children(copy)
}

/// Whether the descendants of other fragments of the box of `fragment` may go into it: it
/// neither clips them nor paints them in a space or shape of its own.
fn takes_children(fragment: &Fragment) -> bool {
    use fagus_style::values::Overflow;
    let FragmentKind::Box { style, .. } = &fragment.kind else {
        return false;
    };
    style.overflow_x == Overflow::Visible
        && style.overflow_y == Overflow::Visible
        && !style.has_transform()
        && style.perspective.is_none()
        && style.clip_path.is_none()
        && style.mask.is_none()
        && style.filter.is_empty()
}

/// Notes in `aliases` what the fragments of `fragment` (with `shape`) copy.
fn alias(fragment: &Fragment, shape: &Shape, aliases: &mut Aliases) {
    aliases.insert(std::ptr::from_ref(fragment), shape.source);
    if let Some(shapes) = &shape.children {
        for (child, shape) in fragment.children.iter().zip(shapes) {
            alias(child, shape, aliases);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use fagus_dom::{Document, NodeId};
    use fagus_layout::units::Rect;
    use fagus_layout::{Carried, PageMarks, PageOrientation, VisibleSides};
    use fagus_style::ComputedStyle;

    use super::*;

    fn px(value: f32) -> Au {
        Au::from_px(value)
    }

    fn boxed(node: NodeId, (y, height): (f32, f32), children: Vec<Fragment>) -> Fragment {
        Fragment {
            rect: Rect::new(Au::ZERO, px(y), px(100.0), px(height)),
            kind: FragmentKind::Box {
                style: Arc::new(ComputedStyle::default()),
                node: Some(node),
                sides: VisibleSides::ALL,
            },
            children: children.into(),
        }
    }

    fn page(mut content: Fragment, carried: Vec<Carried>) -> Page {
        content.rect.x = px(10.0);
        content.rect.y = px(10.0);
        Page {
            width: px(120.0),
            height: px(100.0),
            fragments: vec![content],
            margin: [px(10.0); 4],
            style: Arc::new(ComputedStyle::default()),
            canvas: None,
            bleed: 0.0,
            marks: PageMarks::default(),
            orientation: PageOrientation::Upright,
            scale: 1.0,
            area: Rect::new(px(10.0), px(10.0), px(100.0), px(80.0)),
            shift: 0.0,
            carried,
        }
    }

    #[test]
    fn puts_what_reaches_onto_a_page_before_its_own_content() {
        let document = Document::parse_html("<html><body><p>a</p><div>m</div><p>b</p></body></html>");
        let nodes: Vec<NodeId> = document.descendants(NodeId::DOCUMENT).collect();
        let node = |name: &str, nth: usize| {
            nodes
                .iter()
                .copied()
                .filter(|&node| document.element(node).is_some_and(|data| data.is(name)))
                .nth(nth)
                .expect("element")
        };
        let (html, body) = (node("html", 0), node("body", 0));
        // On the first page, `m` reaches 40 pixels below the end of the page area (80 pixels
        // tall), and the root and body with it; `a` does not.
        let first = page(
            boxed(
                html,
                (0.0, 120.0),
                vec![boxed(
                    body,
                    (0.0, 120.0),
                    vec![
                        boxed(node("p", 0), (0.0, 30.0), Vec::new()),
                        boxed(node("div", 0), (30.0, 90.0), Vec::new()),
                    ],
                )],
            ),
            Vec::new(),
        );
        let second = page(
            boxed(
                html,
                (0.0, 40.0),
                vec![boxed(
                    body,
                    (0.0, 40.0),
                    vec![boxed(node("p", 1), (0.0, 40.0), Vec::new())],
                )],
            ),
            vec![Carried {
                page: 0,
                shift: (Au::ZERO, px(-80.0)),
                from: px(90.0),
                to: px(170.0),
            }],
        );
        let pages = [first, second];
        assert!(merged(&pages[0], &pages).is_none());
        let (content, aliases) = merged(&pages[1], &pages).expect("merged");
        // The root's fragment of the first page without its children, then its body with `m`
        // 80 pixels higher, then the page's own body.
        let kinds: Vec<(Option<NodeId>, Au, usize)> = content
            .children
            .iter()
            .map(|child| match &child.kind {
                FragmentKind::Box { node, .. } => (*node, child.rect.y, child.children.len()),
                _ => (None, child.rect.y, child.children.len()),
            })
            .collect();
        assert_eq!(
            kinds,
            [
                (Some(html), px(-80.0), 0),
                (Some(body), px(-80.0), 1),
                (Some(body), Au::ZERO, 1)
            ]
        );
        // Each fragment stands for the one it copies.
        let source = |fragment: &Fragment| aliases.get(&std::ptr::from_ref(fragment)).copied();
        let first_root = &pages[0].fragments[0];
        let own_root = &pages[1].fragments[0];
        assert_eq!(source(&content), Some(std::ptr::from_ref(own_root)));
        assert_eq!(source(&content.children[0]), Some(std::ptr::from_ref(first_root)));
        assert_eq!(
            source(&content.children[1].children[0]),
            Some(std::ptr::from_ref(&first_root.children[0].children[1]))
        );
        assert_eq!(
            source(&content.children[2]),
            Some(std::ptr::from_ref(&own_root.children[0]))
        );
    }
}

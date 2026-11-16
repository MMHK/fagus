//! Fragments: the result of layout, positioned boxes on a page.

use std::fmt;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use fagus_dom::NodeId;
use fagus_style::ComputedStyle;
use fagus_text::FaceId;

use crate::image::Image;
use crate::tree::BoxId;
use crate::units::{Au, Rect};

/// A positioned box. `rect` is the border box, relative to the parent fragment's border box.
#[derive(Clone, Debug)]
pub struct Fragment {
    pub rect: Rect,
    pub kind: FragmentKind,
    pub children: Children,
}

/// The children of a fragment, shared between its copies: copying a fragment copies a pointer to
/// them, so that the cached layout of a table, whose cells hold the tables nested in them, does
/// not copy those again at every level of nesting. Changing the children of a copy (through
/// `DerefMut`) copies them first where they are shared, one level deep. Fragments without
/// children hold nothing.
#[derive(Clone, Default)]
pub struct Children(Option<Arc<Vec<Fragment>>>);

/// The children of fragments without any.
static NO_CHILDREN: Vec<Fragment> = Vec::new();

impl Children {
    /// No children.
    pub const fn new() -> Self {
        Self(None)
    }

    /// Whether `self` and `other` are one list, shared, rather than lists alike.
    #[cfg(test)]
    pub(crate) fn shared_with(&self, other: &Self) -> bool {
        matches!((&self.0, &other.0), (Some(own), Some(other)) if Arc::ptr_eq(own, other))
    }

    /// The children, taken out of the shared list (copied where it is shared).
    pub fn into_vec(self) -> Vec<Fragment> {
        self.0.map(Arc::unwrap_or_clone).unwrap_or_default()
    }

    /// The children to change, without making a list for a fragment that has none.
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, Fragment> {
        match &mut self.0 {
            Some(children) => Arc::make_mut(children).iter_mut(),
            None => [].iter_mut(),
        }
    }

    /// The last child, to change.
    pub fn last_mut(&mut self) -> Option<&mut Fragment> {
        self.0
            .as_mut()
            .and_then(|children| Arc::make_mut(children).last_mut())
    }

    /// The first child, to change.
    pub fn first_mut(&mut self) -> Option<&mut Fragment> {
        self.0
            .as_mut()
            .and_then(|children| Arc::make_mut(children).first_mut())
    }
}

impl Deref for Children {
    type Target = Vec<Fragment>;

    fn deref(&self) -> &Vec<Fragment> {
        self.0.as_deref().unwrap_or(&NO_CHILDREN)
    }
}

impl DerefMut for Children {
    fn deref_mut(&mut self) -> &mut Vec<Fragment> {
        Arc::make_mut(self.0.get_or_insert_with(Arc::default))
    }
}

impl fmt::Debug for Children {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, formatter)
    }
}

impl From<Vec<Fragment>> for Children {
    fn from(children: Vec<Fragment>) -> Self {
        Self((!children.is_empty()).then(|| Arc::new(children)))
    }
}

impl FromIterator<Fragment> for Children {
    fn from_iter<I: IntoIterator<Item = Fragment>>(iter: I) -> Self {
        Vec::from_iter(iter).into()
    }
}

impl Extend<Fragment> for Children {
    fn extend<I: IntoIterator<Item = Fragment>>(&mut self, iter: I) {
        let mut iter = iter.into_iter().peekable();
        if iter.peek().is_some() {
            (**self).extend(iter);
        }
    }
}

impl IntoIterator for Children {
    type Item = Fragment;
    type IntoIter = std::vec::IntoIter<Fragment>;

    fn into_iter(self) -> Self::IntoIter {
        self.into_vec().into_iter()
    }
}

impl<'a> IntoIterator for &'a Children {
    type Item = &'a Fragment;
    type IntoIter = std::slice::Iter<'a, Fragment>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a> IntoIterator for &'a mut Children {
    type Item = &'a mut Fragment;
    type IntoIter = std::slice::IterMut<'a, Fragment>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

#[derive(Clone, Debug)]
pub enum FragmentKind {
    /// A box with background and borders. `sides` tells which borders belong to this fragment
    /// (a box broken across pages has no bottom border on the first and no top border on the
    /// next page; an inline box broken across lines has no right border on the first line).
    Box {
        style: Arc<ComputedStyle>,
        node: Option<NodeId>,
        sides: VisibleSides,
    },
    /// A line box; children are text runs and atomic inlines.
    Line { baseline: Au },
    /// Glyphs of one face, size and style. `rect.y` is the top of the line box; the glyphs sit on
    /// `baseline` relative to the fragment.
    Text(TextRun),
    /// A container without own painting (anonymous boxes).
    Anonymous,
    /// The static position of an absolutely positioned box, replaced by the box itself once its
    /// containing block is laid out. Never left in finished pages.
    Placeholder(BoxId),
    /// The content of a replaced element: an image drawn into the fragment's rectangle.
    Image(Arc<Image>),
}

/// Which borders of a broken box are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools, reason = "one flag per side")]
pub struct VisibleSides {
    pub top: bool,
    pub right: bool,
    pub bottom: bool,
    pub left: bool,
}

impl VisibleSides {
    pub const ALL: Self = Self {
        top: true,
        right: true,
        bottom: true,
        left: true,
    };

    pub fn all(self) -> bool {
        self.top && self.right && self.bottom && self.left
    }
}

/// A run of shaped glyphs.
#[derive(Clone, Debug)]
pub struct TextRun {
    pub face: FaceId,
    /// Font size in CSS pixels.
    pub size: f32,
    pub style: Arc<ComputedStyle>,
    pub glyphs: Vec<Glyph>,
    /// The text the glyphs represent, for copy and search.
    pub text: String,
    pub baseline: Au,
    /// Whether the run is (part of) a list item's marker outside its box.
    pub marker: bool,
    /// The symbol of a list marker that is drawn as a shape in place of the glyphs.
    pub symbol: Option<MarkerSymbol>,
    /// How the glyphs stand: in a vertical line, `rect` is the run's box on the page and
    /// `baseline` the distance of the glyphs' baseline from its over side (the right one, the
    /// left one in `sideways-lr`).
    pub orientation: GlyphOrientation,
    /// Whether the glyphs are drawn exactly where they are laid out, not moved with the text of
    /// their line to a whole pixel: the stretched and large operators and radical signs of
    /// `MathML`, which Chrome paints apart from text (Blink's `MathMLPainter`).
    pub exact: bool,
}

/// The symbol of a list marker (`disc`, `circle`, `square`, `disclosure-open`,
/// `disclosure-closed`), which Chrome draws as a shape and not as a glyph (Blink's
/// `ListMarker::RelativeSymbolMarkerRect`): its square lies `x` along the line from the start of
/// the run's box and `y` below its over side, in the coordinates of its line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MarkerSymbol {
    pub shape: SymbolShape,
    pub x: f32,
    pub y: f32,
    pub size: f32,
}

/// The shape of a list marker's symbol.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolShape {
    /// A filled circle.
    Disc,
    /// A circle stroked a pixel wide.
    Circle,
    /// A filled square.
    Square,
    /// A triangle that points to the end of the block (`disclosure-open`) or of the line
    /// (`disclosure-closed`).
    Disclosure { open: bool },
}

/// How the glyphs of a run stand on the page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GlyphOrientation {
    /// In a horizontal line.
    #[default]
    Horizontal,
    /// In a vertical line, set sideways: turned a quarter clockwise, the tops of the glyphs to
    /// the right, on their alphabetic baseline.
    Sideways,
    /// In a `sideways-lr` line: turned a quarter counterclockwise.
    SidewaysLeft,
    /// In a vertical line, upright one below the other: their advances run down the line,
    /// their `x_offset` moves them down and their `y_offset` to the right of the baseline.
    Upright,
    /// Text combined upright in a vertical line (`text-combine-upright`): set horizontally in
    /// the middle of its box, which is 1em long; `baseline` lies below the box's top, and
    /// glyphs wider together than [`crate::COMBINED_ROOM`] ems are squeezed to that.
    Combined,
}

/// A glyph with its advance and offsets in CSS pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    pub id: u32,
    pub advance: f32,
    pub x_offset: f32,
    pub y_offset: f32,
    /// Byte range of the glyph's cluster in [`TextRun::text`].
    pub cluster: (u32, u32),
}

/// A laid-out page.
#[derive(Clone, Debug)]
pub struct Page {
    pub width: Au,
    pub height: Au,
    /// The fragments of the page: the content area first, then the margin boxes.
    pub fragments: Vec<Fragment>,
    /// The page margins, around the page box: top, right, bottom, left.
    pub margin: [Au; 4],
    /// The style of the page box, whose background covers the page and its bleed.
    pub style: Arc<ComputedStyle>,
    /// The background of the root element, or of the body, which covers the canvas: the page
    /// box's padding box (CSS Backgrounds 3, §2.11.2). The element draws none of its own.
    pub canvas: Option<Arc<ComputedStyle>>,
    /// How far the page's content may reach past its edges, to be trimmed off (`bleed`), in
    /// pixels.
    pub bleed: f32,
    /// The printer's marks drawn around the page (`marks`).
    pub marks: PageMarks,
    /// How the page is turned on its sheet (`page-orientation`).
    pub orientation: PageOrientation,
    /// How much the content is shrunk to fit the page area: 1, or less where it was laid out
    /// on a page area as much larger, as Chrome shrinks content wider than the page.
    pub scale: f32,
    /// The page area of the page box, on the page, which the content is to fit in before it is
    /// shrunk, as tall as it is before it is rounded up to whole pixels: where the content is
    /// cut off.
    pub area: Rect,
    /// How far to the right of where it is laid out the content is drawn, in pixels: a fraction
    /// of a pixel where the root element is right to left (see [`crate::PageBox::content_x`]),
    /// after its boxes are snapped to whole pixels.
    pub shift: f32,
    /// The content of earlier pages that reaches onto this page, which is drawn under its own.
    pub carried: Vec<Carried>,
}

/// Content of an earlier page that reaches past the end of its page area onto a later page: as
/// Chrome lays the page areas of a document end to end, what overflows one page area (monolithic
/// content taller than the page, relatively positioned boxes) shows on the pages after it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Carried {
    /// The index of the page whose content reaches here.
    pub page: usize,
    /// How far that content is moved to its place on this page.
    pub shift: (Au, Au),
    /// Where this page area starts and ends, in the coordinates of that page: only the content
    /// between shows.
    pub from: Au,
    pub to: Au,
}

/// How a page is turned on its sheet (`page-orientation`, CSS Paged Media 3, §7.3): it is laid
/// out upright, and the sheet holds it turned.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PageOrientation {
    #[default]
    Upright,
    /// A quarter turn counterclockwise.
    RotateLeft,
    /// A quarter turn clockwise.
    RotateRight,
}

/// The printer's marks of `@page` (`marks`, CSS Paged Media 3, §7.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct PageMarks {
    /// Lines where the paper is cut.
    pub crop: bool,
    /// Crosses for aligning the sheets of different colors.
    pub cross: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::Au;

    fn leaf(x: i32) -> Fragment {
        Fragment {
            rect: Rect::new(
                Au::from_px(x as f32),
                Au::ZERO,
                Au::from_px(1.0),
                Au::from_px(1.0),
            ),
            kind: FragmentKind::Anonymous,
            children: Children::new(),
        }
    }

    #[test]
    fn copies_share_children_until_changed() {
        let parent = Fragment {
            children: vec![leaf(1), leaf(2)].into(),
            ..leaf(0)
        };
        let mut copy = parent.clone();
        assert!(std::ptr::eq(parent.children.as_ptr(), copy.children.as_ptr()));
        // Changing the copy's children copies them, one level deep.
        copy.children.push(leaf(3));
        copy.children[0].rect.x = Au::from_px(5.0);
        assert_eq!(parent.children.len(), 2);
        assert_eq!(parent.children[0].rect.x, Au::from_px(1.0));
        assert_eq!(copy.children.len(), 3);
        // A fragment without children holds no list, and changing none makes none.
        let mut empty = leaf(4);
        assert_eq!(empty.children.iter_mut().count(), 0);
        assert!(empty.children.0.is_none());
        assert!(Children::from(Vec::new()).0.is_none());
        assert_eq!(copy.children.into_vec().len(), 3);
    }
}

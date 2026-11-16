//! Intrinsic and extrinsic sizing keywords (CSS Box Sizing 3 and 4): `min-content`,
//! `max-content`, `fit-content` and `stretch` in `width`, `min-width` and `max-width`, and the
//! size contributions of boxes with them.
//!
//! The layout algorithms decide a box's width from a [`crate::block::BoxModel`], which knows
//! lengths and percentages only. Where a box's width, minimum or maximum is a keyword,
//! [`Layouter::sized_width`] resolves them to a content-box width that the algorithms pass on as
//! the box's width.

use fagus_style::ComputedStyle;
use fagus_style::values::{BoxSizing, LengthPercentage, LengthPercentageAuto, MaxSize, Overflow, Size};

use crate::block::{BOTTOM, LEFT, Layouter, RIGHT, TOP};
use crate::tree::{BoxId, BoxKind};
use crate::units::Au;

/// A box's horizontal border and padding (`frame`) and margins, with percentages of a
/// containing block `basis` wide and auto margins as zero.
struct Edges {
    frame: Au,
    margins: Au,
}

impl Edges {
    fn of(layouter: &Layouter<'_>, style: &ComputedStyle, basis: Au) -> Self {
        let model = layouter.box_model(style, basis, Some(Au::ZERO));
        let margin = |value: &LengthPercentageAuto| Au::from_px(value.resolve_or(basis.to_px(), 0.0));
        Edges {
            frame: model.border[LEFT] + model.border[RIGHT] + model.padding[LEFT] + model.padding[RIGHT],
            margins: margin(&style.margin.left) + margin(&style.margin.right),
        }
    }
}

/// Whether a box is a scroll container (`overflow` other than `visible` and `clip`), which has
/// no automatic minimum size (CSS Box Sizing 4, §5.1; CSS Flexbox 1, §4.5).
pub(crate) fn is_scroll_container(style: &ComputedStyle) -> bool {
    style.overflow_x == Overflow::Hidden || style.overflow_y == Overflow::Hidden
}

/// Whether a `width` or `min-width` is one of the content-based keywords.
pub(crate) fn is_keyword(size: &Size) -> bool {
    matches!(
        size,
        Size::MinContent | Size::MaxContent | Size::FitContent(_) | Size::Stretch
    )
}

/// Whether a `max-width` is one of the content-based keywords.
fn is_max_keyword(size: &MaxSize) -> bool {
    matches!(
        size,
        MaxSize::MinContent | MaxSize::MaxContent | MaxSize::FitContent(_) | MaxSize::Stretch
    )
}

/// Whether a box's width depends on a sizing keyword in `width`, `min-width` or `max-width`, or
/// on a `calc-size()` there.
pub(crate) fn has_keyword(style: &ComputedStyle) -> bool {
    is_keyword(&style.width)
        || is_keyword(&style.min_width)
        || is_max_keyword(&style.max_width)
        || calc_size(style, 0).is_some()
        || calc_size(style, 2).is_some()
        || calc_size(style, 4).is_some()
}

/// The calculations of a `calc-size()` in a size of a style (0 `width`, 1 `height`, 2
/// `min-width`, 3 `min-height`, 4 `max-width`, 5 `max-height`).
pub(crate) fn calc_size(style: &ComputedStyle, index: usize) -> Option<&[fagus_style::anchor::Tokens]> {
    style.calc_size.as_ref()?.get(index)?.as_deref()
}

/// A content-box size through the calculations of a `calc-size()`: they take the size as the
/// box sizes it (its border box with `box-sizing: border-box`), percentages of `basis`.
pub(crate) fn calculated(style: &ComputedStyle, index: usize, size: Au, frame: Au, basis: Au) -> Au {
    let Some(chain) = calc_size(style, index) else {
        return size;
    };
    let border_box = style.box_sizing == BoxSizing::BorderBox;
    let sized = if border_box { size + frame } else { size };
    match fagus_style::calc_size::evaluate(chain, sized.to_px(), basis.to_px()) {
        Some(px) if border_box => (Au::from_px(px) - frame).clamp_non_negative(),
        Some(px) => Au::from_px(px),
        None => size,
    }
}

/// A content-box size from a size keyword: the box's min-content and max-content widths and
/// the `fill` width its margin box takes in the available space.
fn keyword(min: Au, max: Au, fill: Au, value: Keyword) -> Au {
    match value {
        Keyword::MinContent => min,
        Keyword::MaxContent => max,
        Keyword::FitContent => max.min(fill.max(min)),
        Keyword::Stretch => fill,
    }
}

#[derive(Clone, Copy)]
enum Keyword {
    MinContent,
    MaxContent,
    FitContent,
    Stretch,
}

impl Keyword {
    fn of(size: &Size) -> Option<Self> {
        match size {
            Size::MinContent => Some(Keyword::MinContent),
            Size::MaxContent => Some(Keyword::MaxContent),
            Size::FitContent(_) => Some(Keyword::FitContent),
            Size::Stretch => Some(Keyword::Stretch),
            _ => None,
        }
    }

    fn of_max(size: &MaxSize) -> Option<Self> {
        match size {
            MaxSize::MinContent => Some(Keyword::MinContent),
            MaxSize::MaxContent => Some(Keyword::MaxContent),
            MaxSize::FitContent(_) => Some(Keyword::FitContent),
            MaxSize::Stretch => Some(Keyword::Stretch),
            _ => None,
        }
    }
}

/// What the width of one box is resolved with.
struct Sizing<'l, 'a> {
    layouter: &'l Layouter<'a>,
    id: BoxId,
    style: &'a ComputedStyle,
    frame: Au,
    /// The content-box width that fills the available space.
    fill: Au,
    basis: f32,
    widths: std::cell::OnceCell<(Au, Au)>,
}

impl<'l, 'a> Sizing<'l, 'a> {
    fn new(layouter: &'l Layouter<'a>, id: BoxId, containing_width: Au, available: Au) -> Self {
        let style = &layouter.tree.get(id).style;
        let edges = Edges::of(layouter, style, containing_width);
        Sizing {
            layouter,
            id,
            style,
            frame: edges.frame,
            fill: (available - edges.frame - edges.margins).clamp_non_negative(),
            basis: containing_width.to_px(),
            widths: std::cell::OnceCell::new(),
        }
    }

    /// A length or percentage as a content-box width.
    fn length(&self, length: &LengthPercentage) -> Au {
        let width = Au::from_px(length.resolve(self.basis));
        if self.style.box_sizing == BoxSizing::BorderBox {
            (width - self.frame).clamp_non_negative()
        } else {
            width
        }
    }

    fn keyword(&self, value: Keyword) -> Au {
        let (min, max) = *self
            .widths
            .get_or_init(|| self.layouter.content_box_widths(self.id));
        keyword(min, max, self.fill, value)
    }

    /// A size through the calculations of a `calc-size()` in size `index` (see [`calc_size`]).
    fn calculated(&self, index: usize, size: Au) -> Au {
        calculated(self.style, index, size, self.frame, Au::from_px(self.basis))
    }

    /// The maximum, then the minimum, which wins over it.
    fn clamp(&self, mut width: Au) -> Au {
        match &self.style.max_width {
            MaxSize::Length(max) => width = width.min(self.calculated(4, self.length(max))),
            other => {
                if let Some(value) = Keyword::of_max(other) {
                    width = width.min(self.calculated(4, self.keyword(value)));
                }
            }
        }
        match &self.style.min_width {
            Size::Length(min) => width = width.max(self.calculated(2, self.length(min))),
            other => {
                if let Some(value) = Keyword::of(other) {
                    width = width.max(self.calculated(2, self.keyword(value)));
                }
            }
        }
        width
    }
}

impl Layouter<'_> {
    /// The min-content and max-content widths of a box's content box.
    pub(crate) fn content_box_widths(&self, id: BoxId) -> (Au, Au) {
        let given_height = self.has_given_height(id);
        self.forget_page_widths();
        if !given_height && let Some(cached) = self.content_cache.borrow().get(&id) {
            return *cached;
        }
        let widths = self.content_box_widths_uncached(id);
        if !given_height && !self.ratio_percentage(id) {
            self.content_cache.borrow_mut().insert(id, widths);
        }
        widths
    }

    fn content_box_widths_uncached(&self, id: BoxId) -> (Au, Au) {
        let style = &self.tree.get(id).style;
        let edges = Edges::of(self, style, Au::ZERO);
        let (min, max) = self.content_widths(id);
        let outside = edges.frame + edges.margins;
        (
            (min - outside).clamp_non_negative(),
            (max - outside).clamp_non_negative(),
        )
    }

    /// The content-box width of box `id` where its `width`, `min-width` or `max-width` is a
    /// sizing keyword, in a containing block `containing_width` wide in which its margin box
    /// may take `available`. `shrink` tells whether an auto width shrinks to fit (floats,
    /// inline blocks, absolutely positioned boxes) rather than filling the available space.
    /// `None` leaves the width to the box model: no keyword is involved (or with `shrink`, the
    /// width is not auto either).
    pub(crate) fn sized_width(
        &self,
        id: BoxId,
        containing_width: Au,
        available: Au,
        shrink: bool,
    ) -> Option<Au> {
        let style = &self.tree.get(id).style;
        let auto = matches!(style.width, Size::Auto);
        if !(has_keyword(style) || (shrink && auto)) {
            return None;
        }
        let sizing = Sizing::new(self, id, containing_width, available);
        let width = match &style.width {
            Size::Length(width) => sizing.length(width),
            // A fixed height gives the width through the aspect ratio (CSS Box Sizing 4, §5).
            Size::Auto
                if let (Size::Length(LengthPercentage::Px(height)), Some(ratio)) =
                    (&style.height, style.aspect_ratio)
                    && ratio > 0.0 =>
            {
                let height = Au::from_px(height * ratio);
                // With `box-sizing: border-box` the ratio is of the border box, as in the box model.
                if style.box_sizing == BoxSizing::BorderBox {
                    (height - sizing.frame).clamp_non_negative()
                } else {
                    height
                }
            }
            Size::Auto if shrink => sizing.keyword(Keyword::FitContent),
            other => Keyword::of(other).map_or(sizing.fill, |value| sizing.keyword(value)),
        };
        Some(sizing.clamp(sizing.calculated(0, width)))
    }

    /// [`Self::sized_width`] of a box that shrinks to fit (a float or an inline block) in a
    /// containing block of height `containing_height`, if that is definite: where the box's
    /// percentage height then is definite too, the percentage heights of the replaced elements
    /// inside refer to it, as in Chrome.
    pub(crate) fn shrunk_width(
        &self,
        id: BoxId,
        containing_width: Au,
        available: Au,
        containing_height: Option<Au>,
    ) -> Option<Au> {
        self.sized_width_in(id, (containing_width, available), true, containing_height)
    }

    /// [`Self::sized_width`] in a containing block of height `containing_height`, if that is
    /// definite: where the box's percentage height then is definite too, the percentage heights
    /// of the replaced elements inside refer to it, as in Chrome (also for a sizing keyword,
    /// `width: max-content` around an image of `height: 100%`).
    pub(crate) fn sized_width_in(
        &self,
        id: BoxId,
        (containing_width, available): (Au, Au),
        shrink: bool,
        containing_height: Option<Au>,
    ) -> Option<Au> {
        let style = &self.tree.get(id).style;
        let percentage = matches!(&style.height, Size::Length(length) if length.fixed().is_none());
        let fits = matches!(style.width, Size::Auto) && shrink || has_keyword(style);
        // In quirks mode a box of an automatic height passes the height of its containing
        // block on (see `Layouter::passes_percentages_on`).
        if fits
            && matches!(style.height, Size::Auto)
            && self.passes_percentages_on(style)
            && let Some(containing_height) = containing_height
        {
            let outer = self
                .height_for_widths
                .replace(Some((id, Some(containing_height))));
            let width = self.sized_width(id, containing_width, available, shrink);
            self.height_for_widths.set(outer);
            return width;
        }
        if percentage
            && fits
            && let Some(containing_height) = containing_height
        {
            let model = self.box_model(style, containing_width, Some(Au::ZERO));
            let frame_y =
                model.border[TOP] + model.border[BOTTOM] + model.padding[TOP] + model.padding[BOTTOM];
            if let Some(height) = crate::ratio::definite_height(style, frame_y, Some(containing_height)) {
                let outer = self
                    .height_for_widths
                    .replace(Some((id, Some((height - frame_y).clamp_non_negative()))));
                let width = self.sized_width(id, containing_width, available, shrink);
                self.height_for_widths.set(outer);
                return width;
            }
        }
        self.sized_width(id, containing_width, available, shrink)
    }

    /// The content-box width a sizing keyword in `width` asks for (see [`Self::sized_width`]),
    /// `None` for other widths.
    pub(crate) fn keyword_width(&self, id: BoxId, containing_width: Au, available: Au) -> Option<Au> {
        let value = Keyword::of(&self.tree.get(id).style.width)?;
        Some(Sizing::new(self, id, containing_width, available).keyword(value))
    }

    /// A content-box `width` clamped by the box's `min-width` and `max-width`, lengths and
    /// keywords (see [`Self::sized_width`]).
    pub(crate) fn clamped_width(&self, id: BoxId, width: Au, containing_width: Au, available: Au) -> Au {
        Sizing::new(self, id, containing_width, available).clamp(width)
    }

    /// The minimum contribution of a grid item in a container `basis` wide (CSS Grid 2, §6.6):
    /// its margin box at its `min-width`, where `min-width: auto` is its min-content
    /// contribution `min_content`, or nothing for a scroll container or for an item of `width:
    /// auto` that spans several columns, one of them flexible (`spans_flexible`).
    pub(crate) fn minimum_contribution(
        &self,
        id: BoxId,
        basis: Au,
        min_content: Au,
        spans_flexible: bool,
    ) -> Au {
        let style = &self.tree.get(id).style;
        let sizing = Sizing::new(self, id, basis, basis);
        let edges = Edges::of(self, style, basis);
        let outside = edges.frame + edges.margins;
        match &style.min_width {
            Size::Length(length) => sizing.length(length) + outside,
            Size::Auto if is_scroll_container(style) => outside,
            Size::Auto if spans_flexible && matches!(style.width, Size::Auto) => outside,
            Size::Auto => min_content,
            other => Keyword::of(other).map_or(outside, |value| sizing.keyword(value) + outside),
        }
    }

    /// The min-content and max-content contributions of a box whose `width`, `min-width` or
    /// `max-width` is a sizing keyword, or whose `min-width` or `max-width` is a fixed length:
    /// its border box plus margins, with its width clamped by its minimum and maximum (CSS Box
    /// Sizing 3, §5.2). `None` for other boxes (and replaced elements and tables, which have their
    /// own), whose contributions are their content's or their fixed width.
    pub(crate) fn keyword_contributions(&self, id: BoxId) -> Option<(Au, Au)> {
        let layout_box = self.tree.get(id);
        let style = &layout_box.style;
        let fixed_bound = matches!(&style.max_width, MaxSize::Length(length) if length.fixed().is_some())
            || matches!(&style.min_width, Size::Length(length) if length.fixed().is_some_and(|px| px > 0.0));
        if matches!(layout_box.kind, BoxKind::Replaced { .. } | BoxKind::Table { .. })
            || !(has_keyword(style) || fixed_bound)
        {
            return None;
        }
        let edges = Edges::of(self, style, Au::ZERO);
        let fixed = |length: &LengthPercentage| {
            length.fixed().map(|px| {
                let width = Au::from_px(px);
                if style.box_sizing == BoxSizing::BorderBox {
                    (width - edges.frame).clamp_non_negative()
                } else {
                    width
                }
            })
        };
        let (min, max) = self.content_box_widths(id);
        // The preferred width: content-based for auto, `fit-content` and `stretch`.
        let (mut low, mut high) = match &style.width {
            Size::Length(width) if let Some(width) = fixed(width) => (width, width),
            Size::MinContent => (min, min),
            Size::MaxContent => (max, max),
            _ => (min, max),
        };
        // A `calc-size()` width calculates with the size its basis gives each contribution
        // (CSS Values 5, §8.1), its percentages of nothing.
        if calc_size(style, 0).is_some() {
            low = calculated(style, 0, low, edges.frame, Au::ZERO);
            high = calculated(style, 0, high, edges.frame, Au::ZERO);
        }
        // A bound for each contribution; `stretch` depends on the available space, which is
        // what the contributions are found for, and bounds nothing.
        let bounds = |value: Keyword| match value {
            Keyword::MinContent => Some((min, min)),
            Keyword::MaxContent => Some((max, max)),
            Keyword::FitContent => Some((min, max)),
            Keyword::Stretch => None,
        };
        let max_bound = match &style.max_width {
            MaxSize::Length(length) => fixed(length).map(|max| (max, max)),
            other => Keyword::of_max(other).and_then(bounds),
        };
        if let Some((for_low, for_high)) = max_bound {
            low = low.min(for_low);
            high = high.min(for_high);
        }
        let min_bound = match &style.min_width {
            Size::Length(length) => fixed(length).map(|min| (min, min)),
            other => Keyword::of(other).and_then(bounds),
        };
        if let Some((for_low, for_high)) = min_bound {
            low = low.max(for_low);
            high = high.max(for_high);
        }
        let outside = edges.frame + edges.margins;
        Some((low + outside, high + outside))
    }
}

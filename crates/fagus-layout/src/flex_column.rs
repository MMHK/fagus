//! Column flex containers across pages, as Blink breaks them
//! (`FlexLayoutAlgorithm::GiveItemsFinalPositionAndSizeForFragmentation` in Chromium,
//! BSD-licensed; see `CHROMIUM-LICENSE`).
//!
//! The items keep the places the unbroken layout gives them (after growing and
//! `justify-content`), counted from the top of the container as if it were not broken, and
//! break like blocks where they reach past the end of a page. The container's height goes on
//! over the pages; its items do not stretch it. An item that moves to the next page, as a
//! whole or in part, shifts the items below it in its flex line down with it: one that moves
//! whole starts at the top of the next page if it would start above it, and the row gap between
//! it and the item before it is not carried over to the next page (the space that
//! `justify-content` or auto margins put there is). An item whose content takes more room broken than unbroken
//! shifts the items below it by as much. The flex lines of a column that wraps break each on
//! its own. A container whose height is its content's goes on as long as its items do, and
//! breaks before its last items where its bottom border and padding do not fit below them.
//!
//! Wrapping columns are laid out here too ([`Layouter::wrapped_column`]).

use fagus_style::values::{BreakBetween, FlexDirection, Size};

use crate::block::{BOTTOM, BreakToken, ContainerLine, Ending, Layouter, LineItem, Outcome, Space, TOP};
use crate::fragment::{Fragment, VisibleSides};
use crate::tree::{BoxId, BoxKind};
use crate::units::{Au, Rect};

/// Where a flex line of a column flex container continues on the next page.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ColumnResume {
    /// The item of the line to go on with (its index in the line); the line's item count when
    /// all of its items are done.
    pub(crate) item: usize,
    /// Where that item continues, if it broke inside.
    pub(crate) inner: Option<Box<BreakToken>>,
    /// Whether that item moved to the next page as a whole, where it starts at the top at the
    /// latest.
    pub(crate) pushed: bool,
    /// How far the items of the line from `item` on have moved down from their places in the
    /// unbroken layout.
    pub(crate) shift: Au,
    /// Items before `item` (by index in the line) that ended on a page before while their
    /// content overflows them and goes on at these tokens, at the top of the page.
    pub(crate) overflow: Vec<(usize, BreakToken)>,
}

impl Layouter<'_> {
    /// Lays out item `id` of a column flex container `width` wide (whose height is
    /// `containing_height` if definite), `own` wide if the container sizes it, at its
    /// hypothetical height (see [`Layouter::content_basis_height`]), and whether that is not
    /// its own (it is then laid out again at that height where it breaks, as a grown item).
    pub(crate) fn column_item(
        &self,
        id: BoxId,
        (width, containing_height): (Au, Option<Au>),
        own: Option<Au>,
    ) -> Option<(Outcome, bool)> {
        let outcome = self.item_in(id, (width, containing_height), own, None)?;
        match self.content_basis_height(id, (width, containing_height), &outcome.fragment) {
            Some(height) => Some((
                self.item_in(id, (width, containing_height), own, Some(height))?,
                true,
            )),
            None => Some((outcome, false)),
        }
    }

    /// The hypothetical border-box height of item `id` of a column flex container `width` wide
    /// (whose height is `containing_height` if definite), laid out as `fragment` at its own
    /// height, where its flex basis is its content's height (`content`, `min-content`,
    /// `max-content`, `fit-content`) and that differs from its own height: that of its content,
    /// no less than its minimum (an automatic one is no more than its own height) and no more
    /// than its maximum (CSS Flexbox 1, §9.2).
    pub(crate) fn content_basis_height(
        &self,
        id: BoxId,
        (width, containing_height): (Au, Option<Au>),
        fragment: &Fragment,
    ) -> Option<Au> {
        let layout_box = self.tree.get(id);
        let style = &layout_box.style;
        if !matches!(
            style.flex_basis,
            Size::MinContent | Size::MaxContent | Size::FitContent(_)
        ) || !matches!(style.height, Size::Length(_))
            || !matches!(layout_box.kind, BoxKind::Block { .. })
            || style.is_multicol()
        {
            return None;
        }
        let model = self.box_model(style, width, None);
        let top = model.border[TOP] + model.padding[TOP];
        let frame = model.padding[BOTTOM] + model.border[BOTTOM];
        let content = content_extent(fragment, top, frame);
        let resolve = |length: &fagus_style::values::LengthPercentage| {
            let px = length
                .fixed()
                .or_else(|| containing_height.map(|height| length.resolve(height.to_px())))?;
            let value = Au::from_px(px);
            Some(if style.box_sizing == fagus_style::values::BoxSizing::BorderBox {
                value.max(top + frame)
            } else {
                value + top + frame
            })
        };
        let max = match &style.max_height {
            fagus_style::values::MaxSize::Length(length) => resolve(length),
            _ => None,
        }
        .unwrap_or(Au::MAX);
        let min = match &style.min_height {
            Size::Length(length) => resolve(length).unwrap_or(Au::ZERO),
            Size::Auto if !crate::sizing::is_scroll_container(style) => {
                content.min(fragment.rect.height).min(max)
            }
            _ => Au::ZERO,
        };
        let height = content.min(max).max(min);
        (height != fragment.rect.height).then_some(height)
    }

    /// The border-box height of the content of item `id` of a column flex container `width`
    /// wide, laid out as `fragment`: that of a block container with a height of its own, or
    /// with a minimum or maximum height, is where its content ends (its content size
    /// suggestion, CSS Flexbox 1, §4.5, and the height an automatic flex basis takes, as
    /// Blink's intrinsic block size); that of other items their height.
    pub(crate) fn item_content_height(&self, id: BoxId, width: Au, fragment: &Fragment) -> Au {
        let layout_box = self.tree.get(id);
        let style = &layout_box.style;
        // A replaced element's is the height its width gives through its ratio, else its
        // natural height.
        if let BoxKind::Replaced { image, .. } = &layout_box.kind
            && matches!(style.height, Size::Length(_))
        {
            let model = self.box_model(style, width, None);
            let frame_x = model.border[crate::block::LEFT]
                + model.border[crate::block::RIGHT]
                + model.padding[crate::block::LEFT]
                + model.padding[crate::block::RIGHT];
            let frame_y =
                model.border[TOP] + model.border[BOTTOM] + model.padding[TOP] + model.padding[BOTTOM];
            let content_width = (fragment.rect.width - frame_x).clamp_non_negative().to_px();
            let natural_ratio = image.as_ref().and_then(|image| image.ratio);
            let ratio = if style.aspect_ratio_auto {
                natural_ratio.or(style.aspect_ratio)
            } else {
                style.aspect_ratio
            }
            .filter(|ratio| *ratio > 0.0);
            let natural_height = image
                .as_ref()
                .filter(|image| image.natural.1)
                .map(|image| image.height);
            return match ratio.map(|ratio| content_width / ratio).or(natural_height) {
                Some(height) => Au::from_px(height) + frame_y,
                None => fragment.rect.height,
            };
        }
        // A table's is that of its rows and captions, whatever its own heights.
        if matches!(layout_box.kind, BoxKind::Table { .. })
            && (matches!(style.height, Size::Length(_))
                || matches!(style.min_height, Size::Length(_))
                || matches!(style.max_height, fagus_style::values::MaxSize::Length(_)))
        {
            return self
                .item(id, width, None, Some(Au::ZERO))
                .map_or(fragment.rect.height, |outcome| outcome.fragment.rect.height);
        }
        if !matches!(layout_box.kind, BoxKind::Block { .. } | BoxKind::Inline { .. })
            || style.is_multicol()
            || self.tree.flows.content_of(id).is_some()
        {
            return fragment.rect.height;
        }
        let model = self.box_model(style, width, None);
        let top = model.border[TOP] + model.padding[TOP];
        let frame = model.padding[BOTTOM] + model.border[BOTTOM];
        if !matches!(style.height, Size::Length(_)) {
            return self.unbounded_height(style, fragment, (top, frame));
        }
        let content = content_extent(fragment, top, frame);
        // A box with an aspect ratio is as tall as its width gives, at least.
        match crate::ratio::ratio(style) {
            Some(ratio) => {
                let frame_x = model.border[crate::block::LEFT]
                    + model.border[crate::block::RIGHT]
                    + model.padding[crate::block::LEFT]
                    + model.padding[crate::block::RIGHT];
                let frame_y = top + model.padding[BOTTOM] + model.border[BOTTOM];
                let height = if crate::ratio::ratio_sizing(style) == fagus_style::values::BoxSizing::BorderBox
                {
                    Au::from_px(fragment.rect.width.to_px() / ratio).max(frame_y)
                } else {
                    Au::from_px((fragment.rect.width - frame_x).clamp_non_negative().to_px() / ratio)
                        + frame_y
                };
                content.max(height)
            }
            None => content,
        }
    }

    /// The border-box height of a box with an automatic height styled `style`, laid out as
    /// `fragment` (`frame` being its top and its bottom border and padding): where its content
    /// ends if a fixed `min-height` or `max-height` made it as tall as that, else its height.
    fn unbounded_height(
        &self,
        style: &fagus_style::ComputedStyle,
        fragment: &Fragment,
        frame: (Au, Au),
    ) -> Au {
        let height = fragment.rect.height;
        let border_box = |px: f32| {
            let value = Au::from_px(px);
            if style.box_sizing == fagus_style::values::BoxSizing::BorderBox {
                value.max(frame.0 + frame.1)
            } else {
                value + frame.0 + frame.1
            }
        };
        let min = match &style.min_height {
            Size::Length(length) => length.fixed().map(border_box),
            _ => None,
        };
        let max = match &style.max_height {
            fagus_style::values::MaxSize::Length(length) => length.fixed().map(border_box),
            _ => None,
        };
        if min != Some(height) && max != Some(height) {
            return height;
        }
        let content = content_extent(fragment, frame.0, frame.1);
        if (min == Some(height) && content < height) || (max == Some(height) && content > height) {
            content
        } else {
            height
        }
    }

    /// Whether `id` is a column flex container one of whose items has children of percentage
    /// heights, or is such a column itself: a definite height that stretches it makes the
    /// heights of its items after flexing definite, which those refer to (CSS Flexbox 1, §9.8;
    /// WPT `css-flexbox/percentage-heights-019`, `flex-item-percentage-in-nested-flex-bsize-002`).
    pub(crate) fn column_items_hold_percentages(&self, id: BoxId) -> bool {
        self.column_items_hold_percentages_in(id, 8)
    }

    /// [`Self::column_items_hold_percentages`], also through items that are such columns
    /// themselves, `depth` levels deep at most.
    fn column_items_hold_percentages_in(&self, id: BoxId, depth: usize) -> bool {
        let layout_box = self.tree.get(id);
        let BoxKind::Flex { children } = &layout_box.kind else {
            return false;
        };
        depth > 0
            && matches!(
                layout_box.style.flex_direction,
                FlexDirection::Column | FlexDirection::ColumnReverse
            )
            && children.iter().any(|&child| {
                self.has_percentage_heights(child) || self.column_items_hold_percentages_in(child, depth - 1)
            })
    }

    /// Whether `id` is a column flex container whose height is fixed or has a minimum, which
    /// [`Layouter::flex_column_part`] breaks across pages also where it is stretched.
    pub(crate) fn sized_column(&self, id: BoxId, space: &Space) -> bool {
        let layout_box = self.tree.get(id);
        let style = &layout_box.style;
        if !matches!(layout_box.kind, BoxKind::Flex { .. })
            || !matches!(
                style.flex_direction,
                FlexDirection::Column | FlexDirection::ColumnReverse
            )
        {
            return false;
        }
        let definite = space.containing_height.is_some();
        let height = matches!(&style.height, Size::Length(length) if length.fixed().is_some() || definite);
        let min = matches!(&style.min_height, Size::Length(length)
            if length.fixed().map_or(definite, |min| min > 0.0));
        height || min
    }

    /// Whether item `id` of a column flex container grows with its content where it breaks,
    /// rather than keeping the height the container gives it (and its content overflowing it),
    /// as in Chrome (`FlexLayoutAlgorithm::MinBlockSizeShouldEncompassIntrinsicSize`): if it
    /// cannot shrink and its flex basis is its content's height, or if its height is automatic
    /// and it cannot shrink or has an automatic minimum height. An item can shrink if its
    /// `flex-shrink` is not zero and the container's height is `definite`.
    fn column_item_grows(&self, id: BoxId, definite: bool) -> bool {
        // Not an item whose children's heights are percentages of its own (Blink's
        // `has_descendant_that_depends_on_percentage_block_size`).
        if self.has_percentage_heights(id) {
            return false;
        }
        let style = &self.tree.get(id).style;
        let can_shrink = style.flex_shrink != 0.0 && definite;
        let auto_height = !matches!(style.height, Size::Length(_));
        let content_basis = match &style.flex_basis {
            Size::Length(_) => false,
            Size::Auto => auto_height,
            _ => true,
        };
        let auto_min = matches!(style.min_height, Size::Auto) && !crate::sizing::is_scroll_container(style);
        (content_basis && !can_shrink) || (auto_height && (!can_shrink || auto_min))
    }

    /// Lays out the part of a column flex container that fits on the page, from `token` on (see
    /// the module documentation). `full` is the border-box
    /// height of a container stretched in a flex row, which it has instead.
    pub(crate) fn flex_column_part(
        &self,
        id: BoxId,
        space: Space,
        token: Option<&BreakToken>,
        width_override: Option<Au>,
        full: Option<Au>,
    ) -> Option<Outcome> {
        // A container whose height is its content's breaks before its last items where its
        // bottom border and padding do not fit below them, as a block does; one with a height
        // of its own moves them to the next page on their own.
        if full.is_none() && matches!(self.tree.get(id).style.height, Size::Auto) {
            return self.ending_on_page(space, false, |ending| {
                self.flex_column_part_in(id, space, token, (width_override, full), ending)
            });
        }
        self.flex_column_part_in(id, space, token, (width_override, full), Ending::Alone)?
            .ok()
    }

    /// [`Layouter::flex_column_part`]; `Err` tells where the content ends when the container's
    /// bottom border and padding do not fit below it and `ending` asks for that.
    #[allow(clippy::too_many_lines, reason = "the steps of breaking a column")]
    fn flex_column_part_in(
        &self,
        id: BoxId,
        space: Space,
        token: Option<&BreakToken>,
        (width_override, full): (Option<Au>, Option<Au>),
        ending: Ending,
    ) -> Option<Result<Outcome, Au>> {
        let Some(remaining) = space.remaining else {
            return Some(Ok(self.container_lines(id, space, width_override, full).0));
        };
        let (outcome, lines) = self.whole_container(
            id,
            Space {
                remaining: None,
                ..space
            },
            width_override,
            full,
        );
        let layout_box = self.tree.get(id);
        let style = &layout_box.style;
        // Whether a break between two items is forced (looked at only on the first page, where
        // the container may fit whole).
        let model = self.flow_model(style, &space, width_override);
        // A forced break before the first item of a line breaks the container below its top
        // border and padding where they separate the item from the container's top, as in
        // Chrome: the container's first part is then empty.
        let separated = token.is_none() && model.border[TOP] + model.padding[TOP] > Au::ZERO;
        let forced = || {
            lines.iter().any(|line| {
                separated
                    && line
                        .boxes
                        .first()
                        .is_some_and(|item| self.forces_break(self.tree.break_before(item.id)))
                    || line.boxes.windows(2).any(|pair| {
                        self.forces_break(self.tree.break_after(pair[0].id))
                            || self.forces_break(self.tree.break_before(pair[1].id))
                            || self.page_name_changes(pair[0].id, pair[1].id)
                    })
                    || line.boxes.iter().any(|item| self.item_breaks_inside(item.id))
            })
        };
        // The first item of each flex line passes its `break-before` on to the container, the
        // last its `break-after`, as in Chrome (the lines are a row of columns).
        let passed = self.column_line_breaks(&lines);
        if token.is_none()
            && crate::parallel::flow_extent(&outcome.fragment) + outcome.collapsed_top.resolve() <= remaining
            && !forced()
        {
            self.forget_container(id);
            return Some(Ok(Outcome {
                passed_breaks: passed,
                ..outcome
            }));
        }
        let clones = style.clones_decorations();
        let continuing = token.is_some();
        let y0 = model.border[TOP] + model.padding[TOP];
        let top = if continuing && !clones { Au::ZERO } else { y0 };
        let bottom_frame = model.padding[BOTTOM] + model.border[BOTTOM];
        let limit = remaining - if continuing { Au::ZERO } else { model.margin[TOP] };
        // Where content may end on this page.
        let mut content_limit = if clones { limit - bottom_frame } else { limit };
        if let Ending::Before(end) = ending {
            content_limit = content_limit.min(end);
        }
        if content_limit <= top {
            if !continuing && !space.at_top {
                return None;
            }
            content_limit = top + Au::EPSILON;
        }
        let page_space = content_limit - top;
        // Whether the container's height is definite, which lets its items shrink.
        let definite = full.is_some()
            || matches!(&style.height, Size::Length(length)
                if length.fixed().is_some() || space.containing_height.is_some());
        let earlier = token.map_or(Au::ZERO, BreakToken::consumed);
        // The place in the unbroken container at which this page's part of the content starts,
        // and at which the next page's does if the container breaks on this one.
        let stitched = y0 + earlier;
        let next_stitched = stitched + page_space;
        // The gap between the items of a line, which does not go on to the next page where an
        // item moves there.
        let row_gap = style.row_gap.as_ref().map_or(Au::ZERO, |gap| {
            Au::from_px(gap.resolve(model.content_width.to_px()))
        });
        let resumes: &[ColumnResume] = match token {
            Some(BreakToken::Columns { lines, .. }) => lines,
            _ => &[],
        };
        let Fragment {
            rect: outer,
            children: originals,
            ..
        } = outcome.fragment;
        // The items' fragments stay shared with the container's whole layout, which is kept for
        // its next pages: only those placed on this page are copied.
        let item_count = lines.iter().map(|line| line.items.end).max().unwrap_or(0);
        let (originals, placeholders) = originals.split_at(item_count.min(originals.len()));
        // The placeholders of out-of-flow children stay on the first page.
        let mut children: Vec<Fragment> = if continuing {
            Vec::new()
        } else {
            placeholders.to_vec()
        };
        let mut placed = false;
        let mut pending = false;
        let mut content_end = top;
        let mut next_lines = Vec::with_capacity(lines.len());
        for (number, line) in lines.iter().enumerate() {
            let originals = &originals[line.items.clone()];
            let mut resume = resumes.get(number).cloned().unwrap_or_default();
            match self.column_line_part(
                line,
                originals,
                &mut resume,
                &mut children,
                ColumnPage {
                    top,
                    content_limit,
                    stitched,
                    next_stitched,
                    // The lines are side by side: each starts the page where the container does.
                    at_top: space.at_top,
                    definite,
                    separated,
                    row_gap,
                },
            ) {
                LinePart::Moves if !placed && !continuing && !space.at_top => return None,
                LinePart::Moves => pending = true,
                LinePart::Placed { end, broken } => {
                    placed = true;
                    pending |= broken;
                    content_end = content_end.max(end);
                }
            }
            next_lines.push(resume);
        }
        let content = if pending {
            page_space.max(content_end - top)
        } else {
            content_end - top
        };
        let used = match full {
            Some(full) => full - y0 - bottom_frame - earlier,
            None => {
                Self::used_height(
                    style,
                    &space.stretch_model(&model),
                    earlier + content,
                    space.containing_height,
                ) - earlier
            }
        }
        .clamp_non_negative();
        // Monolithic content of the items that reaches past the end of the page takes the
        // container's height along, as far as it goes (Blink's `FinishFragmentation`).
        let filled = page_space.max(content_end - top);
        // A container whose height is its content's goes on with the items that do.
        let grows = pending && full.is_none() && matches!(style.height, Size::Auto) && used >= filled;
        let (height, consumed) = if used > filled || grows {
            // The container goes on on the next page.
            (limit.max(top + filled), Some(earlier + filled))
        } else if !clones
            && top + used + bottom_frame > limit
            && used > Au::ZERO
            // A container whose height is its content's and that has no bottom border or
            // padding ends with its content, which may overflow the page (monolithic content).
            && (bottom_frame > Au::ZERO || matches!(ending, Ending::Alone))
        {
            if matches!(ending, Ending::Retry) && content_end > top {
                return Some(Err(content_end));
            }
            // The bottom border and padding do not fit below the content: they go on alone.
            (top + used, Some(earlier + used))
        } else {
            // The container ends; items that reach out of it end with it.
            (top + used + bottom_frame, None)
        };
        let broken = consumed.is_some();
        // A container that ends while its items go on (their content overflowing it) has its
        // items continue on the next page without it, beside what follows it, as in Chrome.
        let overflows = !broken && pending && style.overflow_y == fagus_style::values::Overflow::Visible;
        let next_token = if overflows {
            let mut token = BreakToken::Columns {
                lines: next_lines,
                consumed: crate::block::Consumed::default(),
            };
            token.set_consumed(earlier + page_space);
            Some(BreakToken::Overflow {
                inner: Box::new(token),
            })
        } else {
            consumed.map(|consumed| {
                let mut token = BreakToken::Columns {
                    lines: next_lines,
                    consumed: crate::block::Consumed::default(),
                };
                token.set_consumed(consumed);
                token
            })
        };
        Some(Ok(Outcome {
            passed_breaks: (
                if continuing { BreakBetween::Auto } else { passed.0 },
                if broken { BreakBetween::Auto } else { passed.1 },
            ),
            ..Outcome::new(
                Fragment {
                    rect: Rect::new(outer.x, Au::ZERO, outer.width, height),
                    kind: layout_box.fragment_kind(VisibleSides {
                        top: !continuing || clones,
                        bottom: !broken || clones,
                        ..VisibleSides::ALL
                    }),
                    children: children.into(),
                },
                next_token,
                if continuing { Au::ZERO } else { model.margin[TOP] },
                if broken { Au::ZERO } else { model.margin[BOTTOM] },
            )
        }))
    }

    /// Whether a forced break lies inside item `id` wherever it is: between block-level boxes
    /// in it, or in a column flex container, between its items or before its first item below
    /// its top border or padding (see [`Layouter::flex_column_part`]).
    fn item_breaks_inside(&self, id: BoxId) -> bool {
        if self.breaks_inside(id) {
            return true;
        }
        let layout_box = self.tree.get(id);
        let style = &layout_box.style;
        let BoxKind::Flex { children } = &layout_box.kind else {
            return false;
        };
        if !matches!(
            style.flex_direction,
            FlexDirection::Column | FlexDirection::ColumnReverse
        ) || self.measuring.get()
        {
            return false;
        }
        let items: Vec<BoxId> = self.in_flow(children).collect();
        let separated = style.border_width.top > 0.0 || style.padding.top.fixed().is_none_or(|top| top > 0.0);
        (separated
            && items
                .first()
                .is_some_and(|&first| self.forces_break(self.tree.break_before(first))))
            || items.windows(2).any(|pair| {
                self.forces_break(self.tree.break_after(pair[0]))
                    || self.forces_break(self.tree.break_before(pair[1]))
            })
            || items.iter().any(|&item| self.item_breaks_inside(item))
    }

    /// The `break-before` the first items of the flex lines of a column pass on to the
    /// container, and the `break-after` their last items pass on, as in Chrome
    /// (`FlexLayoutAlgorithm::GiveItemsFinalPositionAndSize`).
    fn column_line_breaks(&self, lines: &[ContainerLine]) -> (BreakBetween, BreakBetween) {
        lines.iter().fold(
            (BreakBetween::Auto, BreakBetween::Auto),
            |(before, after), line| {
                (
                    line.boxes.first().map_or(before, |item| {
                        crate::tree::combine_breaks(before, self.tree.break_before(item.id))
                    }),
                    line.boxes.last().map_or(after, |item| {
                        crate::tree::combine_breaks(after, self.tree.break_after(item.id))
                    }),
                )
            },
        )
    }

    /// Whether the break before item `index` of a flex line of a column is avoided: by its
    /// `break-before` or the `break-after` of the item before it.
    fn avoids_break_before(&self, line: &ContainerLine, index: usize) -> bool {
        let Some(item) = line.boxes.get(index) else {
            return false;
        };
        self.avoids_break(self.tree.break_before(item.id))
            || index
                .checked_sub(1)
                .and_then(|before| line.boxes.get(before))
                .is_some_and(|before| self.avoids_break(self.tree.break_after(before.id)))
    }

    /// Lays out the items of a flex line of a column on the page from where `resume` tells,
    /// into `children`, and updates `resume` to where the line goes on on the next page.
    fn column_line_part(
        &self,
        line: &ContainerLine,
        originals: &[Fragment],
        resume: &mut ColumnResume,
        children: &mut Vec<Fragment>,
        page: ColumnPage,
    ) -> LinePart {
        let ColumnPage {
            top,
            content_limit,
            stitched,
            next_stitched,
            at_top,
            definite,
            separated,
            row_gap,
        } = page;
        // The content of items that ended on the pages before and overflows them comes first,
        // at the top, beside the items that follow them.
        let mut overflow = Vec::new();
        for (index, token) in std::mem::take(&mut resume.overflow) {
            let (Some(item), Some(original)) = (line.boxes.get(index), originals.get(index)) else {
                continue;
            };
            if let Some(outcome) = self.item_part(
                item,
                content_limit - top,
                (at_top, true),
                Some(&token),
                (None, None),
            ) {
                let mut fragment = outcome.fragment;
                fragment.rect.x = original.rect.x;
                fragment.rect.y = top;
                children.push(fragment);
                overflow.extend(outcome.token.map(|token| (index, token)));
            }
        }
        resume.overflow = overflow;
        let first = resume.item;
        let mut placed = false;
        // The end of the last item's margin box on the page.
        let mut line_end = top;
        let mut index = first;
        // Where each item placed on this page started: the children before it, the shift of
        // the line and its end above it, and the gap above it (see `Self::back_to_break`).
        let mut starts: Vec<ItemStart> = Vec::new();
        while let (Some(item), Some(original)) = (line.boxes.get(index), originals.get(index)) {
            // The part of the gap above the item in the unbroken layout that is the row gap:
            // where the item moves to the next page, the row gap does not go on there, but the
            // space that `justify-content` puts between the items does (Chrome 157, measured).
            let gap = index
                .checked_sub(1)
                .and_then(|previous| Some((line.boxes.get(previous)?, originals.get(previous)?)))
                .map_or(Au::ZERO, |(previous, fragment)| {
                    (original.rect.y
                        - item.margins.0
                        - (fragment.rect.y + fragment.rect.height + previous.margins.1))
                        .clamp_non_negative()
                        .min(row_gap)
                });
            if (index > first
                && (self.forces_break(self.tree.break_before(item.id))
                    || self.forces_break(self.tree.break_after(line.boxes[index - 1].id))
                    || self.page_name_changes(line.boxes[index - 1].id, item.id)))
                || (index == 0 && separated && self.forces_break(self.tree.break_before(item.id)))
            {
                // The item starts the next page below its top margin, without the gap.
                self.forced_break.set(true);
                *resume = ColumnResume {
                    item: index,
                    inner: None,
                    pushed: false,
                    shift: next_stitched + item.margins.0 - original.rect.y,
                    overflow: std::mem::take(&mut resume.overflow),
                };
                return LinePart::Placed {
                    end: line_end,
                    broken: true,
                };
            }
            starts.push(ItemStart {
                children: children.len(),
                shift: resume.shift,
                line_end,
                gap,
                overflow: resume.overflow.len(),
            });
            let inner = if index == first { resume.inner.take() } else { None };
            if index == first && resume.pushed {
                // An item that moved to this page starts at its top at the latest.
                resume.shift += (stitched - (original.rect.y + resume.shift)).clamp_non_negative();
                resume.pushed = false;
            }
            let y = if inner.is_some() {
                top
            } else {
                original.rect.y + resume.shift - stitched + top
            };
            let item_at_top = at_top && !placed && y <= top;
            let laid = if inner.is_none()
                && y + crate::parallel::flow_extent(original).max(original.rect.height + item.margins.1)
                    <= content_limit
                && !self.item_breaks_inside(item.id)
            {
                Some((original.clone(), None))
            } else if y >= content_limit && !item_at_top {
                None
            } else {
                let margin_top = if inner.is_some() { Au::ZERO } else { item.margins.0 };
                let height = item
                    .stretched
                    .then(|| {
                        self.stretched_rest(
                            item.id,
                            original.rect.height,
                            item.containing_width,
                            inner.as_deref(),
                        )
                    })
                    .flatten();
                self.fixed_item_height
                    .set(height.is_some() && !self.column_item_grows(item.id, definite));
                // An item that avoids breaks inside breaks all the same where it starts the
                // page, there being no better break, as in Chrome.
                let avoided = self
                    .breaks_avoided_items
                    .replace(item_at_top || self.breaks_avoided_items.get());
                // A block item's height is its content's: the bottom margin of its last child,
                // truncated where it breaks, leaves room for a break before its bottom border
                // and padding (see `Layouter::content_sized`).
                self.content_sized.set(
                    height.is_none()
                        && matches!(
                            self.tree.get(item.id).kind,
                            BoxKind::Block { .. } | BoxKind::Inline { .. }
                        ),
                );
                let outcome = self.item_part(
                    item,
                    content_limit - y + margin_top,
                    (item_at_top, true),
                    inner.as_deref(),
                    (height, None),
                );
                self.content_sized.set(false);
                self.fixed_item_height.set(false);
                self.breaks_avoided_items.set(avoided);
                outcome.map(|outcome| (outcome.fragment, outcome.token))
            };
            let Some((mut fragment, token)) = laid else {
                if !placed && index == first && y <= Au::ZERO && inner.is_none() {
                    // Nothing separates the item from the top of the container.
                    resume.item = index;
                    resume.pushed = true;
                    return LinePart::Moves;
                }
                // Where the break before it is avoided, the line breaks before an earlier item
                // where nothing avoids that break, as in Chrome (Blink's early break).
                if let Some(back) = (first + 1..index)
                    .rev()
                    .find(|&back| !self.avoids_break_before(line, back))
                    .filter(|_| self.avoids_break_before(line, index))
                {
                    let start = &starts[back - first];
                    children.truncate(start.children);
                    resume.overflow.truncate(start.overflow);
                    let left = (content_limit - start.line_end).clamp_non_negative();
                    let shift = start.shift - (start.gap - left).clamp_non_negative();
                    *resume = ColumnResume {
                        item: back,
                        inner: None,
                        pushed: true,
                        shift,
                        overflow: std::mem::take(&mut resume.overflow),
                    };
                    return LinePart::Placed {
                        end: start.line_end,
                        broken: true,
                    };
                }
                // The item moves to the next page; the part of the gap above it that does not
                // fit on this page is not carried over.
                if index > 0 {
                    let left = (content_limit - line_end).clamp_non_negative();
                    if gap > left {
                        resume.shift -= gap - left;
                    }
                }
                *resume = ColumnResume {
                    item: index,
                    inner,
                    pushed: true,
                    shift: resume.shift,
                    overflow: std::mem::take(&mut resume.overflow),
                };
                return LinePart::Placed {
                    end: line_end,
                    broken: true,
                };
            };
            fragment.rect.x = original.rect.x;
            fragment.rect.y = y;
            let end = y + fragment.rect.height;
            children.push(fragment);
            placed = true;
            match token {
                // The item ends here, but its content goes on beside the items that follow it.
                Some(rest @ BreakToken::Overflow { .. }) => resume.overflow.push((index, rest)),
                Some(token) => {
                    *resume = ColumnResume {
                        item: index,
                        inner: Some(Box::new(token)),
                        pushed: false,
                        shift: resume.shift,
                        overflow: std::mem::take(&mut resume.overflow),
                    };
                    return LinePart::Placed { end, broken: true };
                }
                None => {}
            }
            // An item that took more room broken than unbroken moves the items below it.
            let expected = original.rect.y + resume.shift + original.rect.height;
            let reached = end - top + stitched;
            if reached > expected {
                resume.shift += reached - expected;
            }
            line_end = end + item.margins.1;
            index += 1;
        }
        resume.item = line.boxes.len();
        LinePart::Placed {
            end: line_end,
            broken: !resume.overflow.is_empty(),
        }
    }
}

/// Where an item of a flex line of a column started on a page, to break before it again.
struct ItemStart {
    /// How many children the container had before it.
    children: usize,
    /// How far the line had moved down from the unbroken layout before it.
    shift: Au,
    /// The end of the margin box of the item before it on the page.
    line_end: Au,
    /// The gap above it in the unbroken layout.
    gap: Au,
    /// How many items before it overflowed into the next page.
    overflow: usize,
}

/// The page a column's flex line is laid out on: where its content starts and may end, the
/// places in the unbroken container at which this page's content and the next page's start,
/// and whether the line starts the page.
#[derive(Clone, Copy)]
struct ColumnPage {
    top: Au,
    content_limit: Au,
    stitched: Au,
    next_stitched: Au,
    at_top: bool,
    definite: bool,
    /// Whether the container's top border or padding separate its first items from the top of
    /// its first page, where a forced break before them is then honoured.
    separated: bool,
    /// The container's row gap.
    row_gap: Au,
}

/// What a flex line of a column placed on a page.
enum LinePart {
    /// Nothing: its first item does not fit, and nothing in the container comes before it.
    Moves,
    /// Items up to `end`; `broken` if the line goes on on the next page.
    Placed { end: Au, broken: bool },
}

/// An item of a wrapping column, laid out at its hypothetical height.
struct ColumnItem {
    id: BoxId,
    /// Its content width if it does not stretch.
    width: Option<Au>,
    fragment: Fragment,
    margins: (Au, Au),
    /// The width of its margin box if it does not stretch, else its fit-content width in the
    /// container's: what it asks of its line.
    cross: Au,
    /// Its sizes along the column.
    length: crate::flex_sizing::FlexLength,
    grown: bool,
}

/// The height of a column flex container that wraps: `definite` if it has one, else its
/// content's between `min` and `max`; its lines break at `definite` or else at `max` (Blink's
/// `FlexLayoutAlgorithm::MainAxisContentExtent`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct ColumnHeight {
    pub(crate) definite: Option<Au>,
    pub(crate) min: Au,
    pub(crate) max: Option<Au>,
}

impl ColumnHeight {
    /// The height the lines of the column break at, if any.
    pub(crate) fn limit(self) -> Option<Au> {
        self.definite.or(self.max)
    }

    /// The column's content height, its tallest line being `content` tall.
    fn resolve(self, content: Au) -> Au {
        self.definite
            .unwrap_or_else(|| content.min(self.max.unwrap_or(Au::MAX)).max(self.min))
    }
}

/// The maximum content height of a box styled `style` (with box model `model`) from its
/// `max-height`, a length or a percentage of `containing_height` where that is definite.
pub(crate) fn max_content_height(
    style: &fagus_style::ComputedStyle,
    model: &crate::block::BoxModel,
    containing_height: Option<Au>,
) -> Option<Au> {
    let fagus_style::values::MaxSize::Length(length) = &style.max_height else {
        return None;
    };
    let px = length
        .fixed()
        .or_else(|| containing_height.map(|height| length.resolve(height.to_px())))?;
    let value = Au::from_px(px);
    Some(if style.box_sizing == fagus_style::values::BoxSizing::BorderBox {
        let frame = model.border[TOP] + model.border[BOTTOM] + model.padding[TOP] + model.padding[BOTTOM];
        (value - frame).clamp_non_negative()
    } else {
        value
    })
}

/// The ranges of the items (by their outer hypothetical heights `outer`) in the flex lines of a
/// column whose lines break at `limit` with `gap` between items: each line takes items while
/// they fit, at least one (CSS Flexbox 1, §9.3 step 5), or the lines are balanced into at least
/// `balance` lines (see [`crate::flex_balance`]).
pub(crate) fn column_lines(
    outer: &[Au],
    limit: Au,
    gap: Au,
    balance: Option<usize>,
) -> Vec<std::ops::Range<usize>> {
    if let Some(count) = balance {
        return crate::flex_balance::balanced_lines(outer, limit, gap, count);
    }
    let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();
    let mut used = Au::ZERO;
    for (index, &size) in outer.iter().enumerate() {
        match ranges.last_mut() {
            Some(range) if used + gap + size <= limit => {
                range.end = index + 1;
                used += gap + size;
            }
            _ => {
                ranges.push(index..index + 1);
                used = size;
            }
        }
    }
    ranges
}

impl Layouter<'_> {
    /// The max-content width of the content of a column flex container that wraps, whose lines
    /// break at `limit` (`definite` where that is its height): its flex lines side by side with
    /// `gap` between them, each as wide as the largest max-content contribution of its items,
    /// which are laid out at those widths for their heights (Blink's
    /// `FlexLayoutAlgorithm::ComputeMinMaxSizeOfMultilineColumnContainer`).
    pub(crate) fn wrapped_column_width(
        &self,
        style: &fagus_style::ComputedStyle,
        order: &[BoxId],
        (limit, definite): (Au, bool),
        (column_gap, row_gap): (Au, Au),
    ) -> Au {
        use crate::block::{LEFT, RIGHT};
        let mut order = order.to_vec();
        order.sort_by_key(|&child| self.tree.get(child).style.order);
        let items: Vec<(BoxId, crate::flex_sizing::FlexLength, Au)> = order
            .iter()
            .filter_map(|&id| {
                let child_style = &self.tree.get(id).style;
                let contribution = self.intrinsic(id).1;
                let model = self.box_model(child_style, contribution, None);
                let (left, right) = self.flex_margins(child_style, contribution);
                let frame_x = model.border[LEFT]
                    + model.border[RIGHT]
                    + model.padding[LEFT]
                    + model.padding[RIGHT]
                    + left
                    + right;
                let own = matches!(child_style.width, Size::Auto)
                    .then(|| (contribution - frame_x).clamp_non_negative());
                let (outcome, _) = self.column_item(id, (contribution, None), own)?;
                let margin_top = self.vertical_margin(child_style, contribution, true);
                let laid_out = margin_top + outcome.fragment.rect.height + outcome.margin_bottom;
                let content = margin_top
                    + self.item_content_height(id, contribution, &outcome.fragment)
                    + outcome.margin_bottom;
                let captions = captions_height(&outcome.fragment);
                let length = self.column_flex_length(
                    id,
                    contribution,
                    definite.then_some(limit),
                    (laid_out, content, captions),
                );
                Some((id, length, contribution))
            })
            .collect();
        let outer: Vec<Au> = items.iter().map(|(_, length, _)| length.hypothetical()).collect();
        let ranges = column_lines(&outer, limit, row_gap, balance(style));
        let count = i32::try_from(ranges.len()).unwrap_or(i32::MAX);
        ranges
            .into_iter()
            .map(|range| {
                let line = &items[range];
                // The items flex to a definite height first: one whose width follows its height
                // through an aspect ratio is as wide as its flexed height makes it (Blink lays
                // the lines out for this).
                let flexed = (definite
                    && line
                        .iter()
                        .any(|&(id, _, _)| self.ratio_width_at(id, Au::ZERO).is_some()))
                .then(|| {
                    let lengths: Vec<crate::flex_sizing::FlexLength> =
                        line.iter().map(|(_, length, _)| *length).collect();
                    let gaps = row_gap * i32::try_from(line.len().saturating_sub(1)).unwrap_or(i32::MAX);
                    crate::flex_sizing::resolve(&lengths, (limit - gaps).clamp_non_negative())
                });
                line.iter()
                    .enumerate()
                    .map(|(index, &(id, length, contribution))| {
                        flexed
                            .as_ref()
                            .and_then(|sizes| sizes.get(index))
                            .filter(|&&size| size != length.hypothetical())
                            .and_then(|&size| self.ratio_width_at(id, size))
                            .unwrap_or(contribution)
                    })
                    .max()
                    .unwrap_or(Au::ZERO)
            })
            .sum::<Au>()
            + column_gap * (count - 1).max(0)
    }

    /// The outer width of flex item `id` of an automatic width whose width follows its height
    /// through an aspect ratio (its own, or a replaced element's), where its margin box is
    /// `outer` tall: `None` for other items.
    fn ratio_width_at(&self, id: BoxId, outer: Au) -> Option<Au> {
        use crate::block::{LEFT, RIGHT};
        let layout_box = self.tree.get(id);
        let style = &layout_box.style;
        if !matches!(style.width, Size::Auto) || style.is_vertical() {
            return None;
        }
        let model = self.box_model(style, Au::ZERO, Some(Au::ZERO));
        let frame_x = model.border[LEFT] + model.border[RIGHT] + model.padding[LEFT] + model.padding[RIGHT];
        let frame_y = model.border[TOP] + model.border[BOTTOM] + model.padding[TOP] + model.padding[BOTTOM];
        let margins_y =
            self.vertical_margin(style, Au::ZERO, true) + self.vertical_margin(style, Au::ZERO, false);
        let (left, right) = self.flex_margins(style, Au::ZERO);
        let height = (outer - margins_y).clamp_non_negative();
        let width = if matches!(layout_box.kind, BoxKind::Replaced { .. }) {
            if !crate::grid_contribution::has_ratio(&layout_box.kind, style) {
                return None;
            }
            let (width, natural_height) = self.replaced_size(id, Au::ZERO, None, None, true);
            if width <= Au::ZERO || natural_height <= Au::ZERO {
                return None;
            }
            let content = (height - frame_y).clamp_non_negative();
            Au::from_px(content.to_px() * width.to_px() / natural_height.to_px()) + frame_x
        } else {
            let ratio = crate::ratio::ratio(style)?;
            let border_box = crate::ratio::width_from_height(style, ratio, (frame_x, frame_y), height);
            self.clamped_width(
                id,
                (border_box - frame_x).clamp_non_negative(),
                Au::ZERO,
                Au::ZERO,
            ) + frame_x
        };
        Some(width + left + right)
    }

    /// The items of a column flex container that wraps, `height` tall (see [`ColumnHeight`]):
    /// flex lines of items whose hypothetical heights fit in the height its lines break at,
    /// side by side (CSS Flexbox 1, §9.3), each as wide as its widest item, with
    /// `align-content` sharing out the rest of the `width` (stretching them by default); the
    /// items of each line flex to the container's height (§9.7), are justified and aligned in
    /// their line. Returns the fragments, the content height and the lines.
    #[allow(clippy::too_many_lines, reason = "the steps of the flex layout algorithm")]
    pub(crate) fn wrapped_column(
        &self,
        style: &fagus_style::ComputedStyle,
        order: &[BoxId],
        width: Au,
        height: ColumnHeight,
        (column_gap, row_gap): (Au, Au),
    ) -> (Vec<Fragment>, Au, Vec<ContainerLine>) {
        use crate::block::{LEFT, RIGHT, distribute, justify};
        use fagus_style::values::{ContentAlignment, FlexWrap};
        // Percentage heights of the items refer to the container's height.
        let containing_height = height.definite;
        // A balanced column of at least `flex-line-count` lines leaves its items the room of one
        // line each across, as in Blink.
        let available = match balance(style) {
            Some(count) if count > 1 => {
                let count = i32::try_from(count).unwrap_or(i32::MAX);
                ((width - column_gap * (count - 1)) / count).clamp_non_negative()
            }
            _ => width,
        };
        let mut items: Vec<ColumnItem> = order
            .iter()
            .filter_map(|&id| {
                let child_style = &self.tree.get(id).style;
                let own = self.cross_size(style, id, width);
                let model = self.box_model(child_style, width, None);
                let (left, right) = self.flex_margins(child_style, width);
                let side_margins = left + right;
                let frame_x =
                    model.border[LEFT] + model.border[RIGHT] + model.padding[LEFT] + model.padding[RIGHT];
                // Items with a width of their own keep it; the others stretch to their line,
                // and until its width is known, they are as wide as their content (fit-content).
                let stretches = own.is_none() && matches!(child_style.width, Size::Auto);
                let fit = stretches.then(|| {
                    let (min, max) = self.intrinsic(id);
                    max.min(available.max(min))
                });
                let basis_width =
                    own.or_else(|| fit.map(|fit| (fit - side_margins - frame_x).clamp_non_negative()));
                let (outcome, grown) = self.column_item(id, (width, containing_height), basis_width)?;
                let cross = fit.unwrap_or(outcome.fragment.rect.width + side_margins);
                let margins = (
                    self.vertical_margin(child_style, width, true),
                    outcome.margin_bottom,
                );
                let laid_out = margins.0 + outcome.fragment.rect.height + margins.1;
                let content = margins.0 + self.item_content_height(id, width, &outcome.fragment) + margins.1;
                let captions = captions_height(&outcome.fragment);
                let length =
                    self.column_flex_length(id, width, containing_height, (laid_out, content, captions));
                Some(ColumnItem {
                    id,
                    width: own.or_else(|| {
                        (!stretches).then(|| (outcome.fragment.rect.width - frame_x).clamp_non_negative())
                    }),
                    margins,
                    fragment: outcome.fragment,
                    cross,
                    length,
                    grown,
                })
            })
            .collect();
        // Items go into lines while they fit in the height the lines break at.
        let outer: Vec<Au> = items.iter().map(|item| item.length.hypothetical()).collect();
        let ranges = column_lines(&outer, height.limit().unwrap_or(Au::MAX), row_gap, balance(style));
        let gaps = |count: usize| row_gap * (i32::try_from(count).unwrap_or(i32::MAX) - 1).max(0);
        let tallest = ranges
            .iter()
            .map(|range| outer[range.clone()].iter().copied().sum::<Au>() + gaps(range.len()))
            .max()
            .unwrap_or(Au::ZERO);
        let main = height.resolve(tallest);
        // In `column-reverse`, the items of each line run from the bottom up.
        if style.flex_direction == FlexDirection::ColumnReverse {
            for range in &ranges {
                items[range.clone()].reverse();
            }
        }
        // Lines as wide as their widest item; `align-content` shares out the rest.
        let mut crosses: Vec<Au> = ranges
            .iter()
            .map(|range| {
                items[range.clone()]
                    .iter()
                    .map(|item| item.cross)
                    .max()
                    .unwrap_or(Au::ZERO)
            })
            .collect();
        let count = i32::try_from(ranges.len()).unwrap_or(i32::MAX);
        let free = (width - crosses.iter().copied().sum::<Au>() - column_gap * (count - 1).max(0))
            .clamp_non_negative();
        let (mut x, between) = match style.align_content {
            ContentAlignment::Normal | ContentAlignment::Stretch => {
                for cross in &mut crosses {
                    *cross += free / count.max(1);
                }
                (Au::ZERO, Au::ZERO)
            }
            alignment => distribute(alignment, free, ranges.len()),
        };
        let rtl = style.is_rtl();
        let mut fragments = Vec::with_capacity(items.len());
        let mut lines = Vec::with_capacity(ranges.len());
        for (range, &cross) in ranges.iter().zip(&crosses) {
            let line_x = if rtl { width - x - cross } else { x };
            x += cross + column_gap + between;
            let first = fragments.len();
            // The items flex to the container's height.
            let lengths: Vec<crate::flex_sizing::FlexLength> =
                items[range.clone()].iter().map(|item| item.length).collect();
            let targets = crate::flex_sizing::resolve(&lengths, main - gaps(range.len()));
            let mut boxes = Vec::with_capacity(range.len());
            let mut y = Au::ZERO;
            let mut laid = Vec::with_capacity(range.len());
            for (item, target) in items[range.clone()].iter_mut().zip(targets) {
                let child_style = &self.tree.get(item.id).style;
                let model = self.box_model(child_style, width, None);
                let (left, right) = self.flex_margins(child_style, width);
                let side_margins = left + right;
                let frame_x =
                    model.border[LEFT] + model.border[RIGHT] + model.padding[LEFT] + model.padding[RIGHT];
                let content_width = item
                    .width
                    .unwrap_or((cross - side_margins - frame_x).clamp_non_negative());
                let border_box = (target - item.margins.0 - item.margins.1).clamp_non_negative();
                // The height of an item after flexing is definite, as in Chrome: percentage
                // heights of its children refer to it.
                let resized = border_box != item.fragment.rect.height;
                let target = (resized || self.has_percentage_heights(item.id)).then_some(border_box);
                let narrowed = item.fragment.rect.width - frame_x != content_width;
                if (narrowed || target.is_some())
                    && let Some(outcome) =
                        self.item_in(item.id, (width, containing_height), Some(content_width), target)
                {
                    item.fragment = outcome.fragment;
                    item.grown |= target.is_some();
                    // A box no shorter than its content takes the height it is given; the
                    // content of one shorter overflows it. A table is never shorter than its
                    // rows and captions.
                    if let Some(target) = target
                        && !matches!(self.tree.get(item.id).kind, BoxKind::Table { .. })
                    {
                        item.fragment.rect.height = target;
                    }
                }
                // Auto margins and alignment across the line.
                let mut fragment = item.fragment.clone();
                fragment.rect.x = line_x + self.cross_offset(style, item.id, cross, fragment.rect.width);
                fragment.rect.y = y + item.margins.0;
                y = fragment.rect.y + fragment.rect.height + item.margins.1 + row_gap;
                laid.push(fragment);
                boxes.push(LineItem {
                    id: item.id,
                    containing_width: width,
                    width: Some(content_width),
                    containing_height,
                    stretched: item.grown,
                    grows: true,
                    margins: item.margins,
                });
            }
            let end = (y - row_gap).max(Au::ZERO);
            // Free room in the line goes to auto margins, or else `justify-content` shares it
            // out (CSS Flexbox 1, §8.1, §9.5).
            let free = main - end;
            let autos: Vec<(bool, bool)> = items[range.clone()]
                .iter()
                .map(|item| {
                    let margin = &self.tree.get(item.id).style.margin;
                    (margin.top.is_auto(), margin.bottom.is_auto())
                })
                .collect();
            let auto_count: usize = autos
                .iter()
                .map(|&(top, bottom)| usize::from(top) + usize::from(bottom))
                .sum();
            if auto_count > 0 {
                let share = free.clamp_non_negative() / i32::try_from(auto_count).unwrap_or(i32::MAX);
                let mut shift = Au::ZERO;
                for (fragment, (top, bottom)) in laid.iter_mut().zip(autos) {
                    if top {
                        shift += share;
                    }
                    fragment.rect.y += shift;
                    if bottom {
                        shift += share;
                    }
                }
            } else {
                let (start, gap) = justify(style, free, laid.len());
                for (index, fragment) in laid.iter_mut().enumerate() {
                    fragment.rect.y += start + gap * i32::try_from(index).unwrap_or(i32::MAX);
                }
            }
            fragments.extend(laid);
            lines.push(ContainerLine {
                items: first..fragments.len(),
                top: Au::ZERO,
                bottom: end,
                boxes,
            });
        }
        if style.flex_wrap == FlexWrap::WrapReverse {
            // Cross-start is at the other side: the lines go the other way, and so do the margin
            // boxes of the items in them.
            for line in &lines {
                for (fragment, item) in fragments[line.items.clone()].iter_mut().zip(&line.boxes) {
                    let (left, right) = self.flex_margins(&self.tree.get(item.id).style, width);
                    fragment.rect.x = width - fragment.rect.x - fragment.rect.width - right + left;
                }
            }
        }
        (fragments, main, lines)
    }
}

/// The fewest lines a flex container styled `style` balances its items into, if it balances
/// them (`flex-wrap: balance`).
pub(crate) fn balance(style: &fagus_style::ComputedStyle) -> Option<usize> {
    style
        .flex_balance
        .then(|| usize::try_from(style.flex_line_count).unwrap_or(1))
}

/// The height of the captions of a table laid out as `fragment` (its wrapper box with them);
/// zero for other boxes.
pub(crate) fn captions_height(fragment: &Fragment) -> Au {
    if !matches!(fragment.kind, crate::fragment::FragmentKind::Anonymous) {
        return Au::ZERO;
    }
    fragment
        .children
        .iter()
        .find(|child| crate::block::is_table(child))
        .map_or(Au::ZERO, |table| {
            (fragment.rect.height - table.rect.height).clamp_non_negative()
        })
}

/// Where the content of a box laid out as `fragment` ends, below its top border and padding
/// (`top`), with its bottom padding and border (`frame`) below: its border-box height at its
/// content's height. Absolutely positioned children do not count.
fn content_extent(fragment: &Fragment, top: Au, frame: Au) -> Au {
    fragment
        .children
        .iter()
        .filter(|child| {
            !matches!(&child.kind, crate::fragment::FragmentKind::Box { style, .. }
                if style.is_out_of_flow())
        })
        .map(|child| child.rect.y + child.rect.height)
        .fold(top, Au::max)
        + frame
}

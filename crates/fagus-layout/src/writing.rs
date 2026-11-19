//! Writing modes: vertical text and boxes whose lines run another way than their parent's.
//!
//! Layout works in the coordinates of a box's writing mode, where lines always run along x and
//! stack along y: for a box with vertical lines the box tree holds its style turned a quarter
//! (its height is its width, its right side its top in `vertical-rl`), and the layout algorithms,
//! which only know horizontal lines, lay it out as they would horizontal text. Where the writing
//! mode changes, at a box whose content flows another way than the box itself (a flow root), the
//! fragments of the content are turned to the coordinates of the box (CSS Writing Modes 4, §6),
//! and at the end of layout to the page's. Horizontal documents never take these paths.
//!
//! Every flow root holds its content in an anonymous box of its own: the flow root is laid out
//! in its parent's writing mode, with its margins, borders, padding and size, and the content box
//! inside in the flow root's own writing mode. Between orthogonal writing modes (horizontal and
//! vertical) the flow root sizes itself as CSS Writing Modes 4, §7.3 and Chrome do: its content
//! is as long as the room the flow root's height leaves (or, where that is not definite, the
//! page area's), down to its max-content size, and the flow root is as wide as the lines that
//! make.

use std::sync::Arc;

use fagus_style::values::{
    Display, DisplayInside, DisplayOutside, LengthPercentage, MaxSize, Sides, Size, WritingMode,
};
use fagus_style::{ComputedStyle, ComputedValues};
use rustc_hash::FxHashMap;

use crate::block::{BOTTOM, BoxModel, LEFT, Layouter, Outcome, RIGHT, Space, TOP};
use crate::fragment::{Fragment, FragmentKind, VisibleSides};
use crate::tree::{BoxId, BoxKind, BoxTree, InlineItem, LayoutBox};
use crate::units::{Au, Rect};

/// The writing modes of a box tree: the boxes whose content flows another way than they do, and
/// the physical styles of the boxes whose styles are turned.
#[derive(Debug, Default)]
pub(crate) struct Flows {
    /// The flow roots, with the anonymous box that holds their content.
    pub(crate) roots: FxHashMap<BoxId, BoxId>,
    /// The styles as written, by the address of the turned style that stands for them in the box
    /// tree, for painting.
    physical: FxHashMap<usize, Arc<ComputedStyle>>,
    /// Turned styles, by the address of the style as written and the writing mode.
    turned: FxHashMap<(usize, WritingMode), Arc<ComputedStyle>>,
    /// The absolutely positioned boxes whose containing block lays out in another writing mode
    /// than the content around them, with whether their static position is that of their right
    /// and of their bottom edge in the containing block's coordinates rather than of their left
    /// and top ones: it is the corner of the block-start and inline-start sides of the content
    /// they are in.
    pub(crate) static_ends: FxHashMap<BoxId, (bool, bool)>,
    /// The styles as written of the boxes that hold the content of flow roots, by address: the
    /// fragments of such a box are on the page already, with the styles as written, when the
    /// fragments around it turn (see [`to_physical`]).
    contents: rustc_hash::FxHashSet<usize>,
    /// The flow roots that are flex or grid items, whose container sizes them.
    items: rustc_hash::FxHashSet<BoxId>,
}

impl Flows {
    /// The box that holds the content of flow root `id`, if it is one.
    pub(crate) fn content_of(&self, id: BoxId) -> Option<BoxId> {
        if self.roots.is_empty() {
            return None;
        }
        self.roots.get(&id).copied()
    }

    /// Whether a fragment is that of the box that holds the content of a flow root.
    fn is_content(&self, fragment: &Fragment) -> bool {
        !self.contents.is_empty()
            && matches!(
                &fragment.kind,
                FragmentKind::Box { style, node: None, .. }
                    if self.contents.contains(&(Arc::as_ptr(self.physical(style)) as usize))
            )
    }

    /// The style as written of a style in the box tree, turned or not (by reference).
    pub(crate) fn physical_of<'s>(&'s self, style: &'s ComputedStyle) -> &'s ComputedStyle {
        if self.physical.is_empty() {
            return style;
        }
        self.physical
            .get(&(std::ptr::from_ref(style) as usize))
            .map_or(style, |physical| &**physical)
    }

    /// The style as written of a style in the box tree, turned or not.
    pub(crate) fn physical<'s>(&'s self, style: &'s Arc<ComputedStyle>) -> &'s Arc<ComputedStyle> {
        self.physical.get(&(Arc::as_ptr(style) as usize)).unwrap_or(style)
    }

    /// The style to paint a box with that was laid out in writing mode `mode`: the style as
    /// written, or for a style that layout made (the collapsed borders of a table's cells), that
    /// style with its borders on the physical sides they stand for.
    fn physical_box(&self, style: &Arc<ComputedStyle>, mode: WritingMode) -> Arc<ComputedStyle> {
        if let Some(physical) = self.physical.get(&(Arc::as_ptr(style) as usize)) {
            return Arc::clone(physical);
        }
        let widths = &style.border_width;
        if !mode.is_vertical()
            || [widths.top, widths.right, widths.bottom, widths.left]
                .iter()
                .all(|&width| width <= 0.0)
        {
            return Arc::clone(style);
        }
        let mut physical = (**style).clone();
        physical.border_width = physical_sides(&style.border_width, mode);
        physical.border_style = physical_sides(&style.border_style, mode);
        physical.border_color = physical_sides(&style.border_color, mode);
        Arc::new(physical)
    }

    /// `style` turned for layout in writing mode `mode`; the same style in horizontal writing.
    fn turn(&mut self, style: &Arc<ComputedStyle>, mode: WritingMode) -> Arc<ComputedStyle> {
        if !mode.is_vertical() {
            return Arc::clone(style);
        }
        // A style that is turned already stays.
        if self.physical.contains_key(&(Arc::as_ptr(style) as usize)) {
            return Arc::clone(style);
        }
        let key = (Arc::as_ptr(style) as usize, mode);
        if let Some(turned) = self.turned.get(&key) {
            return Arc::clone(turned);
        }
        let turned = Arc::new(logical_style(style, mode));
        self.physical
            .insert(Arc::as_ptr(&turned) as usize, Arc::clone(style));
        self.turned.insert(key, Arc::clone(&turned));
        turned
    }
}

/// The sides of a box in a writing mode, from the physical ones: the top is the block-start
/// side, the left the line-left side (CSS Writing Modes 4, §6.2-6.3).
fn logical_sides<T: Clone>(sides: &Sides<T>, mode: WritingMode) -> Sides<T> {
    let (top, right, bottom, left) = match mode {
        WritingMode::HorizontalTb => return sides.clone(),
        WritingMode::VerticalRl | WritingMode::SidewaysRl => {
            (&sides.right, &sides.bottom, &sides.left, &sides.top)
        }
        WritingMode::VerticalLr => (&sides.left, &sides.bottom, &sides.right, &sides.top),
        WritingMode::SidewaysLr => (&sides.left, &sides.top, &sides.right, &sides.bottom),
    };
    Sides {
        top: top.clone(),
        right: right.clone(),
        bottom: bottom.clone(),
        left: left.clone(),
    }
}

/// The physical sides of a box from those in a writing mode (the inverse of [`logical_sides`]).
fn physical_sides<T: Clone>(sides: &Sides<T>, mode: WritingMode) -> Sides<T> {
    let (top, right, bottom, left) = match mode {
        WritingMode::HorizontalTb => return sides.clone(),
        WritingMode::VerticalRl | WritingMode::SidewaysRl => {
            (&sides.left, &sides.top, &sides.right, &sides.bottom)
        }
        WritingMode::VerticalLr => (&sides.left, &sides.bottom, &sides.right, &sides.top),
        WritingMode::SidewaysLr => (&sides.right, &sides.bottom, &sides.left, &sides.top),
    };
    Sides {
        top: top.clone(),
        right: right.clone(),
        bottom: bottom.clone(),
        left: left.clone(),
    }
}

/// A style turned for layout in a vertical writing mode: sizes, margins, borders, padding and
/// insets on the sides they stand for in its lines (the height is the width, the block-start
/// side the top). What layout reads besides is logical already (line height, `text-align`,
/// floats, flex and grid directions); painting uses the style as written.
fn logical_style(style: &ComputedStyle, mode: WritingMode) -> ComputedStyle {
    let mut turned = style.clone();
    turned.width = style.height.clone();
    turned.height = style.width.clone();
    turned.min_width = style.min_height.clone();
    turned.min_height = style.min_width.clone();
    turned.max_width = style.max_height.clone();
    turned.max_height = style.max_width.clone();
    turned.aspect_ratio = style.aspect_ratio.map(|ratio| 1.0 / ratio);
    turned.contain_intrinsic_size = (style.contain_intrinsic_size.1, style.contain_intrinsic_size.0);
    turned.margin = logical_sides(&style.margin, mode);
    turned.padding = logical_sides(&style.padding, mode);
    turned.border_width = logical_sides(&style.border_width, mode);
    turned.border_style = logical_sides(&style.border_style, mode);
    turned.border_color = logical_sides(&style.border_color, mode);
    turned.inset = logical_sides(&style.inset, mode);
    turned.overflow_x = style.overflow_y;
    turned.overflow_y = style.overflow_x;
    turned.scrollbar_x = style.scrollbar_y;
    turned.scrollbar_y = style.scrollbar_x;
    turned
}

/// The style of the anonymous box that holds a flow root's content: the flow root's inherited
/// properties and those that apply to its content (its display inside, columns, flex and grid
/// containers), without margins, borders, padding or sizes of its own, which stay with the
/// flow root.
fn content_style(style: &ComputedStyle, orthogonal: bool) -> ComputedStyle {
    let vertical = style.writing_mode.is_vertical();
    // Flex and grid containers and multi-column containers (whose columns are as tall as it is
    // where that is definite, and filled in turn, `column-fill: auto`) fill it.
    let flex_or_grid = style.is_multicol()
        || matches!(
            style.display,
            Display::Normal {
                inside: DisplayInside::Flex | DisplayInside::Grid,
                ..
            }
        );
    // A table whose lines run across its parent's has the length its flow root's size gives
    // them where that is set (its `height` in vertical writing modes), as in Chrome; it shrinks
    // to fit otherwise.
    let inline_size = if vertical { &style.height } else { &style.width };
    let table = orthogonal
        && matches!(inline_size, Size::Length(_))
        && matches!(
            style.display,
            Display::Normal {
                inside: DisplayInside::Table,
                ..
            }
        );
    // The borders of a table whose borders collapse are those of its cells' grid, which the
    // content lays out.
    let collapsed = collapses(style);
    let fill = |fills: bool| {
        if fills {
            Size::Length(LengthPercentage::Percent(1.0))
        } else {
            Size::Auto
        }
    };
    let inside = match style.display {
        Display::Normal { inside, .. } => match inside {
            DisplayInside::Flow | DisplayInside::FlowRoot => DisplayInside::FlowRoot,
            other => other,
        },
        _ => DisplayInside::FlowRoot,
    };
    let mut content = ComputedStyle::from(ComputedValues {
        display: Display::Normal {
            outside: DisplayOutside::Block,
            inside,
            list_item: false,
        },
        unicode_bidi: style.unicode_bidi,
        text_overflow: style.text_overflow.clone(),
        line_clamp: style.line_clamp,
        column_count: style.column_count,
        column_width: style.column_width,
        column_gap: style.column_gap.clone(),
        row_gap: style.row_gap.clone(),
        column_rule_width: style.column_rule_width,
        column_rule_style: style.column_rule_style,
        column_rule_color: style.column_rule_color,
        column_fill: style.column_fill,
        flex_direction: style.flex_direction,
        flex_wrap: style.flex_wrap,
        flex_balance: style.flex_balance,
        flex_line_count: style.flex_line_count,
        justify_content: style.justify_content,
        justify_content_safe: style.justify_content_safe,
        align_content: style.align_content,
        align_content_safe: style.align_content_safe,
        align_items: style.align_items,
        justify_items: style.justify_items,
        grid_template_columns: style.grid_template_columns.clone(),
        grid_template_rows: style.grid_template_rows.clone(),
        grid_template_areas: style.grid_template_areas.clone(),
        grid_auto_rows: Arc::clone(&style.grid_auto_rows),
        grid_auto_columns: Arc::clone(&style.grid_auto_columns),
        grid_auto_flow_column: style.grid_auto_flow_column,
        grid_auto_flow_dense: style.grid_auto_flow_dense,
        text_box_trim: style.text_box_trim,
        table_layout: style.table_layout,
        // A flex or grid container fills the flow root in its block direction where that is
        // definite, so that its items align across its lines in all of it (as in Chrome, where
        // the content box is the flow root's own); a table in its inline one. The style is
        // turned after.
        width: fill((flex_or_grid && vertical) || (table && !vertical)),
        height: fill((flex_or_grid && !vertical) || (table && vertical)),
        ..ComputedValues::from(style.inherit())
    });
    // Lines that overflow a flow root that clips them end with an ellipsis: its content box,
    // where they are, clips them too (in the flow root's content box, which is its own).
    if style.text_overflow == fagus_style::values::TextOverflow::Ellipsis
        && (style.overflow_x != fagus_style::values::Overflow::Visible
            || style.overflow_y != fagus_style::values::Overflow::Visible)
    {
        content.overflow_x = fagus_style::values::Overflow::Clip;
        content.overflow_y = fagus_style::values::Overflow::Clip;
    }
    if collapsed {
        content.border_width = style.border_width;
        content.border_style = style.border_style;
        content.border_color = style.border_color;
    }
    content
}

/// Whether a style is that of a table whose borders collapse.
fn collapses(style: &ComputedStyle) -> bool {
    style.border_collapse == fagus_style::values::BorderCollapse::Collapse
        && matches!(
            style.display,
            Display::Normal {
                inside: DisplayInside::Table,
                ..
            }
        )
}

/// The flow root itself keeps none of what its content box took.
fn outer_style(style: &ComputedStyle) -> ComputedStyle {
    let initial = ComputedStyle::initial_values();
    let collapsed = collapses(style);
    ComputedStyle::from(ComputedValues {
        border_width: if collapsed {
            initial.border_width
        } else {
            style.border_width
        },
        border_style: if collapsed {
            initial.border_style
        } else {
            style.border_style
        },
        column_count: None,
        column_width: None,
        line_clamp: None,
        text_box_trim: initial.text_box_trim,
        ..ComputedValues::from(style.clone())
    })
}

/// The writing modes in which the containing blocks of absolutely positioned and fixed boxes
/// lay them out.
#[derive(Clone, Copy)]
struct Containers {
    absolute: WritingMode,
    fixed: WritingMode,
}

/// Prepares a box tree for writing modes: turns the styles of the boxes laid out in vertical
/// writing modes and puts the content of flow roots into boxes of its own.
pub(crate) fn prepare(tree: &mut BoxTree) {
    let Some(root) = tree.root else { return };
    let mut flows = std::mem::take(&mut tree.flows);
    // The root lays out in its own writing mode, which the initial containing block takes.
    let mode = tree.get(root).style.writing_mode;
    let containers = Containers {
        absolute: mode,
        fixed: mode,
    };
    walk(tree, &mut flows, root, mode, containers);
    tree.flows = flows;
}

/// Prepares box `id`, laid out in writing mode `frame`, and the boxes inside, whose absolutely
/// positioned boxes lay out in `containers` unless a box inside contains them.
fn walk(tree: &mut BoxTree, flows: &mut Flows, id: BoxId, frame: WritingMode, containers: Containers) {
    let layout_box = tree.get(id);
    let own = layout_box.style.writing_mode;
    // A positioned box lays out the absolutely positioned boxes inside in its own writing mode.
    let containers = if layout_box.style.contains_absolute() {
        Containers {
            absolute: own,
            ..containers
        }
    } else {
        containers
    };
    // The parts of a table lay out in the writing mode of their table (their cells may not);
    // replaced elements have no lines.
    let keeps_frame = matches!(
        layout_box.kind,
        BoxKind::RowGroup { .. } | BoxKind::Row { .. } | BoxKind::Replaced { .. }
    );
    if own != frame && !keeps_frame {
        // A flow root: its content moves into an anonymous box in its own writing mode.
        let style = Arc::clone(&layout_box.style);
        let written = Arc::new(content_style(&style, !own.is_parallel(frame)));
        flows.contents.insert(Arc::as_ptr(&written) as usize);
        let inner_style = flows.turn(&written, own);
        let kind = std::mem::replace(
            &mut tree.box_mut(id).kind,
            BoxKind::Block { children: Vec::new() },
        );
        let inner = tree.push(LayoutBox {
            style: inner_style,
            node: None,
            generated: false,
            kind,
        });
        if let Some(first_line) = tree.first_lines.remove(&id) {
            tree.first_lines.insert(inner, first_line);
        }
        tree.box_mut(id).kind = BoxKind::Block {
            children: vec![inner],
        };
        flows.roots.insert(id, inner);
        contents(tree, flows, inner, own, containers);
        let outer = Arc::new(outer_style(&style));
        tree.box_mut(id).style = flows.turn(&outer, frame);
        return;
    }
    let content = if keeps_frame { frame } else { own };
    contents(tree, flows, id, content, containers);
    let style = flows.turn(&tree.get(id).style, frame);
    tree.box_mut(id).style = style;
}

/// Prepares the content of box `id`, laid out in writing mode `mode`.
fn contents(tree: &mut BoxTree, flows: &mut Flows, id: BoxId, mode: WritingMode, containers: Containers) {
    if mode.is_vertical() {
        if let Some(first_line) = tree.first_lines.get(&id).cloned() {
            let turned = flows.turn(&first_line, mode);
            tree.first_lines.insert(id, turned);
        }
        match &mut tree.box_mut(id).kind {
            BoxKind::Inline { items } => {
                for item in items {
                    if let Some(style) = item_style(item) {
                        *style = flows.turn(style, mode);
                    }
                }
            }
            BoxKind::Table {
                columns,
                column_backgrounds,
                ..
            } => {
                for column in columns {
                    *column = flows.turn(column, mode);
                }
                for background in column_backgrounds {
                    background.style = flows.turn(&background.style, mode);
                }
            }
            _ => {}
        }
    }
    let rtl = tree.get(id).style.is_rtl();
    let items = matches!(
        tree.get(id).style.display,
        Display::Normal {
            inside: DisplayInside::Flex | DisplayInside::Grid,
            ..
        }
    );
    for child in tree.children(id) {
        if items && tree.get(child).style.writing_mode != mode {
            flows.items.insert(child);
        }
        // An absolutely positioned box lays out in the writing mode of its containing block,
        // where it may be a flow root of its own.
        let frame = match tree.get(child).style.position {
            fagus_style::values::Position::Absolute => containers.absolute,
            fagus_style::values::Position::Fixed => containers.fixed,
            _ => mode,
        };
        if frame != mode {
            flows.static_ends.insert(child, static_ends(mode, rtl, frame));
        }
        walk(tree, flows, child, frame, containers);
    }
}

/// Which edges of a box the static position gives in the coordinates of writing mode
/// `frame`, where the box's containing block lays it out, for a box in content laid out in
/// `mode` (right to left with `rtl`): whether its right edge rather than its left one, and its
/// bottom rather than its top. The static position is the corner of the block-start and the
/// inline-start sides of that content.
fn static_ends(mode: WritingMode, rtl: bool, frame: WritingMode) -> (bool, bool) {
    // Physical sides as indices: top, right, bottom, left.
    let block_start = match mode {
        WritingMode::HorizontalTb => TOP,
        WritingMode::VerticalRl | WritingMode::SidewaysRl => RIGHT,
        WritingMode::VerticalLr | WritingMode::SidewaysLr => LEFT,
    };
    let inline_start = match (mode, rtl) {
        (WritingMode::HorizontalTb, false) => LEFT,
        (WritingMode::HorizontalTb, true) => RIGHT,
        (WritingMode::SidewaysLr, false) => BOTTOM,
        (WritingMode::SidewaysLr, true) => TOP,
        (_, false) => TOP,
        (_, true) => BOTTOM,
    };
    // The side each physical one is in the frame's coordinates.
    let in_frame = |side: usize| match frame {
        WritingMode::HorizontalTb => side,
        WritingMode::VerticalRl | WritingMode::SidewaysRl => [LEFT, TOP, RIGHT, BOTTOM][side],
        WritingMode::VerticalLr => [LEFT, BOTTOM, RIGHT, TOP][side],
        WritingMode::SidewaysLr => [RIGHT, BOTTOM, LEFT, TOP][side],
    };
    let sides = [in_frame(block_start), in_frame(inline_start)];
    (sides.contains(&RIGHT), sides.contains(&BOTTOM))
}

/// The style an inline item carries, if it has one.
fn item_style(item: &mut InlineItem) -> Option<&mut Arc<ComputedStyle>> {
    match item {
        InlineItem::Text { style, .. }
        | InlineItem::LineBreak { style }
        | InlineItem::Open { style, .. }
        | InlineItem::Marker { style, .. }
        | InlineItem::NamedString { style, .. }
        | InlineItem::Leader { style, .. }
        | InlineItem::TargetPage { style, .. }
        | InlineItem::PageNumber { style, .. } => Some(style),
        InlineItem::OutOfFlow(_) | InlineItem::Float(_) | InlineItem::Atomic(_) | InlineItem::Close => None,
    }
}

/// A width and a height.
type Size2 = (Au, Au);

/// Whether a fragment in a line is an inline box, whose content belongs to the line too (and
/// not an atomic inline or a float, whose content is laid out as blocks).
fn is_inline_box(fragment: &Fragment) -> bool {
    matches!(
        &fragment.kind,
        FragmentKind::Box { style, .. } if !style.is_floating() && matches!(
            style.display,
            Display::Normal {
                outside: DisplayOutside::Inline,
                inside: DisplayInside::Flow,
                ..
            }
        )
    )
}

/// Whether the children of a fragment are content of a line: those of a line, of an inline box
/// in one, and of a ruby column and its annotation, which goes over or under its base as the
/// line's text does.
pub(crate) fn holds_line_content(fragment: &Fragment, in_line: bool) -> bool {
    matches!(fragment.kind, FragmentKind::Line { .. })
        || (in_line
            && (is_inline_box(fragment)
                || matches!(
                    &fragment.kind,
                    FragmentKind::Box { style, .. } if matches!(
                        style.ruby_display,
                        fagus_style::values::RubyDisplay::Column | fagus_style::values::RubyDisplay::Annotation
                    )
                )))
}

/// A rectangle laid out in writing mode `mode`, inside a parent of logical size `parent`, on the
/// page. In `vertical-lr` lines stack left to right but their over side, where the ascent of the
/// text goes, is on the right: content of lines (`in_line`) is mirrored within them.
fn physical_rect(rect: Rect, mode: WritingMode, parent: Size2, in_line: bool) -> Rect {
    let Rect { x, y, width, height } = rect;
    let (parent_width, parent_height) = parent;
    match mode {
        WritingMode::HorizontalTb => rect,
        WritingMode::VerticalRl | WritingMode::SidewaysRl => {
            Rect::new(parent_height - y - height, x, height, width)
        }
        WritingMode::VerticalLr if in_line => Rect::new(parent_height - y - height, x, height, width),
        WritingMode::VerticalLr => Rect::new(y, x, height, width),
        WritingMode::SidewaysLr => Rect::new(y, parent_width - x - width, height, width),
    }
}

/// The inverse of [`physical_rect`]: a rectangle on the page, inside a parent of physical size
/// `parent`, in the coordinates of writing mode `mode`.
fn logical_rect(rect: Rect, mode: WritingMode, parent: Size2, in_line: bool) -> Rect {
    let Rect { x, y, width, height } = rect;
    let (parent_width, parent_height) = parent;
    match mode {
        WritingMode::HorizontalTb => rect,
        WritingMode::VerticalRl | WritingMode::SidewaysRl => {
            Rect::new(y, parent_width - x - width, height, width)
        }
        WritingMode::VerticalLr if in_line => Rect::new(y, parent_width - x - width, height, width),
        WritingMode::VerticalLr => Rect::new(y, x, height, width),
        WritingMode::SidewaysLr => Rect::new(parent_height - y - height, x, height, width),
    }
}

/// The physical size of a logical one in writing mode `mode`.
pub(crate) fn physical_size(size: Size2, mode: WritingMode) -> Size2 {
    if mode.is_vertical() {
        (size.1, size.0)
    } else {
        size
    }
}

/// Turns a fragment laid out in writing mode `mode` inside a parent of logical size `parent`
/// to the page's coordinates, with the styles as written.
pub(crate) fn to_physical(fragment: &mut Fragment, mode: WritingMode, parent: Size2, flows: &Flows) {
    to_physical_in(fragment, mode, parent, false, flows);
}

fn to_physical_in(fragment: &mut Fragment, mode: WritingMode, parent: Size2, in_line: bool, flows: &Flows) {
    let own = (fragment.rect.width, fragment.rect.height);
    // A placeholder in a line stands for the line's block-start side, which is not mirrored.
    let in_line = in_line && !matches!(fragment.kind, FragmentKind::Placeholder(_));
    fragment.rect = physical_rect(fragment.rect, mode, parent, in_line);
    let line = holds_line_content(fragment, in_line);
    match &mut fragment.kind {
        FragmentKind::Box { style, sides, .. } => {
            *style = flows.physical_box(style, mode);
            *sides = physical_visible(*sides, mode, in_line);
        }
        FragmentKind::Text(run) => run.style = Arc::clone(flows.physical(&run.style)),
        _ => {}
    }
    for child in &mut fragment.children {
        if flows.is_content(child) {
            // The content of a flow root, with its styles as written already.
            place_physical(child, mode, own, line);
        } else {
            to_physical_in(child, mode, own, line, flows);
        }
    }
}

/// Turns the rectangles of a fragment and its children from the coordinates of writing mode
/// `mode` to the page's, as [`to_physical`] does, without their styles: those of the content of
/// a flow root, written already.
fn place_physical(fragment: &mut Fragment, mode: WritingMode, parent: Size2, in_line: bool) {
    let own = (fragment.rect.width, fragment.rect.height);
    let in_line = in_line && !matches!(fragment.kind, FragmentKind::Placeholder(_));
    fragment.rect = physical_rect(fragment.rect, mode, parent, in_line);
    let line = holds_line_content(fragment, in_line);
    for child in &mut fragment.children {
        place_physical(child, mode, own, line);
    }
}

/// The sides of a box laid out in writing mode `mode` (in a line with `in_line`) that are drawn,
/// on the page: those it was laid out with are on the sides of its lines (the top is its
/// block-start side, the left its line-left one).
fn physical_visible(sides: VisibleSides, mode: WritingMode, in_line: bool) -> VisibleSides {
    if sides.all() || !mode.is_vertical() {
        return sides;
    }
    // In `vertical-lr` the content of lines is mirrored (see `physical_rect`).
    let mode = if in_line && mode == WritingMode::VerticalLr {
        WritingMode::VerticalRl
    } else {
        mode
    };
    let logical = Sides {
        top: sides.top,
        right: sides.right,
        bottom: sides.bottom,
        left: sides.left,
    };
    let physical = physical_sides(&logical, mode);
    VisibleSides {
        top: physical.top,
        right: physical.right,
        bottom: physical.bottom,
        left: physical.left,
    }
}

/// Turns a fragment on the page, inside a parent of physical size `parent`, to the coordinates
/// of writing mode `mode`.
pub(crate) fn from_physical(fragment: &mut Fragment, mode: WritingMode, parent: Size2) {
    from_physical_in(fragment, mode, parent, false);
}

fn from_physical_in(fragment: &mut Fragment, mode: WritingMode, parent: Size2, in_line: bool) {
    let own = (fragment.rect.width, fragment.rect.height);
    let in_line = in_line && !matches!(fragment.kind, FragmentKind::Placeholder(_));
    fragment.rect = logical_rect(fragment.rect, mode, parent, in_line);
    let line = holds_line_content(fragment, in_line);
    for child in &mut fragment.children {
        from_physical_in(child, mode, own, line);
    }
}

impl Layouter<'_> {
    /// The writing mode of the boxes being laid out.
    pub(crate) fn writing_mode(&self) -> WritingMode {
        self.frame.get()
    }

    /// Runs `layout` with the boxes in writing mode `mode`.
    fn in_mode<T>(&self, mode: WritingMode, layout: impl FnOnce() -> T) -> T {
        let outer = self.frame.replace(mode);
        let room = self.cross_room.replace(None);
        let result = layout();
        self.frame.set(outer);
        self.cross_room.set(room);
        result
    }

    /// Runs `layout` with `room` for the content of orthogonal flow roots without a definite
    /// containing block height: that of the flex container whose items they are.
    pub(crate) fn with_cross_room<T>(&self, room: Option<Au>, layout: impl FnOnce() -> T) -> T {
        let outer = self.cross_room.replace(room);
        let result = layout();
        self.cross_room.set(outer);
        result
    }

    /// The length of the content of an orthogonal flow root where its height does not say:
    /// the height of its containing block if that is definite (or of the flex container it is
    /// an item of), else of the initial containing block (the page area), as in Chrome.
    fn orthogonal_room(&self, containing_height: Option<Au>) -> Au {
        containing_height.or(self.cross_room.get()).unwrap_or_else(|| {
            let (width, height) = self.initial_containing_block.get();
            if self.writing_mode().is_vertical() {
                width
            } else {
                height
            }
        })
    }

    /// The inline size of the content of orthogonal flow root `id` (its content box's height):
    /// its definite height, or the room for it, down to the content's max-content size and up
    /// to its min-content size, within `min-height` and `max-height`. `block` is its content
    /// box's width where that is definite.
    pub(crate) fn orthogonal_inline_size(
        &self,
        id: BoxId,
        inner: BoxId,
        (definite, block): (Option<Au>, Option<Au>),
        (containing_width, containing_height): (Au, Option<Au>),
    ) -> Au {
        if let Some(height) = definite {
            return height;
        }
        let style = &self.tree.get(id).style;
        let mode = self.tree.get(inner).style.writing_mode;
        // A form control's lines are as long as its own measure gives them (a text area's
        // columns, a list's longest option). The percentage block sizes of replaced elements
        // inside refer to the flow root's width where that is definite, as in Chrome.
        let (min, max) = match self.tree.control_widths.get(&id) {
            Some(&width) => (Au::from_px(width), Au::from_px(width)),
            None if block.is_some() => self.in_mode(mode, || self.intrinsic_at_height(inner, block)),
            None => self.in_mode(mode, || self.intrinsic(inner)),
        };
        let room = self.orthogonal_room(containing_height);
        // Percentages of the margins and padding refer to the containing block's width.
        let model = self.box_model(style, containing_width, Some(Au::ZERO));
        let frame = model.border[TOP] + model.border[BOTTOM] + model.padding[TOP] + model.padding[BOTTOM];
        let border_box = style.box_sizing == fagus_style::values::BoxSizing::BorderBox;
        let content = |length: &LengthPercentage| {
            length.fixed().map(|px| {
                let px = Au::from_px(px);
                if border_box {
                    (px - frame).clamp_non_negative()
                } else {
                    px
                }
            })
        };
        // The room left by the margins, borders and padding of the flow root.
        let margins = self.vertical_margin(style, containing_width, true)
            + self.vertical_margin(style, containing_width, false);
        let mut size = max.min((room - frame - margins).max(min));
        if let MaxSize::Length(length) = &style.max_height
            && let Some(limit) = content(length)
        {
            size = size.min(limit);
        }
        if let Size::Length(length) = &style.min_height
            && let Some(limit) = content(length)
        {
            size = size.max(limit);
        }
        size
    }

    /// The width of the content box of flow root `id` from the block size of its content, within
    /// `min-width` and `max-width`.
    fn orthogonal_width(&self, id: BoxId, block: Au) -> Au {
        let style = &self.tree.get(id).style;
        let model = self.box_model(style, Au::ZERO, Some(Au::ZERO));
        let frame = model.border[LEFT] + model.border[RIGHT] + model.padding[LEFT] + model.padding[RIGHT];
        let border_box = style.box_sizing == fagus_style::values::BoxSizing::BorderBox;
        let content = |length: &LengthPercentage| {
            length.fixed().map(|px| {
                let px = Au::from_px(px);
                if border_box {
                    (px - frame).clamp_non_negative()
                } else {
                    px
                }
            })
        };
        // Size containment: as wide as `contain-intrinsic-size` says, whatever the content.
        let mut width = if style.size_containment().0 {
            Au::from_px(style.contain_intrinsic_size.0.unwrap_or(0.0))
        } else {
            block
        };
        if let MaxSize::Length(length) = &style.max_width
            && let Some(limit) = content(length)
        {
            width = width.min(limit);
        }
        if let Size::Length(length) = &style.min_width
            && let Some(limit) = content(length)
        {
            width = width.max(limit);
        }
        width
    }

    /// Lays out the content box `inner` of a flow root at inline size `inline`, in its own
    /// writing mode, in a flow root whose block size is `block` where that is definite.
    fn flow_content(&self, inner: BoxId, inline: Au, block: Option<Au>) -> Option<Outcome> {
        let mode = self.tree.get(inner).style.writing_mode;
        let _whole = self.enter_unfragmented();
        self.in_mode(mode, || {
            let space = Space {
                containing_height: block,
                ..Space::unfragmented(inline)
            };
            self.block_level(inner, space, None)
        })
    }

    /// Whether box `id` is a flow root whose content runs across the boxes around it.
    pub(crate) fn is_orthogonal(&self, id: BoxId) -> bool {
        self.tree.flows.content_of(id).is_some_and(|inner| {
            !self
                .tree
                .get(inner)
                .style
                .writing_mode
                .is_parallel(self.writing_mode())
        })
    }

    /// The definite height of the content box of a box, for the length of the lines of its
    /// orthogonal content. A table cell's `height` only sets that of its row, which the cell then
    /// takes (`stretched`).
    fn content_height(
        &self,
        style: &ComputedStyle,
        model: &BoxModel,
        containing_height: Option<Au>,
        stretched: Option<Au>,
    ) -> Option<Au> {
        if !is_table_cell(style) {
            return Self::definite_height(style, model, containing_height, stretched);
        }
        let frame = model.border[TOP] + model.border[BOTTOM] + model.padding[TOP] + model.padding[BOTTOM];
        if let Some(height) = stretched {
            return Some((height - frame).clamp_non_negative());
        }
        let Size::Length(length) = &style.height else {
            return None;
        };
        let height = Au::from_px(length.fixed()?);
        Some(if style.box_sizing == fagus_style::values::BoxSizing::BorderBox {
            (height - frame).clamp_non_negative()
        } else {
            height
        })
    }

    /// The min-content and max-content widths of flow root `id` whose content is orthogonal to
    /// it, plus its horizontal margins: both are the block size of its content laid out as it
    /// would be (CSS Writing Modes 4, §7.3).
    pub(crate) fn orthogonal_widths(&self, id: BoxId, inner: BoxId) -> Option<(Au, Au)> {
        let outer = self.writing_mode();
        let mode = self.tree.get(inner).style.writing_mode;
        if mode.is_parallel(outer) {
            return None;
        }
        let style = &self.tree.get(id).style;
        let model = self.box_model(style, Au::ZERO, None);
        let content = if let Some(empty) = contained_width(style) {
            empty
        } else {
            let definite = self.content_height(style, &model, None, None);
            let inline = self.orthogonal_inline_size(id, inner, (definite, None), (Au::ZERO, None));
            self.flow_content(inner, inline, None)
                .map_or(Au::ZERO, |outcome| outcome.fragment.rect.height)
        };
        let frame = model.border[LEFT]
            + model.border[RIGHT]
            + model.padding[LEFT]
            + model.padding[RIGHT]
            + model.margin[LEFT]
            + model.margin[RIGHT];
        let width = self.orthogonal_width(id, content) + frame;
        Some((width, width))
    }

    /// Lays out flow root `id`, whose content box `inner` flows in another writing mode, as a
    /// whole: it does not break (as in Chrome). `None` if it does not fit in the remaining space.
    pub(crate) fn flow_root(
        &self,
        id: BoxId,
        inner: BoxId,
        space: Space,
        width_override: Option<Au>,
        height_override: Option<Au>,
    ) -> Option<Outcome> {
        let layout_box = self.tree.get(id);
        let style = &layout_box.style;
        let outer = self.writing_mode();
        let mode = self.tree.get(inner).style.writing_mode;
        let model = self.flow_model(style, &space, width_override);
        let orthogonal = !mode.is_parallel(outer);
        // Percentages of the inline size of orthogonal content in block flow refer to the room it
        // has where its containing block's height is not definite (not those of flex and grid
        // items, which their container sizes).
        let space = if orthogonal
            && space.containing_height.is_none()
            && width_override.is_none()
            && height_override.is_none()
            && !self.tree.flows.items.contains(&id)
        {
            Space {
                containing_height: Some(self.orthogonal_room(None)),
                ..space
            }
        } else {
            space
        };
        let (inline, block) = if orthogonal {
            let definite = self.content_height(style, &model, space.containing_height, height_override);
            let fixed = width_override.is_some() || !matches!(style.width, Size::Auto);
            let block = fixed.then_some(model.content_width);
            let inline = self.orthogonal_inline_size(
                id,
                inner,
                (definite, block),
                (space.containing_width, space.containing_height),
            );
            (inline, block)
        } else {
            let definite = Self::definite_height(style, &model, space.containing_height, height_override);
            (model.content_width, definite)
        };
        let mut content = self.flow_content(inner, inline, block)?.fragment;
        let (content_width, content_height) = if orthogonal {
            let width = block.unwrap_or_else(|| {
                self.orthogonal_width(id, contained_width(style).unwrap_or(content.rect.height))
            });
            let mut height = Self::used_height(style, &model, content.rect.width, space.containing_height);
            // A table's size is the least it takes: it grows to fit its columns (CSS 2.1,
            // §17.5.2), across its lines too.
            if matches!(self.tree.get(inner).kind, BoxKind::Table { .. }) {
                height = height.max(content.rect.width);
            }
            (width, height)
        } else {
            let height = Self::used_height(style, &model, content.rect.height, space.containing_height);
            (model.content_width, height)
        };
        let height_override = height_override.map(|height| {
            (height - model.border[TOP] - model.border[BOTTOM] - model.padding[TOP] - model.padding[BOTTOM])
                .clamp_non_negative()
        });
        let content_height = height_override.map_or(content_height, |height| height.max(content_height));
        // Absolutely positioned boxes inside are placed in the writing mode of the content, in
        // the flow root where it is their containing block (at its size, which its content may
        // not fill); the others later, on the page.
        self.in_mode(mode, || {
            let (width, height) = physical_size(physical_size((content_width, content_height), outer), mode);
            let area = Rect::new(content.rect.x, content.rect.y, width, height);
            self.place_absolute(&mut content, area, style.contains_absolute(), None);
        });
        // A table cell aligns its content across its lines, in its own block direction (as in
        // Chrome); its row gives it the length of its lines.
        if orthogonal && is_table_cell(style) {
            let free = (content_width - content.rect.height).clamp_non_negative();
            content.rect.y += match style.vertical_align {
                fagus_style::values::VerticalAlign::Middle => free / 2,
                fagus_style::values::VerticalAlign::Bottom => free,
                _ => Au::ZERO,
            };
        }
        // The content box on the page, and the content in its coordinates there.
        let area = physical_size((content_width, content_height), outer);
        to_physical(&mut content, mode, physical_size(area, mode), &self.tree.flows);
        if outer.is_vertical() {
            from_physical(&mut content, outer, area);
        }
        content.rect.x += model.border[LEFT] + model.padding[LEFT];
        content.rect.y += model.border[TOP] + model.padding[TOP];
        let width = content_width
            + model.border[LEFT]
            + model.border[RIGHT]
            + model.padding[LEFT]
            + model.padding[RIGHT];
        let height = content_height
            + model.border[TOP]
            + model.border[BOTTOM]
            + model.padding[TOP]
            + model.padding[BOTTOM];
        // It moves on to the next page as a whole, also from the top of this one where a margin
        // lies above it there in block flow (monolithic content, as in Chrome).
        let stays = if self.tree.flows.items.contains(&id) {
            space.at_top
        } else {
            self.at_page_top(&space)
        };
        if let Some(remaining) = space.remaining
            && height + model.margin[TOP] > remaining
            && !stays
        {
            return None;
        }
        // A box as wide as its content takes the room beside it in its margins: `auto` ones
        // share it, and in a right-to-left containing block the left one takes it.
        let mut margin_left = model.margin[LEFT];
        if orthogonal && block.is_none() {
            let free = space.containing_width - width;
            margin_left = match (style.margin.left.is_auto(), style.margin.right.is_auto()) {
                (true, true) => (free / 2).clamp_non_negative(),
                (true, false) => (free - model.margin[RIGHT]).clamp_non_negative(),
                (false, false) if space.rtl => free - model.margin[RIGHT],
                _ => margin_left,
            };
        }
        Some(Outcome::new(
            Fragment {
                rect: Rect::new(margin_left, Au::ZERO, width, height),
                kind: layout_box.fragment_kind(VisibleSides::ALL),
                children: vec![content].into(),
            },
            None,
            model.margin[TOP],
            model.margin[BOTTOM],
        ))
    }
}

/// Whether a style is a table cell's.
fn is_table_cell(style: &ComputedStyle) -> bool {
    style.display == Display::Internal(fagus_style::values::InternalDisplay::TableCell)
}

/// The width a flow root with size containment in its width takes from its content, which
/// counts as empty (CSS Containment 2, §3.1): its `contain-intrinsic-size`, else none. `None`
/// without such containment.
fn contained_width(style: &ComputedStyle) -> Option<Au> {
    style
        .size_containment()
        .0
        .then(|| Au::from_px(style.contain_intrinsic_size.0.unwrap_or(0.0)))
}

/// Whether a character in a style stands upright in a vertical line, rather than being set
/// sideways: in `text-orientation: mixed` the characters whose Unicode `Vertical_Orientation`
/// is `U`, `Tu` or `Tr` (UAX #50), the scripts written vertically, their punctuation and
/// symbols; the `vert` glyphs of the face take the place of those that change their form.
pub(crate) fn upright(character: char, style: &ComputedStyle) -> bool {
    use fagus_style::values::TextOrientation;
    if !style.is_vertical() || style.writing_mode.is_sideways() || style.text_combine_upright {
        return false;
    }
    match style.text_orientation {
        TextOrientation::Upright => true,
        TextOrientation::Sideways => false,
        TextOrientation::Mixed => {
            let code = u32::from(character);
            code >= 0xA7
                && UPRIGHT
                    .binary_search_by(|&(start, end)| {
                        if end < code {
                            std::cmp::Ordering::Less
                        } else if start > code {
                            std::cmp::Ordering::Greater
                        } else {
                            std::cmp::Ordering::Equal
                        }
                    })
                    .is_ok()
        }
    }
}

/// The ranges of characters whose `Vertical_Orientation` is `U`, `Tu` or `Tr`, from
/// `VerticalOrientation.txt` of Unicode 18 (unassigned code points as its defaults say), under
/// the Unicode License V3 (`UNICODE-LICENSE`).
const UPRIGHT: &[(u32, u32)] = &[
    (0x00A7, 0x00A7),
    (0x00A9, 0x00A9),
    (0x00AE, 0x00AE),
    (0x00B1, 0x00B1),
    (0x00BC, 0x00BE),
    (0x00D7, 0x00D7),
    (0x00F7, 0x00F7),
    (0x02EA, 0x02EB),
    (0x1100, 0x11FF),
    (0x1401, 0x167F),
    (0x18B0, 0x18FF),
    (0x2016, 0x2016),
    (0x2018, 0x2019),
    (0x201C, 0x201D),
    (0x2020, 0x2021),
    (0x2030, 0x2031),
    (0x203B, 0x203C),
    (0x2042, 0x2042),
    (0x2047, 0x2049),
    (0x2051, 0x2051),
    (0x2065, 0x2065),
    (0x20DD, 0x20E0),
    (0x20E2, 0x20E4),
    (0x2100, 0x2101),
    (0x2103, 0x2109),
    (0x210F, 0x210F),
    (0x2113, 0x2114),
    (0x2116, 0x2117),
    (0x211E, 0x2123),
    (0x2125, 0x2125),
    (0x2127, 0x2127),
    (0x2129, 0x2129),
    (0x212E, 0x212E),
    (0x2135, 0x213F),
    (0x2145, 0x214A),
    (0x214C, 0x214D),
    (0x214F, 0x2189),
    (0x218C, 0x218F),
    (0x221E, 0x221E),
    (0x2234, 0x2235),
    (0x2300, 0x2307),
    (0x230C, 0x231F),
    (0x2324, 0x232B),
    (0x237D, 0x239A),
    (0x23BE, 0x23CD),
    (0x23CF, 0x23CF),
    (0x23D1, 0x23DB),
    (0x23E2, 0x2422),
    (0x2424, 0x24FF),
    (0x25A0, 0x2619),
    (0x2620, 0x2767),
    (0x2776, 0x2793),
    (0x2B12, 0x2B2F),
    (0x2B50, 0x2B59),
    (0x2B97, 0x2B97),
    (0x2BB8, 0x2BD1),
    (0x2BD3, 0x2BEB),
    (0x2BF0, 0x2BFF),
    (0x2E50, 0x2E51),
    (0x2E80, 0xA4CF),
    (0xA960, 0xA97F),
    (0xAC00, 0xD7FF),
    (0xE000, 0xFAFF),
    (0xFE10, 0xFE1F),
    (0xFE30, 0xFE48),
    (0xFE50, 0xFE57),
    (0xFE59, 0xFE62),
    (0xFE67, 0xFE6F),
    (0xFF00, 0xFF0C),
    (0xFF0E, 0xFF1B),
    (0xFF1F, 0xFF60),
    (0xFFBF, 0xFFC1),
    (0xFFC8, 0xFFC9),
    (0xFFD0, 0xFFD1),
    (0xFFD8, 0xFFD9),
    (0xFFDD, 0xFFE7),
    (0xFFEF, 0xFFF8),
    (0xFFFC, 0xFFFD),
    (0x0001_0980, 0x0001_099F),
    (0x0001_1580, 0x0001_15FF),
    (0x0001_1A00, 0x0001_1ABF),
    (0x0001_3000, 0x0001_467F),
    (0x0001_6FE0, 0x0001_91DF),
    (0x0001_AFF0, 0x0001_B2FF),
    (0x0001_CEC0, 0x0001_CEDC),
    (0x0001_CEE0, 0x0001_CEF0),
    (0x0001_CEFE, 0x0001_CFCF),
    (0x0001_D000, 0x0001_D1FF),
    (0x0001_D250, 0x0001_D281),
    (0x0001_D2E0, 0x0001_D37F),
    (0x0001_D800, 0x0001_DAAF),
    (0x0001_F000, 0x0001_F7FF),
    (0x0001_F80C, 0x0001_F80F),
    (0x0001_F848, 0x0001_F84F),
    (0x0001_F85A, 0x0001_F85F),
    (0x0001_F888, 0x0001_F88F),
    (0x0001_F8AE, 0x0001_F8AF),
    (0x0001_F8BC, 0x0001_F8BF),
    (0x0001_F8C2, 0x0001_F8CF),
    (0x0001_F8D9, 0x0001_FAFF),
    (0x0002_0000, 0x0002_FFFD),
    (0x0003_0000, 0x0003_FFFD),
    (0x000F_0000, 0x000F_FFFD),
    (0x0010_0000, 0x0010_FFFD),
];

#[cfg(test)]
mod tests {
    use std::path::Path;

    use fagus_css::{MediaEnvironment, Origin, RuleSet, Stylesheet};
    use fagus_dom::Document;
    use fagus_style::values::TextOrientation;
    use fagus_text::FontDb;

    use super::*;
    use crate::fragment::{GlyphOrientation, Page, TextRun};

    #[test]
    fn turns_sides_back_to_the_physical_ones() {
        let sides = Sides {
            top: 1,
            right: 2,
            bottom: 3,
            left: 4,
        };
        for mode in [
            WritingMode::HorizontalTb,
            WritingMode::VerticalRl,
            WritingMode::VerticalLr,
            WritingMode::SidewaysRl,
            WritingMode::SidewaysLr,
        ] {
            assert_eq!(
                physical_sides(&logical_sides(&sides, mode), mode),
                sides,
                "{mode:?}"
            );
        }
    }

    fn render(html: &str) -> (Document, Vec<Page>) {
        let mut fonts = FontDb::new();
        fonts
            .add_font_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fonts"))
            .expect("test fonts");
        let document = Document::parse_html(html);
        let ua = Stylesheet::parse(fagus_style::USER_AGENT_CSS, Origin::UserAgent, "ua.css");
        let css: String = document
            .descendants(fagus_dom::NodeId::DOCUMENT)
            .filter(|&node| document.element(node).is_some_and(|data| data.is("style")))
            .map(|node| document.text_content(node))
            .collect();
        let author = Stylesheet::parse(&css, Origin::Author, "style");
        let environment = MediaEnvironment::default();
        let rules = RuleSet::new(&[&ua, &author], &environment);
        let styled = fagus_style::compute_styles(&document, &rules, (environment.width, environment.height));
        let pages = crate::layout(&document, &styled, &rules, &fonts, &crate::Images::default());
        (document, pages)
    }

    /// The border box of the element with id `id` on the page: x, y, width and height.
    fn rect(page: &Page, document: &Document, id: &str) -> Option<(f32, f32, f32, f32)> {
        fn walk(
            fragment: &Fragment,
            document: &Document,
            id: &str,
            origin: (f32, f32),
        ) -> Option<(f32, f32, f32, f32)> {
            let x = origin.0 + fragment.rect.x.to_px();
            let y = origin.1 + fragment.rect.y.to_px();
            if let FragmentKind::Box { node: Some(node), .. } = &fragment.kind
                && document.element(*node).and_then(|data| data.id.as_deref()) == Some(id)
            {
                return Some((x, y, fragment.rect.width.to_px(), fragment.rect.height.to_px()));
            }
            fragment
                .children
                .iter()
                .find_map(|child| walk(child, document, id, (x, y)))
        }
        page.fragments
            .iter()
            .find_map(|fragment| walk(fragment, document, id, (0.0, 0.0)))
    }

    /// A text run with its box on the page: x, y, width and height.
    type Placed = ((f32, f32, f32, f32), TextRun);

    /// The text runs of a page with their boxes on it.
    fn runs(page: &Page) -> Vec<Placed> {
        fn walk(fragment: &Fragment, origin: (f32, f32), out: &mut Vec<Placed>) {
            let x = origin.0 + fragment.rect.x.to_px();
            let y = origin.1 + fragment.rect.y.to_px();
            if let FragmentKind::Text(run) = &fragment.kind {
                out.push((
                    (x, y, fragment.rect.width.to_px(), fragment.rect.height.to_px()),
                    run.clone(),
                ));
            }
            for child in &fragment.children {
                walk(child, (x, y), out);
            }
        }
        let mut out = Vec::new();
        for fragment in &page.fragments {
            walk(fragment, (0.0, 0.0), &mut out);
        }
        out
    }

    #[test]
    fn sizes_orthogonal_boxes_by_their_content() {
        let (document, pages) = render(
            "<style>@page { size: 600px 800px; margin: 20px } body { margin: 0; font: 16px/20px Inter }
             .v { writing-mode: vertical-rl } .ib { display: inline-block; border: 2px solid; padding: 3px 5px 7px 11px }
             .b { height: 100px }</style>
             <p>Before <span id=a class='v ib'>Rotated</span> after</p>
             <div id=b class=v>Block</div>
             <div id=c class='v b'>A block that wraps its lines</div>",
        );
        let page = &pages[0];
        // The inline block is as wide as its line and as tall as its text, with its bottom
        // margin edge on the baseline of the line.
        let (_, y, width, height) = rect(page, &document, "a").expect("inline block");
        assert_eq!(width, 20.0 + 4.0 + 16.0);
        let text = runs(page)
            .into_iter()
            .find(|(_, run)| run.text == "Rotated")
            .expect("the text");
        assert_eq!(text.1.orientation, GlyphOrientation::Sideways);
        let length = text.0.3;
        assert!(length > 40.0 && length < 80.0, "{length}");
        assert_eq!(height, (length * 64.0).ceil() / 64.0 + 4.0 + 10.0);
        let after = runs(page)
            .into_iter()
            .find(|(_, run)| run.text.contains("after"))
            .expect("text after");
        assert_eq!(y + height, after.0.1 + after.1.baseline.to_px());
        // A block is as wide as its lines, its lines as long as the text where it fits.
        let (x, _, width, height) = rect(page, &document, "b").expect("block");
        assert_eq!((x, width), (20.0, 20.0));
        assert!(height < 60.0, "{height}");
        // A definite height is the length of the lines, which wrap.
        let (_, _, width, height) = rect(page, &document, "c").expect("wrapping block");
        assert_eq!(height, 100.0);
        assert_eq!(width % 20.0, 0.0);
        assert!(width >= 40.0, "{width}");
    }

    #[test]
    fn lays_out_vertical_documents_from_the_right() {
        let paragraph = "<p>Lines of text that run down the page and stack from the right to the left.</p>";
        let (document, pages) = render(&format!(
            "<style>@page {{ size: 400px 300px; margin: 0 }}
             html {{ writing-mode: vertical-rl; font: 16px/20px Inter }} body {{ margin: 0 }}
             p {{ margin: 0 0 0 10px }} #first {{ border-right: 2px solid }}</style>
             <p id=first>First</p>{}",
            paragraph.repeat(12)
        ));
        let page = &pages[0];
        let (x, y, width, height) = rect(page, &document, "first").expect("first paragraph");
        assert_eq!((x + width, y, width, height), (400.0, 0.0, 22.0, 300.0));
        // The text runs down from the top, turned.
        let first = runs(page)
            .into_iter()
            .find(|(_, run)| run.text == "First")
            .expect("text");
        assert_eq!(first.1.orientation, GlyphOrientation::Sideways);
        assert_eq!((first.0.0, first.0.1), (378.0, 0.0));
        // Pages break between the lines.
        assert!(pages.len() > 1, "{}", pages.len());
        let lefts: Vec<f32> = runs(page).iter().map(|(rect, _)| rect.0).collect();
        assert!(lefts.iter().all(|&left| left >= 0.0), "{lefts:?}");
    }

    #[test]
    fn stretches_cells_with_vertical_lines_to_their_row() {
        let (document, pages) = render(
            "<style>body { margin: 0; font: 14px/18px Inter } table { border-collapse: collapse }
             th { writing-mode: vertical-rl; padding: 0; text-align: left; vertical-align: bottom; width: 60px }
             td { height: 100px; padding: 0 }</style>
             <table><tr><th id=h>X</th><td>cell</td></tr></table>",
        );
        let page = &pages[0];
        let (x, y, width, height) = rect(page, &document, "h").expect("header");
        assert_eq!(height, 100.0);
        // The line starts at the top (`left`), at the block end of the cell (`bottom`): its
        // left.
        let text = runs(page)
            .into_iter()
            .find(|(_, run)| run.text == "X")
            .expect("text");
        assert!((text.0.1 - y).abs() < 2.0, "{text:?} {y}");
        assert!(text.0.0 < x + width / 2.0, "{text:?} {x}");
    }

    #[test]
    fn lays_out_vertical_tables_and_positioned_boxes_in_their_writing_mode() {
        let (document, pages) = render(
            "<style>body { margin: 0; font: 16px/20px Inter } .v { writing-mode: vertical-rl }
             td { padding: 0 } .abs { position: absolute; top: 5px; left: 7px }</style>
             <table class=v><tr><td id=a>One</td><td id=b>Two</td></tr></table>
             <div class=v id=box style='position: relative'>Text <span class=abs id=c>abs</span></div>",
        );
        let page = &pages[0];
        // The cells of a row run down, as the lines of the table do.
        let (a, b) = (
            rect(page, &document, "a").expect("first cell"),
            rect(page, &document, "b").expect("second cell"),
        );
        assert_eq!(a.0, b.0);
        assert!(b.1 >= a.1 + a.3, "{a:?} {b:?}");
        // An absolutely positioned box takes its insets from its containing block on the page.
        let container = rect(page, &document, "box").expect("container");
        let positioned = rect(page, &document, "c").expect("positioned box");
        assert_eq!(
            (positioned.0 - container.0, positioned.1 - container.1),
            (7.0, 5.0)
        );
    }

    #[test]
    fn places_positioned_boxes_of_vertical_content_in_other_containing_blocks() {
        let (document, pages) = render(
            "<style>body { margin: 0; font: 16px/20px Inter } .v { writing-mode: vertical-rl; height: 100px }
             .abs { position: absolute; width: 20px; height: 30px }</style>
             <div id=cb style='position: relative; padding: 5px'>
             <div class=v id=v>ab<span class=abs id=a></span>cd<div class=abs id=b style='top: 10px; left: 10px'></div></div>
             <div class=v id=w style='writing-mode: vertical-lr'>ab<span class=abs id=c></span></div></div>",
        );
        let page = &pages[0];
        let rect = |id| rect(page, &document, id).expect(id);
        let (block, rl, lr) = (rect("cb"), rect("v"), rect("w"));
        // They keep their size on the page and take their insets from the containing block.
        let inset = rect("b");
        assert_eq!(
            (inset.0 - block.0, inset.1 - block.1, inset.2, inset.3),
            (10.0, 10.0, 20.0, 30.0)
        );
        // Without insets, the corner of the block-start and the inline-start sides of the line
        // they are in: its top right in `vertical-rl`, its top left in `vertical-lr`.
        let right = rect("a");
        assert_eq!((right.2, right.3), (20.0, 30.0));
        assert!(
            (right.0 + right.2 - (rl.0 + rl.2)).abs() < 0.01,
            "{right:?} {rl:?}"
        );
        assert!(right.1 > rl.1, "{right:?} {rl:?}");
        let left = rect("c");
        assert!((left.0 - lr.0).abs() < 0.01 && left.1 > lr.1, "{left:?} {lr:?}");
        assert_eq!(
            static_ends(WritingMode::VerticalRl, false, WritingMode::HorizontalTb),
            (true, false)
        );
        assert_eq!(
            static_ends(WritingMode::VerticalLr, false, WritingMode::HorizontalTb),
            (false, false)
        );
        assert_eq!(
            static_ends(WritingMode::SidewaysLr, false, WritingMode::HorizontalTb),
            (false, true)
        );
        assert_eq!(
            static_ends(WritingMode::HorizontalTb, false, WritingMode::VerticalRl),
            (false, true)
        );
    }

    #[test]
    fn puts_ruby_over_vertical_lines_on_their_over_side() {
        let (document, pages) = render(
            "<style>body { margin: 0; font: 16px/40px Inter } div { writing-mode: vertical-lr }
             rt { display: ruby-text }</style>
             <div>a<ruby id=r>b<rt id=t>c</rt></ruby></div>",
        );
        let page = &pages[0];
        let (ruby, annotation) = (
            rect(page, &document, "r").expect("ruby"),
            rect(page, &document, "t").expect("annotation"),
        );
        // Over is the right in `vertical-lr` too.
        assert!(annotation.0 >= ruby.0 + ruby.2 / 2.0, "{ruby:?} {annotation:?}");
    }

    #[test]
    fn sets_combined_and_upright_text() {
        let (_, pages) = render(
            "<style>body { margin: 0; font: 16px/24px Inter } div { writing-mode: vertical-rl }
             .up { text-orientation: upright }</style>
             <div>a<span style='text-combine-upright: all'>12</span>b</div><div class=up>AB</div>",
        );
        let runs = runs(&pages[0]);
        let combined = runs
            .iter()
            .find(|(_, run)| run.text == "12")
            .expect("combined text");
        assert_eq!(combined.1.orientation, GlyphOrientation::Combined);
        assert_eq!(combined.0.3, 16.0);
        // Upright glyphs of a face without vertical metrics advance by its ascent and descent.
        let upright = runs
            .iter()
            .find(|(_, run)| run.text == "AB")
            .expect("upright text");
        assert_eq!(upright.1.orientation, GlyphOrientation::Upright);
        assert!(
            upright
                .1
                .glyphs
                .iter()
                .all(|glyph| (glyph.advance - 20.0).abs() < 0.01),
            "{upright:?}"
        );
    }

    #[test]
    fn sets_combined_text_without_letter_spacing() {
        let (_, pages) = render(
            "<style>body { margin: 0; font: 16px/24px Inter } div { writing-mode: vertical-rl }
             </style><div style='letter-spacing: 2px; word-spacing: 3px'>a<span
             style='text-combine-upright: all'>1 2</span>b</div><div>a<span
             style='text-combine-upright: all'>1 2</span>b</div>",
        );
        let runs = runs(&pages[0]);
        let advances: Vec<f32> = runs
            .iter()
            .filter(|(_, run)| run.text == "1 2")
            .map(|(_, run)| run.glyphs.iter().map(|glyph| glyph.advance).sum())
            .collect();
        // As in Chrome, which sets no letter or word spacing in combined text.
        assert_eq!(advances.len(), 2);
        assert!((advances[0] - advances[1]).abs() < 0.01, "{advances:?}");
    }

    #[test]
    fn turns_rectangles_to_the_page_and_back() {
        let rect = Rect::new(
            Au::from_px(3.0),
            Au::from_px(5.0),
            Au::from_px(7.0),
            Au::from_px(11.0),
        );
        let parent = (Au::from_px(40.0), Au::from_px(30.0));
        for mode in [
            WritingMode::HorizontalTb,
            WritingMode::VerticalRl,
            WritingMode::VerticalLr,
            WritingMode::SidewaysRl,
            WritingMode::SidewaysLr,
        ] {
            for in_line in [false, true] {
                let physical = physical_rect(rect, mode, parent, in_line);
                let back = logical_rect(physical, mode, physical_size(parent, mode), in_line);
                assert_eq!(back, rect, "{mode:?} {in_line}");
            }
        }
        // In `vertical-rl` the block-start side is the right one.
        let physical = physical_rect(rect, WritingMode::VerticalRl, parent, false);
        assert_eq!(
            physical,
            Rect::new(
                Au::from_px(14.0),
                Au::from_px(3.0),
                Au::from_px(11.0),
                Au::from_px(7.0)
            )
        );
    }

    #[test]
    fn turns_styles_and_finds_upright_characters() {
        let mut style = ComputedStyle::from(ComputedValues {
            writing_mode: WritingMode::VerticalRl,
            width: Size::Length(LengthPercentage::Px(10.0)),
            ..ComputedValues::default()
        });
        style.margin.right = fagus_style::values::LengthPercentageAuto::Length(LengthPercentage::Px(4.0));
        let turned = logical_style(&style, WritingMode::VerticalRl);
        assert_eq!(turned.height, Size::Length(LengthPercentage::Px(10.0)));
        assert_eq!(turned.margin.top.resolve_or(0.0, -1.0), 4.0);
        assert!(upright('漢', &style) && upright('。', &style) && upright('ア', &style));
        assert!(!upright('A', &style) && !upright('1', &style));
        style.text_orientation = TextOrientation::Upright;
        assert!(upright('A', &style));
        style.writing_mode = WritingMode::SidewaysRl;
        assert!(!upright('漢', &style));
        assert!(!upright('漢', &ComputedStyle::default()));
    }
}

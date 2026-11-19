//! Glyph outlines from Apple's `hvgl` table, in which PingFang (macOS' Chinese font, in
//! `PingFangUI.ttc`) keeps its glyphs instead of `glyf` or `CFF`.
//!
//! A glyph is a tree of parts: shapes (closed paths of quadratic segments, with a delta for
//! every coordinate per end of every axis) and composites (parts placed with transforms that,
//! like the axis values of their subparts, move with the axes). The layout of the table and
//! the way outlines are made follow `HarfBuzz`'s `hb-aat-var-hvgl-table.hh` and
//! `hb-aat-var-hvgl-table.cc` (commit 3a64fce), under `HarfBuzz`'s "Old MIT" license
//! (`HARFBUZZ-LICENSE`); the table is little-endian, unlike other OpenType tables.

use skrifa::outline::OutlinePen;

/// The most parts a glyph's tree may draw, and how deep it may nest, against loops in broken
/// tables (`HarfBuzz`'s `HB_MAX_GRAPH_EDGE_COUNT` and `HB_MAX_NESTING_LEVEL`).
const MAX_EDGES: u32 = 2048;
const MAX_DEPTH: u32 = 64;

/// How a segment's on-curve point follows from the points around it.
const BLEND_CURVE: u8 = 0;
const BLEND_TANGENT: u8 = 2;
const BLEND_TANGENT_PAIR_FIRST: u8 = 3;

/// An `hvgl` table.
#[derive(Clone, Copy)]
pub(crate) struct Hvgl<'a> {
    data: &'a [u8],
    part_count: u32,
    /// Where the index of the parts starts.
    parts: usize,
    glyph_count: u32,
}

/// An affine transform, as `HarfBuzz`'s `hb_transform_t`: x' = xx·x + xy·y + x0,
/// y' = yx·x + yy·y + y0.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Transform {
    xx: f64,
    yx: f64,
    xy: f64,
    yy: f64,
    x0: f64,
    y0: f64,
}

impl Default for Transform {
    fn default() -> Self {
        Self::scale(1.0)
    }
}

impl Transform {
    fn scale(scale: f64) -> Self {
        Self {
            xx: scale,
            yx: 0.0,
            xy: 0.0,
            yy: scale,
            x0: 0.0,
            y0: 0.0,
        }
    }

    #[allow(clippy::float_cmp, reason = "exactly the identity, as HarfBuzz tests it")]
    fn is_translation(&self) -> bool {
        self.xx == 1.0 && self.yx == 0.0 && self.xy == 0.0 && self.yy == 1.0
    }

    /// This transform followed by `other` (`before`), or `other` followed by this one.
    fn multiply(&mut self, other: &Self, before: bool) {
        let (a, b) = if before { (*other, *self) } else { (*self, *other) };
        *self = Self {
            xx: a.xx * b.xx + a.xy * b.yx,
            yx: a.yx * b.xx + a.yy * b.yx,
            xy: a.xx * b.xy + a.xy * b.yy,
            yy: a.yx * b.xy + a.yy * b.yy,
            x0: a.xx * b.x0 + a.xy * b.y0 + a.x0,
            y0: a.yx * b.x0 + a.yy * b.y0 + a.y0,
        };
    }

    /// Moves by (x, y): after this transform (`before`), or before it.
    fn translate(&mut self, x: f64, y: f64, before: bool) {
        if before {
            self.x0 += x;
            self.y0 += y;
        } else {
            self.x0 += self.xx * x + self.xy * y;
            self.y0 += self.yx * x + self.yy * y;
        }
    }

    /// Turns by `angle` radians around the origin.
    fn rotate(&mut self, angle: f64, before: bool) {
        if angle != 0.0 {
            let (sin, cos) = libm::sincos(angle);
            self.multiply(
                &Self {
                    xx: cos,
                    yx: sin,
                    xy: -sin,
                    yy: cos,
                    x0: 0.0,
                    y0: 0.0,
                },
                before,
            );
        }
    }

    /// Turns by `angle` radians around a point.
    fn rotate_around(&mut self, angle: f64, (x, y): (f64, f64)) {
        if angle != 0.0 {
            let (sin, cos) = libm::sincos(angle);
            self.multiply(
                &Self {
                    xx: cos,
                    yx: sin,
                    xy: -sin,
                    yy: cos,
                    x0: (1.0 - cos) * x + sin * y,
                    y0: -sin * x + (1.0 - cos) * y,
                },
                false,
            );
        }
    }

    fn apply(&self, x: f64, y: f64) -> (f32, f32) {
        (
            (self.x0 + self.xx * x + self.xy * y) as f32,
            (self.y0 + self.yx * x + self.yy * y) as f32,
        )
    }
}

fn u16_at(data: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(offset..offset + 2)?.try_into().ok()?))
}

fn u32_at(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(offset..offset + 4)?.try_into().ok()?))
}

fn f32_at(data: &[u8], offset: usize) -> Option<f64> {
    Some(f64::from(f32::from_le_bytes(
        data.get(offset..offset + 4)?.try_into().ok()?,
    )))
}

fn f64_at(data: &[u8], offset: usize) -> Option<f64> {
    Some(f64::from_le_bytes(data.get(offset..offset + 8)?.try_into().ok()?))
}

/// The limits on drawing one glyph.
struct Budget {
    edges: u32,
    nodes: u32,
    depth: u32,
}

/// The header of a composite part, with the offsets of its arrays from the part's start.
struct Composite {
    axis_count: usize,
    sub_part_count: usize,
    total_parts: usize,
    total_axes: usize,
    master_axis_values: usize,
    extremum_axis_values: usize,
    master_translations: usize,
    master_rotations: usize,
    extremum_translations: usize,
    extremum_rotations: usize,
    sub_parts: usize,
    column_starts: usize,
    master_axis_deltas: usize,
    extremum_axis_deltas: usize,
    translations: usize,
    rotations: usize,
}

impl Composite {
    fn read(part: &[u8]) -> Option<Self> {
        let field = |index: usize| u16_at(part, 2 * index).map(usize::from);
        Some(Self {
            axis_count: field(1)?,
            sub_part_count: field(2)?,
            total_parts: field(3)?,
            total_axes: field(4)?,
            master_axis_values: field(6)?,
            extremum_axis_values: field(7)?,
            master_translations: field(8)?,
            master_rotations: field(9)?,
            extremum_translations: field(10)?,
            extremum_rotations: field(11)?,
            sub_parts: field(12)? * 4,
            column_starts: field(13)? * 4,
            master_axis_deltas: field(14)? * 4,
            extremum_axis_deltas: field(15)? * 4,
            translations: field(16)? * 4,
            rotations: field(17)? * 4,
        })
    }
}

/// The sizes of the trees below a part: how many axis values and transforms it takes.
fn totals(part: &[u8]) -> Option<(usize, usize)> {
    if u16_at(part, 0)? & 1 == 0 {
        Some((usize::from(u16_at(part, 2)?), 1))
    } else {
        let composite = Composite::read(part)?;
        Some((composite.total_axes, composite.total_parts))
    }
}

impl<'a> Hvgl<'a> {
    /// The table in `data`, if it is one of the version (3.1 or later) that this reads.
    pub(crate) fn new(data: &'a [u8]) -> Option<Self> {
        let major = u16_at(data, 0)?;
        let minor = u16_at(data, 2)?;
        if major != 3 || minor < 1 {
            return None;
        }
        let part_count = u32_at(data, 8)?;
        let parts = u32_at(data, 12)? as usize;
        let glyph_count = u32_at(data, 16)?;
        // The index holds an offset per part and one past the last.
        let index_end = parts.checked_add((part_count as usize).checked_add(1)?.checked_mul(4)?)?;
        (index_end <= data.len()).then_some(Self {
            data,
            part_count,
            parts,
            glyph_count,
        })
    }

    /// The data of part `index`.
    fn part(&self, index: u32) -> Option<&'a [u8]> {
        if index >= self.part_count {
            return None;
        }
        let at =
            |index: u32| u32_at(self.data, self.parts + 4 * index as usize).map(|offset| offset as usize);
        let (start, end) = (at(index)?, at(index + 1)?);
        if end < start {
            return None;
        }
        self.data.get(self.parts + start..self.parts + end)
    }

    /// Draws the outline of `glyph` at normalized axis coordinates `coordinates` (in the order of
    /// the font's axes, `avar` applied), in font units times `scale`. False if the table has no
    /// such glyph.
    pub(crate) fn draw(
        &self,
        glyph: u32,
        coordinates: &[f64],
        scale: f64,
        pen: &mut impl OutlinePen,
    ) -> bool {
        if glyph >= self.glyph_count {
            return false;
        }
        let Some(part) = self.part(glyph) else {
            return false;
        };
        let Some((total_axes, total_parts)) = totals(part) else {
            return false;
        };
        let mut axes = vec![0.0; total_axes];
        for (axis, value) in axes.iter_mut().zip(coordinates) {
            *axis = *value;
        }
        let mut transforms = vec![Transform::default(); total_parts.max(1)];
        transforms[0] = Transform::scale(scale);
        let mut budget = Budget {
            edges: MAX_EDGES,
            nodes: total_parts as u32,
            depth: MAX_DEPTH,
        };
        self.draw_part(glyph, &mut axes, &mut transforms, &mut budget, pen);
        true
    }

    fn draw_part(
        &self,
        index: u32,
        axes: &mut [f64],
        transforms: &mut [Transform],
        budget: &mut Budget,
        pen: &mut impl OutlinePen,
    ) {
        if budget.edges == 0 || budget.nodes == 0 || budget.depth == 0 {
            return;
        }
        budget.edges -= 1;
        budget.nodes -= 1;
        budget.depth -= 1;
        if let Some(part) = self.part(index)
            && let Some(flags) = u16_at(part, 0)
        {
            if flags & 1 == 0 {
                if let Some(transform) = transforms.first() {
                    draw_shape(part, axes, transform, pen);
                }
            } else if let Some(composite) = Composite::read(part) {
                self.draw_composite(part, &composite, axes, transforms, budget, pen);
            }
        }
        budget.depth += 1;
    }

    fn draw_composite(
        &self,
        part: &[u8],
        composite: &Composite,
        axes: &mut [f64],
        transforms: &mut [Transform],
        budget: &mut Budget,
        pen: &mut impl OutlinePen,
    ) {
        let axes_len = composite.total_axes.min(axes.len());
        let (head, tail) = axes[..axes_len].split_at_mut(composite.axis_count.min(axes_len));
        apply_to_axes(part, composite, tail, head);
        let transforms_len = composite.total_parts.min(transforms.len());
        let Some((own, below)) = transforms[..transforms_len].split_first_mut() else {
            return;
        };
        let own = *own;
        apply_to_transforms(part, composite, below, head);
        for number in 0..composite.sub_part_count {
            let at = composite.sub_parts + 8 * number;
            let (Some(sub_part), Some(transform_index), Some(axis_index)) =
                (u32_at(part, at), u16_at(part, at + 4), u16_at(part, at + 6))
            else {
                return;
            };
            let (transform_index, axis_index) = (usize::from(transform_index), usize::from(axis_index));
            let Some(transform) = below.get_mut(transform_index) else {
                continue;
            };
            if transform.is_translation() {
                let (x, y) = (transform.x0, transform.y0);
                *transform = own;
                transform.translate(x, y, false);
            } else {
                transform.multiply(&own, true);
            }
            let sub_axes = tail.get_mut(axis_index..).unwrap_or_default();
            self.draw_part(sub_part, sub_axes, &mut below[transform_index..], budget, pen);
        }
    }
}

/// Moves the projection of `point` onto the line from `from` to `to`, within it.
fn project(point: (f64, f64), from: (f64, f64), to: (f64, f64)) -> (f64, f64) {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let length = dx * dx + dy * dy;
    let t = if length == 0.0 {
        0.0
    } else {
        ((dx * (point.0 - from.0) + dy * (point.1 - from.1)) / length).clamp(0.0, 1.0)
    };
    (from.0 + dx * t, from.1 + dy * t)
}

/// Draws a shape part: its master coordinates moved by the deltas of the axes, the on-curve
/// points then made to follow from the off-curve ones by their blend types.
fn draw_shape(part: &[u8], axes: &[f64], transform: &Transform, pen: &mut impl OutlinePen) {
    let field = |index: usize| u16_at(part, 2 * index).map(usize::from);
    let (Some(axis_count), Some(path_count), Some(segment_count)) = (field(1), field(2), field(3)) else {
        return;
    };
    let blend_types = 8 + 2 * path_count;
    let coordinates = (blend_types + segment_count).next_multiple_of(8);
    let rows = 4 * segment_count;
    let Some(blend) = part.get(blend_types..blend_types + segment_count) else {
        return;
    };
    // Every segment: an on-curve point (x, y), then an off-curve one.
    let Some(mut points) = (0..rows)
        .map(|row| f64_at(part, coordinates + 8 * row))
        .collect::<Option<Vec<f64>>>()
    else {
        return;
    };
    // The deltas: a column of a value per row for each end of each axis.
    let deltas = coordinates + 8 * rows;
    for (axis, value) in axes.iter().take(axis_count).enumerate() {
        if *value == 0.0 {
            continue;
        }
        let column = axis * 2 + usize::from(*value > 0.0);
        let start = deltas + 8 * rows * column;
        let scalar = value.abs();
        for (row, point) in points.iter_mut().enumerate() {
            let Some(delta) = f64_at(part, start + 8 * row) else {
                return;
            };
            *point += delta * scalar;
        }
    }

    let mut start = 0;
    for path in 0..path_count {
        let Some(count) = field(4 + path) else { return };
        let end = start + count;
        if end > segment_count {
            break;
        }
        if count == 0 {
            continue;
        }
        let on = |points: &[f64], i: usize| (points[4 * i], points[4 * i + 1]);
        let off = |points: &[f64], i: usize| (points[4 * i + 2], points[4 * i + 3]);
        let mut previous = end - 1;
        for i in start..end {
            match blend[i] {
                BLEND_CURVE => {
                    // The on-curve x holds where between the off-curve points the point lies.
                    let t = points[4 * i].clamp(0.0, 1.0);
                    let (from, to) = (off(&points, previous), off(&points, i));
                    points[4 * i] = from.0 + (to.0 - from.0) * t;
                    points[4 * i + 1] = from.1 + (to.1 - from.1) * t;
                }
                BLEND_TANGENT => {
                    let point = project(on(&points, i), off(&points, previous), off(&points, i));
                    (points[4 * i], points[4 * i + 1]) = point;
                }
                BLEND_TANGENT_PAIR_FIRST => {
                    let next = if i == end - 1 { start } else { i + 1 };
                    let from = off(&points, previous);
                    let point = project(on(&points, i), from, off(&points, next));
                    (points[4 * i], points[4 * i + 1]) = point;
                    let point = project(on(&points, next), from, off(&points, next));
                    (points[4 * next], points[4 * next + 1]) = point;
                }
                // Corners, and the second points of tangent pairs (set with the first).
                _ => {}
            }
            previous = i;
        }
        let (x, y) = on(&points, start);
        let (x, y) = transform.apply(x, y);
        pen.move_to(x, y);
        for i in start..end {
            let next = if i == end - 1 { start } else { i + 1 };
            let (cx, cy) = off(&points, i);
            let (x, y) = on(&points, next);
            let (cx, cy) = transform.apply(cx, cy);
            let (x, y) = transform.apply(x, y);
            pen.quad_to(cx, cy, x, y);
        }
        pen.close();
        start = end;
    }
}

/// Adds a composite's deltas to the axis values of its subparts (`tail`), by its own axis
/// values (`head`).
fn apply_to_axes(part: &[u8], composite: &Composite, tail: &mut [f64], head: &[f64]) {
    let column_starts = composite.column_starts;
    let master_rows = column_starts + 2 * (2 * composite.axis_count + 1);
    let extremum_rows = master_rows + 2 * composite.master_axis_values;
    let mut add = |row: Option<u16>, delta: Option<f64>| {
        if let (Some(row), Some(delta)) = (row, delta)
            && let Some(value) = tail.get_mut(usize::from(row))
        {
            *value += delta;
        }
    };
    for i in 0..composite.master_axis_values {
        add(
            u16_at(part, master_rows + 2 * i),
            f32_at(part, composite.master_axis_deltas + 4 * i),
        );
    }
    for (axis, value) in head.iter().enumerate().take(composite.axis_count) {
        if *value == 0.0 {
            continue;
        }
        let column = axis * 2 + usize::from(*value > 0.0);
        let (Some(first), Some(last)) = (
            u16_at(part, column_starts + 2 * column),
            u16_at(part, column_starts + 2 * (column + 1)),
        ) else {
            continue;
        };
        let last = usize::from(last).min(composite.extremum_axis_values);
        for i in usize::from(first)..last {
            add(
                u16_at(part, extremum_rows + 2 * i),
                f32_at(part, composite.extremum_axis_deltas + 4 * i).map(|delta| delta * value.abs()),
            );
        }
    }
}

/// A row and column of a composite's matrices of transforms.
#[derive(Clone, Copy)]
struct Cell {
    row: usize,
    column: usize,
}

fn cell_at(part: &[u8], offset: usize) -> Option<Cell> {
    Some(Cell {
        row: usize::from(u16_at(part, offset)?),
        column: usize::from(u16_at(part, offset + 2)?),
    })
}

/// Sets the transforms of a composite's subparts (`transforms`, the tree below it) by its axis
/// values (`axes`): the moves and turns at the ends of the axes, scaled by the values, then
/// the master turns and moves.
fn apply_to_transforms(part: &[u8], composite: &Composite, transforms: &mut [Transform], axes: &[f64]) {
    let translations = composite.translations;
    let master_translation_count = composite.master_translations;
    let extremum_translation_count = composite.extremum_translations;
    // Master deltas, extremum deltas, extremum cells, master rows.
    let extremum_translations = translations + 8 * master_translation_count;
    let extremum_translation_cells = extremum_translations + 8 * extremum_translation_count;
    let master_translation_rows = extremum_translation_cells + 4 * extremum_translation_count;
    let rotations = composite.rotations;
    let master_rotation_count = composite.master_rotations;
    let extremum_rotation_count = composite.extremum_rotations;
    let extremum_rotations = rotations + 4 * master_rotation_count;
    let extremum_rotation_cells = extremum_rotations + 4 * extremum_rotation_count;
    let master_rotation_rows = extremum_rotation_cells + 4 * extremum_rotation_count;

    let translation = |i: usize| -> (f64, f64) {
        (
            f32_at(part, extremum_translations + 8 * i).unwrap_or(0.0),
            f32_at(part, extremum_translations + 8 * i + 4).unwrap_or(0.0),
        )
    };
    // The scalar of an axis end (column) at the axis values: none if the axis is at its
    // default or towards its other end.
    let scalar = |column: usize| -> Option<f64> {
        let value = *axes.get(column / 2)?;
        (value != 0.0 && (column & 1 == 1) == (value > 0.0)).then_some(value.abs())
    };

    if extremum_rotation_count == 0 {
        for i in 0..extremum_translation_count {
            let Some(cell) = cell_at(part, extremum_translation_cells + 4 * i) else {
                break;
            };
            let Some(scalar) = scalar(cell.column) else {
                continue;
            };
            let Some(transform) = transforms.get_mut(cell.row) else {
                break;
            };
            let (x, y) = translation(i);
            transform.translate(x * scalar, y * scalar, true);
        }
    } else {
        // The moves and turns of each row in the order of their columns, the two lists
        // walked together.
        let (mut t, mut r) = (0, 0);
        let translation_cell = |t: usize| {
            (t < extremum_translation_count)
                .then(|| cell_at(part, extremum_translation_cells + 4 * t))
                .flatten()
        };
        let rotation_cell = |r: usize| {
            (r < extremum_rotation_count)
                .then(|| cell_at(part, extremum_rotation_cells + 4 * r))
                .flatten()
        };
        loop {
            let (next_translation, next_rotation) = (translation_cell(t), rotation_cell(r));
            let row = next_translation
                .map(|cell| cell.row)
                .into_iter()
                .chain(next_rotation.map(|cell| cell.row))
                .min();
            let Some(row) = row.filter(|row| *row < transforms.len()) else {
                break;
            };
            let mut transform = Transform::default();
            let mut translate_only = true;
            let mut consumed = false;
            loop {
                let row_translation = translation_cell(t).filter(|cell| cell.row == row);
                let row_rotation = rotation_cell(r).filter(|cell| cell.row == row);
                let column = row_translation
                    .map(|cell| cell.column)
                    .into_iter()
                    .chain(row_rotation.map(|cell| cell.column))
                    .min();
                let Some(column) = column.filter(|column| *column < 2 * composite.axis_count) else {
                    break;
                };
                let mut moved = (0.0, 0.0);
                let mut angle = 0.0;
                if row_translation.is_some_and(|cell| cell.column == column) {
                    moved = translation(t);
                    t += 1;
                    consumed = true;
                }
                if row_rotation.is_some_and(|cell| cell.column == column) {
                    angle = f32_at(part, extremum_rotations + 4 * r).unwrap_or(0.0);
                    r += 1;
                    consumed = true;
                }
                let Some(scalar) = scalar(column) else {
                    continue;
                };
                if angle == 0.0 {
                    transform.translate(moved.0 * scalar, moved.1 * scalar, translate_only);
                } else {
                    // The move of a turn is its centre's: the centre of the turn by the whole
                    // angle that moves the origin by it.
                    let (mut x, mut y) = moved;
                    if x != 0.0 || y != 0.0 {
                        let (sin, cos) = libm::sincos(angle);
                        if 1.0 - cos != 0.0 {
                            let ratio = sin / (1.0 - cos);
                            (x, y) = (f64::midpoint(x, -y * ratio), f64::midpoint(y, x * ratio));
                        }
                    }
                    transform.rotate_around(angle * scalar, (x, y));
                    translate_only = false;
                }
            }
            // Cells outside the matrix would stop the walk forever.
            if !consumed {
                break;
            }
            if translate_only {
                transforms[row].translate(transform.x0, transform.y0, true);
            } else {
                transforms[row].multiply(&transform, true);
            }
        }
    }

    for i in 0..master_rotation_count {
        let (Some(row), Some(angle)) = (
            u16_at(part, master_rotation_rows + 2 * i),
            f32_at(part, rotations + 4 * i),
        ) else {
            break;
        };
        let Some(transform) = transforms.get_mut(usize::from(row)) else {
            break;
        };
        transform.rotate(angle, true);
    }
    for i in 0..master_translation_count {
        let (Some(row), Some(x), Some(y)) = (
            u16_at(part, master_translation_rows + 2 * i),
            f32_at(part, translations + 8 * i),
            f32_at(part, translations + 8 * i + 4),
        ) else {
            break;
        };
        let Some(transform) = transforms.get_mut(usize::from(row)) else {
            break;
        };
        transform.translate(x, y, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Collects what a pen is given, rounded to thousandths.
    #[derive(Default)]
    struct Recorder(Vec<String>);

    impl OutlinePen for Recorder {
        fn move_to(&mut self, x: f32, y: f32) {
            self.0.push(format!("M {x} {y}"));
        }
        fn line_to(&mut self, x: f32, y: f32) {
            self.0.push(format!("L {x} {y}"));
        }
        fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
            self.0.push(format!("Q {cx} {cy} {x} {y}"));
        }
        fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {
            self.0.push("C".to_owned());
        }
        fn close(&mut self) {
            self.0.push("Z".to_owned());
        }
    }

    /// A shape part with one axis: a square of 100 units with its off-curve points halfway
    /// along its sides, which the axis moves 10 units right at its maximum. `blend` gives the
    /// segments' blend types; a curve segment's on-curve x holds 0.5.
    fn square(blend: [u8; 4]) -> Vec<u8> {
        let mut part = Vec::new();
        for value in [0u16, 1, 1, 4, 4] {
            part.extend(value.to_le_bytes());
        }
        part.extend(blend);
        part.resize(part.len().next_multiple_of(8), 0);
        let segments = [
            [0.0, 0.0, 50.0, 0.0],
            [100.0, 0.0, 100.0, 50.0],
            [100.0, 100.0, 50.0, 100.0],
            [0.0, 100.0, 0.0, 50.0],
        ];
        for (segment, kind) in segments.iter().zip(blend) {
            let mut segment = *segment;
            if kind == BLEND_CURVE {
                segment[0] = 0.5;
            }
            for value in segment {
                part.extend(f64::to_le_bytes(value));
            }
        }
        // The column of the axis' minimum, then that of its maximum: x moves right.
        for column in 0..2 {
            for row in 0..16 {
                let delta = if column == 1 && row % 2 == 0 { 10.0 } else { 0.0 };
                part.extend(f64::to_le_bytes(delta));
            }
        }
        part
    }

    /// A composite of one axis that places a part: 200 units right, and up by 30 units at the
    /// axis' maximum, which also takes the part's axis along.
    fn composite() -> Vec<u8> {
        let mut part = Vec::new();
        // Flags, axes, subparts, parts and axes of the tree, extremes, then the counts of the
        // sparse master and extremum axis values, master translations and rotations, and
        // extremum translations and rotations; then the offsets (in fours of bytes).
        for value in [1u16, 1, 1, 2, 2, 2, 0, 1, 1, 0, 1, 0, 9, 11, 13, 13, 14, 20] {
            part.extend(value.to_le_bytes());
        }
        // The subpart: part 0, its transform and axis values first in the tree.
        part.extend(0u32.to_le_bytes());
        part.extend([0; 4]);
        // Extremum column starts (minimum: none, maximum: row 0), extremum row 0.
        for value in [0u16, 0, 1, 0] {
            part.extend(value.to_le_bytes());
        }
        // The extremum axis delta.
        part.extend(1.0f32.to_le_bytes());
        // Master translation, extremum translation, its cell (row 0, column 1), master row 0.
        for value in [200.0f32, 0.0, 0.0, 30.0] {
            part.extend(value.to_le_bytes());
        }
        for value in [0u16, 1, 0, 0] {
            part.extend(value.to_le_bytes());
        }
        part
    }

    fn table(parts: &[Vec<u8>]) -> Vec<u8> {
        let mut table = Vec::new();
        for value in [3u16, 1] {
            table.extend(value.to_le_bytes());
        }
        let count = parts.len() as u32;
        for value in [0, count, 24, count, 0] {
            table.extend(u32::to_le_bytes(value));
        }
        let mut offset = 4 * (count + 1);
        for part in parts {
            table.extend(offset.to_le_bytes());
            offset += part.len() as u32;
        }
        table.extend(offset.to_le_bytes());
        for part in parts {
            table.extend(part);
        }
        table
    }

    fn draw(table: &[u8], glyph: u32, coordinates: &[f64]) -> Vec<String> {
        let mut recorder = Recorder::default();
        assert!(
            Hvgl::new(table)
                .expect("a table")
                .draw(glyph, coordinates, 1.0, &mut recorder)
        );
        recorder.0
    }

    #[test]
    fn draws_shapes_moved_by_their_axes() {
        let data = table(&[square([1; 4])]);
        assert_eq!(
            draw(&data, 0, &[0.0]),
            [
                "M 0 0",
                "Q 50 0 100 0",
                "Q 100 50 100 100",
                "Q 50 100 0 100",
                "Q 0 50 0 0",
                "Z"
            ]
        );
        assert_eq!(draw(&data, 0, &[0.5])[..2], ["M 5 0", "Q 55 0 105 0"]);
        // The minimum's deltas are zero.
        assert_eq!(draw(&data, 0, &[-1.0])[0], "M 0 0");
    }

    #[test]
    fn places_on_curve_points_by_their_blend_types() {
        // A curve point halfway between the off-curve points around it; a tangent point
        // on the line through them.
        let data = table(&[square([1, BLEND_CURVE, BLEND_TANGENT, 1])]);
        let drawn = draw(&data, 0, &[0.0]);
        assert_eq!(drawn[1], "Q 50 0 75 25");
        assert_eq!(drawn[2], "Q 100 50 75 75");
    }

    #[test]
    fn places_composite_parts_by_their_transforms() {
        let data = table(&[square([1; 4]), composite()]);
        assert_eq!(draw(&data, 1, &[0.0])[..2], ["M 200 0", "Q 250 0 300 0"]);
        // At half the maximum, the part moves up by 15 and its own axis to 0.5.
        assert_eq!(draw(&data, 1, &[0.5])[..2], ["M 205 15", "Q 255 15 305 15"]);
        assert_eq!(draw(&data, 1, &[-1.0])[0], "M 200 0");
    }

    #[test]
    fn rejects_other_versions_and_broken_tables() {
        let mut data = table(&[square([1; 4])]);
        assert!(Hvgl::new(&data[..20]).is_none(), "too short");
        assert!(
            !Hvgl::new(&data)
                .expect("a table")
                .draw(1, &[], 1.0, &mut Recorder::default())
        );
        // A part cut short draws nothing, without panicking.
        let short = &data[..data.len() - 100];
        let mut recorder = Recorder::default();
        if let Some(table) = Hvgl::new(short) {
            table.draw(0, &[1.0], 1.0, &mut recorder);
        }
        assert_eq!(recorder.0, Vec::<String>::new());
        data[0] = 2;
        assert!(Hvgl::new(&data).is_none(), "version 2");
    }
}

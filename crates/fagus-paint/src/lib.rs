//! Display lists for Fagus.
//!
//! [`paint`] turns the fragments of laid-out pages into a flat list of drawing operations per page,
//! in painting order and in CSS pixels. The PDF writer (and later raster previews) only consume
//! display lists, so they know nothing about CSS.

#![forbid(unsafe_code)]

mod background;
mod border_image;
mod border_shape;
mod carried;
mod clip_margin;
mod color;
mod corner;
mod cull;
pub use cull::invisible_opacity;
pub use fields::{Lift, Widget};
mod document;
#[cfg(test)]
mod effects_tests;
mod fields;
mod filter;
mod mask;
mod reach;
mod reference;
mod shadow;
mod stitched;
mod svg_clip;
mod svg_mask;
mod transform;
pub use svg_clip::svg_clip_path;
pub use svg_mask::{SvgBoxes, svg_mask};
pub use transform::svg_matrix;

use std::sync::Arc;

use fagus_dom::NodeId;
use fagus_layout::units::Au;
use fagus_layout::{Fragment, FragmentKind, Glyph, Page};
use fagus_style::values::{
    AnnotationSide, BorderStyle, DecorationThickness, ObjectFit, OutlineStyle, Rgba, RubyDisplay, SkipInk,
    TextDecorationStyle, Visibility,
};
use fagus_style::{ComputedStyle, ComputedValues};
use fagus_text::{FaceId, FontDb};
use rustc_hash::FxHashMap;

/// A rectangle in CSS pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectF {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Corner radii (horizontal, vertical) in the order top-left, top-right, bottom-right, bottom-left.
pub type Radii = [(f32, f32); 4];

/// A region of `clip-path` that is not a rectangle, in page coordinates.
#[derive(Clone, Debug, PartialEq)]
pub enum ClipGeometry {
    Ellipse {
        center: (f32, f32),
        radii: (f32, f32),
    },
    /// Its points, filled by the even-odd rule or the nonzero one.
    Polygon {
        points: Vec<(f32, f32)>,
        even_odd: bool,
    },
    /// An outline of lines and curves (an SVG `<clipPath>`), filled by the even-odd rule or the
    /// nonzero one.
    Path {
        segments: Vec<fagus_layout::PathSegment>,
        even_odd: bool,
    },
}

/// One drawing operation.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    /// A filled rectangle, optionally with rounded corners.
    Rect {
        rect: RectF,
        color: Rgba,
        radii: Option<Radii>,
    },
    /// One side of a border, as a filled quadrilateral (for solid borders) or a stroked line.
    BorderSide {
        points: [(f32, f32); 4],
        color: Rgba,
        style: BorderStyle,
        width: f32,
    },
    /// A rounded border with the same style and color on all sides: the area between two rounded
    /// rectangles. With `stroke`, a stroke that wide along the middle between them instead, 0 for
    /// a hairline, as Chrome's native theme draws the frames of form controls.
    RoundedBorder {
        outer: RectF,
        outer_radii: Radii,
        inner: RectF,
        inner_radii: Radii,
        color: Rgba,
        stroke: Option<f32>,
    },
    /// Dashes or dots of a rounded border, as Chrome strokes them: along the middle of the
    /// border (`center`) all around, clockwise from the top left corner, sized by the side's
    /// `width`, a stroke `stroke` wide cut to the area between the two rounded rectangles.
    DashedBorder {
        outer: RectF,
        outer_radii: Radii,
        inner: RectF,
        inner_radii: Radii,
        center: RectF,
        center_radii: Radii,
        color: Rgba,
        style: BorderStyle,
        width: f32,
        stroke: f32,
        /// The middle of a border with shaped corners (`corner-shape`), a closed outline
        /// stroked in place of `center`.
        centerline: Option<Arc<[fagus_layout::PathSegment]>>,
    },
    /// A tile of a background layer repeated over `area` as one pattern, for layers of more
    /// tiles than are drawn one by one: `tile` drawn with its top left corner at `origin`, and
    /// again every `step`.
    Tiles {
        area: RectF,
        origin: (f32, f32),
        step: (f32, f32),
        tile: Arc<Item>,
    },
    /// A run of glyphs. `y` is the baseline.
    Text {
        x: f32,
        y: f32,
        face: FaceId,
        size: f32,
        /// The coordinates of a variable face; empty for others.
        variations: Vec<([u8; 4], f32)>,
        color: Rgba,
        glyphs: Vec<Glyph>,
        text: String,
        synthetic: Synthetic,
        /// Whether the glyphs advance down the page, one below the other (upright glyphs of a
        /// vertical line), rather than to the right; their offsets move them right and up.
        vertical: bool,
        /// The colors of the face's palette that `font-palette` gives a color font, by index;
        /// `None` for the face's own first palette.
        palette: Option<Arc<[fagus_text::PaletteColor]>>,
    },
    /// A wavy line (a text decoration) as Blink draws it: a stroke `thickness` wide along one
    /// cubic curve every `wavelength`, the first from `start` on its middle at `y`, going down
    /// first, with control points `control` below and above the middle halfway along; cut to
    /// `clip`.
    Wave {
        start: f32,
        y: f32,
        control: f32,
        wavelength: f32,
        thickness: f32,
        clip: RectF,
        color: Rgba,
    },
    /// A dashed or dotted line (a text decoration) as Blink strokes it: from `from` to `to`, a
    /// stroke `thickness` wide, with dashes or dots sized for a line `dash` pixels wide.
    DashedLine {
        from: (f32, f32),
        to: (f32, f32),
        thickness: f32,
        dash: f32,
        dotted: bool,
        color: Rgba,
    },
    /// An image scaled into a rectangle.
    Image {
        rect: RectF,
        image: Arc<fagus_layout::Image>,
    },
    /// A rectangle (a background tile) filled with a gradient whose geometry is relative to
    /// the rectangle's top left corner.
    Gradient {
        rect: RectF,
        gradient: Arc<GradientPaint>,
    },
    /// A box shadow (CSS Backgrounds 3, §6).
    BoxShadow(Arc<BoxShadowPaint>),
    /// A blurred shadow of a run of glyphs (`text-shadow`); `y` is the baseline. A sharp one is
    /// text of its own.
    TextShadow {
        x: f32,
        y: f32,
        face: FaceId,
        size: f32,
        variations: Vec<([u8; 4], f32)>,
        glyphs: Vec<Glyph>,
        /// The decoration lines that cast the shadow too.
        lines: Vec<RectF>,
        color: Rgba,
        blur: f32,
    },
    PushOpacity(f32),
    /// Blends what follows until the matching [`Item::Pop`] with what lies below it, each thing
    /// drawn on its own; an [`Item::PushIsolated`] right after it makes them blend as one group.
    PushBlend(fagus_style::values::BlendMode),
    /// Draws what follows until the matching [`Item::Pop`] as an isolated group, so that what
    /// it blends blends only within it.
    PushIsolated,
    /// Puts what follows until the matching [`Item::Pop`] on a layer of the PDF that a viewer
    /// can show and hide (`-fagus-layer`).
    PushLayer(Arc<fagus_style::values::Layer>),
    /// Transforms what follows until the matching [`Item::Pop`]: `[a, b, c, d, e, f]` maps
    /// `(x, y)` to `(a·x + c·y + e, b·x + d·y + f)`.
    PushTransform([f32; 6]),
    /// Clips what follows until the matching [`Item::Pop`], to a rectangle with optional rounded
    /// corners.
    PushClip(RectF, Option<Radii>),
    /// Clips what follows until the matching [`Item::Pop`] to a shape of `clip-path` other
    /// than a rectangle.
    PushClipShape(ClipGeometry),
    /// Masks what follows until the matching [`Item::Pop`] with what a mask draws.
    PushMask(Arc<MaskPaint>),
    /// What a box draws, filtered by SVG filters whose primitives only pixels can go through:
    /// drawn as pixels, filtered and drawn as an image, as Chrome prints it.
    Filtered(Arc<FilteredPaint>),
    Pop,
    /// What the drawing operations that follow are to a reader, up to the next tag. Only display
    /// lists painted for a tagged PDF have tags.
    Tag(Tag),
}

/// What drawing operations are to a reader of a tagged PDF.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tag {
    /// Text or an image of the document.
    Content {
        /// The closest element around the content.
        node: NodeId,
        /// The position in reading order: fragments are numbered in tree order, page by page.
        order: u32,
        /// Whether the content is the marker of a list item.
        marker: bool,
    },
    /// Something a reader skips.
    Artifact(Artifact),
}

/// Kinds of content a reader skips.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Artifact {
    /// Backgrounds, borders, lines and other design.
    Layout,
    /// What the margin boxes at the top of a page show.
    Header,
    /// What the margin boxes at the bottom show.
    Footer,
    /// What the other margin boxes show.
    Pagination,
}

/// Styles a face lacks and that are imitated, as browsers do (CSS Fonts 4, `font-synthesis`):
/// italic by slanting the glyphs, bold by outlining them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct Synthetic {
    pub italic: bool,
    pub bold: bool,
}

/// A mask (`mask`): what its items draw, image layers of a box, shows what it masks as far as
/// the items' alpha, or their luminance, reaches.
#[derive(Clone, Debug, PartialEq)]
pub struct MaskPaint {
    pub items: Vec<Item>,
    pub luminance: bool,
}

/// What a box draws, filtered as pixels (see [`Item::Filtered`]).
#[derive(Clone, Debug, PartialEq)]
pub struct FilteredPaint {
    /// What the box draws, unfiltered, without tags.
    pub items: Vec<Item>,
    /// The box's border box: the bounding box of the filters, whose user space starts at its
    /// top left corner.
    pub bounds: RectF,
    /// The filters, in the order they apply.
    pub filters: Vec<PixelFilterStep>,
}

/// One filter of [`FilteredPaint`].
#[derive(Clone, Debug, PartialEq)]
pub enum PixelFilterStep {
    /// The SVG `<filter>` with the id `id` in `markup` (the element with what it refers to).
    Svg { markup: Arc<str>, id: Arc<str> },
    /// A CSS filter function.
    Function(fagus_style::values::Filter),
}

/// A box shadow: a shape, blurred, in one color.
#[derive(Clone, Debug, PartialEq)]
pub struct BoxShadowPaint {
    /// Outer shadows show only outside this area (the border box); inset shadows only inside it
    /// (the padding box).
    pub area: RectF,
    pub area_radii: Option<Radii>,
    /// Before blurring: the box, moved and spread (outer shadows), or the hole that the shadow
    /// surrounds (inset shadows).
    pub shape: RectF,
    pub shape_radii: Option<Radii>,
    /// A part of an outer shadow's shape that casts no shadow: the inside of a ring, where a
    /// rounded border is blurred by `filter`.
    pub hole: Option<(RectF, Option<Radii>)>,
    pub color: Rgba,
    /// The blur radius: a Gaussian blur with half of it as standard deviation.
    pub blur: f32,
    pub inset: bool,
    /// The outlines of `area` and `shape` where the box's corners have other shapes than round
    /// (`corner-shape`), which stand for the rectangles and their radii.
    pub outlines: Option<Arc<ShadowOutlines>>,
}

/// The outlines of the area and the shape of a box shadow (see [`BoxShadowPaint`]).
#[derive(Clone, Debug, PartialEq)]
pub struct ShadowOutlines {
    pub area: Vec<fagus_layout::PathSegment>,
    pub shape: Vec<fagus_layout::PathSegment>,
}

/// A gradient resolved for one tile: geometry in pixels relative to the tile, color stops at
/// offsets from 0 to 1, already sampled for the color space they interpolate in.
#[derive(Clone, Debug, PartialEq)]
pub enum GradientPaint {
    /// A repeating gradient whose period is too short to show, painted in its average color.
    Solid(Rgba),
    Linear {
        start: (f32, f32),
        end: (f32, f32),
        stops: Vec<(f32, Rgba)>,
        repeat: bool,
    },
    /// Circles around `center` up to `radius`, stretched vertically by `scale_y` into ellipses.
    Radial {
        center: (f32, f32),
        radius: f32,
        scale_y: f32,
        stops: Vec<(f32, Rgba)>,
    },
    /// Clockwise from `start_angle` (degrees, 0 = up) around `center`, one full turn.
    Conic {
        center: (f32, f32),
        start_angle: f32,
        stops: Vec<(f32, Rgba)>,
    },
}

/// The display list of one page.
#[derive(Clone, Debug)]
pub struct PageList {
    pub width: f32,
    pub height: f32,
    pub items: Vec<Item>,
    /// How far the content may reach past the page's edges, to be trimmed off.
    pub bleed: f32,
    pub marks: fagus_layout::PageMarks,
    pub orientation: fagus_layout::PageOrientation,
}

/// Builds the display lists of laid-out pages; `tagged` adds the tags of a tagged PDF.
pub fn paint(pages: &[Page], fonts: &FontDb, images: &fagus_layout::Images, tagged: bool) -> Vec<PageList> {
    background::with_tile_budget(|| paint_pages(pages, fonts, images, tagged, false, None).0)
}

/// Builds the display lists of laid-out pages as [`paint`] does, with the controls of `fields`
/// painted for the fields of an interactive form: what each field shows goes into a widget of
/// its own (see [`Lift`]), in the order the controls are painted, and not onto the page.
#[allow(clippy::implicit_hasher, reason = "the engine's maps are all of one hasher")]
pub fn paint_with_fields(
    pages: &[Page],
    fonts: &FontDb,
    images: &fagus_layout::Images,
    tagged: bool,
    fields: &FxHashMap<NodeId, Lift>,
) -> (Vec<PageList>, Vec<Widget>) {
    background::with_tile_budget(|| paint_pages(pages, fonts, images, tagged, false, Some(fields)))
}

/// Paints the viewport of the document of a frame (see [`fagus_layout::FrameDocument`]), laid
/// out as on a screen: as a page, but with the images of the root's background placed in the
/// root's box, as Chrome places them in continuous media.
pub(crate) fn paint_frame(page: &Page, fonts: &FontDb, images: &fagus_layout::Images) -> Vec<Item> {
    background::with_tile_budget(|| {
        paint_pages(std::slice::from_ref(page), fonts, images, false, true, None).0
    })
    .into_iter()
    .next()
    .map(|page| page.items)
    .unwrap_or_default()
}

/// Paints `pages` (see [`paint`]); `continuous` where they are the viewport of a frame (see
/// [`paint_frame`]).
fn paint_pages(
    pages: &[Page],
    fonts: &FontDb,
    images: &fagus_layout::Images,
    tagged: bool,
    continuous: bool,
    fields: Option<&FxHashMap<NodeId, Lift>>,
) -> (Vec<PageList>, Vec<Widget>) {
    let mut order = 0;
    let slices = slices(pages);
    let mut carrier = carried::Carrier::new(pages);
    let mut widgets = Vec::new();
    let lists = pages
        .iter()
        .enumerate()
        .map(|(page_index, page)| {
            // What reaches onto the page from the pages before it, cut to the part that shows
            // there and put beside the page's own content (see `carried`), a repeat of what
            // those pages tag, so an artifact: made for this page only and dropped once it is
            // painted, as copies for all pages at once take gigabytes where content reaches
            // over thousands of pages.
            let merged = carrier.merged(page_index);
            let mut orders = FxHashMap::default();
            if tagged && let Some(content) = page.fragments.first() {
                number(content, &mut order, &mut orders);
            }
            let inline_clips = inline_clip_boxes(
                merged
                    .as_ref()
                    .map(|(content, _)| &**content)
                    .into_iter()
                    .chain(page.fragments.iter().skip(usize::from(merged.is_some()))),
            );
            let mut painter = Painter {
                fonts,
                images,
                items: Vec::new(),
                tagged,
                orders,
                inline_clips,
                slices: &slices,
                aliases: merged.as_ref().map(|(_, aliases)| aliases),
                node: None,
                artifact: None,
                tag: None,
                ruby_annotation: None,
                line_shift: 0.0,
                ruby_shifts: None,
                text_mask: false,
                mask_decorations: Vec::new(),
                pixels: false,
                next_context: NextContext::Other,
                transforms: 0,
                space: None,
                next_space: None,
                fields,
                lifted: Vec::new(),
                page_index,
                placement: fields::Placement::default(),
                layers: Vec::new(),
            };
            painter.design(|painter| page_box(page, images, &mut painter.items));
            let (width, height) = (page.width.to_px(), page.height.to_px());
            // The root element and the margin boxes each form a stacking context. The content of
            // the margin boxes repeats on pages: a reader skips it.
            for index in stacking_order(page) {
                let fragment = match (&merged, index) {
                    (Some((content, _)), 0) => &**content,
                    _ => match page.fragments.get(index) {
                        Some(fragment) => fragment,
                        None => continue,
                    },
                };
                // The root element's background goes on the canvas under the document, over
                // the margin boxes below it.
                if index == 0 {
                    painter.design(|painter| {
                        canvas_background(page, &slices, images, continuous, &mut painter.items);
                    });
                }
                painter.artifact = (index > 0).then(|| margin_artifact(fragment, page.height.to_px()));
                // Chrome cuts the content off at the edges of the page area: what overflows into
                // the margins does not show. Right-to-left content, placed from the right, has
                // the area placed so too.
                let clip = index == 0 && page.area.width > Au::ZERO && page.area.height > Au::ZERO;
                if clip {
                    let rect = page_content_clip(content_area(page), (width, height));
                    painter.items.push(Item::PushClip(rect, None));
                }
                // Content laid out wider than the page shrinks to the page area, from its corner
                // (where the root element's box need not start: its margins and offsets shrink
                // too).
                let shrink = index == 0 && page.scale > 0.0 && page.scale < 1.0;
                if shrink {
                    let (x, y, scale) = (page.area.x.to_px(), page.area.y.to_px(), page.scale);
                    painter.items.push(Item::PushTransform([
                        scale,
                        0.0,
                        0.0,
                        scale,
                        x * (1.0 - scale),
                        y * (1.0 - scale),
                    ]));
                }
                // Right-to-left content, laid out from a whole pixel, is placed a fraction of a
                // pixel further, with the boxes it snaps to whole pixels.
                let shift = index == 0 && page.shift != 0.0;
                if shift {
                    painter
                        .items
                        .push(Item::PushTransform([1.0, 0.0, 0.0, 1.0, page.shift, 0.0]));
                }
                if index == 0 {
                    painter.next_context = NextContext::PageRoot;
                }
                painter.placement = fields::Placement {
                    shrink: shrink.then(|| (page.scale, page.area.x.to_px(), page.area.y.to_px())),
                    shift: if shift { page.shift } else { 0.0 },
                };
                painter.stacking_context(fragment, (0.0, 0.0), None);
                if shift {
                    painter.items.push(Item::Pop);
                }
                if shrink {
                    painter.items.push(Item::Pop);
                }
                if clip {
                    painter.items.push(Item::Pop);
                }
            }
            widgets.extend(painter.lifted.into_iter().map(|(_, widget)| widget));
            // SVG images with HTML in them are drawn as the documents they are.
            let mut items = painter.items;
            document::expand(&mut items, fonts, images);
            // What the clip hides entirely is not drawn at all, as in Chrome.
            cull::cull_hidden(&mut items, fonts, cull::page_bounds(width, height, page.bleed));
            // The list is held until the PDF is written (an estimate: an item and as much again
            // for what it points to).
            fagus_dom::budget::charge(
                fagus_dom::budget::Kind::Painting,
                (items.len() * 2 * size_of::<Item>()) as u64,
            );
            fagus_dom::budget::check_time_now();
            PageList {
                width,
                height,
                items,
                bleed: page.bleed,
                marks: page.marks,
                orientation: page.orientation,
            }
        })
        .collect();
    (lists, widgets)
}

/// The width of the spaces that end the last text of an inline box fragment and hang in it
/// (preserved spaces, other space separators), where nothing comes after them in it but the
/// ends of inline boxes.
fn trailing_spaces(fragment: &Fragment) -> f32 {
    trailing_spaces_in(fragment, false)
}

/// [`trailing_spaces`] in a box that shows its end padding or border where `edged` (an inline box
/// fragment around it whose end it is): the collapsible spaces before such an edge do not end
/// the line, and stay.
fn trailing_spaces_in(fragment: &Fragment, edged: bool) -> f32 {
    let Some(last) = fragment.children.last() else {
        return 0.0;
    };
    let edged = edged
        || matches!(&fragment.kind, FragmentKind::Box { style, sides, .. }
            if sides.right
                && (style.border_width.right > 0.0
                    || !matches!(style.padding.right, fagus_style::values::LengthPercentage::Px(px) if px == 0.0)));
    match &last.kind {
        // Collapsible spaces at the end of a line are removed, as Chrome removes them from the
        // line: their glyphs are dropped, but the room they took stays in the run and in the
        // boxes around it, which reach no further than the glyphs before them.
        FragmentKind::Text(run)
            if run.style.white_space.collapses_spaces()
                && run.text.trim_end().trim_end_matches(' ').len()
                    == run.text.trim_end_matches(' ').len() =>
        {
            if run.text.ends_with(' ') && !edged {
                let advance: f32 = run.glyphs.iter().map(|glyph| glyph.advance).sum();
                (last.rect.width.to_px() - advance).max(0.0)
            } else {
                0.0
            }
        }
        FragmentKind::Text(run) => {
            let spaces: f32 = run
                .glyphs
                .iter()
                .rev()
                .take_while(|glyph| {
                    run.text
                        .get(glyph.cluster.0 as usize..glyph.cluster.1 as usize)
                        .is_some_and(|cluster| {
                            !cluster.is_empty() && cluster.chars().all(char::is_whitespace)
                        })
                })
                .map(|glyph| glyph.advance)
                .sum();
            let advance: f32 = run.glyphs.iter().map(|glyph| glyph.advance).sum();
            (last.rect.width.to_px() - (advance - spaces)).max(0.0)
        }
        FragmentKind::Box { style, .. } if style.display == fagus_style::values::Display::INLINE => {
            trailing_spaces_in(last, edged)
        }
        _ => 0.0,
    }
}

/// How far the content of a page reaches past the left edge of its page area (see
/// [`Page::area`]), as Chrome measures
/// the width of a document to shrink it to its pages: the border boxes, lines and text in it as
/// they are transformed, without what boxes that cut off their overflow leave out, nor fixed
/// boxes. A line reaches as far as its content with the margins of the boxes in it, as Blink's
/// line box does: an inline box or an inline block at the end of a line takes its right margin
/// along.
pub fn content_extent(page: &Page) -> f32 {
    content_reach(page).0 - page.area.x.to_px()
}

/// How much the content of `page` shrinks to fit its page area, as Chrome shrinks a document
/// wider than its pages (1 or more where it fits): the width of the page area against how far
/// the content reaches (see [`content_extent`]). In a vertical document, whose lines run down
/// the pages, it is the height of the area against how far the content reaches down past its
/// top, as Blink measures the document's logical width (its height there): content past the
/// right or left edge goes on the next pages, and that of `sideways-lr` lines, which run up
/// the page, does not count above it.
pub fn content_shrink(page: &Page) -> f32 {
    let vertical = matches!(
        page.fragments.first().map(|content| &content.kind),
        Some(FragmentKind::Box { style, .. }) if style.is_vertical()
    );
    // The area is as tall as the page leaves it ([`Page::area`]), but the content is laid out
    // on it rounded up to whole pixels, as it is as wide: the root reaches that far.
    let (room, extent) = if vertical {
        (
            page.area.height.to_px().ceil(),
            content_reach(page).1 - page.area.y.to_px(),
        )
    } else {
        (page.area.width.to_px(), content_extent(page))
    };
    if room > 0.0 {
        room / extent.max(f32::EPSILON)
    } else {
        1.0
    }
}

/// The advance of the punctuation mark that may hang at the right or bottom end of a run, past
/// spaces: its last glyph in visual order (`hanging-punctuation`). 0 where it is no such mark.
fn hanging_mark(run: &fagus_layout::TextRun) -> f32 {
    let mark = |glyph: &&Glyph| {
        run.text
            .get(glyph.cluster.0 as usize..glyph.cluster.1 as usize)
            .and_then(|cluster| cluster.chars().next())
    };
    run.glyphs
        .iter()
        .rev()
        .find(|glyph| !mark(glyph).is_none_or(char::is_whitespace))
        .filter(|glyph| mark(glyph).is_some_and(fagus_layout::hangable_punctuation))
        .map_or(0.0, |glyph| glyph.advance)
}

/// How far right and how far down the content of `page` reaches on it, as [`content_extent`]
/// measures it.
fn content_reach(page: &Page) -> (f32, f32) {
    /// `outer` applied after `inner`.
    fn compose(outer: [f32; 6], inner: [f32; 6]) -> [f32; 6] {
        [
            outer[0] * inner[0] + outer[2] * inner[1],
            outer[1] * inner[0] + outer[3] * inner[1],
            outer[0] * inner[2] + outer[2] * inner[3],
            outer[1] * inner[2] + outer[3] * inner[3],
            outer[0] * inner[4] + outer[2] * inner[5] + outer[4],
            outer[1] * inner[4] + outer[3] * inner[5] + outer[5],
        ]
    }
    /// The right edge, the width and the bottom edge of a line.
    type LineEdges = (f32, f32, f32);
    /// `line` is the edges of the line a fragment is in, if it is in one.
    fn walk(
        fragment: &Fragment,
        (origin, line): ((f32, f32), Option<LineEdges>),
        matrix: [f32; 6],
        reach: &mut (f32, f32),
    ) {
        let rect = fragment.rect;
        let left = origin.0 + rect.x.to_px();
        let top = origin.1 + rect.y.to_px();
        let (mut width, mut height) = (rect.width.to_px(), rect.height.to_px());
        // An inline box that goes on to the next line takes the spaces that hang at the end of
        // its line, which reach nowhere either: they do not take it past the line.
        if let (FragmentKind::Box { style, .. }, Some((end, _, _))) = (&fragment.kind, line)
            && style.display == fagus_style::values::Display::INLINE
        {
            width = width.min((end - left).max(width - trailing_spaces(fragment)));
        }
        // The right margin of a box in a horizontal line, left to right (of an inline box, on
        // the line where it ends), which the line's content reaches over.
        let margin = match (&fragment.kind, line) {
            (FragmentKind::Box { style, sides, .. }, Some((_, line_width, _)))
                if !style.is_rtl()
                    && !style.writing_mode.is_vertical()
                    && (sides.right || style.display != fagus_style::values::Display::INLINE) =>
            {
                style.margin.right.resolve_or(line_width, 0.0).max(0.0)
            }
            _ => 0.0,
        };
        let line = match fragment.kind {
            FragmentKind::Line { .. } => Some((left + width, width, top + height)),
            FragmentKind::Box { ref style, .. } if style.display != fagus_style::values::Display::INLINE => {
                None
            }
            _ => line,
        };
        // Spaces that end a line hang past it and reach nowhere (CSS Text 3, §4.1.3). The
        // advances of vertical text run down the page; its run is as wide as its column.
        if let FragmentKind::Text(run) = &fragment.kind
            && !run.style.is_rtl()
            && !run.style.writing_mode.is_vertical()
        {
            let spaces: f32 = run
                .glyphs
                .iter()
                .rev()
                .take_while(|glyph| {
                    run.text
                        .get(glyph.cluster.0 as usize..glyph.cluster.1 as usize)
                        .is_some_and(|cluster| {
                            !cluster.is_empty() && cluster.chars().all(char::is_whitespace)
                        })
                })
                .map(|glyph| glyph.advance)
                .sum();
            // Its width as Blink's text fragment has it: the advances rounded up to a layout
            // unit, which is what the content reaches in Chrome's measure. Justified text fills
            // its line exactly, which Chrome does not count as reaching past it.
            let advance: f32 = run.glyphs.iter().map(|glyph| glyph.advance).sum();
            // The spaces that end a line keep their room without glyphs (they are not drawn).
            let covered = run
                .glyphs
                .iter()
                .map(|glyph| glyph.cluster.1 as usize)
                .max()
                .unwrap_or(0);
            let dropped = run
                .text
                .get(covered..)
                .is_some_and(|rest| !rest.is_empty() && rest.chars().all(char::is_whitespace));
            let hanging = if dropped { (width - advance).max(0.0) } else { 0.0 };
            width =
                if run.glyphs.is_empty() || run.style.text_align == fagus_style::values::TextAlign::Justify {
                    (width - spaces - hanging).max(0.0)
                } else {
                    ((advance - spaces).max(0.0) * 64.0).ceil() / 64.0
                };
        }
        // Punctuation that hangs past the end (or the start) of its line is ink overflow (CSS
        // Text 3, §8.2): it reaches no further than the line.
        if let (FragmentKind::Text(run), Some((end, _, bottom))) = (&fragment.kind, line)
            && !run.style.hanging_punctuation.is_none()
        {
            let hanging = hanging_mark(run);
            if run.style.writing_mode.is_vertical() {
                height = height.min((bottom - top).max(height - hanging));
            } else {
                width = width.min((end - left).max(width - hanging));
            }
        }
        let mut matrix = matrix;
        let mut clips = false;
        if let FragmentKind::Box { style, .. } = &fragment.kind {
            if style.has_transform()
                && let Some(transform) = transform::matrix(style, (left, top, width, height), None)
            {
                matrix = compose(matrix, transform);
            }
            clips = style.overflow_x != fagus_style::values::Overflow::Visible;
        }
        for (corner_x, corner_y) in [
            (left, top),
            (left + width + margin, top),
            (left, top + height),
            (left + width + margin, top + height),
        ] {
            reach.0 = reach
                .0
                .max(matrix[0] * corner_x + matrix[2] * corner_y + matrix[4]);
            reach.1 = reach
                .1
                .max(matrix[1] * corner_x + matrix[3] * corner_y + matrix[5]);
        }
        if !clips {
            // Fixed boxes do not widen the document, as in Chrome.
            for child in fragment.children.iter().filter(|child| {
                !matches!(&child.kind, FragmentKind::Box { style, .. }
                    if style.position == fagus_style::values::Position::Fixed)
            }) {
                walk(child, ((left, top), line), matrix, reach);
            }
        }
    }
    let Some(content) = page.fragments.first() else {
        return (0.0, 0.0);
    };
    let mut reach = (0.0_f32, 0.0_f32);
    walk(
        content,
        ((0.0, 0.0), None),
        [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        &mut reach,
    );
    reach
}

/// Numbers the text and images of a page's content in tree order, which is their reading order.
/// Paints the page box: its background, which covers the page and its bleed with its layers
/// positioned in the page box (CSS Paged Media 3, §3.2), the canvas inside its border, and the
/// border, inside the page margins, with the outline around it.
/// The order the content of `page` (its first fragment) and its margin boxes paint in: by
/// their `z-index`, the margin boxes below zero under the content, the others over it, each in
/// the order they come in where their `z-index` is the same, as in Chrome, where each margin
/// box is a stacking context beside the page's.
fn stacking_order(page: &Page) -> Vec<usize> {
    let z = |index: usize| match page.fragments.get(index).map(|fragment| &fragment.kind) {
        Some(FragmentKind::Box { style, .. }) if index > 0 => style.z_index.unwrap_or(0),
        _ => 0,
    };
    let mut order: Vec<usize> = (0..page.fragments.len()).collect();
    order.sort_by_key(|&index| (z(index), usize::from(index > 0 && z(index) >= 0)));
    order
}

/// The page area of `page` that its content shows in: placed as Chrome places it (at the page
/// margins rounded to whole pixels, see [`Page::area`]), but only as wide and tall as the page
/// less its margins, not rounded up as the area that Chrome lays the content out on. So a
/// border at the right edge of a box as wide as the area hardly shows, as in Chrome.
fn content_area(page: &Page) -> RectF {
    let [top, right, bottom, left] = page.margin.map(Au::to_px);
    let area = page.area;
    RectF {
        x: area.x.to_px() + page.shift,
        y: area.y.to_px(),
        width: area
            .width
            .to_px()
            .min((page.width.to_px() - left - right).max(0.0)),
        height: area
            .height
            .to_px()
            .min((page.height.to_px() - top - bottom).max(0.0)),
    }
}

/// Where the content of a page shows: its page area `area`, and no further than the page of
/// `size` (which negative page margins make the area reach past), rounded out to whole device
/// pixels of Chrome's printing (300 per inch) and one more, as Chrome clips it.
fn page_content_clip(area: RectF, (width, height): (f32, f32)) -> RectF {
    const DEVICE: f32 = 300.0 / 96.0;
    let (left, top) = (area.x.max(-1.0 / DEVICE), area.y.max(-1.0 / DEVICE));
    let right = (area.x + area.width).min(((width * DEVICE).ceil() + 1.0) / DEVICE);
    let bottom = (area.y + area.height).min(((height * DEVICE).ceil() + 1.0) / DEVICE);
    RectF {
        x: left,
        y: top,
        width: (right - left).max(0.0),
        height: (bottom - top).max(0.0),
    }
}

/// The rectangle of the page box of `page`: the page less its margins.
fn page_box_rect(page: &Page) -> RectF {
    let (width, height) = (page.width.to_px(), page.height.to_px());
    let [top, right, bottom, left] = page.margin.map(Au::to_px);
    RectF {
        x: left,
        y: top,
        width: (width - left - right).max(0.0),
        height: (height - top - bottom).max(0.0),
    }
}

/// Paints the page box of `page`: its background over the whole sheet, its border and its
/// outline (the root element's background goes on later, see [`canvas_background`]).
fn page_box(page: &Page, images: &fagus_layout::Images, items: &mut Vec<Item>) {
    // Chrome paints the page's background over the page rounded to whole pixels (an A5 page
    // 559.37 pixels wide gets 559 of them, one 300.9 wide 301; Chrome 157, measured).
    let (width, height) = (page.width.to_px().round(), page.height.to_px().round());
    let page_box = page_box_rect(page);
    let sheet = RectF {
        x: -page.bleed,
        y: -page.bleed,
        width: width + 2.0 * page.bleed,
        height: height + 2.0 * page.bleed,
    };
    // A root element in the dark color scheme paints the whole sheet `Canvas` first, as
    // Chrome's view does.
    if page.canvas.as_ref().is_some_and(|canvas| canvas.dark) {
        items.push(Item::Rect {
            rect: sheet,
            color: Rgba::from_u8(0x12, 0x12, 0x12, 1.0),
            radii: None,
        });
    }
    let style = &page.style;
    // `visibility: hidden` in the page context hides the page box's own background, border and
    // outline, not the page's content, as in Chrome.
    if style.visibility != Visibility::Visible {
        return;
    }
    let placement = background::Placement {
        whole: Some(page_box),
        covers: true,
        ..background::Placement::of(sheet, None)
    };
    background::paint(&page_background(style), placement, images, items);
    let radii = radii(style, page_box.width, page_box.height);
    paint_borders(style, page_box, radii, fagus_layout::VisibleSides::ALL, items);
    // The outline goes around the border, in the page margins, as in Chrome.
    if style.outline_width > 0.0 {
        paint_outline(style, page_box, radii, items);
    }
}

/// Paints the root element's background on the canvas of `page`, inside the page box's border.
/// The layers are placed in a box as large as the root's border box drawn whole, at the corner
/// of the canvas, as Chrome places them.
/// Where `continuous` (in the viewport of a frame, see [`paint_frame`]), the layers are placed in
/// the root's own box, as Chrome places them on a screen.
fn canvas_background(
    page: &Page,
    slices: &Slices,
    images: &fagus_layout::Images,
    continuous: bool,
    items: &mut Vec<Item>,
) {
    let (Some(background), Some(root)) = (&page.canvas, page.fragments.first()) else {
        return;
    };
    // A hidden root element leaves the canvas empty, with the background its body gives it
    // (Chrome 157, measured).
    if matches!(&root.kind, FragmentKind::Box { style, .. } if style.visibility != Visibility::Visible) {
        return;
    }
    let page_box = page_box_rect(page);
    let border = &page.style.border_width;
    let canvas = RectF {
        x: page_box.x + border.left,
        y: page_box.y + border.top,
        width: (page_box.width - border.left - border.right).max(0.0),
        height: (page_box.height - border.top - border.bottom).max(0.0),
    };
    let (before, whole) = slices
        .get(&std::ptr::from_ref(root))
        .filter(|slice| slice.flow == Flow::Down)
        .map_or((0.0, root.rect.height.to_px()), |slice| {
            (slice.before, slice.whole)
        });
    // (On a screen, as in a frame, the root's box itself.)
    let (x, y) = if continuous {
        (canvas.x + root.rect.x.to_px(), canvas.y + root.rect.y.to_px())
    } else {
        (canvas.x, canvas.y)
    };
    let placement = background::Placement {
        whole: Some(RectF {
            x,
            y: y - before,
            width: root.rect.width.to_px(),
            height: whole,
        }),
        covers: true,
        viewport: Some(canvas_viewport(page, root)),
        ..background::Placement::of(canvas, None)
    };
    // Where the content's clip (see `content_area`) ends less than a pixel before the canvas,
    // which it does where the page area is placed at the margins rounded down, the canvas ends
    // with it, as in Chrome. Page padding beside the area keeps the canvas.
    // A canvas that the clip does not cut gets none (whose edge would blur the page's).
    let mut clips = false;
    if page.area.width > Au::ZERO && page.area.height > Au::ZERO {
        let clip = page_content_clip(content_area(page), (page.width.to_px(), page.height.to_px()));
        let cut = |end: f32, clip_end: f32| (end > clip_end && end - clip_end < 1.0).then_some(clip_end);
        // Nor does it reach past the page rounded to whole pixels (see `page_box`).
        let (page_width, page_height) = (page.width.to_px().round(), page.height.to_px().round());
        let right = cut(canvas.x + canvas.width, (clip.x + clip.width).min(page_width));
        let bottom = cut(canvas.y + canvas.height, (clip.y + clip.height).min(page_height));
        if right.is_some() || bottom.is_some() {
            clips = true;
            let right = right.unwrap_or(canvas.x + canvas.width);
            let bottom = bottom.unwrap_or(canvas.y + canvas.height);
            items.push(Item::PushClip(
                RectF {
                    width: (right - canvas.x).max(0.0),
                    height: (bottom - canvas.y).max(0.0),
                    ..canvas
                },
                None,
            ));
        }
    }
    background::paint(background, placement, images, items);
    if clips {
        items.push(Item::Pop);
    }
}

/// The area the fixed layers of the root element's background (`background-attachment: fixed`)
/// are sized and positioned in on `page`, whose first fragment is the root's `root`: the page
/// area less the root's margins, at the page area's corner (Chrome 157, measured: with `@page {
/// size: 600px 400px; margin: 30px 50px }` and `html { margin: 10px 40px 0 5px }`, `100% 100%`
/// places a 100 × 50 layer at (405, 310)).
fn canvas_viewport(page: &Page, root: &Fragment) -> RectF {
    let area = content_area(page);
    let FragmentKind::Box { style, .. } = &root.kind else {
        return area;
    };
    let margin = |side: &fagus_style::values::LengthPercentageAuto| side.resolve_or(area.width, 0.0);
    let margins = &style.margin;
    RectF {
        width: (area.width - margin(&margins.left) - margin(&margins.right)).max(0.0),
        height: (area.height - margin(&margins.top) - margin(&margins.bottom)).max(0.0),
        ..area
    }
}

/// The background of the page box as Chrome paints it: its color and its images, but none of
/// its gradients (Chrome 157, measured; linear, radial and repeating alike), whose layers stay
/// empty so that the other layers keep their sizes and positions.
fn page_background(style: &Arc<ComputedStyle>) -> std::borrow::Cow<'_, ComputedStyle> {
    use fagus_style::values::Image;
    if !style
        .background_image
        .iter()
        .any(|image| matches!(image, Image::Gradient(_)))
    {
        return std::borrow::Cow::Borrowed(style);
    }
    let mut without = (**style).clone();
    without.background_image = style
        .background_image
        .iter()
        .map(|image| match image {
            Image::Gradient(_) => Image::None,
            image => image.clone(),
        })
        .collect();
    std::borrow::Cow::Owned(without)
}

/// The reference boxes of the clip paths (`clip-path`) of the inline boxes broken across lines
/// among `fragments` (and their descendants), by element: the border box of their first
/// fragment on the page, which clips all of them, as in Chrome (measured).
fn inline_clip_boxes<'a>(fragments: impl Iterator<Item = &'a Fragment>) -> FxHashMap<NodeId, RectF> {
    fn collect(fragment: &Fragment, (x, y): (f32, f32), boxes: &mut FxHashMap<NodeId, (RectF, usize)>) {
        let (x, y) = (x + fragment.rect.x.to_px(), y + fragment.rect.y.to_px());
        if let FragmentKind::Box {
            node: Some(node),
            style,
            ..
        } = &fragment.kind
            && style.clip_path.is_some()
            && is_inline_box(style)
        {
            let rect = RectF {
                x,
                y,
                width: fragment.rect.width.to_px(),
                height: fragment.rect.height.to_px(),
            };
            boxes.entry(*node).or_insert((rect, 0)).1 += 1;
        }
        for child in &fragment.children {
            collect(child, (x, y), boxes);
        }
    }
    let mut boxes = FxHashMap::default();
    for fragment in fragments {
        collect(fragment, (0.0, 0.0), &mut boxes);
    }
    boxes
        .into_iter()
        .filter(|(_, (_, count))| *count > 1)
        .map(|(node, (rect, _))| (node, rect))
        .collect()
}

/// Where the fragments of boxes broken across pages or columns lie in the boxes drawn whole,
/// by address.
type Slices = FxHashMap<*const Fragment, Slice>;

/// Where a fragment of a box broken across pages or columns lies in the box drawn whole.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Slice {
    /// How far into the box (along the way its block flows) the fragment starts.
    before: f32,
    /// The length of the whole box that way.
    whole: f32,
    /// The way the box's blocks flow, the way the fragments follow each other.
    flow: Flow,
}

/// The physical direction in which the fragments of a broken box follow each other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flow {
    Down,
    /// `vertical-lr` and `sideways-lr`.
    Right,
    /// `vertical-rl` and `sideways-rl`.
    Left,
}

impl Flow {
    fn of(style: &ComputedStyle) -> Self {
        use fagus_style::values::WritingMode;
        match style.writing_mode {
            WritingMode::HorizontalTb => Self::Down,
            WritingMode::VerticalLr | WritingMode::SidewaysLr => Self::Right,
            WritingMode::VerticalRl | WritingMode::SidewaysRl => Self::Left,
        }
    }

    /// Whether a fragment with sides `sides` is cut where the box breaks.
    fn cut(self, sides: fagus_layout::VisibleSides) -> bool {
        match self {
            Self::Down => !(sides.top && sides.bottom),
            Self::Right | Self::Left => !(sides.left && sides.right),
        }
    }

    /// The length of `rect` the way the fragments follow each other.
    fn length(self, rect: fagus_layout::units::Rect) -> f32 {
        match self {
            Self::Down => rect.height.to_px(),
            Self::Right | Self::Left => rect.width.to_px(),
        }
    }
}

impl Slice {
    /// The box drawn whole, around its fragment at `rect`.
    fn whole(self, rect: RectF) -> RectF {
        match self.flow {
            Flow::Down => RectF {
                y: rect.y - self.before,
                height: self.whole,
                ..rect
            },
            Flow::Right => RectF {
                x: rect.x - self.before,
                width: self.whole,
                ..rect
            },
            Flow::Left => RectF {
                x: rect.x + rect.width + self.before - self.whole,
                width: self.whole,
                ..rect
            },
        }
    }
}

/// The slices of the boxes broken across `pages`, whose backgrounds are drawn for the whole box
/// and sliced (CSS Fragmentation 3, §5.4), in any writing mode. Chrome sizes the background of
/// each fragment of a box that clones its decorations for all of them as well, but starts it at
/// each.
fn slices(pages: &[Page]) -> Slices {
    fn collect<'a>(fragment: &'a Fragment, broken: &mut FxHashMap<NodeId, Vec<&'a Fragment>>) {
        if let FragmentKind::Box {
            node: Some(node),
            sides,
            style,
        } = &fragment.kind
            && (Flow::of(style).cut(*sides) || style.clones_decorations() && style.display.is_block_level())
        {
            broken.entry(*node).or_default().push(fragment);
        }
        for child in &fragment.children {
            collect(child, broken);
        }
    }
    let mut broken = FxHashMap::default();
    for fragment in pages.iter().flat_map(|page| &page.fragments) {
        collect(fragment, &mut broken);
    }
    let mut slices = Slices::default();
    for fragments in broken.values().filter(|fragments| fragments.len() > 1) {
        let flow = match &fragments[0].kind {
            FragmentKind::Box { style, .. } => Flow::of(style),
            _ => Flow::Down,
        };
        let whole = fragments.iter().map(|fragment| flow.length(fragment.rect)).sum();
        let mut before = 0.0;
        for fragment in fragments {
            slices.insert(std::ptr::from_ref(*fragment), Slice { before, whole, flow });
            let clones =
                matches!(&fragment.kind, FragmentKind::Box { style, .. } if style.clones_decorations());
            if !clones {
                before += flow.length(fragment.rect);
            }
        }
    }
    slices
}

fn number(fragment: &Fragment, next: &mut u32, orders: &mut FxHashMap<*const Fragment, u32>) {
    if matches!(fragment.kind, FragmentKind::Text(_) | FragmentKind::Image(_)) {
        orders.insert(std::ptr::from_ref(fragment), *next);
        *next += 1;
    }
    for child in &fragment.children {
        number(child, next, orders);
    }
}

/// The kind of artifact a margin box is, by where it is on the page.
fn margin_artifact(fragment: &Fragment, page_height: f32) -> Artifact {
    let middle = fragment.rect.y.to_px() + fragment.rect.height.to_px() / 2.0;
    if middle < page_height / 3.0 {
        Artifact::Header
    } else if middle > page_height * 2.0 / 3.0 {
        Artifact::Footer
    } else {
        Artifact::Pagination
    }
}

/// A positioned descendant or a stacking context, painted after the in-flow content of the
/// stacking context it belongs to.
struct Layer<'f> {
    fragment: &'f Fragment,
    /// The closest element around the fragment's content: the fragment's own or an ancestor's.
    node: Option<NodeId>,
    /// The position of the fragment's parent on the page.
    origin: (f32, f32),
    /// Clips of the ancestors between the stacking context and the fragment.
    clips: Vec<Clip>,
    z: i32,
    /// Whether the fragment establishes a stacking context; positioned boxes with `z-index: auto`
    /// only paint like one.
    context: bool,
    /// The perspective its parent gives it.
    perspective: Option<transform::Perspective>,
    /// Whether it is printed as an isolated group of its own, in which its `mix-blend-mode` has
    /// no backdrop to blend with, as Chrome prints an absolutely positioned or fixed box of the
    /// root element's stacking context whose containing block is the initial one, and the
    /// layers of that stacking context painted after it.
    isolated: bool,
}

struct Painter<'a> {
    fonts: &'a FontDb,
    images: &'a fagus_layout::Images,
    items: Vec<Item>,
    tagged: bool,
    /// The reading order of the text and image fragments, by address.
    orders: FxHashMap<*const Fragment, u32>,
    slices: &'a Slices,
    /// The fragments of the page's content that copy others (see [`carried`]), by address.
    aliases: Option<&'a carried::Aliases>,
    /// The closest element around what is painted.
    node: Option<NodeId>,
    /// Set while painting a margin box: everything in it is this kind of artifact.
    artifact: Option<Artifact>,
    /// The last tag added.
    tag: Option<Tag>,
    /// While the base of a ruby column is painted: the side of its annotation and how far that
    /// reaches (its top or bottom, on the page), past which emphasis marks on that side go.
    ruby_annotation: Option<(AnnotationSide, f32)>,
    /// How far down the page the text being painted moves, so that the top of its line box (or
    /// of the inline box with a fragment of its own it is in) lies on a whole pixel: Chrome draws
    /// text from there, its baseline on a whole pixel as well where the leading is (Blink's
    /// `TextFragmentPainter`).
    line_shift: f32,
    /// While a ruby column in a line is painted: the shifts of the text of its annotation and
    /// of its base in a vertical line. Chrome draws the annotation from the line the column is
    /// in and the base from the column's box, not each from where its own text starts.
    ruby_shifts: Option<(f32, f32)>,
    /// Set while the text mask of a background clipped to text is painted: descendants that
    /// would be painted as layers of their own go into it as in flow (see [`Painter::text_mask`]).
    text_mask: bool,
    /// While a text mask is painted: where the decoration lines of its text lie among the
    /// items, which go into it with the glyphs, as Chrome paints them into it.
    mask_decorations: Vec<std::ops::Range<usize>>,
    /// Set while a box that is drawn as pixels (see [`Item::Filtered`]) is painted: backgrounds
    /// reach under opaque borders, as Chrome draws them, so that no seam lets them show through
    /// where the two meet.
    pixels: bool,
    /// What the next stacking context painted is, until it takes it.
    next_context: NextContext,
    /// How many boxes with a transform are painted around what is painted (the box itself
    /// included): in them fixed backgrounds scroll (CSS Transforms 1, §6), as in Chrome.
    transforms: u32,
    /// The reference boxes of the clip paths of inline boxes broken across lines (see
    /// [`inline_clip_boxes`]), by element.
    inline_clips: FxHashMap<NodeId, RectF>,
    /// The 3D space (`transform-style: preserve-3d`) of the box whose layers are painted, and
    /// the transform that draws in its plane: its transformed children compose their
    /// transforms with it in 3D rather than with its flattened transform.
    space: Option<(transform::Matrix3d, Item)>,
    /// The 3D space the next stacking context painted is in, until it takes it.
    next_space: Option<transform::Matrix3d>,
    /// The controls painted for the fields of an interactive form (see [`paint_with_fields`]).
    fields: Option<&'a FxHashMap<NodeId, Lift>>,
    /// The widgets of the fields on the page, by the address of the control's fragment.
    lifted: Vec<(*const Fragment, Widget)>,
    /// The index of the page painted.
    page_index: usize,
    /// How the content painted is placed on the page.
    placement: fields::Placement,
    /// The layers (`-fagus-layer`) around what is painted, innermost last.
    layers: Vec<Arc<fagus_style::values::Layer>>,
}

/// What the next stacking context painted is, where it matters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum NextContext {
    #[default]
    Other,
    /// That of a page's content (the root element's).
    PageRoot,
    /// A layer printed as an isolated group (see [`Layer::isolated`]).
    Isolated,
}

impl Painter<'_> {
    /// Whether a child is painted in a later phase of its stacking context instead of in flow;
    /// `in_line` tells whether it is on a line. While a text mask is painted, only absolutely
    /// positioned descendants and transformed atomic inlines are.
    fn layered(&self, child: &Fragment, in_line: bool) -> bool {
        if !self.text_mask {
            return is_layered(child);
        }
        matches!(&child.kind, FragmentKind::Box { style, .. }
            if style.is_out_of_flow() || (in_line && style.has_transform() && !is_inline_box(style)))
    }

    /// Paints a stacking context in the order of CSS 2.1, Appendix E.
    /// Paints a stacking context, which its parent gives `perspective`.
    fn stacking_context(
        &mut self,
        fragment: &Fragment,
        origin: (f32, f32),
        perspective: Option<transform::Perspective>,
    ) {
        let page_root = self.next_context == NextContext::PageRoot;
        if page_root {
            self.next_context = NextContext::Other;
        }
        let outer = self.enter(fragment);
        let in_space = self.next_space.take();
        let outer_space = self.space.take();
        let (mut x, mut y) = position(fragment, origin);
        // Chrome paints a box transformed by more than a translation from a whole pixel: the
        // fraction of its position is dropped (Blink keeps it only through translations).
        if let FragmentKind::Box { style, .. } = &fragment.kind
            && (style.has_transform() || perspective.is_some() || in_space.is_some())
        {
            let size = (fragment.rect.width.to_px(), fragment.rect.height.to_px());
            let translates = transform::matrix(style, (x, y, size.0, size.1), perspective)
                .is_some_and(|[a, b, c, d, ..]| [a - 1.0, b, c, d - 1.0].iter().all(|v| v.abs() < 1e-6));
            if !translates {
                (x, y) = (round(x), round(y));
            }
        }
        // In a 3D space, the box's transform composes with the space's; a box that keeps the
        // space (`transform-style: preserve-3d`) passes it on to its children.
        let space = match &fragment.kind {
            FragmentKind::Box { style, .. }
                if in_space.is_some()
                    || (keeps_3d(style, fragment) && (style.has_transform() || perspective.is_some())) =>
            {
                let size = (fragment.rect.width.to_px(), fragment.rect.height.to_px());
                let (local, reference) = transform::matrix3d(style, (x, y, size.0, size.1), perspective);
                let full = in_space.map_or(local, |outer| transform::compose(&outer, &local));
                Some((full, reference, keeps_3d(style, fragment)))
            }
            _ => None,
        };
        let start_effects = self.items.len();
        let layer = match &fragment.kind {
            FragmentKind::Box { style, .. } => style.layer.clone(),
            _ => None,
        };
        let layered = layer.is_some();
        self.layers.extend(layer);
        let mut effects = self.push_effects(fragment, (x, y), perspective, space.map(|(m, r, _)| (m, r)));
        // A box that keeps the space draws its own content in its plane, and its layers in the
        // space (each with its own transform composed with the space's).
        if let Some((full, _, true)) = space
            && effects == 1
            && matches!(
                self.items.last(),
                Some(Item::PushTransform(_) | Item::PushOpacity(_))
            )
            && self.items.len() == start_effects + 1
            && let Some(plane) = self.items.pop()
        {
            effects = 0;
            self.space = Some((full, plane));
        }
        // The root's stacking context too, which leaves the page's canvas (the root's
        // background) out of what blends in it.
        if page_root
            && !matches!(&fragment.kind, FragmentKind::Box { style, .. } if style.is_stacking_context())
            && blends_inside(fragment)
        {
            self.items.push(Item::PushIsolated);
            effects += 1;
        }
        let transformed = matches!(&fragment.kind, FragmentKind::Box { style, .. }
            if style.has_transform() || perspective.is_some());
        self.transforms += u32::from(transformed);
        let start = self.items.len();
        let outer_pixels = self.pixels;
        if let FragmentKind::Box { style, .. } = &fragment.kind
            && reference::filters(&style.filter, self.images)
                .iter()
                .any(|filter| matches!(filter, fagus_style::values::Filter::Url(_)))
        {
            self.pixels = true;
        }
        self.in_plane(|painter| painter.decorations(fragment, x, y));
        let clip = clip_rect(fragment, x, y);
        let mut layers = Vec::new();
        collect_layers(
            fragment,
            (x, y),
            &mut clip.clone().into_iter().collect(),
            (self.node, perspective_of(fragment, (x, y))),
            &mut layers,
        );
        let mut layers = in_tree_order(layers);
        // A stable sort keeps tree order among equal `z-index` values.
        layers.sort_by_key(|layer| layer.z);
        // Chrome prints the absolutely positioned and fixed boxes of the root element's stacking
        // context whose containing block is the initial one as isolated groups of their own, and
        // each layer painted after the first of them too (see `Layer::isolated`).
        // The layers inside such a box blend in its group, with what it paints.
        if page_root {
            let mut groups: Vec<&Fragment> = Vec::new();
            for layer in &mut layers {
                let own = matches!(&layer.fragment.kind, FragmentKind::Box { style, .. }
                    if matches!(style.position, fagus_style::values::Position::Absolute | fagus_style::values::Position::Fixed))
                    && fragment
                        .children
                        .iter()
                        .any(|child| std::ptr::eq(child, layer.fragment));
                if own {
                    groups.push(layer.fragment);
                }
                layer.isolated =
                    own || !groups.is_empty() && !groups.iter().any(|group| contains(group, layer.fragment));
            }
        }
        for layer in layers.iter().filter(|layer| layer.z < 0) {
            self.layer(layer);
        }
        self.in_plane(|painter| {
            painter.in_flow(fragment, x, y, clip.as_ref());
            painter.outlines(fragment, x, y);
        });
        for layer in layers.iter().filter(|layer| layer.z >= 0) {
            self.layer(layer);
        }
        self.space = outer_space;
        // What the box draws, filtered inside its clip and opacity (Filter Effects 1, §2).
        if let FragmentKind::Box { style, .. } = &fragment.kind
            && !style.filter.is_empty()
        {
            let filters = reference::filters(&style.filter, self.images);
            // From the first filter that only pixels go through on, the box is drawn as pixels.
            // Filters around a box drawn as pixels filter pixels too.
            let pixels = filters
                .iter()
                .position(|filter| matches!(filter, fagus_style::values::Filter::Url(_)))
                .or_else(|| {
                    self.items[start..]
                        .iter()
                        .any(|item| matches!(item, Item::Filtered(_)))
                        .then_some(0)
                });
            let vector = &filters[..pixels.unwrap_or(filters.len())];
            filter::apply(&mut self.items, start, vector, style.color, self.tagged);
            if let Some(first) = pixels {
                let bounds = RectF {
                    x,
                    y,
                    width: fragment.rect.width.to_px(),
                    height: fragment.rect.height.to_px(),
                };
                // A shadow in `currentColor` drawn before taints them.
                let tainted = filters[..first].iter().any(|filter| {
                    matches!(
                        filter,
                        fagus_style::values::Filter::DropShadow { color: None, .. }
                    )
                });
                let steps = reference::pixel_steps(&filters[first..], style.color, self.images, tainted);
                filter::as_pixels(&mut self.items, start, bounds, steps);
            }
        }
        self.pixels = outer_pixels;
        self.transforms -= u32::from(transformed);
        self.pop(effects);
        if layered {
            self.layers.pop();
        }
        self.node = outer;
    }

    /// Paints with `paint` in the plane of the box whose layers are painted in a 3D space (see
    /// [`Painter::space`]), or as it is outside one.
    fn in_plane(&mut self, paint: impl FnOnce(&mut Self)) {
        match self.space.take() {
            Some((full, plane)) => {
                self.items.push(plane.clone());
                paint(self);
                self.items.push(Item::Pop);
                self.space = Some((full, plane));
            }
            None => paint(self),
        }
    }

    fn layer(&mut self, layer: &Layer<'_>) {
        let outer = std::mem::replace(&mut self.node, layer.node);
        // In a 3D space, a stacking context that is a child of the space's element and that
        // nothing clips takes its place in the space; other layers are drawn in that box's
        // plane, as in Chrome.
        let space = self.space.take();
        let child =
            matches!(&layer.fragment.kind, FragmentKind::Box { style, .. } if style.parent_preserve_3d);
        let in_plane = match &space {
            Some((full, _)) if layer.context && layer.clips.is_empty() && child => {
                self.next_space = Some(*full);
                false
            }
            Some((_, plane)) => {
                self.items.push(plane.clone());
                true
            }
            None => false,
        };
        for clip in &layer.clips {
            self.items.push(clip.item());
        }
        if layer.context {
            if layer.isolated {
                self.next_context = NextContext::Isolated;
            }
            self.stacking_context(layer.fragment, layer.origin, layer.perspective);
        } else {
            let (x, y) = position(layer.fragment, layer.origin);
            let own = css_clip(layer.fragment, x, y);
            if let Some(clip) = own {
                self.items.push(Item::PushClip(clip, None));
            }
            self.decorations(layer.fragment, x, y);
            self.in_flow(layer.fragment, x, y, clip_rect(layer.fragment, x, y).as_ref());
            self.outlines(layer.fragment, x, y);
            self.pop(usize::from(own.is_some()));
        }
        self.pop(layer.clips.len() + usize::from(in_plane));
        self.next_space = None;
        self.space = space;
        self.node = outer;
    }

    /// Paints the outlines of a box and of the boxes in its flow after their content, as browsers
    /// do; those of boxes painted as layers of their own come with them. A box's overflow clips
    /// the outlines inside it, not its own, which comes after them, as Blink paints a box's
    /// descendant outlines before its own (`kDescendantOutlinesOnly`, then `kSelfOutlineOnly`).
    fn outlines(&mut self, fragment: &Fragment, x: f32, y: f32) {
        let start = self.items.len();
        let clip = clip_rect(fragment, x, y);
        if let Some(clip) = &clip {
            self.items.push(clip.item());
        }
        let before = self.items.len();
        for child in &fragment.children {
            // Layers paint their own outlines; text and images have none.
            if is_layered(child)
                || child.children.is_empty() && !matches!(child.kind, FragmentKind::Box { .. })
            {
                continue;
            }
            let (child_x, child_y) = position(child, (x, y));
            self.outlines(child, child_x, child_y);
        }
        if self.items.len() == before {
            // Nothing inside has an outline: no clip for it.
            self.items.truncate(start);
        } else {
            self.pop(usize::from(clip.is_some()));
        }
        self.own_outline(fragment, x, y);
    }

    /// Paints the outline of a box (not those inside it), with its border box at `x`, `y`.
    fn own_outline(&mut self, fragment: &Fragment, x: f32, y: f32) {
        if let FragmentKind::Box { style, .. } = &fragment.kind
            && style.outline_width > 0.0
            && style.visibility == Visibility::Visible
            && !is_table_row_or_group(style)
        {
            let rect = snap(collapsed_cell_box(
                style,
                RectF {
                    x,
                    y,
                    width: fragment.rect.width.to_px(),
                    height: fragment.rect.height.to_px(),
                },
            ));
            let radii = radii(style, rect.width, rect.height);
            self.design(|painter| paint_outline(style, rect, radii, &mut painter.items));
        }
    }

    /// The address of the fragment that `fragment` copies, or its own (see [`carried`]).
    fn original(&self, fragment: &Fragment) -> *const Fragment {
        let address = std::ptr::from_ref(fragment);
        self.aliases
            .and_then(|aliases| aliases.get(&address).copied())
            .unwrap_or(address)
    }

    /// Makes a box the closest element around what is painted next, if it is an element's box;
    /// returns the one before, to restore.
    fn enter(&mut self, fragment: &Fragment) -> Option<NodeId> {
        let outer = self.node;
        if let FragmentKind::Box { node: Some(node), .. } = &fragment.kind {
            self.node = Some(*node);
        }
        outer
    }

    /// Adds a tag, unless it is the last one added.
    fn tag(&mut self, tag: Tag) {
        if self.tagged && self.tag != Some(tag) {
            self.tag = Some(tag);
            self.items.push(Item::Tag(tag));
        }
    }

    /// Tags what follows as the content of `fragment`, text or an image.
    fn tag_content(&mut self, fragment: &Fragment, marker: bool) {
        if !self.tagged {
            return;
        }
        let order = self.orders.get(&self.original(fragment)).copied();
        let tag = match (self.artifact, self.node, order) {
            (None, Some(node), Some(order)) => Tag::Content { node, order, marker },
            (artifact, ..) => Tag::Artifact(artifact.unwrap_or(Artifact::Layout)),
        };
        self.tag(tag);
    }

    /// Paints the decoration lines of a run of text as design; while a text mask is painted,
    /// marks them to go into it. The text painted after them is content again, as it was before.
    fn decorations_of(&mut self, paint: impl FnOnce(&mut Vec<Item>)) {
        let content = self.tag;
        self.design(|painter| {
            let start = painter.items.len();
            paint(&mut painter.items);
            if painter.text_mask {
                let end = painter.items.len();
                painter.mask_decorations.push(start..end);
            }
        });
        if let Some(tag) = content {
            self.tag(tag);
        }
    }

    /// Tags what follows as design, and removes the tag again if nothing follows.
    fn design(&mut self, paint: impl FnOnce(&mut Self)) {
        let before = (self.items.len(), self.tag);
        self.tag(Tag::Artifact(self.artifact.unwrap_or(Artifact::Layout)));
        let tagged = self.items.len() > before.0;
        paint(self);
        if tagged && self.items.len() == before.0 + 1 {
            self.items.pop();
            self.tag = before.1;
        }
    }

    /// Paints the in-flow content of a box: first the backgrounds and borders of block-level
    /// descendants, then the floats, then all inline content.
    fn in_flow(&mut self, fragment: &Fragment, x: f32, y: f32, clip: Option<&Clip>) {
        let outer = self.enter(fragment);
        self.lifted(fragment, x, y, |painter| {
            if let Some(clip) = clip {
                painter.items.push(clip.item());
            }
            painter.blocks(fragment, x, y);
            painter.floats(fragment, x, y);
            painter.inlines(fragment, x, y, false);
            painter.pop(usize::from(clip.is_some()));
        });
        self.node = outer;
    }

    /// Paints the floats among the descendants, each like a stacking context of its own (CSS 2.1,
    /// Appendix E, step 5).
    fn floats(&mut self, fragment: &Fragment, x: f32, y: f32) {
        let outer = self.enter(fragment);
        let items = holds_items(fragment);
        for child in &fragment.children {
            // Flex and grid items paint their floats themselves.
            if self.layered(child, false) || items {
                continue;
            }
            let (child_x, child_y) = position(child, (x, y));
            match &child.kind {
                FragmentKind::Box { style, .. } if style.is_floating() => {
                    self.decorations(child, child_x, child_y);
                    self.in_flow(
                        child,
                        child_x,
                        child_y,
                        clip_rect(child, child_x, child_y).as_ref(),
                    );
                }
                // A floating table with its captions, in its table wrapper box, paints as one
                // float: the table box's background and borders, then the captions and the table
                // in order (see `blocks`).
                FragmentKind::Anonymous if is_float(child) => self.floating_table(child, child_x, child_y),
                // Atomic inlines paint their floats themselves.
                FragmentKind::Box { style, .. }
                    if matches!(fragment.kind, FragmentKind::Line { .. }) && !is_inline_box(style) => {}
                FragmentKind::Box { .. } => {
                    let clip = clip_rect(child, child_x, child_y);
                    if let Some(clip) = &clip {
                        self.items.push(clip.item());
                    }
                    self.floats(child, child_x, child_y);
                    self.pop(usize::from(clip.is_some()));
                }
                FragmentKind::Anonymous | FragmentKind::Line { .. } => self.floats(child, child_x, child_y),
                FragmentKind::Text(_) | FragmentKind::Placeholder(_) | FragmentKind::Image(_) => {}
            }
        }
        self.node = outer;
    }

    /// Paints a table wrapper box whose table floats, with its captions, as one float: the
    /// table box's background and borders first, then the backgrounds of the captions and the
    /// table's content in order, then their floats and their inline content (see `blocks`).
    fn floating_table(&mut self, wrapper: &Fragment, x: f32, y: f32) {
        let Some(table) = wrapped_table(wrapper) else {
            return;
        };
        let outer = self.enter(wrapper);
        let place = |part: &Fragment| {
            let (part_x, part_y) = position(part, (x, y));
            (part_x, part_y, clip_rect(part, part_x, part_y))
        };
        let (table_x, table_y, _) = place(&wrapper.children[table]);
        self.decorations(&wrapper.children[table], table_x, table_y);
        for (index, part) in wrapper.children.iter().enumerate() {
            let (part_x, part_y, clip) = place(part);
            if index != table {
                self.decorations(part, part_x, part_y);
            }
            if let Some(clip) = &clip {
                self.items.push(clip.item());
            }
            self.blocks(part, part_x, part_y);
            self.pop(usize::from(clip.is_some()));
        }
        // Their floats, then their inline content.
        for inline in [false, true] {
            for part in &wrapper.children {
                let (part_x, part_y, clip) = place(part);
                if let Some(clip) = &clip {
                    self.items.push(clip.item());
                }
                if inline {
                    self.inlines(part, part_x, part_y, false);
                } else {
                    self.floats(part, part_x, part_y);
                }
                self.pop(usize::from(clip.is_some()));
            }
        }
        self.node = outer;
    }

    fn blocks(&mut self, fragment: &Fragment, x: f32, y: f32) {
        let outer = self.enter(fragment);
        let items = holds_items(fragment);
        // The table box of a table wrapper box paints its background and borders before the
        // captions, which paint over them (Chrome 157, measured: a caption with a negative margin
        // hides the table's border), and its cells after the captions above it.
        let table = wrapped_table(fragment).filter(|&index| {
            let child = &fragment.children[index];
            !self.layered(child, false) && !is_float(child)
        });
        if let Some(index) = table {
            let child = &fragment.children[index];
            let (child_x, child_y) = position(child, (x, y));
            self.decorations(child, child_x, child_y);
        }
        for (index, child) in fragment.children.iter().enumerate() {
            if self.layered(child, false) || is_float(child) {
                continue;
            }
            let (child_x, child_y) = position(child, (x, y));
            // Flex and grid items paint with the inline content (see `inlines`).
            if items && matches!(child.kind, FragmentKind::Box { .. } | FragmentKind::Anonymous) {
                continue;
            }
            match &child.kind {
                FragmentKind::Box { .. } => {
                    if table != Some(index) {
                        self.decorations(child, child_x, child_y);
                    }
                    let clip = clip_rect(child, child_x, child_y);
                    if let Some(clip) = &clip {
                        self.items.push(clip.item());
                    }
                    self.lifted(child, child_x, child_y, |painter| {
                        painter.blocks(child, child_x, child_y);
                    });
                    self.pop(usize::from(clip.is_some()));
                }
                FragmentKind::Anonymous => self.blocks(child, child_x, child_y),
                FragmentKind::Line { .. }
                | FragmentKind::Text(_)
                | FragmentKind::Placeholder(_)
                | FragmentKind::Image(_) => {}
            }
        }
        self.node = outer;
    }

    /// Whether the inline box of `fragment` (with `style`) has a fragment of its own in Blink for
    /// what it holds (`ShouldCreateBoxFragmentForChild`): an inline box with margins, another
    /// `vertical-align` than `baseline` or a first available font of another ascent or descent,
    /// or an atomic inline.
    fn uncull(&self, fragment: &Fragment, style: &ComputedStyle) -> bool {
        let mut own = None;
        fragment.children.iter().any(|child| match &child.kind {
            FragmentKind::Box { style: inner, .. } if is_inline_box(inner) => {
                if inner.vertical_align != fagus_style::values::VerticalAlign::Baseline || has_margin(inner) {
                    return true;
                }
                let own = *own.get_or_insert_with(|| self.font_height(style));
                own != self.font_height(inner)
            }
            FragmentKind::Box { style: inner, .. } => !inner.is_out_of_flow() && !inner.is_floating(),
            _ => false,
        })
    }

    /// The rounded ascent and descent of the first available font of a style.
    fn font_height(&self, style: &ComputedStyle) -> Option<(f32, f32)> {
        let face = fagus_layout::face_for(self.fonts, style)?;
        Some(
            self.fonts
                .face(face)
                .rounded_ascent_descent(fagus_layout::used_font_size(self.fonts, style)),
        )
    }

    /// Paints inline content. `in_line` tells whether `fragment`'s children are on a line: inline
    /// boxes and atomic inlines paint their backgrounds here, not with the blocks.
    fn inlines(&mut self, fragment: &Fragment, x: f32, y: f32, in_line: bool) {
        let outer = self.enter(fragment);
        // The annotation of a ruby column is not its base.
        let outer_annotation = self.ruby_annotation;
        if matches!(&fragment.kind, FragmentKind::Box { style, .. } if style.ruby_display == RubyDisplay::Annotation)
        {
            self.ruby_annotation = None;
        }
        let outer_shift = self.line_shift;
        let items = holds_items(fragment);
        // Text right in a box, out of lines (an initial letter), goes from the box's top.
        if !in_line && matches!(fragment.kind, FragmentKind::Box { .. }) {
            self.line_shift = round(y) - y;
        }
        for child in &fragment.children {
            if self.layered(child, in_line) || is_float(child) {
                continue;
            }
            let (child_x, child_y) = position(child, (x, y));
            // Flex and grid items paint like atomic inlines, each like a stacking context of its
            // own (CSS Flexbox 1, §5.4; CSS Grid 2, §9), as in Chrome: over the backgrounds of
            // the blocks after their container, the content of one under the background of the
            // next where they overlap.
            if items && matches!(child.kind, FragmentKind::Box { .. } | FragmentKind::Anonymous) {
                let line_shift = self.line_shift;
                self.decorations(child, child_x, child_y);
                self.in_flow(
                    child,
                    child_x,
                    child_y,
                    clip_rect(child, child_x, child_y).as_ref(),
                );
                self.line_shift = line_shift;
                continue;
            }
            match &child.kind {
                FragmentKind::Box { style, .. } if in_line && is_inline_box(style) => {
                    self.decorations(child, child_x, child_y);
                    let line_shift = self.line_shift;
                    if has_box_fragment(style) || self.uncull(child, style) {
                        self.line_shift = round(child_y) - child_y;
                    }
                    self.inlines(child, child_x, child_y, true);
                    self.line_shift = line_shift;
                }
                // An atomic inline paints like a stacking context of its own.
                FragmentKind::Box { style, .. } if in_line => {
                    let outer_annotation = self.ruby_annotation;
                    let outer_shifts = self.ruby_shifts;
                    self.ruby_shifts = None;
                    if style.ruby_display == RubyDisplay::Column {
                        self.ruby_annotation = ruby_annotation(child, child_y);
                        self.ruby_shifts = Some((self.line_shift, round(child_y) - child_y));
                    }
                    self.decorations(child, child_x, child_y);
                    let line_shift = self.line_shift;
                    self.in_flow(
                        child,
                        child_x,
                        child_y,
                        clip_rect(child, child_x, child_y).as_ref(),
                    );
                    self.line_shift = line_shift;
                    self.ruby_annotation = outer_annotation;
                    self.ruby_shifts = outer_shifts;
                }
                FragmentKind::Box { .. } => {
                    let clip = clip_rect(child, child_x, child_y);
                    if let Some(clip) = &clip {
                        self.items.push(clip.item());
                    }
                    self.lifted(child, child_x, child_y, |painter| {
                        painter.inlines(child, child_x, child_y, false);
                    });
                    self.pop(usize::from(clip.is_some()));
                }
                FragmentKind::Line { .. } => {
                    let line_shift = self.line_shift;
                    let top = line_top(child, child_y);
                    self.line_shift = match (&fragment.kind, self.ruby_shifts) {
                        (FragmentKind::Box { style, .. }, Some((annotation, base)))
                            if is_vertical_line(child) =>
                        {
                            match style.ruby_display {
                                RubyDisplay::Annotation => annotation,
                                RubyDisplay::Column => base,
                                _ => round(top) - top,
                            }
                        }
                        _ => round(top) - top,
                    };
                    self.inlines(child, child_x, child_y, true);
                    self.line_shift = line_shift;
                }
                FragmentKind::Anonymous => self.inlines(child, child_x, child_y, in_line),
                FragmentKind::Text(run) => {
                    // The marker of a list item or of a footnote labels it.
                    if run.style.visibility == Visibility::Visible {
                        self.tag_content(child, run.marker || run.style.footnote_marker);
                    }
                    let size = (child.rect.width.to_px(), child.rect.height.to_px());
                    // A marker outside a vertical line is a box of its own in Chrome, whose text
                    // goes from its top.
                    let outside = run.marker
                        && run.orientation != fagus_layout::GlyphOrientation::Horizontal
                        && matches!(fragment.kind, FragmentKind::Line { .. })
                        && (child_y < y || child_y >= y + fragment.rect.height.to_px());
                    // Stretched operators of formulas are drawn where they are laid out.
                    let shift = if outside {
                        round(child_y) - child_y
                    } else if run.exact {
                        0.0
                    } else {
                        self.line_shift
                    };
                    self.text(run, (child_x, child_y + shift), size);
                }
                // The image of a box with `visibility: hidden` is not drawn.
                FragmentKind::Image(_) if matches!(&fragment.kind, FragmentKind::Box { style, .. } if style.visibility != Visibility::Visible) =>
                    {}
                FragmentKind::Image(image) => {
                    let content = RectF {
                        x: child_x,
                        y: child_y,
                        width: child.rect.width.to_px(),
                        height: child.rect.height.to_px(),
                    };
                    // Chrome draws the image into its rectangle snapped to whole pixels, clipped
                    // to the snapped content box.
                    let (rect, overflows) = match &fragment.kind {
                        FragmentKind::Box { style, .. } => object_rect(style, image, content),
                        _ => (content, false),
                    };
                    let unsnapped = rect;
                    // An inline `<svg>` element is drawn at its size, moved as far as its border
                    // box moves to whole pixels (its padding and border keep their fractions),
                    // and clipped to its snapped box (Blink's `SVGRootPainter`).
                    let (rect, content, overflows) = if image.inline {
                        let snapped = snap(content);
                        let moved = RectF {
                            x: rect.x + round(x) - x,
                            y: rect.y + round(y) - y,
                            ..rect
                        };
                        let outside = moved.x + moved.width > snapped.x + snapped.width + 0.01
                            || moved.y + moved.height > snapped.y + snapped.height + 0.01;
                        (moved, snapped, overflows || outside)
                    } else {
                        (snap(rect), snap(content), overflows)
                    };
                    // What lies outside the box is clipped as its `overflow` says, by default
                    // at its content box (CSS Images 3, §5.5), along the curve of the content
                    // edge of a box with rounded corners (CSS Backgrounds 3, §5.3).
                    let clip = match &fragment.kind {
                        // An inline `<svg>` that shows its overflow in both axes is not cut
                        // along rounded corners either (Chrome 157, measured).
                        FragmentKind::Box { style, .. }
                            if image.inline
                                && style.overflow_x == fagus_style::values::Overflow::Visible
                                && style.overflow_y == fagus_style::values::Overflow::Visible =>
                        {
                            None
                        }
                        FragmentKind::Box { style, .. } => {
                            let (width, height) = (fragment.rect.width.to_px(), fragment.rect.height.to_px());
                            let border = &style.border_width;
                            clip_margin::replaced_clip(
                                style,
                                RectF { x, y, width, height },
                                [border.top, border.right, border.bottom, border.left],
                                content,
                                radii(style, width, height),
                            )
                            .map(|(clip, curves)| {
                                // The content box as it is, where the clip is that (the default).
                                let at_content = covers(clip, content) && covers(content, clip);
                                (if at_content { content } else { snap(clip) }, curves)
                            })
                        }
                        _ => overflows.then_some((content, None)),
                    };
                    // A clip at the content box is needed only where the image overflows it
                    // (whose snapping `overflows` tells); another one where it cuts the image.
                    // An inline `<svg>` may draw beyond its box: the PDF output shows that (see
                    // `fagus_pdf`'s `unclipped`), and this clip cuts it.
                    let clip = clip.filter(|(clip, curves)| {
                        curves.is_some()
                            || overflows
                            || image.inline
                            || (*clip != content && !covers(*clip, rect))
                    });
                    let clipped = clip.is_some();
                    if let Some((clip, curves)) = clip {
                        // Corners of other shapes (`corner-shape`) clip along the inner edge of
                        // the border box's outline, inset to the clip.
                        let shaped = match &fragment.kind {
                            FragmentKind::Box { style, .. } if curves.is_some() => {
                                let (width, height) =
                                    (fragment.rect.width.to_px(), fragment.rect.height.to_px());
                                radii(style, width, height)
                                    .filter(|outer| corner::is_shaped(style, Some(outer)))
                                    .map(|outer| {
                                        let insets = [
                                            clip.y - y,
                                            x + width - clip.x - clip.width,
                                            y + height - clip.y - clip.height,
                                            clip.x - x,
                                        ];
                                        let border_box = RectF { x, y, width, height };
                                        corner::inner_edge(border_box, &outer, insets, style).clip()
                                    })
                            }
                            _ => None,
                        };
                        self.items.push(shaped.unwrap_or(Item::PushClip(clip, curves)));
                    }
                    // An image right in a line is a list item's marker (`list-style-image`).
                    self.tag_content(child, matches!(fragment.kind, FragmentKind::Line { .. }));
                    // An SVG image is fitted into its box as laid out and then stretched to the
                    // snapped box, as Chrome draws it (Blink's `SVGImage::DrawForContainer`
                    // lays it out at the container size and scales that to the destination).
                    // (The document of a frame is laid out on the snapped box itself.)
                    let stretched = (!image.inline
                        && image.format == fagus_layout::ImageFormat::Svg
                        && !self.images.is_frame(&image.data))
                    .then(|| stretch_to_snapped(unsnapped))
                    .flatten();
                    let mut rect = match stretched {
                        Some(matrix) => {
                            self.items.push(Item::PushTransform(matrix));
                            unsnapped
                        }
                        None => rect,
                    };
                    // A zoomed inline `<svg>` draws its content zoomed: one that keeps its own
                    // scale (without a `viewBox`) too.
                    let zoom = match &fragment.kind {
                        FragmentKind::Box { style, .. } if image.inline => style.zoom,
                        _ => 1.0,
                    };
                    let zoomed = (zoom - 1.0).abs() > 1e-6 && zoom > 0.0;
                    if zoomed {
                        self.items
                            .push(Item::PushTransform([zoom, 0.0, 0.0, zoom, 0.0, 0.0]));
                        rect = RectF {
                            x: rect.x / zoom,
                            y: rect.y / zoom,
                            width: rect.width / zoom,
                            height: rect.height / zoom,
                        };
                    }
                    self.items.push(Item::Image {
                        rect,
                        image: Arc::clone(image),
                    });
                    self.pop(usize::from(clipped) + usize::from(stretched.is_some()) + usize::from(zoomed));
                }
                FragmentKind::Placeholder(_) => {}
            }
        }
        self.ruby_annotation = outer_annotation;
        self.line_shift = outer_shift;
        self.node = outer;
    }

    /// Starts the group effects of a stacking context at `(x, y)`; returns how many to pop.
    fn push_effects(
        &mut self,
        fragment: &Fragment,
        (x, y): (f32, f32),
        perspective: Option<transform::Perspective>,
        space: Option<(transform::Matrix3d, (f32, f32, f32, f32))>,
    ) -> usize {
        let unblended = std::mem::take(&mut self.next_context) == NextContext::Isolated;
        let FragmentKind::Box { style, .. } = &fragment.kind else {
            return 0;
        };
        // A stacking context is an isolated group: what blends in it blends with what it draws
        // only (Compositing 1, §3.2), as in Chrome.
        let isolates_blending = style.is_stacking_context() && blends_inside(fragment);
        let mut pushed = 0;
        // The layer holds everything the box paints, its transform and effects too.
        if let Some(layer) = &style.layer {
            self.items.push(Item::PushLayer(Arc::clone(layer)));
            pushed += 1;
        }
        if let Some((full, reference)) = space {
            // In a 3D space: the transform composed in it, flattened. (Chrome prints the backs
            // of boxes with `backface-visibility: hidden` too.)
            self.items.push(match transform::flattened(&full, reference) {
                Some(matrix) => Item::PushTransform(matrix),
                None => Item::PushOpacity(0.0),
            });
            pushed += 1;
        } else if style.has_transform() || perspective.is_some() {
            let width = fragment.rect.width.to_px();
            let height = fragment.rect.height.to_px();
            self.items.push(
                match transform::matrix(style, (x, y, width, height), perspective) {
                    Some(matrix) => Item::PushTransform(matrix),
                    // Behind the viewer: not seen.
                    None => Item::PushOpacity(0.0),
                },
            );
            pushed += 1;
        }
        if let Some(clip) = css_clip(fragment, x, y) {
            self.items.push(Item::PushClip(clip, None));
            pushed += 1;
        }
        if let Some(clip) = &style.clip_path {
            let width = fragment.rect.width.to_px();
            let height = fragment.rect.height.to_px();
            // An inline box broken across lines is clipped in the box of its first fragment.
            let border_box = match &fragment.kind {
                FragmentKind::Box { node: Some(node), .. } if is_inline_box(style) => {
                    self.inline_clips.get(node).copied()
                }
                _ => None,
            }
            .unwrap_or(RectF { x, y, width, height });
            let item = match &clip.shape {
                fagus_style::values::ClipShape::Url(url) => {
                    reference::clip(url, border_box, self.images, style.zoom)
                }
                _ => Some(clip_item(style, clip, border_box)),
            };
            if let Some(item) = item {
                self.items.push(item);
                pushed += 1;
            }
        }
        if let Some(mask) = style.mask.as_ref().filter(|mask| !mask.is_none()) {
            let width = fragment.rect.width.to_px();
            let height = fragment.rect.height.to_px();
            let border_box = RectF { x, y, width, height };
            let radii = radii(style, width, height);
            pushed += mask::push(style, mask, (border_box, radii), self.images, &mut self.items);
        }
        if style.mix_blend_mode != fagus_style::values::BlendMode::Normal && !unblended {
            // The box and its content blend with the backdrop as one group: what it paints
            // composes first (its shadows, its background and the layers inside it over each
            // other), as in Chrome, not each part with the backdrop on its own.
            self.items.push(Item::PushBlend(style.mix_blend_mode));
            self.items.push(Item::PushIsolated);
            pushed += 2;
        } else if style.isolation == fagus_style::values::Isolation::Isolate
            || style.mix_blend_mode != fagus_style::values::BlendMode::Normal
            || isolates_blending
            // Chrome prints a box with a backdrop filter as a group of its own (without the
            // filtered backdrop).
            || style.backdrop_filter
        {
            self.items.push(Item::PushIsolated);
            pushed += 1;
        }
        if style.opacity < 1.0 {
            self.items.push(Item::PushOpacity(style.opacity));
            pushed += 1;
        }
        pushed
    }

    fn pop(&mut self, count: usize) {
        for _ in 0..count {
            self.items.push(Item::Pop);
        }
    }

    /// Background and borders of a box.
    fn decorations(&mut self, fragment: &Fragment, x: f32, y: f32) {
        let FragmentKind::Box { style, sides, .. } = &fragment.kind else {
            return;
        };
        if style.visibility != Visibility::Visible || is_table_row_or_group(style) {
            return;
        }
        let width = fragment.rect.width.to_px();
        let height = fragment.rect.height.to_px();
        // Chrome paints the backgrounds, borders and shadows of boxes into their border boxes
        // snapped to whole pixels (Blink's `ToPixelSnappedRect`).
        let mut rect = snap(RectF { x, y, width, height });
        // The bar of a fraction or radical straddles its top; snapping keeps it at least a
        // pixel thick (Blink's `SnapSizeToPixel`).
        if style.math_bar {
            if rect.height <= 0.0 && height > 0.0 {
                rect.height = 1.0;
            }
            if rect.width <= 0.0 && width > 0.0 {
                rect.width = 1.0;
            }
            rect.y -= (rect.height / 2.0).floor();
        }
        let radii = fragment_radii(style, rect.width, rect.height, *sides);
        let images = self.images;
        // The background of a slice of a broken box is positioned in the box drawn whole.
        let slice = self.slices.get(&self.original(fragment)).copied();
        let whole = slice.map(|slice| snap(slice.whole(RectF { x, y, width, height })));
        // Chrome's theme fills the ground of a control's native frame up to the middle of the
        // border, along which it strokes the frame.
        let framed = style.native_frame.is_some() && *sides == fagus_layout::VisibleSides::ALL;
        let placement = if framed {
            let half = style.border_width.top / 2.0;
            let middle = RectF {
                x: rect.x + half,
                y: rect.y + half,
                width: (rect.width - 2.0 * half).max(0.0),
                height: (rect.height - 2.0 * half).max(0.0),
            };
            background::Placement::of(middle, radii.and_then(|radii| inner_radii(radii, [half; 4])))
        } else {
            background::Placement {
                whole,
                exact: Some(slice.map_or(RectF { x, y, width, height }, |slice| {
                    slice.whole(RectF { x, y, width, height })
                })),
                opaque_border: !self.pixels && opaque_border(style, *sides),
                scrolled: background::scrollable_border_box(style, fragment, rect),
                ..background::Placement::of(rect, radii)
            }
        };
        // An element without text draws its background clipped to the text as if it were
        // clipped to the border box: Chrome's mask of no glyphs is a layer without content,
        // which its PDF output leaves out, and the background with it shows whole (Chrome 157,
        // measured). (Not the background of a table part drawn in a cell, a box of no element.)
        let element = matches!(&fragment.kind, FragmentKind::Box { node: Some(_), .. });
        let text_mask = style
            .clips_background_to_text()
            .then(|| self.text_mask(fragment, x, y, is_inline_box(style)))
            .filter(|mask| {
                !element
                    || mask.items.iter().any(|item| {
                        !is_group(item) && !matches!(item, Item::Text { glyphs, .. } if glyphs.is_empty())
                    })
            });
        let border_area = style.clips_background_to_border_area().then(|| {
            match radii.filter(|radii| corner::is_shaped(style, Some(radii))) {
                Some(curves) => corner::border_area(style, rect, &curves, *sides),
                None => background::border_area(style, rect, radii, *sides),
            }
        });
        let masks = background::ClipMasks {
            text: text_mask.as_ref(),
            border_area: border_area.as_ref(),
        };
        // The background of a cell with collapsed borders reaches halfway into them.
        let half_borders = collapsed_cell_background(style, placement, RectF { x, y, width, height });
        let (background_style, placement) = match &half_borders {
            Some((half, placement)) => (half, *placement),
            None => (&**style, placement),
        };
        let scrolled = scrolled_fixed_backgrounds(background_style, self.transforms > 0);
        let background_style = scrolled.as_ref().unwrap_or(background_style);
        // Fixed layers that scroll with a transformed box keep the box as laid out, unsnapped
        // (Chrome 157).
        let placement = if scrolled.is_some() {
            background::Placement {
                exact: None,
                ..placement
            }
        } else {
            placement
        };
        // A border along basic shapes (`border-shape`), of a box drawn whole.
        if let Some(shape) = style.border_shape.as_deref()
            && *sides == fagus_layout::VisibleSides::ALL
        {
            self.design(|painter| {
                border_shape::paint_box(
                    style,
                    background_style,
                    rect,
                    shape,
                    placement,
                    images,
                    masks,
                    &mut painter.items,
                );
            });
            return;
        }
        // Corners of other shapes than round (`corner-shape`).
        if let Some(curves) = radii.filter(|radii| corner::is_shaped(style, Some(radii))) {
            self.design(|painter| {
                let items = &mut painter.items;
                let border_image =
                    |items: &mut Vec<Item>| border_image::paint(style, rect, *sides, images, items);
                corner::paint_box(
                    style,
                    background_style,
                    rect,
                    &curves,
                    placement,
                    images,
                    masks,
                    |items| {
                        if !border_image(items)
                            && (*sides != fagus_layout::VisibleSides::ALL
                                || corner::styled_border(style, rect, &curves, items).is_none())
                        {
                            paint_borders(style, rect, None, *sides, items);
                        }
                    },
                    items,
                );
            });
            return;
        }
        // The shadows of a slice of a box broken across lines, columns or pages go on past
        // where it is cut; those of a box that clones its decorations are its own.
        let shadow_sides = if style.clones_decorations() {
            fagus_layout::VisibleSides::ALL
        } else {
            *sides
        };
        let shadows = |inset: bool, items: &mut Vec<Item>| {
            shadow::paint_slice(style, rect, radii, shadow_sides, inset, items);
        };
        self.design(|painter| {
            let items = &mut painter.items;
            shadows(false, items);
            background::paint_clipped(background_style, placement, images, masks, items);
            shadows(true, items);
            // A border image is drawn in place of the border.
            if !border_image::paint(style, rect, *sides, images, items) {
                paint_borders(style, rect, radii, *sides, items);
            }
        });
    }

    /// The mask of a background clipped to text (`background-clip: text`): the glyphs of the
    /// text of the box at `(x, y)`, opaque whatever their color, as Chrome paints them for it.
    /// The text of relatively positioned descendants and of descendants with their own
    /// stacking context adds to it at their places, without their transforms or opacity;
    /// that of absolutely positioned descendants and of transformed atomic inlines does not
    /// (Chrome 157, measured).
    fn text_mask(&mut self, fragment: &Fragment, x: f32, y: f32, inline: bool) -> Arc<MaskPaint> {
        let outer_items = std::mem::take(&mut self.items);
        let (outer_tag, outer_tagged) = (self.tag, self.tagged);
        let outer_mask = std::mem::replace(&mut self.text_mask, true);
        let outer_decorations = std::mem::take(&mut self.mask_decorations);
        self.tagged = false;
        if inline {
            self.inlines(fragment, x, y, true);
        } else {
            self.in_flow(fragment, x, y, None);
        }
        let painted = std::mem::replace(&mut self.items, outer_items);
        (self.tag, self.tagged) = (outer_tag, outer_tagged);
        self.text_mask = outer_mask;
        let decorations = std::mem::replace(&mut self.mask_decorations, outer_decorations);
        let items = painted
            .into_iter()
            .enumerate()
            .filter_map(|(index, item)| match item {
                Item::Text { .. } => Some(opaque_text(item)),
                Item::Rect { .. } | Item::Wave { .. } | Item::DashedLine { .. }
                    if decorations.iter().any(|range| range.contains(&index)) =>
                {
                    Some(opaque_text(item))
                }
                // Groups keep the glyphs where they are drawn, and their pushes and pops paired.
                Item::PushOpacity(_)
                | Item::PushBlend(_)
                | Item::PushIsolated
                | Item::PushLayer(_)
                | Item::PushTransform(_)
                | Item::PushClip(..)
                | Item::PushClipShape(_)
                | Item::PushMask(_)
                | Item::Pop => Some(item),
                _ => None,
            })
            .collect();
        Arc::new(MaskPaint {
            items,
            luminance: false,
        })
    }

    /// Paints a run of text whose box lies at `(x, y)` with `size`. A run in a vertical line is
    /// painted as in a horizontal one, turned a quarter onto it (CSS Writing Modes 4, §7.1).
    fn text(&mut self, run: &fagus_layout::TextRun, (x, y): (f32, f32), (width, height): (f32, f32)) {
        use fagus_layout::GlyphOrientation;
        if run.style.visibility != Visibility::Visible {
            return;
        }
        if let Some(symbol) = run.symbol {
            let items = &mut self.items;
            return paint_symbol(run, symbol, RectF { x, y, width, height }, items);
        }
        if run.orientation == GlyphOrientation::Horizontal {
            return self.line_text(run, x, y, width);
        }
        if run.orientation == GlyphOrientation::Combined {
            // Horizontal text in the middle of its box, squeezed where it is too wide.
            let natural: f32 = run.glyphs.iter().map(|glyph| glyph.advance).sum();
            let room = run.size * fagus_layout::COMBINED_ROOM;
            let scale = if natural > room { room / natural } else { 1.0 };
            let left = x + (width - natural * scale) / 2.0;
            if scale < 1.0 {
                self.items.push(Item::PushTransform([
                    scale,
                    0.0,
                    0.0,
                    1.0,
                    left * (1.0 - scale),
                    0.0,
                ]));
            }
            self.line_text(run, left, y, natural);
            if scale < 1.0 {
                self.items.push(Item::Pop);
            }
            return;
        }
        let turn = match run.orientation {
            GlyphOrientation::SidewaysLeft => [0.0, -1.0, 1.0, 0.0, x, y + height],
            _ => [0.0, 1.0, -1.0, 0.0, x + width, y],
        };
        self.items.push(Item::PushTransform(turn));
        self.line_text(run, 0.0, 0.0, height);
        self.items.push(Item::Pop);
    }

    /// Paints a run of text in the coordinates of its line, its box at `(x, y)` and `width`
    /// long.
    fn line_text(&mut self, run: &fagus_layout::TextRun, x: f32, y: f32, width: f32) {
        let baseline = y + run.baseline.to_px();
        let face = self.fonts.face(run.face);
        // Faces a family lacks are made up unless `font-synthesis` forbids it.
        let synthesis = run.style.font_synthesis;
        let synthetic = Synthetic {
            italic: synthesis.style
                && face.synthetic_italic(fagus_layout::text_font_style(run.style.font_style)),
            bold: synthesis.weight && face.synthetic_bold(run.style.font_weight),
        };
        let fonts = self.fonts;
        let variations = fagus_layout::run_variations(fonts, run);
        let palette = fonts.palette(run.face, &font_palette(&run.style.font_palette));
        // Emphasis marks follow the text, in their own color (CSS Text Decoration 3, §3).
        let annotation = self
            .ruby_annotation
            .filter(|(side, _)| *side == run.style.text_emphasis_position)
            .map(|(_, edge)| (edge - baseline).abs());
        let marks = if run.style.text_emphasis_style.is_some() && !run.marker {
            fagus_layout::emphasis_marks(fonts, run, annotation)
        } else {
            None
        };
        let mark_variations = marks.as_ref().map(|marks| marks.variations.clone());
        // Shadows lie below the text, its decorations and its emphasis marks, the first on top
        // (CSS Text Decoration 3, §4). A sharp shadow is all of them again, in its color.
        let upright = run.orientation == fagus_layout::GlyphOrientation::Upright;
        // Chrome finds the vertical origin of a glyph without one in the face from the top of
        // its outline, which synthetic bold thickens: the glyph moves down by 1/64 em.
        let upright_run = || {
            if synthetic.bold && face.vertical_metrics && !face.vertical_origins {
                let shift = run.size / 64.0;
                let shifted: Vec<Glyph> = run
                    .glyphs
                    .iter()
                    .map(|glyph| Glyph {
                        x_offset: glyph.x_offset + shift,
                        ..*glyph
                    })
                    .collect();
                upright_glyphs(&shifted)
            } else {
                upright_glyphs(&run.glyphs)
            }
        };
        if !run.style.text_shadow.is_empty() {
            self.design(|painter| {
                for shadow in run.style.text_shadow.iter().rev() {
                    let color = shadow.color.unwrap_or(run.style.color);
                    if color.is_transparent() {
                        continue;
                    }
                    // The shadow turns with a vertical line, but for that of upright glyphs,
                    // which keeps its offset on the page, as in Chrome.
                    let (shadow_x, shadow_y) = (x + shadow.x, baseline + shadow.y);
                    if shadow.blur > 0.0 {
                        let mut lines = Vec::new();
                        for below in [true, false] {
                            paint_decorations(run, shadow_x, shadow_y, width, fonts, below, &mut lines);
                        }
                        let lines: Vec<RectF> = lines
                            .iter()
                            .filter_map(|line| match line {
                                Item::Rect { rect, .. } => Some(*rect),
                                _ => None,
                            })
                            .collect();
                        if upright {
                            if !lines.is_empty() {
                                painter.items.push(Item::TextShadow {
                                    x: shadow_x,
                                    y: shadow_y,
                                    face: run.face,
                                    size: run.size,
                                    variations: variations.clone(),
                                    glyphs: Vec::new(),
                                    lines,
                                    color,
                                    blur: shadow.blur,
                                });
                            }
                            painter
                                .items
                                .push(Item::PushTransform([0.0, -1.0, 1.0, 0.0, x, baseline]));
                            painter.items.push(Item::TextShadow {
                                x: shadow.x,
                                y: shadow.y,
                                face: run.face,
                                size: run.size,
                                variations: variations.clone(),
                                glyphs: stacked_glyphs(&run.glyphs),
                                lines: Vec::new(),
                                color,
                                blur: shadow.blur,
                            });
                            painter.items.push(Item::Pop);
                            continue;
                        }
                        painter.items.push(Item::TextShadow {
                            x: shadow_x,
                            y: shadow_y,
                            face: run.face,
                            size: run.size,
                            variations: variations.clone(),
                            glyphs: run.glyphs.clone(),
                            lines,
                            color,
                            blur: shadow.blur,
                        });
                        if let (Some(marks), Some(mark_variations)) = (&marks, &mark_variations) {
                            painter.items.push(Item::TextShadow {
                                x: shadow_x + marks.x,
                                y: shadow_y + marks.offset,
                                face: marks.face,
                                size: marks.size,
                                variations: mark_variations.clone(),
                                glyphs: marks.glyphs.clone(),
                                lines: Vec::new(),
                                color,
                                blur: shadow.blur,
                            });
                        }
                        continue;
                    }
                    let start = painter.items.len();
                    for below in [true, false] {
                        paint_decorations(run, shadow_x, shadow_y, width, fonts, below, &mut painter.items);
                    }
                    for item in &mut painter.items[start..] {
                        if let Item::Rect { color: line, .. }
                        | Item::Wave { color: line, .. }
                        | Item::DashedLine { color: line, .. } = item
                        {
                            *line = color;
                        }
                    }
                    if upright {
                        painter
                            .items
                            .push(Item::PushTransform([0.0, -1.0, 1.0, 0.0, x, baseline]));
                        painter.items.push(Item::Text {
                            x: shadow.x,
                            y: shadow.y,
                            face: run.face,
                            size: run.size,
                            variations: variations.clone(),
                            color,
                            glyphs: upright_run(),
                            text: run.text.clone(),
                            synthetic,
                            vertical: true,
                            palette: None,
                        });
                        painter.items.push(Item::Pop);
                    } else {
                        painter.items.push(Item::Text {
                            x: shadow_x,
                            y: shadow_y,
                            face: run.face,
                            size: run.size,
                            variations: variations.clone(),
                            color,
                            glyphs: run.glyphs.clone(),
                            text: run.text.clone(),
                            synthetic,
                            vertical: false,
                            palette: None,
                        });
                    }
                    if let (Some(marks), Some(mark_variations)) = (&marks, &mark_variations) {
                        painter.items.push(Item::Text {
                            x: shadow_x + marks.x,
                            y: shadow_y + marks.offset,
                            face: marks.face,
                            size: marks.size,
                            variations: mark_variations.clone(),
                            color,
                            glyphs: marks.glyphs.clone(),
                            text: marks.text.clone(),
                            synthetic: Synthetic::default(),
                            vertical: false,
                            palette: None,
                        });
                    }
                }
            });
        }
        // Underlines and overlines lie below the text, lines through it above, as in browsers.
        self.decorations_of(|items| paint_decorations(run, x, baseline, width, fonts, true, items));
        if upright {
            // Upright glyphs stand as they are on the page, one below the other from the start
            // of the run, placed from its baseline.
            self.items
                .push(Item::PushTransform([0.0, -1.0, 1.0, 0.0, x, baseline]));
            self.items.push(Item::Text {
                x: 0.0,
                y: 0.0,
                face: run.face,
                size: run.size,
                variations,
                color: run.style.text_fill_color.unwrap_or(run.style.color),
                glyphs: upright_run(),
                text: run.text.clone(),
                synthetic,
                vertical: true,
                palette: palette.clone(),
            });
            self.items.push(Item::Pop);
        } else {
            self.items.push(Item::Text {
                x,
                y: baseline,
                face: run.face,
                size: run.size,
                variations,
                color: run.style.text_fill_color.unwrap_or(run.style.color),
                glyphs: run.glyphs.clone(),
                text: run.text.clone(),
                synthetic,
                vertical: false,
                palette,
            });
        }
        if let (Some(mut marks), Some(mark_variations)) = (marks, mark_variations) {
            let (mut mark_x, mut mark_y, mut vertical) = (x + marks.x, baseline + marks.offset, false);
            // In a vertical line the marks stand upright, each turned about the middle of its
            // ink, as Chrome draws them.
            let upright = on_central_baseline(run);
            if upright && let Some(first) = marks.glyphs.first() {
                let bounds = fonts.face(marks.face).glyph_bounds(first.id, marks.size);
                let [left, top, right, bottom] = bounds.unwrap_or([0.0; 4]);
                let (center_x, center_y) = (f32::midpoint(left, right), f32::midpoint(top, bottom));
                self.items.push(Item::PushTransform([
                    0.0,
                    -1.0,
                    1.0,
                    0.0,
                    mark_x + center_x,
                    mark_y + center_y,
                ]));
                for glyph in &mut marks.glyphs {
                    glyph.x_offset = -center_x;
                    glyph.y_offset = center_y;
                }
                (mark_x, mark_y, vertical) = (0.0, 0.0, true);
            }
            self.items.push(Item::Text {
                x: mark_x,
                y: mark_y,
                face: marks.face,
                size: marks.size,
                variations: mark_variations,
                color: run.style.text_emphasis_color.unwrap_or(run.style.color),
                glyphs: marks.glyphs,
                text: marks.text,
                synthetic: Synthetic::default(),
                vertical,
                palette: None,
            });
            if vertical {
                self.items.push(Item::Pop);
            }
        }
        self.decorations_of(|items| paint_decorations(run, x, baseline, width, fonts, false, items));
    }
}

/// A coordinate rounded to a whole pixel, halves up, as Chrome rounds layout units.
/// The style and placement of the background of a table cell with collapsed borders, as Chrome
/// paints it: its border box spans half of each border (CSS 2.1, §17.6.2), so that the
/// background reaches halfway under the borders. It shows where the borders let it through
/// (their colors are not opaque) and where the cell paints after them (a cell with a transform
/// or a position, which paints as a layer of its own). Fagus's cell spans the whole borders:
/// the area is shrunk, with half borders around the same padding box, and snapped to whole
/// pixels from the cell's unsnapped border box `exact`, as Chrome snaps the box it lays out
/// (a border of 1px leaves half a pixel, which Chrome's background covers), unless the cell is
/// transformed.
fn collapsed_cell_background(
    style: &ComputedStyle,
    placement: background::Placement,
    exact: RectF,
) -> Option<(ComputedStyle, background::Placement)> {
    use fagus_style::values::{BorderCollapse, Display, InternalDisplay};
    // (A cell without a background has nothing to paint there: its style is not copied.)
    if style.border_collapse != BorderCollapse::Collapse
        || style.display != Display::Internal(InternalDisplay::TableCell)
        || style.background_color.is_transparent()
            && style
                .background_image
                .iter()
                .all(|image| matches!(image, fagus_style::values::Image::None))
    {
        return None;
    }
    let widths = style.border_width;
    if [widths.top, widths.right, widths.bottom, widths.left]
        .iter()
        .all(|width| *width <= 0.0)
    {
        return None;
    }
    let mut half = style.clone();
    half.border_width = fagus_style::values::Sides {
        top: widths.top / 2.0,
        right: widths.right / 2.0,
        bottom: widths.bottom / 2.0,
        left: widths.left / 2.0,
    };
    let shrink = |area: RectF| RectF {
        x: area.x + half.border_width.left,
        y: area.y + half.border_width.top,
        width: (area.width - half.border_width.left - half.border_width.right).max(0.0),
        height: (area.height - half.border_width.top - half.border_width.bottom).max(0.0),
    };
    // A cell painted as a layer of its own (turned by a transform) keeps its box as laid out.
    let (border_box, whole) = if style.transform.is_empty() {
        (
            snap(shrink(exact)),
            placement.whole.map(|whole| snap(shrink(whole))),
        )
    } else {
        (shrink(placement.border_box), placement.whole.map(shrink))
    };
    Some((
        half,
        background::Placement {
            border_box,
            whole,
            exact: None,
            ..placement
        },
    ))
}

/// The border box of a box as Chrome lays it out: that of a table cell with collapsed borders
/// spans half of each border (CSS 2.1, §17.6.2), where Fagus's cell `rect` spans the whole
/// borders. Its outline goes around that box.
fn collapsed_cell_box(style: &ComputedStyle, rect: RectF) -> RectF {
    use fagus_style::values::{BorderCollapse, Display, InternalDisplay};
    if style.border_collapse != BorderCollapse::Collapse
        || style.display != Display::Internal(InternalDisplay::TableCell)
    {
        return rect;
    }
    let widths = style.border_width;
    RectF {
        x: rect.x + widths.left / 2.0,
        y: rect.y + widths.top / 2.0,
        width: (rect.width - f32::midpoint(widths.left, widths.right)).max(0.0),
        height: (rect.height - f32::midpoint(widths.top, widths.bottom)).max(0.0),
    }
}

/// Whether a box keeps the 3D space of its transform for its children (`transform-style:
/// preserve-3d`), which the properties that group what it draws (overflow clips, `opacity`,
/// filters, clips, masks, isolation, blending) flatten (CSS Transforms 2, §4).
fn keeps_3d(style: &ComputedStyle, fragment: &Fragment) -> bool {
    use fagus_style::values::{BlendMode, Isolation, Overflow};
    style.preserve_3d
        && style.overflow_x == Overflow::Visible
        && style.overflow_y == Overflow::Visible
        && style.opacity >= 1.0
        && style.filter.is_empty()
        && style.clip_path.is_none()
        && style.mask.as_ref().is_none_or(|mask| mask.is_none())
        && style.isolation != Isolation::Isolate
        && style.mix_blend_mode == BlendMode::Normal
        && css_clip(fragment, 0.0, 0.0).is_none()
}

/// Whether an item begins or ends a group, a transform, a clip or a mask rather than drawing.
pub(crate) fn is_group(item: &Item) -> bool {
    matches!(
        item,
        Item::PushOpacity(_)
            | Item::PushBlend(_)
            | Item::PushIsolated
            | Item::PushLayer(_)
            | Item::PushTransform(_)
            | Item::PushClip(..)
            | Item::PushClipShape(_)
            | Item::PushMask(_)
            | Item::Pop
    )
}

/// A run of glyphs or a decoration line drawn opaque, for a mask of its shapes.
fn opaque_text(mut item: Item) -> Item {
    if let Item::Text { color, .. }
    | Item::Rect { color, .. }
    | Item::Wave { color, .. }
    | Item::DashedLine { color, .. } = &mut item
    {
        *color = Rgba {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
    }
    item
}

fn round(value: f32) -> f32 {
    (value + 0.5).floor()
}

/// A rectangle with its edges rounded to whole pixels, halves up, as Chrome snaps the
/// rectangles it paints boxes into (`PhysicalRect::PixelSnappedRect`).
/// The transform that stretches `rect` to the rectangle it snaps to (see [`snap`]); `None`
/// where they are the same, or `rect` is empty.
fn stretch_to_snapped(rect: RectF) -> Option<[f32; 6]> {
    let snapped = snap(rect);
    if snapped == rect || rect.width <= 0.0 || rect.height <= 0.0 {
        return None;
    }
    let (sx, sy) = (snapped.width / rect.width, snapped.height / rect.height);
    Some([sx, 0.0, 0.0, sy, snapped.x - rect.x * sx, snapped.y - rect.y * sy])
}

fn snap(rect: RectF) -> RectF {
    let (left, top) = (round(rect.x), round(rect.y));
    RectF {
        x: left,
        y: top,
        width: round(rect.x + rect.width) - left,
        height: round(rect.y + rect.height) - top,
    }
}

/// The top on the page of the box of a line at `y`, which Chrome rounds its text by: the line's
/// top, or in a vertical line where its content starts down the page.
fn line_top(line: &Fragment, y: f32) -> f32 {
    if !is_vertical_line(line) {
        return y;
    }
    line.children
        .iter()
        .filter(|child| match &child.kind {
            // Not a marker outside the line.
            FragmentKind::Text(run) => !run.marker || child.rect.y >= Au::ZERO,
            FragmentKind::Box { .. } => !is_float(child) && !is_layered(child),
            _ => false,
        })
        .map(|child| {
            // The line starts before the margin of an inline box at its start.
            let margin = match &child.kind {
                FragmentKind::Box { style, sides, .. } if sides.top && is_inline_box(style) => {
                    match &style.margin.top {
                        fagus_style::values::LengthPercentageAuto::Length(length) => length.resolve(0.0),
                        fagus_style::values::LengthPercentageAuto::Auto => 0.0,
                    }
                }
                _ => 0.0,
            };
            y + child.rect.y.to_px() - margin
        })
        .reduce(f32::min)
        .unwrap_or(y)
}

/// Whether a line is a vertical one: whether its first text is set along it.
fn is_vertical_line(line: &Fragment) -> bool {
    fn vertical(fragment: &Fragment) -> Option<bool> {
        fragment.children.iter().find_map(|child| match &child.kind {
            FragmentKind::Text(run) => Some(run.orientation != fagus_layout::GlyphOrientation::Horizontal),
            FragmentKind::Box { style, .. } if is_inline_box(style) => vertical(child),
            _ => None,
        })
    }
    vertical(line) == Some(true)
}

/// Whether an inline box has a fragment of its own in Chrome, which rounds the text in it by its
/// top (Blink's `LayoutInline::ComputeInitialShouldCreateBoxFragment`): one with a background,
/// borders, padding, margins, shadows or an outline, or one that is positioned or transformed.
/// Whether a style gives its box a margin on any side.
/// Whether a box is a positioned table row or row group, whose fragment (see `fagus_layout`'s
/// tables) draws nothing itself: its cells draw its background and the table its outline.
fn is_table_row_or_group(style: &ComputedStyle) -> bool {
    use fagus_style::values::{Display, InternalDisplay};
    style.is_positioned()
        && matches!(
            style.display,
            Display::Internal(
                InternalDisplay::TableRow
                    | InternalDisplay::TableRowGroup
                    | InternalDisplay::TableHeaderGroup
                    | InternalDisplay::TableFooterGroup
            )
        )
}

fn has_margin(style: &ComputedStyle) -> bool {
    use fagus_style::values::{LengthPercentage, LengthPercentageAuto};
    let margin = &style.margin;
    ![&margin.top, &margin.right, &margin.bottom, &margin.left]
        .into_iter()
        .all(|side| matches!(side, LengthPercentageAuto::Length(LengthPercentage::Px(px) | LengthPercentage::Percent(px)) if *px == 0.0))
}

fn has_box_fragment(style: &ComputedStyle) -> bool {
    use fagus_style::values::{Image, LengthPercentage, LengthPercentageAuto, OutlineStyle};
    let zero = |length: &LengthPercentage| matches!(length, LengthPercentage::Px(px) | LengthPercentage::Percent(px) if *px == 0.0);
    let padding = &style.padding;
    let margin = &style.margin;
    let no_margin = |margin: &LengthPercentageAuto| matches!(margin, LengthPercentageAuto::Length(length) if zero(length));
    let border = &style.border_width;
    !style.background_color.is_transparent()
        || style
            .background_image
            .iter()
            .any(|image| !matches!(image, Image::None))
        || [border.top, border.right, border.bottom, border.left]
            .iter()
            .any(|width| *width > 0.0)
        || ![&padding.top, &padding.right, &padding.bottom, &padding.left]
            .into_iter()
            .all(zero)
        || ![&margin.top, &margin.right, &margin.bottom, &margin.left]
            .into_iter()
            .all(no_margin)
        || !style.box_shadow.is_empty()
        || !matches!(style.outline_style, OutlineStyle::Line(BorderStyle::None)) && style.outline_width > 0.0
        || style.is_positioned()
        || style.has_transform()
}

/// Paints the symbol of a list marker whose run's box is `rect` on the page, as Blink does
/// (`TextFragmentPainter::PaintSymbol`): a disc, circle or square in its square snapped to whole
/// pixels, the circle stroked a pixel wide, or a disclosure triangle in the square as it is.
fn paint_symbol(
    run: &fagus_layout::TextRun,
    symbol: fagus_layout::MarkerSymbol,
    rect: RectF,
    items: &mut Vec<Item>,
) {
    use fagus_layout::{GlyphOrientation, SymbolShape};
    use fagus_style::values::WritingMode;
    let size = symbol.size;
    // The square on the page: in a vertical line from its over side, the right one (or the left
    // one in `sideways-lr`, where Blink places it a line's length above the run's box).
    let (left, top) = match run.orientation {
        GlyphOrientation::Horizontal => (rect.x + symbol.x, rect.y + symbol.y),
        GlyphOrientation::SidewaysLeft => (rect.x + symbol.y, rect.y - symbol.x),
        _ => (rect.x + rect.width - symbol.y - size, rect.y + symbol.x),
    };
    let square = RectF {
        x: left,
        y: top,
        width: size,
        height: size,
    };
    let color = run.style.color;
    let snapped = snap(square);
    let round = |rect: RectF| Some([(rect.width / 2.0, rect.height / 2.0); 4]);
    match symbol.shape {
        SymbolShape::Disc => items.push(Item::Rect {
            rect: snapped,
            color,
            radii: round(snapped),
        }),
        SymbolShape::Square => items.push(Item::Rect {
            rect: snapped,
            color,
            radii: None,
        }),
        SymbolShape::Circle => {
            let grow = |by: f32| RectF {
                x: snapped.x - by,
                y: snapped.y - by,
                width: (snapped.width + 2.0 * by).max(0.0),
                height: (snapped.height + 2.0 * by).max(0.0),
            };
            let (outer, inner) = (grow(0.5), grow(-0.5));
            items.push(Item::RoundedBorder {
                outer,
                outer_radii: round(outer).unwrap_or_default(),
                inner,
                inner_radii: round(inner).unwrap_or_default(),
                color,
                stroke: None,
            });
        }
        SymbolShape::Disclosure { open } => {
            // Open, it points to the end of the block; closed, to the end of the line.
            let style = &run.style;
            let (right, down) = if open {
                match style.writing_mode {
                    WritingMode::HorizontalTb => (None, Some(true)),
                    WritingMode::VerticalRl | WritingMode::SidewaysRl => (Some(false), None),
                    WritingMode::VerticalLr | WritingMode::SidewaysLr => (Some(true), None),
                }
            } else {
                match style.writing_mode {
                    WritingMode::HorizontalTb => (Some(!style.is_rtl()), None),
                    WritingMode::SidewaysLr => (None, Some(style.is_rtl())),
                    _ => (None, Some(!style.is_rtl())),
                }
            };
            let points: [(f32, f32); 3] = match (right, down) {
                (Some(false), _) => [(1.0, 0.0), (0.14, 0.5), (1.0, 1.0)],
                (Some(true), _) => [(0.0, 0.0), (0.86, 0.5), (0.0, 1.0)],
                (None, Some(false)) => [(0.0, 0.93), (0.5, 0.07), (1.0, 0.93)],
                (None, _) => [(0.0, 0.07), (0.5, 0.93), (1.0, 0.07)],
            };
            let [first, second, third] = points.map(|(px, py)| (left + px * size, top + py * size));
            items.push(Item::BorderSide {
                points: [first, second, third, third],
                color,
                style: BorderStyle::Solid,
                width: size,
            });
        }
    }
}

/// The side of the annotation of a ruby column at `y` on the page, and how far it reaches there.
fn ruby_annotation(column: &Fragment, y: f32) -> Option<(AnnotationSide, f32)> {
    column.children.iter().find_map(|child| match &child.kind {
        FragmentKind::Box { style, .. } if style.ruby_display == RubyDisplay::Annotation => {
            let top = y + child.rect.y.to_px();
            Some(match style.ruby_position {
                AnnotationSide::Over => (AnnotationSide::Over, top),
                AnnotationSide::Under => (AnnotationSide::Under, top + child.rect.height.to_px()),
            })
        }
        _ => None,
    })
}

/// The page position of a fragment whose parent is at `origin`.
fn position(fragment: &Fragment, origin: (f32, f32)) -> (f32, f32) {
    (
        origin.0 + fragment.rect.x.to_px(),
        origin.1 + fragment.rect.y.to_px(),
    )
}

/// The index of the table box among the children of `fragment`, where it is a table wrapper
/// box: anonymous, with captions (blocks) beside the table box.
fn wrapped_table(fragment: &Fragment) -> Option<usize> {
    use fagus_style::values::{Display, DisplayInside};
    if !matches!(fragment.kind, FragmentKind::Anonymous) || fragment.children.len() < 2 {
        return None;
    }
    let mut tables = fragment.children.iter().enumerate().filter(|(_, child)| {
        matches!(&child.kind, FragmentKind::Box { style, .. }
            if matches!(style.display, Display::Normal { inside: DisplayInside::Table, .. }))
    });
    match (tables.next(), tables.next()) {
        (Some((index, _)), None) => Some(index),
        _ => None,
    }
}

/// Whether a style belongs to an inline box (`display: inline`), not an atomic inline.
fn is_inline_box(style: &ComputedStyle) -> bool {
    matches!(
        style.display,
        fagus_style::values::Display::Normal {
            outside: fagus_style::values::DisplayOutside::Inline,
            inside: fagus_style::values::DisplayInside::Flow,
            ..
        }
    )
}

/// Whether a fragment is a flex or grid container, whose items paint atomically.
fn holds_items(fragment: &Fragment) -> bool {
    matches!(
        &fragment.kind,
        FragmentKind::Box { style, .. } if matches!(
            style.display,
            fagus_style::values::Display::Normal {
                inside: fagus_style::values::DisplayInside::Flex | fagus_style::values::DisplayInside::Grid,
                ..
            }
        )
    )
}

/// Whether a fragment is a float, painted between the blocks and the inline content.
fn is_float(fragment: &Fragment) -> bool {
    matches!(&fragment.kind, FragmentKind::Box { style, .. } if style.is_floating())
        || wrapped_table(fragment).is_some_and(|index| is_float(&fragment.children[index]))
}

/// Whether a fragment is painted in a later phase of its stacking context instead of in flow.
/// Whether a box in the stacking context of `fragment` (not in one inside it) blends with
/// what is below it (`mix-blend-mode`).
fn blends_inside(fragment: &Fragment) -> bool {
    fragment.children.iter().any(|child| match &child.kind {
        FragmentKind::Box { style, .. } => {
            style.mix_blend_mode != fagus_style::values::BlendMode::Normal
                || (!style.is_stacking_context() && blends_inside(child))
        }
        _ => blends_inside(child),
    })
}

fn is_layered(fragment: &Fragment) -> bool {
    matches!(&fragment.kind, FragmentKind::Box { style, .. } if style.is_positioned() || style.is_stacking_context())
}

/// Collects the positioned descendants and stacking contexts that belong to the stacking context
/// of `fragment` (at `origin`), with the clips of the ancestors in between. `node` is the closest
/// element around the children.
/// Whether `inner` is `outer` or lies in it.
fn contains(outer: &Fragment, inner: &Fragment) -> bool {
    std::ptr::eq(outer, inner) || outer.children.iter().any(|child| contains(child, inner))
}

/// `layers` in tree order: an absolutely positioned or fixed box goes in the fragment of its
/// containing block after the content in flow, which Chrome paints after it only where it
/// follows in the document. Each moves before the first layer of an element after it (by node,
/// in the order the document was parsed) that follows the last layer of an element before it
/// (layers of later elements can come first where an absolutely positioned box is broken across
/// columns). A `::before` pseudo-element goes by its element, before the element's children; any
/// other box of no element of its own (a generated or anonymous box) goes by the element around
/// it.
fn in_tree_order(layers: Vec<Layer<'_>>) -> Vec<Layer<'_>> {
    let own = |layer: &Layer<'_>| match &layer.fragment.kind {
        FragmentKind::Box { node, style, .. } => node.or(style.before_of).or(layer.node),
        _ => None,
    };
    let mut ordered: Vec<Layer<'_>> = Vec::with_capacity(layers.len());
    for layer in layers {
        let placed = match &layer.fragment.kind {
            FragmentKind::Box { style, node, .. }
                if style.is_out_of_flow()
                    && !style.is_floating()
                    && let Some(node) = node.or(style.before_of) =>
            {
                let after = ordered
                    .iter()
                    .rposition(|other| own(other).is_some_and(|other| other <= node))
                    .map_or(0, |index| index + 1);
                ordered[after..]
                    .iter()
                    .position(|other| own(other).is_some_and(|other| other > node))
                    .map(|index| after + index)
            }
            _ => None,
        };
        match placed {
            Some(index) => ordered.insert(index, layer),
            None => ordered.push(layer),
        }
    }
    ordered
}

fn collect_layers<'f>(
    fragment: &'f Fragment,
    origin: (f32, f32),
    clips: &mut Vec<Clip>,
    (node, perspective): (Option<NodeId>, Option<transform::Perspective>),
    layers: &mut Vec<Layer<'f>>,
) {
    // An inline `<svg>` is drawn moved as far as its border box moves to whole pixels, and the
    // boxes of its `<foreignObject>` elements with it (Blink's `SVGRootPainter` snaps the paint
    // offset of all it holds).
    let svg = fragment.children.iter().find_map(|child| match &child.kind {
        FragmentKind::Image(image) if image.inline => {
            Some((round(origin.0) - origin.0, round(origin.1) - origin.1))
        }
        _ => None,
    });
    for child in &fragment.children {
        let child_origin = position(child, origin);
        if let FragmentKind::Box { style, node: own, .. } = &child.kind {
            let node = own.or(node);
            let origin = match svg {
                Some((dx, dy)) if style.is_stacking_context() => (origin.0 + dx, origin.1 + dy),
                _ => origin,
            };
            let css = css_clip(child, child_origin.0, child_origin.1);
            // A box clipped to nothing (`clip: rect(0, 0, 0, 0)`, as in the `sr-only` and
            // `visually-hidden` classes of Tailwind and Bootstrap) is not painted at all, its
            // text not either, as in Chrome.
            if css.is_some_and(|clip| clip.width <= 0.0 || clip.height <= 0.0) {
                continue;
            }
            // A box is seen in the perspective of its parent element only, not in that of the
            // box it is laid out in where that is another (its containing block, or the block
            // around the inline box it is in), as in Chrome.
            let perspective = perspective.filter(|_| style.parent_perspective);
            if style.is_stacking_context() {
                layers.push(Layer {
                    fragment: child,
                    node,
                    origin,
                    clips: clips.clone(),
                    z: style.z_index.unwrap_or(0),
                    context: true,
                    perspective,
                    isolated: false,
                });
                continue;
            }
            if style.is_positioned() {
                layers.push(Layer {
                    fragment: child,
                    node,
                    origin,
                    clips: clips.clone(),
                    z: 0,
                    context: false,
                    perspective,
                    isolated: false,
                });
            }
            // The clip of `clip` goes around the overflow clip of the same box.
            let clip = clip_rect(child, child_origin.0, child_origin.1);
            let count = clips.len();
            clips.extend(css.map(Clip::rect));
            clips.extend(clip);
            collect_layers(
                child,
                child_origin,
                clips,
                (node, perspective_of(child, child_origin)),
                layers,
            );
            clips.truncate(count);
        } else {
            // Lines and anonymous boxes pass their box's perspective on to its children.
            collect_layers(child, child_origin, clips, (node, perspective), layers);
        }
    }
}

/// The perspective a box at `(x, y)` gives its children (`perspective`, around
/// `perspective-origin`).
fn perspective_of(fragment: &Fragment, (x, y): (f32, f32)) -> Option<transform::Perspective> {
    let FragmentKind::Box { style, .. } = &fragment.kind else {
        return None;
    };
    style.perspective.map(|distance| transform::Perspective {
        distance,
        origin: (
            x + style.perspective_origin.0.resolve(fragment.rect.width.to_px()),
            y + style.perspective_origin.1.resolve(fragment.rect.height.to_px()),
        ),
    })
}

/// The clip of an absolutely positioned box with `clip: rect(…)` at `(x, y)` (CSS Masking 1,
/// §6.1): its edges are offsets from the top and left border edges, `auto` the border edges
/// themselves. It clips the box itself, its border and background with the rest.
fn css_clip(fragment: &Fragment, x: f32, y: f32) -> Option<RectF> {
    let FragmentKind::Box { style, .. } = &fragment.kind else {
        return None;
    };
    let [top, right, bottom, left] = style.clip?;
    if !matches!(
        style.position,
        fagus_style::values::Position::Absolute | fagus_style::values::Position::Fixed
    ) {
        return None;
    }
    let (width, height) = (fragment.rect.width.to_px(), fragment.rect.height.to_px());
    let (top, left) = (top.unwrap_or(0.0), left.unwrap_or(0.0));
    let (right, bottom) = (right.unwrap_or(width), bottom.unwrap_or(height));
    Some(RectF {
        x: x + left,
        y: y + top,
        width: (right - left).max(0.0),
        height: (bottom - top).max(0.0),
    })
}

/// The style of a box in a transformed box (or one itself) whose fixed backgrounds scroll with it
/// (CSS Transforms 1, §6: a transform makes fixed backgrounds act as `scroll`, as in Chrome);
/// `None` where nothing changes.
fn scrolled_fixed_backgrounds(style: &ComputedStyle, transformed: bool) -> Option<ComputedStyle> {
    (transformed && style.background_fixed.iter().any(|fixed| *fixed)).then(|| {
        ComputedStyle::from(ComputedValues {
            background_fixed: Arc::from([false]),
            ..ComputedValues::from(style.clone())
        })
    })
}

/// What a clip is: a rectangle, with rounded corners or not.
/// The clip of a box's overflow: a rectangle with rounded corners, or the outline of corners
/// of other shapes (`corner-shape`).
#[derive(Clone, Debug)]
struct Clip {
    rect: RectF,
    radii: Option<Radii>,
    /// What clips in place of the rectangle: an outline (filled by the even-odd rule), or the
    /// clip of the inner edge of shaped corners.
    shape: Option<Arc<ClipShapeItem>>,
}

/// A clip of a [`Clip`] other than a rectangle.
#[derive(Debug)]
enum ClipShapeItem {
    Outline(Vec<fagus_layout::PathSegment>),
    Item(Item),
}

impl Clip {
    fn rect(rect: RectF) -> Self {
        Self {
            rect,
            radii: None,
            shape: None,
        }
    }

    fn item(&self) -> Item {
        match self.shape.as_deref() {
            Some(ClipShapeItem::Outline(segments)) => Item::PushClipShape(ClipGeometry::Path {
                segments: segments.clone(),
                even_odd: true,
            }),
            Some(ClipShapeItem::Item(item)) => item.clone(),
            None => Item::PushClip(self.rect, self.radii),
        }
    }
}

/// How far a clip reaches beyond a box along an axis it does not clip in (see [`clip_rect`]).
const UNCLIPPED: f32 = 1.0e6;

/// The clip of a box whose overflow is not visible: its padding box, rounded by the curves of
/// its padding edge (CSS Backgrounds 3, §5.3). The part of a box broken across pages has no
/// border where it is broken. A table clips at its border box (without its captions), as in
/// Chrome.
fn clip_rect(fragment: &Fragment, x: f32, y: f32) -> Option<Clip> {
    use fagus_style::values::Overflow;
    let FragmentKind::Box { style, sides, .. } = &fragment.kind else {
        return None;
    };
    if style.overflow_x == Overflow::Visible && style.overflow_y == Overflow::Visible {
        return None;
    }
    // A replaced element clips what it shows itself (see `clip_margin::replaced_clip`); one
    // without a margin is clipped at its padding box too, which cuts nothing, as before
    // `overflow-clip-margin` was taken (not an inline `<svg>`, which clipped nothing).
    let image = fragment.children.iter().find_map(|child| match &child.kind {
        FragmentKind::Image(image) => Some(image),
        _ => None,
    });
    if image.is_some_and(|image| {
        use fagus_style::values::BoxArea;
        image.inline
            || !matches!(
                style.overflow_clip_margin,
                (BoxArea::ContentBox | BoxArea::PaddingBox, margin) if margin == 0.0
            )
    }) {
        return None;
    }
    let table = matches!(
        style.display,
        fagus_style::values::Display::Normal {
            inside: fagus_style::values::DisplayInside::Table,
            ..
        }
    );
    let shown = |visible: bool, width: f32| if visible && !table { width } else { 0.0 };
    let border = fagus_style::values::Sides {
        top: shown(sides.top, style.border_width.top),
        right: shown(sides.right, style.border_width.right),
        bottom: shown(sides.bottom, style.border_width.bottom),
        left: shown(sides.left, style.border_width.left),
    };
    let (width, height) = (fragment.rect.width.to_px(), fragment.rect.height.to_px());
    // Chrome clips to the padding box snapped to whole pixels, as it snaps the boxes it paints
    // (Blink's `ToSnappedClipRect`).
    let mut rect = snap(RectF {
        x: x + border.left,
        y: y + border.top,
        width: (width - border.left - border.right).max(0.0),
        height: (height - border.top - border.bottom).max(0.0),
    });
    // `overflow: clip` in one axis only (the other `visible`) clips in that axis alone (CSS
    // Overflow 3, §3.1): the clip reaches as far as anything may in the other. Beside `hidden`,
    // `auto` or `scroll`, `visible` computes to `auto` and clips too.
    let unclipped = (
        style.overflow_x == Overflow::Visible && style.overflow_y == Overflow::Clip,
        style.overflow_y == Overflow::Visible && style.overflow_x == Overflow::Clip,
    );
    if unclipped.0 {
        rect.x -= UNCLIPPED;
        rect.width += 2.0 * UNCLIPPED;
    }
    if unclipped.1 {
        rect.y -= UNCLIPPED;
        rect.height += 2.0 * UNCLIPPED;
    }
    if unclipped.0 || unclipped.1 {
        return Some(Clip {
            rect,
            radii: None,
            shape: None,
        });
    }
    let insets = [border.top, border.right, border.bottom, border.left];
    let shown = [sides.top, sides.right, sides.bottom, sides.left];
    let border_box = RectF { x, y, width, height };
    if !table
        && let Some((clip, curves)) = clip_margin::box_clip(
            style,
            insets,
            shown,
            border_box,
            fragment_radii(style, width, height, *sides),
        )
    {
        // Corners of other shapes (`corner-shape`) clip along the outline of the border box
        // inset (or outset) to the clip, as the inner edge of a border.
        let shape = radii(style, width, height)
            .filter(|outer| corner::is_shaped(style, Some(outer)))
            .map(|outer| {
                let insets = [
                    clip.y - y,
                    x + width - clip.x - clip.width,
                    y + height - clip.y - clip.height,
                    clip.x - x,
                ];
                Arc::new(ClipShapeItem::Item(
                    corner::inner_edge(border_box, &outer, insets, style).clip(),
                ))
            });
        return Some(Clip {
            rect: snap(clip),
            radii: curves,
            shape,
        });
    }
    // A border along shapes (`border-shape`) clips along its shape.
    if let Some(segments) = style
        .border_shape
        .as_deref()
        .and_then(|shape| border_shape::clip_outline(style, border_box, shape))
    {
        return Some(Clip {
            rect,
            radii: None,
            shape: Some(Arc::new(ClipShapeItem::Outline(segments))),
        });
    }
    let outer = radii(style, width, height);
    let curves = outer.and_then(|radii| inner_radii(radii, insets));
    // Corners of other shapes clip along the inner edge of the border.
    let shape = outer
        .filter(|radii| corner::is_shaped(style, Some(radii)))
        .map(|radii| {
            let border_box = RectF { x, y, width, height };
            Arc::new(ClipShapeItem::Item(
                corner::inner_edge(border_box, &radii, insets, style).clip(),
            ))
        });
    Some(Clip {
        rect,
        radii: curves,
        shape,
    })
}

/// Whether `inner` lies within `outer` (to a hundredth of a pixel).
fn covers(outer: RectF, inner: RectF) -> bool {
    inner.x >= outer.x - 0.01
        && inner.y >= outer.y - 0.01
        && inner.x + inner.width <= outer.x + outer.width + 0.01
        && inner.y + inner.height <= outer.y + outer.height + 0.01
}

/// Whether a box's border hides all that lies under it: four solid opaque sides, drawn.
fn opaque_border(style: &ComputedStyle, sides: fagus_layout::VisibleSides) -> bool {
    let border = (
        [
            style.border_width.top,
            style.border_width.right,
            style.border_width.bottom,
            style.border_width.left,
        ],
        [
            style.border_style.top,
            style.border_style.right,
            style.border_style.bottom,
            style.border_style.left,
        ],
        [
            style.border_color.top,
            style.border_color.right,
            style.border_color.bottom,
            style.border_color.left,
        ],
    );
    sides.all()
        && style
            .border_image
            .as_ref()
            .is_none_or(|image| matches!(image.source, fagus_style::values::Image::None))
        && (0..4).all(|side| {
            border.0[side] > 0.0
                && border.1[side] == BorderStyle::Solid
                && style.border_color(border.2[side]).a >= 1.0
        })
}

/// The rounded corners of a fragment of a box broken across lines, columns or pages: where a
/// side is cut off, its corners are square, as the box is drawn whole and sliced (CSS
/// Fragmentation 3, §5.4).
fn sliced_radii(radii: Option<Radii>, sides: fagus_layout::VisibleSides) -> Option<Radii> {
    let mut radii = radii?;
    let shown = [
        sides.top && sides.left,
        sides.top && sides.right,
        sides.bottom && sides.right,
        sides.bottom && sides.left,
    ];
    for (radius, shown) in radii.iter_mut().zip(shown) {
        if !shown {
            *radius = (0.0, 0.0);
        }
    }
    radii.iter().any(|(x, y)| *x > 0.0 && *y > 0.0).then_some(radii)
}

/// The curves of an edge `insets` (top, right, bottom, left) inside one with rounded corners
/// of `radii`; `None` where all its corners are square.
fn inner_radii(radii: Radii, [top, right, bottom, left]: [f32; 4]) -> Option<Radii> {
    let inner = [
        ((radii[0].0 - left).max(0.0), (radii[0].1 - top).max(0.0)),
        ((radii[1].0 - right).max(0.0), (radii[1].1 - top).max(0.0)),
        ((radii[2].0 - right).max(0.0), (radii[2].1 - bottom).max(0.0)),
        ((radii[3].0 - left).max(0.0), (radii[3].1 - bottom).max(0.0)),
    ];
    inner.iter().any(|(x, y)| *x > 0.0 && *y > 0.0).then_some(inner)
}

/// The rounded corners of a fragment of `width` by `height` of a box broken across lines,
/// columns or pages: the corners of the sides it is cut off at are square before the others are
/// scaled down to fit, so that they keep their radii in a slice shorter than the box drawn whole
/// (Blink's `CalcRadiiFor` and `ConstrainRadii`, Chrome 157).
fn fragment_radii(
    style: &ComputedStyle,
    width: f32,
    height: f32,
    sides: fagus_layout::VisibleSides,
) -> Option<Radii> {
    if sides.all() {
        return radii(style, width, height);
    }
    let mut corners = style.border_radius.clone();
    let square = || {
        (
            fagus_style::values::LengthPercentage::ZERO,
            fagus_style::values::LengthPercentage::ZERO,
        )
    };
    if !(sides.top && sides.left) {
        corners.top_left = square();
    }
    if !(sides.top && sides.right) {
        corners.top_right = square();
    }
    if !(sides.bottom && sides.right) {
        corners.bottom_right = square();
    }
    if !(sides.bottom && sides.left) {
        corners.bottom_left = square();
    }
    sliced_radii(radii_of(style, &corners, width, height), sides)
}

fn radii(style: &ComputedStyle, width: f32, height: f32) -> Option<Radii> {
    radii_of(style, &style.border_radius, width, height)
}

/// The radii of `corners` of a box of `style` and `width` by `height`.
fn radii_of(
    style: &ComputedStyle,
    corners: &fagus_style::values::Corners,
    width: f32,
    height: f32,
) -> Option<Radii> {
    let mut radii = corner_radii(corners, width, height)?;
    // Concave corners (`corner-shape`) are scaled down further where opposite ones would meet.
    if style.corner_shape.iter().any(|shape| *shape < 1.0) {
        let factor = corner::opposite_scale(width, height, &radii, style);
        if factor < 1.0 {
            for radius in &mut radii {
                radius.0 *= factor;
                radius.1 *= factor;
            }
        }
    }
    Some(radii)
}

/// The radii of rounded corners in a box of `width` by `height`, scaled down where they would
/// overlap; `None` for square corners.
fn corner_radii(corners: &fagus_style::values::Corners, width: f32, height: f32) -> Option<Radii> {
    if corners.is_zero() {
        return None;
    }
    let corner = |(x, y): &(
        fagus_style::values::LengthPercentage,
        fagus_style::values::LengthPercentage,
    )| { (x.resolve(width).max(0.0), y.resolve(height).max(0.0)) };
    let mut radii = [
        corner(&corners.top_left),
        corner(&corners.top_right),
        corner(&corners.bottom_right),
        corner(&corners.bottom_left),
    ];
    // Scale down overlapping radii (CSS Backgrounds 3, §5.5).
    let factor = [
        width / (radii[0].0 + radii[1].0),
        height / (radii[1].1 + radii[2].1),
        width / (radii[2].0 + radii[3].0),
        height / (radii[3].1 + radii[0].1),
    ]
    .into_iter()
    .filter(|factor| factor.is_finite())
    .fold(1.0_f32, f32::min);
    if factor < 1.0 {
        for radius in &mut radii {
            radius.0 *= factor;
            radius.1 *= factor;
        }
    }
    Some(radii)
}

/// The clip of a `clip-path` for a box with `border_box` (CSS Masking 1, §6; CSS Shapes 1,
/// §3): its shape in the reference box, percentages of whose size.
fn clip_item(style: &ComputedStyle, clip: &fagus_style::values::ClipPath, border_box: RectF) -> Item {
    use fagus_style::values::{ClipShape, ShapeRadius};
    let border_radii = radii(style, border_box.width, border_box.height);
    let (reference, reference_radii) = background::area(style, border_box, border_radii, clip.reference);
    let (width, height) = (reference.width, reference.height);
    let point = |(x, y): &(
        fagus_style::values::LengthPercentage,
        fagus_style::values::LengthPercentage,
    )| { (reference.x + x.resolve(width), reference.y + y.resolve(height)) };
    match &clip.shape {
        // Corners of other shapes (`corner-shape`) clip along the outline of the border box,
        // inset to the reference box as the inner edge of a border.
        ClipShape::Box
            if let Some(outer) = border_radii.filter(|outer| corner::is_shaped(style, Some(outer))) =>
        {
            let insets = [
                reference.y - border_box.y,
                border_box.x + border_box.width - reference.x - reference.width,
                border_box.y + border_box.height - reference.y - reference.height,
                reference.x - border_box.x,
            ];
            corner::inner_edge(border_box, &outer, insets, style).clip()
        }
        ClipShape::Box => Item::PushClip(reference, reference_radii),
        ClipShape::Inset { insets, radii } => {
            let [top, right, bottom, left] = [
                insets[0].resolve(height),
                insets[1].resolve(width),
                insets[2].resolve(height),
                insets[3].resolve(width),
            ];
            let rect = RectF {
                x: reference.x + left,
                y: reference.y + top,
                width: (width - left - right).max(0.0),
                height: (height - top - bottom).max(0.0),
            };
            Item::PushClip(rect, corner_radii(radii, rect.width, rect.height))
        }
        ClipShape::Ellipse {
            radii,
            circle,
            center,
        } => {
            let (cx, cy) = point(center);
            let (left, top) = (cx - reference.x, cy - reference.y);
            let (right, bottom) = (width - left, height - top);
            // Percentages of a circle's radius refer to the box's normalized diagonal.
            let radius = |radius: &ShapeRadius, horizontal: bool| match radius {
                ShapeRadius::Length(length) if *circle => {
                    length.resolve(libm::hypotf(width, height) / 2f32.sqrt())
                }
                ShapeRadius::Length(length) => length.resolve(if horizontal { width } else { height }),
                ShapeRadius::ClosestSide if *circle => {
                    left.abs().min(right.abs()).min(top.abs()).min(bottom.abs())
                }
                ShapeRadius::FarthestSide if *circle => {
                    left.abs().max(right.abs()).max(top.abs()).max(bottom.abs())
                }
                ShapeRadius::ClosestSide if horizontal => left.abs().min(right.abs()),
                ShapeRadius::ClosestSide => top.abs().min(bottom.abs()),
                ShapeRadius::FarthestSide if horizontal => left.abs().max(right.abs()),
                ShapeRadius::FarthestSide => top.abs().max(bottom.abs()),
            };
            Item::PushClipShape(ClipGeometry::Ellipse {
                center: (cx, cy),
                radii: (radius(&radii.0, true).max(0.0), radius(&radii.1, false).max(0.0)),
            })
        }
        ClipShape::Polygon { even_odd, points } => Item::PushClipShape(ClipGeometry::Polygon {
            points: points.iter().map(point).collect(),
            even_odd: *even_odd,
        }),
        ClipShape::Path { even_odd, data } => {
            match fagus_layout::path_data::parse(data, (0.0, 0.0)) {
                // The path's pixels are zoomed with the element (Chrome 157, measured).
                Some(segments) => Item::PushClipShape(ClipGeometry::Path {
                    segments: zoomed_path(segments, style.zoom, (reference.x, reference.y)),
                    even_odd: *even_odd,
                }),
                // Path data that draws nothing clips everything away.
                None => Item::PushClip(
                    RectF {
                        width: 0.0,
                        height: 0.0,
                        ..reference
                    },
                    None,
                ),
            }
        }
        // Resolved by the caller.
        ClipShape::Url(_) => Item::PushClip(reference, None),
    }
}

/// Path segments scaled by `zoom` about their origin and moved to `origin`.
fn zoomed_path(
    mut segments: Vec<fagus_layout::PathSegment>,
    zoom: f32,
    (x, y): (f32, f32),
) -> Vec<fagus_layout::PathSegment> {
    use fagus_layout::PathSegment;
    let zoom = if zoom > 0.0 { zoom } else { 1.0 };
    for segment in &mut segments {
        match segment {
            PathSegment::Move(px, py) | PathSegment::Line(px, py) => {
                (*px, *py) = (x + *px * zoom, y + *py * zoom);
            }
            PathSegment::Cubic(points) => {
                for [px, py] in points.as_chunks_mut::<2>().0 {
                    (*px, *py) = (x + *px * zoom, y + *py * zoom);
                }
            }
            PathSegment::Close => {}
        }
    }
    segments
}

fn paint_borders(
    style: &ComputedStyle,
    rect: RectF,
    radii: Option<Radii>,
    sides: fagus_layout::VisibleSides,
    items: &mut Vec<Item>,
) {
    let shown = |visible: bool, width: f32| if visible { width } else { 0.0 };
    let widths = [
        shown(sides.top, style.border_width.top),
        shown(sides.right, style.border_width.right),
        shown(sides.bottom, style.border_width.bottom),
        shown(sides.left, style.border_width.left),
    ];
    if widths.iter().all(|width| *width <= 0.0) {
        return;
    }
    let styles = [
        style.border_style.top,
        style.border_style.right,
        style.border_style.bottom,
        style.border_style.left,
    ];
    let colors = border_colors(style);
    let styles: [BorderStyle; 4] = std::array::from_fn(|side| effective_style(styles[side], widths[side]));
    let RectF { x, y, width, height } = rect;
    let [top, right, bottom, left] = widths;
    // Four solid sides of one color are one shape, the area between the border edge and the
    // padding edge, without seams where the sides meet. (Where a side is missing, its edges
    // would meet and leave a hairline.)
    if widths.iter().all(|width| *width > 0.0)
        && styles.iter().all(|style| *style == BorderStyle::Solid)
        && colors.iter().all(|color| *color == colors[0])
    {
        let inner = RectF {
            x: x + left,
            y: y + top,
            width: (width - left - right).max(0.0),
            height: (height - top - bottom).max(0.0),
        };
        let square = [(0.0, 0.0); 4];
        let (outer_radii, inner_radii) = match radii {
            Some(radii) => (radii, inner_radii(radii, widths).unwrap_or(square)),
            None => (square, square),
        };
        // Sides of one width Chrome strokes along their middle (rounded where the corners are
        // round all through the border), which viewers draw as crisp as Chrome's: a stroke
        // that thin covers whole pixels, where the area between two edges would be faint.
        let uniform = widths.iter().all(|width| (*width - top).abs() < 1e-3)
            && outer_radii
                .iter()
                .all(|&(rx, ry)| (rx == 0.0 && ry == 0.0) || (rx >= top && ry >= top));
        items.push(Item::RoundedBorder {
            outer: rect,
            outer_radii,
            inner,
            inner_radii,
            color: colors[0],
            stroke: style
                .native_frame
                .filter(|_| sides == fagus_layout::VisibleSides::ALL)
                .or(uniform.then_some(top)),
        });
        return;
    }
    // Each side is a trapezoid; corners are split diagonally.
    let (x1, y1, x2, y2) = (x, y, x + width, y + height);
    let quads = [
        [(x1, y1), (x2, y1), (x2 - right, y1 + top), (x1 + left, y1 + top)],
        [
            (x2, y1),
            (x2, y2),
            (x2 - right, y2 - bottom),
            (x2 - right, y1 + top),
        ],
        [
            (x2, y2),
            (x1, y2),
            (x1 + left, y2 - bottom),
            (x2 - right, y2 - bottom),
        ],
        [
            (x1, y2),
            (x1, y1),
            (x1 + left, y1 + top),
            (x1 + left, y2 - bottom),
        ],
    ];
    // With rounded corners, a solid side is its part of the area between the curved edges: the
    // wedge from its outer corners through the inner ones, as Chrome splits the corners.
    // The box inset by the part `fraction` of each side's width (or by the pixels `round`ed
    // from it), its corners as much smaller.
    let inset = |part: &dyn Fn(f32) -> f32| {
        let insets = widths.map(part);
        let inset = RectF {
            x: x + insets[3],
            y: y + insets[0],
            width: (width - insets[1] - insets[3]).max(0.0),
            height: (height - insets[0] - insets[2]).max(0.0),
        };
        let curves = radii.and_then(|radii| inner_radii(radii, insets));
        (inset, curves.unwrap_or([(0.0, 0.0); 4]))
    };
    let ring = radii.map(|radii| {
        let (inner, curves) = inset(&|width| width);
        (radii, inner, curves)
    });
    // Dashes, dots and double lines of one color and width all round are drawn as one; other
    // sides each in its wedge, the 3D styles in their shade of each side.
    let uniform = matches!(
        styles[0],
        BorderStyle::Dashed | BorderStyle::Dotted | BorderStyle::Double
    ) && (0..4).all(|side| {
        widths[side].to_bits() == widths[0].to_bits()
            && styles[side] == styles[0]
            && colors[side] == colors[0]
    });
    // Straight sides go in Blink's order: the more translucent first; of one alpha dots and
    // dashes first, then the 3D styles, then the solid ones, which draw over the corners they
    // share with them; for one style the top, the bottom, the right and the left.
    let mut order = [0, 1, 2, 3];
    if ring.is_none() {
        let priority = |style: BorderStyle| match style {
            BorderStyle::None | BorderStyle::Hidden => 0,
            BorderStyle::Dotted | BorderStyle::Dashed | BorderStyle::Double => 1,
            BorderStyle::Solid => 3,
            _ => 2,
        };
        // More translucent sides go first (Blink's `ComplexBorderInfo` sorts by alpha first).
        let alpha = |side: usize| (colors[side].a.clamp(0.0, 1.0) * 255.0).round() as u8;
        order.sort_by_key(|&side| (alpha(side), priority(styles[side]), [0, 2, 1, 3][side]));
    }
    // The straight sides drawn so far.
    let mut painted = [false; 4];
    for side in order {
        if widths[side] <= 0.0 || !styles[side].is_visible() || colors[side].is_transparent() {
            continue;
        }
        let Some((outer_radii, inner, inner_radii)) = ring else {
            let mut points = quads[side];
            // Dots and dashes meet dots, dashes and double lines of another color or style
            // along the diagonal of the corner: they are cut to it (Blink's miters). Where
            // there is no miter, they fill the corner.
            let clip = matches!(styles[side], BorderStyle::Dashed | BorderStyle::Dotted) && {
                let miter = |adjacent: usize| {
                    widths[adjacent] > 0.0
                        && matches!(
                            styles[adjacent],
                            BorderStyle::Dashed | BorderStyle::Dotted | BorderStyle::Double
                        )
                        && (colors[adjacent] != colors[side]
                            || colors[adjacent].is_transparent()
                            || styles[adjacent] != styles[side])
                };
                let (start, end) = (miter((side + 3) % 4), miter((side + 1) % 4));
                // Without a miter, the end of the side goes straight across.
                let square = |outer: (f32, f32), inner: (f32, f32)| {
                    if side % 2 == 0 {
                        (outer.0, inner.1)
                    } else {
                        (inner.0, outer.1)
                    }
                };
                if !start {
                    points[3] = square(points[0], points[3]);
                }
                if !end {
                    points[2] = square(points[1], points[2]);
                }
                start || end
            };
            if clip {
                items.push(Item::PushClipShape(ClipGeometry::Polygon {
                    points: points.to_vec(),
                    even_odd: false,
                }));
            }
            // `inset` and `outset` sides are solid ones of their shades (Blink's
            // `BorderStyleHasUnmatchedColorsAtCorner`).
            let points = if matches!(
                effective_style(styles[side], widths[side]),
                BorderStyle::Solid | BorderStyle::Inset | BorderStyle::Outset
            ) {
                let shaded: [Rgba; 4] = std::array::from_fn(|index| {
                    shaded_color(styles[index], widths[index], colors[index], index)
                });
                solid_side_miters(quads[side], side, (widths, styles, shaded), painted)
            } else {
                quads[side]
            };
            border_side(points, side, colors[side], styles[side], widths[side], items);
            if clip {
                items.push(Item::Pop);
            }
            painted[side] = true;
            continue;
        };
        // The lines through the corners go on to the edge of the square that holds the corner's
        // curves, so that the wedges of two sides meet along them and nowhere else.
        let [outer_start, outer_end, inner_end, inner_start] = quads[side];
        let corner = |outer: (f32, f32), inner: (f32, f32)| {
            // Corners clockwise from the top left: the one at this outer point.
            let index = match (outer.0 > x + width / 2.0, outer.1 > y + height / 2.0) {
                (false, false) => 0,
                (true, false) => 1,
                (true, true) => 2,
                (false, true) => 3,
            };
            let (dx, dy) = ((inner.0 - outer.0).abs(), (inner.1 - outer.1).abs());
            let size = (outer_radii[index].0.max(dx), outer_radii[index].1.max(dy));
            let along = match (dx > 0.0, dy > 0.0) {
                (true, true) => (size.0 / dx).min(size.1 / dy),
                (true, false) => size.0 / dx,
                (false, true) => size.1 / dy,
                (false, false) => return inner,
            };
            (
                outer.0 + (inner.0 - outer.0) * along.max(1.0),
                outer.1 + (inner.1 - outer.1) * along.max(1.0),
            )
        };
        let wedge = vec![
            outer_start,
            outer_end,
            corner(outer_end, inner_end),
            corner(outer_start, inner_start),
        ];
        if !uniform {
            items.push(Item::PushClipShape(ClipGeometry::Polygon {
                points: wedge,
                even_odd: false,
            }));
        }
        let color = colors[side];
        let ring = |(outer, outer_radii): (RectF, Radii), (inner, inner_radii): (RectF, Radii), color| {
            Item::RoundedBorder {
                outer,
                outer_radii,
                inner,
                inner_radii,
                color,
                stroke: None,
            }
        };
        let (outer, inner) = ((rect, outer_radii), (inner, inner_radii));
        // The top and left sides of `inset` and `groove` are dark, the others light.
        let top_or_left = side == 0 || side == 3;
        let (dark, light) = shades(color);
        match styles[side] {
            // Chrome strokes along the middle cut to whole pixels.
            BorderStyle::Dashed | BorderStyle::Dotted => {
                let (center, center_radii) = inset(&|width| (width / 2.0).trunc());
                let adjacent = [widths[(side + 1) % 4], widths[(side + 3) % 4]];
                items.push(Item::DashedBorder {
                    outer: rect,
                    outer_radii,
                    inner: inner.0,
                    inner_radii: inner.1,
                    center,
                    center_radii,
                    color,
                    style: styles[side],
                    width: widths[side],
                    stroke: widths[side].max(adjacent[0]).max(adjacent[1]),
                    centerline: None,
                });
            }
            // Two lines of a third, as Blink rounds them.
            BorderStyle::Double if widths[side] >= 3.0 => {
                items.push(ring(outer, inset(&|width| (width / 3.0).round()), color));
                items.push(ring(inset(&|width| (width * 2.0 / 3.0).round()), inner, color));
            }
            BorderStyle::Inset | BorderStyle::Outset => {
                let darker = top_or_left == (styles[side] == BorderStyle::Inset);
                items.push(ring(outer, inner, if darker { dark } else { light }));
            }
            BorderStyle::Groove | BorderStyle::Ridge => {
                let outer_dark = top_or_left == (styles[side] == BorderStyle::Groove);
                let (first, second) = if outer_dark { (dark, light) } else { (light, dark) };
                let center = inset(&|width| (width / 2.0).trunc());
                items.push(ring(outer, center, first));
                items.push(ring(center, inner, second));
            }
            _ => items.push(ring(outer, inner, color)),
        }
        if uniform {
            break;
        }
        items.push(Item::Pop);
    }
}

/// The outline of a straight solid, `inset` or `outset` border side (`colors` the shades the
/// sides are filled with), `side` counted from the top clockwise (`quad` split diagonally at
/// both corners), as Blink draws it (`ComputeMiter`): its end goes straight
/// across the corner, filling it, where the side beside it is missing, or is still to be drawn
/// (`painted` says which are drawn) over it in a style that fills it, or has its color and style;
/// it is cut along the diagonal (a miter) only where the colors or styles meet there. Sides of
/// one color thus meet without a seam.
fn solid_side_miters(
    quad: [(f32, f32); 4],
    side: usize,
    (widths, styles, colors): ([f32; 4], [BorderStyle; 4], [Rgba; 4]),
    painted: [bool; 4],
) -> [(f32, f32); 4] {
    let miter = |adjacent: usize| {
        if widths[adjacent] <= 0.0 || !styles[adjacent].is_visible() {
            return false;
        }
        // Transparent sides count as drawn.
        let drawn = painted[adjacent] || colors[adjacent].is_transparent();
        let fills = !matches!(
            styles[adjacent],
            BorderStyle::Dotted | BorderStyle::Dashed | BorderStyle::Double
        );
        if !drawn && fills {
            return false;
        }
        colors[adjacent] != colors[side] || styles[adjacent] != styles[side]
    };
    let square = |outer: (f32, f32), inner: (f32, f32)| {
        if side.is_multiple_of(2) {
            (outer.0, inner.1)
        } else {
            (inner.0, outer.1)
        }
    };
    let mut points = quad;
    if !miter((side + 3) % 4) {
        points[3] = square(points[0], points[3]);
    }
    if !miter((side + 1) % 4) {
        points[2] = square(points[1], points[2]);
    }
    points
}

/// Paints one side of a border, `side` counted from the top clockwise. `points` are its outer
/// edge (first two) and its inner edge (last two, backwards). Dashes and dots are stroked when
/// drawn; the other styles become bands of the side filled as Chrome fills them.
fn border_side(
    points: [(f32, f32); 4],
    side: usize,
    color: Rgba,
    style: BorderStyle,
    width: f32,
    items: &mut Vec<Item>,
) {
    // The band from `from` to `to` of the side's width, counted from its outer edge.
    let band = |from: f32, to: f32| {
        let at = |outer: (f32, f32), inner: (f32, f32), part: f32| {
            (
                outer.0 + (inner.0 - outer.0) * part,
                outer.1 + (inner.1 - outer.1) * part,
            )
        };
        [
            at(points[0], points[3], from),
            at(points[1], points[2], from),
            at(points[1], points[2], to),
            at(points[0], points[3], to),
        ]
    };
    let style = effective_style(style, width);
    if matches!(style, BorderStyle::Dashed | BorderStyle::Dotted) {
        items.push(Item::BorderSide {
            points,
            color,
            style,
            width,
        });
        return;
    }
    let mut fill = |points: [(f32, f32); 4], color: Rgba| {
        items.push(Item::BorderSide {
            points,
            color,
            style: BorderStyle::Solid,
            width,
        });
    };
    // The top and left sides of `inset` and `groove` are dark, the others light.
    let top_or_left = side == 0 || side == 3;
    let (dark, light) = shades(color);
    match style {
        // Two lines of a third, as Blink rounds them; too thin for that, one line.
        BorderStyle::Double if width >= 3.0 => {
            let pixels = width.round();
            let outer = ((pixels + 1.0) / 3.0).floor() / pixels;
            let inner = ((pixels * 2.0 + 1.0) / 3.0).floor() / pixels;
            fill(band(0.0, outer), color);
            fill(band(inner, 1.0), color);
        }
        BorderStyle::Inset | BorderStyle::Outset => {
            let darker = top_or_left == (style == BorderStyle::Inset);
            fill(points, if darker { dark } else { light });
        }
        BorderStyle::Groove | BorderStyle::Ridge => {
            let half = f32::midpoint(width.round(), 1.0).floor() / width.round().max(1.0);
            let outer_dark = top_or_left == (style == BorderStyle::Groove);
            let (outer, inner) = if outer_dark { (dark, light) } else { (light, dark) };
            fill(band(0.0, half), outer);
            fill(band(half, 1.0), inner);
        }
        _ => fill(points, color),
    }
}

/// The color of a border side in `border_style` whose `border-color` is `color` (`None` for
/// `currentcolor`): the 3D styles in `currentcolor` are drawn in `#eee`, whatever the text's
/// color, except on tables and their parts, as Blink draws them
/// (`ComputedStyleUtils::BorderSideColor`).
fn side_color(style: &ComputedStyle, color: Option<Rgba>, border_style: BorderStyle) -> Rgba {
    let table_type = matches!(
        style.display,
        fagus_style::values::Display::Internal(_)
            | fagus_style::values::Display::Normal {
                inside: fagus_style::values::DisplayInside::Table,
                ..
            }
    );
    match (color, border_style) {
        (None, BorderStyle::Inset | BorderStyle::Outset | BorderStyle::Groove | BorderStyle::Ridge)
            if !table_type =>
        {
            Rgba::from_u8(238, 238, 238, 1.0)
        }
        _ => style.border_color(color),
    }
}

/// The colors of the border sides of `style`, from the top clockwise (see [`side_color`]).
pub(crate) fn border_colors(style: &ComputedStyle) -> [Rgba; 4] {
    [
        side_color(style, style.border_color.top, style.border_style.top),
        side_color(style, style.border_color.right, style.border_style.right),
        side_color(style, style.border_color.bottom, style.border_style.bottom),
        side_color(style, style.border_color.left, style.border_style.left),
    ]
}

/// The color a straight side of `style`, `width` and `color` (`side` counted from the top
/// clockwise) is filled with: of an `inset` or `outset` side, its darker or lighter shade (see
/// [`border_side`]).
fn shaded_color(style: BorderStyle, width: f32, color: Rgba, side: usize) -> Rgba {
    let style = effective_style(style, width);
    if !matches!(style, BorderStyle::Inset | BorderStyle::Outset) {
        return color;
    }
    let (dark, light) = shades(color);
    if (side == 0 || side == 3) == (style == BorderStyle::Inset) {
        dark
    } else {
        light
    }
}

/// The style a border side is drawn in: `groove` and `ridge` thinner than two pixels are solid,
/// as in Blink (`BorderEdge::EffectiveStyle`).
fn effective_style(style: BorderStyle, width: f32) -> BorderStyle {
    match style {
        BorderStyle::Groove | BorderStyle::Ridge if width < 2.0 => BorderStyle::Solid,
        style => style,
    }
}

/// The darker and lighter shades of a color for the 3D border styles, as Blink computes them
/// (`Color::Dark` and `Color::Light`); black becomes two grays, so that it shows.
fn shades(color: Rgba) -> (Rgba, Rgba) {
    // Channels in steps of 1/256, cut as Blink does.
    let scaled = |color: Rgba, factor: f32| {
        let channel = |value: f32| (value * factor * 255.999_98).floor().clamp(0.0, 255.0) / 255.0;
        Rgba {
            r: channel(color.r),
            g: channel(color.g),
            b: channel(color.b),
            a: color.a,
        }
    };
    let light = |color: Rgba| {
        let value = color.r.max(color.g).max(color.b);
        if value <= 0.0 {
            Rgba {
                a: color.a,
                ..Rgba::from_u8(0x54, 0x54, 0x54, 1.0)
            }
        } else {
            scaled(color, (value + 0.33).min(1.0) / value)
        }
    };
    // Very dark colors get two lighter shades for contrast, and very light ones stay as they
    // are on their light sides (Blink's `CalculateInsetOutsetColor`): the light side of a
    // fieldset's `groove` in `#efefef` is `#efefef`.
    let luminance = relative_luminance(color);
    if luminance <= 0.014_443_844 {
        let gray = light(color);
        return (gray, light(gray));
    }
    let value = color.r.max(color.g).max(color.b);
    let lighter = if luminance > 0.830_77 { color } else { light(color) };
    (scaled(color, ((value - 0.33) / value).max(0.0)), lighter)
}

/// The relative luminance of a color (WCAG 2), from its sRGB channels.
fn relative_luminance(color: Rgba) -> f32 {
    let linear = |value: f32| {
        if value <= 0.040_45 {
            value / 12.92
        } else {
            libm::powf((value + 0.055) / 1.055, 2.4)
        }
    };
    0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
}

/// The outline of a box (CSS UI 4, §5): a border around its border box, `outline-offset` away
/// from it, with corners that follow the box's; `auto` is the focus ring Chrome prints, a black
/// ring 2px wide on the edge of the box and a white one around it, with corners of 3px.
fn paint_outline(style: &ComputedStyle, rect: RectF, radii: Option<Radii>, items: &mut Vec<Item>) {
    let border_box = rect;
    let grow = |rect: RectF, by: f32| RectF {
        x: rect.x - by,
        y: rect.y - by,
        width: (rect.width + 2.0 * by).max(0.0),
        height: (rect.height + 2.0 * by).max(0.0),
    };
    // A negative offset does not take the outline past the middle of the box, in each axis
    // apart (CSS UI 4, §5.4; Blink's `AdjustedOutlineOffset`, on the snapped box).
    let half = |size: f32| -(size.round() / 2.0).trunc();
    let (offset_x, offset_y) = (
        style.outline_offset.max(half(rect.width)),
        style.outline_offset.max(half(rect.height)),
    );
    let rect = RectF {
        x: rect.x - offset_x,
        y: rect.y - offset_y,
        width: (rect.width + 2.0 * offset_x).max(0.0),
        height: (rect.height + 2.0 * offset_y).max(0.0),
    };
    let color = style.outline_color.unwrap_or(style.color);
    match style.outline_style {
        OutlineStyle::Auto => {
            for (center, ring) in [(1.0, Rgba::WHITE), (0.0, color)] {
                items.push(Item::RoundedBorder {
                    outer: grow(rect, center + 1.0),
                    outer_radii: [(4.0, 4.0); 4],
                    inner: grow(rect, center - 1.0),
                    inner_radii: [(2.0, 2.0); 4],
                    color: ring,
                    stroke: None,
                });
            }
        }
        // (An outline that is not drawn has its style left uncopied, as anonymous boxes keep
        // the initial width with the initial style `none`.)
        OutlineStyle::Line(line) if !line.is_visible() => {}
        OutlineStyle::Line(line) => {
            let width = style.outline_width;
            // A border along shapes (`border-shape`): a solid outline right around the shape.
            if let Some(shape) = style.border_shape.as_deref()
                && border_shape::paint_outline(
                    style,
                    border_box,
                    shape,
                    (width, style.outline_offset),
                    color,
                    items,
                )
            {
                return;
            }
            // Corners of other shapes than round (`corner-shape`) shape the outline too.
            if let Some(radii) = radii.filter(|radii| corner::is_shaped(style, Some(radii))) {
                corner::paint_outline(
                    style,
                    border_box,
                    &radii,
                    line,
                    width,
                    style.outline_offset,
                    color,
                    items,
                );
                return;
            }
            let by = style.outline_offset + width;
            let radii = radii.map(|radii| {
                radii.map(|(x, y)| {
                    if x > 0.0 && y > 0.0 {
                        ((x + by).max(0.0), (y + by).max(0.0))
                    } else {
                        (x, y)
                    }
                })
            });
            let mut border = style.clone();
            border.border_width = fagus_style::values::Sides::all(width);
            border.border_style = fagus_style::values::Sides::all(line);
            border.border_color = fagus_style::values::Sides::all(Some(color));
            paint_borders(
                &border,
                grow(rect, width),
                radii,
                fagus_layout::VisibleSides::ALL,
                items,
            );
        }
    }
}

/// The glyphs of an upright run in a vertical line, for drawing them with vertical advances in
/// coordinates that stand as the page does (from the run's baseline): their offset across the
/// line goes right, the one along it down.
fn upright_glyphs(glyphs: &[Glyph]) -> Vec<Glyph> {
    glyphs
        .iter()
        .map(|glyph| Glyph {
            x_offset: glyph.y_offset,
            y_offset: -glyph.x_offset,
            ..*glyph
        })
        .collect()
}

/// The glyphs of an upright run as [`upright_glyphs`] places them, for drawing that knows only
/// horizontal advances: each one below the one before, by offsets alone.
fn stacked_glyphs(glyphs: &[Glyph]) -> Vec<Glyph> {
    let mut down = 0.0;
    glyphs
        .iter()
        .map(|glyph| {
            let stacked = Glyph {
                advance: 0.0,
                x_offset: glyph.y_offset,
                y_offset: -(down + glyph.x_offset),
                ..*glyph
            };
            down += glyph.advance;
            stacked
        })
        .collect()
}

/// Paints the decorations of a text run: its underlines and overlines (`below`), else its lines
/// through, placed and sized as Chrome does, from the size and the first available font of its
/// style whatever face and size its glyphs come in. A line is a tenth of the font size thick
/// (cut to whole pixels) unless `text-decoration-thickness` says otherwise (rounded); an
/// underline lies half its thickness below the baseline, at least 1px, or
/// `text-underline-offset` below it.
/// Whether a run of text sits on the central baseline of a vertical line (not set all sideways).
fn on_central_baseline(run: &fagus_layout::TextRun) -> bool {
    use fagus_layout::GlyphOrientation;
    matches!(
        run.orientation,
        GlyphOrientation::Upright | GlyphOrientation::Sideways
    ) && !run.style.writing_mode.is_sideways()
        && run.style.text_orientation != fagus_style::values::TextOrientation::Sideways
}

fn paint_decorations(
    run: &fagus_layout::TextRun,
    x: f32,
    baseline: f32,
    width: f32,
    fonts: &FontDb,
    below: bool,
    items: &mut Vec<Item>,
) {
    if run.style.decorations.is_empty() || width <= 0.0 {
        return;
    }
    let face = fonts.face(fagus_layout::face_for(fonts, &run.style).unwrap_or(run.face));
    // Decorations of the box and its ancestors (CSS Text Decoration 3, §2), as thick as the font
    // of the box that draws them makes them; underlines lie where that font puts them, overlines
    // and lines through that far from the top of the text (as in Chrome).
    for decoration in run.style.decorations.iter() {
        let size = decoration.font_size;
        let scale = face.scale(size);
        let exact = match decoration.thickness {
            DecorationThickness::Auto => size / 10.0,
            DecorationThickness::FromFont => face.metrics.underline_thickness * scale,
            DecorationThickness::Px(px) => px,
        };
        let thickness = match decoration.thickness {
            DecorationThickness::Auto => exact.floor(),
            _ => exact.round(),
        }
        .max(1.0);
        let gap = (exact / 2.0).ceil().max(1.0);
        let line = decoration.line;
        // In vertical lines on the central baseline, an underline goes under the em box (over it
        // in Japanese and Korean), a pixel away, as in Chrome.
        if below && line.underline && on_central_baseline(run) {
            let metrics = &face.metrics;
            let ascent = (metrics.ascender * scale).round();
            let half = (metrics.ascender + metrics.descender) * scale / 2.0;
            let offset = decoration.underline_offset.map_or(0.0, f32::round);
            let over = run
                .style
                .lang
                .as_deref()
                .is_some_and(|lang| lang.starts_with("ja") || lang.starts_with("ko"));
            let top = if over {
                baseline - ascent + (half - size / 2.0 - offset).round().floor() - 1.0 - thickness
            } else {
                baseline - ascent + (half + size / 2.0 + offset).round().floor() + 1.0
            };
            decoration_line(decoration, Line::Under, x, top, width, thickness, exact, items);
        }
        let tops = if below {
            [
                (line.underline && !on_central_baseline(run)).then(|| {
                    // `under` starts at the bottom of the em box, rounded up.
                    let under = decoration.under.then(|| {
                        let metrics = &face.metrics;
                        (size * metrics.descender / (metrics.ascender + metrics.descender)).ceil()
                    });
                    baseline
                        + match (under, decoration.underline_offset) {
                            (Some(under), offset) => under + offset.map_or(0.0, f32::round),
                            (None, Some(offset)) => offset.round(),
                            (None, None) => gap,
                        }
                }),
                line.overline
                    .then(|| baseline - face.rounded_ascent_descent(run.style.font_size).0 - thickness),
            ]
        } else {
            [
                line.line_through.then(|| {
                    // Two thirds of the decorating font's ascent down from the top of the
                    // text, centered (Blink's `ComputeLineThroughLineData`).
                    // With what Chrome adds to the ascent of some fonts.
                    let top = baseline - face.rounded_ascent_descent(run.style.font_size).0;
                    let adjustment =
                        face.rounded_ascent_descent(size).0 - (face.metrics.ascender * scale).round();
                    top + 2.0 * (face.metrics.ascender * scale + adjustment) / 3.0 - exact / 2.0
                }),
                None,
            ]
        };
        let kinds = if below {
            [Line::Under, Line::Over]
        } else {
            [Line::Through, Line::Through]
        };
        for (kind, top) in kinds.into_iter().zip(tops) {
            let Some(top) = top else { continue };
            let band = decoration_band(decoration, kind, top, thickness, exact);
            // Underlines and overlines leave gaps where they would cross glyphs, as in Chrome:
            // where the glyphs reach into the line's unrounded thickness, widened by it.
            let gaps = if below && run.style.text_decoration_skip_ink != SkipInk::None {
                ink_gaps(run, x, baseline, fonts, band, exact.min(13.0))
            } else {
                Vec::new()
            };
            if gaps.is_empty() {
                decoration_line(decoration, kind, x, top, width, thickness, exact, items);
                continue;
            }
            let mut start = x;
            for (left, right) in gaps.into_iter().chain([(x + width, f32::INFINITY)]) {
                let end = left.min(x + width);
                if end > start {
                    match decoration.style {
                        TextDecorationStyle::Solid | TextDecorationStyle::Double => {
                            decoration_line(
                                decoration,
                                kind,
                                start,
                                top,
                                end - start,
                                thickness,
                                exact,
                                items,
                            );
                        }
                        // Patterns run on across the gaps.
                        _ => {
                            let height = band.1 - band.0;
                            let clip = RectF {
                                x: start,
                                y: band.0 - height,
                                width: end - start,
                                height: 3.0 * height,
                            };
                            items.push(Item::PushClip(clip, None));
                            decoration_line(decoration, kind, x, top, width, thickness, exact, items);
                            items.push(Item::Pop);
                        }
                    }
                }
                start = start.max(right);
            }
        }
    }
}

/// Which line of a text decoration is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Line {
    Under,
    Over,
    Through,
}

/// How far a cubic curve whose control points lie `d` off its axis, one on either side, strays
/// from the axis: `d` times this.
const WAVE_HEIGHT: f32 = 0.288_675;

/// A wave of a decoration `exact` pixels thick, as Blink's `MakeWave` makes it: its wavelength,
/// the distance of its control points from its middle (both half pixels and more), and the
/// rows it covers around its middle, from the whole pixel above it to the one below.
fn wave(exact: f32) -> (f32, f32, (f32, f32)) {
    let thickness = exact.max(1.0);
    let wavelength = 1.0 + 2.0 * (2.0 * thickness + 0.5).round();
    let control = 0.5 + (3.0 * thickness + 0.5).round();
    // The middle lies half a pixel down the rows the wave is cut to.
    let reach = WAVE_HEIGHT * control + exact / 2.0;
    let rows = ((0.5 - reach).floor() - 0.5, (0.5 + reach).ceil() - 0.5);
    (wavelength, control, rows)
}

/// How far from the line a wavy or second double line lies (Blink's `ComputeLineData`): below
/// an underline, above an overline, a whole number of pixels below a line-through.
fn line_offset(kind: Line, exact: f32, wavy: bool) -> f32 {
    match kind {
        Line::Under => exact + 1.0,
        Line::Over => -(exact + 1.0),
        Line::Through if wavy => 0.0,
        Line::Through => (exact + 1.0).floor(),
    }
}

/// The heights a decoration line in its style covers, from its `top`.
fn decoration_band(
    decoration: &fagus_style::values::Decoration,
    kind: Line,
    top: f32,
    thickness: f32,
    exact: f32,
) -> (f32, f32) {
    match decoration.style {
        TextDecorationStyle::Double => {
            let offset = line_offset(kind, exact, false);
            (top + offset.min(0.0), top + offset.max(0.0) + thickness)
        }
        TextDecorationStyle::Wavy => {
            let (_, _, rows) = wave(exact);
            let middle = top + line_offset(kind, exact, true) + 0.5;
            (middle + rows.0, middle + rows.1)
        }
        _ => (top, top + exact),
    }
}

/// Whether an underline skips the ink of a character with `text-decoration-skip-ink: auto`:
/// not that of CJK ideographs, kana, Hangul and CJK symbols, nor of `/`, `\` and `_`, as
/// Blink's `CanTextDecorationSkipInk` decides (BSD-licensed, see `CHROMIUM-LICENSE` of
/// `fagus-layout`).
fn skips_ink(character: char) -> bool {
    !matches!(character,
        '/' | '\\' | '_'
        | '\u{1100}'..='\u{11FF}'
        | '\u{2E80}'..='\u{2FDF}'
        | '\u{2FF0}'..='\u{303F}'
        | '\u{3040}'..='\u{31FF}'
        | '\u{3200}'..='\u{9FFF}'
        | '\u{A960}'..='\u{A97F}'
        | '\u{AC00}'..='\u{D7FF}'
        | '\u{F900}'..='\u{FAFF}'
        | '\u{FE30}'..='\u{FE4F}'
        | '\u{FF00}'..='\u{FFEF}'
        | '\u{20000}'..='\u{3FFFF}')
}

/// Where a line between the heights `band` crosses the glyphs of a run set from `x` on its
/// baseline: for each glyph the extent of its outline there, widened by `dilation` on either
/// side, merged where they overlap (Chrome clips these out of underlines and overlines). With
/// `text-decoration-skip-ink: auto`, glyphs of characters that do not skip ink are left out.
fn ink_gaps(
    run: &fagus_layout::TextRun,
    x: f32,
    baseline: f32,
    fonts: &FontDb,
    band: (f32, f32),
    dilation: f32,
) -> Vec<(f32, f32)> {
    let face = fonts.face(run.face);
    let variations = fagus_layout::run_variations(fonts, run);
    let relative = (band.0 - baseline, band.1 - baseline);
    let mut gaps: Vec<(f32, f32)> = Vec::new();
    let mut pen = x;
    for glyph in &run.glyphs {
        let origin = pen + glyph.x_offset;
        pen += glyph.advance;
        if run.style.text_decoration_skip_ink == SkipInk::Auto
            && run
                .text
                .get(glyph.cluster.0 as usize..)
                .and_then(|text| text.chars().next())
                .is_some_and(|character| !skips_ink(character))
        {
            continue;
        }
        // A glyph raised or lowered by its offset meets the band elsewhere.
        let band = (relative.0 + glyph.y_offset, relative.1 + glyph.y_offset);
        let Some((left, right)) = face.intercept(glyph.id, run.size, &variations, band) else {
            continue;
        };
        let gap = (origin + left - dilation, origin + right + dilation);
        match gaps.last_mut() {
            Some(last) if gap.0 <= last.1 => last.1 = last.1.max(gap.1),
            _ => gaps.push(gap),
        }
    }
    gaps
}

/// One decoration line in its style, with its top at `top`, as Blink's `DecorationLinePainter`
/// draws it.
#[allow(clippy::too_many_arguments, reason = "the geometry of one decoration line")]
fn decoration_line(
    decoration: &fagus_style::values::Decoration,
    kind: Line,
    x: f32,
    top: f32,
    width: f32,
    thickness: f32,
    exact: f32,
    items: &mut Vec<Item>,
) {
    let color = decoration.color;
    let rect = |x: f32, y: f32, width: f32, height: f32| Item::Rect {
        rect: RectF { x, y, width, height },
        color,
        radii: None,
    };
    // Absurdly wide text draws its dots and dashes as a line.
    let style = match decoration.style {
        TextDecorationStyle::Dotted | TextDecorationStyle::Dashed if width / thickness > 100_000.0 => {
            TextDecorationStyle::Solid
        }
        style => style,
    };
    match style {
        // Chrome fills solid lines from a whole pixel down the page (Blink's `SnapYAxis`).
        TextDecorationStyle::Solid => items.push(rect(x, round(top), width, thickness)),
        // The second line lies a pixel beyond the first: below an underline, above an overline.
        TextDecorationStyle::Double => {
            items.push(rect(x, round(top), width, thickness));
            items.push(rect(
                x,
                round(top + line_offset(kind, exact, false)),
                width,
                thickness,
            ));
        }
        // A stroke as thick as the line, along the middle of its pixel rows from whole pixel to
        // whole pixel, with the dashes or dots of a line of its rounded thickness: dashes two
        // or three times as long, square dots up to 3px, round ones from there.
        TextDecorationStyle::Dotted | TextDecorationStyle::Dashed => {
            let dash = exact.round();
            let mut y = (top + (exact / 2.0).max(0.5)).floor();
            if dash.rem_euclid(2.0) >= 1.0 {
                y += 0.5;
            }
            let (mut from, mut to) = (x.trunc(), (x + width).trunc());
            let dotted = style == TextDecorationStyle::Dotted;
            // Round dots stand inside the ends.
            if dotted && dash > 3.0 {
                (from, to) = (from + dash / 2.0, to - dash / 2.0);
            }
            items.push(Item::DashedLine {
                from: (from, y),
                to: (to, y),
                thickness: exact,
                dash,
                dotted,
                color,
            });
        }
        // Blink's wave, from the start of the line, its middle half a pixel below the line's
        // top moved by the wave's offset.
        TextDecorationStyle::Wavy => {
            let (wavelength, control, rows) = wave(exact);
            let y = top + line_offset(kind, exact, true) + 0.5;
            items.push(Item::Wave {
                start: x,
                y,
                control,
                wavelength,
                thickness: exact,
                clip: RectF {
                    x,
                    y: y + rows.0,
                    width,
                    height: rows.1 - rows.0,
                },
                color,
            });
        }
    }
}

/// Converts layout units to pixels (for callers that work with fragments directly).
pub fn px(value: Au) -> f32 {
    value.to_px()
}

/// Where an image is drawn in its content box by `object-fit` and `object-position` (CSS Images
/// 3, §5.5), and whether it reaches out of the box.
fn object_rect(style: &ComputedStyle, image: &fagus_layout::Image, content: RectF) -> (RectF, bool) {
    let (width, height) = natural_object_size(image, content);
    if style.object_fit == ObjectFit::Fill || width <= 0.0 || height <= 0.0 {
        return (content, false);
    }
    let contain = (content.width / width).min(content.height / height);
    let scale = match style.object_fit {
        ObjectFit::Cover => (content.width / width).max(content.height / height),
        ObjectFit::None => 1.0,
        ObjectFit::ScaleDown => contain.min(1.0),
        ObjectFit::Contain | ObjectFit::Fill => contain,
    };
    let (width, height) = (width * scale, height * scale);
    let (x, y) = &style.object_position;
    let rect = RectF {
        x: content.x + x.resolve(content.width - width),
        y: content.y + y.resolve(content.height - height),
        width,
        height,
    };
    let tolerance = 0.01;
    let overflows = rect.x < content.x - tolerance
        || rect.y < content.y - tolerance
        || rect.x + rect.width > content.x + content.width + tolerance
        || rect.y + rect.height > content.y + content.height + tolerance;
    (rect, overflows)
}

/// The size `object-fit` scales an image from: its natural size, the missing parts of which
/// (an SVG image without `width` or `height`) the default sizing algorithm gives it against the
/// content box as the default object size (CSS Images 3, §5.2): a natural width or height
/// with the ratio, or the largest size of the ratio within the box (Chrome 157, measured).
fn natural_object_size(image: &fagus_layout::Image, content: RectF) -> (f32, f32) {
    match (
        image.natural,
        image.ratio.filter(|ratio| ratio.is_finite() && *ratio > 0.0),
    ) {
        ((true, true), _) => (image.width, image.height),
        ((true, false), ratio) => (
            image.width,
            ratio.map_or(content.height, |ratio| image.width / ratio),
        ),
        ((false, true), ratio) => (
            ratio.map_or(content.width, |ratio| image.height * ratio),
            image.height,
        ),
        ((false, false), Some(ratio)) => {
            if content.width / ratio <= content.height {
                (content.width, content.width / ratio)
            } else {
                (content.height * ratio, content.height)
            }
        }
        ((false, false), None) => (content.width, content.height),
    }
}

/// The palette of `fagus-text` for a `font-palette`.
fn font_palette(palette: &fagus_style::values::FontPalette) -> fagus_text::FontPalette {
    use fagus_style::values::FontPalette as Style;
    match palette {
        Style::Normal => fagus_text::FontPalette::Normal,
        Style::Light => fagus_text::FontPalette::Light,
        Style::Dark => fagus_text::FontPalette::Dark,
        Style::Named(name) => fagus_text::FontPalette::Named(Arc::clone(name)),
        Style::Mix(mix) => {
            fagus_text::FontPalette::Mix(Arc::new((font_palette(&mix.0), font_palette(&mix.1), mix.2)))
        }
    }
}

#[cfg(test)]
mod tests {
    use fagus_layout::ImageFormat;
    use fagus_style::values::{Decoration, LengthPercentage, TextDecorationLine};

    use super::*;

    #[test]
    fn meets_sides_of_one_color_without_a_miter_as_blink() {
        use fagus_layout::VisibleSides;
        use fagus_style::values::Sides;
        let cyan = Rgba::from_u8(0, 255, 255, 1.0);
        let style = ComputedStyle::from(ComputedValues {
            border_width: Sides::all(10.0),
            border_style: Sides::all(BorderStyle::Solid),
            border_color: Sides {
                bottom: Some(cyan),
                ..Sides::all(Some(Rgba::BLACK))
            },
            ..ComputedValues::default()
        });
        let rect = RectF {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 300.0,
        };
        let mut items = Vec::new();
        paint_borders(&style, rect, None, VisibleSides::ALL, &mut items);
        let sides: Vec<[(f32, f32); 4]> = items
            .iter()
            .filter_map(|item| match item {
                Item::BorderSide { points, .. } => Some(*points),
                _ => None,
            })
            .collect();
        // Top, bottom, right, left: the top and the bottom go across the corners, which the
        // sides still to come fill; the right and the left meet the black top square and the
        // cyan bottom along the diagonal.
        assert_eq!(
            sides,
            vec![
                [(0.0, 0.0), (200.0, 0.0), (200.0, 10.0), (0.0, 10.0)],
                [(200.0, 300.0), (0.0, 300.0), (0.0, 290.0), (200.0, 290.0)],
                [(200.0, 0.0), (200.0, 300.0), (190.0, 290.0), (190.0, 0.0)],
                [(0.0, 300.0), (0.0, 0.0), (10.0, 0.0), (10.0, 290.0)],
            ]
        );
    }

    #[test]
    fn meets_the_shades_of_inset_sides_as_solid_sides_of_two_colors() {
        use fagus_layout::VisibleSides;
        use fagus_style::values::Sides;
        // `<td>` of `<table border=1>`: 1px inset, dark at the top and left, light at the
        // bottom and right.
        let style = ComputedStyle::from(ComputedValues {
            border_width: Sides::all(1.0),
            border_style: Sides::all(BorderStyle::Inset),
            border_color: Sides::all(Some(Rgba::from_u8(128, 128, 128, 1.0))),
            ..ComputedValues::default()
        });
        let rect = RectF {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
        };
        let mut items = Vec::new();
        paint_borders(&style, rect, None, VisibleSides::ALL, &mut items);
        let sides: Vec<[(f32, f32); 4]> = items
            .iter()
            .filter_map(|item| match item {
                Item::BorderSide { points, .. } => Some(*points),
                _ => None,
            })
            .collect();
        // The top and the bottom go across the corners as rectangles (as Chrome writes
        // them); the right meets the dark top and the left the light bottom along the diagonal,
        // and each goes straight where it meets the side of its own shade.
        assert_eq!(
            sides,
            vec![
                [(0.0, 0.0), (100.0, 0.0), (100.0, 1.0), (0.0, 1.0)],
                [(100.0, 50.0), (0.0, 50.0), (0.0, 49.0), (100.0, 49.0)],
                [(100.0, 0.0), (100.0, 50.0), (99.0, 50.0), (99.0, 1.0)],
                [(0.0, 50.0), (0.0, 0.0), (1.0, 0.0), (1.0, 49.0)],
            ]
        );
    }

    #[test]
    fn paints_translucent_border_sides_first() {
        use fagus_layout::VisibleSides;
        use fagus_style::values::Sides;
        // An `<hr>` with a translucent solid bottom: Chrome draws the bottom first, across the
        // corners, and the inset sides after it, cut along the diagonal.
        let style = ComputedStyle::from(ComputedValues {
            border_width: Sides {
                top: 0.0,
                ..Sides::all(1.0)
            },
            border_style: Sides {
                bottom: BorderStyle::Solid,
                ..Sides::all(BorderStyle::Inset)
            },
            border_color: Sides {
                bottom: Some(Rgba::from_u8(0, 0, 0, 0.07)),
                ..Sides::all(Some(Rgba::from_u8(128, 128, 128, 1.0)))
            },
            ..ComputedValues::default()
        });
        let rect = RectF {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 1.0,
        };
        let mut items = Vec::new();
        paint_borders(&style, rect, None, VisibleSides::ALL, &mut items);
        let sides: Vec<[(f32, f32); 4]> = items
            .iter()
            .filter_map(|item| match item {
                Item::BorderSide { points, .. } => Some(*points),
                _ => None,
            })
            .collect();
        assert_eq!(sides.len(), 3, "{items:?}");
        assert_eq!(sides[0], [(100.0, 1.0), (0.0, 1.0), (0.0, 0.0), (100.0, 0.0)]);
        // The left side meets the bottom along the diagonal.
        assert_eq!(sides[2], [(0.0, 1.0), (0.0, 0.0), (1.0, 0.0), (1.0, 0.0)]);
    }

    #[test]
    fn cuts_the_content_off_at_the_edges_of_the_page_too() {
        let clip = |x: f32, width: f32| {
            let area = RectF {
                x,
                y: x,
                width,
                height: width,
            };
            let rect = page_content_clip(area, (300.0, 300.0));
            [rect.x, rect.width].map(|value| (value * 100.0).round() / 100.0)
        };
        // A page area inside the page is the clip.
        assert_eq!(clip(20.0, 260.0), [20.0, 260.0]);
        // Negative page margins make the area larger than the page: the page and a device
        // pixel (300 per inch) around it.
        assert_eq!(clip(-20.0, 340.0), [-0.32, 300.8]);
    }

    #[test]
    fn shows_the_content_as_far_as_the_page_margins_not_the_rounded_area() {
        use fagus_layout::PageMarks;
        // A4 with margins of 15mm: Chrome places the area at 57px and lays the content out
        // 681px wide, but cuts it off 680.31px from there, where the margin begins.
        let margin = Au::from_px(15.0 * 96.0 / 25.4);
        let page = Page {
            width: Au::from_px(793.7),
            height: Au::from_px(1122.5),
            fragments: Vec::new(),
            margin: [margin; 4],
            style: Arc::new(ComputedStyle::default()),
            canvas: None,
            bleed: 0.0,
            marks: PageMarks::default(),
            orientation: fagus_layout::PageOrientation::Upright,
            scale: 1.0,
            area: fagus_layout::units::Rect::new(
                Au::from_px(57.0),
                Au::from_px(57.0),
                Au::from_px(681.0),
                Au::from_px(1010.0),
            ),
            shift: 0.0,
            carried: Vec::new(),
        };
        let area = content_area(&page);
        assert_eq!(area.x, 57.0);
        assert!(
            (area.width - (page.width - margin - margin).to_px()).abs() < 0.01,
            "{}",
            area.width
        );
        assert!((area.width - 680.31).abs() < 0.05, "{}", area.width);
        assert!(
            (area.height - (page.height - margin - margin).to_px()).abs() < 0.01,
            "{}",
            area.height
        );
    }

    #[test]
    fn paints_margin_boxes_by_their_z_index_around_the_content() {
        use fagus_layout::units::Rect;
        use fagus_layout::{PageMarks, VisibleSides};
        let margin_box = |z_index: Option<i32>| Fragment {
            rect: Rect::new(Au::ZERO, Au::ZERO, Au::from_px(10.0), Au::from_px(10.0)),
            kind: FragmentKind::Box {
                style: Arc::new(ComputedStyle::from(ComputedValues {
                    z_index,
                    ..ComputedValues::default()
                })),
                node: None,
                sides: VisibleSides::ALL,
            },
            children: Vec::new().into(),
        };
        let area = Rect::new(Au::ZERO, Au::ZERO, Au::from_px(100.0), Au::from_px(100.0));
        let page = Page {
            width: Au::from_px(100.0),
            height: Au::from_px(100.0),
            fragments: vec![
                Fragment {
                    rect: area,
                    kind: FragmentKind::Anonymous,
                    children: Vec::new().into(),
                },
                margin_box(Some(-1)),
                margin_box(None),
                margin_box(Some(-2)),
                margin_box(Some(1)),
                margin_box(Some(0)),
            ],
            margin: [Au::ZERO; 4],
            style: Arc::new(ComputedStyle::default()),
            canvas: None,
            bleed: 0.0,
            marks: PageMarks::default(),
            orientation: fagus_layout::PageOrientation::Upright,
            scale: 1.0,
            area,
            shift: 0.0,
            carried: Vec::new(),
        };
        assert_eq!(stacking_order(&page), vec![3, 1, 0, 2, 5, 4]);
    }

    #[test]
    fn strokes_native_frames_along_the_middle_of_the_border() {
        use fagus_layout::VisibleSides;
        use fagus_style::values::Sides;
        let style = ComputedStyle::from(ComputedValues {
            border_width: Sides::all(1.0),
            border_style: Sides::all(BorderStyle::Solid),
            border_color: Sides::all(Some(Rgba::BLACK)),
            native_frame: Some(0.0),
            ..ComputedValues::default()
        });
        let rect = RectF {
            x: 0.0,
            y: 0.0,
            width: 40.0,
            height: 20.0,
        };
        let border = |sides| {
            let mut items = Vec::new();
            paint_borders(&style, rect, Some([(2.5, 2.5); 4]), sides, &mut items);
            items
        };
        // A hairline between the edges, as Chrome's theme strokes a field.
        assert!(matches!(
            &border(VisibleSides::ALL)[..],
            [Item::RoundedBorder {
                stroke: Some(0.0),
                ..
            }]
        ));
        // A slice of a broken control keeps its border as CSS draws it.
        let sliced = VisibleSides {
            bottom: false,
            ..VisibleSides::ALL
        };
        let items = border(sliced);
        assert_ne!(items, Vec::new());
        assert!(
            !items
                .iter()
                .any(|item| matches!(item, Item::RoundedBorder { stroke: Some(_), .. })),
            "{items:?}"
        );
    }

    #[test]
    fn strokes_borders_of_one_width_and_color_as_chrome() {
        use fagus_layout::VisibleSides;
        use fagus_style::values::Sides;
        let style = |widths: [f32; 4]| {
            ComputedStyle::from(ComputedValues {
                border_width: Sides {
                    top: widths[0],
                    right: widths[1],
                    bottom: widths[2],
                    left: widths[3],
                },
                border_style: Sides::all(BorderStyle::Solid),
                ..ComputedValues::default()
            })
        };
        let rect = RectF {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
        };
        let stroke = |widths: [f32; 4], radii: Option<Radii>| {
            let mut items = Vec::new();
            paint_borders(&style(widths), rect, radii, VisibleSides::ALL, &mut items);
            match &items[..] {
                [Item::RoundedBorder { stroke, .. }] => *stroke,
                _ => panic!("one shape: {items:?}"),
            }
        };
        // Square corners, or corners round all through the border: a stroke along its middle.
        assert_eq!(stroke([1.0; 4], None), Some(1.0));
        assert_eq!(stroke([3.0; 4], Some([(8.0, 8.0); 4])), Some(3.0));
        // Sides of different widths, or corners rounder outside than in: the area between.
        assert_eq!(stroke([1.0, 1.0, 4.0, 1.0], None), None);
        assert_eq!(stroke([3.0; 4], Some([(2.0, 2.0); 4])), None);
    }

    #[test]
    fn rounds_the_corners_of_sides_and_slices_them_at_breaks() {
        use fagus_layout::VisibleSides;
        use fagus_style::values::Sides;
        let round = Some([(8.0, 8.0); 4]);
        let sides = |top, right, bottom, left| VisibleSides {
            top,
            right,
            bottom,
            left,
        };
        // The start of an inline box broken across lines keeps the corners of its start.
        assert_eq!(
            sliced_radii(round, sides(true, false, true, true)),
            Some([(8.0, 8.0), (0.0, 0.0), (0.0, 0.0), (8.0, 8.0)])
        );
        assert_eq!(sliced_radii(round, sides(true, false, true, false)), None);
        // A callout's one solid side is its wedge of the area between the curved edges.
        let style = ComputedStyle::from(ComputedValues {
            border_width: Sides {
                top: 0.0,
                right: 0.0,
                bottom: 0.0,
                left: 4.0,
            },
            border_style: Sides {
                top: BorderStyle::None,
                right: BorderStyle::None,
                bottom: BorderStyle::None,
                left: BorderStyle::Solid,
            },
            ..ComputedValues::default()
        });
        let rect = RectF {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
        };
        let mut items = Vec::new();
        paint_borders(&style, rect, Some([(6.0, 6.0); 4]), VisibleSides::ALL, &mut items);
        let [
            Item::PushClipShape(ClipGeometry::Polygon { points, .. }),
            Item::RoundedBorder {
                inner, inner_radii, ..
            },
            Item::Pop,
        ] = &items[..]
        else {
            panic!("a wedge of a ring: {items:?}");
        };
        // The wedge reaches as far as the corners' curves.
        assert_eq!(points[..], [(0.0, 50.0), (0.0, 0.0), (6.0, 0.0), (6.0, 50.0)]);
        assert_eq!(inner.x, 4.0);
        assert_eq!(inner_radii[0], (2.0, 6.0));
    }

    #[test]
    fn paints_the_colors_and_images_of_the_page_box_but_not_its_gradients() {
        use fagus_style::values::{Gradient, GradientKind, Image, LinearDirection};
        let gradient = Image::Gradient(Arc::new(Gradient {
            kind: GradientKind::Linear(LinearDirection::Angle(180.0)),
            repeating: false,
            stops: Vec::new(),
            interpolation: None,
        }));
        let url = Image::Url(Arc::from("mark.png"), 1.0);
        let style = Arc::new(ComputedStyle::from(ComputedValues {
            background_color: Rgba::from_u8(0xdd, 0xff, 0xdd, 1.0),
            background_image: Arc::from([gradient.clone(), url.clone()]),
            ..ComputedValues::default()
        }));
        // The gradient's layer stays, empty, so that the image keeps its size and position.
        let painted = page_background(&style);
        assert_eq!(&*painted.background_image, &[Image::None, url.clone()]);
        assert_eq!(painted.background_color, style.background_color);
        // A background without gradients is the page's own.
        let plain = Arc::new(ComputedStyle::from(ComputedValues {
            background_image: Arc::from([url]),
            ..ComputedValues::default()
        }));
        assert!(matches!(page_background(&plain), std::borrow::Cow::Borrowed(_)));
    }

    #[test]
    fn draws_the_page_box_the_canvas_and_slices_of_broken_boxes() {
        use fagus_layout::units::Rect;
        use fagus_layout::{PageMarks, VisibleSides};
        let colored = |color: Rgba| {
            Arc::new(ComputedStyle::from(ComputedValues {
                background_color: color,
                ..ComputedValues::default()
            }))
        };
        let red = Rgba {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        let blue = Rgba {
            r: 0.0,
            b: 1.0,
            ..red
        };
        // A box broken across two pages, 60 pixels of it on the first, 40 on the second.
        let page = |height: f32, sides: VisibleSides| {
            let broken = Fragment {
                rect: Rect::new(Au::ZERO, Au::ZERO, Au::from_px(100.0), Au::from_px(height)),
                kind: FragmentKind::Box {
                    style: colored(blue),
                    node: Some(NodeId::DOCUMENT),
                    sides,
                },
                children: Vec::new().into(),
            };
            Page {
                width: Au::from_px(200.0),
                height: Au::from_px(100.0),
                fragments: vec![Fragment {
                    rect: Rect::new(
                        Au::from_px(10.0),
                        Au::from_px(10.0),
                        Au::from_px(180.0),
                        Au::from_px(80.0),
                    ),
                    kind: FragmentKind::Anonymous,
                    children: vec![broken].into(),
                }],
                margin: [Au::from_px(10.0); 4],
                style: colored(red),
                canvas: Some(colored(blue)),
                bleed: 0.0,
                marks: PageMarks::default(),
                orientation: fagus_layout::PageOrientation::Upright,

                scale: 1.0,

                area: Rect::default(),

                shift: 0.0,

                carried: Vec::new(),
            }
        };
        let pages = [
            page(
                60.0,
                VisibleSides {
                    bottom: false,
                    ..VisibleSides::ALL
                },
            ),
            page(
                40.0,
                VisibleSides {
                    top: false,
                    ..VisibleSides::ALL
                },
            ),
        ];
        let slices = slices(&pages);
        let found = |page: &Page| {
            slices
                .get(&std::ptr::from_ref(&page.fragments[0].children[0]))
                .copied()
        };
        let down = |before| Slice {
            before,
            whole: 100.0,
            flow: Flow::Down,
        };
        assert_eq!(found(&pages[0]), Some(down(0.0)));
        assert_eq!(found(&pages[1]), Some(down(60.0)));
        // The page box's background covers the sheet; the canvas lies inside the page margins.
        let mut items = Vec::new();
        page_box(&pages[0], &fagus_layout::Images::default(), &mut items);
        canvas_background(
            &pages[0],
            &slices,
            &fagus_layout::Images::default(),
            false,
            &mut items,
        );
        let rects: Vec<(RectF, Rgba)> = items
            .iter()
            .filter_map(|item| match item {
                Item::Rect { rect, color, .. } => Some((*rect, *color)),
                _ => None,
            })
            .collect();
        let rect = |x, y, width, height| RectF { x, y, width, height };
        assert_eq!(
            rects,
            [
                (rect(0.0, 0.0, 200.0, 100.0), red),
                (rect(10.0, 10.0, 180.0, 80.0), blue)
            ]
        );
        // With a left margin of 10.5px, the content shows from 10px (the margin rounded) as far
        // as the page box is wide: the canvas, from the margin, ends where the content does.
        let mut shifted = page(60.0, VisibleSides::ALL);
        shifted.margin[3] = Au::from_px(10.5);
        shifted.area = Rect::new(
            Au::from_px(10.0),
            Au::from_px(10.0),
            Au::from_px(180.0),
            Au::from_px(80.0),
        );
        let mut items = Vec::new();
        canvas_background(
            &shifted,
            &slices,
            &fagus_layout::Images::default(),
            false,
            &mut items,
        );
        let clip = items.iter().find_map(|item| match item {
            Item::PushClip(rect, None) => Some(*rect),
            _ => None,
        });
        assert_eq!(clip, Some(rect(10.5, 10.0, 179.0, 80.0)));
        // A canvas the content's clip does not cut, or cuts by more than a pixel (page padding
        // beside the area, which the canvas covers), has no clip of its own.
        let canvas_clip = |area: Rect| {
            let mut page = page(60.0, VisibleSides::ALL);
            page.area = area;
            let mut items = Vec::new();
            canvas_background(
                &page,
                &slices,
                &fagus_layout::Images::default(),
                false,
                &mut items,
            );
            items.iter().any(|item| matches!(item, Item::PushClip(..)))
        };
        assert!(!canvas_clip(Rect::new(
            Au::from_px(10.0),
            Au::from_px(10.0),
            Au::from_px(180.0),
            Au::from_px(80.0)
        )));
        assert!(!canvas_clip(Rect::new(
            Au::from_px(40.0),
            Au::from_px(10.0),
            Au::from_px(120.0),
            Au::from_px(80.0)
        )));
    }

    #[test]
    fn paints_the_table_box_under_its_captions() {
        use fagus_layout::units::Rect;
        use fagus_layout::{PageMarks, VisibleSides};
        use fagus_style::values::{Display, DisplayInside, DisplayOutside, Float};
        let color = |g: f32| Rgba {
            r: 0.0,
            g,
            b: 0.0,
            a: 1.0,
        };
        let block = |y: f32, style: ComputedStyle| Fragment {
            rect: Rect::new(Au::ZERO, Au::from_px(y), Au::from_px(100.0), Au::from_px(30.0)),
            kind: FragmentKind::Box {
                style: Arc::new(style),
                node: Some(NodeId::DOCUMENT),
                sides: VisibleSides::ALL,
            },
            children: Vec::new().into(),
        };
        // A table wrapper box: a caption above the table box, which a negative margin pulls
        // over the table's top border, and one below it.
        let order = |float: Float| {
            let table = ComputedStyle::from(ComputedValues {
                display: Display::Normal {
                    outside: DisplayOutside::Block,
                    inside: DisplayInside::Table,
                    list_item: false,
                },
                float,
                background_color: color(0.5),
                ..ComputedValues::default()
            });
            let caption = |g| {
                ComputedStyle::from(ComputedValues {
                    background_color: color(g),
                    ..ComputedValues::default()
                })
            };
            let wrapper = Fragment {
                rect: Rect::new(Au::ZERO, Au::ZERO, Au::from_px(100.0), Au::from_px(70.0)),
                kind: FragmentKind::Anonymous,
                children: vec![
                    block(0.0, caption(0.25)),
                    block(20.0, table),
                    block(40.0, caption(0.75)),
                ]
                .into(),
            };
            let page = Page {
                width: Au::from_px(200.0),
                height: Au::from_px(100.0),
                fragments: vec![Fragment {
                    rect: Rect::new(Au::ZERO, Au::ZERO, Au::from_px(200.0), Au::from_px(100.0)),
                    kind: FragmentKind::Anonymous,
                    children: vec![wrapper].into(),
                }],
                margin: [Au::ZERO; 4],
                style: Arc::new(ComputedStyle::default()),
                canvas: None,
                bleed: 0.0,
                marks: PageMarks::default(),
                orientation: fagus_layout::PageOrientation::Upright,
                scale: 1.0,
                area: Rect::new(Au::ZERO, Au::ZERO, Au::from_px(200.0), Au::from_px(100.0)),
                shift: 0.0,
                carried: Vec::new(),
            };
            let lists = paint(&[page], &FontDb::new(), &fagus_layout::Images::default(), false);
            lists[0]
                .items
                .iter()
                .filter_map(|item| match item {
                    Item::Rect { color, .. } => Some(color.g),
                    _ => None,
                })
                .collect::<Vec<f32>>()
        };
        assert_eq!(order(Float::None), [0.5, 0.25, 0.75]);
        assert_eq!(order(Float::Left), [0.5, 0.25, 0.75]);
    }

    #[test]
    fn cuts_the_content_off_at_the_page_area() {
        use fagus_layout::units::Rect;
        use fagus_layout::{PageMarks, VisibleSides};
        // A box that reaches 30 pixels into the left and top margins.
        let page = Page {
            width: Au::from_px(200.0),
            height: Au::from_px(100.0),
            fragments: vec![Fragment {
                rect: Rect::new(
                    Au::from_px(10.0),
                    Au::from_px(10.0),
                    Au::from_px(180.0),
                    Au::from_px(80.0),
                ),
                kind: FragmentKind::Anonymous,
                children: vec![Fragment {
                    rect: Rect::new(
                        Au::from_px(-30.0),
                        Au::from_px(-30.0),
                        Au::from_px(100.0),
                        Au::from_px(50.0),
                    ),
                    kind: FragmentKind::Box {
                        style: Arc::new(ComputedStyle::from(ComputedValues {
                            background_color: Rgba {
                                r: 0.0,
                                g: 1.0,
                                b: 0.0,
                                a: 1.0,
                            },
                            ..ComputedValues::default()
                        })),
                        node: None,
                        sides: VisibleSides::ALL,
                    },
                    children: Vec::new().into(),
                }]
                .into(),
            }],
            margin: [Au::from_px(10.0); 4],
            style: Arc::new(ComputedStyle::default()),
            canvas: None,
            bleed: 0.0,
            marks: PageMarks::default(),
            orientation: fagus_layout::PageOrientation::Upright,
            scale: 1.0,
            area: Rect::new(
                Au::from_px(10.0),
                Au::from_px(10.0),
                Au::from_px(180.0),
                Au::from_px(80.0),
            ),
            shift: 0.0,
            carried: Vec::new(),
        };
        let lists = paint(&[page], &FontDb::new(), &fagus_layout::Images::default(), false);
        let items = &lists[0].items;
        let clip = items
            .iter()
            .position(|item| matches!(item, Item::PushClip(rect, None) if *rect == RectF { x: 10.0, y: 10.0, width: 180.0, height: 80.0 }))
            .expect("clip to the page area");
        let filled = items
            .iter()
            .position(|item| matches!(item, Item::Rect { rect, .. } if (rect.x + 20.0).abs() < 0.01))
            .expect("the box");
        assert!(clip < filled);
        assert!(matches!(items.last(), Some(Item::Pop)));
    }

    #[test]
    fn clips_overflow_only_along_the_axis_that_clips() {
        use fagus_layout::VisibleSides;
        use fagus_layout::units::Rect;
        use fagus_style::values::Overflow;
        let clipping = |overflow_x: Overflow, overflow_y: Overflow| Fragment {
            rect: Rect::new(Au::ZERO, Au::ZERO, Au::from_px(50.0), Au::from_px(100.0)),
            kind: FragmentKind::Box {
                style: Arc::new(ComputedStyle::from(ComputedValues {
                    overflow_x,
                    overflow_y,
                    ..ComputedValues::default()
                })),
                node: None,
                sides: VisibleSides::ALL,
            },
            children: Vec::new().into(),
        };
        // `overflow-x: clip` alone clips the sides of the box, not what lies above or below it.
        let clip = clip_rect(&clipping(Overflow::Clip, Overflow::Visible), 10.0, 20.0).expect("clip");
        assert_eq!((clip.rect.x, clip.rect.width), (10.0, 50.0));
        assert!(clip.rect.y < -1000.0 && clip.rect.y + clip.rect.height > 1000.0);
        let clip = clip_rect(&clipping(Overflow::Clip, Overflow::Clip), 10.0, 20.0).expect("clip");
        assert_eq!(
            (clip.rect.x, clip.rect.y, clip.rect.width, clip.rect.height),
            (10.0, 20.0, 50.0, 100.0)
        );
        assert!(clip_rect(&clipping(Overflow::Visible, Overflow::Visible), 0.0, 0.0).is_none());
        // `overflow-x: auto` (hidden here) makes `visible` compute to `auto`: both axes clip.
        let clip = clip_rect(&clipping(Overflow::Hidden, Overflow::Visible), 10.0, 20.0).expect("clip");
        assert_eq!((clip.rect.y, clip.rect.height), (20.0, 100.0));
    }

    #[test]
    fn snaps_overflow_clips_to_whole_pixels_like_chrome() {
        use fagus_layout::VisibleSides;
        use fagus_layout::units::Rect;
        use fagus_style::values::Overflow;
        // A box of 100.4 × 50.45 px at (20.3, 10.37): Chrome clips from (20, 10) to (121, 61)
        // (Chrome 157, measured), as it paints the box itself.
        let fragment = Fragment {
            rect: Rect::new(Au::ZERO, Au::ZERO, Au::from_px(100.4), Au::from_px(50.45)),
            kind: FragmentKind::Box {
                style: Arc::new(ComputedStyle::from(ComputedValues {
                    overflow_x: Overflow::Hidden,
                    overflow_y: Overflow::Hidden,
                    ..ComputedValues::default()
                })),
                node: None,
                sides: VisibleSides::ALL,
            },
            children: Vec::new().into(),
        };
        let clip = clip_rect(&fragment, 20.3, 10.37).expect("clip");
        assert_eq!(
            (clip.rect.x, clip.rect.y, clip.rect.width, clip.rect.height),
            (20.0, 10.0, 101.0, 51.0)
        );
    }

    #[test]
    fn clips_tables_at_their_border_box() {
        use fagus_layout::VisibleSides;
        use fagus_layout::units::Rect;
        use fagus_style::values::{Display, DisplayInside, DisplayOutside, Overflow, Sides};
        let clipping = |display: Display| Fragment {
            rect: Rect::new(Au::ZERO, Au::ZERO, Au::from_px(90.0), Au::from_px(90.0)),
            kind: FragmentKind::Box {
                style: Arc::new(ComputedStyle::from(ComputedValues {
                    display,
                    overflow_x: Overflow::Hidden,
                    overflow_y: Overflow::Hidden,
                    border_width: Sides::all(20.0),
                    ..ComputedValues::default()
                })),
                node: None,
                sides: VisibleSides::ALL,
            },
            children: Vec::new().into(),
        };
        // A block clips at its padding box; a table at its border box, as Chrome clips it
        // (css-tables/tentative/paint/overflow-hidden-table).
        let clip = clip_rect(&clipping(Display::BLOCK), 0.0, 0.0).expect("clip");
        assert_eq!((clip.rect.x, clip.rect.width), (20.0, 50.0));
        let table = Display::Normal {
            outside: DisplayOutside::Block,
            inside: DisplayInside::Table,
            list_item: false,
        };
        let clip = clip_rect(&clipping(table), 0.0, 0.0).expect("clip");
        assert_eq!(
            (clip.rect.x, clip.rect.y, clip.rect.width, clip.rect.height),
            (0.0, 0.0, 90.0, 90.0)
        );
    }

    #[test]
    fn clips_absolute_boxes_to_their_clip_rectangles() {
        use fagus_layout::units::Rect;
        use fagus_layout::{PageMarks, VisibleSides};
        use fagus_style::values::Position;
        let colored = |g: f32| Rgba {
            r: 1.0,
            g,
            b: 0.0,
            a: 1.0,
        };
        // `clip: rect(5px, 60px, 25px, auto)`, and `clip: rect(0, 0, 0, 0)` as in `sr-only`.
        let positioned = |x: f32, clip: [Option<f32>; 4], g: f32| Fragment {
            rect: Rect::new(
                Au::from_px(x),
                Au::from_px(0.0),
                Au::from_px(100.0),
                Au::from_px(40.0),
            ),
            kind: FragmentKind::Box {
                style: Arc::new(ComputedStyle::from(ComputedValues {
                    position: Position::Absolute,
                    clip: Some(clip),
                    background_color: colored(g),
                    ..ComputedValues::default()
                })),
                node: None,
                sides: VisibleSides::ALL,
            },
            children: Vec::new().into(),
        };
        let area = Rect::new(
            Au::from_px(0.0),
            Au::from_px(0.0),
            Au::from_px(300.0),
            Au::from_px(100.0),
        );
        let page = Page {
            width: Au::from_px(300.0),
            height: Au::from_px(100.0),
            fragments: vec![Fragment {
                rect: area,
                kind: FragmentKind::Anonymous,
                children: vec![
                    positioned(10.0, [Some(5.0), Some(60.0), Some(25.0), None], 0.5),
                    positioned(150.0, [Some(0.0); 4], 1.0),
                ]
                .into(),
            }],
            margin: [Au::from_px(0.0); 4],
            style: Arc::new(ComputedStyle::default()),
            canvas: None,
            bleed: 0.0,
            marks: PageMarks::default(),
            orientation: fagus_layout::PageOrientation::Upright,
            scale: 1.0,
            area,
            shift: 0.0,
            carried: Vec::new(),
        };
        let lists = paint(&[page], &FontDb::new(), &fagus_layout::Images::default(), false);
        let items = &lists[0].items;
        let clip = items
            .iter()
            .position(|item| matches!(item, Item::PushClip(rect, None) if *rect == RectF { x: 10.0, y: 5.0, width: 60.0, height: 20.0 }))
            .expect("the clip of the first box");
        let filled = items
            .iter()
            .position(|item| matches!(item, Item::Rect { color, .. } if *color == colored(0.5)))
            .expect("the first box");
        assert!(clip < filled);
        // The box clipped to nothing is not painted at all.
        assert!(
            !items
                .iter()
                .any(|item| matches!(item, Item::Rect { color, .. } if *color == colored(1.0)))
        );
    }

    #[test]
    fn measures_text_to_its_advances_rounded_up_to_layout_units() {
        use fagus_layout::units::Rect;
        use fagus_layout::{Glyph, PageMarks, TextRun};
        // Advances of 100.001 pixels reach 100 1/64 pixels, as Blink's text fragment does.
        let glyph = |advance: f32, cluster: u32| Glyph {
            id: 1,
            advance,
            x_offset: 0.0,
            y_offset: 0.0,
            cluster: (cluster, cluster + 1),
        };
        let mut fonts = FontDb::new();
        fonts.add_default_fonts();
        let face = fonts
            .query(&["Inter"], 400.0, fagus_text::FontStyle::Normal)
            .expect("Inter");
        let text = Fragment {
            rect: Rect::new(Au::ZERO, Au::ZERO, Au::from_px(100.0), Au::from_px(20.0)),
            kind: FragmentKind::Text(TextRun {
                face,
                size: 16.0,
                style: Arc::new(ComputedStyle::default()),
                glyphs: vec![glyph(50.0, 0), glyph(50.001, 1)],
                text: "ab".to_owned(),
                baseline: Au::from_px(16.0),
                marker: false,
                symbol: None,
                orientation: fagus_layout::GlyphOrientation::Horizontal,
                exact: false,
            }),
            children: Vec::new().into(),
        };
        // A page area narrower than the text, which overflows it.
        let area = Rect::new(Au::ZERO, Au::ZERO, Au::from_px(80.0), Au::from_px(100.0));
        let page = Page {
            width: Au::from_px(300.0),
            height: Au::from_px(100.0),
            fragments: vec![Fragment {
                rect: area,
                kind: FragmentKind::Anonymous,
                children: vec![text].into(),
            }],
            margin: [Au::ZERO; 4],
            style: Arc::new(ComputedStyle::default()),
            canvas: None,
            bleed: 0.0,
            marks: PageMarks::default(),
            orientation: fagus_layout::PageOrientation::Upright,
            scale: 1.0,
            area,
            shift: 0.0,
            carried: Vec::new(),
        };
        assert!((content_extent(&page) - (100.0 + 1.0 / 64.0)).abs() < 1e-4);
        // Vertical text reaches as far as its column is wide, whatever its advances (which run
        // down the page).
        let mut page = page;
        page.fragments[0].rect.width = Au::from_px(10.0);
        let text = &mut page.fragments[0].children[0];
        text.rect.width = Au::from_px(20.0);
        if let FragmentKind::Text(run) = &mut text.kind {
            run.style = Arc::new(ComputedStyle::from(ComputedValues {
                writing_mode: fagus_style::values::WritingMode::VerticalRl,
                ..ComputedValues::default()
            }));
        }
        assert!((content_extent(&page) - 20.0).abs() < 1e-4);
    }

    #[test]
    fn keeps_the_radii_of_slices_shorter_than_their_corners() {
        use fagus_layout::VisibleSides;
        let round = (LengthPercentage::Px(80.0), LengthPercentage::Px(80.0));
        let style = ComputedStyle::from(ComputedValues {
            border_radius: fagus_style::values::Corners {
                top_left: round.clone(),
                top_right: round.clone(),
                bottom_right: round.clone(),
                bottom_left: round,
            },
            ..ComputedValues::default()
        });
        // A box 200px wide whose first slice is 100px tall: its top corners keep their 80px, the
        // bottom ones being square where it is cut (WPT css-break borders-005, Chrome 157).
        let first = VisibleSides {
            bottom: false,
            ..VisibleSides::ALL
        };
        assert_eq!(
            fragment_radii(&style, 200.0, 100.0, first),
            Some([(80.0, 80.0), (80.0, 80.0), (0.0, 0.0), (0.0, 0.0)])
        );
        // Whole, all four corners shrink to fit its height.
        assert_eq!(
            fragment_radii(&style, 200.0, 100.0, VisibleSides::ALL),
            Some([(50.0, 50.0); 4])
        );
    }

    #[test]
    fn slices_boxes_broken_across_columns_of_vertical_writing_modes() {
        // A slice 100px wide, 100px into a box 290px wide drawn whole: in `vertical-lr` the box
        // goes on to the right of it, in `vertical-rl` to the left.
        let rect = RectF {
            x: 300.0,
            y: 10.0,
            width: 100.0,
            height: 50.0,
        };
        let slice = |flow| Slice {
            before: 100.0,
            whole: 290.0,
            flow,
        };
        let whole = |x, width| RectF { x, width, ..rect };
        assert_eq!(slice(Flow::Right).whole(rect), whole(200.0, 290.0));
        assert_eq!(slice(Flow::Left).whole(rect), whole(210.0, 290.0));
        assert_eq!(
            slice(Flow::Down).whole(rect),
            RectF {
                y: -90.0,
                height: 290.0,
                ..rect
            }
        );
        let vertical = ComputedStyle::from(ComputedValues {
            writing_mode: fagus_style::values::WritingMode::VerticalRl,
            ..ComputedValues::default()
        });
        assert_eq!(Flow::of(&vertical), Flow::Left);
        // Cut on its left, a slice of a `vertical-rl` box is broken; cut on its top, it is not.
        let sides = |top, left| fagus_layout::VisibleSides {
            top,
            left,
            ..fagus_layout::VisibleSides::ALL
        };
        assert!(Flow::Left.cut(sides(true, false)));
        assert!(!Flow::Left.cut(sides(false, true)));
    }

    #[test]
    fn cuts_the_shadows_of_sliced_boxes_where_they_break() {
        use fagus_layout::units::Rect;
        use fagus_layout::{PageMarks, VisibleSides};
        use fagus_style::values::BoxShadow;
        let document = fagus_dom::Document::parse_html("<div></div>");
        let node = document.descendants(NodeId::DOCUMENT).last();
        let style = Arc::new(ComputedStyle::from(ComputedValues {
            box_shadow: Arc::from([BoxShadow {
                color: Some(Rgba::BLACK),
                x: 0.0,
                y: 0.0,
                blur: 4.0,
                spread: 10.0,
                inset: false,
            }]),
            ..ComputedValues::default()
        }));
        // A box 60px tall in two slices of 30px, one on each page.
        let slice = |sides: VisibleSides| Fragment {
            rect: Rect::new(Au::ZERO, Au::ZERO, Au::from_px(100.0), Au::from_px(30.0)),
            kind: FragmentKind::Box {
                style: Arc::clone(&style),
                node,
                sides,
            },
            children: Vec::new().into(),
        };
        let area = Rect::new(Au::ZERO, Au::ZERO, Au::from_px(300.0), Au::from_px(100.0));
        let page = |fragment: Fragment| Page {
            width: Au::from_px(300.0),
            height: Au::from_px(100.0),
            fragments: vec![fragment],
            margin: [Au::ZERO; 4],
            style: Arc::new(ComputedStyle::default()),
            canvas: None,
            bleed: 0.0,
            marks: PageMarks::default(),
            orientation: fagus_layout::PageOrientation::Upright,
            scale: 1.0,
            area,
            shift: 0.0,
            carried: Vec::new(),
        };
        let first = slice(VisibleSides {
            bottom: false,
            ..VisibleSides::ALL
        });
        let last = slice(VisibleSides {
            top: false,
            ..VisibleSides::ALL
        });
        let fonts = FontDb::new();
        let lists = paint(
            &[page(first), page(last)],
            &fonts,
            &fagus_layout::Images::default(),
            false,
        );
        for (index, list) in lists.iter().enumerate() {
            let clip = list.items.iter().find_map(|item| match item {
                Item::PushClip(rect, None) if rect.width > 1000.0 => Some(*rect),
                _ => None,
            });
            let shape = list.items.iter().find_map(|item| match item {
                Item::BoxShadow(paint) => Some(paint.shape),
                _ => None,
            });
            let (Some(clip), Some(shape)) = (clip, shape) else {
                panic!("a cut shadow: {:?}", list.items);
            };
            // The slice going on past the break by the blur, and spread.
            assert_eq!(shape.height, 54.0, "{:?}", list.items);
            assert_eq!(shape.y, if index == 0 { -10.0 } else { -14.0 });
            // Cut at the break: the bottom of the first slice, the top of the last.
            if index == 0 {
                assert_eq!(clip.y + clip.height, 30.0);
            } else {
                assert_eq!(clip.y, 0.0);
            }
        }
    }

    #[test]
    fn prints_boxes_positioned_in_the_initial_containing_block_as_isolated_groups() {
        use fagus_layout::units::Rect;
        use fagus_layout::{PageMarks, VisibleSides};
        use fagus_style::values::{BlendMode, Position};
        // Nodes in the order of the document.
        let document = fagus_dom::Document::parse_html("<p>a</p><p>b</p><p>c</p>");
        let nodes: Vec<NodeId> = document.descendants(NodeId::DOCUMENT).collect();
        let colored = |g: f32| Rgba {
            r: 1.0,
            g,
            b: 0.0,
            a: 1.0,
        };
        let block =
            |node: usize, position: Position, blend: BlendMode, g: f32, children: Vec<Fragment>| Fragment {
                rect: Rect::new(Au::ZERO, Au::ZERO, Au::from_px(100.0), Au::from_px(40.0)),
                kind: FragmentKind::Box {
                    style: Arc::new(ComputedStyle::from(ComputedValues {
                        position,
                        mix_blend_mode: blend,
                        background_color: colored(g),
                        ..ComputedValues::default()
                    })),
                    node: Some(nodes[node]),
                    sides: VisibleSides::ALL,
                },
                children: children.into(),
            };
        let area = Rect::new(Au::ZERO, Au::ZERO, Au::from_px(300.0), Au::from_px(100.0));
        let page = |children: Vec<Fragment>| Page {
            width: Au::from_px(300.0),
            height: Au::from_px(100.0),
            fragments: vec![Fragment {
                rect: area,
                kind: FragmentKind::Anonymous,
                children: children.into(),
            }],
            margin: [Au::ZERO; 4],
            style: Arc::new(ComputedStyle::default()),
            canvas: None,
            bleed: 0.0,
            marks: PageMarks::default(),
            orientation: fagus_layout::PageOrientation::Upright,
            scale: 1.0,
            area,
            shift: 0.0,
            carried: Vec::new(),
        };
        // A box that blends before the absolutely positioned one (which layout puts after the
        // content in flow, though it comes first in the document) blends; the absolutely
        // positioned box and the box after it are isolated groups, painted in document order.
        let body = block(
            3,
            Position::Static,
            BlendMode::Normal,
            0.0,
            vec![
                block(4, Position::Static, BlendMode::Multiply, 0.2, Vec::new()),
                block(8, Position::Relative, BlendMode::Multiply, 0.4, Vec::new()),
            ],
        );
        let absolute = block(6, Position::Absolute, BlendMode::Multiply, 0.6, Vec::new());
        let lists = paint(
            &[page(vec![body, absolute])],
            &FontDb::new(),
            &fagus_layout::Images::default(),
            false,
        );
        let items = &lists[0].items;
        let filled = |g: f32| {
            items
                .iter()
                .position(|item| matches!(item, Item::Rect { color, .. } if *color == colored(g)))
                .expect("box")
        };
        let blends: Vec<usize> = items
            .iter()
            .enumerate()
            .filter(|(_, item)| matches!(item, Item::PushBlend(_)))
            .map(|(index, _)| index)
            .collect();
        assert_eq!(blends.len(), 1, "{items:?}");
        assert!(blends[0] < filled(0.2));
        // The blending box blends as one group, not each thing it draws on its own.
        assert!(matches!(items[blends[0] + 1], Item::PushIsolated), "{items:?}");
        assert!(
            filled(0.2) < filled(0.6) && filled(0.6) < filled(0.4),
            "{items:?}"
        );
        // The page's content is an isolated group too, which leaves the canvas below out of
        // what blends.
        let first_group = items
            .iter()
            .position(|item| matches!(item, Item::PushIsolated))
            .expect("group");
        assert!(first_group < blends[0] && first_group < filled(0.2), "{items:?}");
        assert_eq!(
            items
                .iter()
                .filter(|item| matches!(item, Item::PushIsolated))
                .count(),
            4
        );
    }

    #[test]
    fn orders_boxes_of_no_element_by_the_element_around_them() {
        use fagus_layout::units::Rect;
        use fagus_layout::{PageMarks, VisibleSides};
        use fagus_style::values::Position;
        // The cells of a relatively positioned table row are held in a box of no element (see
        // `fagus_layout`'s tables): it paints after an absolutely positioned box that comes
        // before the table in the document, though layout puts that box after the table.
        let document = fagus_dom::Document::parse_html("<div><p>a</p><table></table></div>");
        let nodes: Vec<NodeId> = document.descendants(NodeId::DOCUMENT).collect();
        let (paragraph, table) = (
            nodes
                .iter()
                .copied()
                .find(|&node| document.element(node).is_some_and(|data| data.is("p")))
                .expect("p"),
            nodes
                .iter()
                .copied()
                .find(|&node| document.element(node).is_some_and(|data| data.is("table")))
                .expect("table"),
        );
        let colored = |g: f32| Rgba {
            r: 1.0,
            g,
            b: 0.0,
            a: 1.0,
        };
        let fragment = |node: Option<NodeId>, position: Position, g: f32, children: Vec<Fragment>| Fragment {
            rect: Rect::new(Au::ZERO, Au::ZERO, Au::from_px(100.0), Au::from_px(40.0)),
            kind: FragmentKind::Box {
                style: Arc::new(ComputedStyle::from(ComputedValues {
                    position,
                    background_color: colored(g),
                    ..ComputedValues::default()
                })),
                node,
                sides: VisibleSides::ALL,
            },
            children: children.into(),
        };
        let row = fragment(None, Position::Relative, 0.4, Vec::new());
        let table = fragment(Some(table), Position::Static, 0.0, vec![row]);
        let absolute = fragment(Some(paragraph), Position::Absolute, 0.6, Vec::new());
        let area = Rect::new(Au::ZERO, Au::ZERO, Au::from_px(300.0), Au::from_px(100.0));
        let page = Page {
            width: Au::from_px(300.0),
            height: Au::from_px(100.0),
            fragments: vec![Fragment {
                rect: area,
                kind: FragmentKind::Anonymous,
                children: vec![table, absolute].into(),
            }],
            margin: [Au::ZERO; 4],
            style: Arc::new(ComputedStyle::default()),
            canvas: None,
            bleed: 0.0,
            marks: PageMarks::default(),
            orientation: fagus_layout::PageOrientation::Upright,
            scale: 1.0,
            area,
            shift: 0.0,
            carried: Vec::new(),
        };
        let lists = paint(&[page], &FontDb::new(), &fagus_layout::Images::default(), false);
        let items = &lists[0].items;
        let filled = |g: f32| {
            items
                .iter()
                .position(|item| matches!(item, Item::Rect { color, .. } if *color == colored(g)))
                .expect("box")
        };
        assert!(filled(0.6) < filled(0.4), "{items:?}");
    }

    #[test]
    fn paints_absolute_before_pseudo_elements_before_the_children_of_their_element() {
        use fagus_layout::units::Rect;
        use fagus_layout::{PageMarks, VisibleSides};
        use fagus_style::values::Position;
        // `.line::before` is absolutely positioned in `.line`, which layout puts after `.line`'s
        // relatively positioned child: it paints before the child all the same, as it comes
        // before it in tree order (Chrome 157: a timeline's line under its dots).
        let document = fagus_dom::Document::parse_html("<div class=line><div class=dot></div></div>");
        let element = |class: &str| {
            document
                .descendants(NodeId::DOCUMENT)
                .find(|&node| {
                    document
                        .element(node)
                        .is_some_and(|data| data.classes.iter().any(|name| &**name == class))
                })
                .expect("element")
        };
        let (line, dot) = (element("line"), element("dot"));
        let colored = |g: f32| Rgba {
            r: 1.0,
            g,
            b: 0.0,
            a: 1.0,
        };
        let fragment = |node: Option<NodeId>,
                        before_of: Option<NodeId>,
                        position: Position,
                        g: f32,
                        children: Vec<Fragment>| {
            Fragment {
                rect: Rect::new(Au::ZERO, Au::ZERO, Au::from_px(100.0), Au::from_px(40.0)),
                kind: FragmentKind::Box {
                    style: Arc::new(ComputedStyle::from(ComputedValues {
                        position,
                        background_color: colored(g),
                        before_of,
                        ..ComputedValues::default()
                    })),
                    node,
                    sides: VisibleSides::ALL,
                },
                children: children.into(),
            }
        };
        let child = fragment(Some(dot), None, Position::Relative, 0.4, Vec::new());
        let before = fragment(None, Some(line), Position::Absolute, 0.6, Vec::new());
        let container = fragment(Some(line), None, Position::Relative, 0.2, vec![child, before]);
        let area = Rect::new(Au::ZERO, Au::ZERO, Au::from_px(300.0), Au::from_px(100.0));
        let page = Page {
            width: Au::from_px(300.0),
            height: Au::from_px(100.0),
            fragments: vec![container],
            margin: [Au::ZERO; 4],
            style: Arc::new(ComputedStyle::default()),
            canvas: None,
            bleed: 0.0,
            marks: PageMarks::default(),
            orientation: fagus_layout::PageOrientation::Upright,
            scale: 1.0,
            area,
            shift: 0.0,
            carried: Vec::new(),
        };
        let lists = paint(&[page], &FontDb::new(), &fagus_layout::Images::default(), false);
        let items = &lists[0].items;
        let filled = |g: f32| {
            items
                .iter()
                .position(|item| matches!(item, Item::Rect { color, .. } if *color == colored(g)))
                .expect("box")
        };
        assert!(
            filled(0.2) < filled(0.6) && filled(0.6) < filled(0.4),
            "{items:?}"
        );
    }

    #[test]
    fn paints_the_page_background_over_the_page_rounded_to_whole_pixels() {
        use fagus_layout::PageMarks;
        use fagus_layout::units::Rect;
        // An A5 page, 559.37 × 793.7 pixels: its background is 559 × 794 pixels large.
        let color = Rgba::from_u8(0x1d, 0x35, 0x57, 1.0);
        let (width, height) = (Au::from_px(559.37), Au::from_px(793.7));
        let page = Page {
            width,
            height,
            fragments: Vec::new(),
            margin: [Au::ZERO; 4],
            style: Arc::new(ComputedStyle::from(ComputedValues {
                background_color: color,
                ..ComputedValues::default()
            })),
            canvas: None,
            bleed: 0.0,
            marks: PageMarks::default(),
            orientation: fagus_layout::PageOrientation::Upright,
            scale: 1.0,
            area: Rect::new(Au::ZERO, Au::ZERO, width, height),
            shift: 0.0,
            carried: Vec::new(),
        };
        let lists = paint(&[page], &FontDb::new(), &fagus_layout::Images::default(), false);
        let rect = lists[0]
            .items
            .iter()
            .find_map(|item| match item {
                Item::Rect {
                    rect, color: fill, ..
                } if *fill == color => Some(*rect),
                _ => None,
            })
            .expect("page background");
        assert_eq!((rect.width, rect.height), (559.0, 794.0));
    }

    #[test]
    fn paints_absolute_boxes_after_their_containing_block_in_later_columns() {
        use fagus_layout::units::Rect;
        use fagus_layout::{PageMarks, VisibleSides};
        use fagus_style::values::Position;
        // Nodes in the order of the document.
        let document = fagus_dom::Document::parse_html("<p>a</p><p>b</p><p>c</p>");
        let nodes: Vec<NodeId> = document.descendants(NodeId::DOCUMENT).collect();
        let colored = |g: f32| Rgba {
            r: 1.0,
            g,
            b: 0.0,
            a: 1.0,
        };
        let block = |node: usize, position: Position, g: f32, children: Vec<Fragment>| Fragment {
            rect: Rect::new(Au::ZERO, Au::ZERO, Au::from_px(100.0), Au::from_px(40.0)),
            kind: FragmentKind::Box {
                style: Arc::new(ComputedStyle::from(ComputedValues {
                    position,
                    background_color: colored(g),
                    ..ComputedValues::default()
                })),
                node: Some(nodes[node]),
                sides: VisibleSides::ALL,
            },
            children: children.into(),
        };
        let column = |children: Vec<Fragment>| Fragment {
            rect: Rect::new(Au::ZERO, Au::ZERO, Au::from_px(100.0), Au::from_px(100.0)),
            kind: FragmentKind::Anonymous,
            children: children.into(),
        };
        let area = Rect::new(Au::ZERO, Au::ZERO, Au::from_px(300.0), Au::from_px(100.0));
        // The part of a later absolutely positioned box (node 8) in the first column comes
        // before the containing block (node 4), which holds the part of an earlier one (node 6)
        // in the second: that one is painted after its containing block all the same.
        let page = Page {
            width: Au::from_px(300.0),
            height: Au::from_px(100.0),
            fragments: vec![Fragment {
                rect: area,
                kind: FragmentKind::Anonymous,
                children: vec![
                    column(vec![block(8, Position::Absolute, 0.6, Vec::new())]),
                    column(vec![block(
                        4,
                        Position::Relative,
                        0.2,
                        vec![block(6, Position::Absolute, 0.4, Vec::new())],
                    )]),
                ]
                .into(),
            }],
            margin: [Au::ZERO; 4],
            style: Arc::new(ComputedStyle::default()),
            canvas: None,
            bleed: 0.0,
            marks: PageMarks::default(),
            orientation: fagus_layout::PageOrientation::Upright,
            scale: 1.0,
            area,
            shift: 0.0,
            carried: Vec::new(),
        };
        let lists = paint(&[page], &FontDb::new(), &fagus_layout::Images::default(), false);
        let items = &lists[0].items;
        let filled = |g: f32| {
            items
                .iter()
                .position(|item| matches!(item, Item::Rect { color, .. } if *color == colored(g)))
                .expect("box")
        };
        assert!(filled(0.2) < filled(0.4), "{items:?}");
    }

    #[test]
    fn draws_what_reaches_past_the_page_area_on_the_pages_after() {
        use fagus_layout::units::Rect;
        use fagus_layout::{Carried, PageMarks, VisibleSides};
        let px = Au::from_px;
        let filled = |y: f32, height: f32, children: Vec<Fragment>| Fragment {
            rect: Rect::new(Au::ZERO, px(y), px(100.0), px(height)),
            kind: FragmentKind::Box {
                style: Arc::new(ComputedStyle::from(ComputedValues {
                    background_color: Rgba {
                        r: 0.0,
                        g: 1.0,
                        b: 0.0,
                        a: 1.0,
                    },
                    ..ComputedValues::default()
                })),
                node: None,
                sides: VisibleSides::ALL,
            },
            children: children.into(),
        };
        let page = |children: Vec<Fragment>, carried: Vec<Carried>| Page {
            width: px(200.0),
            height: px(100.0),
            fragments: vec![Fragment {
                rect: Rect::new(px(10.0), px(10.0), px(180.0), px(80.0)),
                kind: FragmentKind::Anonymous,
                children: children.into(),
            }],
            margin: [px(10.0); 4],
            style: Arc::new(ComputedStyle::default()),
            canvas: None,
            bleed: 0.0,
            marks: PageMarks::default(),
            orientation: fagus_layout::PageOrientation::Upright,
            scale: 1.0,
            area: Rect::new(px(10.0), px(10.0), px(180.0), px(80.0)),
            shift: 0.0,
            carried,
        };
        // A box 20 pixels down the first page area reaching 100 pixels below its end, and a box
        // above it that does not: the second page shows the first box 80 pixels higher, the
        // third 160 pixels higher; neither shows the other box.
        let first = page(
            vec![filled(0.0, 10.0, Vec::new()), filled(20.0, 160.0, Vec::new())],
            Vec::new(),
        );
        let onto = |page: usize| Carried {
            page: 0,
            shift: (Au::ZERO, px(-80.0) * i32::try_from(page).unwrap_or(0)),
            from: px(10.0 + 80.0 * page as f32),
            to: px(90.0 + 80.0 * page as f32),
        };
        let pages = [
            first,
            page(Vec::new(), vec![onto(1)]),
            page(Vec::new(), vec![onto(2)]),
        ];
        let lists = paint(&pages, &FontDb::new(), &fagus_layout::Images::default(), false);
        let boxes = |page: usize| -> Vec<RectF> {
            lists[page]
                .items
                .iter()
                .filter_map(|item| match item {
                    Item::Rect { rect, .. } => Some(*rect),
                    _ => None,
                })
                .collect()
        };
        let rect = |y, height| RectF {
            x: 10.0,
            y,
            width: 100.0,
            height,
        };
        assert_eq!(boxes(0), [rect(10.0, 10.0), rect(30.0, 160.0)]);
        assert_eq!(boxes(1), [rect(-50.0, 160.0)]);
        assert_eq!(boxes(2), [rect(-130.0, 160.0)]);
    }

    #[test]
    fn strokes_rounded_dashed_borders_along_their_middle() {
        use fagus_layout::VisibleSides;
        use fagus_style::values::Sides;
        let style = |border_style| {
            ComputedStyle::from(ComputedValues {
                border_width: Sides::all(3.0),
                border_style: Sides::all(border_style),
                ..ComputedValues::default()
            })
        };
        let rect = RectF {
            x: 20.0,
            y: 20.0,
            width: 120.0,
            height: 80.0,
        };
        let radii = Some([(16.0, 16.0); 4]);
        // Dashes of one width and color all round are one stroke, along the middle cut to
        // whole pixels, as Chrome strokes them.
        let mut items = Vec::new();
        paint_borders(
            &style(BorderStyle::Dashed),
            rect,
            radii,
            VisibleSides::ALL,
            &mut items,
        );
        let [
            Item::DashedBorder {
                center,
                center_radii,
                width,
                stroke,
                ..
            },
        ] = items.as_slice()
        else {
            panic!("one dashed border: {items:?}");
        };
        assert_eq!(
            (center.x, center.y, center.width, center.height),
            (21.0, 21.0, 118.0, 78.0)
        );
        assert_eq!((*center_radii, *width, *stroke), ([(15.0, 15.0); 4], 3.0, 3.0));
        // A double border is two rings; a groove's sides each have their shade, in their wedges.
        let mut items = Vec::new();
        paint_borders(
            &style(BorderStyle::Double),
            rect,
            radii,
            VisibleSides::ALL,
            &mut items,
        );
        assert_eq!(items.len(), 2);
        let mut items = Vec::new();
        paint_borders(
            &style(BorderStyle::Groove),
            rect,
            radii,
            VisibleSides::ALL,
            &mut items,
        );
        let wedges = items
            .iter()
            .filter(|item| matches!(item, Item::PushClipShape(_)))
            .count();
        assert_eq!((wedges, items.len()), (4, 16));
    }

    #[test]
    fn draws_outlines_around_the_border_box() {
        let bounds = |items: &[Item]| {
            items.iter().fold(
                (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY),
                |(left, top, right, bottom), item| match item {
                    Item::BorderSide { points, .. } => points.iter().fold(
                        (left, top, right, bottom),
                        |(left, top, right, bottom), &(x, y)| {
                            (left.min(x), top.min(y), right.max(x), bottom.max(y))
                        },
                    ),
                    Item::RoundedBorder { outer, .. } => (
                        left.min(outer.x),
                        top.min(outer.y),
                        right.max(outer.x + outer.width),
                        bottom.max(outer.y + outer.height),
                    ),
                    _ => (left, top, right, bottom),
                },
            )
        };
        let rect = RectF {
            x: 100.0,
            y: 100.0,
            width: 60.0,
            height: 40.0,
        };
        let outline = |width: f32, offset: f32, style: OutlineStyle| {
            let style = ComputedStyle::from(ComputedValues {
                outline_width: width,
                outline_style: style,
                outline_offset: offset,
                ..ComputedValues::default()
            });
            let mut items = Vec::new();
            paint_outline(&style, rect, None, &mut items);
            bounds(&items)
        };
        // Outside the border box, `outline-offset` away from it; inside with a negative one.
        assert_eq!(
            outline(2.0, 0.0, OutlineStyle::Line(BorderStyle::Solid)),
            (98.0, 98.0, 162.0, 142.0)
        );
        assert_eq!(
            outline(2.0, 3.0, OutlineStyle::Line(BorderStyle::Solid)),
            (95.0, 95.0, 165.0, 145.0)
        );
        assert_eq!(
            outline(2.0, -6.0, OutlineStyle::Line(BorderStyle::Solid)),
            (104.0, 104.0, 156.0, 136.0)
        );
        // A negative offset stops at the middle of the box, in each axis apart (WPT
        // `css-ui/outline-013`): the 60 × 40 box leaves an outline 20px wide around its middle.
        assert_eq!(
            outline(10.0, -25.0, OutlineStyle::Line(BorderStyle::Solid)),
            (115.0, 110.0, 145.0, 130.0)
        );
        // The focus ring: a black ring on the edge, a white one around it.
        assert_eq!(outline(3.0, 0.0, OutlineStyle::Auto), (98.0, 98.0, 162.0, 142.0));
    }

    #[test]
    fn clips_to_the_shapes_of_clip_path() {
        use fagus_style::values::{BoxArea, ClipPath, ClipShape, Corners, ShapeRadius};
        let px = LengthPercentage::Px;
        let percent = LengthPercentage::Percent;
        let border_box = RectF {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
        };
        let clip = |shape: ClipShape, reference: BoxArea| {
            let style = ComputedStyle::from(ComputedValues {
                border_width: fagus_style::values::Sides {
                    top: 5.0,
                    right: 5.0,
                    bottom: 5.0,
                    left: 5.0,
                },
                ..ComputedValues::default()
            });
            clip_item(
                &style,
                &ClipPath {
                    shape,
                    reference,
                    svg_reference: fagus_style::values::TransformBox::StrokeBox,
                },
                border_box,
            )
        };
        // A circle of the closest side about the center of the border box.
        let circle = ClipShape::Ellipse {
            radii: (ShapeRadius::ClosestSide, ShapeRadius::ClosestSide),
            circle: true,
            center: (percent(0.5), percent(0.5)),
        };
        assert_eq!(
            clip(circle, BoxArea::BorderBox),
            Item::PushClipShape(ClipGeometry::Ellipse {
                center: (60.0, 45.0),
                radii: (25.0, 25.0)
            })
        );
        // Insets from the padding box, percentages of its size.
        let inset = ClipShape::Inset {
            insets: [px(5.0), percent(0.1), px(5.0), px(0.0)],
            radii: Corners::default(),
        };
        assert_eq!(
            clip(inset, BoxArea::PaddingBox),
            Item::PushClip(
                RectF {
                    x: 15.0,
                    y: 30.0,
                    width: 81.0,
                    height: 30.0
                },
                None
            )
        );
        let triangle = ClipShape::Polygon {
            even_odd: false,
            points: vec![
                (percent(0.5), px(0.0)),
                (percent(1.0), percent(1.0)),
                (px(0.0), percent(1.0)),
            ],
        };
        assert_eq!(
            clip(triangle, BoxArea::BorderBox),
            Item::PushClipShape(ClipGeometry::Polygon {
                points: vec![(60.0, 20.0), (110.0, 70.0), (10.0, 70.0)],
                even_odd: false
            })
        );
        // The margin box, with corners that grow less than its margins where they are smaller.
        let style = ComputedStyle::from(ComputedValues {
            margin: fagus_style::values::Sides {
                top: fagus_style::values::LengthPercentageAuto::Length(px(50.0)),
                right: fagus_style::values::LengthPercentageAuto::Length(percent(0.1)),
                bottom: fagus_style::values::LengthPercentageAuto::Auto,
                left: fagus_style::values::LengthPercentageAuto::Length(px(50.0)),
            },
            ..ComputedValues::default()
        });
        let (margin_box, radii) =
            background::area(&style, border_box, Some([(10.0, 10.0); 4]), BoxArea::MarginBox);
        assert_eq!(
            margin_box,
            RectF {
                x: -40.0,
                y: -30.0,
                width: 160.0,
                height: 100.0
            }
        );
        let radii = radii.expect("radii");
        assert!(
            (radii[0].0 - 34.4).abs() < 1e-3 && (radii[0].1 - 34.4).abs() < 1e-3,
            "{radii:?}"
        );
        assert_eq!(radii[2], (20.0, 10.0));
    }

    #[test]
    fn casts_the_shadows_of_text_on_its_emphasis_marks() {
        use fagus_layout::units::Rect;
        use fagus_layout::{PageMarks, TextRun};
        use fagus_style::values::{EmphasisMark, EmphasisShape, TextShadow};
        let mut fonts = FontDb::new();
        fonts.add_default_fonts();
        let face = fonts
            .query(&["Inter"], 400.0, fagus_text::FontStyle::Normal)
            .expect("Inter");
        let scale = fonts.face(face).scale(16.0);
        let glyphs = fonts
            .shape(face, "ab")
            .glyphs
            .iter()
            .map(|glyph| Glyph {
                id: glyph.id,
                advance: glyph.x_advance * scale,
                x_offset: 0.0,
                y_offset: 0.0,
                cluster: (glyph.cluster, glyph.cluster + 1),
            })
            .collect();
        let red = Rgba {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        let run = TextRun {
            face,
            size: 16.0,
            style: Arc::new(ComputedStyle::from(ComputedValues {
                font_family: Arc::from(["Inter".to_owned()]),
                text_emphasis_style: Some(EmphasisMark::Shape {
                    shape: EmphasisShape::Dot,
                    filled: true,
                }),
                text_shadow: Arc::from([TextShadow {
                    x: 3.0,
                    y: 3.0,
                    blur: 0.0,
                    color: Some(red),
                }]),
                ..ComputedValues::default()
            })),
            glyphs,
            text: "ab".to_owned(),
            baseline: Au::from_px(15.0),
            marker: false,
            symbol: None,
            orientation: fagus_layout::GlyphOrientation::Horizontal,
            exact: false,
        };
        let page = Page {
            width: Au::from_px(200.0),
            height: Au::from_px(100.0),
            fragments: vec![Fragment {
                rect: Rect::new(Au::ZERO, Au::ZERO, Au::from_px(200.0), Au::from_px(100.0)),
                kind: FragmentKind::Anonymous,
                children: vec![Fragment {
                    rect: Rect::new(Au::ZERO, Au::from_px(20.0), Au::from_px(20.0), Au::from_px(20.0)),
                    kind: FragmentKind::Text(run),
                    children: Vec::new().into(),
                }]
                .into(),
            }],
            margin: [Au::ZERO; 4],
            style: Arc::new(ComputedStyle::default()),
            canvas: None,
            bleed: 0.0,
            marks: PageMarks::default(),
            orientation: fagus_layout::PageOrientation::Upright,

            scale: 1.0,

            area: Rect::default(),

            shift: 0.0,

            carried: Vec::new(),
        };
        let lists = paint(&[page], &fonts, &fagus_layout::Images::default(), false);
        // The marks, as the text, again in the shadow's color and offset below them.
        let marks: Vec<(f32, f32, Rgba)> = lists[0]
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Text {
                    x, y, color, text, ..
                } if text.starts_with('\u{2022}') => Some((*x, *y, *color)),
                _ => None,
            })
            .collect();
        assert_eq!(marks.len(), 2, "{marks:?}");
        assert_eq!(marks[0].2, red);
        assert!((marks[0].0 - marks[1].0 - 3.0).abs() < 0.01 && (marks[0].1 - marks[1].1 - 3.0).abs() < 0.01);
    }

    #[test]
    fn draws_stretched_operators_and_math_bars_as_chrome() {
        use fagus_layout::units::Rect;
        use fagus_layout::{PageMarks, TextRun, VisibleSides};
        let mut fonts = FontDb::new();
        fonts.add_default_fonts();
        let face = fonts
            .query(&["Inter"], 400.0, fagus_text::FontStyle::Normal)
            .expect("Inter");
        let rect = |x: f32, y: f32, width: f32, height: f32| {
            Rect::new(
                Au::from_px(x),
                Au::from_px(y),
                Au::from_px(width),
                Au::from_px(height),
            )
        };
        let run = |text: &str, exact: bool| Fragment {
            rect: rect(0.0, 0.0, 10.0, 16.0),
            kind: FragmentKind::Text(TextRun {
                face,
                size: 16.0,
                style: Arc::new(ComputedStyle::default()),
                glyphs: Vec::new(),
                text: text.to_owned(),
                baseline: Au::from_px(16.0),
                marker: false,
                symbol: None,
                orientation: fagus_layout::GlyphOrientation::Horizontal,
                exact,
            }),
            children: Vec::new().into(),
        };
        // A bar 1.25 px thick from y = 30.375: snapped from 30 to 32, 2 px, then moved up by
        // half that. One 0.5 px thick at 40.125 snaps to nothing but stays a pixel.
        let bar = |y: f32, height: f32| Fragment {
            rect: rect(5.0, y, 20.0, height),
            kind: FragmentKind::Box {
                style: Arc::new(ComputedStyle::from(ComputedValues {
                    background_color: Rgba::BLACK,
                    math_bar: true,
                    ..ComputedValues::default()
                })),
                node: None,
                sides: VisibleSides::ALL,
            },
            children: Vec::new().into(),
        };
        // A line at a fraction of a pixel: its text moves to a whole pixel, a stretched
        // operator stays where it is.
        let line = Fragment {
            rect: rect(0.0, 10.3125, 100.0, 20.0),
            kind: FragmentKind::Line {
                baseline: Au::from_px(16.0),
            },
            children: vec![run("x", false), run("(", true)].into(),
        };
        let page = Page {
            width: Au::from_px(200.0),
            height: Au::from_px(100.0),
            fragments: vec![Fragment {
                rect: rect(0.0, 0.0, 200.0, 100.0),
                kind: FragmentKind::Anonymous,
                children: vec![line, bar(30.375, 1.25), bar(40.125, 0.5)].into(),
            }],
            margin: [Au::ZERO; 4],
            style: Arc::new(ComputedStyle::default()),
            canvas: None,
            bleed: 0.0,
            marks: PageMarks::default(),
            orientation: fagus_layout::PageOrientation::Upright,
            scale: 1.0,
            area: Rect::default(),
            shift: 0.0,
            carried: Vec::new(),
        };
        let lists = paint(&[page], &fonts, &fagus_layout::Images::default(), false);
        let items = &lists[0].items;
        let baselines: Vec<(String, f32)> = items
            .iter()
            .filter_map(|item| match item {
                Item::Text { y, text, .. } => Some((text.clone(), *y)),
                _ => None,
            })
            .collect();
        assert_eq!(baselines, [("x".to_owned(), 26.0), ("(".to_owned(), 26.3125)]);
        let rects: Vec<RectF> = items
            .iter()
            .filter_map(|item| match item {
                Item::Rect { rect, .. } => Some(*rect),
                _ => None,
            })
            .collect();
        let snapped = |x: f32, y: f32, width: f32, height: f32| RectF { x, y, width, height };
        assert_eq!(
            rects,
            [snapped(5.0, 29.0, 20.0, 2.0), snapped(5.0, 40.0, 20.0, 1.0)]
        );
    }

    #[test]
    fn snaps_boxes_text_and_list_symbols_to_whole_pixels_like_chrome() {
        use fagus_layout::units::Rect;
        use fagus_layout::{MarkerSymbol, PageMarks, SymbolShape, TextRun, VisibleSides};
        let mut fonts = FontDb::new();
        fonts.add_default_fonts();
        let face = fonts
            .query(&["Inter"], 400.0, fagus_text::FontStyle::Normal)
            .expect("Inter");
        let rect = |x: f32, y: f32, width: f32, height: f32| {
            Rect::new(
                Au::from_px(x),
                Au::from_px(y),
                Au::from_px(width),
                Au::from_px(height),
            )
        };
        let run = |text: &str, symbol: Option<MarkerSymbol>| TextRun {
            face,
            size: 16.0,
            style: Arc::new(ComputedStyle::default()),
            glyphs: Vec::new(),
            text: text.to_owned(),
            baseline: Au::from_px(16.0),
            marker: symbol.is_some(),
            symbol,
            orientation: fagus_layout::GlyphOrientation::Horizontal,
            exact: false,
        };
        let text = |x: f32, run: TextRun| Fragment {
            rect: rect(x, 0.0, 20.0, 20.0),
            kind: FragmentKind::Text(run),
            children: Vec::new().into(),
        };
        let bullet = MarkerSymbol {
            shape: SymbolShape::Disc,
            x: 21.0,
            y: 9.0,
            size: 5.0,
        };
        // A box at a fraction of a pixel, with a line of text and a marker in it.
        let block = Fragment {
            rect: rect(10.375, 10.3125, 30.25, 20.375),
            kind: FragmentKind::Box {
                style: Arc::new(ComputedStyle::from(ComputedValues {
                    background_color: Rgba::BLACK,
                    ..ComputedValues::default()
                })),
                node: None,
                sides: VisibleSides::ALL,
            },
            children: vec![Fragment {
                rect: rect(0.0, 0.3125, 30.25, 20.0),
                kind: FragmentKind::Line {
                    baseline: Au::from_px(16.0),
                },
                children: vec![text(-17.625, run("", Some(bullet))), text(0.0, run("x", None))].into(),
            }]
            .into(),
        };
        let page = Page {
            width: Au::from_px(200.0),
            height: Au::from_px(100.0),
            fragments: vec![Fragment {
                rect: rect(0.0, 0.0, 200.0, 100.0),
                kind: FragmentKind::Anonymous,
                children: vec![block].into(),
            }],
            margin: [Au::ZERO; 4],
            style: Arc::new(ComputedStyle::default()),
            canvas: None,
            bleed: 0.0,
            marks: PageMarks::default(),
            orientation: fagus_layout::PageOrientation::Upright,

            scale: 1.0,

            area: Rect::default(),

            shift: 0.0,

            carried: Vec::new(),
        };
        let lists = paint(&[page], &fonts, &fagus_layout::Images::default(), false);
        let items = &lists[0].items;
        let rects: Vec<RectF> = items
            .iter()
            .filter_map(|item| match item {
                Item::Rect { rect, .. } => Some(*rect),
                _ => None,
            })
            .collect();
        let snapped = |x: f32, y: f32, width: f32, height: f32| RectF { x, y, width, height };
        // The background on whole pixels, its far edges rounded too.
        assert!(rects.contains(&snapped(10.0, 10.0, 31.0, 21.0)), "{rects:?}");
        // The line's top (10.625) rounds to 11, the baseline 16 below it.
        let baseline = items.iter().find_map(|item| match item {
            Item::Text { x, y, .. } => Some((*x, *y)),
            _ => None,
        });
        assert_eq!(baseline, Some((10.375, 27.0)));
        // The bullet's square at (13.75, 20.0), moved with the line, snapped.
        assert!(rects.contains(&snapped(14.0, 20.0, 5.0, 5.0)), "{rects:?}");
    }

    #[test]
    fn moves_text_with_inline_boxes_that_hold_another_font_to_whole_pixels() {
        use fagus_layout::units::Rect;
        use fagus_layout::{PageMarks, TextRun, VisibleSides};
        let mut fonts = FontDb::new();
        fonts.add_default_fonts();
        let face = fonts
            .query(&["Inter"], 400.0, fagus_text::FontStyle::Normal)
            .expect("Inter");
        let rect =
            |y: f32, height: f32| Rect::new(Au::ZERO, Au::from_px(y), Au::from_px(20.0), Au::from_px(height));
        let style = |font_size: f32| {
            Arc::new(ComputedStyle::from(ComputedValues {
                font_family: vec!["Inter".to_owned()].into(),
                font_size,
                ..ComputedValues::default()
            }))
        };
        let text = Fragment {
            rect: rect(0.0, 20.0),
            kind: FragmentKind::Text(TextRun {
                face,
                size: 16.0,
                style: style(16.0),
                glyphs: Vec::new(),
                text: "x".to_owned(),
                baseline: Au::from_px(16.0),
                marker: false,
                symbol: None,
                orientation: fagus_layout::GlyphOrientation::Horizontal,
                exact: false,
            }),
            children: Vec::new().into(),
        };
        let inline = |style: Arc<ComputedStyle>, y: f32, children: Vec<Fragment>| Fragment {
            rect: rect(y, 20.0),
            kind: FragmentKind::Box {
                style,
                node: None,
                sides: VisibleSides::ALL,
            },
            children: children.into(),
        };
        // A line at 10.625 (rounded to 11) with an inline box at 12.875 in it, which holds its
        // text and an inline box in a larger font: Blink gives it a fragment of its own, whose
        // top goes to a whole pixel (13), and its text with it.
        let page = |holds_larger: bool| {
            let mut children = vec![text.clone()];
            if holds_larger {
                children.push(inline(style(30.0), 0.0, Vec::new()));
            }
            Page {
                width: Au::from_px(200.0),
                height: Au::from_px(100.0),
                fragments: vec![Fragment {
                    rect: Rect::new(Au::ZERO, Au::ZERO, Au::from_px(200.0), Au::from_px(100.0)),
                    kind: FragmentKind::Anonymous,
                    children: vec![Fragment {
                        rect: rect(10.625, 30.0),
                        kind: FragmentKind::Line {
                            baseline: Au::from_px(18.25),
                        },
                        children: vec![inline(style(16.0), 2.25, children)].into(),
                    }]
                    .into(),
                }],
                margin: [Au::ZERO; 4],
                style: Arc::new(ComputedStyle::default()),
                canvas: None,
                bleed: 0.0,
                marks: PageMarks::default(),
                orientation: fagus_layout::PageOrientation::Upright,
                scale: 1.0,
                area: Rect::default(),
                shift: 0.0,
                carried: Vec::new(),
            }
        };
        let baseline = |page: Page| {
            let lists = paint(&[page], &fonts, &fagus_layout::Images::default(), false);
            lists[0].items.iter().find_map(|item| match item {
                Item::Text { y, .. } => Some(*y),
                _ => None,
            })
        };
        // The text's top at 12.875 moves with the line's: by 0.375.
        assert_eq!(baseline(page(false)), Some(29.25));
        // With the box's: by 0.125.
        assert_eq!(baseline(page(true)), Some(29.0));
    }

    #[test]
    fn paints_flex_and_grid_items_one_after_the_other() {
        use fagus_layout::units::Rect;
        use fagus_layout::{PageMarks, TextRun, VisibleSides};
        use fagus_style::values::{Display, DisplayInside, DisplayOutside};
        let mut fonts = FontDb::new();
        fonts.add_default_fonts();
        let face = fonts
            .query(&["Inter"], 400.0, fagus_text::FontStyle::Normal)
            .expect("Inter");
        let rect =
            |x: f32, width: f32| Rect::new(Au::from_px(x), Au::ZERO, Au::from_px(width), Au::from_px(20.0));
        let boxed = |style: ComputedStyle, x: f32, width: f32, children: Vec<Fragment>| Fragment {
            rect: rect(x, width),
            kind: FragmentKind::Box {
                style: Arc::new(style),
                node: None,
                sides: VisibleSides::ALL,
            },
            children: children.into(),
        };
        // The text of the first item reaches into the second, which has a background.
        let line = Fragment {
            rect: rect(0.0, 100.0),
            kind: FragmentKind::Line {
                baseline: Au::from_px(16.0),
            },
            children: vec![Fragment {
                rect: rect(0.0, 100.0),
                kind: FragmentKind::Text(TextRun {
                    face,
                    size: 16.0,
                    style: Arc::new(ComputedStyle::default()),
                    glyphs: Vec::new(),
                    text: "Overflowing".to_owned(),
                    baseline: Au::from_px(16.0),
                    marker: false,
                    symbol: None,
                    orientation: fagus_layout::GlyphOrientation::Horizontal,
                    exact: false,
                }),
                children: Vec::new().into(),
            }]
            .into(),
        };
        let block = ComputedStyle::from(ComputedValues {
            display: Display::BLOCK,
            ..ComputedValues::default()
        });
        let flex = ComputedStyle::from(ComputedValues {
            display: Display::Normal {
                outside: DisplayOutside::Block,
                inside: DisplayInside::Flex,
                list_item: false,
            },
            ..ComputedValues::default()
        });
        let filled = ComputedStyle::from(ComputedValues {
            background_color: Rgba::BLACK,
            ..ComputedValues::from(block.clone())
        });
        let container = boxed(
            flex,
            0.0,
            200.0,
            vec![
                boxed(block, 0.0, 50.0, vec![line]),
                boxed(filled, 50.0, 100.0, Vec::new()),
            ],
        );
        let page = Page {
            width: Au::from_px(200.0),
            height: Au::from_px(100.0),
            fragments: vec![container],
            margin: [Au::ZERO; 4],
            style: Arc::new(ComputedStyle::default()),
            canvas: None,
            bleed: 0.0,
            marks: PageMarks::default(),
            orientation: fagus_layout::PageOrientation::Upright,

            scale: 1.0,

            area: Rect::default(),

            shift: 0.0,

            carried: Vec::new(),
        };
        let lists = paint(&[page], &fonts, &fagus_layout::Images::default(), false);
        let items = &lists[0].items;
        let text = items.iter().position(|item| matches!(item, Item::Text { .. }));
        let background = items.iter().position(|item| matches!(item, Item::Rect { .. }));
        // As in Chrome, the second item's background goes over the first item's text.
        assert!(text.is_some() && text < background, "{items:?}");
    }

    #[test]
    fn paints_flex_items_over_the_backgrounds_of_later_blocks() {
        use fagus_layout::units::Rect;
        use fagus_layout::{PageMarks, VisibleSides};
        use fagus_style::values::{Display, DisplayInside, DisplayOutside};
        let boxed = |style: ComputedStyle, y: f32, children: Vec<Fragment>| Fragment {
            rect: Rect::new(Au::ZERO, Au::from_px(y), Au::from_px(100.0), Au::from_px(40.0)),
            kind: FragmentKind::Box {
                style: Arc::new(style),
                node: None,
                sides: VisibleSides::ALL,
            },
            children: children.into(),
        };
        let filled = |color: Rgba| {
            ComputedStyle::from(ComputedValues {
                display: Display::BLOCK,
                background_color: color,
                ..ComputedValues::default()
            })
        };
        let flex = ComputedStyle::from(ComputedValues {
            display: Display::Normal {
                outside: DisplayOutside::Block,
                inside: DisplayInside::Flex,
                list_item: false,
            },
            ..ComputedValues::default()
        });
        // A flex item, and a block after its container pulled up over it.
        let content = Fragment {
            rect: Rect::new(Au::ZERO, Au::ZERO, Au::from_px(100.0), Au::from_px(100.0)),
            kind: FragmentKind::Anonymous,
            children: vec![
                boxed(flex, 0.0, vec![boxed(filled(Rgba::BLACK), 0.0, Vec::new())]),
                boxed(filled(Rgba::WHITE), 20.0, Vec::new()),
            ]
            .into(),
        };
        let page = Page {
            width: Au::from_px(100.0),
            height: Au::from_px(100.0),
            fragments: vec![content],
            margin: [Au::ZERO; 4],
            style: Arc::new(ComputedStyle::default()),
            canvas: None,
            bleed: 0.0,
            marks: PageMarks::default(),
            orientation: fagus_layout::PageOrientation::Upright,
            scale: 1.0,
            area: Rect::default(),
            shift: 0.0,
            carried: Vec::new(),
        };
        let lists = paint(&[page], &FontDb::new(), &fagus_layout::Images::default(), false);
        let colors: Vec<Rgba> = lists[0]
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Rect { color, .. } => Some(*color),
                _ => None,
            })
            .collect();
        // As in Chrome, the item paints with the inline content, over the block's background.
        assert_eq!(colors, [Rgba::WHITE, Rgba::BLACK]);
    }

    #[test]
    fn leaves_gaps_where_underlines_cross_glyphs() {
        let mut fonts = FontDb::new();
        fonts.add_default_fonts();
        let face = fonts
            .query(&["Inter"], 400.0, fagus_text::FontStyle::Normal)
            .expect("Inter");
        // The underlines of `text` at 16px from x = 0, as their spans.
        let spans = |text: &str, skip: SkipInk| {
            let shaped = fonts.shape(face, text);
            let scale = fonts.face(face).scale(16.0);
            let glyphs = shaped
                .glyphs
                .iter()
                .map(|glyph| Glyph {
                    id: glyph.id,
                    advance: glyph.x_advance * scale,
                    x_offset: 0.0,
                    y_offset: 0.0,
                    cluster: (glyph.cluster, glyph.cluster + 1),
                })
                .collect::<Vec<_>>();
            let width = glyphs.iter().map(|glyph| glyph.advance).sum();
            let run = fagus_layout::TextRun {
                face,
                size: 16.0,
                style: Arc::new(ComputedStyle::from(ComputedValues {
                    decorations: Arc::from([Decoration {
                        line: TextDecorationLine {
                            underline: true,
                            overline: false,
                            line_through: false,
                        },
                        style: TextDecorationStyle::Solid,
                        color: Rgba::BLACK,
                        thickness: DecorationThickness::Auto,
                        underline_offset: None,
                        under: false,
                        font_size: 16.0,
                    }]),
                    font_family: Arc::from(["Inter".to_owned()]),
                    text_decoration_skip_ink: skip,
                    ..ComputedValues::default()
                })),
                glyphs,
                text: text.to_owned(),
                baseline: Au::ZERO,
                marker: false,
                symbol: None,
                orientation: fagus_layout::GlyphOrientation::Horizontal,
                exact: false,
            };
            let mut items = Vec::new();
            paint_decorations(&run, 0.0, 100.0, width, &fonts, true, &mut items);
            items
                .iter()
                .map(|item| match item {
                    Item::Rect { rect, .. } => (rect.x, rect.x + rect.width),
                    _ => panic!("a rectangle"),
                })
                .collect::<Vec<_>>()
        };
        // Without descenders, one line; with them, the line breaks around them, unless
        // `text-decoration-skip-ink: none`.
        assert_eq!(spans("xxx", SkipInk::Auto).len(), 1);
        assert_eq!(spans("gyp", SkipInk::None).len(), 1);
        let gyp = spans("gyp", SkipInk::Auto);
        assert!(gyp.len() >= 2, "{gyp:?}");
        assert!(gyp.iter().all(|(start, end)| end > start));
    }

    #[test]
    fn cjk_characters_keep_the_ink_of_underlines() {
        for character in ['中', 'あ', 'カ', '한', '，', '_', '/', '𠀀'] {
            assert!(!skips_ink(character), "{character}");
        }
        for character in ['g', 'y', 'Ä', 'ф', 'ي'] {
            assert!(skips_ink(character), "{character}");
        }
    }

    #[test]
    fn places_decorations_like_chrome() {
        let mut fonts = FontDb::new();
        fonts.add_default_fonts();
        let face = fonts
            .query(&["Inter"], 400.0, fagus_text::FontStyle::Normal)
            .expect("Inter");
        // The tops of the lines below the baseline and their heights, for text at `size`.
        let lines = |size: f32, decoration: Decoration, below: bool| {
            let run = fagus_layout::TextRun {
                face,
                size,
                style: Arc::new(ComputedStyle::from(ComputedValues {
                    decorations: Arc::from([Decoration {
                        font_size: size,
                        ..decoration
                    }]),
                    font_size: size,
                    ..ComputedValues::default()
                })),
                glyphs: Vec::new(),
                text: String::new(),
                baseline: Au::ZERO,
                marker: false,
                symbol: None,
                orientation: fagus_layout::GlyphOrientation::Horizontal,
                exact: false,
            };
            let mut items = Vec::new();
            paint_decorations(&run, 0.0, 100.0, 50.0, &fonts, below, &mut items);
            items
                .iter()
                .map(|item| match item {
                    Item::Rect { rect, .. } => (rect.y - 100.0, rect.height),
                    _ => panic!("a rectangle"),
                })
                .collect::<Vec<_>>()
        };
        let line = |underline: bool, overline: bool, line_through: bool| TextDecorationLine {
            underline,
            overline,
            line_through,
        };
        let underline = Decoration {
            line: line(true, false, false),
            style: TextDecorationStyle::Solid,
            color: Rgba::BLACK,
            thickness: DecorationThickness::Auto,
            underline_offset: None,
            under: false,
            font_size: 16.0,
        };
        // As measured in Chrome with Inter.
        for (size, expected) in [
            (16.0, (1.0, 1.0)),
            (20.0, (1.0, 2.0)),
            (32.0, (2.0, 3.0)),
            (48.0, (3.0, 4.0)),
        ] {
            assert_eq!(lines(size, underline, true), [expected], "{size}px");
        }
        let with = |thickness, offset| Decoration {
            thickness,
            underline_offset: offset,
            ..underline
        };
        assert_eq!(
            lines(10.0, with(DecorationThickness::Px(3.0), None), true),
            [(2.0, 3.0)]
        );
        assert_eq!(
            lines(16.0, with(DecorationThickness::Px(1.5), None), true),
            [(1.0, 2.0)]
        );
        assert_eq!(
            lines(32.0, with(DecorationThickness::FromFont, None), true),
            [(2.0, 2.0)]
        );
        assert_eq!(
            lines(32.0, with(DecorationThickness::Auto, Some(4.0)), true),
            [(4.0, 3.0)]
        );
        let under = Decoration {
            under: true,
            ..underline
        };
        assert_eq!(lines(32.0, under, true), [(7.0, 3.0)]);
        let double = Decoration {
            style: TextDecorationStyle::Double,
            ..underline
        };
        assert_eq!(lines(16.0, double, true), [(1.0, 1.0), (4.0, 1.0)]);
        // Overlines lie below the text, lines through above it.
        let overline = Decoration {
            line: line(false, true, false),
            ..underline
        };
        assert_eq!(lines(16.0, overline, true), [(-17.0, 1.0)]);
        assert_eq!(lines(16.0, overline, false), []);
        let through = Decoration {
            line: line(false, false, true),
            ..underline
        };
        assert_eq!(lines(10.0, through, false), [(-4.0, 1.0)]);
        assert_eq!(lines(32.0, through, false), [(-12.0, 3.0)]);

        // Dots and dashes are stroked as thick as the line, along the middle of its rows from
        // whole pixel to whole pixel, sized for its rounded thickness; waves as Blink makes them.
        assert_eq!(wave(1.6), (9.0, 5.5, (-2.5, 2.5)));
        assert_eq!(wave(3.2), (15.0, 10.5, (-5.5, 5.5)));
        let dashed = Decoration {
            style: TextDecorationStyle::Dashed,
            ..underline
        };
        let mut items = Vec::new();
        decoration_line(&dashed, Line::Under, 10.4, 26.2, 204.1, 1.0, 1.6, &mut items);
        match items.as_slice() {
            [
                Item::DashedLine {
                    from,
                    to,
                    thickness,
                    dash,
                    dotted: false,
                    ..
                },
            ] => assert_eq!(
                (*from, *to, *thickness, *dash),
                ((10.0, 27.0), (214.0, 27.0), 1.6, 2.0)
            ),
            _ => panic!("one dashed line: {items:?}"),
        }

        // Far from the origin, where adding a dot to a float leaves it where it was, the dots
        // are one stroke.
        let dotted = Decoration {
            style: TextDecorationStyle::Dotted,
            ..underline
        };
        let run = fagus_layout::TextRun {
            face,
            size: 16.0,
            style: Arc::new(ComputedStyle::from(ComputedValues {
                decorations: Arc::from([dotted]),
                ..ComputedValues::default()
            })),
            glyphs: Vec::new(),
            text: String::new(),
            baseline: Au::ZERO,
            marker: false,
            symbol: None,
            orientation: fagus_layout::GlyphOrientation::Horizontal,
            exact: false,
        };
        let mut items = Vec::new();
        paint_decorations(&run, -1e9, 100.0, 50.0, &fonts, true, &mut items);
        assert!(matches!(
            items.as_slice(),
            [Item::DashedLine { dotted: true, .. }]
        ));
    }

    #[test]
    fn keeps_very_light_colors_and_lightens_very_dark_ones_in_3d_borders() {
        // Blink's `CalculateInsetOutsetColor`: a color lighter than `#ebebeb` stays as it is on
        // the light side (a fieldset's `groove` in `ThreeDFace`, `#efefef`, is `#9b9b9b` and
        // `#efefef` in Chrome); one darker than `#202020` is lightened once and twice.
        let bytes = |color: Rgba| [color.r, color.g, color.b].map(|channel| (channel * 255.0).round() as u8);
        let (dark, light) = shades(Rgba::from_u8(0xef, 0xef, 0xef, 1.0));
        assert_eq!((bytes(dark), bytes(light)), ([155; 3], [239; 3]));
        let (dark, light) = shades(Rgba::from_u8(0x10, 0x10, 0x10, 1.0));
        assert_eq!((bytes(dark), bytes(light)), ([100; 3], [184; 3]));
    }

    #[test]
    fn draws_3d_borders_in_currentcolor_from_light_gray() {
        // Blink's `ComputedStyleUtils::BorderSideColor`: `inset`, `outset`, `groove` and `ridge`
        // in `currentcolor` are `#eee`, whatever the text's color (measured: `#9a9a9a` and
        // `#eeeeee` for `border: 2px inset` in red text); a color of their own is theirs.
        let style = ComputedStyle::from(ComputedValues {
            color: Rgba::from_u8(255, 0, 0, 1.0),
            ..ComputedValues::default()
        });
        let gray = Rgba::from_u8(238, 238, 238, 1.0);
        for border_style in [
            BorderStyle::Inset,
            BorderStyle::Outset,
            BorderStyle::Groove,
            BorderStyle::Ridge,
        ] {
            assert_eq!(side_color(&style, None, border_style), gray);
            assert_eq!(side_color(&style, Some(Rgba::BLACK), border_style), Rgba::BLACK);
        }
        assert_eq!(side_color(&style, None, BorderStyle::Solid), style.color);
        // Tables and their parts keep the text's color (`<table border>`).
        let table = ComputedStyle::from(ComputedValues {
            display: fagus_style::values::Display::Normal {
                outside: fagus_style::values::DisplayOutside::Block,
                inside: fagus_style::values::DisplayInside::Table,
                list_item: false,
            },
            ..ComputedValues::from(style.clone())
        });
        assert_eq!(side_color(&table, None, BorderStyle::Outset), style.color);
        let bytes = |color: Rgba| [color.r, color.g, color.b].map(|channel| (channel * 255.0).round() as u8);
        let (dark, light) = shades(gray);
        assert_eq!((bytes(dark), bytes(light)), ([154; 3], [238; 3]));
    }

    #[test]
    fn shades_and_splits_borders_like_chrome() {
        let bytes = |color: Rgba| [color.r, color.g, color.b].map(|channel| (channel * 255.0).round() as u8);
        // The shades Chrome draws (measured).
        for (color, dark, light) in [
            ([0, 0, 0], [84, 84, 84], [168, 168, 168]),
            ([255, 255, 255], [171, 171, 171], [255, 255, 255]),
            ([128, 128, 128], [44, 44, 44], [212, 212, 212]),
            ([0x33, 0x66, 0xcc], [30, 60, 120], [63, 127, 255]),
        ] {
            let (shade_dark, shade_light) = shades(Rgba::from_u8(color[0], color[1], color[2], 1.0));
            assert_eq!(
                (bytes(shade_dark), bytes(shade_light)),
                (dark, light),
                "{color:?}"
            );
        }
        // The top side of a box 100 wide with borders of `width`: its bands, as the distances
        // of their outer and inner edges from the top.
        let bands = |style: BorderStyle, width: f32| {
            let points = [(0.0, 0.0), (100.0, 0.0), (100.0 - width, width), (width, width)];
            let mut items = Vec::new();
            border_side(points, 0, Rgba::BLACK, style, width, &mut items);
            items
                .iter()
                .map(|item| match item {
                    Item::BorderSide { points, color, .. } => (points[0].1, points[3].1, bytes(*color)[0]),
                    _ => panic!("a border side"),
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(bands(BorderStyle::Double, 12.0), [(0.0, 4.0, 0), (8.0, 12.0, 0)]);
        assert_eq!(bands(BorderStyle::Double, 7.0), [(0.0, 2.0, 0), (5.0, 7.0, 0)]);
        assert_eq!(bands(BorderStyle::Double, 2.0), [(0.0, 2.0, 0)]);
        assert_eq!(bands(BorderStyle::Inset, 2.0), [(0.0, 2.0, 84)]);
        // Grooves and ridges thinner than two pixels are solid, in the border's color (Blink's
        // `BorderEdge::EffectiveStyle`).
        assert_eq!(bands(BorderStyle::Groove, 1.0), [(0.0, 1.0, 0)]);
        assert_eq!(bands(BorderStyle::Ridge, 1.5), [(0.0, 1.5, 0)]);
        assert_eq!(bands(BorderStyle::Outset, 2.0), [(0.0, 2.0, 168)]);
        assert_eq!(
            bands(BorderStyle::Groove, 12.0),
            [(0.0, 6.0, 84), (6.0, 12.0, 168)]
        );
        assert_eq!(
            bands(BorderStyle::Ridge, 12.0),
            [(0.0, 6.0, 168), (6.0, 12.0, 84)]
        );
    }

    #[test]
    fn places_images_by_object_fit() {
        let image = fagus_layout::Image {
            data: Arc::new(Vec::new()),
            format: ImageFormat::Png,
            width: 120.0,
            height: 40.0,
            natural: (true, true),
            ratio: Some(3.0),
            inline: false,
            orientation: 1,
            filters: Arc::from([]),
            pad: 0,
        };
        let content = RectF {
            x: 10.0,
            y: 10.0,
            width: 60.0,
            height: 60.0,
        };
        let place = |fit: ObjectFit, position: (f32, f32)| {
            let style = ComputedStyle::from(ComputedValues {
                object_fit: fit,
                object_position: (
                    LengthPercentage::Percent(position.0),
                    LengthPercentage::Percent(position.1),
                ),
                ..ComputedValues::default()
            });
            let (rect, overflows) = object_rect(&style, &image, content);
            ((rect.x, rect.y, rect.width, rect.height), overflows)
        };
        let center = (0.5, 0.5);
        assert_eq!(place(ObjectFit::Fill, center), ((10.0, 10.0, 60.0, 60.0), false));
        assert_eq!(
            place(ObjectFit::Contain, (0.0, 1.0)),
            ((10.0, 50.0, 60.0, 20.0), false)
        );
        // Covered and clipped, centered or at the top left.
        assert_eq!(
            place(ObjectFit::Cover, center),
            ((-50.0, 10.0, 180.0, 60.0), true)
        );
        assert_eq!(
            place(ObjectFit::Cover, (0.0, 0.0)),
            ((10.0, 10.0, 180.0, 60.0), true)
        );
        assert_eq!(place(ObjectFit::None, center), ((-20.0, 20.0, 120.0, 40.0), true));
        // Scaled down to fit, not up.
        assert_eq!(
            place(ObjectFit::ScaleDown, center),
            place(ObjectFit::Contain, center)
        );
    }

    #[test]
    fn sizes_svg_images_without_a_natural_size_for_object_fit() {
        // An SVG with a `viewBox` of 16 × 8 and no `width` or `height` (WPT
        // `object-fit-none-svg-003i`): its size is the largest of its ratio in the box.
        let image = |natural: (bool, bool), ratio: Option<f32>| fagus_layout::Image {
            data: Arc::new(Vec::new()),
            format: ImageFormat::Svg,
            width: 16.0,
            height: 8.0,
            natural,
            ratio,
            inline: false,
            orientation: 1,
            filters: Arc::from([]),
            pad: 0,
        };
        let content = RectF {
            x: 0.0,
            y: 0.0,
            width: 48.0,
            height: 32.0,
        };
        let place = |fit: ObjectFit, image: &fagus_layout::Image| {
            let style = ComputedStyle::from(ComputedValues {
                object_fit: fit,
                object_position: (LengthPercentage::Percent(0.0), LengthPercentage::Percent(0.0)),
                ..ComputedValues::default()
            });
            let (rect, _) = object_rect(&style, image, content);
            (rect.width, rect.height)
        };
        let ratio_only = image((false, false), Some(2.0));
        assert_eq!(place(ObjectFit::None, &ratio_only), (48.0, 24.0));
        assert_eq!(place(ObjectFit::ScaleDown, &ratio_only), (48.0, 24.0));
        // A natural width with the ratio, and neither: the box.
        assert_eq!(
            place(ObjectFit::None, &image((true, false), Some(2.0))),
            (16.0, 8.0)
        );
        assert_eq!(place(ObjectFit::None, &image((false, false), None)), (48.0, 32.0));
        // An image with a natural size keeps it.
        assert_eq!(
            place(ObjectFit::None, &image((true, true), Some(2.0))),
            (16.0, 8.0)
        );
    }

    #[test]
    fn moves_inline_svg_by_the_snapping_of_its_border_box() {
        use fagus_layout::units::Rect;
        let mut fonts = FontDb::new();
        fonts.add_default_fonts();
        // An `<svg>` box at (20.4, 0.3) with a padding of 5.6px: Chrome snaps the box to
        // (20, 0) and draws the content 5.6px inside, at (25.6, 5.6), not at whole pixels.
        let image = Arc::new(fagus_layout::Image {
            data: Arc::new(b"<svg xmlns='http://www.w3.org/2000/svg'/>".to_vec()),
            format: ImageFormat::Svg,
            width: 50.0,
            height: 50.0,
            natural: (true, true),
            ratio: Some(1.0),
            inline: true,
            orientation: 1,
            filters: Arc::from([]),
            pad: 0,
        });
        let content = Fragment {
            rect: Rect::new(
                Au::from_px(5.6),
                Au::from_px(5.6),
                Au::from_px(50.0),
                Au::from_px(50.0),
            ),
            kind: FragmentKind::Image(image),
            children: Vec::new().into(),
        };
        let mut svg = test_box(ComputedStyle::default(), 20.4, 61.2, vec![content]);
        svg.rect.y = Au::from_px(0.3);
        let page = test_page(200.0, vec![svg]);
        let lists = paint(&[page], &fonts, &fagus_layout::Images::default(), false);
        let rect = lists[0]
            .items
            .iter()
            .find_map(|item| match item {
                Item::Image { rect, .. } => Some(*rect),
                _ => None,
            })
            .expect("image");
        assert!(
            (rect.x - 25.6).abs() < 0.02 && (rect.y - 5.6).abs() < 0.02,
            "{rect:?}"
        );
    }

    #[test]
    fn stretches_svg_images_to_their_snapped_box() {
        // A box 100.4 × 33.47 at (20.3, 20.3) snaps to 101 × 34 at (20, 20): an SVG fitted into
        // the box is stretched to that, as in Chrome.
        let rect = RectF {
            x: 20.3,
            y: 20.3,
            width: 100.4,
            height: 33.47,
        };
        let [sx, b, c, sy, tx, ty] = stretch_to_snapped(rect).expect("a transform");
        assert_eq!((b, c), (0.0, 0.0));
        let map = |x: f32, y: f32| (sx * x + tx, sy * y + ty);
        let corners = [
            map(rect.x, rect.y),
            map(rect.x + rect.width, rect.y + rect.height),
        ];
        for ((x, y), (want_x, want_y)) in corners.into_iter().zip([(20.0, 20.0), (121.0, 54.0)]) {
            assert!((x - want_x).abs() < 1e-3 && (y - want_y).abs() < 1e-3, "{x} {y}");
        }
        // A box on whole pixels stays as it is.
        let whole = RectF {
            x: 20.0,
            y: 20.0,
            width: 100.0,
            height: 34.0,
        };
        assert_eq!(stretch_to_snapped(whole), None);
    }

    /// A page of `width` × 100 with `fragments`, for the tests below.
    fn test_page(width: f32, fragments: Vec<Fragment>) -> Page {
        use fagus_layout::units::Rect;
        Page {
            width: Au::from_px(width),
            height: Au::from_px(100.0),
            fragments,
            margin: [Au::ZERO; 4],
            style: Arc::new(ComputedStyle::default()),
            canvas: None,
            bleed: 0.0,
            marks: fagus_layout::PageMarks::default(),
            orientation: fagus_layout::PageOrientation::Upright,
            scale: 1.0,
            area: Rect::default(),
            shift: 0.0,
            carried: Vec::new(),
        }
    }

    #[test]
    fn clips_inline_boxes_broken_across_lines_in_their_first_fragment() {
        use fagus_layout::VisibleSides;
        use fagus_layout::units::Rect;
        use fagus_style::values::{BoxArea, ClipPath, ClipShape, Display};
        let style = Arc::new(ComputedStyle::from(ComputedValues {
            display: Display::INLINE,
            clip_path: Some(Arc::new(ClipPath {
                shape: ClipShape::Box,
                reference: BoxArea::BorderBox,
                svg_reference: fagus_style::values::TransformBox::StrokeBox,
            })),
            ..ComputedValues::default()
        }));
        let node = NodeId::DOCUMENT;
        let span = |x: f32, width: f32| Fragment {
            rect: Rect::new(Au::from_px(x), Au::ZERO, Au::from_px(width), Au::from_px(20.0)),
            kind: FragmentKind::Box {
                style: Arc::clone(&style),
                node: Some(node),
                sides: VisibleSides::ALL,
            },
            children: Vec::new().into(),
        };
        let line = |y: f32, child: Fragment| Fragment {
            rect: Rect::new(Au::ZERO, Au::from_px(y), Au::from_px(300.0), Au::from_px(20.0)),
            kind: FragmentKind::Line { baseline: Au::ZERO },
            children: vec![child].into(),
        };
        let block = test_box(
            ComputedStyle::default(),
            10.0,
            300.0,
            vec![line(0.0, span(200.0, 100.0)), line(20.0, span(0.0, 250.0))],
        );
        // The span on the first line at (210, 0), on the second at (10, 20): the first clips.
        let boxes = inline_clip_boxes(std::iter::once(&block));
        assert_eq!(
            boxes.get(&node),
            Some(&RectF {
                x: 210.0,
                y: 0.0,
                width: 100.0,
                height: 20.0
            })
        );
        // One that is not broken clips in its own box.
        let whole = test_box(
            ComputedStyle::default(),
            0.0,
            300.0,
            vec![line(0.0, span(0.0, 50.0))],
        );
        assert!(inline_clip_boxes(std::iter::once(&whole)).is_empty());
    }

    /// A box of `style` at `(x, 0)` of `width` × 20.
    fn test_box(style: ComputedStyle, x: f32, width: f32, children: Vec<Fragment>) -> Fragment {
        use fagus_layout::VisibleSides;
        use fagus_layout::units::Rect;
        Fragment {
            rect: Rect::new(Au::from_px(x), Au::ZERO, Au::from_px(width), Au::from_px(20.0)),
            kind: FragmentKind::Box {
                style: Arc::new(style),
                node: None,
                sides: VisibleSides::ALL,
            },
            children: children.into(),
        }
    }

    #[test]
    fn clips_backgrounds_to_the_glyphs_of_the_text() {
        use fagus_layout::TextRun;
        use fagus_layout::units::Rect;
        use fagus_style::values::Display;
        let mut fonts = FontDb::new();
        fonts.add_default_fonts();
        let face = fonts
            .query(&["Inter"], 400.0, fagus_text::FontStyle::Normal)
            .expect("Inter");
        // A title in transparent text whose gradient shows through its glyphs.
        let text_style = ComputedStyle::from(ComputedValues {
            color: Rgba::TRANSPARENT,
            ..ComputedValues::default()
        });
        let line = Fragment {
            rect: Rect::new(Au::ZERO, Au::ZERO, Au::from_px(100.0), Au::from_px(20.0)),
            kind: FragmentKind::Line {
                baseline: Au::from_px(16.0),
            },
            children: vec![Fragment {
                rect: Rect::new(Au::ZERO, Au::ZERO, Au::from_px(100.0), Au::from_px(20.0)),
                kind: FragmentKind::Text(TextRun {
                    face,
                    size: 16.0,
                    style: Arc::new(text_style.clone()),
                    glyphs: Vec::new(),
                    text: "Certificate".to_owned(),
                    baseline: Au::from_px(16.0),
                    marker: false,
                    symbol: None,
                    orientation: fagus_layout::GlyphOrientation::Horizontal,
                    exact: false,
                }),
                children: Vec::new().into(),
            }]
            .into(),
        };
        let title = ComputedStyle::from(ComputedValues {
            display: Display::BLOCK,
            background_color: Rgba::from_u8(0xb0, 0x8d, 0x57, 1.0),
            background_clip_text: Arc::from([true]),
            ..ComputedValues::from(text_style)
        });
        let page = test_page(200.0, vec![test_box(title, 0.0, 200.0, vec![line])]);
        let lists = paint(&[page], &fonts, &fagus_layout::Images::default(), false);
        let items = &lists[0].items;
        let mask = items
            .iter()
            .position(|item| matches!(item, Item::PushMask(mask) if !mask.luminance));
        let background = mask.and_then(|mask| {
            items[mask..]
                .iter()
                .position(|item| matches!(item, Item::Rect { .. }))
                .map(|offset| mask + offset)
        });
        assert!(mask.is_some() && mask < background, "{items:?}");
        let Some(Item::PushMask(mask)) = mask.map(|index| &items[index]) else {
            unreachable!();
        };
        // The mask is the text, opaque, whatever its color.
        assert!(!mask.luminance);
        assert!(
            matches!(&mask.items[..], [Item::Text { color, text, .. }] if *color == Rgba::BLACK && text == "Certificate"),
            "{:?}",
            mask.items
        );
        assert!(
            matches!(items.get(background.unwrap_or(0) + 1), Some(Item::Pop)),
            "{items:?}"
        );
    }

    #[test]
    fn puts_the_text_of_relatively_positioned_descendants_in_the_text_mask() {
        use fagus_layout::TextRun;
        use fagus_layout::units::Rect;
        use fagus_style::values::{Display, Position};
        let mut fonts = FontDb::new();
        fonts.add_default_fonts();
        let face = fonts
            .query(&["Inter"], 400.0, fagus_text::FontStyle::Normal)
            .expect("Inter");
        let text_style = ComputedStyle::from(ComputedValues {
            color: Rgba::TRANSPARENT,
            ..ComputedValues::default()
        });
        let line = |text: &str| Fragment {
            rect: Rect::new(Au::ZERO, Au::ZERO, Au::from_px(100.0), Au::from_px(20.0)),
            kind: FragmentKind::Line {
                baseline: Au::from_px(16.0),
            },
            children: vec![Fragment {
                rect: Rect::new(Au::ZERO, Au::ZERO, Au::from_px(100.0), Au::from_px(20.0)),
                kind: FragmentKind::Text(TextRun {
                    face,
                    size: 16.0,
                    style: Arc::new(text_style.clone()),
                    glyphs: Vec::new(),
                    text: text.to_owned(),
                    baseline: Au::from_px(16.0),
                    marker: false,
                    symbol: None,
                    orientation: fagus_layout::GlyphOrientation::Horizontal,
                    exact: false,
                }),
                children: Vec::new().into(),
            }]
            .into(),
        };
        let block = |position: Position, opacity: f32| {
            ComputedStyle::from(ComputedValues {
                display: Display::BLOCK,
                position,
                opacity,
                ..ComputedValues::from(text_style.clone())
            })
        };
        let title = ComputedStyle::from(ComputedValues {
            display: Display::BLOCK,
            background_color: Rgba::from_u8(0xb0, 0x8d, 0x57, 1.0),
            background_clip_text: Arc::from([true]),
            ..ComputedValues::from(text_style.clone())
        });
        let children = vec![
            test_box(block(Position::Relative, 1.0), 0.0, 50.0, vec![line("relative")]),
            test_box(
                block(Position::Static, 0.5),
                50.0,
                50.0,
                vec![line("translucent")],
            ),
            test_box(
                block(Position::Absolute, 1.0),
                100.0,
                50.0,
                vec![line("absolute")],
            ),
        ];
        let page = test_page(200.0, vec![test_box(title, 0.0, 200.0, children)]);
        let lists = paint(&[page], &fonts, &fagus_layout::Images::default(), false);
        let Some(Item::PushMask(mask)) = lists[0]
            .items
            .iter()
            .find(|item| matches!(item, Item::PushMask(mask) if !mask.luminance))
        else {
            panic!("no mask: {:?}", lists[0].items);
        };
        // The text of the relatively positioned and of the translucent block goes into the mask
        // (opaque), that of the absolutely positioned one not (Chrome 157).
        let texts: Vec<&str> = mask
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, ["relative", "translucent"], "{:?}", mask.items);
        assert!(!mask.items.iter().any(|item| matches!(item, Item::PushOpacity(_))));
    }

    #[test]
    fn clips_only_the_background_layers_of_text_to_the_text() {
        let mask = Arc::new(MaskPaint {
            items: Vec::new(),
            luminance: false,
        });
        let paint = |clips: [bool; 2]| {
            let style = ComputedStyle::from(ComputedValues {
                background_color: Rgba::from_u8(0xb0, 0x8d, 0x57, 1.0),
                background_image: Arc::from([
                    fagus_style::values::Image::None,
                    fagus_style::values::Image::None,
                ]),
                background_clip_text: Arc::from(clips),
                ..ComputedValues::default()
            });
            let rect = RectF {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 20.0,
            };
            let mut items = Vec::new();
            background::paint_clipped(
                &style,
                background::Placement::of(rect, None),
                &fagus_layout::Images::default(),
                background::ClipMasks {
                    text: Some(&mask),
                    border_area: None,
                },
                &mut items,
            );
            items
        };
        // The color is clipped as the bottom layer: to the text where that layer is `text`.
        // Before it, as Chrome, the background once more through the edges of its box rounded
        // out to device pixels.
        let clipped = paint([false, true]);
        assert!(
            matches!(
                &clipped[..],
                [
                    Item::PushMask(edges),
                    Item::PushIsolated,
                    Item::Rect { .. },
                    Item::Pop,
                    Item::Pop,
                    Item::PushMask(text),
                    Item::Rect { .. },
                    Item::Pop
                ] if edges.luminance && !text.luminance
            ),
            "{clipped:?}"
        );
        let Item::PushMask(edges) = &clipped[0] else {
            unreachable!();
        };
        assert!(matches!(
            &edges.items[..],
            [Item::Rect { color: white, .. }, Item::Rect { color: black, rect, .. }]
                if *white == Rgba::WHITE && *black == Rgba::BLACK && rect.x <= 0.0 && rect.width >= 100.0
        ));
        assert!(matches!(&paint([true, false])[..], [Item::Rect { .. }]));
    }

    #[test]
    fn clips_backgrounds_to_where_the_borders_paint_whatever_their_color() {
        use fagus_layout::VisibleSides;
        use fagus_style::values::{BorderStyle, Sides};
        let rect = RectF {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 60.0,
        };
        let style = |border: BorderStyle| {
            ComputedStyle::from(ComputedValues {
                background_color: Rgba::from_u8(0, 0, 255, 1.0),
                border_width: Sides::all(10.0),
                border_style: Sides::all(border),
                border_color: Sides::all(Some(Rgba::TRANSPARENT)),
                background_clip_border_area: Arc::from([true]),
                ..ComputedValues::default()
            })
        };
        // The mask: the transparent border, painted opaque; dashes for a dashed one.
        let solid = background::border_area(&style(BorderStyle::Solid), rect, None, VisibleSides::ALL);
        assert!(matches!(
            &solid.items[..],
            [Item::RoundedBorder { color, .. }] if *color == Rgba::BLACK
        ));
        let dashed = background::border_area(&style(BorderStyle::Dashed), rect, None, VisibleSides::ALL);
        assert!(dashed.items.iter().any(|item| matches!(
            item,
            Item::BorderSide {
                style: BorderStyle::Dashed,
                ..
            }
        )));
        // The color, as the bottom layer, shows through it; the layer reaches over the border box.
        let mut items = Vec::new();
        background::paint_clipped(
            &style(BorderStyle::Solid),
            background::Placement {
                opaque_border: true,
                ..background::Placement::of(rect, None)
            },
            &fagus_layout::Images::default(),
            background::ClipMasks {
                text: None,
                border_area: Some(&solid),
            },
            &mut items,
        );
        assert!(matches!(
            &items[..],
            [Item::PushMask(_), Item::Rect { rect: drawn, .. }, Item::Pop] if *drawn == rect
        ));
    }

    #[test]
    fn puts_decoration_lines_into_text_masks() {
        use fagus_layout::TextRun;
        use fagus_layout::units::Rect;
        use fagus_style::values::{
            Decoration, DecorationThickness, Display, TextDecorationLine, TextDecorationStyle,
        };
        let mut fonts = FontDb::new();
        fonts.add_default_fonts();
        let face = fonts
            .query(&["Inter"], 400.0, fagus_text::FontStyle::Normal)
            .expect("Inter");
        // Gradient text, underlined: transparent glyphs and line, through which the
        // background shows.
        let text_style = ComputedStyle::from(ComputedValues {
            text_fill_color: Some(Rgba::TRANSPARENT),
            decorations: Arc::from([Decoration {
                line: TextDecorationLine {
                    underline: true,
                    overline: false,
                    line_through: false,
                },
                style: TextDecorationStyle::Solid,
                color: Rgba::TRANSPARENT,
                thickness: DecorationThickness::Auto,
                underline_offset: None,
                under: false,
                font_size: 16.0,
            }]),
            ..ComputedValues::default()
        });
        let line = Fragment {
            rect: Rect::new(Au::ZERO, Au::ZERO, Au::from_px(100.0), Au::from_px(20.0)),
            kind: FragmentKind::Line {
                baseline: Au::from_px(16.0),
            },
            children: vec![Fragment {
                rect: Rect::new(Au::ZERO, Au::ZERO, Au::from_px(100.0), Au::from_px(20.0)),
                kind: FragmentKind::Text(TextRun {
                    face,
                    size: 16.0,
                    style: Arc::new(text_style.clone()),
                    glyphs: Vec::new(),
                    text: "Gradient".to_owned(),
                    baseline: Au::from_px(16.0),
                    marker: false,
                    symbol: None,
                    orientation: fagus_layout::GlyphOrientation::Horizontal,
                    exact: false,
                }),
                children: Vec::new().into(),
            }]
            .into(),
        };
        let title = ComputedStyle::from(ComputedValues {
            display: Display::BLOCK,
            background_color: Rgba::from_u8(0xb0, 0x8d, 0x57, 1.0),
            background_clip_text: Arc::from([true]),
            ..ComputedValues::from(text_style)
        });
        let page = test_page(200.0, vec![test_box(title, 0.0, 200.0, vec![line])]);
        let lists = paint(&[page], &fonts, &fagus_layout::Images::default(), false);
        let items = &lists[0].items;
        let Some(Item::PushMask(mask)) = items
            .iter()
            .find(|item| matches!(item, Item::PushMask(mask) if !mask.luminance))
        else {
            panic!("a text mask: {items:?}");
        };
        assert!(
            matches!(
                &mask.items[..],
                [Item::Rect { color: line, .. }, Item::Text { color, .. }]
                    if *line == Rgba::BLACK && *color == Rgba::BLACK
            ),
            "{:?}",
            mask.items
        );
        // The glyphs themselves are drawn in their fill color.
        assert!(
            items
                .iter()
                .any(|item| matches!(item, Item::Text { color, .. } if *color == Rgba::TRANSPARENT))
        );
    }

    #[test]
    fn paints_the_backgrounds_of_collapsed_cells_halfway_under_their_borders() {
        use fagus_style::values::{BorderCollapse, Display, InternalDisplay, Sides, TransformFunction};
        let mut fonts = FontDb::new();
        fonts.add_default_fonts();
        // A header cell turned upside down, as in timetables, with collapsed borders of 2px
        // above and below and 6px at the sides, which the table draws.
        let cell = |collapse: BorderCollapse| {
            ComputedStyle::from(ComputedValues {
                display: Display::Internal(InternalDisplay::TableCell),
                border_collapse: collapse,
                border_width: Sides {
                    top: 2.0,
                    right: 6.0,
                    bottom: 2.0,
                    left: 6.0,
                },
                background_color: Rgba::from_u8(0xee, 0xee, 0xff, 1.0),
                transform: Arc::from([TransformFunction::Rotate(180.0)]),
                ..ComputedValues::default()
            })
        };
        let background = |style: ComputedStyle| {
            let page = test_page(200.0, vec![test_box(style, 10.0, 100.0, Vec::new())]);
            let lists = paint(&[page], &fonts, &fagus_layout::Images::default(), false);
            lists[0]
                .items
                .iter()
                .find_map(|item| match item {
                    Item::Rect { rect, .. } => Some(*rect),
                    _ => None,
                })
                .expect("background")
        };
        let half = background(cell(BorderCollapse::Collapse));
        assert_eq!((half.x, half.y, half.width, half.height), (13.0, 1.0, 94.0, 18.0));
        // Separate borders are the cell's own: its background fills its border box.
        let whole = background(cell(BorderCollapse::Separate));
        assert_eq!(
            (whole.x, whole.y, whole.width, whole.height),
            (10.0, 0.0, 100.0, 20.0)
        );
    }

    #[test]
    fn snaps_the_backgrounds_of_collapsed_cells_with_odd_borders_to_whole_pixels() {
        use fagus_style::values::{BorderCollapse, Display, InternalDisplay, Sides};
        let mut fonts = FontDb::new();
        fonts.add_default_fonts();
        // A header cell above a collapsed border of 1px: its box ends half a pixel into the
        // border, which Chrome rounds down to the border's bottom.
        let style = ComputedStyle::from(ComputedValues {
            display: Display::Internal(InternalDisplay::TableCell),
            border_collapse: BorderCollapse::Collapse,
            border_width: Sides {
                top: 0.0,
                right: 0.0,
                bottom: 1.0,
                left: 0.0,
            },
            background_color: Rgba::from_u8(0x22, 0x33, 0x33, 1.0),
            ..ComputedValues::default()
        });
        let page = test_page(200.0, vec![test_box(style, 10.0, 100.0, Vec::new())]);
        let lists = paint(&[page], &fonts, &fagus_layout::Images::default(), false);
        let rect = lists[0]
            .items
            .iter()
            .find_map(|item| match item {
                Item::Rect { rect, .. } => Some(*rect),
                _ => None,
            })
            .expect("background");
        assert_eq!(
            (rect.x, rect.y, rect.width, rect.height),
            (10.0, 0.0, 100.0, 20.0)
        );
    }
    #[test]
    fn draws_the_outlines_of_collapsed_cells_around_half_their_borders() {
        use fagus_style::values::{
            BorderCollapse, BorderStyle, Display, InternalDisplay, OutlineStyle, Sides,
        };
        let mut fonts = FontDb::new();
        fonts.add_default_fonts();
        // A cell between collapsed borders of 2px, whose fragment spans them whole: Chrome's
        // border box ends in the middle of each, and the outline 5px inside it.
        let style = ComputedStyle::from(ComputedValues {
            display: Display::Internal(InternalDisplay::TableCell),
            border_collapse: BorderCollapse::Collapse,
            border_width: Sides::all(2.0),
            outline_width: 1.0,
            outline_style: OutlineStyle::Line(BorderStyle::Solid),
            outline_offset: -5.0,
            ..ComputedValues::default()
        });
        let page = test_page(200.0, vec![test_box(style, 10.0, 100.0, Vec::new())]);
        let lists = paint(&[page], &fonts, &fagus_layout::Images::default(), false);
        let outer = lists[0].items.iter().find_map(|item| match item {
            Item::RoundedBorder { outer, .. } => Some(*outer),
            _ => None,
        });
        // The box from (11, 1) to (109, 19); the outline 1px wide ends 5px inside it.
        assert_eq!(
            outer.map(|rect| (rect.x, rect.y, rect.width, rect.height)),
            Some((15.0, 5.0, 90.0, 10.0))
        );
    }
}

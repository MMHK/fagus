//! The computed style of a box.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::values::{
    AnnotationSide, BoxShadow, ContainerType, Decoration, DecorationThickness, EmphasisMark, RubyAlign,
    RubyDisplay, ShapeOutside, SkipInk, TextBoxTrim, TextEdge, TextShadow, TransformFunction,
    UnderlinePosition,
};
use crate::values::{
    Appearance, BackgroundRepeat, BackgroundSize, BlendMode, BorderCollapse, BorderImage, BorderStyle,
    BoxArea, BoxDecorationBreak, BoxOrient, BoxSizing, BreakBetween, BreakInside, CaptionSide, Clear,
    ClipPath, ColumnFill, ColumnSpan, Content, ContentAlignment, ContentItem, Corners, Direction, Display,
    Filter, FlexDirection, FlexWrap, Float, FontStyle, FontUnits, GridAreas, Image, ItemAlignment,
    LengthPercentage, LengthPercentageAuto, LineBreak, LineHeight, ListStylePosition, ListStyleType,
    MaskLayers, MaxSize, ObjectFit, Overflow, Position, Rgba, Scrollbar, Sides, Size, TableLayout, TextAlign,
    TextDecorationLine, TextDecorationStyle, TextOrientation, TextTransform, TrackList, TrackSize,
    UnicodeBidi, VerticalAlign, Visibility, WhiteSpace, WritingMode,
};
use crate::values::{
    BlockJustify, EmptyCells, FontKerning, FontSizeAdjust, FontSizeBasis, FontSynthesis, FontVariant,
    GridLine, Hyphens, ImageOrientation, ImageRendering, Isolation, OpticalSizing, OutlineStyle,
    OverflowWrap, Repeat, SelfFlags, TabSize, TextFit, TextJustify, TextOverflow, TextSpacingTrim,
    TextWrapStyle, WordBreak,
};

/// Custom properties (`--name`) with their substituted token values, shared between elements that
/// do not change them.
pub type CustomProperties =
    Arc<FxHashMap<Arc<str>, Arc<lightningcss::properties::custom::TokenList<'static>>>>;

/// The computed values of all properties Fagus supports.
#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::struct_excessive_bools, reason = "flags of independent properties")]
pub struct ComputedStyle {
    // Box model and layout.
    pub display: Display,
    pub position: Position,
    /// Whether the box was inline-level before `float` or absolute positioning made it
    /// block-level: an absolutely positioned box that was block-level has its static position
    /// at the start of the next line when it follows content on a line.
    pub originally_inline: bool,
    pub inset: Sides<LengthPercentageAuto>,
    pub z_index: Option<i32>,
    /// Anchor positioning (`anchor-name`, `position-area`, insets with `anchor()`, …), `None`
    /// for most elements.
    pub anchoring: Option<Arc<crate::anchor::Anchoring>>,
    /// The calculations of `calc-size()` in the sizes, which layout applies.
    pub calc_size: Option<Arc<crate::calc_size::CalcSizes>>,
    pub float: Float,
    /// How many lines the box shows (`-webkit-line-clamp` on a vertical `-webkit-box`; Chrome
    /// 157 ignores `line-clamp`), the last ending with an ellipsis where more follows.
    pub line_clamp: Option<u32>,
    /// `-webkit-line-clamp`, which takes effect with `-webkit-box-orient: vertical`.
    pub webkit_line_clamp: Option<u32>,
    /// `-webkit-box-orient`.
    pub box_orient: BoxOrient,
    /// The other `-webkit-box-*` properties, and whether `display` is `-webkit-box`.
    pub legacy_box: crate::webkit_box::LegacyBox,
    pub clear: Clear,
    pub box_sizing: BoxSizing,
    pub width: Size,
    pub height: Size,
    pub min_width: Size,
    pub min_height: Size,
    pub max_width: MaxSize,
    pub max_height: MaxSize,
    pub aspect_ratio: Option<f32>,
    /// `aspect-ratio: auto && <ratio>`: a replaced element's natural ratio wins over the given one.
    pub aspect_ratio_auto: bool,
    pub margin: Sides<LengthPercentageAuto>,
    pub padding: Sides<LengthPercentage>,
    /// Computed border widths: 0 where the style is `none` or `hidden`.
    pub border_width: Sides<f32>,
    /// The border widths as the cascade gives them, before a style of `none` or `hidden` makes
    /// them 0, which children take with `inherit`, as in Chrome.
    pub border_width_declared: Sides<f32>,
    pub border_style: Sides<BorderStyle>,
    /// `None` means `currentColor`.
    pub border_color: Sides<Option<Rgba>>,
    pub border_radius: Corners,
    /// `corner-shape`: the curvature of each corner (top left, top right, bottom right, bottom
    /// left) as Chrome keeps it, 2 to the power of the `superellipse()` parameter: 2 is
    /// `round`, 1 `bevel`, ½ `scoop`, 4 `squircle`.
    pub corner_shape: [f32; 4],
    /// `None` until a `border-image` property is set.
    pub border_image: Option<Arc<BorderImage>>,
    /// `border-shape` (CSS Borders 4): `None` for `none`.
    pub border_shape: Option<Arc<crate::values::BorderShape>>,
    pub overflow_x: Overflow,
    pub overflow_y: Overflow,
    /// The scroll bars of `overflow-x` and `overflow-y`.
    pub scrollbar_x: Scrollbar,
    pub scrollbar_y: Scrollbar,
    pub text_overflow: TextOverflow,
    pub box_decoration_break: BoxDecorationBreak,
    pub container_type: ContainerType,
    /// `scroll-marker-group`: where a scroll container has its scroll marker group.
    pub scroll_marker_group: crate::values::ScrollMarkerGroup,
    /// Size containment from `contain` (`size`, `strict`): the box is sized as if empty.
    pub contain_size: bool,
    /// Inline-size containment from `contain: inline-size`.
    pub contain_inline_size: bool,
    /// Whether `contain` sets any containment (`size`, `inline-size`, `layout`, `style` or
    /// `paint`, or a shorthand of them).
    pub contain_any: bool,
    /// Layout or paint containment from `contain` (`layout`, `paint`, `strict`, `content`):
    /// the box is the containing block of its absolutely positioned and fixed descendants
    /// (CSS Containment 2, §3.2 and §3.4).
    pub contain_layout: bool,
    /// Layout containment itself (`layout`, `strict`, `content`, or `content-visibility:
    /// hidden`), not paint containment: the box has no baseline (CSS Containment 2, §3.2).
    pub layout_containment: bool,
    /// Whether `will-change` names a property whose other values make the box the containing
    /// block of its absolutely positioned and fixed descendants (`transform`, `translate`,
    /// `rotate`, `scale`, `perspective`, `filter`, `contain`), which it then is (CSS Will Change
    /// 1, §2.1).
    pub will_change_contains: bool,
    /// Whether `backdrop-filter` is other than `none`, which makes the box a stacking context
    /// and the containing block of its absolutely positioned and fixed descendants (Filter
    /// Effects 2, §3). The filter itself is not drawn.
    pub backdrop_filter: bool,
    /// `contain-intrinsic-width` and `contain-intrinsic-height`: the size a box with size
    /// containment has as if empty, `None` for `none`.
    pub contain_intrinsic_size: (Option<f32>, Option<f32>),
    /// `margin-trim` in the block axis: whether the block-start margins of the first children
    /// and the block-end margins of the last children of a block container are trimmed where
    /// they adjoin its edges (CSS Box 4, §3.3).
    pub margin_trim_block: (bool, bool),
    /// Whether `content-visibility: hidden` skips the box's contents: they are neither laid out
    /// nor painted, and the box has size, layout, style and paint containment (CSS Contain 2,
    /// §4). `auto` shows them in print, as in Chrome.
    pub content_hidden: bool,
    /// `overflow-clip-margin`: the box an `overflow: clip` (in both axes) or paint containment
    /// clips at, and how far beyond it (CSS Overflow 4, §3.2).
    pub overflow_clip_margin: (BoxArea, f32),
    /// `container-name`.
    pub container_names: Arc<[String]>,
    /// `initial-letter` (on `::first-letter`): how many lines the letter spans and how many it
    /// sinks into.
    pub initial_letter: Option<(f32, f32)>,
    /// `outline-width` in pixels, 0 without an outline.
    pub outline_width: f32,
    pub outline_style: OutlineStyle,
    /// `None` means `currentColor`.
    pub outline_color: Option<Rgba>,
    pub outline_offset: f32,
    pub visibility: Visibility,
    /// `appearance`: whether a form control has its native look (`auto`, and the other values
    /// but `none`).
    pub appearance: Appearance,
    /// `accent-color` (inherited): the color of checked checkboxes and radio buttons and of the
    /// value of progress bars and sliders; `None` for `auto`, the native accent.
    pub accent_color: Option<Rgba>,
    /// Whether the element uses the dark color scheme (`color-scheme`, inherited): it lists
    /// `dark` and not `light`, as Chrome picks for a page that prefers light. `light-dark()` and
    /// system colors follow it.
    pub dark: bool,
    /// Whether author style sets the element's background or border, which gives a form control
    /// the look CSS describes in place of its native one, as in Chrome.
    pub author_frame: bool,
    /// Whether the top and the bottom margin are the defaults of the user agent stylesheet that
    /// quirks mode drops at the start and end of `body` and table cells (the margins Blink gives
    /// in quirky ems: the top margins of `p`, headings, lists and `pre`, the bottom margins of
    /// `p` and `pre`).
    pub quirky_margins: (bool, bool),
    pub opacity: f32,
    pub mix_blend_mode: BlendMode,
    pub clip_path: Option<Arc<ClipPath>>,
    /// `clip: rect(top, right, bottom, left)` (CSS Masking 1, §6.1): the offsets of the edges of
    /// the clip from the top and left border edges, `None` for `auto` (the border edge itself).
    /// It applies to absolutely positioned boxes only. `None` for `clip: auto`.
    pub clip: Option<[Option<f32>; 4]>,
    /// `shape-outside`, `shape-margin` and `shape-image-threshold` (CSS Shapes 1, §6).
    pub shape_outside: Option<Arc<ShapeOutside>>,
    pub shape_margin: LengthPercentage,
    pub shape_image_threshold: f32,
    pub filter: Arc<[Filter]>,
    /// `None` until a mask property is set.
    pub mask: Option<Arc<MaskLayers>>,
    pub isolation: Isolation,
    /// `background-blend-mode`, one per background layer, repeated as needed.
    pub background_blend_mode: Arc<[BlendMode]>,
    pub box_shadow: Arc<[BoxShadow]>,
    pub transform: Arc<[TransformFunction]>,
    /// `translate`: across, down and, in pixels, toward the viewer.
    pub translate: Option<(LengthPercentage, LengthPercentage, f32)>,
    /// `rotate`: degrees about the axis `(x, y, z)`, clockwise about z.
    pub rotate: Option<(f32, f32, f32, f32)>,
    pub scale: Option<(f32, f32)>,
    pub transform_origin: (LengthPercentage, LengthPercentage),
    /// `transform-box`: the box that `transform-origin` and the percentages of the transform
    /// refer to.
    pub transform_box: crate::values::TransformBox,
    /// `perspective`, in pixels: the distance of the viewer from the plane of the box's children.
    pub perspective: Option<f32>,
    pub perspective_origin: (LengthPercentage, LengthPercentage),
    /// `transform-style: preserve-3d`: the box's transformed children share the 3D space of
    /// its own transform rather than being flattened into its plane.
    pub preserve_3d: bool,
    /// Whether the element's parent (in the document) has `transform-style: preserve-3d`: a
    /// box takes part in the 3D space of its parent element only.
    pub parent_preserve_3d: bool,
    /// Whether the element's parent (in the document, not the box it is laid out in) has a
    /// `perspective`, the only box whose perspective it is seen in (CSS Transforms 2, §6.1).
    pub parent_perspective: bool,

    // Color and background.
    pub color: Rgba,
    pub background_color: Rgba,
    /// The value of `background-color` when it depends on `currentcolor` (`currentcolor`, or a
    /// `color-mix()` or relative color of it), as declared: an element that inherits it
    /// resolves it with its own `color`, as in Chrome (CSS Color 4, §6.4).
    pub background_color_current: Option<Arc<lightningcss::properties::custom::TokenList<'static>>>,
    /// Background layers, top first. The other background lists repeat to match its length.
    pub background_image: Arc<[Image]>,
    pub background_position_x: Arc<[LengthPercentage]>,
    pub background_position_y: Arc<[LengthPercentage]>,
    pub background_size: Arc<[BackgroundSize]>,
    pub background_repeat: Arc<[(BackgroundRepeat, BackgroundRepeat)]>,
    pub background_origin: Arc<[BoxArea]>,
    /// Which background layers are fixed to the viewport (`background-attachment: fixed`),
    /// in the order of `background_image`, repeated as the list is shorter.
    pub background_fixed: Arc<[bool]>,
    /// Which background layers scroll with the box's content (`background-attachment: local`),
    /// in the order of `background_image`, repeated as the list is shorter: those of a scroll
    /// container are sized and positioned in its scrollable overflow area. Empty where none is.
    pub background_local: Arc<[bool]>,
    pub background_clip: Arc<[BoxArea]>,
    /// Which layers of `background-clip` are `text`, in the order of `background_clip`: such a
    /// layer shows only through the glyphs of the box's text (CSS Backgrounds 4, §2.1), within
    /// the border box. Empty where none is.
    pub background_clip_text: Arc<[bool]>,
    /// Which layers of `background-clip` are `border-area`, in the order of `background_clip`:
    /// such a layer shows only where the box's borders paint, whatever their color (CSS
    /// Backgrounds 4, §2.1), and through the glyphs of its text too where it is also `text`.
    /// Empty where none is.
    pub background_clip_border_area: Arc<[bool]>,

    // Fonts and text.
    pub font_family: Arc<[String]>,
    pub font_size: f32,
    /// Where the font size comes from, for the size of `monospace` text.
    pub font_size_basis: FontSizeBasis,
    pub font_weight: f32,
    pub font_style: FontStyle,
    /// `font-stretch` in percent of normal.
    pub font_stretch: f32,
    /// The metrics of the first available font that `ex`, `ch`, `cap` and `ic` refer to.
    pub font_units: FontUnits,
    pub font_variant: FontVariant,
    pub font_kerning: FontKerning,
    pub font_synthesis: FontSynthesis,
    pub font_size_adjust: FontSizeAdjust,
    /// `font-feature-settings`: features and their values.
    pub font_feature_settings: Arc<[([u8; 4], u32)]>,
    /// `font-variation-settings`: axes and their values.
    pub font_variation_settings: Arc<[([u8; 4], f32)]>,
    pub font_optical_sizing: OpticalSizing,
    /// `math-depth` (inherited): how deep in scripts of a formula the element is, which
    /// `font-size: math` scales the font size by (`MathML` Core, §3.1.3).
    pub math_depth: i32,
    /// `math-style: compact` (inherited): the element is in the compact style of inline
    /// formulas and scripts, not in display style.
    pub math_compact: bool,
    /// `math-shift: compact` (inherited): superscripts are shifted up less (cramped).
    pub math_shift_compact: bool,
    pub line_height: LineHeight,
    pub letter_spacing: f32,
    pub word_spacing: f32,
    /// `letter-spacing` and `word-spacing` (inherited) where they hold a percentage: pixels and
    /// the fraction of the font size, which each element resolves against its own font size
    /// (CSS Text 4, §7), as Chrome does.
    pub spacing_percentages: [Option<(f32, f32)>; 2],
    /// `direction` (inherited).
    pub direction: Direction,
    pub unicode_bidi: UnicodeBidi,
    /// `writing-mode` (inherited).
    pub writing_mode: WritingMode,
    /// `text-orientation` (inherited).
    pub text_orientation: TextOrientation,
    /// `text-combine-upright: all`: the text of the box is set upright in the space of one
    /// character of a vertical line (tate-chu-yoko).
    pub text_combine_upright: bool,
    pub text_align: TextAlign,
    /// Whether `text-align` is `-webkit-left`, `-webkit-center` or `-webkit-right` (from `align`
    /// attributes and `<center>`), which also align block-level children that do not fill the
    /// line and have no auto margins, as in Chrome (inherited with `text-align`).
    pub align_blocks: bool,
    /// `text-align-last`; `None` for `auto` (inherited).
    pub text_align_last: Option<TextAlign>,
    pub text_justify: TextJustify,
    /// `tab-size` (inherited).
    pub tab_size: TabSize,
    pub text_indent: LengthPercentage,
    /// `text-indent: … each-line`: lines after a forced break are indented too (inherited).
    pub text_indent_each_line: bool,
    /// `text-indent: … hanging`: the lines that would not be indented are, and the others not
    /// (inherited).
    pub text_indent_hanging: bool,
    pub text_transform: TextTransform,
    pub white_space: WhiteSpace,
    pub text_wrap_style: TextWrapStyle,
    pub vertical_align: VerticalAlign,
    pub text_decoration_line: TextDecorationLine,
    pub text_decoration_style: TextDecorationStyle,
    /// `None` means `currentColor`.
    pub text_decoration_color: Option<Rgba>,
    pub text_decoration_thickness: DecorationThickness,
    /// `text-underline-offset` in pixels, `None` for `auto` (inherited).
    pub text_underline_offset: Option<f32>,
    /// `text-underline-position` (inherited).
    pub text_underline_position: UnderlinePosition,
    pub text_decoration_skip_ink: SkipInk,
    /// The decorations of this box and of the ancestors that propagate theirs to it.
    pub decorations: Arc<[Decoration]>,
    /// `text-shadow` (inherited), the first on top.
    pub text_shadow: Arc<[TextShadow]>,
    /// `text-emphasis-style` (inherited): the mark drawn by each character, `None` for `none`.
    pub text_emphasis_style: Option<EmphasisMark>,
    /// `text-emphasis-color` (inherited); `None` means `currentColor`.
    pub text_emphasis_color: Option<Rgba>,
    /// `-webkit-text-fill-color`, which glyphs are filled with: `None` for `currentcolor`.
    pub text_fill_color: Option<Rgba>,
    /// `text-emphasis-position` (inherited).
    pub text_emphasis_position: AnnotationSide,
    /// The part the box plays in ruby layout, from its `display`.
    pub ruby_display: RubyDisplay,
    /// `ruby-position` (inherited).
    pub ruby_position: AnnotationSide,
    /// `ruby-align` (inherited).
    pub ruby_align: RubyAlign,
    /// `text-spacing-trim` (inherited).
    pub text_spacing_trim: TextSpacingTrim,
    /// `text-autospace: normal` (inherited): a little space goes between ideographs and the
    /// letters and digits of other scripts. Chrome's initial value is `no-autospace`.
    pub text_autospace: bool,
    /// `hanging-punctuation` (inherited).
    pub hanging_punctuation: crate::values::HangingPunctuation,
    /// `text-fit` (inherited).
    pub text_fit: TextFit,
    /// The factor by which layout made `font_size` larger or smaller for `text-fit` (not a CSS
    /// property): 1 where it did not. The metrics of the font are those at the size before,
    /// scaled, as Chrome paints such text scaled.
    pub text_fit_scale: f32,
    /// `text-box-trim`.
    pub text_box_trim: TextBoxTrim,
    /// `text-box-edge` (inherited): the edges over and under the text a trimmed block keeps.
    pub text_box_edge: (TextEdge, TextEdge),
    pub lang: Option<Arc<str>>,
    pub hyphens: Hyphens,
    /// `hyphenate-character` (inherited): the text shown where a word is hyphenated, `None`
    /// for `auto`.
    pub hyphenate_character: Option<Arc<str>>,
    /// `font-palette` (inherited).
    pub font_palette: crate::values::FontPalette,
    /// `hyphenate-limit-chars` (inherited): the fewest characters of a word that is hyphenated,
    /// before a hyphen and after it; 0 for `auto`.
    pub hyphenate_limit_chars: [u32; 3],
    pub overflow_wrap: OverflowWrap,
    pub word_break: WordBreak,
    /// `line-break` (inherited).
    pub line_break: LineBreak,

    // Lists and generated content.
    pub list_style_type: ListStyleType,
    pub list_style_position: ListStylePosition,
    /// `list-style-image`: the marker an image, in place of the type's, where it loads.
    pub list_style_image: Image,
    pub content: Content,
    pub counter_reset: Vec<(String, i32)>,
    pub counter_increment: Vec<(String, i32)>,
    pub counter_set: Vec<(String, i32)>,
    pub quotes: Option<Arc<[(String, String)]>>,

    // Tables.
    pub table_layout: TableLayout,
    pub border_collapse: BorderCollapse,
    /// Horizontal and vertical `border-spacing`.
    pub border_spacing: (f32, f32),
    pub caption_side: CaptionSide,
    pub empty_cells: EmptyCells,

    /// `field-sizing: content`: a form control is as large as its content, not as its `size`,
    /// `cols` and `rows` or its longest option.
    pub field_sizing_content: bool,
    /// `frame-sizing: content-height` (or `content-block-size`): an `<iframe>` whose document
    /// opts in (`<meta name=responsive-embedded-sizing>`) is as tall as that document's content.
    pub frame_sizing_content_height: bool,
    /// Whether `resize` lets the box be resized (any value but `none`): a `<textarea>` shows its
    /// resize handle then.
    pub resize: bool,

    // Flexbox and grid.
    pub flex_direction: FlexDirection,
    pub flex_wrap: FlexWrap,
    /// `flex-wrap: balance` (CSS Flexible Box Layout 2): the container wraps (`flex_wrap` is
    /// `wrap`) into lines of even free space.
    pub flex_balance: bool,
    /// `flex-line-count`: the fewest lines a balanced container wraps into.
    pub flex_line_count: u32,
    pub flex_grow: f32,
    pub flex_shrink: f32,
    pub flex_basis: Size,
    pub order: i32,
    pub justify_content: ContentAlignment,
    /// Whether `justify-content` is `safe`: items that overflow the line start at its start.
    pub justify_content_safe: bool,
    pub align_content: ContentAlignment,
    /// Whether `align-content` is `unsafe`: the content of a block container that overflows it
    /// moves as far as it says, past the start of the box.
    pub align_content_unsafe: bool,
    /// Whether `align-content` is `safe`: the lines of a flex container that overflow it start
    /// at its start.
    pub align_content_safe: bool,
    pub align_items: ItemAlignment,
    /// How `align-items` was written beyond [`ItemAlignment`]: `safe`, and `flex-start` or
    /// `flex-end` rather than `start` or `end`.
    pub align_items_flags: SelfFlags,
    /// How `align-self` was written beyond [`ItemAlignment`] (see `align_items_flags`).
    pub align_self_flags: SelfFlags,
    pub align_self: ItemAlignment,
    pub justify_items: ItemAlignment,
    pub justify_self: ItemAlignment,
    /// `justify-items` as block layout uses it (see [`BlockJustify`]).
    pub justify_items_block: BlockJustify,
    /// `justify-self` as block layout uses it (see [`BlockJustify`]).
    pub justify_self_block: BlockJustify,
    /// `None` means `normal`.
    pub row_gap: Option<LengthPercentage>,
    pub column_gap: Option<LengthPercentage>,
    pub grid_template_columns: TrackList,
    pub grid_template_rows: TrackList,
    pub grid_template_areas: Option<Arc<GridAreas>>,
    pub grid_auto_rows: Arc<[TrackSize]>,
    pub grid_auto_columns: Arc<[TrackSize]>,
    /// `grid-auto-flow: column`.
    pub grid_auto_flow_column: bool,
    /// `grid-auto-flow: dense`.
    pub grid_auto_flow_dense: bool,
    pub grid_row_start: GridLine,
    pub grid_row_end: GridLine,
    pub grid_column_start: GridLine,
    pub grid_column_end: GridLine,

    // Fragmentation and paged media.
    pub break_before: BreakBetween,
    pub break_after: BreakBetween,
    pub break_inside: BreakInside,
    pub orphans: u32,
    pub widows: u32,
    pub page: Option<Arc<str>>,
    pub string_set: Vec<(String, Vec<ContentItem>)>,
    /// `position: running(name)`.
    pub running: Option<Arc<str>>,
    /// `float: footnote`: the element is a footnote, which goes into the footnote area of the
    /// page its call is on (CSS Generated Content for Paged Media 3, §2). `float` is then `none`.
    pub footnote: bool,
    /// `footnote-display`.
    pub footnote_display: crate::values::FootnoteDisplay,
    /// `footnote-policy`.
    pub footnote_policy: crate::values::FootnotePolicy,
    /// Whether this is the style of a `::footnote-marker`, whose text labels its footnote in a
    /// tagged PDF.
    pub footnote_marker: bool,
    /// `bookmark-level`: the level of the element's entry in the PDF outline (CSS GCPM 3, §5).
    pub bookmark_level: crate::values::BookmarkLevel,
    /// `bookmark-label`, when it is not the element's text.
    pub bookmark_label: Option<Vec<ContentItem>>,
    /// `bookmark-state: closed`.
    pub bookmark_closed: bool,

    /// `column-count`; `None` for `auto`.
    pub column_count: Option<u32>,
    /// `column-width` in pixels; `None` for `auto`.
    pub column_width: Option<f32>,
    pub column_rule_width: f32,
    pub column_rule_style: BorderStyle,
    /// `None` means `currentColor`.
    pub column_rule_color: Option<Rgba>,
    pub column_fill: ColumnFill,
    pub column_span: ColumnSpan,
    /// `column-height` in pixels; `None` for `auto`.
    pub column_height: Option<f32>,
    /// `column-wrap`: whether rows of columns wrap below one another, `None` for `auto` (they
    /// wrap where `column-height` is set).
    pub column_wrap: Option<bool>,
    /// `-fagus-repeat`: where a row of a table's header or footer shows.
    pub repeat: Repeat,
    pub object_fit: ObjectFit,
    pub image_orientation: ImageOrientation,
    pub image_rendering: ImageRendering,
    /// Whether `text-rendering: optimizeSpeed` turns off the ligatures and contextual forms
    /// that are on by default, as Chrome does.
    pub text_rendering_speed: bool,
    /// `object-position`: where the content of an image lies in its box, as the offsets of
    /// background positions (percentages of the free space).
    pub object_position: (LengthPercentage, LengthPercentage),
    /// The SVG painting properties (`fill`, `stroke`, …) that CSS declares for the element, as
    /// names and values in CSS text for the SVG renderer; not inherited here, since the SVG
    /// renderer inherits them.
    pub svg_paint: Arc<[(&'static str, String)]>,

    /// The effective zoom: the product of the `zoom` of the element and of its ancestors, by
    /// which all absolute lengths the element's properties specify have been multiplied (CSS
    /// Viewport 1, §5). Layout scales the natural sizes of images and the fixed sizes of form
    /// controls by it.
    pub zoom: f32,
    /// The element's own `zoom`, its effective zoom over its parent's: what `zoom: inherit` of
    /// a child takes.
    pub specified_zoom: f32,
    /// The width of the stroke with which Chrome's native theme draws the frame of a form
    /// control along the middle of its border, in place of the border; 0 for a hairline, the
    /// thinnest line a device can draw. `None` for other boxes. Layout sets it.
    pub native_frame: Option<f32>,
    /// Whether the box is the bar of a `MathML` fraction or radical, which Chrome snaps to whole
    /// pixels and then moves up by half its snapped height, so that it straddles its top
    /// (Blink's `MathMLPainter::PaintBar`). Layout sets it.
    pub math_bar: bool,
    /// The element whose `::before` pseudo-element has this style, which comes before the
    /// element's children in tree order (and in the painting order of positioned boxes); `None`
    /// for other styles. Not inherited.
    pub before_of: Option<fagus_dom::NodeId>,

    pub custom_properties: CustomProperties,
}

/// The initial values, made once: a style starts as a copy, which only counts references.
static INITIAL: std::sync::LazyLock<ComputedStyle> = std::sync::LazyLock::new(ComputedStyle::initial);

impl Default for ComputedStyle {
    /// The initial values of all properties.
    fn default() -> Self {
        INITIAL.clone()
    }
}

impl ComputedStyle {
    /// The initial values of all properties, shared.
    pub fn initial_values() -> &'static Self {
        &INITIAL
    }

    fn initial() -> Self {
        let auto = LengthPercentageAuto::Auto;
        Self {
            display: Display::INLINE,
            position: Position::Static,
            originally_inline: false,
            inset: Sides::all(auto.clone()),
            z_index: None,
            anchoring: None,
            calc_size: None,
            float: Float::None,
            line_clamp: None,
            webkit_line_clamp: None,
            box_orient: BoxOrient::Horizontal,
            legacy_box: crate::webkit_box::LegacyBox::default(),
            clear: Clear::None,
            box_sizing: BoxSizing::ContentBox,
            width: Size::Auto,
            height: Size::Auto,
            min_width: Size::Auto,
            min_height: Size::Auto,
            max_width: MaxSize::None,
            max_height: MaxSize::None,
            aspect_ratio: None,
            aspect_ratio_auto: true,
            margin: Sides::all(LengthPercentageAuto::ZERO),
            padding: Sides::all(LengthPercentage::ZERO),
            border_width: Sides::all(0.0),
            border_width_declared: Sides::all(0.0),
            border_style: Sides::all(BorderStyle::None),
            border_color: Sides::all(None),
            border_radius: Corners::default(),
            corner_shape: [2.0; 4],
            border_image: None,
            border_shape: None,
            overflow_x: Overflow::Visible,
            overflow_y: Overflow::Visible,
            scrollbar_x: Scrollbar::None,
            scrollbar_y: Scrollbar::None,
            text_overflow: TextOverflow::Clip,
            box_decoration_break: BoxDecorationBreak::Slice,
            container_type: ContainerType::Normal,
            scroll_marker_group: crate::values::ScrollMarkerGroup::None,
            contain_size: false,
            contain_inline_size: false,
            contain_any: false,
            contain_layout: false,
            layout_containment: false,
            will_change_contains: false,
            backdrop_filter: false,
            contain_intrinsic_size: (None, None),
            overflow_clip_margin: (BoxArea::PaddingBox, 0.0),
            content_hidden: false,
            margin_trim_block: (false, false),
            container_names: Arc::from([]),
            initial_letter: None,
            outline_width: 3.0,
            outline_style: OutlineStyle::default(),
            outline_color: None,
            outline_offset: 0.0,
            visibility: Visibility::Visible,
            appearance: Appearance::Auto,
            accent_color: None,
            dark: false,
            author_frame: false,
            quirky_margins: (false, false),
            opacity: 1.0,
            mix_blend_mode: BlendMode::Normal,
            clip_path: None,
            clip: None,
            shape_outside: None,
            shape_margin: LengthPercentage::ZERO,
            shape_image_threshold: 0.0,
            filter: Arc::from([]),
            mask: None,
            isolation: Isolation::Auto,
            background_blend_mode: Arc::from([]),
            box_shadow: Arc::from([]),
            transform: Arc::from([]),
            translate: None,
            rotate: None,
            scale: None,
            transform_origin: (LengthPercentage::Percent(0.5), LengthPercentage::Percent(0.5)),
            transform_box: crate::values::TransformBox::ViewBox,
            perspective: None,
            perspective_origin: (LengthPercentage::Percent(0.5), LengthPercentage::Percent(0.5)),
            parent_perspective: false,
            preserve_3d: false,
            parent_preserve_3d: false,
            color: Rgba::BLACK,
            background_color: Rgba::TRANSPARENT,
            background_color_current: None,
            background_image: Arc::from([Image::None]),
            background_position_x: Arc::from([LengthPercentage::ZERO]),
            background_position_y: Arc::from([LengthPercentage::ZERO]),
            background_size: Arc::from([BackgroundSize::Explicit(
                LengthPercentageAuto::Auto,
                LengthPercentageAuto::Auto,
            )]),
            background_repeat: Arc::from([(BackgroundRepeat::Repeat, BackgroundRepeat::Repeat)]),
            background_origin: Arc::from([BoxArea::PaddingBox]),
            background_fixed: Arc::from([false]),
            background_local: Arc::from([]),
            background_clip: Arc::from([BoxArea::BorderBox]),
            background_clip_text: Arc::from([]),
            background_clip_border_area: Arc::from([]),
            // The standard font (`-webkit-standard` in Chrome), which is the serif one but for
            // some languages.
            font_family: Arc::from(["-webkit-standard".to_owned()]),
            font_size: 16.0,
            font_size_basis: FontSizeBasis::default(),
            font_weight: 400.0,
            font_style: FontStyle::Normal,
            font_stretch: 100.0,
            font_units: FontUnits::default(),
            font_variant: FontVariant::default(),
            font_kerning: FontKerning::Auto,
            font_synthesis: FontSynthesis::default(),
            font_size_adjust: FontSizeAdjust::None,
            font_feature_settings: Arc::from([]),
            font_variation_settings: Arc::from([]),
            font_optical_sizing: OpticalSizing::Auto,
            math_depth: 0,
            math_compact: false,
            math_shift_compact: false,
            line_height: LineHeight::Normal,
            letter_spacing: 0.0,
            word_spacing: 0.0,
            spacing_percentages: [None, None],
            direction: Direction::Ltr,
            unicode_bidi: UnicodeBidi::Normal,
            writing_mode: WritingMode::HorizontalTb,
            text_orientation: TextOrientation::Mixed,
            text_combine_upright: false,
            text_align: TextAlign::Start,
            align_blocks: false,
            text_align_last: None,
            text_justify: TextJustify::Auto,
            tab_size: TabSize::Spaces(8.0),
            text_indent: LengthPercentage::ZERO,
            text_indent_each_line: false,
            text_indent_hanging: false,
            text_transform: TextTransform::None,
            white_space: WhiteSpace::NORMAL,
            text_wrap_style: TextWrapStyle::Auto,
            vertical_align: VerticalAlign::Baseline,
            text_decoration_line: TextDecorationLine::default(),
            text_decoration_style: TextDecorationStyle::Solid,
            text_decoration_color: None,
            text_decoration_thickness: DecorationThickness::Auto,
            text_underline_offset: None,
            text_underline_position: UnderlinePosition::Auto,
            text_decoration_skip_ink: SkipInk::Auto,
            decorations: Arc::from([]),
            text_shadow: Arc::from([]),
            text_emphasis_style: None,
            text_emphasis_color: None,
            text_fill_color: None,
            text_emphasis_position: AnnotationSide::Over,
            ruby_display: RubyDisplay::None,
            ruby_position: AnnotationSide::Over,
            ruby_align: RubyAlign::SpaceAround,
            text_spacing_trim: TextSpacingTrim::Normal,
            text_autospace: false,
            hanging_punctuation: crate::values::HangingPunctuation::default(),
            text_fit: TextFit::default(),
            text_fit_scale: 1.0,
            text_box_trim: TextBoxTrim::default(),
            text_box_edge: (TextEdge::Text, TextEdge::Text),
            lang: None,
            hyphens: Hyphens::Manual,
            hyphenate_character: None,
            font_palette: crate::values::FontPalette::Normal,
            hyphenate_limit_chars: [0; 3],
            overflow_wrap: OverflowWrap::Normal,
            word_break: WordBreak::Normal,
            line_break: LineBreak::Auto,
            list_style_type: ListStyleType::Disc,
            list_style_image: Image::None,
            list_style_position: ListStylePosition::Outside,
            content: Content::Normal,
            counter_reset: Vec::new(),
            counter_increment: Vec::new(),
            counter_set: Vec::new(),
            quotes: None,
            table_layout: TableLayout::Auto,
            border_collapse: BorderCollapse::Separate,
            border_spacing: (0.0, 0.0),
            caption_side: CaptionSide::Top,
            empty_cells: EmptyCells::Show,
            field_sizing_content: false,
            frame_sizing_content_height: false,
            resize: false,
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::NoWrap,
            flex_balance: false,
            flex_line_count: 1,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            flex_basis: Size::Auto,
            order: 0,
            justify_content: ContentAlignment::Normal,
            justify_content_safe: false,
            align_content: ContentAlignment::Normal,
            align_content_unsafe: false,
            align_content_safe: false,
            align_items: ItemAlignment::Normal,
            align_self: ItemAlignment::Auto,
            align_items_flags: SelfFlags::default(),
            align_self_flags: SelfFlags::default(),
            justify_items: ItemAlignment::Normal,
            justify_self: ItemAlignment::Auto,
            justify_items_block: BlockJustify::NORMAL,
            justify_self_block: BlockJustify::AUTO,
            row_gap: None,
            column_gap: None,
            grid_template_columns: TrackList::default(),
            grid_template_rows: TrackList::default(),
            grid_template_areas: None,
            grid_auto_rows: Arc::from([]),
            grid_auto_columns: Arc::from([]),
            grid_auto_flow_column: false,
            grid_auto_flow_dense: false,
            grid_row_start: GridLine::Auto,
            grid_row_end: GridLine::Auto,
            grid_column_start: GridLine::Auto,
            grid_column_end: GridLine::Auto,
            break_before: BreakBetween::Auto,
            break_after: BreakBetween::Auto,
            break_inside: BreakInside::Auto,
            orphans: 2,
            widows: 2,
            page: None,
            string_set: Vec::new(),
            running: None,
            footnote: false,
            footnote_display: crate::values::FootnoteDisplay::Block,
            footnote_policy: crate::values::FootnotePolicy::Auto,
            footnote_marker: false,
            bookmark_level: crate::values::BookmarkLevel::Auto,
            bookmark_label: None,
            bookmark_closed: false,
            column_count: None,
            column_width: None,
            column_rule_width: 3.0,
            column_rule_style: BorderStyle::None,
            column_rule_color: None,
            column_fill: ColumnFill::Balance,
            column_span: ColumnSpan::None,
            column_height: None,
            column_wrap: None,
            repeat: Repeat::Auto,
            object_fit: ObjectFit::Fill,
            image_orientation: ImageOrientation::FromImage,
            image_rendering: ImageRendering::Auto,
            text_rendering_speed: false,
            object_position: (LengthPercentage::Percent(0.5), LengthPercentage::Percent(0.5)),
            svg_paint: Arc::from([]),
            zoom: 1.0,
            specified_zoom: 1.0,
            native_frame: None,
            math_bar: false,
            before_of: None,
            custom_properties: Arc::default(),
        }
    }
}

impl ComputedStyle {
    /// A style for a child: inherited properties come from `self`, all others are initial.
    #[must_use]
    pub fn inherit(&self) -> Self {
        Self {
            color: self.color,
            font_family: Arc::clone(&self.font_family),
            font_size: self.font_size,
            font_size_basis: self.font_size_basis,
            font_weight: self.font_weight,
            font_style: self.font_style,
            font_stretch: self.font_stretch,
            font_units: self.font_units,
            font_variant: self.font_variant,
            font_kerning: self.font_kerning,
            font_synthesis: self.font_synthesis,
            font_size_adjust: self.font_size_adjust.clone(),
            font_feature_settings: Arc::clone(&self.font_feature_settings),
            font_variation_settings: Arc::clone(&self.font_variation_settings),
            font_optical_sizing: self.font_optical_sizing,
            math_depth: self.math_depth,
            math_compact: self.math_compact,
            math_shift_compact: self.math_shift_compact,
            line_height: self.line_height,
            letter_spacing: self.letter_spacing,
            word_spacing: self.word_spacing,
            spacing_percentages: self.spacing_percentages,
            direction: self.direction,
            writing_mode: self.writing_mode,
            text_orientation: self.text_orientation,
            text_align: self.text_align,
            align_blocks: self.align_blocks,
            text_align_last: self.text_align_last,
            text_justify: self.text_justify,
            tab_size: self.tab_size,
            text_indent: self.text_indent.clone(),
            text_indent_each_line: self.text_indent_each_line,
            text_indent_hanging: self.text_indent_hanging,
            text_transform: self.text_transform,
            white_space: self.white_space,
            text_wrap_style: self.text_wrap_style,
            visibility: self.visibility,
            accent_color: self.accent_color,
            dark: self.dark,
            lang: self.lang.clone(),
            hyphenate_character: self.hyphenate_character.clone(),
            font_palette: self.font_palette.clone(),
            hyphenate_limit_chars: self.hyphenate_limit_chars,
            hyphens: self.hyphens,
            overflow_wrap: self.overflow_wrap,
            word_break: self.word_break,
            line_break: self.line_break,
            text_underline_offset: self.text_underline_offset,
            text_underline_position: self.text_underline_position,
            text_decoration_skip_ink: self.text_decoration_skip_ink,
            image_orientation: self.image_orientation,
            image_rendering: self.image_rendering,
            text_rendering_speed: self.text_rendering_speed,
            text_shadow: Arc::clone(&self.text_shadow),
            text_emphasis_style: self.text_emphasis_style.clone(),
            text_emphasis_color: self.text_emphasis_color,
            text_fill_color: self.text_fill_color,
            text_emphasis_position: self.text_emphasis_position,
            ruby_position: self.ruby_position,
            ruby_align: self.ruby_align,
            text_spacing_trim: self.text_spacing_trim,
            text_autospace: self.text_autospace,
            hanging_punctuation: self.hanging_punctuation,
            text_fit: self.text_fit,
            text_box_edge: self.text_box_edge,
            list_style_type: self.list_style_type.clone(),
            list_style_position: self.list_style_position,
            list_style_image: self.list_style_image.clone(),
            quotes: self.quotes.clone(),
            border_collapse: self.border_collapse,
            border_spacing: self.border_spacing,
            caption_side: self.caption_side,
            empty_cells: self.empty_cells,
            orphans: self.orphans,
            widows: self.widows,
            zoom: self.zoom,
            custom_properties: Arc::clone(&self.custom_properties),
            parent_perspective: self.perspective.is_some(),
            parent_preserve_3d: self.preserve_3d,
            // The initial `justify-items: legacy` takes on a `legacy` value of the parent's.
            justify_items_block: if self.justify_items_block.legacy {
                self.justify_items_block
            } else {
                BlockJustify::NORMAL
            },
            ..Self::default()
        }
    }

    /// The OpenType features that `font-kerning`, `font-variant-ligatures`, `-east-asian`,
    /// `-numeric`, `-alternates` and `-position` ask for, in Chrome's order; the features for
    /// capitals depend on the face and `font-feature-settings` comes after them. Letter spacing
    /// turns off common and contextual ligatures, as in Chrome.
    pub fn font_features(&self) -> Vec<([u8; 4], u32)> {
        use crate::values::{EAST_ASIAN_FEATURES, FontVariantPosition, Ligatures, NUMERIC_FEATURES};
        let variant = &self.font_variant;
        let mut features = Vec::new();
        if self.font_kerning == FontKerning::None {
            features.push((*b"kern", 0));
        }
        let [common, discretionary, historical, contextual] = variant.ligatures;
        let spaced = self.letter_spacing != 0.0;
        // `text-rendering: optimizeSpeed` turns off what is on by default.
        let off = |state: Ligatures| {
            state == Ligatures::Off || (state == Ligatures::Normal && self.text_rendering_speed)
        };
        // Ligatures turned on or off say so, also against the features of an `@font-face`
        // rule, which come before.
        if spaced || off(common) {
            features.extend([(*b"liga", 0), (*b"clig", 0)]);
        } else if common == Ligatures::On {
            features.extend([(*b"liga", 1), (*b"clig", 1)]);
        }
        match discretionary {
            Ligatures::On => features.push((*b"dlig", 1)),
            Ligatures::Off => features.push((*b"dlig", 0)),
            Ligatures::Normal => {}
        }
        match historical {
            Ligatures::On => features.push((*b"hlig", 1)),
            Ligatures::Off => features.push((*b"hlig", 0)),
            Ligatures::Normal => {}
        }
        if spaced || off(contextual) {
            features.push((*b"calt", 0));
        } else if contextual == Ligatures::On {
            features.push((*b"calt", 1));
        }
        for (bit, (_, tag)) in EAST_ASIAN_FEATURES.iter().enumerate() {
            if variant.east_asian & (1 << bit) != 0 {
                features.push((*tag, 1));
            }
        }
        for (bit, (_, tag)) in NUMERIC_FEATURES.iter().enumerate() {
            if variant.numeric & (1 << bit) != 0 {
                features.push((*tag, 1));
            }
        }
        if variant.historical_forms {
            features.push((*b"hist", 1));
        }
        match variant.position {
            FontVariantPosition::Normal => {}
            FontVariantPosition::Sub => features.push((*b"subs", 1)),
            FontVariantPosition::Super => features.push((*b"sups", 1)),
        }
        features
    }

    /// The used `line-height` in pixels, in units of 1/64 pixel as browsers lay out lines: a
    /// number multiplies the font size rounded to such units and the product is cut to them, a
    /// length is rounded to them (as in Chrome). `normal` uses the font's own line spacing.
    pub fn line_height_px(&self, normal: f32) -> f32 {
        const UNITS: f32 = 64.0;
        match self.line_height {
            LineHeight::Normal => normal,
            LineHeight::Number(factor) => {
                let size = (self.font_size * UNITS).round() / UNITS;
                // Chrome keeps a number as a percentage (in single precision) and takes that
                // percentage of the size: `1.5` stays exact, and `1.3333333` of 30 pixels comes
                // out a little under 40 and is cut to 2559/64 pixels.
                #[allow(clippy::cast_possible_truncation, reason = "a percentage, as Chrome keeps it")]
                let percentage = (f64::from(factor) * 100.0) as f32;
                (size * percentage / 100.0 * UNITS).floor() / UNITS
            }
            LineHeight::Px(px) => (px * UNITS).round() / UNITS,
        }
    }

    /// The border color of a side, with `currentColor` resolved.
    pub fn border_color(&self, color: Option<Rgba>) -> Rgba {
        color.unwrap_or(self.color)
    }

    /// Whether any containment applies to the box: from `contain`, or a query container's
    /// (CSS Containment 2, §3; CSS Conditional 5, §6.1). The `html` and `body` elements
    /// with it do not give their backgrounds to the canvas.
    pub fn has_containment(&self) -> bool {
        self.contain_any || self.container_type != ContainerType::Normal
    }

    /// Whether the box lays out its content in columns (CSS Multi-column Layout 1): its
    /// `column-count`, `column-width` or `column-height` (Level 2) is not `auto`, as in Blink.
    pub fn is_multicol(&self) -> bool {
        self.column_count.is_some() || self.column_width.is_some() || self.column_height.is_some()
    }

    /// Whether the box has size containment in its width and in its height (`contain`, or a
    /// size query container): it is sized as if it had no content (CSS Containment 2, §3.1).
    pub fn size_containment(&self) -> (bool, bool) {
        let size = self.contain_size || self.container_type == ContainerType::Size;
        let inline = size || self.contain_inline_size || self.container_type == ContainerType::InlineSize;
        if self.is_vertical() {
            (size, inline)
        } else {
            (inline, size)
        }
    }

    /// Whether the box floats (`float` other than `none`).
    pub fn is_floating(&self) -> bool {
        self.float != Float::None
    }

    /// Whether the box is out of normal flow: absolutely positioned, fixed, running or a
    /// footnote.
    pub fn is_out_of_flow(&self) -> bool {
        matches!(self.position, Position::Absolute | Position::Fixed)
            || self.running.is_some()
            || self.footnote
    }

    /// Whether the box is positioned (any `position` but `static`).
    pub fn is_positioned(&self) -> bool {
        self.position != Position::Static
    }

    /// Whether a layer of the background is clipped to the box's text (`background-clip: text`).
    pub fn clips_background_to_text(&self) -> bool {
        self.background_clip_text.iter().any(|&text| text)
    }

    /// Whether a layer of the background is clipped to the area of the box's borders
    /// (`background-clip: border-area`).
    pub fn clips_background_to_border_area(&self) -> bool {
        self.background_clip_border_area.iter().any(|&border| border)
    }

    /// Whether the box is transformed (`transform`, `translate`, `rotate` or `scale`).
    pub fn has_transform(&self) -> bool {
        !self.transform.is_empty()
            || self.translate.is_some()
            || self.rotate.is_some()
            || self.scale.is_some()
    }

    /// Whether the box is the containing block of absolutely positioned descendants.
    pub fn contains_absolute(&self) -> bool {
        self.is_positioned() || self.has_transform() || self.contains_out_of_flow()
    }

    /// Whether layout or paint containment, `will-change`, `backdrop-filter` or `transform-style:
    /// preserve-3d` make the box the containing block of its absolutely positioned and fixed
    /// descendants.
    pub fn contains_out_of_flow(&self) -> bool {
        self.contain_layout
            || self.will_change_contains
            || self.backdrop_filter
            // Not that of an inline box, which transforms do not apply to.
            || (self.preserve_3d && self.display != Display::INLINE)
    }

    /// Whether the box's inline base direction is right to left (`direction: rtl`).
    pub fn is_rtl(&self) -> bool {
        self.direction == Direction::Rtl
    }

    /// Whether the box's lines are vertical (`writing-mode` other than `horizontal-tb`).
    pub fn is_vertical(&self) -> bool {
        self.writing_mode.is_vertical()
    }

    /// The physical side `text-align` (or `text-align-last`) `align` stands for in this box:
    /// `start` and `end` by its direction, the others as they are.
    pub fn physical_align(&self, align: TextAlign) -> TextAlign {
        match (align, self.is_rtl()) {
            (TextAlign::Start, false) | (TextAlign::End, true) => TextAlign::Left,
            (TextAlign::Start, true) | (TextAlign::End, false) => TextAlign::Right,
            (align, _) => align,
        }
    }

    /// Whether each fragment of the box has its own border, padding and margin
    /// (`box-decoration-break: clone`).
    pub fn clones_decorations(&self) -> bool {
        self.box_decoration_break == BoxDecorationBreak::Clone
    }

    /// Whether the box establishes a stacking context (CSS 2.1 Appendix E, CSS Color 4 §3.2,
    /// CSS Transforms 1 §3): a fixed or sticky box always does (CSS Positioned Layout 3, §6),
    /// with `z-index: auto` too, as in Chrome.
    pub fn is_stacking_context(&self) -> bool {
        (self.is_positioned() && self.z_index.is_some())
            || matches!(self.position, Position::Fixed | Position::Sticky)
            || self.opacity < 1.0
            || self.has_transform()
            || self.mix_blend_mode != BlendMode::Normal
            || self.isolation == Isolation::Isolate
            || self.clip_path.is_some()
            || !self.filter.is_empty()
            || self.mask.as_ref().is_some_and(|mask| !mask.is_none())
            || self.perspective.is_some()
            || self.contains_out_of_flow()
    }
}

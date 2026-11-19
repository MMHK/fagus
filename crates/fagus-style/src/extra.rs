//! Properties that lightningcss stores as raw tokens and Fagus parses itself.

use lightningcss::properties::custom::{Token, TokenList, TokenOrValue};

use crate::compute::Context;
use crate::style::ComputedValues;
use crate::values::{
    AnnotationSide, RubyAlign, TextBoxTrim, TextEdge, TextOrientation, TextSpacingTrim, WritingMode,
};
use crate::values::{
    BookmarkLevel, BorderCollapse, BorderStyle, BoxArea, BreakBetween, BreakInside, CalcNode, CaptionSide,
    Clear, ColumnFill, ColumnSpan, Content, ContentItem, ContentTarget, EmptyCells, Float, FootnoteDisplay,
    FootnotePolicy, Layer, LayerShown, LengthPercentage, ListStyleType, MAX_COLUMNS, ObjectFit,
    OpticalSizing, Repeat, StringWhich, TableLayout, UnderlinePosition,
};
use crate::values::{
    EAST_ASIAN_FEATURES, FontKerning, FontMetric, FontSizeAdjust, FontSynthesis, FontVariant,
    FontVariantCaps, FontVariantPosition, Ligatures, NUMERIC_FEATURES, TextWrapStyle, WhiteSpaceCollapse,
};

/// Applies a declaration of a property that lightningcss does not parse.
pub(crate) fn apply(name: &str, tokens: &TokenList<'static>, style: &mut ComputedValues, context: &Context) {
    let name = name.to_ascii_lowercase();
    if crate::anchor::apply_property(&name, tokens, style) {
        return;
    }
    let tokens = significant(tokens);
    let first_ident = tokens
        .first()
        .and_then(|token| ident(token))
        .map(str::to_ascii_lowercase);
    let keyword = first_ident.as_deref().unwrap_or("");
    match name.as_str() {
        "break-before" => style.break_before = break_between(keyword).unwrap_or(style.break_before),
        "break-after" => style.break_after = break_between(keyword).unwrap_or(style.break_after),
        "page-break-before" => style.break_before = legacy_break(keyword).unwrap_or(style.break_before),
        "page-break-after" => style.break_after = legacy_break(keyword).unwrap_or(style.break_after),
        "break-inside" | "page-break-inside" => {
            style.break_inside = match keyword {
                "auto" => BreakInside::Auto,
                "avoid" => BreakInside::Avoid,
                "avoid-page" => BreakInside::AvoidPage,
                "avoid-column" => BreakInside::AvoidColumn,
                _ => style.break_inside,
            }
        }
        "orphans" => {
            if let Some(value) = tokens
                .first()
                .and_then(|token| integer(token))
                .filter(|value| *value > 0)
            {
                style.orphans = value as u32;
            }
        }
        "widows" => {
            if let Some(value) = tokens
                .first()
                .and_then(|token| integer(token))
                .filter(|value| *value > 0)
            {
                style.widows = value as u32;
            }
        }
        "initial-letter" => {
            let size = tokens
                .first()
                .and_then(|token| number(token))
                .filter(|size| *size >= 1.0);
            // A drop letter (the default) sinks as many lines as its size rounded up, as in
            // Chrome 157 (not the size rounded down the spec gives).
            let sink = match tokens.get(1) {
                None => size.map(f32::ceil),
                Some(token) => match ident(token).map(str::to_ascii_lowercase).as_deref() {
                    Some("drop") => size.map(f32::ceil),
                    Some("raise") => Some(1.0),
                    _ => integer(token).filter(|sink| *sink >= 1).map(|sink| sink as f32),
                },
            };
            style.initial_letter = match (keyword, size, sink) {
                ("normal", ..) => None,
                (_, Some(size), Some(sink)) => Some((size, sink)),
                _ => style.initial_letter,
            };
        }
        "shape-outside" => {
            // The tokens again, apart.
            let spaced = tokens
                .iter()
                .flat_map(|&token| [token.clone(), TokenOrValue::Token(Token::WhiteSpace(" ".into()))])
                .collect();
            if let Some(shape) = crate::shape::shape_outside(&TokenList(spaced), context) {
                style.shape_outside = shape;
            }
        }
        "shape-margin" => {
            if let Some(margin) = tokens
                .first()
                .and_then(|token| length_percentage(token, context))
                .filter(|margin| !matches!(margin, LengthPercentage::Px(px) if *px < 0.0))
                .filter(|margin| !matches!(margin, LengthPercentage::Percent(percent) if *percent < 0.0))
            {
                style.shape_margin = margin;
            }
        }
        "shape-image-threshold" => {
            let threshold = tokens.first().and_then(|token| match token {
                TokenOrValue::Token(Token::Percentage { unit_value, .. }) => Some(*unit_value),
                _ => number(token),
            });
            if let Some(threshold) = threshold.filter(|threshold| threshold.is_finite()) {
                style.shape_image_threshold = threshold.clamp(0.0, 1.0);
            }
        }
        // The start of the inline axis is on the right in a right-to-left containing block.
        "float" => {
            let (float, footnote) = match (keyword, context.parent_rtl) {
                ("left", _) | ("inline-start", false) | ("inline-end", true) => (Float::Left, false),
                ("right", _) | ("inline-end", false) | ("inline-start", true) => (Float::Right, false),
                ("none", _) => (Float::None, false),
                // A footnote does not float beside the text (CSS GCPM 3, §2.1).
                ("footnote", _) => (Float::None, true),
                _ => (style.float, style.footnote),
            };
            (style.float, style.footnote) = (float, footnote);
        }
        "footnote-display" => {
            style.footnote_display = match keyword {
                "block" => FootnoteDisplay::Block,
                "inline" => FootnoteDisplay::Inline,
                "compact" => FootnoteDisplay::Compact,
                _ => style.footnote_display,
            }
        }
        "footnote-policy" => {
            style.footnote_policy = match keyword {
                "auto" => FootnotePolicy::Auto,
                "line" => FootnotePolicy::Line,
                "block" => FootnotePolicy::Block,
                _ => style.footnote_policy,
            }
        }
        "clear" => {
            style.clear = match (keyword, context.parent_rtl) {
                ("left", _) | ("inline-start", false) | ("inline-end", true) => Clear::Left,
                ("right", _) | ("inline-end", false) | ("inline-start", true) => Clear::Right,
                ("both", _) => Clear::Both,
                ("none", _) => Clear::None,
                _ => style.clear,
            }
        }
        "clip" => match tokens.as_slice() {
            [TokenOrValue::Function(function)] if function.name.0.eq_ignore_ascii_case("rect") => {
                if let Some(rect) = clip_rect(&function.arguments, context) {
                    style.clip = Some(rect);
                }
            }
            _ if keyword == "auto" && tokens.len() == 1 => style.clip = None,
            _ => {}
        },
        "border-collapse" => {
            style.border_collapse = match keyword {
                "collapse" => BorderCollapse::Collapse,
                "separate" => BorderCollapse::Separate,
                _ => style.border_collapse,
            }
        }
        "table-layout" => {
            style.table_layout = match keyword {
                "fixed" => TableLayout::Fixed,
                "auto" => TableLayout::Auto,
                _ => style.table_layout,
            }
        }
        "hyphenate-character" => match tokens.first() {
            Some(TokenOrValue::Token(Token::String(text))) if tokens.len() == 1 => {
                style.hyphenate_character = Some(text.as_ref().into());
            }
            _ if keyword == "auto" => style.hyphenate_character = None,
            _ => {}
        },
        // One to three values, each `auto` (0) or a positive integer; those left out are `auto`.
        "hyphenate-limit-chars" => {
            let values: Option<Vec<u32>> = tokens
                .iter()
                .map(|token| match ident(token) {
                    Some(word) if word.eq_ignore_ascii_case("auto") => Some(0),
                    Some(_) => None,
                    None => integer(token).filter(|value| *value > 0).map(i32::unsigned_abs),
                })
                .collect();
            if let Some(values) = values.filter(|values| (1..=3).contains(&values.len())) {
                let mut limits = [0; 3];
                limits[..values.len()].copy_from_slice(&values);
                style.hyphenate_limit_chars = limits;
            }
        }
        // `auto` leaves the quotes to the language; pairs of strings, of which there must be an
        // even number.
        "quotes" => match keyword {
            "auto" => style.quotes = None,
            "none" => style.quotes = Some(std::sync::Arc::from([])),
            _ => {
                let strings: Option<Vec<String>> = tokens
                    .iter()
                    .map(|token| match token {
                        TokenOrValue::Token(Token::String(text)) => Some(text.to_string()),
                        _ => None,
                    })
                    .collect();
                if let Some(strings) = strings.filter(|strings| !strings.is_empty() && strings.len() % 2 == 0)
                {
                    style.quotes = Some(
                        strings
                            .chunks(2)
                            .map(|pair| (pair[0].clone(), pair[1].clone()))
                            .collect(),
                    );
                }
            }
        },
        "caption-side" => {
            style.caption_side = match keyword {
                "bottom" => CaptionSide::Bottom,
                "top" => CaptionSide::Top,
                _ => style.caption_side,
            }
        }
        "page" => {
            // A page name is a case-sensitive `<custom-ident>`, so it keeps its letters.
            style.page = (keyword != "auto" && !keyword.is_empty()).then(|| {
                tokens
                    .first()
                    .and_then(|token| ident(token))
                    .unwrap_or_default()
                    .into()
            });
        }
        "column-count" => {
            if keyword == "auto" {
                style.column_count = None;
            } else if let Some(count) = tokens
                .first()
                .and_then(|token| integer(token))
                .filter(|count| *count > 0)
            {
                style.column_count = Some(count.unsigned_abs().min(MAX_COLUMNS));
            }
        }
        "column-width" => {
            if keyword == "auto" {
                style.column_width = None;
            } else if let Some(width) = tokens.first().and_then(|token| length(token, context)) {
                style.column_width = Some(width.max(1.0));
            }
        }
        "columns" => {
            // Width and count, in any order, then a height after a slash; what is left out is
            // `auto` (CSS Multi-column Layout 2, §3.5).
            let (mut count, mut width, mut height) = (None, None, None);
            let mut slash = false;
            for token in &tokens {
                if matches!(token, TokenOrValue::Token(Token::Delim('/'))) && !slash {
                    slash = true;
                } else if slash {
                    if height.is_some() {
                        return;
                    }
                    height = match length(token, context) {
                        Some(value) => Some(value.max(0.0)),
                        None if ident(token).is_some_and(|word| word.eq_ignore_ascii_case("auto")) => None,
                        None => return,
                    };
                } else if let Some(value) = integer(token).filter(|value| *value > 0) {
                    count = Some(value.unsigned_abs().min(MAX_COLUMNS));
                } else if let Some(value) = length(token, context) {
                    width = Some(value.max(1.0));
                } else if ident(token).is_none_or(|word| !word.eq_ignore_ascii_case("auto")) {
                    return;
                }
            }
            style.column_count = count;
            style.column_width = width;
            style.column_height = height;
            style.column_wrap = None;
        }
        "column-rule-width" => {
            if let Some(width) = tokens.first().and_then(|token| rule_width(token, context)) {
                style.column_rule_width = width;
            }
        }
        "column-rule-style" => {
            if let Some(rule) = border_style(keyword) {
                style.column_rule_style = rule;
            }
        }
        "-webkit-text-fill-color" => {
            if let [token] = tokens.as_slice()
                && let Ok(color) = color(token)
            {
                style.text_fill_color = color;
            }
        }
        "column-rule-color" => {
            if let Some(Ok(color)) = tokens.first().map(|token| color(token)) {
                style.column_rule_color = color;
            }
        }
        "column-rule" => {
            let (mut width, mut rule, mut color) = (
                crate::compute::snap_border_width(3.0 * context.zoom),
                BorderStyle::None,
                None,
            );
            for token in &tokens {
                if let Some(value) = rule_width(token, context) {
                    width = value;
                } else if let Some(value) =
                    ident(token).and_then(|word| border_style(&word.to_ascii_lowercase()))
                {
                    rule = value;
                } else if let Ok(value) = self::color(token) {
                    color = value;
                } else {
                    return;
                }
            }
            style.column_rule_width = width;
            style.column_rule_style = rule;
            style.column_rule_color = color;
        }
        "flex-line-count" => {
            if let Some(count) = tokens
                .first()
                .and_then(|token| integer(token))
                .filter(|count| *count > 0)
            {
                style.flex_line_count = u32::try_from(count).unwrap_or(1);
            }
        }
        "column-fill" => {
            style.column_fill = match keyword {
                "auto" => ColumnFill::Auto,
                "balance" | "balance-all" => ColumnFill::Balance,
                _ => style.column_fill,
            }
        }
        "column-height" => {
            if keyword == "auto" {
                style.column_height = None;
            } else if let Some(height) = tokens.first().and_then(|token| length(token, context)) {
                style.column_height = Some(height.max(0.0));
            }
        }
        "column-wrap" => {
            style.column_wrap = match keyword {
                "auto" => None,
                "wrap" => Some(true),
                "nowrap" => Some(false),
                _ => style.column_wrap,
            }
        }
        "column-span" => {
            style.column_span = match keyword {
                "all" => ColumnSpan::All,
                "none" => ColumnSpan::None,
                _ => style.column_span,
            }
        }
        "field-sizing" => {
            style.field_sizing_content = match keyword {
                "content" => true,
                "fixed" => false,
                _ => style.field_sizing_content,
            }
        }
        "frame-sizing" => {
            style.frame_sizing_content_height = match keyword {
                "content-height" | "content-block-size" => true,
                "auto" | "content-width" | "content-inline-size" => false,
                _ => style.frame_sizing_content_height,
            }
        }
        "empty-cells" => {
            style.empty_cells = match keyword {
                "show" => EmptyCells::Show,
                "hide" => EmptyCells::Hide,
                _ => style.empty_cells,
            }
        }
        "contain" => {
            let words: Vec<String> = tokens
                .iter()
                .filter_map(|token| ident(token))
                .map(str::to_ascii_lowercase)
                .collect();
            if words.len() == tokens.len() {
                style.contain_size = words.iter().any(|word| word == "size" || word == "strict");
                style.contain_inline_size = words.iter().any(|word| word == "inline-size");
                style.contain_any = words.iter().any(|word| word != "none");
                style.contain_layout = words
                    .iter()
                    .any(|word| matches!(word.as_str(), "layout" | "paint" | "strict" | "content"));
                style.layout_containment = words
                    .iter()
                    .any(|word| matches!(word.as_str(), "layout" | "strict" | "content"));
                // Paint containment clips the content to the padding box, as `overflow: clip`.
                if words
                    .iter()
                    .any(|word| matches!(word.as_str(), "paint" | "strict" | "content"))
                {
                    for overflow in [&mut style.overflow_x, &mut style.overflow_y] {
                        if *overflow == crate::values::Overflow::Visible {
                            *overflow = crate::values::Overflow::Clip;
                        }
                    }
                }
            }
        }
        "will-change" => {
            // `will-change: auto` or a list of property names.
            style.will_change_contains = tokens.iter().filter_map(|token| ident(token)).any(|name| {
                matches!(
                    name.to_ascii_lowercase().as_str(),
                    "transform" | "translate" | "rotate" | "scale" | "perspective" | "filter" | "contain"
                )
            });
        }
        "margin-trim" => {
            // `none`, `block`, `inline`, or any of the four sides, each at most once; only the
            // block axis matters for block containers.
            let words: Vec<String> = tokens
                .iter()
                .filter_map(|token| ident(token))
                .map(str::to_ascii_lowercase)
                .collect();
            let sides = ["block-start", "inline-start", "block-end", "inline-end"];
            let valid = words.len() == tokens.len()
                && match words.as_slice() {
                    [one] if matches!(one.as_str(), "none" | "block" | "inline") => true,
                    _ => {
                        !words.is_empty()
                            && words.iter().all(|word| sides.contains(&word.as_str()))
                            && words
                                .iter()
                                .enumerate()
                                .all(|(index, word)| !words[..index].contains(word))
                    }
                };
            if valid {
                let has = |side: &str| words.iter().any(|word| word == side || word == "block");
                style.margin_trim_block = (has("block-start"), has("block-end"));
            }
        }
        "content-visibility" => {
            style.content_hidden = match keyword {
                "hidden" => true,
                "visible" | "auto" => false,
                _ => style.content_hidden,
            }
        }
        "overflow-clip-margin" => {
            // `<visual-box> || <length>`, each at most once; Chrome 157 takes negative lengths
            // too (measured), which the spec does not.
            let mut area = None;
            let mut margin = None;
            let valid = tokens.iter().all(|token| {
                let keyword = ident(token).map(str::to_ascii_lowercase);
                let box_area = match keyword.as_deref() {
                    Some("border-box") => Some(BoxArea::BorderBox),
                    Some("padding-box") => Some(BoxArea::PaddingBox),
                    Some("content-box") => Some(BoxArea::ContentBox),
                    _ => None,
                };
                if let Some(box_area) = box_area {
                    area.replace(box_area).is_none()
                } else if let Some(length) = length(token, context) {
                    margin.replace(length).is_none()
                } else {
                    false
                }
            });
            if valid && !tokens.is_empty() {
                style.overflow_clip_margin = (area.unwrap_or(BoxArea::PaddingBox), margin.unwrap_or(0.0));
            }
        }
        "scroll-marker-group" if tokens.len() == 1 => {
            style.scroll_marker_group = match keyword {
                "none" => crate::values::ScrollMarkerGroup::None,
                "before" => crate::values::ScrollMarkerGroup::Before,
                "after" => crate::values::ScrollMarkerGroup::After,
                _ => style.scroll_marker_group,
            };
        }
        "contain-intrinsic-size"
        | "contain-intrinsic-width"
        | "contain-intrinsic-height"
        | "contain-intrinsic-inline-size"
        | "contain-intrinsic-block-size" => {
            // `none`, a length, or `auto <length>` (no size is remembered in print), per axis.
            let mut sizes = Vec::new();
            let mut index = 0;
            while index < tokens.len() {
                let token = tokens[index];
                match ident(token).map(str::to_ascii_lowercase).as_deref() {
                    Some("none") => sizes.push(None),
                    Some("auto") => {
                        index += 1;
                        sizes.push(tokens.get(index).and_then(|token| length(token, context)));
                    }
                    _ => sizes.push(length(token, context)),
                }
                index += 1;
            }
            // The logical longhands as the physical ones of the box's writing mode.
            let vertical = style.is_vertical();
            let (width, height) = match (name.as_str(), sizes.as_slice()) {
                ("contain-intrinsic-inline-size", [width]) if !vertical => {
                    (*width, style.contain_intrinsic_size.1)
                }
                ("contain-intrinsic-block-size", [height]) if !vertical => {
                    (style.contain_intrinsic_size.0, *height)
                }
                ("contain-intrinsic-inline-size", [height]) => (style.contain_intrinsic_size.0, *height),
                ("contain-intrinsic-block-size", [width]) => (*width, style.contain_intrinsic_size.1),
                ("contain-intrinsic-width", [width]) => (*width, style.contain_intrinsic_size.1),
                ("contain-intrinsic-height", [height]) => (style.contain_intrinsic_size.0, *height),
                ("contain-intrinsic-size", [both]) => (*both, *both),
                ("contain-intrinsic-size", [width, height]) => (*width, *height),
                _ => style.contain_intrinsic_size,
            };
            style.contain_intrinsic_size = (width, height);
        }
        "outline-offset" => {
            // Snapped as a border width on either side of zero, as Chrome computes it (measured).
            if let Some(offset) = tokens.first().and_then(|token| length(token, context)) {
                style.outline_offset = crate::compute::snap_border_width(offset.abs()).copysign(offset);
            }
        }
        "text-underline-offset" => {
            // Percentages refer to 1em.
            if keyword == "auto" {
                style.text_underline_offset = None;
            } else if let Some(offset) = tokens.first().and_then(|token| length_percentage(token, context)) {
                style.text_underline_offset = Some(offset.resolve(context.font_size));
            }
        }
        "text-underline-position" => {
            let under = tokens
                .iter()
                .any(|token| ident(token).is_some_and(|ident| ident.eq_ignore_ascii_case("under")));
            style.text_underline_position = if under {
                UnderlinePosition::Under
            } else {
                UnderlinePosition::Auto
            };
        }
        // Chrome takes neither `alternate` nor `inter-character`: the value is inherited then.
        "ruby-position" => match keyword {
            "over" => style.ruby_position = AnnotationSide::Over,
            "under" => style.ruby_position = AnnotationSide::Under,
            _ => {}
        },
        "ruby-align" => {
            style.ruby_align = match keyword {
                "space-around" => RubyAlign::SpaceAround,
                "space-between" => RubyAlign::SpaceBetween,
                "start" => RubyAlign::Start,
                "center" => RubyAlign::Center,
                _ => style.ruby_align,
            }
        }
        "text-spacing-trim" if tokens.len() == 1 => {
            style.text_spacing_trim = match keyword {
                "normal" => TextSpacingTrim::Normal,
                "space-all" => TextSpacingTrim::SpaceAll,
                "space-first" => TextSpacingTrim::SpaceFirst,
                "trim-start" => TextSpacingTrim::TrimStart,
                _ => style.text_spacing_trim,
            }
        }
        "text-fit" => style.text_fit = text_fit(&tokens).unwrap_or(style.text_fit),
        "hanging-punctuation" => {
            let words: Option<Vec<&str>> = tokens.iter().map(|token| ident(token)).collect();
            if let Some(value) = words.and_then(crate::values::HangingPunctuation::parse) {
                style.hanging_punctuation = value;
            }
        }
        "text-autospace" if tokens.len() == 1 => {
            style.text_autospace = match keyword {
                "normal" => true,
                "no-autospace" => false,
                _ => style.text_autospace,
            }
        }
        // `-webkit-writing-mode` is an alias that takes the three CSS values alone (the
        // cascade drops it with others).
        "-webkit-writing-mode" if tokens.len() == 1 => {
            style.writing_mode = match keyword {
                "horizontal-tb" => WritingMode::HorizontalTb,
                "vertical-rl" => WritingMode::VerticalRl,
                "vertical-lr" => WritingMode::VerticalLr,
                _ => style.writing_mode,
            }
        }
        // The SVG 1.1 values stand for the CSS ones, as in browsers.
        "writing-mode" if tokens.len() == 1 => {
            style.writing_mode = match keyword {
                "horizontal-tb" | "lr" | "lr-tb" | "rl" | "rl-tb" => WritingMode::HorizontalTb,
                "vertical-rl" | "tb" | "tb-rl" => WritingMode::VerticalRl,
                "vertical-lr" => WritingMode::VerticalLr,
                "sideways-rl" => WritingMode::SidewaysRl,
                "sideways-lr" => WritingMode::SidewaysLr,
                _ => style.writing_mode,
            }
        }
        "text-orientation" if tokens.len() == 1 => {
            style.text_orientation = match keyword {
                "mixed" => TextOrientation::Mixed,
                "upright" => TextOrientation::Upright,
                "sideways" | "sideways-right" => TextOrientation::Sideways,
                _ => style.text_orientation,
            }
        }
        "text-combine-upright" if tokens.len() == 1 => {
            style.text_combine_upright = match keyword {
                "all" => true,
                "none" => false,
                _ => style.text_combine_upright,
            }
        }
        "text-box-trim" => {
            if let Some(trim) = text_box_trim(keyword).filter(|_| tokens.len() == 1) {
                style.text_box_trim = trim;
            }
        }
        "text-box-edge" => {
            let words: Option<Vec<&str>> = tokens.iter().map(|token| ident(token)).collect();
            if let Some(edge) = words.and_then(|words| text_box_edge(&words)) {
                style.text_box_edge = edge;
            }
        }
        // `normal`, or a trim and edges in either order: edges alone trim both sides, a trim
        // alone keeps the edges of the text.
        "text-box" => {
            let Some(words) = tokens
                .iter()
                .map(|token| ident(token))
                .collect::<Option<Vec<&str>>>()
            else {
                return;
            };
            if keyword == "normal" && words.len() == 1 {
                style.text_box_trim = TextBoxTrim::default();
                style.text_box_edge = (TextEdge::Text, TextEdge::Text);
                return;
            }
            let trim_at = words
                .iter()
                .position(|word| text_box_trim(&word.to_ascii_lowercase()).is_some());
            let edges: Vec<&str> = words
                .iter()
                .enumerate()
                .filter(|(index, _)| Some(*index) != trim_at)
                .map(|(_, word)| *word)
                .collect();
            // The edges must be next to each other.
            if trim_at.is_some_and(|at| at > 0 && at + 1 < words.len()) {
                return;
            }
            let edge = if edges.is_empty() {
                Some((TextEdge::Text, TextEdge::Text))
            } else {
                text_box_edge(&edges)
            };
            let trim = match trim_at {
                Some(at) => text_box_trim(&words[at].to_ascii_lowercase()),
                None => Some(TextBoxTrim {
                    start: true,
                    end: true,
                }),
            };
            if let (Some(trim), Some(edge)) = (trim, edge) {
                style.text_box_trim = trim;
                style.text_box_edge = edge;
            }
        }
        "-fagus-repeat" => {
            style.repeat = match keyword {
                "auto" => Repeat::Auto,
                "at-break" => Repeat::AtBreak,
                _ => style.repeat,
            }
        }
        "-fagus-layer" => {
            if let Some(layer) = layer(&tokens) {
                style.layer = layer.map(std::sync::Arc::new);
            }
        }
        "object-fit" => {
            style.object_fit = match keyword {
                "fill" => ObjectFit::Fill,
                "contain" => ObjectFit::Contain,
                "cover" => ObjectFit::Cover,
                "none" => ObjectFit::None,
                "scale-down" => ObjectFit::ScaleDown,
                _ => style.object_fit,
            }
        }
        "object-position" => {
            if let Some(position) = position(&tokens, context) {
                style.object_position = position;
            }
        }
        // A value that combines `element()` with other values is invalid (CSS Generated Content
        // for Paged Media 3), here at computed-value time, after `var()`: `content` is `normal`.
        "content" if fagus_css::paged::misused_element(&tokens) => style.content = Content::Normal,
        "content" => style.content = content(&tokens),
        "counter-reset" => style.counter_reset = counters(&tokens, 0),
        "counter-increment" => style.counter_increment = counters(&tokens, 1),
        "counter-set" => style.counter_set = counters(&tokens, 0),
        "string-set" => style.string_set = string_set(&tokens),
        "bookmark-level" => {
            // `none` or a positive integer; another value is invalid and leaves the level.
            if keyword == "none" {
                style.bookmark_level = BookmarkLevel::None;
            } else if let Some(level) = tokens
                .first()
                .and_then(|token| integer(token))
                .and_then(|level| u8::try_from(level).ok())
                .filter(|level| *level > 0)
            {
                style.bookmark_level = BookmarkLevel::Level(level);
            }
        }
        "bookmark-label" => {
            // `content(text)`, the initial value, is the element's text.
            let items = content_items(&tokens);
            style.bookmark_label = (items != [ContentItem::Content(ContentTarget::Text)]).then_some(items);
        }
        "bookmark-state" => style.bookmark_closed = keyword == "closed",
        "font-variant" => {
            // All longhands, capitals too; `none` turns ligatures off.
            match keyword {
                "normal" if tokens.len() == 1 => style.font_variant = FontVariant::default(),
                "none" if tokens.len() == 1 => {
                    style.font_variant = FontVariant {
                        ligatures: [Ligatures::Off; 4],
                        ..FontVariant::default()
                    };
                }
                _ => {
                    if let Some(variant) = font_variant(&tokens, |_| true) {
                        style.font_variant = variant;
                    }
                }
            }
        }
        "font-variant-ligatures" => match keyword {
            "normal" if tokens.len() == 1 => style.font_variant.ligatures = [Ligatures::Normal; 4],
            "none" if tokens.len() == 1 => style.font_variant.ligatures = [Ligatures::Off; 4],
            _ => {
                if let Some(variant) =
                    font_variant(&tokens, |part| matches!(part, VariantPart::Ligatures(..)))
                {
                    style.font_variant.ligatures = variant.ligatures;
                }
            }
        },
        "font-variant-numeric" => match keyword {
            "normal" if tokens.len() == 1 => style.font_variant.numeric = 0,
            _ => {
                if let Some(variant) = font_variant(&tokens, |part| matches!(part, VariantPart::Numeric(_))) {
                    style.font_variant.numeric = variant.numeric;
                }
            }
        },
        "font-variant-east-asian" => match keyword {
            "normal" if tokens.len() == 1 => style.font_variant.east_asian = 0,
            _ => {
                if let Some(variant) = font_variant(&tokens, |part| matches!(part, VariantPart::EastAsian(_)))
                {
                    style.font_variant.east_asian = variant.east_asian;
                }
            }
        },
        "font-variant-position" => {
            style.font_variant.position = match keyword {
                "normal" => FontVariantPosition::Normal,
                "sub" => FontVariantPosition::Sub,
                "super" => FontVariantPosition::Super,
                _ => style.font_variant.position,
            }
        }
        "font-variant-alternates" => {
            // Only `historical-forms` needs no `@font-feature-values`.
            style.font_variant.historical_forms = tokens.iter().any(|token| {
                ident(token).is_some_and(|ident| ident.eq_ignore_ascii_case("historical-forms"))
            });
        }
        "isolation" => {
            style.isolation = match keyword {
                "auto" => crate::values::Isolation::Auto,
                "isolate" => crate::values::Isolation::Isolate,
                _ => style.isolation,
            }
        }
        "background-blend-mode" => {
            // A blend mode per layer; an invalid one drops the declaration.
            let mut modes = Vec::new();
            for group in tokens.split(|token| matches!(token, TokenOrValue::Token(Token::Comma))) {
                match group {
                    [mode] => match ident(mode).and_then(crate::values::BlendMode::of) {
                        Some(mode) => modes.push(mode),
                        None => return,
                    },
                    _ => return,
                }
            }
            style.background_blend_mode = modes.into();
        }
        "image-orientation" => {
            style.image_orientation = match keyword {
                "from-image" => crate::values::ImageOrientation::FromImage,
                "none" => crate::values::ImageOrientation::None,
                _ => style.image_orientation,
            }
        }
        "border-shape" => {
            if keyword == "none" && tokens.len() == 1 {
                style.border_shape = None;
            } else if let Some(shape) = border_shape(&tokens, context) {
                style.border_shape = Some(std::sync::Arc::new(shape));
            }
        }
        _ if name.starts_with("corner-") && name.ends_with("-shape") => {
            let Some(shapes) = tokens
                .iter()
                .map(|token| corner_shape(token))
                .collect::<Option<Vec<f32>>>()
            else {
                return;
            };
            // Logical corners in horizontal left-to-right text.
            let corners: &[usize] = match name.as_str() {
                "corner-shape" => &[0, 1, 2, 3],
                "corner-top-left-shape" | "corner-start-start-shape" => &[0],
                "corner-top-right-shape" | "corner-start-end-shape" => &[1],
                "corner-bottom-right-shape" | "corner-end-end-shape" => &[2],
                "corner-bottom-left-shape" | "corner-end-start-shape" => &[3],
                "corner-top-shape" => &[0, 1],
                "corner-right-shape" => &[1, 2],
                "corner-bottom-shape" => &[3, 2],
                "corner-left-shape" => &[0, 3],
                _ => return,
            };
            // One to four values, as `border-radius` gives its corners; two for a side.
            let pick = |index: usize| -> Option<f32> {
                let at = match (corners.len(), shapes.len()) {
                    (_, 1) => 0,
                    (2, 2) | (4, 4) => index,
                    (4, 2) => index % 2,
                    (4, 3) => [0, 1, 2, 1][index],
                    _ => return None,
                };
                shapes.get(at).copied()
            };
            let mut shape = style.corner_shape;
            for (index, corner) in corners.iter().enumerate() {
                let Some(value) = pick(index) else {
                    return;
                };
                shape[*corner] = value;
            }
            style.corner_shape = shape;
        }
        "-webkit-line-clamp" => {
            style.webkit_line_clamp = match (keyword, tokens.first().and_then(|token| integer(token))) {
                ("none", _) => None,
                (_, Some(lines)) if lines > 0 => Some(lines.unsigned_abs()),
                _ => return,
            };
        }
        "white-space-collapse" => {
            style.white_space.collapse = match keyword {
                "collapse" => WhiteSpaceCollapse::Collapse,
                "preserve" => WhiteSpaceCollapse::Preserve,
                "preserve-breaks" => WhiteSpaceCollapse::PreserveBreaks,
                "break-spaces" => WhiteSpaceCollapse::BreakSpaces,
                _ => style.white_space.collapse,
            }
        }
        "text-wrap-mode" => match keyword {
            "wrap" => style.white_space.nowrap = false,
            "nowrap" => style.white_space.nowrap = true,
            _ => {}
        },
        "text-wrap-style" => {
            if let Some(wrap) = text_wrap_style(keyword) {
                style.text_wrap_style = wrap;
            }
        }
        "text-wrap" => {
            // A mode and a style in either order, each at most once; what is left out is reset.
            let (mut nowrap, mut wrap) = (None, None);
            for token in &tokens {
                let keyword = ident(token).map(str::to_ascii_lowercase);
                match keyword.as_deref() {
                    Some("wrap") if nowrap.is_none() => nowrap = Some(false),
                    Some("nowrap") if nowrap.is_none() => nowrap = Some(true),
                    Some(keyword) if wrap.is_none() && text_wrap_style(keyword).is_some() => {
                        wrap = text_wrap_style(keyword);
                    }
                    _ => return,
                }
            }
            if !tokens.is_empty() {
                style.white_space.nowrap = nowrap.unwrap_or(false);
                style.text_wrap_style = wrap.unwrap_or(TextWrapStyle::Auto);
            }
        }
        "font-size-adjust" => {
            // `none`, or a metric (the x-height without one) and an aspect value or `from-font`.
            if keyword == "none" && tokens.len() == 1 {
                style.font_size_adjust = FontSizeAdjust::None;
                return;
            }
            let (metric, value) = match tokens.as_slice() {
                [value] => (FontMetric::ExHeight, value),
                [metric, value] => {
                    let metric = match ident(metric).map(str::to_ascii_lowercase).as_deref() {
                        Some("ex-height") => FontMetric::ExHeight,
                        Some("cap-height") => FontMetric::CapHeight,
                        Some("ch-width") => FontMetric::ChWidth,
                        Some("ic-width") => FontMetric::IcWidth,
                        Some("ic-height") => FontMetric::IcHeight,
                        _ => return,
                    };
                    (metric, value)
                }
                _ => return,
            };
            // The aspect value may be a math function of numbers (`calc(1462 / 2048)`).
            let aspect = number(value).or_else(|| match value {
                TokenOrValue::Function(function) => crate::math::evaluate_function(function, None)
                    .and_then(crate::math::Quantity::plain)
                    .filter(crate::math::Quantity::is_number)
                    .map(|quantity| quantity.value as f32),
                _ => None,
            });
            if ident(value).is_some_and(|value| value.eq_ignore_ascii_case("from-font")) {
                style.font_size_adjust = FontSizeAdjust::FromFont(metric, None);
            } else if let Some(aspect) = aspect.filter(|aspect| aspect.is_finite() && *aspect >= 0.0) {
                style.font_size_adjust = FontSizeAdjust::Aspect(metric, aspect);
            }
        }
        "font-kerning" => {
            style.font_kerning = match keyword {
                "auto" => FontKerning::Auto,
                "normal" => FontKerning::Normal,
                "none" => FontKerning::None,
                _ => style.font_kerning,
            }
        }
        "font-synthesis" => {
            if keyword == "none" && tokens.len() == 1 {
                style.font_synthesis = FontSynthesis {
                    weight: false,
                    style: false,
                    small_caps: false,
                };
            } else {
                let named = |name: &str| {
                    tokens
                        .iter()
                        .any(|token| ident(token).is_some_and(|ident| ident.eq_ignore_ascii_case(name)))
                };
                let known = ["weight", "style", "small-caps", "position"];
                if tokens.iter().all(|token| {
                    ident(token)
                        .is_some_and(|ident| known.iter().any(|name| ident.eq_ignore_ascii_case(name)))
                }) {
                    style.font_synthesis = FontSynthesis {
                        weight: named("weight"),
                        style: named("style"),
                        small_caps: named("small-caps"),
                    };
                }
            }
        }
        "font-synthesis-weight" | "font-synthesis-style" | "font-synthesis-small-caps" => {
            let allowed = match keyword {
                "auto" => true,
                "none" => false,
                _ => return,
            };
            match name.as_str() {
                "font-synthesis-weight" => style.font_synthesis.weight = allowed,
                "font-synthesis-style" => style.font_synthesis.style = allowed,
                _ => style.font_synthesis.small_caps = allowed,
            }
        }
        "font-variation-settings" => {
            // `normal`, or axes with values; an invalid value leaves the property as it was.
            let mut settings = Vec::new();
            for group in tokens.split(|token| matches!(token, TokenOrValue::Token(Token::Comma))) {
                match group {
                    [tag, value] => {
                        let Some(tag) = string(tag).and_then(|tag| <[u8; 4]>::try_from(tag.as_bytes()).ok())
                        else {
                            return;
                        };
                        let Some(value) = number(value) else { return };
                        settings.push((tag, value));
                    }
                    [keyword]
                        if ident(keyword).is_some_and(|keyword| keyword.eq_ignore_ascii_case("normal")) => {}
                    _ => return,
                }
            }
            style.font_variation_settings = settings.into();
        }
        "font-optical-sizing" => {
            style.font_optical_sizing = match keyword {
                "auto" => OpticalSizing::Auto,
                "none" => OpticalSizing::None,
                _ => style.font_optical_sizing,
            }
        }
        // An invalid value leaves the property as it was.
        "font-feature-settings" => {
            if let Some(features) = feature_settings(&tokens) {
                style.font_feature_settings = features.into();
            }
        }
        _ => {}
    }
}

/// A value of `text-box-trim`.
fn text_box_trim(keyword: &str) -> Option<TextBoxTrim> {
    let (start, end) = match keyword {
        "none" => (false, false),
        "trim-start" => (true, false),
        "trim-end" => (false, true),
        "trim-both" => (true, true),
        _ => return None,
    };
    Some(TextBoxTrim { start, end })
}

/// A value of `text-box-edge`: `auto`, or the edge over the text and the one under it. One
/// edge stands for both, so it must suit both (`text`), as Chrome takes it.
fn text_box_edge(words: &[&str]) -> Option<(TextEdge, TextEdge)> {
    let over = |word: &str| match word.to_ascii_lowercase().as_str() {
        "text" | "auto" => Some(TextEdge::Text),
        "cap" => Some(TextEdge::Cap),
        "ex" => Some(TextEdge::Ex),
        _ => None,
    };
    let under = |word: &str| match word.to_ascii_lowercase().as_str() {
        "text" => Some(TextEdge::Text),
        "alphabetic" => Some(TextEdge::Alphabetic),
        _ => None,
    };
    match words {
        [word] if word.eq_ignore_ascii_case("auto") || word.eq_ignore_ascii_case("text") => {
            Some((TextEdge::Text, TextEdge::Text))
        }
        [first, second] if !first.eq_ignore_ascii_case("auto") => Some((over(first)?, under(second)?)),
        _ => None,
    }
}

/// `text-fit` as Chrome parses it (`ConsumeTextFit`): `none`, `grow` or `shrink`, then
/// optionally `consistent`, `per-line` or `per-line-all`, then optionally a percentage not below
/// zero. `None` where the value is not one.
pub(crate) fn text_fit(tokens: &[&TokenOrValue<'static>]) -> Option<crate::values::TextFit> {
    use crate::values::{TextFit, TextFitKind, TextFitTarget};
    let mut rest = tokens.iter();
    let kind = match ident(rest.next()?)?.to_ascii_lowercase().as_str() {
        "none" => TextFitKind::None,
        "grow" => TextFitKind::Grow,
        "shrink" => TextFitKind::Shrink,
        _ => return None,
    };
    let mut fit = TextFit {
        kind,
        ..TextFit::default()
    };
    let mut next = rest.next();
    if let Some(target) = next.and_then(|token| ident(token)) {
        fit.target = match target.to_ascii_lowercase().as_str() {
            "consistent" => TextFitTarget::Consistent,
            "per-line" => TextFitTarget::PerLine,
            "per-line-all" => TextFitTarget::PerLineAll,
            _ => return None,
        };
        next = rest.next();
    }
    if let Some(token) = next {
        match token {
            TokenOrValue::Token(Token::Percentage { unit_value, .. })
                if unit_value.is_finite() && *unit_value >= 0.0 =>
            {
                fit.limit = Some(*unit_value);
            }
            _ => return None,
        }
    }
    rest.next().is_none().then_some(fit)
}
/// The features of the `font-feature-settings` descriptor of an `@font-face` rule, as for the
/// property; `None` for an invalid value.
pub fn font_face_features(tokens: &TokenList<'static>) -> Option<Vec<([u8; 4], u32)>> {
    feature_settings(&significant(tokens))
}

/// The features of a value of `font-feature-settings` (the property or the `@font-face`
/// descriptor), without white space: `normal`, or features with values (`on` and `off` too);
/// `None` for an invalid value.
fn feature_settings(tokens: &[&TokenOrValue<'static>]) -> Option<Vec<([u8; 4], u32)>> {
    let mut features = Vec::new();
    for group in tokens.split(|token| matches!(token, TokenOrValue::Token(Token::Comma))) {
        let Some(tag) = group.first().and_then(|token| string(token)) else {
            if let [keyword] = group
                && ident(keyword).is_some_and(|keyword| keyword.eq_ignore_ascii_case("normal"))
            {
                continue;
            }
            return None;
        };
        let tag = <[u8; 4]>::try_from(tag.as_bytes()).ok()?;
        if !tag.iter().all(|byte| (0x20..=0x7e).contains(byte)) {
            return None;
        }
        let value = match group.get(1..) {
            Some([]) | None => 1,
            Some([token]) => match (
                integer(token),
                ident(token).map(str::to_ascii_lowercase).as_deref(),
            ) {
                (Some(value), _) if value >= 0 => value.unsigned_abs(),
                (_, Some("on")) => 1,
                (_, Some("off")) => 0,
                _ => return None,
            },
            Some(_) => return None,
        };
        features.push((tag, value));
    }
    Some(features)
}

/// The tokens without whitespace and comments.
pub(crate) fn significant<'t>(tokens: &'t TokenList<'static>) -> Vec<&'t TokenOrValue<'static>> {
    tokens
        .0
        .iter()
        .filter(|token| {
            !matches!(
                token,
                TokenOrValue::Token(Token::WhiteSpace(_) | Token::Comment(_))
            )
        })
        .collect()
}

/// The value of `-fagus-layer`: `none`, or a name and where the layer shows; `None` where the
/// value is invalid.
#[allow(clippy::option_option, reason = "invalid, `none` or a layer")]
fn layer(tokens: &[&TokenOrValue<'static>]) -> Option<Option<Layer>> {
    match tokens {
        [only] if ident(only).is_some_and(|keyword| keyword.eq_ignore_ascii_case("none")) => Some(None),
        [name, rest @ ..] if rest.len() <= 1 => {
            let name = string(name)?.trim();
            if name.is_empty() {
                return None;
            }
            let shown = match rest
                .first()
                .map(|token| ident(token).map(str::to_ascii_lowercase))
            {
                None => LayerShown::Always,
                Some(Some(keyword)) if keyword == "screen" => LayerShown::Screen,
                Some(Some(keyword)) if keyword == "print" => LayerShown::Print,
                Some(_) => return None,
            };
            Some(Some(Layer {
                name: std::sync::Arc::from(name),
                shown,
            }))
        }
        _ => None,
    }
}

fn ident<'t>(token: &'t TokenOrValue<'static>) -> Option<&'t str> {
    match token {
        TokenOrValue::Token(Token::Ident(ident)) => Some(ident.as_ref()),
        _ => None,
    }
}

fn string<'t>(token: &'t TokenOrValue<'static>) -> Option<&'t str> {
    match token {
        TokenOrValue::Token(Token::String(string)) => Some(string.as_ref()),
        _ => None,
    }
}

/// The edges of `rect(top, right, bottom, left)` in `clip`: lengths or `auto`, separated by
/// commas or (the old syntax) by spaces alone.
fn clip_rect(arguments: &TokenList<'static>, context: &Context) -> Option<[Option<f32>; 4]> {
    let mut rect = [None; 4];
    for (edge, token) in rect.iter_mut().zip(clip_edges(arguments)?) {
        if ident(token).is_none() {
            *edge = Some(length(token, context)?);
        }
    }
    Some(rect)
}

/// The four edges of the arguments of `rect()` in `clip`, each `auto`, a length or `0`.
fn clip_edges<'t, 'i>(arguments: &'t TokenList<'i>) -> Option<Vec<&'t TokenOrValue<'i>>> {
    let tokens: Vec<_> = arguments
        .0
        .iter()
        .filter(|token| {
            !matches!(
                token,
                TokenOrValue::Token(Token::WhiteSpace(_) | Token::Comment(_))
            )
        })
        .collect();
    let comma = |token: &&TokenOrValue<'_>| matches!(token, TokenOrValue::Token(Token::Comma));
    let commas = tokens.iter().filter(|token| comma(token)).count();
    let edges: Vec<_> = tokens.into_iter().filter(|token| !comma(token)).collect();
    let edge = |token: &&TokenOrValue<'_>| match token {
        TokenOrValue::Token(Token::Ident(name)) => name.eq_ignore_ascii_case("auto"),
        TokenOrValue::Token(Token::Number { value, .. }) => *value == 0.0,
        TokenOrValue::Length(_) => true,
        _ => false,
    };
    (edges.len() == 4 && matches!(commas, 0 | 3) && edges.iter().all(edge)).then_some(edges)
}

/// Whether a value of `clip` is valid: `auto` or `rect()` with four edges.
pub(crate) fn is_clip(tokens: &TokenList<'_>) -> bool {
    let tokens: Vec<_> = tokens
        .0
        .iter()
        .filter(|token| {
            !matches!(
                token,
                TokenOrValue::Token(Token::WhiteSpace(_) | Token::Comment(_))
            )
        })
        .collect();
    match tokens.as_slice() {
        [TokenOrValue::Token(Token::Ident(name))] => name.eq_ignore_ascii_case("auto"),
        [TokenOrValue::Function(function)] => {
            function.name.0.eq_ignore_ascii_case("rect") && clip_edges(&function.arguments).is_some()
        }
        _ => false,
    }
}

/// A length in pixels (zero may be written without a unit).
fn length(token: &TokenOrValue<'static>, context: &Context) -> Option<f32> {
    match token {
        TokenOrValue::Length(value) => Some(context.length_value(value)).filter(|value| value.is_finite()),
        TokenOrValue::Token(Token::Number { value, .. }) if *value == 0.0 => Some(0.0),
        _ => None,
    }
}

/// A length or percentage, a math function of them among them (`calc(50% - 10px)`).
fn length_percentage(token: &TokenOrValue<'static>, context: &Context) -> Option<LengthPercentage> {
    match token {
        TokenOrValue::Token(Token::Percentage { unit_value, .. }) if unit_value.is_finite() => {
            Some(LengthPercentage::Percent(*unit_value))
        }
        TokenOrValue::Function(function)
            if ["calc", "min", "max", "clamp"]
                .iter()
                .any(|name| function.name.0.eq_ignore_ascii_case(name)) =>
        {
            use lightningcss::properties::Property;
            use lightningcss::properties::custom::{CustomProperty, CustomPropertyName};
            use lightningcss::traits::Parse;
            let css = Property::Custom(CustomProperty {
                name: CustomPropertyName::Unknown(lightningcss::values::ident::Ident("x".into())),
                value: TokenList(vec![token.clone()]),
            })
            .value_to_css_string(lightningcss::stylesheet::PrinterOptions::default())
            .ok()?;
            let parsed = fagus_css::guarded(|| {
                lightningcss::values::length::LengthPercentage::parse_string(&css)
                    .ok()
                    .map(|value| context.length_percentage(&value))
            })??;
            Some(parsed)
        }
        _ => length(token, context).map(LengthPercentage::Px),
    }
}

/// A position as in `background-position` and `object-position` (CSS Backgrounds 3, §3.6):
/// one or two keywords or lengths, or keywords with offsets, as the offsets from the left and
/// top edges.
fn position(
    tokens: &[&TokenOrValue<'static>],
    context: &Context,
) -> Option<(LengthPercentage, LengthPercentage)> {
    #[derive(Clone, Copy, PartialEq)]
    enum Axis {
        X,
        Y,
        Either,
    }
    // A keyword: its axis, and whether it measures from the far edge.
    let keyword = |token: &TokenOrValue<'static>| -> Option<(Axis, f32)> {
        match ident(token)?.to_ascii_lowercase().as_str() {
            "left" => Some((Axis::X, 0.0)),
            "right" => Some((Axis::X, 1.0)),
            "top" => Some((Axis::Y, 0.0)),
            "bottom" => Some((Axis::Y, 1.0)),
            "center" => Some((Axis::Either, 0.5)),
            _ => None,
        }
    };
    // An offset from the edge a keyword names.
    let from = |edge: f32, offset: LengthPercentage| {
        if edge == 0.0 {
            offset
        } else {
            LengthPercentage::from_node(CalcNode::Sum(
                Box::new(CalcNode::Percent(1.0)),
                Box::new(CalcNode::Product(-1.0, Box::new(offset.to_node()))),
            ))
        }
    };
    match tokens {
        [one] => match keyword(one) {
            Some((Axis::Y, edge)) => Some((LengthPercentage::Percent(0.5), LengthPercentage::Percent(edge))),
            Some((_, edge)) => Some((LengthPercentage::Percent(edge), LengthPercentage::Percent(0.5))),
            None => Some((length_percentage(one, context)?, LengthPercentage::Percent(0.5))),
        },
        [first, second] => {
            let value = |token: &TokenOrValue<'static>| {
                keyword(token)
                    .map(|(_, edge)| LengthPercentage::Percent(edge))
                    .or_else(|| length_percentage(token, context))
            };
            let swapped =
                matches!(keyword(first), Some((Axis::Y, _))) || matches!(keyword(second), Some((Axis::X, _)));
            if swapped {
                Some((value(second)?, value(first)?))
            } else {
                Some((value(first)?, value(second)?))
            }
        }
        [first, first_offset, second, second_offset] => {
            let (first_axis, first_edge) = keyword(first)?;
            let (second_axis, second_edge) = keyword(second)?;
            let first = from(first_edge, length_percentage(first_offset, context)?);
            let second = from(second_edge, length_percentage(second_offset, context)?);
            match (first_axis, second_axis) {
                (Axis::Y, _) | (_, Axis::X) => Some((second, first)),
                _ => Some((first, second)),
            }
        }
        _ => None,
    }
}

/// A color, or `currentColor` (`Ok(None)`). Named colors come as identifiers.
fn color(token: &TokenOrValue<'static>) -> Result<Option<crate::values::Rgba>, ()> {
    use lightningcss::traits::Parse;
    if let TokenOrValue::Color(color) = token {
        return Ok(crate::compute::color(color));
    }
    let word = ident(token).ok_or(())?;
    if word.eq_ignore_ascii_case("currentcolor") {
        return Ok(None);
    }
    let parsed = lightningcss::values::color::CssColor::parse_string(word).map_err(|_| ())?;
    Ok(crate::compute::color(&parsed))
}

/// The width of a line: a length or `thin`, `medium` or `thick`.
fn rule_width(token: &TokenOrValue<'static>, context: &Context) -> Option<f32> {
    let width = match ident(token).map(str::to_ascii_lowercase).as_deref() {
        Some("thin") => context.zoom,
        Some("medium") => 3.0 * context.zoom,
        Some("thick") => 5.0 * context.zoom,
        _ => length(token, context)?,
    };
    Some(crate::compute::snap_border_width(width))
}

fn border_style(keyword: &str) -> Option<BorderStyle> {
    Some(match keyword {
        "none" => BorderStyle::None,
        "hidden" => BorderStyle::Hidden,
        "solid" => BorderStyle::Solid,
        "dashed" => BorderStyle::Dashed,
        "dotted" => BorderStyle::Dotted,
        "double" => BorderStyle::Double,
        "groove" => BorderStyle::Groove,
        "ridge" => BorderStyle::Ridge,
        "inset" => BorderStyle::Inset,
        "outset" => BorderStyle::Outset,
        _ => return None,
    })
}

/// A part of `font-variant` that a keyword sets: which kind of ligature, which bit of the numeric
/// or East Asian features (with the group of bits that exclude each other), capitals, position or
/// historical forms.
#[derive(Clone, Copy, PartialEq, Eq)]
enum VariantPart {
    Ligatures(usize, Ligatures),
    Numeric(usize),
    EastAsian(usize),
    Caps(FontVariantCaps),
    Position(FontVariantPosition),
    HistoricalForms,
}

impl VariantPart {
    fn of(keyword: &str) -> Option<Self> {
        let keyword = keyword.to_ascii_lowercase();
        let ligature = [
            "common-ligatures",
            "discretionary-ligatures",
            "historical-ligatures",
            "contextual",
        ];
        if let Some(index) = ligature.iter().position(|name| *name == keyword) {
            return Some(Self::Ligatures(index, Ligatures::On));
        }
        if let Some(index) = keyword
            .strip_prefix("no-")
            .and_then(|name| ligature.iter().position(|ligature| *ligature == name))
        {
            return Some(Self::Ligatures(index, Ligatures::Off));
        }
        if let Some(bit) = NUMERIC_FEATURES.iter().position(|(name, _)| *name == keyword) {
            return Some(Self::Numeric(bit));
        }
        if let Some(bit) = EAST_ASIAN_FEATURES.iter().position(|(name, _)| *name == keyword) {
            return Some(Self::EastAsian(bit));
        }
        Some(match keyword.as_str() {
            "small-caps" => Self::Caps(FontVariantCaps::SmallCaps),
            "all-small-caps" => Self::Caps(FontVariantCaps::AllSmallCaps),
            "petite-caps" => Self::Caps(FontVariantCaps::PetiteCaps),
            "all-petite-caps" => Self::Caps(FontVariantCaps::AllPetiteCaps),
            "unicase" => Self::Caps(FontVariantCaps::Unicase),
            "titling-caps" => Self::Caps(FontVariantCaps::TitlingCaps),
            "sub" => Self::Position(FontVariantPosition::Sub),
            "super" => Self::Position(FontVariantPosition::Super),
            "historical-forms" => Self::HistoricalForms,
            _ => return None,
        })
    }

    /// The group of keywords of which a value takes at most one.
    fn group(self) -> (u8, usize) {
        match self {
            Self::Ligatures(index, _) => (0, index),
            // Figures, spacing and fractions take one of two keywords each.
            Self::Numeric(bit) => (1, if bit < 6 { bit / 2 } else { bit }),
            // Variants take one of six, widths one of two.
            Self::EastAsian(bit) => (
                2,
                if bit < 6 {
                    0
                } else if bit < 8 {
                    1
                } else {
                    2
                },
            ),
            Self::Caps(_) => (3, 0),
            Self::Position(_) => (4, 0),
            Self::HistoricalForms => (5, 0),
        }
    }
}

/// A `font-variant` value of keywords that `allowed` accepts, each group at most once.
fn font_variant(
    tokens: &[&TokenOrValue<'static>],
    allowed: impl Fn(VariantPart) -> bool,
) -> Option<FontVariant> {
    let mut variant = FontVariant::default();
    let mut groups = Vec::new();
    for token in tokens {
        let part = VariantPart::of(ident(token)?).filter(|part| allowed(*part))?;
        if groups.contains(&part.group()) {
            return None;
        }
        groups.push(part.group());
        match part {
            VariantPart::Ligatures(index, value) => variant.ligatures[index] = value,
            VariantPart::Numeric(bit) => variant.numeric |= 1 << bit,
            VariantPart::EastAsian(bit) => variant.east_asian |= 1 << bit,
            VariantPart::Caps(caps) => variant.caps = caps,
            VariantPart::Position(position) => variant.position = position,
            VariantPart::HistoricalForms => variant.historical_forms = true,
        }
    }
    (!tokens.is_empty()).then_some(variant)
}

fn text_wrap_style(keyword: &str) -> Option<TextWrapStyle> {
    Some(match keyword {
        "auto" => TextWrapStyle::Auto,
        "balance" => TextWrapStyle::Balance,
        "pretty" => TextWrapStyle::Pretty,
        "stable" => TextWrapStyle::Stable,
        _ => return None,
    })
}

/// Whether `tokens` are a valid value of multi-column property `name` (`column-count`,
/// `column-width`, `columns` or `column-rule`); `None` for other properties. A count is a positive integer, a
/// width a non-negative length, and `columns` has at most one of each before an optional slash
/// and height (CSS Multi-column Layout 2, §3).
pub(crate) fn is_columns_value(name: &str, tokens: &TokenList<'static>) -> Option<bool> {
    #[derive(PartialEq)]
    enum Part {
        Auto,
        Count,
        Width,
    }
    fn part(token: &TokenOrValue<'static>) -> Option<Part> {
        match token {
            TokenOrValue::Token(Token::Ident(word)) if word.eq_ignore_ascii_case("auto") => Some(Part::Auto),
            TokenOrValue::Length(length) if length.to_unit_value().0 >= 0.0 => Some(Part::Width),
            TokenOrValue::Token(Token::Number { value, .. }) if *value == 0.0 => Some(Part::Width),
            // A `calc()` may be either; Fagus resolves it where it applies.
            TokenOrValue::Function(_) => Some(Part::Auto),
            token => integer(token).filter(|count| *count > 0).map(|_| Part::Count),
        }
    }
    let tokens = significant(tokens);
    Some(match name {
        "column-count" => {
            matches!(tokens.as_slice(), [token] if matches!(part(token), Some(Part::Auto | Part::Count)))
        }
        "column-width" => {
            matches!(tokens.as_slice(), [token] if matches!(part(token), Some(Part::Auto | Part::Width)))
        }
        "columns" => {
            let slash = tokens
                .iter()
                .position(|token| matches!(token, TokenOrValue::Token(Token::Delim('/'))));
            let (before, after) = tokens.split_at(slash.unwrap_or(tokens.len()));
            let parts: Option<Vec<Part>> = before.iter().map(|token| part(token)).collect();
            let before_valid = parts.is_some_and(|parts| {
                (1..=2).contains(&parts.len())
                    && parts.iter().filter(|part| **part == Part::Count).count() <= 1
                    && parts.iter().filter(|part| **part == Part::Width).count() <= 1
            });
            let after_valid = match after {
                [] => true,
                [_, height] => matches!(part(height), Some(Part::Auto | Part::Width)),
                _ => false,
            };
            before_valid && after_valid
        }
        "column-rule" => {
            // A width, a style and a color, each at most once, in any order.
            let kind = |token: &TokenOrValue<'static>| {
                let word = ident(token).map(str::to_ascii_lowercase);
                if matches!(word.as_deref(), Some("thin" | "medium" | "thick"))
                    || matches!(token, TokenOrValue::Length(length) if length.to_unit_value().0 >= 0.0)
                    || matches!(token, TokenOrValue::Token(Token::Number { value, .. }) if *value == 0.0)
                    || matches!(token, TokenOrValue::Function(function) if function.name.0.eq_ignore_ascii_case("calc"))
                {
                    Some(0)
                } else if word.as_deref().and_then(border_style).is_some() {
                    Some(1)
                } else if color(token).is_ok() {
                    Some(2)
                } else {
                    None
                }
            };
            let mut seen = [false; 3];
            !tokens.is_empty()
                && tokens.len() <= 3
                && tokens
                    .iter()
                    .all(|token| kind(token).is_some_and(|part| !std::mem::replace(&mut seen[part], true)))
        }
        _ => return None,
    })
}

/// A value of `border-shape`: one or two basic shapes, each with a box (`half-border-box` by
/// default for one, `border-box` and `padding-box` for two); `None` if it is not one.
fn border_shape(tokens: &[&TokenOrValue<'static>], context: &Context) -> Option<crate::values::BorderShape> {
    use crate::values::{BoxArea, ShapeReference};
    let mut shapes: Vec<(crate::values::ClipShape, Option<ShapeReference>)> = Vec::new();
    for token in tokens {
        if let Some(name) = ident(token) {
            let reference = match name.to_ascii_lowercase().as_str() {
                "half-border-box" => ShapeReference::HalfBorderBox,
                "border-box" => ShapeReference::Box(BoxArea::BorderBox),
                "padding-box" => ShapeReference::Box(BoxArea::PaddingBox),
                "content-box" => ShapeReference::Box(BoxArea::ContentBox),
                "margin-box" => ShapeReference::Box(BoxArea::MarginBox),
                _ => return None,
            };
            let last = shapes.last_mut()?;
            if last.1.is_some() {
                return None;
            }
            last.1 = Some(reference);
            continue;
        }
        let TokenOrValue::Function(_) = token else {
            return None;
        };
        let list = TokenList(vec![(*token).clone()]);
        let shape = match crate::background::css(&list.0) {
            Some(css) if css.trim_start().to_ascii_lowercase().starts_with("path(") => {
                let (even_odd, data) = path_shape(&css)?;
                crate::values::ClipShape::Path {
                    even_odd,
                    data: std::sync::Arc::from(data),
                }
            }
            _ => {
                let clip = fagus_css::guarded(|| crate::shape::newer_clip_path(&list, context))??;
                match &clip.shape {
                    crate::values::ClipShape::Box | crate::values::ClipShape::Url(_) => return None,
                    shape => shape.clone(),
                }
            }
        };
        shapes.push((shape, None));
    }
    let mut shapes = shapes.into_iter();
    let (outer, outer_box) = shapes.next()?;
    let inner = shapes.next();
    if shapes.next().is_some() {
        return None;
    }
    Some(match inner {
        None => crate::values::BorderShape {
            outer: (outer, outer_box.unwrap_or(ShapeReference::HalfBorderBox)),
            inner: None,
        },
        Some((inner, inner_box)) => crate::values::BorderShape {
            outer: (
                outer,
                outer_box.unwrap_or(ShapeReference::Box(BoxArea::BorderBox)),
            ),
            inner: Some((
                inner,
                inner_box.unwrap_or(ShapeReference::Box(BoxArea::PaddingBox)),
            )),
        },
    })
}

/// The fill rule and the path data of `path([<fill-rule>,] "…")`.
pub(crate) fn path_shape(css: &str) -> Option<(bool, String)> {
    let inner = css.trim().get(5..)?.strip_suffix(')')?.trim();
    let (even_odd, rest) = match inner.split_once(',') {
        Some((rule, rest)) if !rule.trim_start().starts_with(['"', '\'']) => {
            (rule.trim().eq_ignore_ascii_case("evenodd"), rest.trim())
        }
        _ => (false, inner),
    };
    let data = rest
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .or_else(|| rest.strip_prefix('\'').and_then(|rest| rest.strip_suffix('\'')))?;
    Some((even_odd, data.to_owned()))
}

/// Whether tokens are a value of `corner-shape` (one to four shapes) or of one of its longhands
/// (one shape, two for a side), which Chrome takes.
pub(crate) fn is_corner_shape(name: &str, tokens: &TokenList<'static>) -> bool {
    let tokens = significant(tokens);
    let most = match name {
        "corner-shape" => 4,
        "corner-top-shape" | "corner-right-shape" | "corner-bottom-shape" | "corner-left-shape" => 2,
        _ => 1,
    };
    (1..=most).contains(&tokens.len()) && tokens.iter().all(|token| corner_shape(token).is_some())
}

/// A value of `corner-shape` as Chrome's curvature: 2 to the power of the `superellipse()`
/// parameter, from 1/1000 (`notch`) to 1000 (`square`).
fn corner_shape(token: &TokenOrValue<'static>) -> Option<f32> {
    let parameter = match token {
        TokenOrValue::Token(Token::Ident(keyword)) => match keyword.to_ascii_lowercase().as_str() {
            "round" => 1.0,
            "scoop" => -1.0,
            "bevel" => 0.0,
            "notch" => f32::NEG_INFINITY,
            "square" => f32::INFINITY,
            "squircle" => 2.0,
            _ => return None,
        },
        TokenOrValue::Function(function) if function.name.0.eq_ignore_ascii_case("superellipse") => {
            match significant(&function.arguments).as_slice() {
                [TokenOrValue::Token(Token::Number { value, .. })] => *value,
                [TokenOrValue::Token(Token::Ident(keyword))] => match keyword.to_ascii_lowercase().as_str() {
                    "infinity" => f32::INFINITY,
                    "-infinity" => f32::NEG_INFINITY,
                    _ => return None,
                },
                _ => return None,
            }
        }
        _ => return None,
    };
    Some(libm::exp2f(parameter).clamp(0.001, 1000.0))
}

fn number(token: &TokenOrValue<'static>) -> Option<f32> {
    match token {
        TokenOrValue::Token(Token::Number { value, .. }) if value.is_finite() => Some(*value),
        _ => None,
    }
}

fn integer(token: &TokenOrValue<'static>) -> Option<i32> {
    match token {
        TokenOrValue::Token(Token::Number {
            int_value: Some(value),
            ..
        }) => Some(*value),
        _ => None,
    }
}

fn break_between(keyword: &str) -> Option<BreakBetween> {
    Some(match keyword {
        "auto" => BreakBetween::Auto,
        "avoid" => BreakBetween::Avoid,
        "avoid-page" => BreakBetween::AvoidPage,
        "page" => BreakBetween::Page,
        "left" => BreakBetween::Left,
        "right" => BreakBetween::Right,
        "recto" => BreakBetween::Recto,
        "verso" => BreakBetween::Verso,
        "column" => BreakBetween::Column,
        "avoid-column" => BreakBetween::AvoidColumn,
        _ => return None,
    })
}

/// `page-break-before` and `page-break-after` (CSS 2.1), mapped to `break-*` as specified.
fn legacy_break(keyword: &str) -> Option<BreakBetween> {
    Some(match keyword {
        "always" => BreakBetween::Page,
        "auto" => BreakBetween::Auto,
        "avoid" => BreakBetween::Avoid,
        "left" => BreakBetween::Left,
        "right" => BreakBetween::Right,
        _ => return None,
    })
}

pub(crate) fn list_style_by_name(name: &str) -> ListStyleType {
    match name.to_ascii_lowercase().as_str() {
        "disc" => ListStyleType::Disc,
        "circle" => ListStyleType::Circle,
        "square" => ListStyleType::Square,
        "decimal" => ListStyleType::Decimal,
        "decimal-leading-zero" => ListStyleType::DecimalLeadingZero,
        "lower-alpha" | "lower-latin" => ListStyleType::LowerAlpha,
        "upper-alpha" | "upper-latin" => ListStyleType::UpperAlpha,
        "lower-roman" => ListStyleType::LowerRoman,
        "upper-roman" => ListStyleType::UpperRoman,
        "none" => ListStyleType::None,
        _ => ListStyleType::Named(name.to_owned()),
    }
}

fn which(token: Option<&&TokenOrValue<'static>>) -> StringWhich {
    match token
        .and_then(|token| ident(token))
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("start") => StringWhich::Start,
        Some("last") => StringWhich::Last,
        Some("first-except") => StringWhich::FirstExcept,
        _ => StringWhich::First,
    }
}

/// Items of `content` or of a `string-set` value.
fn content_items(tokens: &[&TokenOrValue<'static>]) -> Vec<ContentItem> {
    let mut items = Vec::new();
    for token in tokens {
        match token {
            TokenOrValue::Token(Token::String(text)) => items.push(ContentItem::String(text.to_string())),
            TokenOrValue::Url(url) => items.push(ContentItem::Image(url.url.to_string())),
            TokenOrValue::Token(Token::UnquotedUrl(url)) => items.push(ContentItem::Image(url.to_string())),
            TokenOrValue::Token(Token::Ident(keyword)) => match keyword.to_ascii_lowercase().as_str() {
                "open-quote" => items.push(ContentItem::OpenQuote),
                "close-quote" => items.push(ContentItem::CloseQuote),
                "no-open-quote" => items.push(ContentItem::NoOpenQuote),
                "no-close-quote" => items.push(ContentItem::NoCloseQuote),
                _ => {}
            },
            TokenOrValue::Function(function) => {
                let arguments = significant(&function.arguments);
                let parts: Vec<Vec<&TokenOrValue<'static>>> = arguments
                    .split(|token| matches!(token, TokenOrValue::Token(Token::Comma)))
                    .map(<[&TokenOrValue<'static>]>::to_vec)
                    .collect();
                let first_ident = |index: usize| {
                    parts
                        .get(index)
                        .and_then(|part| part.first())
                        .and_then(|token| ident(token))
                        .map(str::to_owned)
                };
                let first_string = |index: usize| {
                    parts
                        .get(index)
                        .and_then(|part| part.first())
                        .and_then(|token| string(token))
                        .map(str::to_owned)
                };
                match function.name.0.to_ascii_lowercase().as_str() {
                    "counter" => {
                        if let Some(name) = first_ident(0) {
                            let style = first_ident(1)
                                .map_or(ListStyleType::Decimal, |style| list_style_by_name(&style));
                            items.push(ContentItem::Counter { name, style });
                        }
                    }
                    "counters" => {
                        if let (Some(name), Some(separator)) = (first_ident(0), first_string(1)) {
                            let style = first_ident(2)
                                .map_or(ListStyleType::Decimal, |style| list_style_by_name(&style));
                            items.push(ContentItem::Counters {
                                name,
                                separator,
                                style,
                            });
                        }
                    }
                    "attr" => {
                        if let Some(name) = first_ident(0) {
                            items.push(ContentItem::Attr(name));
                        }
                    }
                    "string" => {
                        if let Some(name) = first_ident(0) {
                            items.push(ContentItem::NamedString {
                                name,
                                which: which(parts.get(1).and_then(|part| part.first())),
                            });
                        }
                    }
                    "content" => items.push(ContentItem::Content(
                        match first_ident(0)
                            .map(|target| target.to_ascii_lowercase())
                            .as_deref()
                        {
                            Some("before") => ContentTarget::Before,
                            Some("after") => ContentTarget::After,
                            Some("first-letter") => ContentTarget::FirstLetter,
                            Some("marker") => ContentTarget::Marker,
                            _ => ContentTarget::Text,
                        },
                    )),
                    "element" => {
                        if let Some(name) = first_ident(0) {
                            items.push(ContentItem::Element {
                                name,
                                which: which(parts.get(1).and_then(|part| part.first())),
                            });
                        }
                    }
                    name @ ("target-counter" | "target-counters" | "target-text") => {
                        let Some(target) = parts.first().and_then(|part| target_url(part)) else {
                            continue;
                        };
                        match name {
                            "target-counter" => {
                                if let Some(counter) = first_ident(1) {
                                    let style = first_ident(2)
                                        .map_or(ListStyleType::Decimal, |style| list_style_by_name(&style));
                                    items.push(ContentItem::TargetCounter {
                                        target,
                                        name: counter,
                                        style,
                                    });
                                }
                            }
                            "target-counters" => {
                                if let (Some(counter), Some(separator)) = (first_ident(1), first_string(2)) {
                                    let style = first_ident(3)
                                        .map_or(ListStyleType::Decimal, |style| list_style_by_name(&style));
                                    items.push(ContentItem::TargetCounters {
                                        target,
                                        name: counter,
                                        separator,
                                        style,
                                    });
                                }
                            }
                            _ => items.push(ContentItem::TargetText {
                                target,
                                which: match first_ident(1).map(|which| which.to_ascii_lowercase()).as_deref()
                                {
                                    Some("before") => ContentTarget::Before,
                                    Some("after") => ContentTarget::After,
                                    Some("first-letter") => ContentTarget::FirstLetter,
                                    _ => ContentTarget::Text,
                                },
                            }),
                        }
                    }
                    "leader" => {
                        let fill = match first_ident(0).as_deref() {
                            Some("dotted") => ".".to_owned(),
                            Some("solid") => "_".to_owned(),
                            Some("space") => " ".to_owned(),
                            _ => first_string(0).unwrap_or_else(|| ".".to_owned()),
                        };
                        items.push(ContentItem::Leader(fill));
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    items
}

/// The URL a `target-*()` function points to: `attr(name)`, `url(…)` or a string.
fn target_url(tokens: &[&TokenOrValue<'static>]) -> Option<crate::values::TargetUrl> {
    use crate::values::TargetUrl;
    match tokens.first()? {
        TokenOrValue::Function(function) if function.name.0.eq_ignore_ascii_case("attr") => {
            significant(&function.arguments)
                .first()
                .and_then(|token| ident(token))
                .map(|name| TargetUrl::Attr(name.to_ascii_lowercase()))
        }
        TokenOrValue::Url(url) => Some(TargetUrl::Url(url.url.to_string())),
        TokenOrValue::Token(Token::UnquotedUrl(url)) => Some(TargetUrl::Url(url.to_string())),
        token => string(token).map(|url| TargetUrl::Url(url.to_owned())),
    }
}

fn content(tokens: &[&TokenOrValue<'static>]) -> Content {
    match tokens
        .first()
        .and_then(|token| ident(token))
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("normal") if tokens.len() == 1 => Content::Normal,
        Some("none") if tokens.len() == 1 => Content::None,
        // The alternative text after a `/` is for speech, not shown.
        _ => {
            let shown = tokens
                .iter()
                .position(|token| matches!(token, TokenOrValue::Token(Token::Delim('/'))))
                .map_or(tokens, |slash| &tokens[..slash]);
            Content::Items(content_items(shown))
        }
    }
}

/// `counter-reset`, `counter-increment` and `counter-set`: names, each with an optional integer.
fn counters(tokens: &[&TokenOrValue<'static>], default: i32) -> Vec<(String, i32)> {
    let mut result: Vec<(String, i32)> = Vec::new();
    for token in tokens {
        if let Some(name) = ident(token) {
            if name.eq_ignore_ascii_case("none") {
                return Vec::new();
            }
            result.push((name.to_owned(), default));
        } else if let Some(value) = integer(token)
            && let Some(last) = result.last_mut()
        {
            last.1 = value;
        }
    }
    result
}

/// `string-set: name content-list [, name content-list]*`.
fn string_set(tokens: &[&TokenOrValue<'static>]) -> Vec<(String, Vec<ContentItem>)> {
    tokens
        .split(|token| matches!(token, TokenOrValue::Token(Token::Comma)))
        .filter_map(|part| {
            let (name, rest) = part.split_first()?;
            let name = ident(name)?;
            if name.eq_ignore_ascii_case("none") {
                return None;
            }
            Some((name.to_owned(), content_items(rest)))
        })
        .collect()
}

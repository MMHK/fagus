//! The `zoom` property (CSS Viewport 1, §5, "standardized CSS zoom" as Chrome implements it).
//!
//! An element's effective zoom is the product of its own `zoom` and its parent's effective
//! zoom. Every absolute length the element's properties specify is multiplied by it when the
//! value is computed (see [`crate::compute::Context::zoom`]); font-relative lengths follow from
//! the zoomed font size. Inherited values that hold lengths were zoomed for the parent: they
//! are scaled to the element's zoom where it differs ("rezoomed", as Blink does for its
//! `affected_by_zoom` properties).

use lightningcss::properties::PropertyId;
use lightningcss::properties::custom::{CustomPropertyName, Token, TokenList, TokenOrValue};

use crate::style::ComputedStyle;
use crate::values::{
    BackgroundSize, CalcNode, DecorationThickness, LengthPercentage, LengthPercentageAuto, LineHeight,
    MaxSize, Size, TabSize, VerticalAlign,
};

/// The cascade's key of `zoom`, which lightningcss keeps as tokens.
pub(crate) fn zoom_id() -> PropertyId<'static> {
    PropertyId::Custom(CustomPropertyName::Unknown(lightningcss::values::ident::Ident(
        "zoom".into(),
    )))
}

/// The factor a `zoom` declaration gives: a non-negative number or percentage (or a
/// calculation of one), or `normal`; zero means no zoom, as in Chrome. `None` for a value Chrome
/// rejects (a negative one, `reset`).
pub(crate) fn specified(tokens: &TokenList<'static>) -> Option<f32> {
    let mut significant = tokens.0.iter().filter(|token| {
        !matches!(
            token,
            TokenOrValue::Token(Token::WhiteSpace(_) | Token::Comment(_))
        )
    });
    let (Some(token), None) = (significant.next(), significant.next()) else {
        return None;
    };
    let factor = match token {
        TokenOrValue::Token(Token::Number { value, .. }) => *value,
        TokenOrValue::Token(Token::Percentage { unit_value, .. }) => *unit_value,
        TokenOrValue::Token(Token::Ident(ident)) if ident.eq_ignore_ascii_case("normal") => 1.0,
        TokenOrValue::Function(_) => {
            use lightningcss::traits::Parse;
            use lightningcss::values::percentage::NumberOrPercentage;
            match NumberOrPercentage::parse_string(&crate::variables::to_css(tokens)?).ok()? {
                NumberOrPercentage::Number(number) => number,
                NumberOrPercentage::Percentage(percentage) => percentage.0,
            }
        }
        _ => return None,
    };
    if !factor.is_finite() || factor < 0.0 {
        return None;
    }
    Some(if factor == 0.0 { 1.0 } else { factor })
}

/// Scales the inherited values of `style` that hold lengths by `ratio`, the element's zoom
/// over its parent's: `font-size`, `line-height`, `letter-spacing`, `word-spacing`,
/// `text-indent`, `tab-size`, `text-shadow`, `text-underline-offset` and `border-spacing`.
pub(crate) fn rezoom_inherited(style: &mut ComputedStyle, ratio: f32) {
    style.font_size *= ratio;
    if let LineHeight::Px(px) = &mut style.line_height {
        *px *= ratio;
    }
    style.letter_spacing *= ratio;
    style.word_spacing *= ratio;
    style.text_indent = scaled(&style.text_indent, ratio);
    if let TabSize::Px(px) = &mut style.tab_size {
        *px *= ratio;
    }
    style.text_shadow = scaled_text_shadow(&style.text_shadow, ratio);
    if let Some(offset) = &mut style.text_underline_offset {
        *offset *= ratio;
    }
    style.border_spacing = (style.border_spacing.0 * ratio, style.border_spacing.1 * ratio);
}

/// Scales the lengths of one property of `style` that it took over from its parent with
/// `inherit` (or `unset`) by `ratio`, the element's zoom over its parent's. Chrome computes such
/// a value again for the element's zoom, inherited or not: `width: inherit` under `zoom: 3` is
/// three times as wide as the parent (Chrome 157, measured).
pub(crate) fn rezoom_property(id: &PropertyId<'_>, style: &mut ComputedStyle, ratio: f32) {
    let lpa = |value: &mut LengthPercentageAuto| {
        if let LengthPercentageAuto::Length(length) = value {
            *length = scaled(length, ratio);
        }
    };
    let pair = |(x, y): &(LengthPercentage, LengthPercentage)| (scaled(x, ratio), scaled(y, ratio));
    let width = |width: f32| crate::compute::snap_border_width(width * ratio);
    match id {
        PropertyId::Top => lpa(&mut style.inset.top),
        PropertyId::Right => lpa(&mut style.inset.right),
        PropertyId::Bottom => lpa(&mut style.inset.bottom),
        PropertyId::Left => lpa(&mut style.inset.left),
        PropertyId::Width => style.width = scaled_size(&style.width, ratio),
        PropertyId::Height => style.height = scaled_size(&style.height, ratio),
        PropertyId::MinWidth => style.min_width = scaled_size(&style.min_width, ratio),
        PropertyId::MinHeight => style.min_height = scaled_size(&style.min_height, ratio),
        PropertyId::MaxWidth => style.max_width = scaled_max_size(&style.max_width, ratio),
        PropertyId::MaxHeight => style.max_height = scaled_max_size(&style.max_height, ratio),
        PropertyId::FlexBasis(_) => style.flex_basis = scaled_size(&style.flex_basis, ratio),
        PropertyId::MarginTop => lpa(&mut style.margin.top),
        PropertyId::MarginRight => lpa(&mut style.margin.right),
        PropertyId::MarginBottom => lpa(&mut style.margin.bottom),
        PropertyId::MarginLeft => lpa(&mut style.margin.left),
        PropertyId::PaddingTop => style.padding.top = scaled(&style.padding.top, ratio),
        PropertyId::PaddingRight => style.padding.right = scaled(&style.padding.right, ratio),
        PropertyId::PaddingBottom => style.padding.bottom = scaled(&style.padding.bottom, ratio),
        PropertyId::PaddingLeft => style.padding.left = scaled(&style.padding.left, ratio),
        PropertyId::BorderTopWidth => style.border_width.top = width(style.border_width.top),
        PropertyId::BorderRightWidth => style.border_width.right = width(style.border_width.right),
        PropertyId::BorderBottomWidth => style.border_width.bottom = width(style.border_width.bottom),
        PropertyId::BorderLeftWidth => style.border_width.left = width(style.border_width.left),
        PropertyId::BorderTopLeftRadius(_) => {
            style.border_radius.top_left = pair(&style.border_radius.top_left);
        }
        PropertyId::BorderTopRightRadius(_) => {
            style.border_radius.top_right = pair(&style.border_radius.top_right);
        }
        PropertyId::BorderBottomRightRadius(_) => {
            style.border_radius.bottom_right = pair(&style.border_radius.bottom_right);
        }
        PropertyId::BorderBottomLeftRadius(_) => {
            style.border_radius.bottom_left = pair(&style.border_radius.bottom_left);
        }
        PropertyId::OutlineWidth => style.outline_width = width(style.outline_width),
        PropertyId::BoxShadow(_) if !style.box_shadow.is_empty() => {
            style.box_shadow = style
                .box_shadow
                .iter()
                .map(|shadow| crate::values::BoxShadow {
                    x: shadow.x * ratio,
                    y: shadow.y * ratio,
                    blur: shadow.blur * ratio,
                    spread: shadow.spread * ratio,
                    ..*shadow
                })
                .collect();
        }
        PropertyId::Translate => {
            if let Some((x, y, z)) = &style.translate {
                style.translate = Some((scaled(x, ratio), scaled(y, ratio), z * ratio));
            }
        }
        PropertyId::TransformOrigin(_) => style.transform_origin = pair(&style.transform_origin),
        PropertyId::PerspectiveOrigin(_) => style.perspective_origin = pair(&style.perspective_origin),
        PropertyId::Perspective(_) => style.perspective = style.perspective.map(|length| length * ratio),
        PropertyId::BackgroundPositionX => {
            style.background_position_x = style
                .background_position_x
                .iter()
                .map(|x| scaled(x, ratio))
                .collect();
        }
        PropertyId::BackgroundPositionY => {
            style.background_position_y = style
                .background_position_y
                .iter()
                .map(|y| scaled(y, ratio))
                .collect();
        }
        PropertyId::BackgroundSize => {
            style.background_size = style
                .background_size
                .iter()
                .map(|size| match size {
                    BackgroundSize::Explicit(width, height) => {
                        let (mut width, mut height) = (width.clone(), height.clone());
                        lpa(&mut width);
                        lpa(&mut height);
                        BackgroundSize::Explicit(width, height)
                    }
                    other => other.clone(),
                })
                .collect();
        }
        PropertyId::RowGap => style.row_gap = style.row_gap.as_ref().map(|gap| scaled(gap, ratio)),
        PropertyId::ColumnGap => style.column_gap = style.column_gap.as_ref().map(|gap| scaled(gap, ratio)),
        PropertyId::VerticalAlign => {
            if let VerticalAlign::Length(length) = &style.vertical_align {
                style.vertical_align = VerticalAlign::Length(scaled(length, ratio));
            }
        }
        PropertyId::TextDecorationThickness => {
            if let DecorationThickness::Px(px) = &mut style.text_decoration_thickness {
                *px *= ratio;
            }
        }
        PropertyId::LineHeight => {
            if let LineHeight::Px(px) = &mut style.line_height {
                *px *= ratio;
            }
        }
        PropertyId::LetterSpacing => style.letter_spacing *= ratio,
        PropertyId::WordSpacing => style.word_spacing *= ratio,
        PropertyId::TextIndent => style.text_indent = scaled(&style.text_indent, ratio),
        PropertyId::TabSize(_) => {
            if let TabSize::Px(px) = &mut style.tab_size {
                *px *= ratio;
            }
        }
        PropertyId::TextShadow => style.text_shadow = scaled_text_shadow(&style.text_shadow, ratio),
        PropertyId::BorderSpacing => {
            style.border_spacing = (style.border_spacing.0 * ratio, style.border_spacing.1 * ratio);
        }
        PropertyId::Custom(CustomPropertyName::Unknown(name)) => match name.0.as_ref() {
            "column-width" => style.column_width = style.column_width.map(|width| width * ratio),
            "column-rule-width" => style.column_rule_width = width(style.column_rule_width),
            "outline-offset" => {
                style.outline_offset =
                    crate::compute::snap_border_width((style.outline_offset * ratio).abs())
                        .copysign(style.outline_offset);
            }
            "text-underline-offset" => {
                if let Some(offset) = &mut style.text_underline_offset {
                    *offset *= ratio;
                }
            }
            "object-position" => style.object_position = pair(&style.object_position),
            "shape-margin" => style.shape_margin = scaled(&style.shape_margin, ratio),
            "overflow-clip-margin" => style.overflow_clip_margin.1 *= ratio,
            "contain-intrinsic-size"
            | "contain-intrinsic-width"
            | "contain-intrinsic-height"
            | "contain-intrinsic-inline-size"
            | "contain-intrinsic-block-size" => {
                let (width, height) = style.contain_intrinsic_size;
                style.contain_intrinsic_size = (
                    width.map(|width| width * ratio),
                    height.map(|height| height * ratio),
                );
            }
            _ => {}
        },
        _ => {}
    }
}

/// The initial value of a property at an effective zoom, where it holds a length: the `medium`
/// widths of borders, outlines and column rules, 3px zoomed.
pub(crate) fn initial_widths(id: &PropertyId<'_>, style: &mut ComputedStyle) {
    let medium = || crate::compute::snap_border_width(3.0 * style.zoom);
    match id {
        PropertyId::BorderTopWidth => style.border_width.top = medium(),
        PropertyId::BorderRightWidth => style.border_width.right = medium(),
        PropertyId::BorderBottomWidth => style.border_width.bottom = medium(),
        PropertyId::BorderLeftWidth => style.border_width.left = medium(),
        PropertyId::OutlineWidth => style.outline_width = medium(),
        PropertyId::Custom(CustomPropertyName::Unknown(name))
            if matches!(name.0.as_ref(), "column-rule-width" | "column-rule") =>
        {
            style.column_rule_width = medium();
        }
        _ => {}
    }
}

fn scaled_size(size: &Size, ratio: f32) -> Size {
    match size {
        Size::Length(length) => Size::Length(scaled(length, ratio)),
        Size::FitContent(Some(length)) => Size::FitContent(Some(scaled(length, ratio))),
        other => other.clone(),
    }
}

fn scaled_max_size(size: &MaxSize, ratio: f32) -> MaxSize {
    match size {
        MaxSize::Length(length) => MaxSize::Length(scaled(length, ratio)),
        MaxSize::FitContent(Some(length)) => MaxSize::FitContent(Some(scaled(length, ratio))),
        other => other.clone(),
    }
}

fn scaled_text_shadow(
    shadows: &std::sync::Arc<[crate::values::TextShadow]>,
    ratio: f32,
) -> std::sync::Arc<[crate::values::TextShadow]> {
    if shadows.is_empty() {
        return std::sync::Arc::clone(shadows);
    }
    shadows
        .iter()
        .map(|shadow| crate::values::TextShadow {
            x: shadow.x * ratio,
            y: shadow.y * ratio,
            blur: shadow.blur * ratio,
            ..*shadow
        })
        .collect()
}

/// A length-percentage with its length scaled by `ratio`.
pub(crate) fn scaled(value: &LengthPercentage, ratio: f32) -> LengthPercentage {
    match value {
        LengthPercentage::Px(px) => LengthPercentage::Px(px * ratio),
        LengthPercentage::Percent(percent) => LengthPercentage::Percent(*percent),
        LengthPercentage::Calc(node) => LengthPercentage::from_node(scaled_node(node, ratio)),
    }
}

fn scaled_node(node: &CalcNode, ratio: f32) -> CalcNode {
    let boxed = |node: &CalcNode| Box::new(scaled_node(node, ratio));
    let list = |nodes: &[CalcNode]| nodes.iter().map(|node| scaled_node(node, ratio)).collect();
    match node {
        CalcNode::Px(px) => CalcNode::Px(px * ratio),
        CalcNode::Percent(percent) => CalcNode::Percent(*percent),
        CalcNode::Sum(a, b) => CalcNode::Sum(boxed(a), boxed(b)),
        CalcNode::Product(factor, node) => CalcNode::Product(*factor, boxed(node)),
        CalcNode::Min(nodes) => CalcNode::Min(list(nodes)),
        CalcNode::Max(nodes) => CalcNode::Max(list(nodes)),
        CalcNode::Clamp(min, value, max) => CalcNode::Clamp(boxed(min), boxed(value), boxed(max)),
        CalcNode::Abs(node) => CalcNode::Abs(boxed(node)),
        CalcNode::Round(rounding, value, step) => CalcNode::Round(*rounding, boxed(value), boxed(step)),
        CalcNode::Mod(a, b) => CalcNode::Mod(boxed(a), boxed(b)),
        CalcNode::Rem(a, b) => CalcNode::Rem(boxed(a), boxed(b)),
        CalcNode::Hypot(nodes) => CalcNode::Hypot(list(nodes)),
    }
}

#[cfg(test)]
mod tests {
    use fagus_css::{MediaEnvironment, Origin, RuleSet, Stylesheet};
    use fagus_dom::{Document, NodeId};

    use crate::values::{LengthPercentage, LengthPercentageAuto, LineHeight, Size};
    use crate::{ComputedStyle, StyledDocument, USER_AGENT_CSS, compute_styles};

    fn styles(html: &str) -> (Document, StyledDocument) {
        let document = Document::parse_html(html);
        let ua = Stylesheet::parse(USER_AGENT_CSS, Origin::UserAgent, "ua.css");
        let environment = MediaEnvironment::default();
        let rules = RuleSet::new(&[&ua], &environment);
        let styled = compute_styles(&document, &rules, (800.0, 600.0));
        (document, styled)
    }

    fn by_id<'a>(document: &Document, styled: &'a StyledDocument, id: &str) -> &'a ComputedStyle {
        let node = document
            .descendants(NodeId::DOCUMENT)
            .find(|&node| {
                document
                    .element(node)
                    .is_some_and(|data| data.id.as_deref() == Some(id))
            })
            .expect("element with id");
        styled.style(node).expect("styled element")
    }

    fn px(size: &Size) -> f32 {
        match size {
            Size::Length(LengthPercentage::Px(px)) => *px,
            other => panic!("not a length: {other:?}"),
        }
    }

    #[test]
    fn multiplies_absolute_lengths_by_the_effective_zoom() {
        let (document, styled) = styles(
            "<div id=a style='zoom:2;width:100px;margin:3px;font-size:10px'>\
               <div id=b style='zoom:150%;width:10px;height:1em'></div>\
               <div id=c style='width:1rem;height:10vw'></div></div>\
             <div id=d style='zoom:0;width:100px'></div><div id=e style='zoom:-1;width:100px'></div>\
             <div id=f style='zoom:normal;width:100px'></div><div id=g style='zoom:reset;width:100px'></div>\
             <div id=h style='zoom:calc(2);width:10px'></div>",
        );
        let a = by_id(&document, &styled, "a");
        assert!((a.zoom - 2.0).abs() < 1e-6);
        assert!((px(&a.width) - 200.0).abs() < 1e-4);
        assert_eq!(
            a.margin.top,
            LengthPercentageAuto::Length(LengthPercentage::Px(6.0))
        );
        assert!((a.font_size - 20.0).abs() < 1e-4);
        let b = by_id(&document, &styled, "b");
        assert!((b.zoom - 3.0).abs() < 1e-6, "zoom multiplies down the tree");
        assert!((px(&b.width) - 30.0).abs() < 1e-4);
        // The inherited font size of 10px is zoomed by three: `em` refers to it.
        assert!((b.font_size - 30.0).abs() < 1e-4);
        assert!((px(&b.height) - 30.0).abs() < 1e-4);
        let c = by_id(&document, &styled, "c");
        // `rem` is the root's font size zoomed by the element's zoom, viewport units too.
        assert!((px(&c.width) - 32.0).abs() < 1e-4);
        assert!((px(&c.height) - 160.0).abs() < 1e-4);
        assert!(
            (px(&by_id(&document, &styled, "h").width) - 20.0).abs() < 1e-4,
            "calc()"
        );
        for id in ["d", "e", "f", "g"] {
            let style = by_id(&document, &styled, id);
            assert!((style.zoom - 1.0).abs() < 1e-6, "{id}");
            assert!((px(&style.width) - 100.0).abs() < 1e-4, "{id}");
        }
    }

    #[test]
    fn refers_container_units_and_queries_to_unzoomed_sizes() {
        let document = Document::parse_html(
            "<div id=a class=box><p id=a1 class=w></p></div>\
             <div id=b class=box style='zoom:2'><p id=b1 class=w></p></div>",
        );
        let ua = Stylesheet::parse(USER_AGENT_CSS, Origin::UserAgent, "ua.css");
        let author = Stylesheet::parse(
            ".box { container-type: inline-size } .w { width: 10cqw; zoom: 2 }
             @container (min-width: 300px) { .w { height: 5px } }",
            Origin::Author,
            "test.css",
        );
        let rules = RuleSet::new(&[&ua, &author], &MediaEnvironment::default());
        // Both containers are laid out 400px wide, the second zoomed from 200px.
        let mut sizes = crate::ContainerSizes::default();
        for id in ["a", "b"] {
            let node = document
                .descendants(NodeId::DOCUMENT)
                .find(|&node| {
                    document
                        .element(node)
                        .is_some_and(|data| data.id.as_deref() == Some(id))
                })
                .expect("container");
            sizes.insert(
                node,
                fagus_css::ContainerSize {
                    width: 400.0,
                    height: None,
                    font_size: 16.0,
                },
            );
        }
        let styled = crate::compute_styles_in(&document, &rules, (800.0, 600.0), &sizes);
        // `cqw` is a hundredth of the container's own width, zoomed as other lengths of the
        // element are; queries ask the container's own width (Chrome 157, measured).
        let a1 = by_id(&document, &styled, "a1");
        assert!((px(&a1.width) - 80.0).abs() < 1e-4);
        assert!((px(&a1.height) - 10.0).abs() < 1e-4);
        let b1 = by_id(&document, &styled, "b1");
        assert!((px(&b1.width) - 80.0).abs() < 1e-4);
        assert_eq!(b1.height, Size::Auto);
    }

    #[test]
    fn rezooms_inherited_lengths_and_snaps_zoomed_border_widths() {
        let (document, styled) = styles(
            "<div style='line-height:20px;letter-spacing:2px;text-indent:7px;border-spacing:2px'>\
               <div id=a style='zoom:2'></div></div>\
             <div id=b style='zoom:1.5;border:1px solid;outline:thin solid'></div>\
             <div id=c style='zoom:0.5;border:3px solid'></div>\
             <div id=d style='zoom:1.3;border:3px solid;column-rule-style:solid'></div>",
        );
        let a = by_id(&document, &styled, "a");
        assert_eq!(a.line_height, LineHeight::Px(40.0));
        assert!((a.letter_spacing - 4.0).abs() < 1e-6);
        assert_eq!(a.text_indent, LengthPercentage::Px(14.0));
        assert_eq!(a.border_spacing, (4.0, 4.0));
        let b = by_id(&document, &styled, "b");
        assert!((b.border_width.top - 1.0).abs() < 1e-6, "1.5px is cut to 1px");
        assert!((b.outline_width - 1.0).abs() < 1e-6);
        assert!((by_id(&document, &styled, "c").border_width.top - 1.0).abs() < 1e-6);
        let d = by_id(&document, &styled, "d");
        assert!((d.border_width.left - 3.0).abs() < 1e-6, "3.9px is cut to 3px");
        assert!(
            (d.column_rule_width - 3.0).abs() < 1e-6,
            "medium is 3.9px, cut to 3px"
        );
    }

    #[test]
    fn inherits_the_parents_own_zoom_and_rezooms_inherited_lengths() {
        let (document, styled) = styles(
            "<div style='zoom:3'><div id=a style='zoom:inherit'><div id=b style='zoom:inherit'></div></div></div>\
             <div style='zoom:2;width:50px;padding-left:10px;border-left:4px solid'>\
               <div id=c style='zoom:3;width:inherit;padding-left:inherit;border-left:inherit'></div>\
               <div id=d style='zoom:0.5;width:inherit;margin-top:inherit'></div></div>\
             <div id=e style='zoom:2;border-style:solid'></div><div id=f style='zoom:1.5;border-top-style:solid'></div>",
        );
        // `zoom: inherit` takes the parent's own zoom, not its effective one (Chrome 157).
        let first = by_id(&document, &styled, "a");
        assert!((first.zoom - 9.0).abs() < 1e-5 && (first.specified_zoom - 3.0).abs() < 1e-6);
        assert!((by_id(&document, &styled, "b").zoom - 27.0).abs() < 1e-4);
        // A length taken with `inherit` is the parent's, computed again at the element's zoom.
        let wide = by_id(&document, &styled, "c");
        assert!((px(&wide.width) - 300.0).abs() < 1e-3, "{:?}", wide.width);
        assert_eq!(wide.padding.left, LengthPercentage::Px(60.0));
        assert!((wide.border_width.left - 24.0).abs() < 1e-4);
        let narrow = by_id(&document, &styled, "d");
        assert!((px(&narrow.width) - 50.0).abs() < 1e-3, "{:?}", narrow.width);
        // The initial border width is `medium`, zoomed and snapped as other border widths.
        let doubled = by_id(&document, &styled, "e");
        assert!(
            (doubled.border_width.top - 6.0).abs() < 1e-6 && (doubled.border_width.left - 6.0).abs() < 1e-6
        );
        let half = by_id(&document, &styled, "f");
        assert!((half.border_width.top - 4.0).abs() < 1e-6, "4.5px is cut to 4px");
        assert!(half.border_width.left.abs() < 1e-6, "no border style, no width");
    }
}

//! Sizing values that lightningcss does not parse or that Chrome takes otherwise: sizing
//! keywords in `flex-basis` and `flex`, `fit-content()` in sizes, `stretch` maximums,
//! `aspect-ratio: auto && <ratio>`, `contain` and `contain-intrinsic-size`.

use fagus_css::{MediaEnvironment, Origin, RuleSet, Stylesheet};
use fagus_dom::{Document, NodeId};

use crate::values::{MaxSize, Size};
use crate::{ComputedStyle, USER_AGENT_CSS, compute_styles};

/// The computed style of the first `div` under `css`.
fn style(css: &str) -> ComputedStyle {
    let document = Document::parse_html("<div></div>");
    let ua = Stylesheet::parse(USER_AGENT_CSS, Origin::UserAgent, "ua.css");
    let author = Stylesheet::parse(&format!("div {{ {css} }}"), Origin::Author, "test.css");
    let environment = MediaEnvironment::default();
    let rules = RuleSet::new(&[&ua, &author], &environment);
    let styled = compute_styles(&document, &rules, (environment.width, environment.height));
    let node = document
        .descendants(NodeId::DOCUMENT)
        .find(|&node| document.element(node).is_some_and(|data| data.is("div")))
        .expect("div");
    styled.style(node).expect("styled").as_ref().clone()
}

#[test]
fn takes_sizing_keywords_in_flex_bases() {
    assert_eq!(style("flex-basis: min-content").flex_basis, Size::MinContent);
    assert_eq!(style("flex-basis: content").flex_basis, Size::MaxContent);
    assert_eq!(
        style("flex-basis: -webkit-fill-available").flex_basis,
        Size::Stretch
    );
    let flex = style("flex: 2 3 fit-content");
    assert_eq!((flex.flex_grow, flex.flex_shrink), (2.0, 3.0));
    assert_eq!(flex.flex_basis, Size::FitContent(None));
    let flex = style("flex: max-content");
    assert_eq!(
        (flex.flex_grow, flex.flex_shrink, flex.flex_basis.clone()),
        (1.0, 1.0, Size::MaxContent)
    );
}

#[test]
fn takes_fit_content_functions_in_sizes_as_auto() {
    let style = style("width: fit-content(100px); max-width: fit-content(50%)");
    assert_eq!(style.width, Size::Auto);
    assert_eq!(style.max_width, MaxSize::None);
}

#[test]
fn keeps_stretch_maximums() {
    assert_eq!(style("max-width: stretch").max_width, MaxSize::Stretch);
    assert_eq!(
        style("max-height: -webkit-fill-available").max_height,
        MaxSize::Stretch
    );
}

#[test]
fn tells_ratios_that_give_way_to_natural_ones() {
    assert!(!style("aspect-ratio: 2").aspect_ratio_auto);
    assert!(style("aspect-ratio: auto 2").aspect_ratio_auto);
}

#[test]
fn computes_containment() {
    let contained = style("contain: size; contain-intrinsic-size: 80px auto 20px");
    assert!(contained.contain_size);
    assert_eq!(contained.contain_intrinsic_size, (Some(80.0), Some(20.0)));
    assert_eq!(contained.size_containment(), (true, true));
    let inline = style("contain: inline-size layout; contain-intrinsic-width: 90px");
    assert_eq!(inline.size_containment(), (true, false));
    assert_eq!(inline.contain_intrinsic_size, (Some(90.0), None));
    // The logical longhands follow the writing mode.
    let vertical = style(
        "writing-mode: vertical-rl; contain-intrinsic-inline-size: 50px; contain-intrinsic-block-size: 30px",
    );
    assert_eq!(vertical.contain_intrinsic_size, (Some(30.0), Some(50.0)));
    assert_eq!(
        style("container-type: inline-size").size_containment(),
        (true, false)
    );
    // Paint containment clips.
    assert_eq!(style("contain: strict").overflow_x, crate::values::Overflow::Clip);
}

#[test]
fn reads_whether_resize_lets_a_box_be_resized() {
    assert!(!style("").resize);
    assert!(style("resize: both").resize);
    assert!(style("resize: vertical").resize);
    assert!(!style("resize: both; resize: none").resize);
    assert!(style("resize: both; resize: bogus").resize);
    assert!(!style("resize: both; resize: initial").resize);
}

#[test]
fn reads_field_sizing_and_the_scroll_bars_of_overflow() {
    use crate::values::{Overflow, Scrollbar};
    assert!(style("field-sizing: content").field_sizing_content);
    assert!(!style("field-sizing: content; field-sizing: fixed").field_sizing_content);
    assert!(!style("field-sizing: bogus").field_sizing_content);
    assert!(style("frame-sizing: content-height").frame_sizing_content_height);
    assert!(!style("frame-sizing: content-height; frame-sizing: auto").frame_sizing_content_height);
    assert!(!style("frame-sizing: content-width").frame_sizing_content_height);
    let auto = style("overflow: auto");
    assert_eq!(
        (auto.overflow_x, auto.scrollbar_x),
        (Overflow::Hidden, Scrollbar::Auto)
    );
    let scroll = style("overflow-x: scroll; overflow-y: hidden");
    assert_eq!(
        (scroll.scrollbar_x, scroll.scrollbar_y),
        (Scrollbar::Scroll, Scrollbar::None)
    );
}

#[test]
fn resolves_system_colors_as_chrome_prints_them() {
    // Chrome 157 on macOS in its light color scheme (measured).
    use crate::values::Rgba;
    let background = |color: &str| style(&format!("background-color: {color}")).background_color;
    assert_eq!(background("ThreeDFace"), Rgba::from_u8(0xef, 0xef, 0xef, 1.0));
    assert_eq!(background("ButtonFace"), Rgba::from_u8(0xef, 0xef, 0xef, 1.0));
    assert_eq!(background("Canvas"), Rgba::from_u8(0xff, 0xff, 0xff, 1.0));
    assert_eq!(background("GrayText"), Rgba::from_u8(0x80, 0x80, 0x80, 1.0));
    assert_eq!(background("LinkText"), Rgba::from_u8(0x00, 0x00, 0xee, 1.0));
    assert_eq!(background("Mark"), Rgba::from_u8(0xff, 0xff, 0x00, 1.0));
    assert_eq!(background("ButtonBorder"), Rgba::from_u8(0, 0, 0, 1.0));
}

#[test]
fn takes_viewport_inline_and_block_units_along_the_element_s_own_axes() {
    let environment = MediaEnvironment::default();
    let px = |size: &Size| match size {
        Size::Length(crate::values::LengthPercentage::Px(px)) => *px,
        other => panic!("not a length: {other:?}"),
    };
    let tenth = |size: &Size, of: f32| assert!((px(size) - of / 10.0).abs() < 0.01, "{size:?}");
    let horizontal = style("width: 10vi; height: 10vb");
    tenth(&horizontal.width, environment.width);
    tenth(&horizontal.height, environment.height);
    // In a vertical writing mode the inline axis runs down the viewport.
    let vertical = style("writing-mode: vertical-rl; width: 10vb; height: 10vi");
    tenth(&vertical.width, environment.width);
    tenth(&vertical.height, environment.height);
}

#[test]
fn parses_overflow_clip_margins() {
    use crate::values::BoxArea;
    assert_eq!(style("").overflow_clip_margin, (BoxArea::PaddingBox, 0.0));
    assert_eq!(
        style("overflow-clip-margin: 10px").overflow_clip_margin,
        (BoxArea::PaddingBox, 10.0)
    );
    assert_eq!(
        style("overflow-clip-margin: 5px content-box").overflow_clip_margin,
        (BoxArea::ContentBox, 5.0)
    );
    assert_eq!(
        style("overflow-clip-margin: border-box").overflow_clip_margin,
        (BoxArea::BorderBox, 0.0)
    );
    // Chrome takes negative margins; repeated parts are invalid.
    assert_eq!(
        style("overflow-clip-margin: -10px").overflow_clip_margin,
        (BoxArea::PaddingBox, -10.0)
    );
    assert_eq!(
        style("overflow-clip-margin: 1px 2px").overflow_clip_margin,
        (BoxArea::PaddingBox, 0.0)
    );
}

#[test]
fn parses_block_margin_trims() {
    assert_eq!(style("margin-trim: block").margin_trim_block, (true, true));
    assert_eq!(
        style("margin-trim: block-end inline-start").margin_trim_block,
        (false, true)
    );
    assert_eq!(style("margin-trim: inline").margin_trim_block, (false, false));
    assert_eq!(
        style("margin-trim: block-start block-start").margin_trim_block,
        (false, false)
    );
}

#[test]
fn gives_hidden_content_visibility_containment() {
    let hidden = style("content-visibility: hidden");
    assert!(hidden.content_hidden && hidden.contain_layout && hidden.size_containment() == (true, true));
    assert_eq!(hidden.overflow_x, crate::values::Overflow::Clip);
    let auto = style("content-visibility: auto");
    assert!(!auto.content_hidden && !auto.contain_layout);
    assert!(!style("content-visibility: hidden; display: contents").content_hidden);
}

#[test]
fn maps_logical_intrinsic_sizes_by_the_writing_mode() {
    // The inline size of a vertical box is its height, whichever comes first in the rule.
    let vertical = style(
        "contain: size; contain-intrinsic-inline-size: 50px; contain-intrinsic-block-size: 100px; writing-mode: vertical-lr",
    );
    assert_eq!(vertical.contain_intrinsic_size, (Some(100.0), Some(50.0)));
    let horizontal = style("contain-intrinsic-inline-size: 50px; contain-intrinsic-block-size: 100px");
    assert_eq!(horizontal.contain_intrinsic_size, (Some(50.0), Some(100.0)));
}

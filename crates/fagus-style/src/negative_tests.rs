//! Negative values in properties that take none: written out, Chrome drops the declaration
//! (so an earlier one applies); computed by a function, the value is clamped at zero.

use fagus_css::{MediaEnvironment, Origin, RuleSet, Stylesheet};
use fagus_dom::{Document, NodeId};

use crate::values::LengthPercentage;
use crate::{ComputedStyle, USER_AGENT_CSS, compute_styles};

/// The computed style of the first `div` of `html` under `css`.
fn style(html: &str, css: &str) -> ComputedStyle {
    let document = Document::parse_html(html);
    let ua = Stylesheet::parse(USER_AGENT_CSS, Origin::UserAgent, "ua.css");
    // The engine writes author style sheets as Chrome reads them before parsing them.
    let css = format!("div {{ {css} }}");
    let css = fagus_css::author_css(&css, false).unwrap_or(css);
    let author = Stylesheet::parse(&css, Origin::Author, "test.css");
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
fn drops_written_out_negative_paddings() {
    let dropped = style("<div></div>", "padding: 10px 30px; padding: -8px 8px");
    assert_eq!(dropped.padding.top, LengthPercentage::Px(10.0));
    assert_eq!(dropped.padding.left, LengthPercentage::Px(30.0));
    let attribute = style(r#"<div style="padding: 20px; padding: -4px"></div>"#, "");
    assert_eq!(attribute.padding.top, LengthPercentage::Px(20.0));
    let width = style(
        "<div></div>",
        "width: 200px; width: -50px; line-height: 40px; line-height: -2",
    );
    assert_eq!(
        width.width,
        crate::values::Size::Length(LengthPercentage::Px(200.0))
    );
}

#[test]
fn clamps_computed_negative_paddings_at_zero() {
    let clamped = style("<div></div>", "padding: 10px; padding: calc(-8px) 8px");
    assert_eq!(clamped.padding.top, LengthPercentage::Px(0.0));
    assert_eq!(clamped.padding.left, LengthPercentage::Px(8.0));
    let mixed = style("<div></div>", "padding-top: calc(10% - 50px)");
    assert_eq!(mixed.padding.top.resolve(200.0), 0.0);
    assert_eq!(mixed.padding.top.resolve(1000.0), 50.0);
}

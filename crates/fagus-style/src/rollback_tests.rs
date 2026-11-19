//! Cascade details as Chrome 157 resolves them: `revert-layer` in important declarations,
//! reverted declarations that leave a form control its native look and logical properties their
//! physical ones, nested `@scope` rules, and the values of `content` and `shape-outside` that
//! lightningcss keeps as tokens (WPT `css-cascade`, `css-ui`, `css-content`, `css-shapes`).

use std::sync::Arc;

use fagus_css::{MediaEnvironment, Origin, RuleSet, Stylesheet};
use fagus_dom::{Document, NodeId};

use crate::values::{ClipShape, Content, ContentItem, LengthPercentage, Rgba, ShapeBox, ShapeOutside, Size};
use crate::{ComputedStyle, StyledDocument, USER_AGENT_CSS, compute_styles};

fn styles(html: &str, css: &str) -> (Document, StyledDocument) {
    let document = Document::parse_html(html);
    let ua = Stylesheet::parse(USER_AGENT_CSS, Origin::UserAgent, "ua.css");
    let author = Stylesheet::parse(css, Origin::Author, "test.css");
    let environment = MediaEnvironment::default();
    let rules = RuleSet::new(&[&ua, &author], &environment);
    let styled = compute_styles(&document, &rules, (environment.width, environment.height));
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

#[test]
fn rolls_important_declarations_back_to_the_layers_below() {
    // The important `revert-layer` of the second layer wins over the third layer's important
    // declaration and takes the first layer's normal one (WPT `revert-layer-005`).
    let (document, styled) = styles(
        r#"<div id="target"></div>"#,
        "@layer { #target { background-color: green } }
        @layer { #target { background-color: red; background-color: red !important; background-color: revert-layer !important } }
        @layer { #target { background-color: red; background-color: red !important } }",
    );
    assert_eq!(
        by_id(&document, &styled, "target").background_color,
        Rgba::from_u8(0, 128, 0, 1.0)
    );
}

#[test]
fn keeps_the_native_look_of_controls_whose_frame_is_reverted() {
    let (document, styled) = styles(
        r#"<input id="all"><input id="each"><input id="styled">"#,
        "#all { background-color: red; border-color: red; all: revert }
        #each { border-block-start-color: revert; border-top-width: revert; border-inline-start-width: revert; background-color: revert }
        #styled { background-color: red }",
    );
    let style = |id| by_id(&document, &styled, id);
    assert!(!style("all").author_frame);
    assert!(!style("each").author_frame);
    assert!(style("styled").author_frame);
    // A reverted logical property leaves the physical one the user agent's `border` sets.
    assert_eq!(style("each").border_width.top, 2.0);
    assert_eq!(style("each").border_width.left, 2.0);
}

#[test]
fn matches_nested_scopes_from_the_roots_around_them() {
    // `:scope` in the prelude of a nested `@scope` is the outer root (WPT
    // `scope-inner-pseudo-scope-crash`).
    let (document, styled) = styles(
        r#"<div id="root" class="a"><p id="inside" class="b"></p></div><p id="outside" class="b"></p>"#,
        "@scope (.a) { @scope (:scope) { :scope { color: green } } }
        @scope (.a) { @scope (.b) { :scope { color: blue } } }",
    );
    let style = |id| by_id(&document, &styled, id);
    assert_eq!(style("root").color, Rgba::from_u8(0, 128, 0, 1.0));
    assert_eq!(style("inside").color, Rgba::from_u8(0, 0, 255, 1.0));
    assert_eq!(style("outside").color, Rgba::BLACK);
}

#[test]
fn reads_content_and_shape_outside_that_lightningcss_keeps_as_tokens() {
    let (document, styled) = styles(
        r#"<div id="alt"></div><div id="path"></div><div id="even"></div>"#,
        r#"#alt { content: url(a.png) / "Alternative" }
        #path { float: left; shape-outside: path('M 0 0 L 10 0 L 0 10 Z') content-box }
        #even { float: left; shape-outside: path(evenodd, "M 0 0 h 10 v 10 z") }"#,
    );
    let style = |id| by_id(&document, &styled, id);
    assert_eq!(
        style("alt").content,
        Content::Items(vec![ContentItem::Image("a.png".to_owned())])
    );
    assert_eq!(
        style("path").shape_outside,
        Some(Arc::new(ShapeOutside::Shape {
            shape: ClipShape::Path {
                even_odd: false,
                data: Arc::from("M 0 0 L 10 0 L 0 10 Z"),
            },
            reference: ShapeBox::ContentBox,
        }))
    );
    assert!(matches!(
        style("even").shape_outside.as_deref(),
        Some(ShapeOutside::Shape {
            shape: ClipShape::Path { even_odd: true, .. },
            reference: ShapeBox::MarginBox,
        })
    ));
}

#[test]
fn interpolates_registered_custom_properties_between_keyframes() {
    // Halfway from 50px to `revert-layer`, which reverts to the element's own 150px (WPT
    // `revert-layer-011`); a number interpolates too, a property of any value flips at the
    // middle and one registered as a color keeps its own interpolation out of it.
    let (document, styled) = styles(
        r#"<div id="length"></div><div id="number"></div><div id="any"></div><div id="layered"></div>"#,
        "@property --x { syntax: '<length>'; initial-value: 0px; inherits: false }
        @property --n { syntax: '<number>'; initial-value: 0; inherits: false }
        @keyframes length { from { --x: 50px } to { --x: revert-layer } }
        @keyframes number { from { --n: 1 } to { --n: 3 } }
        @keyframes any { from { --a: 10px } to { --a: 30px } }
        div { animation: 2s linear -1s paused; height: 10px }
        #length { width: var(--x); --x: 150px; animation-name: length }
        #number { width: calc(var(--n) * 10px); animation-name: number }
        #any { width: var(--a); animation-name: any }
        @layer one { #layered { --w: 40px } }
        @layer two { #layered { --w: revert-layer; width: var(--w, 5px) } }",
    );
    let width = |id| by_id(&document, &styled, id).width.clone();
    assert_eq!(width("length"), Size::Length(LengthPercentage::Px(100.0)));
    assert_eq!(width("number"), Size::Length(LengthPercentage::Px(20.0)));
    assert_eq!(width("any"), Size::Length(LengthPercentage::Px(30.0)));
    // `revert-layer` on a custom property outside animations.
    assert_eq!(width("layered"), Size::Length(LengthPercentage::Px(40.0)));
}

#[test]
fn all_leaves_direction_and_unicode_bidi_alone() {
    // WPT `all-prop-001`: `all: initial` after `direction: rtl; unicode-bidi: bidi-override`
    // resets everything else.
    let (document, styled) = styles(
        r#"<div id="test"></div>"#,
        "#test { direction: rtl; unicode-bidi: bidi-override; color: red; --x: 1 }
        #test { all: initial }
",
    );
    let style = by_id(&document, &styled, "test");
    assert_eq!(style.direction, crate::values::Direction::Rtl);
    assert_eq!(style.unicode_bidi, crate::values::UnicodeBidi::BidiOverride);
    assert_eq!(style.color, Rgba::from_u8(0, 0, 0, 1.0));
    assert!(style.custom_properties.contains_key("--x"));
}

//! Cascade details as Chrome 157 resolves them: `revert-layer` in important declarations,
//! reverted declarations that leave a form control its native look and logical properties their
//! physical ones, nested `@scope` rules, and the values of `content` and `shape-outside` that
//! lightningcss keeps as tokens (WPT `css-cascade`, `css-ui`, `css-content`, `css-shapes`).

use std::sync::Arc;

use fagus_css::{MediaEnvironment, Origin, RuleSet, Stylesheet};
use fagus_dom::{Document, NodeId};

use crate::values::{ClipShape, Content, ContentItem, Rgba, ShapeBox, ShapeOutside};
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

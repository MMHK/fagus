//! Transforms in 3D and animated transforms and filters, as Chrome 157 prints them:
//! `transform-style`, the perspective of the parent element only, `scale3d()`, and transforms
//! and filters between two keyframes.

use fagus_css::{MediaEnvironment, Origin, RuleSet, Stylesheet};
use fagus_dom::{Document, NodeId};

use crate::values::{Filter, TransformFunction};
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
fn keeps_3d_space_and_perspective_of_the_parent_element() {
    let (document, styled) = styles(
        r#"<div id="space"><div id="child"><div id="grandchild"></div></div></div><span id="inline"></span>"#,
        "#space, #inline { transform-style: preserve-3d; perspective: 100px }
         #child { transform: scale3d(1, 1, 2) }
         #grandchild { transform: scaleZ(3) scale3d(2, 2, 1) }",
    );
    let style = |id| by_id(&document, &styled, id);
    assert!(style("space").preserve_3d && !style("child").preserve_3d);
    // `transform-style: preserve-3d` makes a stacking context and the containing block of
    // positioned descendants.
    assert!(style("space").is_stacking_context() && style("space").contains_absolute());
    // Not an inline box, which transforms do not apply to.
    assert!(!style("inline").contains_absolute());
    // Only the children of an element are seen in its perspective and take part in its space.
    assert!(style("child").parent_perspective && !style("grandchild").parent_perspective);
    assert!(style("child").parent_preserve_3d && !style("grandchild").parent_preserve_3d);
    // A scale in depth is kept.
    assert_eq!(
        &*style("child").transform,
        [TransformFunction::Matrix3d([
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 1.0
        ])]
    );
    assert!(matches!(
        style("grandchild").transform[1],
        TransformFunction::Scale(2.0, 2.0)
    ));
}

#[test]
fn prints_filters_and_transforms_between_keyframes() {
    let (document, styled) = styles(
        r#"<div id="filtered"></div><div id="turned"></div>"#,
        "@keyframes fade { from { filter: contrast(0%) blur(20px) } to { filter: none } }
         @keyframes turn { to { rotate: 180deg; translate: 100px; transform: scale(3) } }
         div { animation: 4s linear -1s paused }
         #filtered { animation-name: fade }
         #turned { animation-name: turn }",
    );
    let style = |id| by_id(&document, &styled, id);
    // A quarter of the way from the first keyframe to `none`.
    assert_eq!(
        &*style("filtered").filter,
        [Filter::Contrast(0.25), Filter::Blur(15.0)]
    );
    let turned = style("turned");
    assert_eq!(turned.rotate, Some((0.0, 0.0, 1.0, 45.0)));
    assert!(turned.translate.is_some());
    assert_eq!(&*turned.transform, [TransformFunction::Scale(1.5, 1.5)]);
}

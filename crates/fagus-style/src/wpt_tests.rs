//! Values that the Web Platform Tests of colors, masking, sizing and display measure, as Chrome
//! 157 computes them.

use fagus_css::{MediaEnvironment, Origin, RuleSet, Stylesheet};
use fagus_dom::{Document, NodeId};

use crate::values::{BoxArea, ClipShape, Display, LengthPercentage, ObjectFit};
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
fn clamps_the_lightness_of_lab_colors() {
    // Lightness past its range is clamped (WPT `css-color/lab-l-over-100-1` and the like), and
    // so is a negative chroma.
    let (document, styled) = styles(
        r#"<div id="a"></div><div id="b"></div><div id="c"></div><div id="d"></div>
        <div id="e"></div><div id="f"></div><div id="g"></div><div id="h"></div>"#,
        "#a { color: lab(150 150 20) } #b { color: lab(100 150 20) }
         #c { color: oklch(1.5 0.2 30) } #d { color: oklch(1 0.2 30) }
         #e { color: lch(50 -30 20) } #f { color: lch(50 0 20) }
         #g { color: oklab(-0.5 0.1 0.1) } #h { color: oklab(0 0.1 0.1) }",
    );
    for (clamped, limit) in [("a", "b"), ("c", "d"), ("e", "f"), ("g", "h")] {
        assert_eq!(
            by_id(&document, &styled, clamped).color,
            by_id(&document, &styled, limit).color,
            "{clamped}"
        );
    }
}

#[test]
fn computes_newer_shapes_of_clip_path() {
    let (document, styled) = styles(
        r#"<div id="xywh"></div><div id="rect"></div><div id="shape"></div><div id="bad"></div>"#,
        "#xywh { clip-path: xywh(10px 20px 50% 60px round 5px) content-box }
         #rect { clip-path: rect(10px auto 70px 20%) }
         #shape { clip-path: shape(from 0 0, line to 100% 0, line to 50% 100%, close) }
         #bad { clip-path: circle(10px); clip-path: xywh(10px oops) }",
    );
    let clip = |id| {
        by_id(&document, &styled, id)
            .clip_path
            .clone()
            .unwrap_or_else(|| panic!("no clip path on {id}"))
    };
    let xywh = clip("xywh");
    assert_eq!(xywh.reference, BoxArea::ContentBox);
    let ClipShape::Inset { insets, .. } = &xywh.shape else {
        panic!("xywh() is an inset: {xywh:?}");
    };
    assert_eq!(insets[0], LengthPercentage::Px(20.0));
    assert_eq!(insets[1].resolve(200.0), 90.0);
    assert_eq!(insets[3], LengthPercentage::Px(10.0));
    let rect = clip("rect");
    let ClipShape::Inset { insets, .. } = &rect.shape else {
        panic!("rect() is an inset: {rect:?}");
    };
    assert_eq!(insets[1].resolve(200.0), 0.0);
    assert_eq!(insets[2].resolve(100.0), 30.0);
    assert_eq!(insets[3].resolve(200.0), 40.0);
    assert!(matches!(clip("shape").shape, ClipShape::Polygon { .. }));
    // A shape that does not parse leaves the declaration before it in force.
    assert!(matches!(
        clip("bad").shape,
        ClipShape::Ellipse { circle: true, .. }
    ));
}

#[test]
fn treats_degenerate_aspect_ratios_as_auto() {
    // A term that rounds to nothing in Blink's layout units makes the ratio degenerate, which
    // `0/1` would otherwise make infinitely tall (WPT `css-sizing/aspect-ratio/zero-or-infinity-006`).
    let (document, styled) = styles(
        r#"<div id="zero"></div><div id="tiny"></div><div id="small"></div><div id="wide"></div>"#,
        "#zero { aspect-ratio: 0/1 } #tiny { aspect-ratio: 1/0.00000000000001 }
         #small { aspect-ratio: 0.01/1 } #wide { aspect-ratio: 16/9 }",
    );
    assert_eq!(by_id(&document, &styled, "zero").aspect_ratio, None);
    assert_eq!(by_id(&document, &styled, "tiny").aspect_ratio, None);
    assert!(by_id(&document, &styled, "small").aspect_ratio.is_some());
    assert_eq!(by_id(&document, &styled, "wide").aspect_ratio, Some(16.0 / 9.0));
}

#[test]
fn drops_run_in_and_styles_videos_as_chrome() {
    // Chrome 157 does not support `display: run-in`: the declaration is dropped. A video's
    // poster is contained in it, and its `width` and `height` attributes size it.
    let (document, styled) = styles(
        r#"<div id="run" style="display: run-in"></div><span id="inline" style="display: block; display: run-in"></span>
        <video id="video" width="100" height="50" align="left"></video>"#,
        "",
    );
    assert_eq!(by_id(&document, &styled, "run").display, Display::BLOCK);
    assert_eq!(by_id(&document, &styled, "inline").display, Display::BLOCK);
    let video = by_id(&document, &styled, "video");
    assert_eq!(video.object_fit, ObjectFit::Contain);
    assert_eq!(
        video.width,
        crate::values::Size::Length(LengthPercentage::Px(100.0))
    );
    assert_eq!(
        video.height,
        crate::values::Size::Length(LengthPercentage::Px(50.0))
    );
    assert!(!video.is_floating());
}

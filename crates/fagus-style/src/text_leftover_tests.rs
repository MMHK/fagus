//! Markers, text transforms and font properties as Chrome computes them: the white space of
//! markers inside and outside the item, the markers of `::before` and `::after` list items,
//! `text-transform: math-auto`, math functions in `font-size-adjust` and the ligatures that
//! `font-variant-ligatures` turns on.

use fagus_css::{MediaEnvironment, Origin, RuleSet, Stylesheet};
use fagus_dom::{Document, NodeId};

use crate::values::{FontMetric, FontSizeAdjust, TextTransform, WhiteSpace};
use crate::{ComputedStyle, PseudoKey, StyledDocument, USER_AGENT_CSS, compute_styles};

fn styles(html: &str, css: &str) -> (Document, StyledDocument) {
    let document = Document::parse_html(html);
    let ua = Stylesheet::parse(USER_AGENT_CSS, Origin::UserAgent, "ua.css");
    assert!(ua.diagnostics().is_empty(), "{:#?}", ua.diagnostics());
    let author = Stylesheet::parse(css, Origin::Author, "test.css");
    let environment = MediaEnvironment::default();
    let rules = RuleSet::new(&[&ua, &author], &environment);
    let styled = compute_styles(&document, &rules, (environment.width, environment.height));
    (document, styled)
}

fn element(document: &Document, id: &str) -> NodeId {
    document
        .descendants(NodeId::DOCUMENT)
        .find(|&node| {
            document
                .element(node)
                .is_some_and(|data| data.id.as_deref() == Some(id))
        })
        .expect("element with id")
}

fn by_id<'a>(document: &Document, styled: &'a StyledDocument, id: &str) -> &'a ComputedStyle {
    styled.style(element(document, id)).expect("styled element")
}

#[test]
fn keeps_the_spaces_of_outside_markers_and_takes_the_items_white_space_inside() {
    let (document, styled) = styles(
        "<ol><li id=outside>a</li><li id=inside>b</li><li id=pre>c</li></ol>",
        "#inside, #pre { list-style-position: inside } #pre { white-space: pre-wrap }",
    );
    let marker = |id: &str| {
        styled
            .pseudo(element(&document, id), PseudoKey::Marker)
            .expect("marker")
            .white_space
    };
    // Blink's `AdjustStyleForMarker`: `pre` outside, the item's own inside.
    assert_eq!(marker("outside"), WhiteSpace::PRE);
    assert_eq!(marker("inside"), WhiteSpace::NORMAL);
    assert_eq!(marker("pre"), WhiteSpace::PRE_WRAP);
    // Blink's `marker.css`: markers isolate their text, and text alignment and indents of the
    // item do not reach them.
    let style = styled
        .pseudo(element(&document, "inside"), PseudoKey::Marker)
        .expect("marker");
    assert_eq!(style.unicode_bidi, crate::values::UnicodeBidi::Isolate);
}

#[test]
fn gives_before_and_after_list_items_markers_of_the_user_agent() {
    let (document, styled) = styles(
        "<div id=item></div><div id=plain></div>",
        "#item::before { content: 'x'; display: list-item; font-size: 20px }
         #item::after { content: 'y'; display: list-item; list-style-position: inside }
         #item::marker { color: red }
         #plain::before { content: 'z' }",
    );
    let item = element(&document, "item");
    let before = styled
        .pseudo(item, PseudoKey::BeforeMarker)
        .expect("::before::marker");
    // It inherits from the `::before`, with the user agent's `::marker` declarations, and not
    // those for the element's own marker.
    assert!((before.font_size - 20.0).abs() < 1e-6);
    assert_eq!(before.white_space, WhiteSpace::PRE);
    assert_eq!(before.color, by_id(&document, &styled, "item").color);
    assert!(before.font_features().contains(&(*b"tnum", 1)));
    let after = styled
        .pseudo(item, PseudoKey::AfterMarker)
        .expect("::after::marker");
    assert_eq!(after.white_space, WhiteSpace::NORMAL);
    let plain = element(&document, "plain");
    assert!(styled.pseudo(plain, PseudoKey::BeforeMarker).is_none());
}

#[test]
fn computes_text_transform_math_auto() {
    let (document, styled) = styles(
        "<p id=a><span id=b></span></p><p id=c></p>",
        "#a { text-transform: math-auto } #c { text-transform: uppercase; text-transform: math-auto x }",
    );
    let transform = |id: &str| by_id(&document, &styled, id).text_transform;
    // Inherited; a value that is not one is dropped.
    assert_eq!(
        (transform("a"), transform("b")),
        (TextTransform::MathAuto, TextTransform::MathAuto)
    );
    assert_eq!(transform("c"), TextTransform::Uppercase);
}

#[test]
fn computes_math_functions_in_font_size_adjust() {
    let (document, styled) = styles(
        "<p id=a></p><p id=b></p>",
        "#a { font-size-adjust: cap-height calc(1462 / 2048) }
         #b { font-size-adjust: calc(0.25 * 2) }",
    );
    let adjust = |id: &str| by_id(&document, &styled, id).font_size_adjust.clone();
    let FontSizeAdjust::Aspect(FontMetric::CapHeight, aspect) = adjust("a") else {
        panic!("{:?}", adjust("a"));
    };
    assert!((aspect - 1462.0 / 2048.0).abs() < 1e-6);
    assert_eq!(adjust("b"), FontSizeAdjust::Aspect(FontMetric::ExHeight, 0.5));
}

#[test]
fn turns_ligatures_on_and_off_as_font_variant_ligatures_asks() {
    let (document, styled) = styles(
        "<p id=on></p><p id=off></p><p id=normal></p><p id=spaced></p>",
        "#on { font-variant-ligatures: common-ligatures contextual no-discretionary-ligatures }
         #off { font-variant-ligatures: no-common-ligatures historical-ligatures }
         #spaced { font-variant-ligatures: common-ligatures; letter-spacing: 1px }",
    );
    let features = |id: &str| by_id(&document, &styled, id).font_features();
    // On and off, so that they win over the features of an `@font-face` rule.
    let on = features("on");
    for feature in [(*b"liga", 1), (*b"clig", 1), (*b"calt", 1), (*b"dlig", 0)] {
        assert!(on.contains(&feature), "{feature:?} in {on:?}");
    }
    let off = features("off");
    for feature in [(*b"liga", 0), (*b"clig", 0), (*b"hlig", 1)] {
        assert!(off.contains(&feature), "{feature:?} in {off:?}");
    }
    assert_eq!(features("normal"), Vec::<([u8; 4], u32)>::new());
    // Letter spacing turns them off all the same.
    let spaced = features("spaced");
    assert!(spaced.contains(&(*b"liga", 0)) && !spaced.contains(&(*b"liga", 1)));
}
